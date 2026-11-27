use notify::EventKind;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::scanner::should_skip_event_path;
use crate::watcher::backend::WatchEvent;

/// Quiet period after the last event before marking a root as having detected changes (TASK-1G decision n).
pub const DEBOUNCE_QUIET_PERIOD: Duration = Duration::from_secs(2);

/// Maximum wait from the first event of a continuous stream before forcing a mark (TASK-1G decision n).
pub const DEBOUNCE_MAX_WAIT: Duration = Duration::from_secs(30);

/// Fixed-size per-root inbox folded directly inside the `notify` callback (O(1) memory).
#[derive(Debug, Default)]
pub struct RootInbox {
    pub debounce: RootDebounceState,
    pub error_seen: bool,
}

/// Pure `Instant`-driven debouncer state machine (no internal clocks or timers).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RootDebounceState {
    first_event_at: Option<Instant>,
    last_event_at: Option<Instant>,
}

impl RootDebounceState {
    /// Records one relevant filesystem event at `now`.
    pub fn record_event(&mut self, now: Instant) {
        if self.first_event_at.is_none() {
            self.first_event_at = Some(now);
        }
        self.last_event_at = Some(now);
    }

    /// Returns `true` and clears the pending burst if either:
    /// - `DEBOUNCE_QUIET_PERIOD` (2 s) has elapsed since the last event, or
    /// - `DEBOUNCE_MAX_WAIT` (30 s) has elapsed since the first event of a continuous stream.
    pub fn poll_ready(&mut self, now: Instant) -> bool {
        let (Some(first), Some(last)) = (self.first_event_at, self.last_event_at) else {
            return false;
        };

        let quiet_elapsed = now.saturating_duration_since(last) >= DEBOUNCE_QUIET_PERIOD;
        let max_wait_elapsed = now.saturating_duration_since(first) >= DEBOUNCE_MAX_WAIT;

        if quiet_elapsed || max_wait_elapsed {
            self.first_event_at = None;
            self.last_event_at = None;
            true
        } else {
            false
        }
    }

    /// Clears any pending un-emitted burst.
    pub fn clear(&mut self) {
        self.first_event_at = None;
        self.last_event_at = None;
    }

    /// Returns whether a burst is currently pending.
    pub fn is_pending(&self) -> bool {
        self.first_event_at.is_some()
    }
}

/// Folds a single `WatchEvent` into a fixed-size `RootInbox` in O(1) memory.
pub fn fold_event_into_inbox(inbox: &mut RootInbox, event: WatchEvent, now: Instant) {
    match event {
        WatchEvent::Changed => {
            inbox.debounce.record_event(now);
        }
        WatchEvent::Error => {
            inbox.error_seen = true;
            inbox.debounce.record_event(now);
        }
    }
}

/// Decides whether a raw `notify::Event` should mark changes for `root_path`.
///
/// - Any `Create`, `Modify` (including `Modify(Name(_))` renames), `Remove`, `Other`, `Any`,
///   or overflow (`need_rescan()`) event counts as a change.
/// - Read-only `Access` events without rescan flag are ignored.
/// - Events whose paths are all skipped by the scanner's always-skipped or `.photoslibrary` rules
///   (reused from `scanner::should_skip_event_path`) are dropped.
pub fn classify_notify_event(root_path: Option<&Path>, event: &notify::Event) -> bool {
    if event.need_rescan() {
        return true;
    }

    match event.kind {
        EventKind::Access(_) => false,
        EventKind::Create(_)
        | EventKind::Modify(_)
        | EventKind::Remove(_)
        | EventKind::Other
        | EventKind::Any => {
            if event.paths.is_empty() {
                return true;
            }
            event
                .paths
                .iter()
                .any(|p| !should_skip_event_path(root_path, p))
        }
    }
}

use std::time::Instant;

use crate::domain::{RootId, RootStatus};

/// Minimum cooldown between the end of a root's last scan and an automatic scan (60 seconds).
pub const AUTO_SCAN_MIN_GAP_MS: i64 = 60_000;

/// Maximum age since a root's last finished scan before 24-hour reconciliation triggers.
pub const RECONCILE_MAX_AGE_MS: i64 = 24 * 60 * 60 * 1000;

/// Input snapshot for one root passed into the pure automatic scan scheduler.
#[derive(Debug, Clone)]
pub struct SchedulerRootInput {
    pub id: RootId,
    pub status: RootStatus,
    pub changes_detected: bool,
    pub changes_detected_at: Option<Instant>,
    pub has_active_job: bool,
    /// Most recent finished/cancelled/failed scan end time (used for the 60 s cooldown).
    pub last_scan_ended_at_ms: Option<i64>,
    /// Last completed scan timestamp (`library_roots.last_scan_at_ms`, used for 24 h reconciliation).
    pub last_completed_scan_at_ms: Option<i64>,
}

/// Reason why the scheduler selected a root for an automatic scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoScanReason {
    ChangesDetected,
    Reconciliation24h,
}

/// Decision returned by `decide_next_auto_scan`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoScanCandidate {
    pub root_id: RootId,
    pub reason: AutoScanReason,
}

/// Pure automatic scan decision function (TASK-1G decision p).
///
/// Rules:
/// 1. Only when `auto_scan_enabled` is `true`.
/// 2. At most one automatic scan running at a time across the whole app (`!any_auto_scan_running`).
/// 3. Only `Active` roots without a running job on that root.
/// 4. At least 60 s (`AUTO_SCAN_MIN_GAP_MS`) after that root's last scan ended.
/// 5. Prefer roots with `changes_detected`, oldest change first (`changes_detected_at` ascending).
/// 6. Otherwise, pick a root whose last finished scan ended more than 24 h ago (`RECONCILE_MAX_AGE_MS`),
///    oldest scan first.
pub fn decide_next_auto_scan(
    auto_scan_enabled: bool,
    any_auto_scan_running: bool,
    roots: &[SchedulerRootInput],
    now_instant: Instant,
    now_ms: i64,
) -> Option<AutoScanCandidate> {
    if !auto_scan_enabled || any_auto_scan_running {
        return None;
    }

    let eligible: Vec<&SchedulerRootInput> = roots
        .iter()
        .filter(|r| r.status == RootStatus::Active && !r.has_active_job)
        .filter(|r| r.last_completed_scan_at_ms.is_some())
        .filter(|r| match r.last_scan_ended_at_ms {
            Some(ended_ms) => now_ms.saturating_sub(ended_ms) >= AUTO_SCAN_MIN_GAP_MS,
            None => true,
        })
        .collect();

    // 1. Watcher-detected changes take priority, oldest change first.
    let changed_candidate = eligible
        .iter()
        .filter(|r| r.changes_detected)
        .min_by(|a, b| {
            let ta = a.changes_detected_at.unwrap_or(now_instant);
            let tb = b.changes_detected_at.unwrap_or(now_instant);
            ta.cmp(&tb).then_with(|| a.id.as_str().cmp(b.id.as_str()))
        });

    if let Some(c) = changed_candidate {
        return Some(AutoScanCandidate {
            root_id: c.id.clone(),
            reason: AutoScanReason::ChangesDetected,
        });
    }

    // 2. Reconciliation scan when a root's last finished scan ended more than 24 h ago.
    let stale_candidate = eligible
        .iter()
        .filter_map(|r| {
            let ended_ms = r.last_completed_scan_at_ms?;
            if now_ms.saturating_sub(ended_ms) > RECONCILE_MAX_AGE_MS {
                Some((ended_ms, *r))
            } else {
                None
            }
        })
        .min_by(|(ta, a), (tb, b)| ta.cmp(tb).then_with(|| a.id.as_str().cmp(b.id.as_str())));

    stale_candidate.map(|(_, r)| AutoScanCandidate {
        root_id: r.id.clone(),
        reason: AutoScanReason::Reconciliation24h,
    })
}

pub mod backend;
pub mod debounce;
pub mod manager;
pub mod scheduler;

#[cfg(test)]
mod tests;

pub use backend::{NotifyWatchBackend, WatchBackend, WatchCallback, WatchEvent};
pub use debounce::{
    classify_notify_event, fold_event_into_inbox, RootDebounceState, RootInbox, DEBOUNCE_MAX_WAIT,
    DEBOUNCE_QUIET_PERIOD,
};
pub use manager::{run_auto_scan_tick, spawn_watcher_and_scheduler, WatcherManager};
pub use scheduler::{
    decide_next_auto_scan, AutoScanCandidate, AutoScanReason, SchedulerRootInput,
    AUTO_SCAN_MIN_GAP_MS, RECONCILE_MAX_AGE_MS,
};

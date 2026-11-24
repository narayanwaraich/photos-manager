use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Cancellation token for a scan operation (Decision j).
#[derive(Clone, Debug, Default)]
pub struct ScanCancel(Arc<AtomicBool>);

impl ScanCancel {
    pub fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// All stable warning codes emitted by the scanner (Decision k).
pub const ALL_WARNING_CODES: &[&str] = &[
    "path_key_collision",
    "path_not_unicode",
    "entry_unreadable",
    "file_time_invalid",
    "size_out_of_range",
    "path_not_inside_root",
    "path_parent_component",
    "path_current_dir_component",
    "path_absolute_component",
    "path_empty_component",
    "path_nul_byte",
    "path_invalid_separator",
    "path_verbatim_mismatch",
    "path_invalid_native_length",
];

/// A bounded scan warning (Decision a, l).
///
/// Holds a stable code and a root-relative display path.
/// Never contains an absolute path or raw OS error string in the code field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanWarning {
    pub code: String,
    pub path_display: String,
}

/// Cumulative statistics of a scan run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanCounts {
    pub folders_seen: u64,
    pub files_seen: u64,
    pub new: u64,
    pub changed: u64,
    pub unchanged: u64,
    pub moved: u64,
    pub marked_missing: u64,
    pub marked_inaccessible: u64,
    pub unsupported: u64,
    pub skipped_by_rule: u64,
}

/// Detailed report generated at the end of a scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanReport {
    pub generation: String,
    pub counts: ScanCounts,
    pub warning_count: u64,
    pub warnings: Vec<ScanWarning>,
    pub elapsed: Duration,
}

/// Periodic progress notification delivered after each committed batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanProgress {
    pub folders_seen: u64,
    pub files_seen: u64,
    pub batches_committed: u64,
}

/// The outcome of a scan execution (Decision a).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanOutcome {
    Completed(ScanReport),
    Cancelled(ScanReport),
}

impl ScanOutcome {
    pub fn report(&self) -> &ScanReport {
        match self {
            ScanOutcome::Completed(r) | ScanOutcome::Cancelled(r) => r,
        }
    }
}

/// Configuration options and failure injection hooks for `scan_root`.
#[derive(Debug, Clone)]
pub struct ScanOptions {
    pub batch_size: usize,
    pub generation: Option<String>,
    #[cfg(test)]
    pub failure_hook_after_batch: Option<usize>,
    #[cfg(test)]
    pub failure_in_move_step: bool,
    #[cfg(test)]
    pub failure_in_final_marking: bool,
    #[cfg(test)]
    pub panic_in_walker_after_batch: Option<usize>,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            batch_size: 500,
            generation: None,
            #[cfg(test)]
            failure_hook_after_batch: None,
            #[cfg(test)]
            failure_in_move_step: false,
            #[cfg(test)]
            failure_in_final_marking: false,
            #[cfg(test)]
            panic_in_walker_after_batch: None,
        }
    }
}

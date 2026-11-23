use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use crate::domain::RootId;
use crate::scanner::error::ScanError;

static ACTIVE_SCANS: OnceLock<Mutex<HashSet<RootId>>> = OnceLock::new();

fn active_scans_set() -> &'static Mutex<HashSet<RootId>> {
    ACTIVE_SCANS.get_or_init(|| Mutex::new(HashSet::new()))
}

/// An RAII guard ensuring only one scan per root runs concurrently (Decision j).
pub struct ScanGuard {
    root_id: RootId,
}

impl ScanGuard {
    /// Attempts to acquire the scan guard for the given root.
    /// Returns `ScanError::AlreadyRunning` if a scan for this root is currently active.
    pub fn acquire(root_id: &RootId) -> Result<Self, ScanError> {
        let mut set = match active_scans_set().lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };

        if set.contains(root_id) {
            return Err(ScanError::AlreadyRunning(root_id.to_string()));
        }

        set.insert(root_id.clone());
        Ok(Self {
            root_id: root_id.clone(),
        })
    }
}

impl Drop for ScanGuard {
    fn drop(&mut self) {
        let mut set = match active_scans_set().lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        set.remove(&self.root_id);
    }
}

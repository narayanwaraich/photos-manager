use sqlx::SqlitePool;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::database;
use crate::domain::{CatalogueError, RootError, RootId, RootStatus, VolumeInfo};

/// Default timeout for live root availability checks in `list_roots` (2 seconds).
pub const DEFAULT_REACHABLE_TIMEOUT: Duration = Duration::from_secs(2);

/// Timeout for pre-scan availability checks (5 seconds for sleeping external disks, TASK-1F decision d).
pub const SCAN_AVAILABILITY_TIMEOUT: Duration = Duration::from_secs(5);

/// Reason a root is definitely not available (TASK-1F decision c).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnavailableReason {
    Unreachable,
    VolumeChanged,
}

/// Three-valued outcome of a live root availability check (TASK-1F decision a, c).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AvailabilityOutcome {
    Available,
    NotAvailable(UnavailableReason),
    Unknown,
}

impl AvailabilityOutcome {
    pub fn is_available(self) -> bool {
        matches!(self, Self::Available)
    }
}

/// RAII guard that removes a `RootId` from the active in-flight set when the blocking
/// inspection thread exits or panics.
struct InFlightCheckGuard {
    set: Arc<Mutex<HashSet<RootId>>>,
    id: RootId,
}

impl Drop for InFlightCheckGuard {
    fn drop(&mut self) {
        let mut set = match self.set.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        set.remove(&self.id);
    }
}

/// Checks live availability of a root path off the async executor with a timeout and
/// per-root in-flight deduplication (TASK-1F decisions c, d, e).
///
/// Runs `is_dir_fn` and `describer` inside `spawn_blocking`. Returns `Unknown` on timeout,
/// join error/panic, an already in-flight check for this root, or a `describer` error.
pub async fn check_root_availability<F, D>(
    root_id: &RootId,
    root_path: PathBuf,
    stored_fs_uuid: Option<String>,
    in_flight: &Arc<Mutex<HashSet<RootId>>>,
    timeout: Duration,
    is_dir_fn: F,
    describer: D,
) -> AvailabilityOutcome
where
    F: Fn(&Path) -> bool + Send + Sync + 'static,
    D: Fn(&Path) -> Result<VolumeInfo, RootError> + Send + Sync + 'static,
{
    let already_in_flight = {
        let mut set = match in_flight.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if set.contains(root_id) {
            true
        } else {
            set.insert(root_id.clone());
            false
        }
    };

    if already_in_flight {
        return AvailabilityOutcome::Unknown;
    }

    let set_clone = Arc::clone(in_flight);
    let id_clone = root_id.clone();

    let blocking_handle = tokio::task::spawn_blocking(move || {
        let _guard = InFlightCheckGuard {
            set: set_clone,
            id: id_clone,
        };

        if !is_dir_fn(&root_path) {
            return AvailabilityOutcome::NotAvailable(UnavailableReason::Unreachable);
        }

        let vol_info = match describer(&root_path) {
            Ok(info) => info,
            Err(_) => return AvailabilityOutcome::Unknown,
        };

        match (stored_fs_uuid.as_deref(), vol_info.fs_uuid.as_deref()) {
            (Some(stored), Some(live)) => {
                if crate::platform::same_volume_identity(stored, live) {
                    AvailabilityOutcome::Available
                } else {
                    AvailabilityOutcome::NotAvailable(UnavailableReason::VolumeChanged)
                }
            }
            // Limit (decision c): if either fs_uuid is NULL (e.g. Linux stub or share without UUID),
            // volume comparison is skipped and directory existence decides availability.
            _ => AvailabilityOutcome::Available,
        }
    });

    match tokio::time::timeout(timeout, blocking_handle).await {
        Ok(Ok(outcome)) => outcome,
        _ => AvailabilityOutcome::Unknown,
    }
}

/// Applies a live availability check outcome to a root's stored `status` (TASK-1F decisions a, b).
///
/// This is the single function that transitions stored `status` from live availability checks:
/// - `Active` + `NotAvailable(_)` -> `Offline`
/// - `Offline` + `Available` -> `Active`
/// - `Paused` and `Removed` are never changed.
/// - `Unknown` changes nothing.
/// - Skips the database write when status would not change.
/// - Never writes `files` or `folders` rows.
pub async fn apply_availability_result(
    pool: &SqlitePool,
    root_id: &RootId,
    known_status: Option<RootStatus>,
    outcome: AvailabilityOutcome,
) -> Result<RootStatus, RootError> {
    if let Some(status) = known_status {
        let needs_transition = matches!(
            (status, outcome),
            (RootStatus::Active, AvailabilityOutcome::NotAvailable(_))
                | (RootStatus::Offline, AvailabilityOutcome::Available)
        );
        if !needs_transition {
            return Ok(status);
        }
    }

    if matches!(outcome, AvailabilityOutcome::Unknown) {
        let mut conn = pool.acquire().await.map_err(CatalogueError::Database)?;
        let root = database::roots::get_root(&mut conn, root_id)
            .await?
            .ok_or_else(|| {
                RootError::Catalogue(CatalogueError::NotFound {
                    entity: "library_root",
                    id: root_id.to_string(),
                })
            })?;
        return Ok(root.status);
    }

    let mut tx = pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(CatalogueError::Database)?;

    let root = database::roots::get_root(&mut tx, root_id)
        .await?
        .ok_or_else(|| {
            RootError::Catalogue(CatalogueError::NotFound {
                entity: "library_root",
                id: root_id.to_string(),
            })
        })?;

    let target_status = match (root.status, outcome) {
        (RootStatus::Active, AvailabilityOutcome::NotAvailable(_)) => Some(RootStatus::Offline),
        (RootStatus::Offline, AvailabilityOutcome::Available) => Some(RootStatus::Active),
        _ => None,
    };

    if let Some(new_status) = target_status {
        let now_ms = database::now_ms()?;
        database::roots::set_root_status(&mut tx, root_id, new_status, now_ms).await?;
        tx.commit().await.map_err(CatalogueError::Database)?;
        Ok(new_status)
    } else {
        Ok(root.status)
    }
}

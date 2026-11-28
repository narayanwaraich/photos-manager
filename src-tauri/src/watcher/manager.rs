use sqlx::SqlitePool;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::database;
use crate::domain::{RootId, RootStatus, WatchingState};
use crate::jobs::{JobManager, ScanTrigger};
use crate::library::availability::{check_root_availability, DEFAULT_REACHABLE_TIMEOUT};
use crate::watcher::backend::{NotifyWatchBackend, WatchBackend, WatchCallback, WatchEvent};
use crate::watcher::debounce::{fold_event_into_inbox, RootInbox};
use crate::watcher::scheduler::{decide_next_auto_scan, SchedulerRootInput};

struct RootWatchEntry {
    watched_path: Option<PathBuf>,
    watching: WatchingState,
    changes_detected: bool,
    changes_detected_at: Option<Instant>,
    inbox: Arc<Mutex<RootInbox>>,
}

impl Default for RootWatchEntry {
    fn default() -> Self {
        Self {
            watched_path: None,
            watching: WatchingState::Off,
            changes_detected: false,
            changes_detected_at: None,
            inbox: Arc::new(Mutex::new(RootInbox::default())),
        }
    }
}

/// Manages per-root filesystem watchers and in-memory debounced change hints (TASK-1G D35).
///
/// Watcher events never write to any catalogue table (`files`, `folders`, `library_roots`, `jobs`).
/// They only fold timestamps into a fixed-size per-root inbox.
pub struct WatcherManager {
    backend: Arc<dyn WatchBackend>,
    entries: Mutex<HashMap<RootId, RootWatchEntry>>,
}

impl Default for WatcherManager {
    fn default() -> Self {
        Self::new_real()
    }
}

impl WatcherManager {
    pub fn new_real() -> Self {
        Self::with_backend(Arc::new(NotifyWatchBackend::new()))
    }

    pub fn with_backend(backend: Arc<dyn WatchBackend>) -> Self {
        Self {
            backend,
            entries: Mutex::new(HashMap::new()),
        }
    }

    /// Ensures an `Active`, available root has a running watcher on `path`.
    /// Watcher start failures degrade to `WatchingState::Off` and never fail the caller.
    pub fn ensure_watching(&self, root_id: &RootId, path: &Path) {
        let inbox = {
            let mut entries = match self.entries.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            let entry = entries.entry(root_id.clone()).or_default();
            if entry.watched_path.as_deref() == Some(path) && entry.watching != WatchingState::Off {
                return;
            }
            entry.watched_path = Some(path.to_path_buf());
            Arc::clone(&entry.inbox)
        };

        // Stop any prior stream for this root outside the entries lock.
        self.backend.stop_watch(root_id);

        let cb: WatchCallback = Arc::new(move |ev: WatchEvent| {
            let mut guard = match inbox.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            fold_event_into_inbox(&mut guard, ev, Instant::now());
        });

        let start_res = self.backend.start_watch(root_id, path, cb);

        let mut entries = match self.entries.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if let Some(entry) = entries.get_mut(root_id) {
            // Only commit state if this path was not superseded concurrently by stop/remove.
            if entry.watched_path.as_deref() == Some(path) {
                match start_res {
                    Ok(()) => {
                        entry.watching = WatchingState::On;
                    }
                    Err(err_msg) => {
                        eprintln!(
                            "failed to start filesystem watcher for root {}: {}",
                            root_id, err_msg
                        );
                        entry.watching = WatchingState::Off;
                        entry.watched_path = None;
                    }
                }
            }
        } else if start_res.is_ok() {
            drop(entries);
            self.backend.stop_watch(root_id);
        }
    }

    /// Stops watching `root_id` (e.g. when the root goes Offline or Paused).
    pub fn stop_watching(&self, root_id: &RootId) {
        {
            let mut entries = match self.entries.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            if let Some(entry) = entries.get_mut(root_id) {
                entry.watched_path = None;
                entry.watching = WatchingState::Off;
                let mut inbox = match entry.inbox.lock() {
                    Ok(g) => g,
                    Err(p) => p.into_inner(),
                };
                inbox.debounce.clear();
                inbox.error_seen = false;
            }
        }
        self.backend.stop_watch(root_id);
    }

    /// Stops watching and removes all in-memory state when a root is removed from the library.
    pub fn remove_root(&self, root_id: &RootId) {
        {
            let mut entries = match self.entries.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            entries.remove(root_id);
        }
        self.backend.stop_watch(root_id);
    }

    /// Clears the in-memory `changes_detected` mark when a scan starts on `root_id`.
    pub fn clear_changes_detected(&self, root_id: &RootId) {
        let mut entries = match self.entries.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if let Some(entry) = entries.get_mut(root_id) {
            entry.changes_detected = false;
            entry.changes_detected_at = None;
            let mut inbox = match entry.inbox.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            inbox.debounce.clear();
        }
    }

    /// Advances all per-root debouncers up to `now` and folds errors into `watching = Degraded` + `changes_detected = true`.
    pub fn flush_debouncers(&self, now: Instant) {
        let mut entries = match self.entries.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        for entry in entries.values_mut() {
            let mut inbox = match entry.inbox.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            if inbox.error_seen {
                inbox.error_seen = false;
                if entry.watching == WatchingState::On {
                    entry.watching = WatchingState::Degraded;
                }
                entry.changes_detected = true;
                entry.changes_detected_at.get_or_insert(now);
            }
            if inbox.debounce.poll_ready(now) {
                entry.changes_detected = true;
                entry.changes_detected_at.get_or_insert(now);
            }
        }
    }

    /// Returns `(changes_detected, watching)` for `root_id`.
    pub fn status_for_root(&self, root_id: &RootId) -> (bool, WatchingState) {
        let entries = match self.entries.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        match entries.get(root_id) {
            Some(e) => (e.changes_detected, e.watching),
            None => (false, WatchingState::Off),
        }
    }

    /// Returns `(changes_detected, changes_detected_at)` for scheduler ordering.
    pub fn change_info_for_root(&self, root_id: &RootId) -> (bool, Option<Instant>) {
        let entries = match self.entries.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        match entries.get(root_id) {
            Some(e) => (e.changes_detected, e.changes_detected_at),
            None => (false, None),
        }
    }

    /// Test helper to inject a changes-detected state with an explicit timestamp.
    #[cfg(test)]
    pub fn set_changes_detected_for_test(&self, root_id: &RootId, at: Instant) {
        let mut entries = match self.entries.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let entry = entries.entry(root_id.clone()).or_default();
        entry.changes_detected = true;
        entry.changes_detected_at = Some(at);
    }

    /// Synchronises watchers on startup after checking each `Active` root's availability.
    /// Never watches an offline, paused, removed, or unavailable root (decision m).
    pub async fn sync_startup_watchers(
        &self,
        pool: &SqlitePool,
        in_flight: &Arc<Mutex<HashSet<RootId>>>,
    ) {
        let items = match crate::library::roots::list_roots_with_availability(pool, in_flight).await
        {
            Ok(i) => i,
            Err(err) => {
                eprintln!("startup watcher sync failed to list roots: {}", err.code());
                return;
            }
        };

        for item in items {
            let status = match crate::library::apply_availability_result(
                pool,
                &item.summary.id,
                Some(item.summary.status),
                item.availability,
            )
            .await
            {
                Ok(s) => s,
                Err(err) => {
                    eprintln!(
                        "startup watcher availability update failed for root {}: {}",
                        item.summary.id,
                        err.code()
                    );
                    item.summary.status
                }
            };

            if status == RootStatus::Active && item.availability.is_available() {
                if let Ok(Some(row)) = {
                    let mut conn_res = pool.acquire().await;
                    match conn_res {
                        Ok(ref mut conn) => database::roots::get_root(conn, &item.summary.id).await,
                        Err(e) => Err(crate::domain::CatalogueError::Database(e)),
                    }
                } {
                    let path = crate::filesystem::blob_to_native(&row.path_native);
                    self.ensure_watching(&item.summary.id, &path);
                }
            } else {
                self.stop_watching(&item.summary.id);
            }
        }
    }

    /// Also supports checking a single root availability directly when starting a watch.
    pub async fn sync_root_after_availability(
        &self,
        root_id: &RootId,
        root_path: PathBuf,
        stored_fs_uuid: Option<String>,
        status: RootStatus,
        in_flight: &Arc<Mutex<HashSet<RootId>>>,
    ) {
        if status != RootStatus::Active {
            self.stop_watching(root_id);
            return;
        }
        let outcome = check_root_availability(
            root_id,
            root_path.clone(),
            stored_fs_uuid,
            in_flight,
            DEFAULT_REACHABLE_TIMEOUT,
            |p| p.is_dir(),
            crate::library::volume::describe_volume,
        )
        .await;
        if outcome.is_available() {
            self.ensure_watching(root_id, &root_path);
        } else {
            self.stop_watching(root_id);
        }
    }
}

/// Runs one tick of the debouncer flush + automatic scan scheduler (called every 1 s by the runner).
pub async fn run_auto_scan_tick(
    pool: &SqlitePool,
    watcher_manager: &Arc<WatcherManager>,
    job_manager: &Arc<JobManager>,
    now_instant: Instant,
    now_ms: i64,
) {
    watcher_manager.flush_debouncers(now_instant);

    let mut conn = match pool.acquire().await {
        Ok(c) => c,
        Err(err) => {
            eprintln!("auto-scan tick failed to acquire DB connection: {err}");
            return;
        }
    };

    let auto_scan_enabled = match database::get_auto_scan(&mut conn).await {
        Ok(v) => v,
        Err(err) => {
            eprintln!(
                "auto-scan tick failed to read setting [{}]: {}",
                err.code(),
                err
            );
            return;
        }
    };

    if !auto_scan_enabled || job_manager.has_active_auto_scan() {
        return;
    }

    let records = match database::roots::list_roots_summaries(&mut conn).await {
        Ok(r) => r,
        Err(err) => {
            eprintln!(
                "auto-scan tick failed to list roots [{}]: {}",
                err.code(),
                err
            );
            return;
        }
    };
    drop(conn);

    let inputs: Vec<SchedulerRootInput> = records
        .iter()
        .map(|r| {
            let (changes_detected, changes_detected_at) =
                watcher_manager.change_info_for_root(&r.id);
            let last_job_finished = r.last_job.as_ref().and_then(|j| j.finished_at_ms);
            let last_scan_ended_at_ms = match (r.last_scan_at_ms, last_job_finished) {
                (Some(a), Some(b)) => Some(a.max(b)),
                (Some(a), None) => Some(a),
                (None, Some(b)) => Some(b),
                (None, None) => None,
            };

            SchedulerRootInput {
                id: r.id.clone(),
                status: r.status,
                changes_detected: changes_detected && r.last_scan_at_ms.is_some(),
                changes_detected_at,
                has_active_job: job_manager.active_job_for_root(&r.id).is_some(),
                last_scan_ended_at_ms,
                last_completed_scan_at_ms: r.last_scan_at_ms,
            }
        })
        .collect();

    let any_auto = job_manager.has_active_auto_scan();
    let Some(candidate) =
        decide_next_auto_scan(auto_scan_enabled, any_auto, &inputs, now_instant, now_ms)
    else {
        return;
    };

    if let Err(err) = job_manager
        .start_scan_with_trigger(candidate.root_id.clone(), ScanTrigger::Auto, |_| {})
        .await
    {
        if err.code == "root_offline" || err.code == "root_volume_changed" {
            watcher_manager.stop_watching(&candidate.root_id);
        }
        eprintln!(
            "auto-scan skipped/refused for root {}: [{}]",
            candidate.root_id, err.code
        );
    }
}

/// Spawns startup watcher synchronization and the 1-second debouncer/scheduler runner task.
pub fn spawn_watcher_and_scheduler(
    pool: SqlitePool,
    in_flight: Arc<Mutex<HashSet<RootId>>>,
    watcher_manager: Arc<WatcherManager>,
    job_manager: Arc<JobManager>,
) {
    tauri::async_runtime::spawn(async move {
        watcher_manager
            .sync_startup_watchers(&pool, &in_flight)
            .await;

        let mut ticker = tokio::time::interval(Duration::from_secs(1));
        loop {
            ticker.tick().await;
            let now_instant = Instant::now();
            let now_ms = database::now_ms().unwrap_or(0);
            run_auto_scan_tick(&pool, &watcher_manager, &job_manager, now_instant, now_ms).await;
        }
    });
}

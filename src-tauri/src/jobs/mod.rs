use sqlx::SqlitePool;
use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};

use crate::commands::error::{fixed_message_for_code, IpcError};
use crate::database::{self, FinalizeJobParams, NewJobParams};
use crate::domain::{CatalogueError, RootId};
use crate::scanner::{
    scan_root_guarded, ScanCancel, ScanCounts, ScanError, ScanGuard, ScanOptions, ScanOutcome,
    ScanProgress,
};

#[cfg(test)]
pub mod tests;

/// Unique identifier for a background job.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct JobId(pub String);

impl JobId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for JobId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for JobId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for JobId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Typed job kind (Decision a: only Scan in Phase 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobKind {
    Scan,
}

impl JobKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Scan => "scan",
        }
    }
}

/// Status of a job matching the database CHECK constraint (spec §5.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Queued,
    Running,
    Cancelling,
    Completed,
    Cancelled,
    Failed,
    Paused,
}

impl JobStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Cancelling => "cancelling",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
            Self::Paused => "paused",
        }
    }
}

/// DTO for counts reported in scan events.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanCountsDto {
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

impl From<ScanCounts> for ScanCountsDto {
    fn from(c: ScanCounts) -> Self {
        Self {
            folders_seen: c.folders_seen,
            files_seen: c.files_seen,
            new: c.new,
            changed: c.changed,
            unchanged: c.unchanged,
            moved: c.moved,
            marked_missing: c.marked_missing,
            marked_inaccessible: c.marked_inaccessible,
            unsupported: c.unsupported,
            skipped_by_rule: c.skipped_by_rule,
        }
    }
}

/// DTO for warnings reported in scan events.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanWarningDto {
    pub code: String,
    pub rel_path: String,
}

/// Tagged union of all events emitted during a scan (Decision g).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ScanEvent {
    #[serde(rename_all = "camelCase")]
    Started { job_id: String, root_id: String },
    #[serde(rename_all = "camelCase")]
    Progress {
        job_id: String,
        folders_seen: u64,
        files_seen: u64,
        batches: u64,
    },
    #[serde(rename_all = "camelCase")]
    Finished {
        job_id: String,
        outcome: String, // "completed" | "cancelled"
        counts: ScanCountsDto,
        warning_count: u64,
        warnings: Vec<ScanWarningDto>,
        elapsed_ms: u64,
    },
    #[serde(rename_all = "camelCase")]
    Failed { job_id: String, code: String },
}

/// Result returned immediately when `start_scan` creates the job (Decision g).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartScanResult {
    pub job_id: String,
}

/// Active in-memory job handle held while a scan is running (Decision c).
pub struct RunningJob {
    pub root_id: RootId,
    pub cancel: ScanCancel,
}

/// Process-wide job manager tracking in-flight jobs and coordinating cancellation (Decision a, c).
pub struct JobManager {
    pool: SqlitePool,
    active_jobs: Mutex<HashMap<JobId, RunningJob>>,
}

impl JobManager {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            active_jobs: Mutex::new(HashMap::new()),
        }
    }

    /// Looks up active job ID for a root, if currently running.
    pub fn active_job_for_root(&self, root_id: &RootId) -> Option<String> {
        let map = match self.active_jobs.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        for (job_id, job) in map.iter() {
            if &job.root_id == root_id {
                return Some(job_id.to_string());
            }
        }
        None
    }

    /// Removes an active job from the in-memory map.
    pub fn remove_active_job(&self, job_id: &str) {
        let mut map = match self.active_jobs.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        map.remove(&JobId::from(job_id));
    }

    /// Cancels a running scan job (Decision c).
    ///
    /// Sets the `ScanCancel` atomic flag and transitions the database row to `cancelling`.
    /// Returns `job_not_found` if the job is unknown or already finished.
    pub async fn cancel_scan_impl(&self, job_id_str: &str) -> Result<(), IpcError> {
        let job_id = JobId::from(job_id_str);
        let cancel = {
            let map = match self.active_jobs.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            match map.get(&job_id) {
                Some(job) => job.cancel.clone(),
                None => {
                    return Err(IpcError::new(
                        "job_not_found",
                        fixed_message_for_code("job_not_found"),
                    ));
                }
            }
        };

        // 1. Signal cancellation
        cancel.cancel();

        // 2. Update status in database to 'cancelling'
        let now_ms = database::now_ms().map_err(IpcError::from)?;
        let mut conn = self
            .pool
            .acquire()
            .await
            .map_err(CatalogueError::Database)
            .map_err(IpcError::from)?;
        database::update_job_status(&mut conn, job_id.as_str(), "cancelling", now_ms)
            .await
            .map_err(IpcError::from)?;

        Ok(())
    }

    /// Core implementation of `start_scan` (Decision a, c, d, g, h).
    ///
    /// 1. Acquires `ScanGuard` for the root. If already running, returns `scan_already_running` immediately.
    /// 2. Generates UUID v7 `job_id` and `scan_generation`.
    /// 3. Persists the `jobs` row with status `running` and generation in `payload_json` before touching disk.
    /// 4. Stores `RunningJob` in the in-memory map.
    /// 5. Emits `ScanEvent::Started`.
    /// 6. Spawns `run_tracked_job` supervisor.
    /// 7. Returns `StartScanResult { job_id }` immediately.
    pub async fn start_scan_impl<S>(
        self: &Arc<Self>,
        root_id: RootId,
        sink: S,
    ) -> Result<StartScanResult, IpcError>
    where
        S: Fn(ScanEvent) + Send + Sync + 'static,
    {
        // 1. Acquire ScanGuard BEFORE touching anything (Decision d)
        let guard = match ScanGuard::acquire(&root_id) {
            Ok(g) => g,
            Err(ScanError::AlreadyRunning(_)) => {
                return Err(IpcError::new(
                    "scan_already_running",
                    fixed_message_for_code("scan_already_running"),
                ));
            }
            Err(e) => return Err(IpcError::from(e)),
        };

        let job_id = uuid::Uuid::now_v7().to_string();
        let scan_generation = uuid::Uuid::now_v7().to_string();
        let now_ms = database::now_ms().map_err(IpcError::from)?;

        let payload_json = serde_json::json!({
            "root_id": root_id.as_str(),
            "generation": scan_generation,
        })
        .to_string();

        // 2. Persist job row BEFORE scan touches disk (spec §6.2 step 1, Decision d)
        let mut conn = self
            .pool
            .acquire()
            .await
            .map_err(CatalogueError::Database)
            .map_err(IpcError::from)?;
        database::insert_job(
            &mut conn,
            &NewJobParams {
                id: &job_id,
                root_id: Some(root_id.as_str()),
                job_type: "scan",
                status: "running",
                priority: 50,
                payload_json: &payload_json,
                now_ms,
            },
        )
        .await
        .map_err(IpcError::from)?;
        drop(conn);

        // 3. Register running job for cancellation (Decision c)
        let cancel = ScanCancel::new();
        {
            let mut map = match self.active_jobs.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            map.insert(
                JobId::from(job_id.clone()),
                RunningJob {
                    root_id: root_id.clone(),
                    cancel: cancel.clone(),
                },
            );
        }

        // 4. Emit started event (Decision g)
        sink(ScanEvent::Started {
            job_id: job_id.clone(),
            root_id: root_id.to_string(),
        });

        // 5. Setup watch channel for progress throttling (Decision g)
        let (watch_tx, watch_rx) = tokio::sync::watch::channel(None);

        let options = ScanOptions {
            generation: Some(scan_generation),
            ..Default::default()
        };

        let pool_for_scan = self.pool.clone();
        let root_id_for_scan = root_id.clone();
        let cancel_for_scan = cancel.clone();

        let progress_cb = move |p: ScanProgress| {
            let _ = watch_tx.send_replace(Some(p));
        };

        let scan_fut = async move {
            scan_root_guarded(
                guard,
                &pool_for_scan,
                &root_id_for_scan,
                options,
                cancel_for_scan,
                progress_cb,
            )
            .await
        };

        // 6. Spawn panic-isolated supervisor (Decision h)
        run_tracked_job(
            self.pool.clone(),
            job_id.clone(),
            root_id,
            Arc::clone(self),
            scan_fut,
            watch_rx,
            sink,
        );

        Ok(StartScanResult { job_id })
    }
}

/// Generic tracked job runner with panic isolation, dual-rate throttling, and retention pruning (Decision e, g, h).
///
/// Spawns a worker task running `worker_future` and a supervisor task that:
/// - Throttles live progress to at most 4 events per second (250 ms ticker).
/// - Persists progress to the `jobs` table at most once per second.
/// - Catches worker task panics or aborts and records status `failed` with code `internal`.
/// - Finalizes status (`completed`, `cancelled`, or `failed`) with exact counts in a single transaction.
/// - Prunes older finished jobs exceeding 20 per root.
/// - Emits exactly ONE terminal event (`finished` or `failed`) last.
pub fn run_tracked_job<F, S>(
    pool: SqlitePool,
    job_id: String,
    root_id: RootId,
    manager: Arc<JobManager>,
    worker_future: F,
    mut watch_rx: tokio::sync::watch::Receiver<Option<ScanProgress>>,
    sink: S,
) -> tokio::task::JoinHandle<()>
where
    F: Future<Output = Result<ScanOutcome, ScanError>> + Send + 'static,
    S: Fn(ScanEvent) + Send + Sync + 'static,
{
    let worker_handle = tokio::spawn(worker_future);

    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_millis(250));
        let mut tick_count: u32 = 0;
        let mut last_progress_done: i64 = 0;
        let mut worker_handle = worker_handle;

        let worker_res = loop {
            tokio::select! {
                res = &mut worker_handle => {
                    break res;
                }
                _ = ticker.tick() => {
                    tick_count = tick_count.wrapping_add(1);
                    if watch_rx.has_changed().unwrap_or(false) {
                        if let Some(p) = *watch_rx.borrow_and_update() {
                            last_progress_done = p.files_seen as i64;
                            sink(ScanEvent::Progress {
                                job_id: job_id.clone(),
                                folders_seen: p.folders_seen,
                                files_seen: p.files_seen,
                                batches: p.batches_committed,
                            });
                        }
                    }

                    // Once per second (every 4 ticks): write progress to jobs table (Decision e)
                    if tick_count.is_multiple_of(4) {
                        if let Ok(now_ms) = database::now_ms() {
                            if let Ok(mut conn) = pool.acquire().await {
                                let _ = database::update_job_progress(&mut conn, &job_id, last_progress_done, now_ms).await;
                            }
                        }
                    }
                }
            }
        };

        // Scan ended: remove from active jobs map (Decision c)
        manager.remove_active_job(&job_id);

        let now_ms = database::now_ms().unwrap_or(0);

        match worker_res {
            // Case 1: Panic isolation (worker panicked or was aborted) (Decision h)
            Err(_join_err) => {
                if let Ok(mut tx) = pool.begin_with("BEGIN IMMEDIATE").await {
                    let _ = database::finalize_job(
                        &mut tx,
                        &FinalizeJobParams {
                            id: &job_id,
                            status: "failed",
                            progress_done: last_progress_done,
                            warning_count: 0,
                            error_count: 1,
                            error_code: Some("internal"),
                            error_message: Some(fixed_message_for_code("internal")),
                            finished_at_ms: now_ms,
                            updated_at_ms: now_ms,
                        },
                    )
                    .await;

                    let _ = database::prune_finished_jobs(&mut tx, root_id.as_str(), 20).await;
                    let _ = tx.commit().await;
                }

                // Exactly one terminal event, last (Decision g)
                sink(ScanEvent::Failed {
                    job_id,
                    code: "internal".to_string(),
                });
            }

            // Case 2: Scan returned ScanError
            Ok(Err(scan_err)) => {
                let code = scan_err.code();
                if let Ok(mut tx) = pool.begin_with("BEGIN IMMEDIATE").await {
                    let _ = database::finalize_job(
                        &mut tx,
                        &FinalizeJobParams {
                            id: &job_id,
                            status: "failed",
                            progress_done: last_progress_done,
                            warning_count: 0,
                            error_count: 1,
                            error_code: Some(code),
                            error_message: Some(fixed_message_for_code(code)),
                            finished_at_ms: now_ms,
                            updated_at_ms: now_ms,
                        },
                    )
                    .await;

                    let _ = database::prune_finished_jobs(&mut tx, root_id.as_str(), 20).await;
                    let _ = tx.commit().await;
                }

                sink(ScanEvent::Failed {
                    job_id,
                    code: code.to_string(),
                });
            }

            // Case 3: Scan cancelled (Decision c, i)
            Ok(Ok(ScanOutcome::Cancelled(report))) => {
                let warnings: Vec<ScanWarningDto> = report
                    .warnings
                    .into_iter()
                    .take(100)
                    .map(|w| ScanWarningDto {
                        code: w.code,
                        rel_path: w.path_display,
                    })
                    .collect();

                if let Ok(mut tx) = pool.begin_with("BEGIN IMMEDIATE").await {
                    let _ = database::finalize_job(
                        &mut tx,
                        &FinalizeJobParams {
                            id: &job_id,
                            status: "cancelled",
                            progress_done: report.counts.files_seen as i64,
                            warning_count: report.warning_count,
                            error_count: 0,
                            error_code: None,
                            error_message: None,
                            finished_at_ms: now_ms,
                            updated_at_ms: now_ms,
                        },
                    )
                    .await;

                    let _ = database::prune_finished_jobs(&mut tx, root_id.as_str(), 20).await;
                    let _ = tx.commit().await;
                }

                sink(ScanEvent::Finished {
                    job_id,
                    outcome: "cancelled".to_string(),
                    counts: report.counts.into(),
                    warning_count: report.warning_count,
                    warnings,
                    elapsed_ms: report.elapsed.as_millis() as u64,
                });
            }

            // Case 4: Scan completed successfully (Decision e, g)
            Ok(Ok(ScanOutcome::Completed(report))) => {
                let warnings: Vec<ScanWarningDto> = report
                    .warnings
                    .into_iter()
                    .take(100)
                    .map(|w| ScanWarningDto {
                        code: w.code,
                        rel_path: w.path_display,
                    })
                    .collect();

                if let Ok(mut tx) = pool.begin_with("BEGIN IMMEDIATE").await {
                    let _ = database::finalize_job(
                        &mut tx,
                        &FinalizeJobParams {
                            id: &job_id,
                            status: "completed",
                            progress_done: report.counts.files_seen as i64,
                            warning_count: report.warning_count,
                            error_count: 0,
                            error_code: None,
                            error_message: None,
                            finished_at_ms: now_ms,
                            updated_at_ms: now_ms,
                        },
                    )
                    .await;

                    let _ = database::prune_finished_jobs(&mut tx, root_id.as_str(), 20).await;
                    let _ = tx.commit().await;
                }

                sink(ScanEvent::Finished {
                    job_id,
                    outcome: "completed".to_string(),
                    counts: report.counts.into(),
                    warning_count: report.warning_count,
                    warnings,
                    elapsed_ms: report.elapsed.as_millis() as u64,
                });
            }
        }
    })
}

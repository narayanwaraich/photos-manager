use sqlx::SqlitePool;
use std::collections::{HashMap, HashSet};
use std::future::Future;
#[cfg(test)]
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::commands::error::{fixed_message_for_code, IpcError};
use crate::database::{self, FinalizeJobParams, NewJobParams};
use crate::domain::{CatalogueError, RootId, RootStatus};
#[cfg(test)]
use crate::domain::{RootError, VolumeInfo};
use crate::library::{
    apply_availability_result, check_root_availability, AvailabilityOutcome, UnavailableReason,
    SCAN_AVAILABILITY_TIMEOUT,
};
use crate::scanner::{
    scan_root_guarded_with_in_flight, ScanCancel, ScanCounts, ScanError, ScanGuard, ScanOptions,
    ScanOutcome, ScanProgress,
};

#[cfg(test)]
pub mod tests;

/// All stable error codes that `jobs/` or `scanner/` can emit for a scan failure (TASK-1F decision j).
/// Kept as the single Rust source of truth; verified bidirectionally against `SCAN_FAILURE_MESSAGES` in `src/lib/scanText.ts`.
pub const ALL_SCAN_FAILURE_CODES: &[&str] = &[
    "scan_root_unreachable",
    "scan_walk_failed",
    "scan_root_option_unsupported",
    "scan_already_running",
    "root_offline",
    "root_volume_changed",
    "root_busy",
    "root_not_found",
    "job_not_found",
    "interrupted",
    "internal",
    "database",
    "not_found",
    "invalid_enum",
];

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
        recorded: bool,
    },
    #[serde(rename_all = "camelCase")]
    Failed {
        job_id: String,
        code: String,
        recorded: bool,
    },
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

#[cfg(test)]
pub type FinalWriteHookFn = Arc<
    dyn Fn(usize) -> std::pin::Pin<Box<dyn Future<Output = Result<(), CatalogueError>> + Send>>
        + Send
        + Sync,
>;

#[cfg(test)]
#[derive(Clone, Default)]
pub struct JobManagerTestDeps {
    pub availability_timeout: Option<std::time::Duration>,
    pub is_dir_fn: Option<Arc<dyn Fn(&Path) -> bool + Send + Sync>>,
    pub describer: Option<Arc<dyn Fn(&Path) -> Result<VolumeInfo, RootError> + Send + Sync>>,
    pub final_write_hook: Option<FinalWriteHookFn>,
}

/// Process-wide job manager tracking in-flight jobs and coordinating cancellation (Decision a, c).
pub struct JobManager {
    pool: SqlitePool,
    active_jobs: Mutex<HashMap<JobId, RunningJob>>,
    in_flight_checks: Arc<Mutex<HashSet<RootId>>>,
    #[cfg(test)]
    test_deps: Mutex<Option<JobManagerTestDeps>>,
}

impl JobManager {
    pub fn new(pool: SqlitePool) -> Self {
        Self::new_with_in_flight(pool, Arc::new(Mutex::new(HashSet::new())))
    }

    /// Creates a `JobManager` sharing the given `in_flight_checks` set with `list_roots`.
    pub fn new_with_in_flight(
        pool: SqlitePool,
        in_flight_checks: Arc<Mutex<HashSet<RootId>>>,
    ) -> Self {
        Self {
            pool,
            active_jobs: Mutex::new(HashMap::new()),
            in_flight_checks,
            #[cfg(test)]
            test_deps: Mutex::new(None),
        }
    }

    #[cfg(test)]
    pub fn set_test_deps(&self, deps: JobManagerTestDeps) {
        let mut guard = match self.test_deps.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        *guard = Some(deps);
    }

    #[cfg(test)]
    fn get_test_deps(&self) -> Option<JobManagerTestDeps> {
        let guard = match self.test_deps.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        guard.clone()
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

        // 2. Update status in database to 'cancelling' if still running (Decision c, R2)
        let now_ms = database::now_ms().map_err(IpcError::from)?;
        let mut conn = self
            .pool
            .acquire()
            .await
            .map_err(CatalogueError::Database)
            .map_err(IpcError::from)?;
        // Row count (0 if job finished concurrently) is unneeded; database errors still propagate via `?`.
        let _rows_updated = database::mark_job_cancelling(&mut conn, job_id.as_str(), now_ms)
            .await
            .map_err(IpcError::from)?;

        Ok(())
    }

    /// Core implementation of `start_scan` (Decision a, c, d, g, h, R4).
    ///
    /// Generates UUID v7 `scan_generation` and calls `start_scan_with_options`.
    pub async fn start_scan_impl<S>(
        self: &Arc<Self>,
        root_id: RootId,
        sink: S,
    ) -> Result<StartScanResult, IpcError>
    where
        S: Fn(ScanEvent) + Send + Sync + 'static,
    {
        let scan_generation = uuid::Uuid::now_v7().to_string();
        let options = ScanOptions {
            generation: Some(scan_generation),
            ..Default::default()
        };
        self.start_scan_with_options(root_id, options, sink).await
    }

    /// Internal scan starter taking explicit `ScanOptions` (spec §6.2 step 1, Decision d, R4).
    ///
    /// 1. Acquires `ScanGuard` for the root. If already running, returns `scan_already_running` immediately.
    /// 2. Verifies root existence and runs pre-flight availability check (5 s timeout, same-volume guard)
    ///    BEFORE creating any `jobs` row (TASK-1F decisions c, d).
    /// 3. Generates UUID v7 `job_id` and persists the `jobs` row with status `running`.
    /// 4. Stores `RunningJob` in the in-memory map.
    /// 5. Emits `ScanEvent::Started`.
    /// 6. Spawns `run_tracked_job` supervisor.
    /// 7. Returns `StartScanResult { job_id }` immediately.
    async fn start_scan_with_options<S>(
        self: &Arc<Self>,
        root_id: RootId,
        options: ScanOptions,
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

        // 2. Fetch root & stored volume fs_uuid, then run pre-flight availability check BEFORE creating any job row
        let mut conn = self
            .pool
            .acquire()
            .await
            .map_err(CatalogueError::Database)
            .map_err(IpcError::from)?;
        let root = match database::roots::get_root(&mut conn, &root_id)
            .await
            .map_err(IpcError::from)?
        {
            Some(r) => r,
            None => {
                return Err(IpcError::new(
                    "root_not_found",
                    fixed_message_for_code("root_not_found"),
                ));
            }
        };

        let stored_fs_uuid = match &root.volume_id {
            Some(vid) => database::volumes::get_volume(&mut conn, vid)
                .await
                .map_err(IpcError::from)?
                .and_then(|v| v.fs_uuid),
            None => None,
        };
        drop(conn);

        let root_path = crate::filesystem::blob_to_native(&root.path_native);
        #[cfg(test)]
        let test_deps = self.get_test_deps();
        #[cfg(test)]
        let timeout = test_deps
            .as_ref()
            .and_then(|d| d.availability_timeout)
            .unwrap_or(SCAN_AVAILABILITY_TIMEOUT);
        #[cfg(not(test))]
        let timeout = SCAN_AVAILABILITY_TIMEOUT;

        let availability = {
            #[cfg(test)]
            {
                let is_dir_fn = test_deps
                    .as_ref()
                    .and_then(|d| d.is_dir_fn.clone())
                    .unwrap_or_else(|| Arc::new(|p: &Path| p.is_dir()));
                let describer = test_deps
                    .as_ref()
                    .and_then(|d| d.describer.clone())
                    .unwrap_or_else(|| Arc::new(crate::library::volume::describe_volume));

                check_root_availability(
                    &root_id,
                    root_path,
                    stored_fs_uuid,
                    &self.in_flight_checks,
                    timeout,
                    move |p| is_dir_fn(p),
                    move |p| describer(p),
                )
                .await
            }
            #[cfg(not(test))]
            {
                check_root_availability(
                    &root_id,
                    root_path,
                    stored_fs_uuid,
                    &self.in_flight_checks,
                    timeout,
                    |p| p.is_dir(),
                    crate::library::volume::describe_volume,
                )
                .await
            }
        };

        match availability {
            AvailabilityOutcome::Available => {
                if root.status == RootStatus::Offline {
                    apply_availability_result(
                        &self.pool,
                        &root_id,
                        Some(root.status),
                        availability,
                    )
                    .await
                    .map_err(IpcError::from)?;
                }
            }
            AvailabilityOutcome::NotAvailable(UnavailableReason::Unreachable) => {
                apply_availability_result(&self.pool, &root_id, Some(root.status), availability)
                    .await
                    .map_err(IpcError::from)?;
                return Err(IpcError::new(
                    "root_offline",
                    fixed_message_for_code("root_offline"),
                ));
            }
            AvailabilityOutcome::NotAvailable(UnavailableReason::VolumeChanged) => {
                apply_availability_result(&self.pool, &root_id, Some(root.status), availability)
                    .await
                    .map_err(IpcError::from)?;
                return Err(IpcError::new(
                    "root_volume_changed",
                    fixed_message_for_code("root_volume_changed"),
                ));
            }
            AvailabilityOutcome::Unknown => {
                // Unknown never writes status, but refuses start_scan without creating a job row.
                return Err(IpcError::new(
                    "root_offline",
                    fixed_message_for_code("root_offline"),
                ));
            }
        }

        let job_id = uuid::Uuid::now_v7().to_string();
        let now_ms = database::now_ms().map_err(IpcError::from)?;

        let payload_json = serde_json::json!({
            "root_id": root_id.as_str(),
            "generation": options.generation,
        })
        .to_string();

        // 3. Persist job row BEFORE scan touches disk (spec §6.2 step 1, Decision d)
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

        // 4. Register running job for cancellation (Decision c)
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

        // 5. Emit started event (Decision g)
        sink(ScanEvent::Started {
            job_id: job_id.clone(),
            root_id: root_id.to_string(),
        });

        // 6. Setup watch channel for progress throttling (Decision g)
        let (watch_tx, watch_rx) = tokio::sync::watch::channel(None);

        let pool_for_scan = self.pool.clone();
        let root_id_for_scan = root_id.clone();
        let cancel_for_scan = cancel.clone();
        let in_flight_for_scan = Arc::clone(&self.in_flight_checks);

        let progress_cb = move |p: ScanProgress| {
            // Latest progress wins; any unread intermediate value is intentionally replaced.
            let _prev = watch_tx.send_replace(Some(p));
        };

        let scan_fut = async move {
            scan_root_guarded_with_in_flight(
                guard,
                &pool_for_scan,
                &root_id_for_scan,
                options,
                cancel_for_scan,
                &in_flight_for_scan,
                progress_cb,
            )
            .await
        };

        // 7. Spawn panic-isolated supervisor (Decision h)
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

async fn try_finalize_once(
    pool: &SqlitePool,
    root_id: &RootId,
    params: &FinalizeJobParams<'_>,
    attempt: usize,
    #[cfg(test)] hook: Option<&FinalWriteHookFn>,
) -> Result<(), CatalogueError> {
    #[cfg(not(test))]
    let _ = attempt;

    #[cfg(test)]
    if let Some(h) = hook {
        h(attempt).await?;
    }

    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    database::finalize_job(&mut tx, params).await?;
    database::prune_finished_jobs(&mut tx, root_id.as_str(), 20).await?;
    tx.commit().await?;
    Ok(())
}

/// Persists the terminal job state and prunes older finished jobs in a single transaction,
/// retrying the whole sequence once after 200 ms on any error (TASK-1F decision k, risk 63).
async fn finalize_and_prune_with_retry(
    pool: &SqlitePool,
    root_id: &RootId,
    params: &FinalizeJobParams<'_>,
    #[cfg(test)] hook: Option<&FinalWriteHookFn>,
) -> bool {
    if try_finalize_once(
        pool,
        root_id,
        params,
        0,
        #[cfg(test)]
        hook,
    )
    .await
    .is_ok()
    {
        return true;
    }

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    if try_finalize_once(
        pool,
        root_id,
        params,
        1,
        #[cfg(test)]
        hook,
    )
    .await
    .is_ok()
    {
        return true;
    }

    eprintln!(
        "error recording terminal job state: job_id={} code={}",
        params.id,
        params.error_code.unwrap_or(params.status)
    );
    false
}

/// Generic tracked job runner with panic isolation, dual-rate throttling, and retention pruning (Decision e, g, h).
///
/// Spawns a worker task running `worker_future` and a supervisor task that:
/// - Throttles live progress to at most 4 events per second (250 ms ticker).
/// - Persists progress to the `jobs` table at most once per second.
/// - Catches worker task panics or aborts and records status `failed` with code `internal`.
/// - Finalizes status (`completed`, `cancelled`, or `failed`) with exact counts in a single transaction, retrying once on error.
/// - Prunes older finished jobs exceeding 20 per root.
/// - Removes the active job entry AFTER the final write attempt and BEFORE emitting the terminal event.
/// - Emits exactly ONE terminal event (`finished` or `failed`) last, carrying `recorded`.
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
                                // Best-effort periodic progress write; final count is persisted at completion.
                                let _ = database::update_job_progress(&mut conn, &job_id, last_progress_done, now_ms).await;
                            }
                        }
                    }
                }
            }
        };

        let now_ms = database::now_ms().unwrap_or(0);

        enum TerminalOutcomeKind {
            Failed {
                code: &'static str,
            },
            Finished {
                outcome: &'static str,
                counts: ScanCountsDto,
                warning_count: u64,
                warnings: Vec<ScanWarningDto>,
                elapsed_ms: u64,
            },
        }

        let (status, progress_done, warning_count, error_count, error_code, outcome_kind) =
            match worker_res {
                Err(_join_err) => (
                    "failed",
                    last_progress_done,
                    0u64,
                    1u64,
                    Some("internal"),
                    TerminalOutcomeKind::Failed { code: "internal" },
                ),
                Ok(Err(scan_err)) => {
                    let code = scan_err.code();
                    if matches!(scan_err, ScanError::RootUnreachable { .. }) {
                        if let Err(err) = apply_availability_result(
                            &pool,
                            &root_id,
                            None,
                            AvailabilityOutcome::NotAvailable(UnavailableReason::Unreachable),
                        )
                        .await
                        {
                            eprintln!(
                                "failed to mark root offline after scan_root_unreachable: job_id={} err_code={}",
                                job_id,
                                err.code()
                            );
                        }
                    }
                    (
                        "failed",
                        last_progress_done,
                        0u64,
                        1u64,
                        Some(code),
                        TerminalOutcomeKind::Failed { code },
                    )
                }
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
                    (
                        "cancelled",
                        report.counts.files_seen as i64,
                        report.warning_count,
                        0u64,
                        None,
                        TerminalOutcomeKind::Finished {
                            outcome: "cancelled",
                            counts: report.counts.into(),
                            warning_count: report.warning_count,
                            warnings,
                            elapsed_ms: report.elapsed.as_millis() as u64,
                        },
                    )
                }
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
                    (
                        "completed",
                        report.counts.files_seen as i64,
                        report.warning_count,
                        0u64,
                        None,
                        TerminalOutcomeKind::Finished {
                            outcome: "completed",
                            counts: report.counts.into(),
                            warning_count: report.warning_count,
                            warnings,
                            elapsed_ms: report.elapsed.as_millis() as u64,
                        },
                    )
                }
            };

        let error_message = error_code.map(fixed_message_for_code);
        let finalize_params = FinalizeJobParams {
            id: &job_id,
            status,
            progress_done,
            warning_count,
            error_count,
            error_code,
            error_message,
            finished_at_ms: now_ms,
            updated_at_ms: now_ms,
        };

        #[cfg(test)]
        let hook = manager.get_test_deps().and_then(|d| d.final_write_hook);

        let recorded = finalize_and_prune_with_retry(
            &pool,
            &root_id,
            &finalize_params,
            #[cfg(test)]
            hook.as_ref(),
        )
        .await;

        // Order (TASK-1F decision k): remove active job AFTER final write attempt and BEFORE terminal event.
        manager.remove_active_job(&job_id);

        match outcome_kind {
            TerminalOutcomeKind::Failed { code } => {
                sink(ScanEvent::Failed {
                    job_id,
                    code: code.to_string(),
                    recorded,
                });
            }
            TerminalOutcomeKind::Finished {
                outcome,
                counts,
                warning_count,
                warnings,
                elapsed_ms,
            } => {
                sink(ScanEvent::Finished {
                    job_id,
                    outcome: outcome.to_string(),
                    counts,
                    warning_count,
                    warnings,
                    elapsed_ms,
                    recorded,
                });
            }
        }
    })
}

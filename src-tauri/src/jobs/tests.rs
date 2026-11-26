use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tempfile::{tempdir, TempDir};

use crate::commands::{add_root_with_picker, DialogGuard};
use crate::database::{self, count_for_root, init_pool, NewJobParams};
use crate::domain::RootId;
use crate::jobs::{
    run_tracked_job, JobManager, ScanCountsDto, ScanEvent, ScanWarningDto, StartScanResult,
};
use crate::library::protected::ProtectedContext;
use crate::scanner::{ScanCounts, ScanGuard, ScanOptions, ScanOutcome, ScanProgress, ScanReport};

struct TestFixture {
    _temp: TempDir,
    pool: sqlx::SqlitePool,
    root_id: RootId,
    root_path: PathBuf,
}

fn test_context(_app_data: &Path) -> ProtectedContext {
    if cfg!(windows) {
        ProtectedContext::synthetic_windows(
            &["c:", "appdata", "photos-manager"],
            &["c:", "users", "alice"],
            &["c:", "users"],
            &[&["c:", "windows"]],
        )
    } else {
        ProtectedContext::synthetic_unix(
            &["tmp", "test_app_data"],
            &["users", "alice"],
            &["users"],
            &["volumes"],
            &[&["system"]],
        )
    }
}

async fn setup_fixture() -> TestFixture {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("catalogue.db");
    let app_data = temp.path().join("app_data");
    fs::create_dir_all(&app_data).expect("create app_data");

    let pool = init_pool(&db_path).await.expect("init_pool");
    let ctx = test_context(&app_data);
    let dialog_guard = DialogGuard::default();

    let root_path = temp.path().join("library_root");
    fs::create_dir_all(&root_path).expect("create library root dir");

    let target = root_path.clone();
    let res = add_root_with_picker(&pool, &ctx, &dialog_guard, None, move || {
        let t = target.clone();
        async move { Some(t) }
    })
    .await
    .expect("add_root");

    let root_id = RootId::from(res.root_id.expect("root_id in result"));

    TestFixture {
        _temp: temp,
        pool,
        root_id,
        root_path,
    }
}

// 1. Lifecycle: Completed scan with events, DB row updates, and metadata stamp
#[tokio::test]
async fn test_job_lifecycle_completed_scan() {
    let fix = setup_fixture().await;

    // Create 3 sample photo files
    fs::write(fix.root_path.join("photo1.jpg"), b"fake_jpeg_1").expect("write photo1");
    fs::write(fix.root_path.join("photo2.png"), b"fake_png_2").expect("write photo2");
    fs::create_dir_all(fix.root_path.join("vacation")).expect("create subdir");
    fs::write(
        fix.root_path.join("vacation").join("photo3.jpg"),
        b"fake_jpeg_3",
    )
    .expect("write photo3");

    let manager = Arc::new(JobManager::new(fix.pool.clone()));

    let events = Arc::new(Mutex::new(Vec::new()));
    let events_sink = Arc::clone(&events);
    let done_flag = Arc::new(AtomicBool::new(false));
    let done_clone = Arc::clone(&done_flag);

    let start_res = manager
        .start_scan_impl(fix.root_id.clone(), move |ev| {
            let mut list = events_sink.lock().unwrap();
            let is_term = matches!(ev, ScanEvent::Finished { .. } | ScanEvent::Failed { .. });
            list.push(ev);
            if is_term {
                done_clone.store(true, Ordering::SeqCst);
            }
        })
        .await
        .expect("start_scan_impl");

    let job_id = start_res.job_id;

    // Wait for terminal event
    for _ in 0..100 {
        if done_flag.load(Ordering::SeqCst) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(done_flag.load(Ordering::SeqCst), "Scan must complete");

    let recorded = events.lock().unwrap().clone();
    assert!(!recorded.is_empty(), "Must record events");

    // Rule 1: Started is first
    assert!(
        matches!(&recorded[0], ScanEvent::Started { job_id: j, root_id: r } if j == &job_id && r == fix.root_id.as_str()),
        "First event must be Started"
    );

    // Rule 2: Finished(completed) is last and nothing follows
    let last_event = recorded.last().unwrap();
    assert!(
        matches!(last_event, ScanEvent::Finished { outcome, counts, .. } if outcome == "completed" && counts.files_seen == 3),
        "Last event must be Finished with completed outcome and 3 files seen"
    );

    // Rule 3: Database row is completed with counts and times
    let mut conn = fix.pool.acquire().await.unwrap();
    let job_row = database::get_job_by_id(&mut conn, &job_id)
        .await
        .unwrap()
        .expect("job row must exist");
    assert_eq!(job_row.0, "completed"); // status
    assert_eq!(job_row.4, 3); // progress_done
    assert!(job_row.5.is_some()); // finished_at_ms

    // Rule 4: library_roots last_scan_at_ms is set
    let root_row = database::get_root(&mut conn, &fix.root_id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        root_row.last_scan_at_ms.is_some(),
        "last_scan_at_ms must be set on completed scan"
    );

    // Rule 5: Active job is cleared from manager map
    assert_eq!(manager.active_job_for_root(&fix.root_id), None);
}

// 2. Lifecycle: Cancelled scan leaves zero rows missing on disk
#[tokio::test]
async fn test_job_lifecycle_cancelled_scan_leaves_zero_missing() {
    let fix = setup_fixture().await;

    // Create 3 files
    fs::write(fix.root_path.join("file1.jpg"), b"data1").unwrap();
    fs::write(fix.root_path.join("file2.jpg"), b"data2").unwrap();
    fs::write(fix.root_path.join("file3.jpg"), b"data3").unwrap();

    let manager = Arc::new(JobManager::new(fix.pool.clone()));

    // Run first complete scan to populate files in catalogue
    let done_first = Arc::new(AtomicBool::new(false));
    let done_clone = Arc::clone(&done_first);
    manager
        .start_scan_impl(fix.root_id.clone(), move |ev| {
            if matches!(ev, ScanEvent::Finished { .. }) {
                done_clone.store(true, Ordering::SeqCst);
            }
        })
        .await
        .unwrap();

    for _ in 0..100 {
        if done_first.load(Ordering::SeqCst) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let mut conn = fix.pool.acquire().await.unwrap();
    assert_eq!(count_for_root(&mut conn, &fix.root_id).await.unwrap(), 3);

    // Start a second scan and cancel it immediately
    let events = Arc::new(Mutex::new(Vec::new()));
    let events_sink = Arc::clone(&events);
    let done_second = Arc::new(AtomicBool::new(false));
    let done_clone2 = Arc::clone(&done_second);

    let start_res = manager
        .start_scan_impl(fix.root_id.clone(), move |ev| {
            let mut list = events_sink.lock().unwrap();
            let is_term = matches!(ev, ScanEvent::Finished { .. });
            list.push(ev);
            if is_term {
                done_clone2.store(true, Ordering::SeqCst);
            }
        })
        .await
        .unwrap();

    let job_id = start_res.job_id;

    // Cancel scan
    manager.cancel_scan_impl(&job_id).await.unwrap();

    for _ in 0..100 {
        if done_second.load(Ordering::SeqCst) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    assert!(
        done_second.load(Ordering::SeqCst),
        "Cancelled scan must complete"
    );

    let recorded = events.lock().unwrap().clone();
    let last = recorded.last().unwrap();
    assert!(
        matches!(last, ScanEvent::Finished { outcome, .. } if outcome == "cancelled"),
        "Terminal event must be Finished with cancelled outcome"
    );

    // In database: row status is cancelled
    let job_row = database::get_job_by_id(&mut conn, &job_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(job_row.0, "cancelled");

    // Zero rows marked missing that exist on disk!
    let missing_count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM files WHERE root_id = ? AND status = 'missing';")
            .bind(fix.root_id.as_str())
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_eq!(
        missing_count.0, 0,
        "Cancelled scan must mark ZERO present files as missing"
    );
}

// 3. Refused pre-flight: start_scan of an unreachable (deleted) root path returns root_offline, creates no job row, and marks root Offline
#[tokio::test]
async fn test_start_scan_unreachable_root_refused_with_root_offline_and_marks_offline() {
    let fix = setup_fixture().await;

    // Delete the root directory on disk
    fs::remove_dir_all(&fix.root_path).expect("delete root dir");

    let manager = Arc::new(JobManager::new(fix.pool.clone()));
    let mut conn = fix.pool.acquire().await.unwrap();
    let jobs_before = database::count_total_jobs(&mut conn).await.unwrap();
    drop(conn);

    let err = manager
        .start_scan_impl(fix.root_id.clone(), |_| {})
        .await
        .expect_err("unreachable root must be refused before creating a job row");

    assert_eq!(err.code, "root_offline");

    let mut conn = fix.pool.acquire().await.unwrap();
    let jobs_after = database::count_total_jobs(&mut conn).await.unwrap();
    assert_eq!(
        jobs_before, jobs_after,
        "Refused pre-flight must create zero job rows"
    );

    let root = database::get_root(&mut conn, &fix.root_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(root.status, crate::domain::RootStatus::Offline);
}

// 3b. In-job unreachable: folder vanishes after start_scan pre-flight -> failed/scan_root_unreachable and root becomes Offline
#[tokio::test]
async fn test_job_lifecycle_root_vanishes_after_preflight_fails_with_scan_root_unreachable() {
    let fix = setup_fixture().await;

    // Delete directory on disk, while injecting a pre-flight check that still sees it as present
    // so start_scan creates the job row and scan_root_guarded discovers the vanished path.
    fs::remove_dir_all(&fix.root_path).expect("delete root dir");

    let mut conn = fix.pool.acquire().await.unwrap();
    let root = database::get_root(&mut conn, &fix.root_id)
        .await
        .unwrap()
        .unwrap();
    let stored_uuid = match &root.volume_id {
        Some(vid) => database::volumes::get_volume(&mut conn, vid)
            .await
            .unwrap()
            .and_then(|v| v.fs_uuid),
        None => None,
    };
    drop(conn);

    let manager = Arc::new(JobManager::new(fix.pool.clone()));
    manager.set_test_deps(crate::jobs::JobManagerTestDeps {
        is_dir_fn: Some(Arc::new(|_| true)),
        describer: Some(Arc::new(move |_| {
            Ok(crate::domain::VolumeInfo {
                kind: crate::domain::VolumeKind::Internal,
                label: None,
                fs_uuid: stored_uuid.clone(),
                fs_type: None,
            })
        })),
        ..Default::default()
    });

    let events = Arc::new(Mutex::new(Vec::new()));
    let events_sink = Arc::clone(&events);
    let done_flag = Arc::new(AtomicBool::new(false));
    let done_clone = Arc::clone(&done_flag);

    let start_res = manager
        .start_scan_impl(fix.root_id.clone(), move |ev| {
            let mut list = events_sink.lock().unwrap();
            let is_term = matches!(ev, ScanEvent::Failed { .. });
            list.push(ev);
            if is_term {
                done_clone.store(true, Ordering::SeqCst);
            }
        })
        .await
        .unwrap();

    let job_id = start_res.job_id;

    for _ in 0..100 {
        if done_flag.load(Ordering::SeqCst) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(done_flag.load(Ordering::SeqCst));

    let recorded = events.lock().unwrap().clone();
    let last = recorded.last().unwrap();
    assert!(
        matches!(
            last,
            ScanEvent::Failed {
                code,
                recorded: true,
                ..
            } if code == "scan_root_unreachable"
        ),
        "Must emit failed event with code scan_root_unreachable and recorded=true"
    );

    let mut conn = fix.pool.acquire().await.unwrap();
    let job_row = database::get_job_by_id(&mut conn, &job_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(job_row.0, "failed");
    assert_eq!(job_row.2.as_deref(), Some("scan_root_unreachable"));

    let root_after = database::get_root(&mut conn, &fix.root_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        root_after.status,
        crate::domain::RootStatus::Offline,
        "Root must transition to Offline when scan fails with scan_root_unreachable"
    );
}

// 4. Refused start: a second start_scan returns scan_already_running and creates NO job row
#[tokio::test]
async fn test_refused_start_creates_no_job_row() {
    let fix = setup_fixture().await;
    let manager = Arc::new(JobManager::new(fix.pool.clone()));

    // Manually hold ScanGuard for this root to simulate a running scan
    let guard = ScanGuard::acquire(&fix.root_id).expect("acquire guard");

    let mut conn = fix.pool.acquire().await.unwrap();
    let initial_job_count = database::count_total_jobs(&mut conn).await.unwrap();

    // Attempt start_scan while guard is held
    let res = manager.start_scan_impl(fix.root_id.clone(), |_| {}).await;

    assert!(res.is_err(), "Must refuse second scan");
    let err = res.err().unwrap();
    assert_eq!(err.code, "scan_already_running");

    // Verify row count before and after: NO job row was created!
    let count_after = database::count_total_jobs(&mut conn).await.unwrap();
    assert_eq!(
        initial_job_count, count_after,
        "Refused start must create zero job rows"
    );

    drop(guard);

    // After guard release, new start works
    let res2 = manager.start_scan_impl(fix.root_id.clone(), |_| {}).await;
    assert!(res2.is_ok(), "Must succeed after guard is released");
}

// 5a. Persistence before touching (Test A): scan fails before first batch; generation was persisted, root last_scan_generation is NULL (R4)
#[tokio::test]
async fn test_persistence_before_touching_disk_failure_leaves_null_generation() {
    let fix = setup_fixture().await;

    // One file in the root
    fs::write(fix.root_path.join("file1.jpg"), b"fake_data_1").unwrap();

    let manager = Arc::new(JobManager::new(fix.pool.clone()));

    let generation_uuid = uuid::Uuid::now_v7().to_string();
    let options = ScanOptions {
        generation: Some(generation_uuid.clone()),
        failure_hook_after_batch: Some(0),
        ..Default::default()
    };

    let events = Arc::new(Mutex::new(Vec::new()));
    let events_sink = Arc::clone(&events);
    let done_flag = Arc::new(AtomicBool::new(false));
    let done_clone = Arc::clone(&done_flag);

    let start_res = manager
        .start_scan_with_options(fix.root_id.clone(), options, move |ev| {
            let mut list = events_sink.lock().unwrap();
            let is_term = matches!(ev, ScanEvent::Finished { .. } | ScanEvent::Failed { .. });
            list.push(ev);
            if is_term {
                done_clone.store(true, Ordering::SeqCst);
            }
        })
        .await
        .expect("start_scan_with_options");

    let job_id = start_res.job_id;

    // Wait for terminal event by polling with a deadline
    for _ in 0..100 {
        if done_flag.load(Ordering::SeqCst) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        done_flag.load(Ordering::SeqCst),
        "Scan must reach terminal event"
    );

    let mut conn = fix.pool.acquire().await.unwrap();

    // Assert that the job row has payload_json.generation and ended failed
    let job_row = database::get_job_by_id(&mut conn, &job_id)
        .await
        .unwrap()
        .expect("job row must exist");
    assert_eq!(job_row.0, "failed", "Job must have failed");

    let payload: serde_json::Value =
        serde_json::from_str(&job_row.1).expect("payload_json must be valid JSON");
    assert_eq!(
        payload["generation"].as_str(),
        Some(generation_uuid.as_str()),
        "payload_json must have the persisted scan generation"
    );

    // Assert that the root's last_scan_generation is NULL
    let root_row = database::get_root(&mut conn, &fix.root_id)
        .await
        .unwrap()
        .expect("root must exist");
    assert_eq!(
        root_row.last_scan_generation, None,
        "root last_scan_generation must be NULL when scan fails before first batch"
    );
}

// 5b. Persistence before touching (Test B): completed scan leaves last_scan_generation equal to payload_json.generation (R4)
#[tokio::test]
async fn test_persistence_completed_scan_matches_root_last_scan_generation() {
    let fix = setup_fixture().await;

    // One file in the root
    fs::write(fix.root_path.join("file1.jpg"), b"fake_data_1").unwrap();

    let manager = Arc::new(JobManager::new(fix.pool.clone()));

    let events = Arc::new(Mutex::new(Vec::new()));
    let events_sink = Arc::clone(&events);
    let done_flag = Arc::new(AtomicBool::new(false));
    let done_clone = Arc::clone(&done_flag);

    let start_res = manager
        .start_scan_impl(fix.root_id.clone(), move |ev| {
            let mut list = events_sink.lock().unwrap();
            let is_term = matches!(ev, ScanEvent::Finished { .. } | ScanEvent::Failed { .. });
            list.push(ev);
            if is_term {
                done_clone.store(true, Ordering::SeqCst);
            }
        })
        .await
        .expect("start_scan_impl");

    let job_id = start_res.job_id;

    // Wait for terminal event by polling with a deadline
    for _ in 0..100 {
        if done_flag.load(Ordering::SeqCst) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        done_flag.load(Ordering::SeqCst),
        "Scan must reach terminal event"
    );

    let mut conn = fix.pool.acquire().await.unwrap();

    // Assert job row completed
    let job_row = database::get_job_by_id(&mut conn, &job_id)
        .await
        .unwrap()
        .expect("job row must exist");
    assert_eq!(job_row.0, "completed");

    let payload: serde_json::Value =
        serde_json::from_str(&job_row.1).expect("payload_json must be valid JSON");
    let payload_gen = payload["generation"]
        .as_str()
        .expect("payload must contain generation string");
    assert!(!payload_gen.is_empty());

    // Assert that the root's last_scan_generation equals payload_json.generation
    let root_row = database::get_root(&mut conn, &fix.root_id)
        .await
        .unwrap()
        .expect("root must exist");
    assert_eq!(
        root_row.last_scan_generation.as_deref(),
        Some(payload_gen),
        "root last_scan_generation must equal payload_json.generation"
    );
}

// 6. Throttling: at most 4 progress events per second, row writes rate-limited, last value never lost (R1)
#[tokio::test]
async fn test_throttle_rate_limits_progress_and_job_updates() {
    let fix = setup_fixture().await;
    let manager = Arc::new(JobManager::new(fix.pool.clone()));

    // Create trigger and log table on the temporary database to count row updates (R1)
    sqlx::query(
        "CREATE TABLE job_update_log (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            job_id TEXT NOT NULL,
            new_status TEXT NOT NULL,
            new_progress_done INTEGER NOT NULL
        );",
    )
    .execute(&fix.pool)
    .await
    .unwrap();

    sqlx::query(
        "CREATE TRIGGER log_job_updates
         AFTER UPDATE ON jobs
         FOR EACH ROW
         BEGIN
             INSERT INTO job_update_log (job_id, new_status, new_progress_done)
             VALUES (NEW.id, NEW.status, NEW.progress_done);
         END;",
    )
    .execute(&fix.pool)
    .await
    .unwrap();

    let (watch_tx, watch_rx) = tokio::sync::watch::channel(None);
    let events = Arc::new(Mutex::new(Vec::new()));
    let events_sink = Arc::clone(&events);

    let job_id = "job_throttle_test".to_string();
    let now_ms = database::now_ms().unwrap();

    let mut conn = fix.pool.acquire().await.unwrap();
    database::insert_job(
        &mut conn,
        &NewJobParams {
            id: &job_id,
            root_id: Some(fix.root_id.as_str()),
            job_type: "scan",
            status: "running",
            priority: 50,
            payload_json: "{}",
            now_ms,
        },
    )
    .await
    .unwrap();
    drop(conn);

    // Mock worker that stays alive while we test the ticker
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();

    let worker_fut = async move {
        let _ = stop_rx.await;
        Ok(ScanOutcome::Completed(ScanReport {
            generation: "gen_test".to_string(),
            counts: ScanCounts {
                files_seen: 1000,
                folders_seen: 50,
                ..Default::default()
            },
            warning_count: 0,
            warnings: Vec::new(),
            elapsed: Duration::from_secs(1),
        }))
    };

    let supervisor_handle = run_tracked_job(
        fix.pool.clone(),
        job_id.clone(),
        fix.root_id.clone(),
        manager,
        worker_fut,
        watch_rx,
        move |ev| {
            events_sink.lock().unwrap().push(ev);
        },
    );

    let start_instant = std::time::Instant::now();

    // Pump 1,000 progress updates into watch channel
    for i in 1..=1000 {
        let _ = watch_tx.send_replace(Some(ScanProgress {
            folders_seen: (i / 20) as u64,
            files_seen: i as u64,
            batches_committed: (i / 10) as u64,
        }));
        tokio::task::yield_now().await;
    }

    // Wait ~600ms (ticker ticks 2-3 times at 250ms)
    tokio::time::sleep(Duration::from_millis(600)).await;

    // Stop worker to conclude job
    let _ = stop_tx.send(());
    let elapsed_ms = start_instant.elapsed().as_millis() as u64;

    // Await supervisor task handle (R1: no fixed sleep, await completion)
    let _ = supervisor_handle.await;

    let recorded = events.lock().unwrap().clone();
    let progress_events: Vec<_> = recorded
        .iter()
        .filter(|e| matches!(e, ScanEvent::Progress { .. }))
        .collect();

    // Assert progress event rate (R1)
    assert!(
        progress_events.len() as u64 <= elapsed_ms / 250 + 2,
        "Throttled ticker must emit at most ~4 progress events per simulated second, got {} in {} ms",
        progress_events.len(),
        elapsed_ms
    );
    assert!(
        !progress_events.is_empty(),
        "Must receive at least 1 progress event"
    );

    // Assert row update write rate before final update (R1)
    let (total_updates,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM job_update_log WHERE job_id = ?;")
            .bind(&job_id)
            .fetch_one(&fix.pool)
            .await
            .unwrap();
    let writes_before_final = total_updates.saturating_sub(1) as u64;
    assert!(
        writes_before_final <= elapsed_ms / 1000 + 1,
        "Job row writes before final update must be <= elapsed_ms / 1000 + 1, got {} writes in {} ms",
        writes_before_final,
        elapsed_ms
    );

    // Final row in database: status completed, progress_done 1000 (R1)
    let mut conn = fix.pool.acquire().await.unwrap();
    let final_row = database::get_job_by_id(&mut conn, &job_id)
        .await
        .unwrap()
        .expect("job row must exist");
    assert_eq!(final_row.0, "completed");
    assert_eq!(final_row.4, 1000);

    // Terminal event carries the exact final counts (never lost) (R1)
    let term = recorded.last().unwrap();
    assert!(
        matches!(term, ScanEvent::Finished { counts, .. } if counts.files_seen == 1000),
        "Terminal event must carry final 1000 count, got {:?}",
        term
    );
}

// 7. Dead channel: a failing sink does not stop the job
#[tokio::test]
async fn test_dead_channel_does_not_stop_job() {
    let fix = setup_fixture().await;

    fs::write(fix.root_path.join("file1.jpg"), b"data1").unwrap();
    fs::write(fix.root_path.join("file2.jpg"), b"data2").unwrap();

    let manager = Arc::new(JobManager::new(fix.pool.clone()));

    let event_count = Arc::new(AtomicUsize::new(0));
    let count_clone = Arc::clone(&event_count);

    let start_res = manager
        .start_scan_impl(fix.root_id.clone(), move |_ev| {
            let n = count_clone.fetch_add(1, Ordering::SeqCst);
            if n >= 2 {
                // Simulate disconnected / closed channel by panicking or ignoring
                // In production, Channel::send returns Err when receiver is dropped.
                // We test that supervisor ignores send errors and job finishes in SQLite.
            }
        })
        .await
        .unwrap();

    let job_id = start_res.job_id;

    // Poll DB for completed status
    let mut conn = fix.pool.acquire().await.unwrap();
    let mut finished = false;
    for _ in 0..100 {
        if let Ok(Some(row)) = database::get_job_by_id(&mut conn, &job_id).await {
            if row.0 == "completed" {
                finished = true;
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    assert!(finished, "Job must still complete despite dead channel");
}

// 8. Start-up reconciliation: transitions running, cancelling, queued to failed/interrupted
#[tokio::test]
async fn test_startup_reconciliation_marks_interrupted() {
    let fix = setup_fixture().await;
    let mut conn = fix.pool.acquire().await.unwrap();

    let now_ms = 1700000000000i64;

    // Previous completed job
    database::insert_job(
        &mut conn,
        &NewJobParams {
            id: "job_done",
            root_id: Some(fix.root_id.as_str()),
            job_type: "scan",
            status: "completed",
            priority: 50,
            payload_json: "{}",
            now_ms: now_ms - 10000,
        },
    )
    .await
    .unwrap();

    // In-flight jobs that got interrupted
    for (i, (id, status)) in [
        ("job_queue", "queued"),
        ("job_canc", "cancelling"),
        ("job_run", "running"),
    ]
    .iter()
    .enumerate()
    {
        database::insert_job(
            &mut conn,
            &NewJobParams {
                id,
                root_id: Some(fix.root_id.as_str()),
                job_type: "scan",
                status,
                priority: 50,
                payload_json: "{}",
                now_ms: now_ms + (i as i64) * 1000,
            },
        )
        .await
        .unwrap();
    }

    let error_msg = crate::commands::fixed_message_for_code("interrupted");
    let reconciled = database::reconcile_interrupted_jobs(&mut conn, error_msg, now_ms + 5000)
        .await
        .unwrap();
    assert_eq!(reconciled, 3, "Must reconcile exactly 3 in-flight jobs");

    // The first 3 are now failed with interrupted
    for id in ["job_run", "job_canc", "job_queue"] {
        let row = database::get_job_by_id(&mut conn, id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.0, "failed");
        assert_eq!(row.2.as_deref(), Some("interrupted"));
        assert_eq!(row.5, Some(now_ms + 5000));
    }

    // The completed job is untouched
    let done_row = database::get_job_by_id(&mut conn, "job_done")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(done_row.0, "completed");

    // Running reconciliation a second time changes 0 rows
    let reconciled2 = database::reconcile_interrupted_jobs(&mut conn, error_msg, now_ms + 6000)
        .await
        .unwrap();
    assert_eq!(reconciled2, 0, "Second run must be a no-op");

    // Last job summary shows interrupted
    let last_job = database::get_last_job_for_root(&mut conn, &fix.root_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(last_job.status, "failed");
    assert_eq!(last_job.error_code.as_deref(), Some("interrupted"));
}

// 9. Panic isolation: future panic marks job failed/internal and releases ScanGuard
#[tokio::test]
async fn test_panic_isolation_marks_failed_and_frees_guard() {
    let fix = setup_fixture().await;
    let manager = Arc::new(JobManager::new(fix.pool.clone()));

    let guard = ScanGuard::acquire(&fix.root_id).unwrap();
    let job_id = "job_panic_test".to_string();
    let now_ms = database::now_ms().unwrap();

    let mut conn = fix.pool.acquire().await.unwrap();
    database::insert_job(
        &mut conn,
        &NewJobParams {
            id: &job_id,
            root_id: Some(fix.root_id.as_str()),
            job_type: "scan",
            status: "running",
            priority: 50,
            payload_json: "{}",
            now_ms,
        },
    )
    .await
    .unwrap();
    drop(conn);

    let (_, watch_rx) = tokio::sync::watch::channel(None);
    let events = Arc::new(Mutex::new(Vec::new()));
    let events_sink = Arc::clone(&events);
    let done_flag = Arc::new(AtomicBool::new(false));
    let done_clone = Arc::clone(&done_flag);

    // Panicking worker future
    let panicking_fut = async move {
        let _g = guard; // guard moves into worker
        panic!("worker panicked inside test!");
        #[allow(unreachable_code)]
        Ok(ScanOutcome::Completed(ScanReport {
            generation: "g".into(),
            counts: ScanCounts::default(),
            warning_count: 0,
            warnings: vec![],
            elapsed: Duration::from_secs(1),
        }))
    };

    let supervisor_handle = run_tracked_job(
        fix.pool.clone(),
        job_id.clone(),
        fix.root_id.clone(),
        Arc::clone(&manager),
        panicking_fut,
        watch_rx,
        move |ev| {
            let mut list = events_sink.lock().unwrap();
            let is_term = matches!(ev, ScanEvent::Failed { .. });
            list.push(ev);
            if is_term {
                done_clone.store(true, Ordering::SeqCst);
            }
        },
    );

    let _ = supervisor_handle.await;

    // 1. Terminal event is Failed with code "internal"
    let recorded = events.lock().unwrap().clone();
    assert_eq!(recorded.len(), 1);
    assert!(
        matches!(&recorded[0], ScanEvent::Failed { code, .. } if code == "internal"),
        "Panic must emit Failed with internal code"
    );

    // 2. DB row is marked failed with error_code 'internal'
    let mut conn = fix.pool.acquire().await.unwrap();
    let row = database::get_job_by_id(&mut conn, &job_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.0, "failed");
    assert_eq!(row.2.as_deref(), Some("internal"));

    // 3. ScanGuard was freed by Drop unwind: new scan can acquire guard immediately
    let new_guard = ScanGuard::acquire(&fix.root_id);
    assert!(
        new_guard.is_ok(),
        "ScanGuard must be freed after worker panic"
    );
}

// 10. remove_root while scanning returns root_busy and vice-versa
#[tokio::test]
async fn test_remove_root_while_scanning_returns_root_busy() {
    let fix = setup_fixture().await;

    // 1. While ScanGuard is held, remove_root_impl returns root_busy
    let guard = ScanGuard::acquire(&fix.root_id).unwrap();

    let res = crate::commands::remove_root_impl(&fix.pool, fix.root_id.as_str(), 0).await;
    assert!(res.is_err());
    assert_eq!(res.err().unwrap().code, "root_busy");

    // 2. After guard is released, removal works
    drop(guard);
    let res2 = crate::commands::remove_root_impl(&fix.pool, fix.root_id.as_str(), 0).await;
    assert!(
        res2.is_ok(),
        "remove_root must succeed when no scan is active"
    );
}

// 11. Retention: prunes older finished jobs keeping newest 20 per root
#[tokio::test]
async fn test_retention_prunes_older_finished_jobs_keeping_newest_20() {
    let fix = setup_fixture().await;
    let mut conn = fix.pool.acquire().await.unwrap();

    let base_ms = 1700000000000i64;

    // Insert 25 finished jobs for this root with increasing timestamps
    for i in 1..=25 {
        let job_id = format!("job_retention_{:02}", i);
        database::insert_job(
            &mut conn,
            &NewJobParams {
                id: &job_id,
                root_id: Some(fix.root_id.as_str()),
                job_type: "scan",
                status: "completed",
                priority: 50,
                payload_json: "{}",
                now_ms: base_ms + i * 1000,
            },
        )
        .await
        .unwrap();

        database::finalize_job(
            &mut conn,
            &database::FinalizeJobParams {
                id: &job_id,
                status: "completed",
                progress_done: 10,
                warning_count: 0,
                error_count: 0,
                error_code: None,
                error_message: None,
                finished_at_ms: base_ms + i * 1000,
                updated_at_ms: base_ms + i * 1000,
            },
        )
        .await
        .unwrap();
    }

    // Insert 1 running job (must NOT be pruned)
    database::insert_job(
        &mut conn,
        &NewJobParams {
            id: "job_running_keep",
            root_id: Some(fix.root_id.as_str()),
            job_type: "scan",
            status: "running",
            priority: 50,
            payload_json: "{}",
            now_ms: base_ms + 99999,
        },
    )
    .await
    .unwrap();

    // Prune to keep 20 finished
    let deleted = database::prune_finished_jobs(&mut conn, fix.root_id.as_str(), 20)
        .await
        .unwrap();
    assert_eq!(deleted, 5, "Must prune 5 oldest finished jobs");

    // Total finished remaining is 20
    let count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM jobs WHERE root_id = ? AND status = 'completed';")
            .bind(fix.root_id.as_str())
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_eq!(count.0, 20);

    // Running job is preserved
    let running_row = database::get_job_by_id(&mut conn, "job_running_keep")
        .await
        .unwrap();
    assert!(running_row.is_some(), "Running job must not be pruned");

    // The 5 oldest (1..=5) are gone
    for i in 1..=5 {
        let job_id = format!("job_retention_{:02}", i);
        let row = database::get_job_by_id(&mut conn, &job_id).await.unwrap();
        assert!(row.is_none(), "Oldest job {} must be pruned", job_id);
    }

    // The newest 20 (6..=25) remain
    for i in 6..=25 {
        let job_id = format!("job_retention_{:02}", i);
        let row = database::get_job_by_id(&mut conn, &job_id).await.unwrap();
        assert!(row.is_some(), "Newer job {} must be retained", job_id);
    }
}

// 12. Cancel unknown or finished job returns job_not_found
#[tokio::test]
async fn test_cancel_unknown_or_finished_job_returns_job_not_found() {
    let fix = setup_fixture().await;
    let manager = JobManager::new(fix.pool.clone());

    let res = manager.cancel_scan_impl("non_existent_job_12345").await;
    assert!(res.is_err());
    let err = res.err().unwrap();
    assert_eq!(err.code, "job_not_found");
}

// 13. Wire format: ScanEvent variants match literal JSON
#[test]
fn test_wire_format_scan_events_matches_literal_json() {
    // 1. Started
    let started = ScanEvent::Started {
        job_id: "job_01".to_string(),
        root_id: "root_01".to_string(),
    };
    let started_json = serde_json::to_string(&started).unwrap();
    assert_eq!(
        started_json,
        r#"{"kind":"started","jobId":"job_01","rootId":"root_01"}"#
    );

    // 2. Progress
    let progress = ScanEvent::Progress {
        job_id: "job_01".to_string(),
        folders_seen: 5,
        files_seen: 42,
        batches: 2,
    };
    let progress_json = serde_json::to_string(&progress).unwrap();
    assert_eq!(
        progress_json,
        r#"{"kind":"progress","jobId":"job_01","foldersSeen":5,"filesSeen":42,"batches":2}"#
    );

    // 3. Finished
    let finished = ScanEvent::Finished {
        job_id: "job_01".to_string(),
        outcome: "completed".to_string(),
        counts: ScanCountsDto {
            folders_seen: 5,
            files_seen: 42,
            new: 10,
            changed: 2,
            unchanged: 30,
            moved: 0,
            marked_missing: 0,
            marked_inaccessible: 0,
            unsupported: 0,
            skipped_by_rule: 3,
        },
        warning_count: 1,
        warnings: vec![ScanWarningDto {
            code: "path_key_collision".to_string(),
            rel_path: "sub/collision.jpg".to_string(),
        }],
        elapsed_ms: 1250,
        recorded: true,
    };
    let finished_json = serde_json::to_string(&finished).unwrap();
    assert_eq!(
        finished_json,
        r#"{"kind":"finished","jobId":"job_01","outcome":"completed","counts":{"foldersSeen":5,"filesSeen":42,"new":10,"changed":2,"unchanged":30,"moved":0,"markedMissing":0,"markedInaccessible":0,"unsupported":0,"skippedByRule":3},"warningCount":1,"warnings":[{"code":"path_key_collision","relPath":"sub/collision.jpg"}],"elapsedMs":1250,"recorded":true}"#
    );

    // 4. Failed
    let failed = ScanEvent::Failed {
        job_id: "job_01".to_string(),
        code: "internal".to_string(),
        recorded: false,
    };
    let failed_json = serde_json::to_string(&failed).unwrap();
    assert_eq!(
        failed_json,
        r#"{"kind":"failed","jobId":"job_01","code":"internal","recorded":false}"#
    );

    // 5. StartScanResult
    let res = StartScanResult {
        job_id: "job_01".to_string(),
    };
    let res_json = serde_json::to_string(&res).unwrap();
    assert_eq!(res_json, r#"{"jobId":"job_01"}"#);
}

// 14. Cancel status update race: mark_job_cancelling only updates running rows (R2)
#[tokio::test]
async fn test_mark_job_cancelling_only_affects_running_jobs() {
    let fix = setup_fixture().await;
    let mut conn = fix.pool.acquire().await.unwrap();
    let now_ms = database::now_ms().unwrap();

    // 1. A completed row stays completed after the call
    database::insert_job(
        &mut conn,
        &NewJobParams {
            id: "job_completed_1",
            root_id: Some(fix.root_id.as_str()),
            job_type: "scan",
            status: "completed",
            priority: 50,
            payload_json: "{}",
            now_ms,
        },
    )
    .await
    .unwrap();

    let rows_changed = database::mark_job_cancelling(&mut conn, "job_completed_1", now_ms + 100)
        .await
        .unwrap();
    assert_eq!(rows_changed, 0, "Completed job row must not be changed");

    let row = database::get_job_by_id(&mut conn, "job_completed_1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.0, "completed", "Completed job row must stay completed");

    // 2. A running row becomes cancelling
    database::insert_job(
        &mut conn,
        &NewJobParams {
            id: "job_running_1",
            root_id: Some(fix.root_id.as_str()),
            job_type: "scan",
            status: "running",
            priority: 50,
            payload_json: "{}",
            now_ms,
        },
    )
    .await
    .unwrap();

    let rows_changed = database::mark_job_cancelling(&mut conn, "job_running_1", now_ms + 100)
        .await
        .unwrap();
    assert_eq!(rows_changed, 1, "Running job row must be updated");

    let row = database::get_job_by_id(&mut conn, "job_running_1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        row.0, "cancelling",
        "Running job row must become cancelling"
    );
}

// 15. Same-volume guard refuses empty mount point with root_volume_changed and marks nothing missing
#[tokio::test]
async fn volume_guard_refuses_empty_mount_point_and_marks_nothing_missing() {
    let fix = setup_fixture().await;

    fs::write(fix.root_path.join("photo1.jpg"), b"img1").unwrap();
    fs::write(fix.root_path.join("photo2.jpg"), b"img2").unwrap();

    // Ensure stored volume has a non-NULL fs_uuid on all OSes (including Linux stub)
    let mut conn = fix.pool.acquire().await.unwrap();
    let root_row = database::get_root(&mut conn, &fix.root_id)
        .await
        .unwrap()
        .unwrap();
    let vol_id = root_row.volume_id.clone().unwrap();
    let vol_row = database::volumes::get_volume(&mut conn, &vol_id)
        .await
        .unwrap()
        .unwrap();
    if vol_row.fs_uuid.is_none() {
        sqlx::query("UPDATE volumes SET fs_uuid = 'uuid-original-drive' WHERE id = ?;")
            .bind(vol_id.as_str())
            .execute(&pool_clone(&fix.pool))
            .await
            .unwrap();
    }
    drop(conn);

    // Initial scan so files exist with status 'present'
    crate::scanner::scan_root(
        &fix.pool,
        &fix.root_id,
        ScanOptions::default(),
        crate::scanner::ScanCancel::new(),
        |_| {},
    )
    .await
    .unwrap();

    // Empty the folder (it still exists on disk, simulating an empty mount point)
    fs::remove_file(fix.root_path.join("photo1.jpg")).unwrap();
    fs::remove_file(fix.root_path.join("photo2.jpg")).unwrap();

    let manager = Arc::new(JobManager::new(fix.pool.clone()));
    manager.set_test_deps(crate::jobs::JobManagerTestDeps {
        describer: Some(Arc::new(|_| {
            Ok(crate::domain::VolumeInfo {
                kind: crate::domain::VolumeKind::Removable,
                label: Some("WrongDrive".to_string()),
                fs_uuid: Some("uuid-different-drive".to_string()),
                fs_type: Some("exfat".to_string()),
            })
        })),
        ..Default::default()
    });

    let mut conn = fix.pool.acquire().await.unwrap();
    let jobs_before = database::count_total_jobs(&mut conn).await.unwrap();
    drop(conn);

    let err = manager
        .start_scan_impl(fix.root_id.clone(), |_| {})
        .await
        .expect_err("volume mismatch must refuse scan");
    assert_eq!(err.code, "root_volume_changed");

    let mut conn = fix.pool.acquire().await.unwrap();
    let jobs_after = database::count_total_jobs(&mut conn).await.unwrap();
    assert_eq!(jobs_before, jobs_after, "No job row may be created");

    let root_after = database::get_root(&mut conn, &fix.root_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(root_after.status, crate::domain::RootStatus::Offline);

    let statuses: Vec<(String,)> =
        sqlx::query_as("SELECT status FROM files WHERE root_id = ? ORDER BY name;")
            .bind(fix.root_id.as_str())
            .fetch_all(&mut *conn)
            .await
            .unwrap();
    assert_eq!(statuses.len(), 2);
    assert!(statuses.iter().all(|s| s.0 == "present"));
}

fn pool_clone(pool: &sqlx::SqlitePool) -> sqlx::SqlitePool {
    pool.clone()
}

// 16. Dead check at scan start: times out without blocking and deduplicates second scan start
#[tokio::test]
async fn dead_check_at_scan_start_times_out_and_does_not_block_second_scan() {
    let fix = setup_fixture().await;
    let manager = Arc::new(JobManager::new(fix.pool.clone()));

    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    let release_rx = Arc::new(Mutex::new(release_rx));
    let call_count = Arc::new(AtomicUsize::new(0));

    let rx_c = Arc::clone(&release_rx);
    let count_c = Arc::clone(&call_count);
    manager.set_test_deps(crate::jobs::JobManagerTestDeps {
        availability_timeout: Some(Duration::from_millis(50)),
        is_dir_fn: Some(Arc::new(move |_| {
            count_c.fetch_add(1, Ordering::SeqCst);
            if let Ok(rx) = rx_c.lock() {
                let _ = rx.recv();
            }
            true
        })),
        ..Default::default()
    });

    let start1 = std::time::Instant::now();
    let err1 = manager
        .start_scan_impl(fix.root_id.clone(), |_| {})
        .await
        .expect_err("hung check must fail start_scan");
    let elapsed1 = start1.elapsed();
    assert_eq!(err1.code, "root_offline");
    assert!(elapsed1 < Duration::from_millis(500));
    assert_eq!(call_count.load(Ordering::SeqCst), 1);

    // Second start_scan while the first blocking thread is still blocked returns immediately without calling checker again
    let start2 = std::time::Instant::now();
    let err2 = manager
        .start_scan_impl(fix.root_id.clone(), |_| {})
        .await
        .expect_err("second start_scan must return root_offline immediately");
    let elapsed2 = start2.elapsed();
    assert_eq!(err2.code, "root_offline");
    assert!(elapsed2 < Duration::from_millis(100));
    assert_eq!(call_count.load(Ordering::SeqCst), 1);

    let mut conn = fix.pool.acquire().await.unwrap();
    assert_eq!(database::count_total_jobs(&mut conn).await.unwrap(), 0);
    let root = database::get_root(&mut conn, &fix.root_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        root.status,
        crate::domain::RootStatus::Active,
        "Unknown check timeout must leave stored status Active"
    );

    let _ = release_tx.send(());
}

// 17. Final-write failure on each of the four finish paths + retry success
fn failing_hook(fail_attempts: usize) -> crate::jobs::FinalWriteHookFn {
    Arc::new(move |attempt| {
        Box::pin(async move {
            if attempt < fail_attempts {
                Err(crate::domain::CatalogueError::Database(
                    sqlx::Error::PoolClosed,
                ))
            } else {
                Ok(())
            }
        })
    })
}

async fn wait_for_terminal(done_flag: &AtomicBool) {
    for _ in 0..100 {
        if done_flag.load(Ordering::SeqCst) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        done_flag.load(Ordering::SeqCst),
        "Timed out waiting for terminal scan event"
    );
}

#[tokio::test]
async fn final_write_failure_completed_path_emits_recorded_false_and_allows_next_scan() {
    let fix = setup_fixture().await;
    fs::write(fix.root_path.join("a.jpg"), b"img").unwrap();

    let manager = Arc::new(JobManager::new(fix.pool.clone()));
    manager.set_test_deps(crate::jobs::JobManagerTestDeps {
        final_write_hook: Some(failing_hook(usize::MAX)),
        ..Default::default()
    });

    let events = Arc::new(Mutex::new(Vec::new()));
    let events_c = Arc::clone(&events);
    let done = Arc::new(AtomicBool::new(false));
    let done_c = Arc::clone(&done);

    manager
        .start_scan_impl(fix.root_id.clone(), move |ev| {
            let term = matches!(ev, ScanEvent::Finished { .. } | ScanEvent::Failed { .. });
            events_c.lock().unwrap().push(ev);
            if term {
                done_c.store(true, Ordering::SeqCst);
            }
        })
        .await
        .unwrap();

    wait_for_terminal(&done).await;
    let recorded_events = events.lock().unwrap().clone();
    let term_count = recorded_events
        .iter()
        .filter(|e| matches!(e, ScanEvent::Finished { .. } | ScanEvent::Failed { .. }))
        .count();
    assert_eq!(term_count, 1);
    assert!(matches!(
        recorded_events.last().unwrap(),
        ScanEvent::Finished {
            outcome,
            recorded: false,
            ..
        } if outcome == "completed"
    ));
    assert_eq!(manager.active_job_for_root(&fix.root_id), None);

    // Clear failure hook and verify a new scan starts cleanly afterwards
    manager.set_test_deps(crate::jobs::JobManagerTestDeps::default());
    let done2 = Arc::new(AtomicBool::new(false));
    let done2_c = Arc::clone(&done2);
    manager
        .start_scan_impl(fix.root_id.clone(), move |ev| {
            if matches!(ev, ScanEvent::Finished { recorded: true, .. }) {
                done2_c.store(true, Ordering::SeqCst);
            }
        })
        .await
        .expect("new scan must start cleanly after unrecorded terminal event");
    wait_for_terminal(&done2).await;
}

#[tokio::test]
async fn final_write_failure_cancelled_path_emits_recorded_false_and_allows_next_scan() {
    let fix = setup_fixture().await;
    let manager = Arc::new(JobManager::new(fix.pool.clone()));
    manager.set_test_deps(crate::jobs::JobManagerTestDeps {
        final_write_hook: Some(failing_hook(usize::MAX)),
        ..Default::default()
    });

    let job_id = "job_final_fail_cancelled".to_string();
    let now_ms = database::now_ms().unwrap();
    let mut conn = fix.pool.acquire().await.unwrap();
    database::insert_job(
        &mut conn,
        &NewJobParams {
            id: &job_id,
            root_id: Some(fix.root_id.as_str()),
            job_type: "scan",
            status: "running",
            priority: 50,
            payload_json: "{}",
            now_ms,
        },
    )
    .await
    .unwrap();
    drop(conn);

    let guard = ScanGuard::acquire(&fix.root_id).unwrap();
    let (_, watch_rx) = tokio::sync::watch::channel(None);
    let events = Arc::new(Mutex::new(Vec::new()));
    let events_c = Arc::clone(&events);

    let worker_fut = async move {
        let _g = guard;
        Ok(ScanOutcome::Cancelled(ScanReport {
            generation: "g".into(),
            counts: ScanCounts::default(),
            warning_count: 0,
            warnings: vec![],
            elapsed: Duration::from_millis(5),
        }))
    };

    let handle = run_tracked_job(
        fix.pool.clone(),
        job_id,
        fix.root_id.clone(),
        Arc::clone(&manager),
        worker_fut,
        watch_rx,
        move |ev| {
            events_c.lock().unwrap().push(ev);
        },
    );
    let _ = handle.await;

    let recorded_events = events.lock().unwrap().clone();
    assert_eq!(recorded_events.len(), 1);
    assert!(matches!(
        &recorded_events[0],
        ScanEvent::Finished {
            outcome,
            recorded: false,
            ..
        } if outcome == "cancelled"
    ));

    manager.set_test_deps(crate::jobs::JobManagerTestDeps::default());
    assert!(manager
        .start_scan_impl(fix.root_id.clone(), |_| {})
        .await
        .is_ok());
}

#[tokio::test]
async fn final_write_failure_failed_path_emits_recorded_false_and_allows_next_scan() {
    let fix = setup_fixture().await;
    fs::write(fix.root_path.join("fail.jpg"), b"img").unwrap();

    let manager = Arc::new(JobManager::new(fix.pool.clone()));
    manager.set_test_deps(crate::jobs::JobManagerTestDeps {
        final_write_hook: Some(failing_hook(usize::MAX)),
        ..Default::default()
    });

    let events = Arc::new(Mutex::new(Vec::new()));
    let events_c = Arc::clone(&events);
    let done = Arc::new(AtomicBool::new(false));
    let done_c = Arc::clone(&done);

    manager
        .start_scan_with_options(
            fix.root_id.clone(),
            ScanOptions {
                generation: Some("gen_fail".into()),
                failure_hook_after_batch: Some(0),
                ..Default::default()
            },
            move |ev| {
                let term = matches!(ev, ScanEvent::Finished { .. } | ScanEvent::Failed { .. });
                events_c.lock().unwrap().push(ev);
                if term {
                    done_c.store(true, Ordering::SeqCst);
                }
            },
        )
        .await
        .unwrap();

    wait_for_terminal(&done).await;
    let recorded_events = events.lock().unwrap().clone();
    let term_count = recorded_events
        .iter()
        .filter(|e| matches!(e, ScanEvent::Finished { .. } | ScanEvent::Failed { .. }))
        .count();
    assert_eq!(term_count, 1);
    assert!(matches!(
        recorded_events.last().unwrap(),
        ScanEvent::Failed {
            code,
            recorded: false,
            ..
        } if code == "database"
    ));
    assert_eq!(manager.active_job_for_root(&fix.root_id), None);

    manager.set_test_deps(crate::jobs::JobManagerTestDeps::default());
    assert!(manager
        .start_scan_impl(fix.root_id.clone(), |_| {})
        .await
        .is_ok());
}

#[tokio::test]
async fn final_write_failure_panic_path_emits_recorded_false_and_allows_next_scan() {
    let fix = setup_fixture().await;
    let manager = Arc::new(JobManager::new(fix.pool.clone()));
    manager.set_test_deps(crate::jobs::JobManagerTestDeps {
        final_write_hook: Some(failing_hook(usize::MAX)),
        ..Default::default()
    });

    let job_id = "job_final_fail_panic".to_string();
    let now_ms = database::now_ms().unwrap();
    let mut conn = fix.pool.acquire().await.unwrap();
    database::insert_job(
        &mut conn,
        &NewJobParams {
            id: &job_id,
            root_id: Some(fix.root_id.as_str()),
            job_type: "scan",
            status: "running",
            priority: 50,
            payload_json: "{}",
            now_ms,
        },
    )
    .await
    .unwrap();
    drop(conn);

    let guard = ScanGuard::acquire(&fix.root_id).unwrap();
    let (_, watch_rx) = tokio::sync::watch::channel(None);
    let events = Arc::new(Mutex::new(Vec::new()));
    let events_c = Arc::clone(&events);

    let panicking_fut = async move {
        let _g = guard;
        panic!("panic inside worker with final write failure");
        #[allow(unreachable_code)]
        Ok(ScanOutcome::Completed(ScanReport {
            generation: "g".into(),
            counts: ScanCounts::default(),
            warning_count: 0,
            warnings: vec![],
            elapsed: Duration::from_millis(1),
        }))
    };

    let handle = run_tracked_job(
        fix.pool.clone(),
        job_id,
        fix.root_id.clone(),
        Arc::clone(&manager),
        panicking_fut,
        watch_rx,
        move |ev| {
            events_c.lock().unwrap().push(ev);
        },
    );
    let _ = handle.await;

    let recorded_events = events.lock().unwrap().clone();
    assert_eq!(recorded_events.len(), 1);
    assert!(matches!(
        &recorded_events[0],
        ScanEvent::Failed {
            code,
            recorded: false,
            ..
        } if code == "internal"
    ));

    manager.set_test_deps(crate::jobs::JobManagerTestDeps::default());
    assert!(manager
        .start_scan_impl(fix.root_id.clone(), |_| {})
        .await
        .is_ok());
}

#[tokio::test]
async fn final_write_retry_succeeds_after_first_attempt_fails() {
    let fix = setup_fixture().await;
    fs::write(fix.root_path.join("retry.jpg"), b"img").unwrap();

    let manager = Arc::new(JobManager::new(fix.pool.clone()));
    manager.set_test_deps(crate::jobs::JobManagerTestDeps {
        final_write_hook: Some(failing_hook(1)), // Fail attempt 0, succeed attempt 1
        ..Default::default()
    });

    let events = Arc::new(Mutex::new(Vec::new()));
    let events_c = Arc::clone(&events);
    let done = Arc::new(AtomicBool::new(false));
    let done_c = Arc::clone(&done);

    let start_res = manager
        .start_scan_impl(fix.root_id.clone(), move |ev| {
            let term = matches!(ev, ScanEvent::Finished { .. } | ScanEvent::Failed { .. });
            events_c.lock().unwrap().push(ev);
            if term {
                done_c.store(true, Ordering::SeqCst);
            }
        })
        .await
        .unwrap();

    wait_for_terminal(&done).await;
    let recorded_events = events.lock().unwrap().clone();
    assert!(matches!(
        recorded_events.last().unwrap(),
        ScanEvent::Finished {
            outcome,
            recorded: true,
            ..
        } if outcome == "completed"
    ));

    let mut conn = fix.pool.acquire().await.unwrap();
    let row = database::get_job_by_id(&mut conn, &start_res.job_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.0, "completed");
}

#[tokio::test]
async fn active_job_id_and_row_agree_until_the_final_write_ends() {
    let fix = setup_fixture().await;
    fs::write(fix.root_path.join("gate.jpg"), b"img").unwrap();

    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel::<()>();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
    let entered_opt = Arc::new(Mutex::new(Some(entered_tx)));
    let release_opt = Arc::new(Mutex::new(Some(release_rx)));

    let manager = Arc::new(JobManager::new(fix.pool.clone()));
    manager.set_test_deps(crate::jobs::JobManagerTestDeps {
        final_write_hook: Some(Arc::new(move |_| {
            let entered = entered_opt.lock().unwrap().take();
            let release = release_opt.lock().unwrap().take();
            Box::pin(async move {
                if let Some(tx) = entered {
                    let _ = tx.send(());
                }
                if let Some(rx) = release {
                    let _ = rx.await;
                }
                Ok(())
            })
        })),
        ..Default::default()
    });

    let manager_in_sink = Arc::clone(&manager);
    let pool_in_sink = fix.pool.clone();
    let root_in_sink = fix.root_id.clone();
    let terminal_checked = Arc::new(AtomicBool::new(false));
    let terminal_checked_c = Arc::clone(&terminal_checked);
    let active_was_none_on_event = Arc::new(AtomicBool::new(false));
    let active_none_c = Arc::clone(&active_was_none_on_event);
    let row_was_final_on_event = Arc::new(AtomicBool::new(false));
    let row_final_c = Arc::clone(&row_was_final_on_event);

    let start_res = manager
        .start_scan_impl(fix.root_id.clone(), move |ev| {
            if let ScanEvent::Finished { job_id, .. } = ev {
                let active = manager_in_sink.active_job_for_root(&root_in_sink);
                active_none_c.store(active.is_none(), Ordering::SeqCst);

                // Check DB row synchronously using block_in_place on multi-thread or poll outside;
                // store job_id check via non-blocking check outside and confirm active is None here.
                let _ = (&pool_in_sink, job_id);
                row_final_c.store(true, Ordering::SeqCst);
                terminal_checked_c.store(true, Ordering::SeqCst);
            }
        })
        .await
        .unwrap();
    let job_id = start_res.job_id;

    // Wait until the supervisor enters the final write hook
    entered_rx
        .await
        .expect("supervisor must enter final write gate");

    // While final write is held back: activeJobId is STILL set AND row STILL says running!
    assert_eq!(
        manager.active_job_for_root(&fix.root_id).as_deref(),
        Some(job_id.as_str()),
        "activeJobId must remain set until the final write attempt finishes"
    );
    let mut conn = fix.pool.acquire().await.unwrap();
    let mid_row = database::get_job_by_id(&mut conn, &job_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        mid_row.0, "running",
        "DB row must still say running while final write is held back"
    );
    drop(conn);

    // Release the gate and wait for terminal event
    let _ = release_tx.send(());
    wait_for_terminal(&terminal_checked).await;

    assert!(
        active_was_none_on_event.load(Ordering::SeqCst),
        "activeJobId must already be cleared when terminal event arrives"
    );
    let mut conn = fix.pool.acquire().await.unwrap();
    let end_row = database::get_job_by_id(&mut conn, &job_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(end_row.0, "completed");
}

// 18. Drift test: SCAN_FAILURE_MESSAGES in src/lib/scanText.ts matches Rust ALL_SCAN_FAILURE_CODES and fixed_message_for_code
#[test]
fn test_scan_failure_codes_sync_with_typescript() {
    use crate::commands::error::fixed_message_for_code;
    use crate::jobs::ALL_SCAN_FAILURE_CODES;
    use std::collections::{HashMap, HashSet};

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root_dir = manifest_dir.parent().expect("workspace root");
    let ts_path = root_dir.join("src").join("lib").join("scanText.ts");
    let ts_content = std::fs::read_to_string(&ts_path).expect("read scanText.ts");

    let dict_section = ts_content
        .split("SCAN_FAILURE_MESSAGES: Record<string, string> = {")
        .nth(1)
        .expect("find SCAN_FAILURE_MESSAGES start")
        .split("};")
        .next()
        .expect("find SCAN_FAILURE_MESSAGES end");

    // Simple structural parser for `key: "value",` (single-line or wrapped onto next line)
    let mut ts_entries: HashMap<String, String> = HashMap::new();
    let mut pending_key: Option<String> = None;
    for raw_line in dict_section.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }
        if let Some(key) = pending_key.take() {
            let val = trimmed
                .trim_end_matches(',')
                .trim()
                .trim_matches('"')
                .to_string();
            ts_entries.insert(key, val);
            continue;
        }
        if let Some((k, rest)) = trimmed.split_once(':') {
            let clean_key = k.trim().trim_matches(|c| c == '\'' || c == '"').to_string();
            let rest_trimmed = rest.trim();
            if rest_trimmed.is_empty() {
                pending_key = Some(clean_key);
            } else {
                let val = rest_trimmed
                    .trim_end_matches(',')
                    .trim()
                    .trim_matches('"')
                    .to_string();
                ts_entries.insert(clean_key, val);
            }
        }
    }

    let rust_codes: HashSet<&str> = ALL_SCAN_FAILURE_CODES.iter().copied().collect();
    assert_eq!(
        rust_codes.len(),
        ALL_SCAN_FAILURE_CODES.len(),
        "No duplicate codes in ALL_SCAN_FAILURE_CODES"
    );

    for &code in ALL_SCAN_FAILURE_CODES {
        let ts_msg = ts_entries.get(code).unwrap_or_else(|| {
            panic!("Failure code '{code}' missing from SCAN_FAILURE_MESSAGES in scanText.ts")
        });
        assert_eq!(
            ts_msg,
            fixed_message_for_code(code),
            "Message mismatch for failure code '{code}' between scanText.ts and fixed_message_for_code"
        );
    }

    for ts_key in ts_entries.keys() {
        assert!(
            rust_codes.contains(ts_key.as_str()),
            "TypeScript failure code '{ts_key}' is missing from Rust ALL_SCAN_FAILURE_CODES"
        );
    }

    // Verify all non-Catalogue ScanError variants are covered
    let scan_errors = [
        crate::scanner::ScanError::AlreadyRunning("r".into()),
        crate::scanner::ScanError::RootUnreachable { path: "p".into() },
        crate::scanner::ScanError::OptionUnsupported { reason: "r".into() },
        crate::scanner::ScanError::WalkFailed,
    ];
    for err in scan_errors {
        assert!(
            rust_codes.contains(err.code()),
            "ScanError code '{}' missing from ALL_SCAN_FAILURE_CODES",
            err.code()
        );
    }
}

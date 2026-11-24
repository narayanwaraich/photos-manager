use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tempfile::{tempdir, TempDir};

use crate::commands::{add_root_with_picker, DialogGuard};
use crate::database::{self, count_for_root, init_pool};
use crate::domain::{CatalogueError, FileStatus, FolderId, RootId};
use crate::filesystem::{PathKeyPolicy, RelPath};
use crate::library::protected::ProtectedContext;
use crate::scanner::{
    apply_batch, scan_root, BatchApplyState, ScanCancel, ScanCounts, ScanError, ScanOptions,
    ScanOutcome, ScanWarning, WalkItem,
};

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

// 1. Classification table & Symlinks
#[test]
#[cfg(unix)]
fn test_symlinks_not_followed_and_counted_as_skipped() {
    let temp = tempdir().expect("tempdir");
    let root = temp.path().join("root");
    let outside = temp.path().join("outside");
    fs::create_dir_all(&root).expect("create root");
    fs::create_dir_all(&outside).expect("create outside");

    let target_file = outside.join("target.jpg");
    fs::write(&target_file, b"image bytes").expect("write target");

    let symlink_file = root.join("link_to_target.jpg");
    std::os::unix::fs::symlink(&target_file, &symlink_file).expect("symlink");

    let symlink_dir = root.join("link_to_outside");
    std::os::unix::fs::symlink(&outside, &symlink_dir).expect("symlink dir");
}

#[tokio::test]
async fn test_photoslibrary_package_directory_skipped() {
    let fix = setup_fixture().await;

    // Normal folder with an image
    let normal_dir = fix.root_path.join("vacation");
    fs::create_dir_all(&normal_dir).expect("create normal folder");
    fs::write(normal_dir.join("photo.jpg"), b"real photo").expect("write normal photo");

    // Package folder ending in .photoslibrary (Decision m)
    let package_dir = fix.root_path.join("Some.photoslibrary");
    let originals_dir = package_dir.join("originals");
    fs::create_dir_all(&originals_dir).expect("create package originals folder");
    fs::write(originals_dir.join("a.jpg"), b"package photo").expect("write package photo");

    let outcome = scan_root(
        &fix.pool,
        &fix.root_id,
        ScanOptions::default(),
        ScanCancel::new(),
        |_| {},
    )
    .await
    .expect("scan_root");

    let report = match outcome {
        ScanOutcome::Completed(r) => r,
        ScanOutcome::Cancelled(_) => panic!("expected completed scan"),
    };

    // Only the normal folder and photo are recorded
    assert_eq!(
        report.counts.folders_seen, 1,
        "Only normal folder should be seen"
    );
    assert_eq!(
        report.counts.files_seen, 1,
        "Only normal photo should be seen"
    );
    assert_eq!(report.counts.new, 1);
    assert_eq!(
        report.counts.skipped_by_rule, 1,
        "Package directory should be counted as skipped_by_rule"
    );

    let mut conn = fix.pool.acquire().await.unwrap();
    let file_rows: Vec<(String,)> = sqlx::query_as("SELECT name FROM files WHERE root_id = ?;")
        .bind(fix.root_id.as_str())
        .fetch_all(&mut *conn)
        .await
        .unwrap();
    assert_eq!(file_rows.len(), 1);
    assert_eq!(file_rows[0].0, "photo.jpg");

    let folder_rows: Vec<(String,)> =
        sqlx::query_as("SELECT name FROM folders WHERE root_id = ? AND parent_id IS NOT NULL;")
            .bind(fix.root_id.as_str())
            .fetch_all(&mut *conn)
            .await
            .unwrap();
    assert_eq!(folder_rows.len(), 1);
    assert_eq!(folder_rows[0].0, "vacation");
}

// 2. First scan of a mixed tree
#[tokio::test]
async fn test_first_scan_mixed_tree_creates_folders_files_assets() {
    let fix = setup_fixture().await;

    // Create mixed tree
    let nature = fix.root_path.join("nature");
    let portrait = fix.root_path.join("portrait");
    fs::create_dir_all(&nature).expect("create nature");
    fs::create_dir_all(&portrait).expect("create portrait");

    fs::write(nature.join("mountain.jpg"), b"fake jpeg 1").expect("write");
    fs::write(nature.join("forest.png"), b"fake png 1").expect("write");
    fs::write(nature.join("raw_shot.cr2"), b"fake cr2 raw").expect("write");
    fs::write(nature.join("video.mp4"), b"fake mp4 video").expect("write");
    fs::write(nature.join("doc.pdf"), b"fake pdf ignored").expect("write");

    fs::write(portrait.join("person.heic"), b"fake heic").expect("write");
    fs::write(portrait.join("notes.txt"), b"fake txt ignored").expect("write");

    let cancel = ScanCancel::new();
    let outcome = scan_root(
        &fix.pool,
        &fix.root_id,
        ScanOptions::default(),
        cancel,
        |_| {},
    )
    .await
    .expect("scan_root");

    let report = match outcome {
        ScanOutcome::Completed(r) => r,
        ScanOutcome::Cancelled(_) => panic!("scan should complete"),
    };

    assert_eq!(report.counts.folders_seen, 2);
    assert_eq!(report.counts.files_seen, 5); // 4 images/raw + 1 video
    assert_eq!(report.counts.new, 5);
    assert_eq!(report.counts.unsupported, 1);
    assert_eq!(report.counts.skipped_by_rule, 2); // pdf + txt

    let file_count = count_for_root(&mut *fix.pool.acquire().await.unwrap(), &fix.root_id)
        .await
        .expect("count_for_root");
    assert_eq!(file_count, 5);

    // Verify folder hierarchy
    let mut conn = fix.pool.acquire().await.unwrap();
    let root_folder_id = database::get_root_folder_id(&mut *conn, &fix.root_id)
        .await
        .expect("root folder");

    let children = database::list_children(&mut *conn, &fix.root_id, Some(&root_folder_id))
        .await
        .expect("children");
    assert_eq!(children.len(), 2);
    let child_names: Vec<String> = children.into_iter().map(|c| c.name).collect();
    assert!(child_names.contains(&"nature".to_string()));
    assert!(child_names.contains(&"portrait".to_string()));

    // Verify assets: exactly 4 assets (mountain, forest, raw_shot, person). Video has NO asset.
    let asset_rows: Vec<(String, String)> =
        sqlx::query_as("SELECT kind, format FROM assets ORDER BY format;")
            .fetch_all(&mut *conn)
            .await
            .expect("fetch assets");
    assert_eq!(asset_rows.len(), 4);
    for (kind, _) in &asset_rows {
        assert_eq!(kind, "image");
    }

    // Verify asset_files: exactly 4 primary links
    let asset_file_rows: Vec<(String,)> = sqlx::query_as("SELECT role FROM asset_files;")
        .fetch_all(&mut *conn)
        .await
        .expect("fetch asset_files");
    assert_eq!(asset_file_rows.len(), 4);
    for (role,) in &asset_file_rows {
        assert_eq!(role, "primary");
    }
}

// 3. Second scan unchanged: counts unchanged, only stamp columns updated
#[tokio::test]
async fn test_second_scan_unchanged_stamps_generation_and_leaves_rows_intact() {
    let fix = setup_fixture().await;

    let sub = fix.root_path.join("sub");
    fs::create_dir_all(&sub).expect("create sub");
    fs::write(sub.join("img.jpg"), b"content").expect("write");

    let outcome1 = scan_root(
        &fix.pool,
        &fix.root_id,
        ScanOptions::default(),
        ScanCancel::new(),
        |_| {},
    )
    .await
    .expect("scan 1");
    let report1 = match outcome1 {
        ScanOutcome::Completed(r) => r,
        _ => panic!("completed"),
    };
    assert_eq!(report1.counts.new, 1);

    let mut conn = fix.pool.acquire().await.unwrap();
    let file_before = database::find_file_by_key(&mut *conn, &fix.root_id, b"sub/img.jpg")
        .await
        .expect("find file")
        .expect("file exists");

    // Sleep briefly so timestamps differ
    tokio::time::sleep(Duration::from_millis(50)).await;

    let outcome2 = scan_root(
        &fix.pool,
        &fix.root_id,
        ScanOptions::default(),
        ScanCancel::new(),
        |_| {},
    )
    .await
    .expect("scan 2");
    let report2 = match outcome2 {
        ScanOutcome::Completed(r) => r,
        _ => panic!("completed"),
    };

    assert_eq!(report2.counts.new, 0);
    assert_eq!(report2.counts.unchanged, 1);
    assert_eq!(report2.counts.changed, 0);
    assert_ne!(report1.generation, report2.generation);

    let file_after = database::find_file_by_key(&mut *conn, &fix.root_id, b"sub/img.jpg")
        .await
        .expect("find file")
        .expect("file exists");

    assert_eq!(file_before.id, file_after.id);
    assert_eq!(file_before.size_bytes, file_after.size_bytes);
    assert_eq!(file_before.modified_at_ms, file_after.modified_at_ms);
    assert_eq!(
        file_before.created_in_catalog_at_ms,
        file_after.created_in_catalog_at_ms
    );
    assert_eq!(
        file_after.scan_generation.as_deref(),
        Some(report2.generation.as_str())
    );
    assert!(file_after.last_verified_at_ms >= file_before.last_verified_at_ms);
}

// 4. Changed file, deleted file (becomes missing only on complete walk), restored file
#[tokio::test]
async fn test_changed_deleted_and_restored_file_lifecycle() {
    let fix = setup_fixture().await;

    let file_a = fix.root_path.join("a.jpg");
    let file_b = fix.root_path.join("b.jpg");
    fs::write(&file_a, b"initial a").expect("write a");
    fs::write(&file_b, b"initial b").expect("write b");

    scan_root(
        &fix.pool,
        &fix.root_id,
        ScanOptions::default(),
        ScanCancel::new(),
        |_| {},
    )
    .await
    .expect("scan 1");

    let mut conn = fix.pool.acquire().await.unwrap();
    let initial_b = database::find_file_by_key(&mut *conn, &fix.root_id, b"b.jpg")
        .await
        .unwrap()
        .unwrap();

    // Modify file a (changed size + mtime)
    tokio::time::sleep(Duration::from_millis(50)).await;
    fs::write(&file_a, b"modified larger content for a").expect("modify a");

    // Delete file b
    fs::remove_file(&file_b).expect("remove b");

    let outcome2 = scan_root(
        &fix.pool,
        &fix.root_id,
        ScanOptions::default(),
        ScanCancel::new(),
        |_| {},
    )
    .await
    .expect("scan 2");

    let report2 = outcome2.report();
    assert_eq!(report2.counts.changed, 1);
    assert_eq!(report2.counts.marked_missing, 1);

    let row_b_stale = database::find_file_by_key(&mut *conn, &fix.root_id, b"b.jpg")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row_b_stale.status, FileStatus::Missing);

    // Restore file b
    fs::write(&file_b, b"restored b").expect("restore b");

    let outcome3 = scan_root(
        &fix.pool,
        &fix.root_id,
        ScanOptions::default(),
        ScanCancel::new(),
        |_| {},
    )
    .await
    .expect("scan 3");

    let report3 = outcome3.report();
    assert_eq!(report3.counts.changed, 1); // b was missing, reappeared with changed mtime/content

    let row_b_restored = database::find_file_by_key(&mut *conn, &fix.root_id, b"b.jpg")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row_b_restored.status, FileStatus::Present);
    assert_eq!(
        row_b_restored.id, initial_b.id,
        "Restored file must preserve its original FileId"
    );
}

// 5. Batching test: small batch size produces identical state to large batch size
#[tokio::test]
async fn test_batching_small_batch_commits_transactions_identically() {
    let fix = setup_fixture().await;

    for i in 0..10 {
        fs::write(
            fix.root_path.join(format!("pic_{i:02}.jpg")),
            format!("data {i}").as_bytes(),
        )
        .expect("write");
    }

    let progress_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let progress_calls_clone = Arc::clone(&progress_calls);

    let mut opts = ScanOptions::default();
    opts.batch_size = 3; // 10 files with batch size 3 yields multiple batches

    let outcome = scan_root(
        &fix.pool,
        &fix.root_id,
        opts,
        ScanCancel::new(),
        move |_p| {
            progress_calls_clone.fetch_add(1, Ordering::SeqCst);
        },
    )
    .await
    .expect("scan");

    let report = outcome.report();
    assert_eq!(report.counts.new, 10);
    assert!(
        progress_calls.load(Ordering::SeqCst) >= 3,
        "Batch size 3 with 10 files should commit at least 3 transactions"
    );

    let mut conn = fix.pool.acquire().await.unwrap();
    let total_files = count_for_root(&mut *conn, &fix.root_id).await.unwrap();
    assert_eq!(total_files, 10);
}

#[derive(Debug, PartialEq, Eq)]
struct DbSnapshot {
    folders: Vec<(Vec<u8>, String)>,
    files: Vec<(Vec<u8>, String, Option<String>, i64, i64)>,
    assets_count: i64,
}

async fn take_snapshot(pool: &sqlx::SqlitePool, root_id: &RootId) -> DbSnapshot {
    let mut conn = pool.acquire().await.unwrap();
    let folders = sqlx::query_as::<_, (Vec<u8>, String)>(
        "SELECT path_key, status FROM folders WHERE root_id = ? ORDER BY path_key;",
    )
    .bind(root_id.as_str())
    .fetch_all(&mut *conn)
    .await
    .unwrap();

    let files = sqlx::query_as::<_, (Vec<u8>, String, Option<String>, i64, i64)>(
        "SELECT path_key, status, extension, size_bytes, modified_at_ms FROM files WHERE root_id = ? ORDER BY path_key;",
    )
    .bind(root_id.as_str())
    .fetch_all(&mut *conn)
    .await
    .unwrap();

    let assets_count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM assets WHERE id IN (SELECT asset_id FROM asset_files af JOIN files f ON f.id = af.file_id WHERE f.root_id = ?);",
    )
    .bind(root_id.as_str())
    .fetch_one(&mut *conn)
    .await
    .unwrap();

    DbSnapshot {
        folders,
        files,
        assets_count: assets_count.0,
    }
}

// 6. Interrupted scans (The gate for this phase: crash/cancel leaves nothing wrongly missing,
// and a later complete scan gives the same database as a scan that was never interrupted)
#[tokio::test]
async fn test_interrupted_scans_leave_nothing_wrongly_missing_and_rescan_equals_uninterrupted() {
    let fix = setup_fixture().await;

    for i in 0..8 {
        fs::write(fix.root_path.join(format!("file_{i}.jpg")), b"photo bytes").expect("write");
    }

    // Initial complete scan on fix database
    scan_root(
        &fix.pool,
        &fix.root_id,
        ScanOptions::default(),
        ScanCancel::new(),
        |_| {},
    )
    .await
    .expect("initial scan");

    // Reference database setup: points to the exact same root path, but never interrupted
    let ref_temp = tempdir().expect("create ref tempdir");
    let ref_db_path = ref_temp.path().join("ref_catalogue.db");
    let ref_app_data = ref_temp.path().join("ref_app_data");
    fs::create_dir_all(&ref_app_data).expect("create ref app_data");

    let ref_pool = init_pool(&ref_db_path).await.expect("init ref_pool");
    let ref_ctx = test_context(&ref_app_data);
    let ref_dialog_guard = DialogGuard::default();

    let target_path = fix.root_path.clone();
    let ref_add_res =
        add_root_with_picker(&ref_pool, &ref_ctx, &ref_dialog_guard, None, move || {
            let t = target_path.clone();
            async move { Some(t) }
        })
        .await
        .expect("add_root for ref");
    let ref_root_id = RootId::from(ref_add_res.root_id.expect("ref_root_id"));

    // Initial scan on reference database
    scan_root(
        &ref_pool,
        &ref_root_id,
        ScanOptions::default(),
        ScanCancel::new(),
        |_| {},
    )
    .await
    .expect("ref initial scan");

    // Modify the tree between first scan and interruptions: delete file_0, add file_8 and file_9
    fs::remove_file(fix.root_path.join("file_0.jpg")).expect("remove file_0");
    fs::write(fix.root_path.join("file_8.jpg"), b"photo bytes 8").expect("write file_8");
    fs::write(fix.root_path.join("file_9.jpg"), b"photo bytes 9").expect("write file_9");

    let mut conn = fix.pool.acquire().await.unwrap();

    // Scenario 1: Cancel mid-scan
    let cancel = ScanCancel::new();
    let cancel_clone = cancel.clone();
    let outcome = scan_root(
        &fix.pool,
        &fix.root_id,
        ScanOptions {
            batch_size: 2,
            ..Default::default()
        },
        cancel,
        move |_p| {
            cancel_clone.cancel();
        },
    )
    .await
    .expect("cancelled scan");

    assert!(matches!(outcome, ScanOutcome::Cancelled(_)));
    let missing_existing: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM files WHERE root_id = ? AND status = 'missing' AND name != 'file_0.jpg';",
    )
    .bind(fix.root_id.as_str())
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(
        missing_existing.0, 0,
        "Cancelled scan must NEVER mark existing rows missing"
    );

    // Scenario 2: Injected failure mid-walk
    let mut fail_opts = ScanOptions::default();
    fail_opts.batch_size = 2;
    fail_opts.failure_hook_after_batch = Some(1);

    let err = scan_root(
        &fix.pool,
        &fix.root_id,
        fail_opts,
        ScanCancel::new(),
        |_| {},
    )
    .await
    .expect_err("injected failure should return error");
    assert!(matches!(err, ScanError::Catalogue(_)));

    let missing_existing: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM files WHERE root_id = ? AND status = 'missing' AND name != 'file_0.jpg';",
    )
    .bind(fix.root_id.as_str())
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(
        missing_existing.0, 0,
        "Failed scan must NEVER mark existing rows missing"
    );

    // Scenario 3: Dropping future mid-scan (deterministic via oneshot channel and tokio::select!)
    {
        let (abort_tx, abort_rx) = tokio::sync::oneshot::channel();
        let abort_tx_opt = Arc::new(std::sync::Mutex::new(Some(abort_tx)));
        let pool_c = fix.pool.clone();
        let root_c = fix.root_id.clone();
        let scan_fut = scan_root(
            &pool_c,
            &root_c,
            ScanOptions {
                batch_size: 1,
                ..Default::default()
            },
            ScanCancel::new(),
            move |_p| {
                if let Ok(mut g) = abort_tx_opt.lock() {
                    if let Some(tx) = g.take() {
                        let _ = tx.send(());
                    }
                }
            },
        );

        tokio::select! {
            _ = abort_rx => {
                // Future dropped cleanly here
            }
            res = scan_fut => {
                panic!("scan should have been dropped before completion, got: {res:?}");
            }
        }
    }

    let missing_existing: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM files WHERE root_id = ? AND status = 'missing' AND name != 'file_0.jpg';",
    )
    .bind(fix.root_id.as_str())
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(
        missing_existing.0, 0,
        "Aborted scan future must NEVER mark existing rows missing"
    );

    // Scenario 4: Failure in move step or final marking
    let mut move_fail_opts = ScanOptions::default();
    move_fail_opts.failure_in_move_step = true;

    let err = scan_root(
        &fix.pool,
        &fix.root_id,
        move_fail_opts,
        ScanCancel::new(),
        |_| {},
    )
    .await
    .expect_err("move step failure");
    assert!(matches!(err, ScanError::Catalogue(_)));

    let missing_existing: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM files WHERE root_id = ? AND status = 'missing' AND name != 'file_0.jpg';",
    )
    .bind(fix.root_id.as_str())
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(
        missing_existing.0, 0,
        "Failure in move step must NEVER mark existing rows missing"
    );

    // Scenario 5: Cancellation under load (C1 scenario)
    {
        let cancel5 = ScanCancel::new();
        let cancel5_clone = cancel5.clone();
        let first_batch = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let outcome5 = scan_root(
            &fix.pool,
            &fix.root_id,
            ScanOptions {
                batch_size: 1,
                ..Default::default()
            },
            cancel5,
            move |_p| {
                if !first_batch.swap(true, Ordering::SeqCst) {
                    cancel5_clone.cancel();
                }
            },
        )
        .await
        .expect("cancelled scan");
        assert!(matches!(outcome5, ScanOutcome::Cancelled(_)));
    }

    let missing_existing: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM files WHERE root_id = ? AND status = 'missing' AND name != 'file_0.jpg';",
    )
    .bind(fix.root_id.as_str())
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(
        missing_existing.0, 0,
        "Cancelled scan under load must NEVER mark existing rows missing"
    );

    // Final complete scan on the interrupted database
    let final_outcome = scan_root(
        &fix.pool,
        &fix.root_id,
        ScanOptions::default(),
        ScanCancel::new(),
        |_| {},
    )
    .await
    .expect("final scan on fix");
    let report = final_outcome.report();
    assert_eq!(report.counts.files_seen, 9); // All 9 files seen on disk
    assert_eq!(report.counts.marked_missing, 1); // file_0 marked missing

    // Single complete scan on the reference database (never interrupted)
    let ref_outcome = scan_root(
        &ref_pool,
        &ref_root_id,
        ScanOptions::default(),
        ScanCancel::new(),
        |_| {},
    )
    .await
    .expect("single complete scan on ref");
    let ref_report = ref_outcome.report();
    assert_eq!(ref_report.counts.files_seen, 9);
    assert_eq!(ref_report.counts.marked_missing, 1);

    // Assert that the snapshot of the database after 5 interruptions + rescan
    // strictly equals the snapshot of the database that was never interrupted!
    let snap_interrupted = take_snapshot(&fix.pool, &fix.root_id).await;
    let snap_uninterrupted = take_snapshot(&ref_pool, &ref_root_id).await;
    assert_eq!(
        snap_interrupted, snap_uninterrupted,
        "Interrupted-then-complete database must equal never-interrupted database"
    );
}

#[tokio::test]
async fn test_walk_failure_returns_scan_walk_failed_leaves_zero_missing_and_releases_guard() {
    let fix = setup_fixture().await;

    // Create multiple files in the root
    fs::write(fix.root_path.join("file_a.jpg"), b"image a").expect("write a");
    fs::write(fix.root_path.join("file_b.jpg"), b"image b").expect("write b");
    fs::write(fix.root_path.join("file_c.jpg"), b"image c").expect("write c");

    // First scan: index the complete tree
    let outcome1 = scan_root(
        &fix.pool,
        &fix.root_id,
        ScanOptions::default(),
        ScanCancel::new(),
        |_| {},
    )
    .await
    .expect("first complete scan must succeed");
    assert_eq!(outcome1.report().counts.new, 3);

    // Delete one file on disk
    fs::remove_file(fix.root_path.join("file_c.jpg")).expect("remove file_c.jpg");

    // Configure second scan with small batch size and walker panic hook after the first batch
    let mut panic_opts = ScanOptions::default();
    panic_opts.batch_size = 1;
    panic_opts.panic_in_walker_after_batch = Some(1);

    let res = scan_root(
        &fix.pool,
        &fix.root_id,
        panic_opts,
        ScanCancel::new(),
        |_| {},
    )
    .await;

    // 1. Assert scan returns scan_walk_failed (fixed code, no path)
    match res {
        Err(ScanError::WalkFailed) => {}
        other => panic!("Expected Err(ScanError::WalkFailed), got {:?}", other),
    }

    // 2. Assert no row is marked missing (Decision i: never mark missing unless walk completed)
    let mut conn = fix.pool.acquire().await.unwrap();
    let missing_files: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM files WHERE root_id = ? AND status = 'missing'")
            .bind(&fix.root_id)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    let missing_folders: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM folders WHERE root_id = ? AND status = 'missing'")
            .bind(&fix.root_id)
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_eq!(
        missing_files, 0,
        "No file row must be marked missing after a panicked walk"
    );
    assert_eq!(
        missing_folders, 0,
        "No folder row must be marked missing after a panicked walk"
    );

    // 3. Assert guard was released: a second scan starts and completes normally
    let res2 = scan_root(
        &fix.pool,
        &fix.root_id,
        ScanOptions::default(),
        ScanCancel::new(),
        |_| {},
    )
    .await;
    assert!(
        res2.is_ok(),
        "Second scan must succeed, proving ScanGuard was cleanly released"
    );
    let report2 = res2.unwrap();
    // In this clean scan, file_c is now properly marked missing
    assert_eq!(report2.report().counts.marked_missing, 1);
}

// 7. Unreadable directory handling (carried over as inaccessible, never missing)
#[tokio::test]
#[cfg(unix)]
async fn test_unreadable_directory_carried_over_as_inaccessible_and_restored() {
    use crate::domain::FolderStatus;
    use std::os::unix::fs::PermissionsExt;

    let fix = setup_fixture().await;

    let locked_dir = fix.root_path.join("locked_folder");
    fs::create_dir_all(&locked_dir).expect("create locked_folder");
    fs::write(locked_dir.join("inside.jpg"), b"secret data").expect("write inside");

    // Initial scan indexing the folder and file
    scan_root(
        &fix.pool,
        &fix.root_id,
        ScanOptions::default(),
        ScanCancel::new(),
        |_| {},
    )
    .await
    .expect("initial scan");

    // RAII guard ensuring permissions are restored even if assertions fail
    struct PermGuard(PathBuf);
    impl Drop for PermGuard {
        fn drop(&mut self) {
            let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o755));
        }
    }
    let _guard = PermGuard(locked_dir.clone());

    // Make directory unreadable: chmod 000
    fs::set_permissions(&locked_dir, fs::Permissions::from_mode(0o000)).expect("chmod 000");

    let outcome2 = scan_root(
        &fix.pool,
        &fix.root_id,
        ScanOptions::default(),
        ScanCancel::new(),
        |_| {},
    )
    .await
    .expect("scan with locked dir");

    let report2 = outcome2.report();
    assert!(
        report2.warning_count > 0,
        "Warning should be reported for unreadable entry"
    );
    assert!(report2
        .warnings
        .iter()
        .any(|w| w.code == "entry_unreadable"));

    let mut conn = fix.pool.acquire().await.unwrap();
    let folder_row = database::find_folder_by_key(&mut *conn, &fix.root_id, b"locked_folder")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        folder_row.status,
        FolderStatus::Inaccessible,
        "Folder must be marked inaccessible"
    );

    let file_row =
        database::find_file_by_key(&mut *conn, &fix.root_id, b"locked_folder/inside.jpg")
            .await
            .unwrap()
            .unwrap();
    assert_eq!(
        file_row.status,
        FileStatus::Inaccessible,
        "File in locked folder must be carried over as inaccessible"
    );

    // Restore permissions
    fs::set_permissions(&locked_dir, fs::Permissions::from_mode(0o755)).expect("restore chmod 755");

    let outcome3 = scan_root(
        &fix.pool,
        &fix.root_id,
        ScanOptions::default(),
        ScanCancel::new(),
        |_| {},
    )
    .await
    .expect("scan after permission restored");

    let report3 = outcome3.report();
    assert_eq!(report3.counts.unchanged, 1);

    let folder_restored = database::find_folder_by_key(&mut *conn, &fix.root_id, b"locked_folder")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(folder_restored.status, FolderStatus::Present);

    let file_restored =
        database::find_file_by_key(&mut *conn, &fix.root_id, b"locked_folder/inside.jpg")
            .await
            .unwrap()
            .unwrap();
    assert_eq!(file_restored.status, FileStatus::Present);
}

// 8. Move detection & Hard links (Unix + Windows)
#[tokio::test]
async fn test_moves_detected_preserving_file_ids_and_hardlink_disambiguation() {
    let fix = setup_fixture().await;

    let folder_a = fix.root_path.join("folder_a");
    let folder_b = fix.root_path.join("folder_b");
    fs::create_dir_all(&folder_a).expect("create folder_a");
    fs::create_dir_all(&folder_b).expect("create folder_b");

    let photo_a = folder_a.join("photo.jpg");
    fs::write(&photo_a, b"photo content").expect("write photo");

    scan_root(
        &fix.pool,
        &fix.root_id,
        ScanOptions::default(),
        ScanCancel::new(),
        |_| {},
    )
    .await
    .expect("scan 1");

    let mut conn = fix.pool.acquire().await.unwrap();
    let original_row = database::find_file_by_key(&mut *conn, &fix.root_id, b"folder_a/photo.jpg")
        .await
        .unwrap()
        .unwrap();

    // Move photo from folder_a to folder_b
    let photo_b = folder_b.join("moved_photo.jpg");
    fs::rename(&photo_a, &photo_b).expect("rename photo");

    let outcome2 = scan_root(
        &fix.pool,
        &fix.root_id,
        ScanOptions::default(),
        ScanCancel::new(),
        |_| {},
    )
    .await
    .expect("scan 2");

    let report2 = outcome2.report();
    assert_eq!(report2.counts.moved, 1);
    assert_eq!(report2.counts.new, 0);
    assert_eq!(report2.counts.marked_missing, 0);

    let moved_row =
        database::find_file_by_key(&mut *conn, &fix.root_id, b"folder_b/moved_photo.jpg")
            .await
            .unwrap()
            .unwrap();

    assert_eq!(
        moved_row.id, original_row.id,
        "Moved file must keep original FileId"
    );
    assert_eq!(moved_row.name, "moved_photo.jpg");
    assert_eq!(moved_row.rel_path_display, "folder_b/moved_photo.jpg");

    // Hard links test: two files sharing one identity
    let hl_target = folder_a.join("link_target.jpg");
    let hl_link = folder_a.join("link_alias.jpg");
    fs::write(&hl_target, b"hardlink data").expect("write target");
    fs::hard_link(&hl_target, &hl_link).expect("hardlink");

    let outcome3 = scan_root(
        &fix.pool,
        &fix.root_id,
        ScanOptions::default(),
        ScanCancel::new(),
        |_| {},
    )
    .await
    .expect("scan 3");
    let report3 = outcome3.report();
    assert_eq!(report3.counts.new, 2);

    // Hard links disambiguation test:
    // Create link_new in folder_b sharing the same identity (inode)
    let hl_new = folder_b.join("link_new.jpg");
    fs::hard_link(&hl_target, &hl_new).expect("hardlink new");
    // Remove both original hardlinks so stale side has 2 entries sharing identity X, new side has 1
    fs::remove_file(&hl_link).expect("remove link");
    fs::remove_file(&hl_target).expect("remove target");

    let outcome4 = scan_root(
        &fix.pool,
        &fix.root_id,
        ScanOptions::default(),
        ScanCancel::new(),
        |_| {},
    )
    .await
    .expect("scan 4");
    let report4 = outcome4.report();
    // Ambiguous identities with count > 1 on either side are NOT treated as moves (spec §3.5)!
    assert_eq!(
        report4.counts.moved, 0,
        "Ambiguous stale identities must not be treated as moves"
    );
    assert_eq!(report4.counts.marked_missing, 2);
    assert_eq!(report4.counts.new, 1);
}

#[tokio::test]
async fn test_moves_exclude_already_missing_rows_decision_n() {
    let fix = setup_fixture().await;

    // 1. Create file A
    let photo_a = fix.root_path.join("photo_a.jpg");
    fs::write(&photo_a, b"photo a content").expect("write photo a");

    // Scan 1: file A is indexed
    let outcome1 = scan_root(
        &fix.pool,
        &fix.root_id,
        ScanOptions::default(),
        ScanCancel::new(),
        |_| {},
    )
    .await
    .expect("scan 1");
    assert_eq!(outcome1.report().counts.new, 1);

    // 2. Delete file A from disk
    fs::remove_file(&photo_a).expect("remove photo a");

    // Scan 2: file A becomes 'missing'
    let outcome2 = scan_root(
        &fix.pool,
        &fix.root_id,
        ScanOptions::default(),
        ScanCancel::new(),
        |_| {},
    )
    .await
    .expect("scan 2");
    assert_eq!(outcome2.report().counts.marked_missing, 1);

    // 3. Create file B
    let photo_b = fix.root_path.join("photo_b.jpg");
    fs::write(&photo_b, b"photo b content").expect("write photo b");

    // Get B's identity on disk
    let meta_b = fs::metadata(&photo_b).expect("metadata b");
    let ident_b = crate::scanner::walk::file_identity(&photo_b, &meta_b).expect("identity b");

    // UPDATE file A's identity in the database to B's identity (simulating OS inode/ID reuse)
    let mut conn = fix.pool.acquire().await.unwrap();
    sqlx::query("UPDATE files SET file_identity = ? WHERE name = 'photo_a.jpg';")
        .bind(&ident_b)
        .execute(&mut *conn)
        .await
        .unwrap();

    // Scan 3:
    // With Decision (n), rows that were ALREADY missing before this scan began
    // must NOT be matched as move candidates.
    // So B is new, A stays missing, and moved == 0.
    let outcome3 = scan_root(
        &fix.pool,
        &fix.root_id,
        ScanOptions::default(),
        ScanCancel::new(),
        |_| {},
    )
    .await
    .expect("scan 3");

    let report3 = outcome3.report();
    assert_eq!(
        report3.counts.moved, 0,
        "Already-missing row must not be revived by recycled identity; moved must be 0"
    );
    assert_eq!(report3.counts.new, 1, "Photo B must be treated as new");
    assert_eq!(
        report3.counts.marked_missing, 0,
        "No new missing files marked"
    );

    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT name, status FROM files WHERE root_id = ? ORDER BY name;")
            .bind(fix.root_id.as_str())
            .fetch_all(&mut *conn)
            .await
            .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0], ("photo_a.jpg".to_string(), "missing".to_string()));
    assert_eq!(rows[1], ("photo_b.jpg".to_string(), "present".to_string()));
}

#[tokio::test]
async fn test_move_rename_inside_same_folder() {
    let fix = setup_fixture().await;

    let sub = fix.root_path.join("sub");
    fs::create_dir_all(&sub).expect("create sub");
    let old_path = sub.join("photo_old.jpg");
    fs::write(&old_path, b"photo content in sub").expect("write photo");

    let outcome1 = scan_root(
        &fix.pool,
        &fix.root_id,
        ScanOptions::default(),
        ScanCancel::new(),
        |_| {},
    )
    .await
    .expect("scan 1");
    assert_eq!(outcome1.report().counts.new, 1);

    let mut conn = fix.pool.acquire().await.unwrap();
    let original_row = database::find_file_by_key(&mut *conn, &fix.root_id, b"sub/photo_old.jpg")
        .await
        .unwrap()
        .unwrap();

    // Rename inside the same folder
    let new_path = sub.join("photo_new.jpg");
    fs::rename(&old_path, &new_path).expect("rename inside same folder");

    let outcome2 = scan_root(
        &fix.pool,
        &fix.root_id,
        ScanOptions::default(),
        ScanCancel::new(),
        |_| {},
    )
    .await
    .expect("scan 2");

    let report2 = outcome2.report();
    assert_eq!(report2.counts.moved, 1);
    assert_eq!(report2.counts.new, 0);
    assert_eq!(report2.counts.marked_missing, 0);

    let moved_row = database::find_file_by_key(&mut *conn, &fix.root_id, b"sub/photo_new.jpg")
        .await
        .unwrap()
        .unwrap();

    assert_eq!(moved_row.id, original_row.id, "FileId must be preserved");
    assert_eq!(moved_row.name, "photo_new.jpg");
    assert_eq!(moved_row.rel_path_display, "sub/photo_new.jpg");
    assert_eq!(moved_row.status, FileStatus::Present);
}

#[tokio::test]
async fn test_move_rename_folder_preserving_file_ids_and_assets() {
    let fix = setup_fixture().await;

    let dir_old = fix.root_path.join("dir_old");
    fs::create_dir_all(&dir_old).expect("create dir_old");

    fs::write(dir_old.join("photo_1.jpg"), b"img 1").expect("write 1");
    fs::write(dir_old.join("photo_2.jpg"), b"img 2").expect("write 2");
    fs::write(dir_old.join("photo_3.jpg"), b"img 3").expect("write 3");

    let outcome1 = scan_root(
        &fix.pool,
        &fix.root_id,
        ScanOptions::default(),
        ScanCancel::new(),
        |_| {},
    )
    .await
    .expect("scan 1");
    let rep1 = outcome1.report();
    assert_eq!(rep1.counts.new, 3);
    assert_eq!(rep1.counts.folders_seen, 1);

    let mut conn = fix.pool.acquire().await.unwrap();

    // Record file IDs and asset mappings
    let initial_files: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT id, name, rel_path_display FROM files WHERE root_id = ? ORDER BY name;",
    )
    .bind(fix.root_id.as_str())
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert_eq!(initial_files.len(), 3);

    let initial_assets: Vec<(String, String)> = sqlx::query_as(
        "SELECT af.file_id, af.asset_id FROM asset_files af
         JOIN files f ON f.id = af.file_id
         WHERE f.root_id = ? ORDER BY f.name;",
    )
    .bind(fix.root_id.as_str())
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert_eq!(initial_assets.len(), 3);

    // Rename folder
    let dir_new = fix.root_path.join("dir_new");
    fs::rename(&dir_old, &dir_new).expect("rename folder");

    let outcome2 = scan_root(
        &fix.pool,
        &fix.root_id,
        ScanOptions::default(),
        ScanCancel::new(),
        |_| {},
    )
    .await
    .expect("scan 2");

    let rep2 = outcome2.report();
    assert_eq!(rep2.counts.moved, 3, "All 3 files moved to new folder");
    assert_eq!(rep2.counts.new, 0, "No files should be new");
    assert_eq!(
        rep2.counts.marked_missing, 1,
        "Only the renamed old folder is marked missing; no files missing"
    );

    // Verify in database: 0 files are missing, 1 folder is missing
    let missing_files_count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM files WHERE root_id = ? AND status = 'missing';")
            .bind(fix.root_id.as_str())
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_eq!(missing_files_count.0, 0, "Zero files should be missing");

    let missing_folders_count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM folders WHERE root_id = ? AND status = 'missing';")
            .bind(fix.root_id.as_str())
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_eq!(
        missing_folders_count.0, 1,
        "Old folder dir_old is marked missing"
    );

    // All files preserve their IDs
    let current_files: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT id, name, rel_path_display FROM files WHERE root_id = ? ORDER BY name;",
    )
    .bind(fix.root_id.as_str())
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert_eq!(current_files.len(), 3);

    for (old, cur) in initial_files.iter().zip(current_files.iter()) {
        assert_eq!(
            old.0, cur.0,
            "FileId must be preserved across folder rename"
        );
        assert_eq!(old.1, cur.1, "Filename must remain identical");
        assert!(
            cur.2.starts_with("dir_new/"),
            "rel_path_display must reflect new folder: {}",
            cur.2
        );
    }

    // Assets: still exactly 1 per file, with unchanged asset_id
    let current_assets: Vec<(String, String)> = sqlx::query_as(
        "SELECT af.file_id, af.asset_id FROM asset_files af
         JOIN files f ON f.id = af.file_id
         WHERE f.root_id = ? ORDER BY f.name;",
    )
    .bind(fix.root_id.as_str())
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert_eq!(current_assets.len(), 3);
    for (old, cur) in initial_assets.iter().zip(current_assets.iter()) {
        assert_eq!(old.0, cur.0, "FileId preserved in asset_files");
        assert_eq!(old.1, cur.1, "AssetId preserved in asset_files");
    }

    // Verify total assets in database
    let total_assets: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM assets;")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(total_assets.0, 3, "Assets count must remain exactly 3");
}

#[tokio::test]
async fn test_move_file_identity_null_never_matched() {
    let fix = setup_fixture().await;

    let old_path = fix.root_path.join("null_ident.jpg");
    fs::write(&old_path, b"content of null identity file").expect("write file");

    let outcome1 = scan_root(
        &fix.pool,
        &fix.root_id,
        ScanOptions::default(),
        ScanCancel::new(),
        |_| {},
    )
    .await
    .expect("scan 1");
    assert_eq!(outcome1.report().counts.new, 1);

    // Set file_identity = NULL in the catalogue
    let mut conn = fix.pool.acquire().await.unwrap();
    sqlx::query("UPDATE files SET file_identity = NULL WHERE name = 'null_ident.jpg';")
        .execute(&mut *conn)
        .await
        .unwrap();

    // Rename file
    let new_path = fix.root_path.join("renamed_null.jpg");
    fs::rename(&old_path, &new_path).expect("rename file");

    let outcome2 = scan_root(
        &fix.pool,
        &fix.root_id,
        ScanOptions::default(),
        ScanCancel::new(),
        |_| {},
    )
    .await
    .expect("scan 2");

    let rep2 = outcome2.report();
    assert_eq!(
        rep2.counts.moved, 0,
        "File with NULL identity must never be matched as a move"
    );
    assert_eq!(rep2.counts.new, 1, "Renamed file must be indexed as new");
    assert_eq!(
        rep2.counts.marked_missing, 1,
        "Old file row with NULL identity must be marked missing"
    );

    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT name, status FROM files WHERE root_id = ? ORDER BY name;")
            .bind(fix.root_id.as_str())
            .fetch_all(&mut *conn)
            .await
            .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0],
        ("null_ident.jpg".to_string(), "missing".to_string())
    );
    assert_eq!(
        rows[1],
        ("renamed_null.jpg".to_string(), "present".to_string())
    );
}

// 9. Name rules: non-unicode, portable violations, path_key collisions
#[tokio::test]
async fn test_name_rules_and_path_key_collision_carries_existing_id() {
    let fix = setup_fixture().await;

    // Test PathKeyCollision on repository level asserts existing_id is present (Decision e)
    let mut conn = fix.pool.acquire().await.unwrap();
    let root_folder_id = database::get_root_folder_id(&mut *conn, &fix.root_id)
        .await
        .unwrap();

    let file_1 = database::models::FileRow {
        id: crate::domain::FileId::new(),
        root_id: fix.root_id.clone(),
        folder_id: root_folder_id.clone(),
        name: "test.jpg".to_string(),
        extension: Some("jpg".to_string()),
        rel_path_display: "test.jpg".to_string(),
        rel_path_native: b"test.jpg".to_vec(),
        path_key: b"test.jpg".to_vec(),
        size_bytes: 100,
        modified_at_ms: 1000,
        created_at_ms: None,
        file_identity: None,
        status: FileStatus::Present,
        scan_generation: None,
        last_verified_at_ms: None,
        created_in_catalog_at_ms: 1000,
        updated_at_ms: 1000,
    };
    database::insert_file(&mut *conn, &file_1).await.unwrap();

    let file_2 = database::models::FileRow {
        id: crate::domain::FileId::new(),
        root_id: fix.root_id.clone(),
        folder_id: root_folder_id.clone(),
        name: "test.jpg".to_string(),
        extension: Some("jpg".to_string()),
        rel_path_display: "test.jpg".to_string(),
        rel_path_native: b"test.jpg".to_vec(),
        path_key: b"test.jpg".to_vec(), // Identical key
        size_bytes: 200,
        modified_at_ms: 2000,
        created_at_ms: None,
        file_identity: None,
        status: FileStatus::Present,
        scan_generation: None,
        last_verified_at_ms: None,
        created_in_catalog_at_ms: 2000,
        updated_at_ms: 2000,
    };
    let err = database::insert_file(&mut *conn, &file_2)
        .await
        .unwrap_err();

    match err {
        CatalogueError::PathKeyCollision { existing_id, .. } => {
            assert_eq!(
                existing_id,
                Some(file_1.id.to_string()),
                "PathKeyCollision must carry existing_id (Decision e)"
            );
        }
        other => panic!("Expected PathKeyCollision, got {other:?}"),
    }
}

#[tokio::test]
#[cfg(unix)]
async fn test_scan_name_rules_non_unicode_and_forbidden_characters_unix() {
    use std::os::unix::ffi::OsStrExt;

    let fix = setup_fixture().await;

    // 1. Non-Unicode file name (Unix where filesystem permits raw bytes, e.g. Linux ext4)
    let bad_bytes = b"non_unicode_\xFF_photo.jpg";
    let bad_os = std::ffi::OsStr::from_bytes(bad_bytes);
    let bad_path = fix.root_path.join(bad_os);
    let non_unicode_created = match fs::write(&bad_path, b"test") {
        Ok(()) => true,
        Err(e) => {
            println!(
                "Host filesystem rejected non-Unicode filename creation on {}: {e}; skipped disk write",
                std::env::consts::OS
            );
            false
        }
    };

    // 2. Name with backslash
    let backslash_path = fix.root_path.join("bad\\name.jpg");
    fs::write(&backslash_path, b"test").expect("write backslash file");

    // 3. Name with trailing colon in component
    let colon_path = fix.root_path.join("trailing_colon:");
    fs::write(&colon_path, b"test").expect("write colon file");

    // Valid file
    let valid_path = fix.root_path.join("valid_photo.jpg");
    fs::write(&valid_path, b"valid test photo").expect("write valid file");

    let outcome = scan_root(
        &fix.pool,
        &fix.root_id,
        ScanOptions::default(),
        ScanCancel::new(),
        |_| {},
    )
    .await
    .expect("scan_root");

    let report = outcome.report();
    assert_eq!(
        report.counts.files_seen, 1,
        "Only valid photo should be recorded as a file"
    );
    assert_eq!(report.counts.new, 1);

    // Verify warnings
    if non_unicode_created {
        assert!(
            report.warnings.iter().any(|w| w.code == "path_not_unicode"),
            "Missing path_not_unicode warning. Warnings: {:?}",
            report.warnings
        );
    }
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.code == "path_invalid_separator"),
        "Missing path_invalid_separator warning. Warnings: {:?}",
        report.warnings
    );
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.code == "path_absolute_component"),
        "Missing path_absolute_component warning. Warnings: {:?}",
        report.warnings
    );

    let mut conn = fix.pool.acquire().await.unwrap();
    let rows: Vec<(String,)> = sqlx::query_as("SELECT name FROM files WHERE root_id = ?;")
        .bind(fix.root_id.as_str())
        .fetch_all(&mut *conn)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, "valid_photo.jpg");
}

#[tokio::test]
async fn test_scan_name_rules_path_key_collision_through_scan() {
    let fix = setup_fixture().await;

    // Try to create NFC and NFD versions of the same file name.
    let nfc_name = "caf\u{00e9}.jpg";
    let nfd_name = "cafe\u{0301}.jpg";

    let p1 = fix.root_path.join(nfc_name);
    let p2 = fix.root_path.join(nfd_name);

    fs::write(&p1, b"nfc photo bytes").expect("write nfc file");

    let create_nfd = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&p2);

    match create_nfd {
        Ok(mut f) => {
            use std::io::Write;
            f.write_all(b"nfd photo bytes").expect("write nfd file");
            drop(f);

            let outcome = scan_root(
                &fix.pool,
                &fix.root_id,
                ScanOptions::default(),
                ScanCancel::new(),
                |_| {},
            )
            .await
            .expect("scan_root");

            let report = outcome.report();
            assert_eq!(
                report.counts.new, 1,
                "First file wins; colliding second file is not counted as new"
            );
            assert!(
                report
                    .warnings
                    .iter()
                    .any(|w| w.code == "path_key_collision"),
                "Second colliding file must produce path_key_collision warning"
            );

            let mut conn = fix.pool.acquire().await.unwrap();
            let rows: Vec<(String,)> = sqlx::query_as("SELECT name FROM files WHERE root_id = ?;")
                .bind(fix.root_id.as_str())
                .fetch_all(&mut *conn)
                .await
                .unwrap();
            assert_eq!(rows.len(), 1, "Only first file remains in catalogue");
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            // Filesystem (such as APFS on macOS or NTFS on Windows) folds NFC/NFD or is case/normalization-insensitive.
            // On such filesystems, the filesystem itself prevents creating two colliding names in the same folder.
            println!(
                "Host filesystem normalization-folds NFC/NFD; skipped live NFC/NFD collision (verified on {}: {})",
                std::env::consts::OS,
                e
            );
        }
        Err(e) => panic!("Unexpected filesystem error: {e}"),
    }
}

// 10. Guard: second scan for same root rejected with ScanAlreadyRunning
#[tokio::test]
async fn test_scan_guard_rejects_concurrent_scan_on_same_root() {
    let fix = setup_fixture().await;

    let _guard = crate::scanner::ScanGuard::acquire(&fix.root_id).expect("acquire guard");

    let err = scan_root(
        &fix.pool,
        &fix.root_id,
        ScanOptions::default(),
        ScanCancel::new(),
        |_| {},
    )
    .await
    .expect_err("concurrent scan must fail");

    assert_eq!(err.code(), "scan_already_running");
    drop(_guard);

    // After guard is dropped, scan works
    let outcome = scan_root(
        &fix.pool,
        &fix.root_id,
        ScanOptions::default(),
        ScanCancel::new(),
        |_| {},
    )
    .await;
    assert!(outcome.is_ok());
}

// 11. Unreachable root and follow_symlinks refusal
#[tokio::test]
async fn test_unreachable_root_and_symlink_refusal() {
    let fix = setup_fixture().await;

    // Delete root directory
    fs::remove_dir_all(&fix.root_path).expect("delete root dir");

    let err = scan_root(
        &fix.pool,
        &fix.root_id,
        ScanOptions::default(),
        ScanCancel::new(),
        |_| {},
    )
    .await
    .expect_err("unreachable root");

    assert_eq!(err.code(), "scan_root_unreachable");
}

// 12. Warnings capped at 100 with warning_count accurate (live walk on Unix)
#[tokio::test]
#[cfg(unix)]
async fn test_warnings_capped_at_100() {
    use std::os::unix::fs::PermissionsExt;
    struct CleanPerms(Vec<PathBuf>);
    impl Drop for CleanPerms {
        fn drop(&mut self) {
            for p in &self.0 {
                let _ = fs::set_permissions(p, fs::Permissions::from_mode(0o755));
            }
        }
    }

    let fix = setup_fixture().await;

    let mut paths = Vec::new();
    for i in 0..120 {
        let dir = fix.root_path.join(format!("locked_{i:03}"));
        fs::create_dir_all(&dir).expect("create dir");
        let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o000));
        paths.push(dir);
    }
    let _guard = CleanPerms(paths);

    let outcome = scan_root(
        &fix.pool,
        &fix.root_id,
        ScanOptions::default(),
        ScanCancel::new(),
        |_| {},
    )
    .await
    .expect("scan_root");

    let rep = outcome.report();
    assert_eq!(rep.warning_count, 120);
    assert_eq!(rep.warnings.len(), 100);
}

// C6 synthetic test: 120 synthetic WalkItem::Warning items yield warning_count == 120 and 100 stored (cross-platform)
#[tokio::test]
async fn test_synthetic_warnings_capped_at_100_via_apply_batch() {
    let fix = setup_fixture().await;

    let mut folder_map = HashMap::new();
    folder_map.insert(
        RelPath::empty(),
        FolderId::from("root_folder_id".to_string()),
    );
    let mut counts = ScanCounts::default();
    let mut warning_count: u64 = 0;
    let mut warnings = Vec::new();

    let mut state = BatchApplyState {
        root_id: &fix.root_id,
        key_policy: PathKeyPolicy {
            case_sensitive: true,
        },
        scan_generation: "gen_test",
        now_ms: 1000,
        folder_map: &mut folder_map,
        counts: &mut counts,
        warning_count: &mut warning_count,
        warnings: &mut warnings,
    };

    let items: Vec<WalkItem> = (0..120)
        .map(|i| {
            WalkItem::Warning(ScanWarning {
                code: "synthetic_warning".to_string(),
                path_display: format!("path_{i}"),
            })
        })
        .collect();

    let mut tx = fix.pool.begin().await.unwrap();
    apply_batch(&mut tx, items, &mut state).await.unwrap();
    tx.commit().await.unwrap();

    assert_eq!(*state.warning_count, 120);
    assert_eq!(state.warnings.len(), 100);
}

// C6 synthetic test: WalkItem::InaccessibleDir for a folder that has rows below it marks the subtree
// inaccessible and final marking does not make any of it missing (cross-platform)
#[tokio::test]
async fn test_synthetic_inaccessible_dir_marks_subtree_inaccessible_and_never_missing() {
    let fix = setup_fixture().await;

    let sub = fix.root_path.join("sub");
    fs::create_dir_all(&sub).expect("create sub");
    fs::write(sub.join("photo_1.jpg"), b"data 1").expect("write 1");
    fs::write(sub.join("photo_2.jpg"), b"data 2").expect("write 2");

    // Initial complete scan: records sub folder and 2 files
    let outcome1 = scan_root(
        &fix.pool,
        &fix.root_id,
        ScanOptions::default(),
        ScanCancel::new(),
        |_| {},
    )
    .await
    .expect("scan 1");
    assert_eq!(outcome1.report().counts.new, 2);

    let mut conn = fix.pool.acquire().await.unwrap();
    let root_folder_id = database::get_root_folder_id(&mut *conn, &fix.root_id)
        .await
        .unwrap();

    // Now simulate a scan where `sub` directory was unreadable (InaccessibleDir)
    let new_generation = "gen_inaccessible_test";
    let mut folder_map = HashMap::new();
    folder_map.insert(RelPath::empty(), root_folder_id);
    let mut counts = ScanCounts::default();
    let mut warning_count: u64 = 0;
    let mut warnings = Vec::new();

    let mut state = BatchApplyState {
        root_id: &fix.root_id,
        key_policy: PathKeyPolicy {
            case_sensitive: true,
        },
        scan_generation: new_generation,
        now_ms: 2000,
        folder_map: &mut folder_map,
        counts: &mut counts,
        warning_count: &mut warning_count,
        warnings: &mut warnings,
    };

    let sub_rel = RelPath::from_components(&["sub"]).unwrap();
    let items = vec![WalkItem::InaccessibleDir {
        rel_path: sub_rel,
        warning: ScanWarning {
            code: "entry_unreadable".to_string(),
            path_display: "sub".to_string(),
        },
    }];

    let mut tx = fix.pool.begin().await.unwrap();
    apply_batch(&mut *tx, items, &mut state).await.unwrap();

    // Run final marking (missing files and folders)
    let (files_missing, folders_missing) =
        database::finalize_scan_marking(&mut tx, &fix.root_id, new_generation, 2000)
            .await
            .unwrap();
    tx.commit().await.unwrap();

    assert_eq!(files_missing, 0, "No files should be marked missing");
    assert_eq!(folders_missing, 0, "No folders should be marked missing");

    // Verify sub folder is inaccessible
    let folder_status: (String,) =
        sqlx::query_as("SELECT status FROM folders WHERE root_id = ? AND name = 'sub';")
            .bind(fix.root_id.as_str())
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_eq!(folder_status.0, "inaccessible");

    // Verify files below sub are inaccessible, NOT missing
    let file_statuses: Vec<(String, String)> =
        sqlx::query_as("SELECT name, status FROM files WHERE root_id = ? ORDER BY name;")
            .bind(fix.root_id.as_str())
            .fetch_all(&mut *conn)
            .await
            .unwrap();
    assert_eq!(file_statuses.len(), 2);
    assert_eq!(
        file_statuses[0],
        ("photo_1.jpg".to_string(), "inaccessible".to_string())
    );
    assert_eq!(
        file_statuses[1],
        ("photo_2.jpg".to_string(), "inaccessible".to_string())
    );
}

// C8 test: synthetic WalkItem::InaccessibleFile marks the file inaccessible in the database (cross-platform)
#[tokio::test]
async fn test_synthetic_inaccessible_file_marked_inaccessible() {
    let fix = setup_fixture().await;

    let photo = fix.root_path.join("locked.jpg");
    fs::write(&photo, b"data").expect("write");

    let outcome1 = scan_root(
        &fix.pool,
        &fix.root_id,
        ScanOptions::default(),
        ScanCancel::new(),
        |_| {},
    )
    .await
    .expect("scan 1");
    assert_eq!(outcome1.report().counts.new, 1);

    let mut conn = fix.pool.acquire().await.unwrap();
    let root_folder_id = database::get_root_folder_id(&mut *conn, &fix.root_id)
        .await
        .unwrap();

    let new_generation = "gen_file_inaccessible";
    let mut folder_map = HashMap::new();
    folder_map.insert(RelPath::empty(), root_folder_id);
    let mut counts = ScanCounts::default();
    let mut warning_count: u64 = 0;
    let mut warnings = Vec::new();

    let mut state = BatchApplyState {
        root_id: &fix.root_id,
        key_policy: PathKeyPolicy {
            case_sensitive: true,
        },
        scan_generation: new_generation,
        now_ms: 2000,
        folder_map: &mut folder_map,
        counts: &mut counts,
        warning_count: &mut warning_count,
        warnings: &mut warnings,
    };

    let photo_rel = RelPath::from_components(&["locked.jpg"]).unwrap();
    let items = vec![WalkItem::InaccessibleFile {
        rel_path: photo_rel,
        warning: ScanWarning {
            code: "entry_unreadable".to_string(),
            path_display: "locked.jpg".to_string(),
        },
    }];

    let mut tx = fix.pool.begin().await.unwrap();
    apply_batch(&mut tx, items, &mut state).await.unwrap();
    tx.commit().await.unwrap();

    assert_eq!(*state.warning_count, 1);
    assert_eq!(state.counts.marked_inaccessible, 1);

    let file_status: (String,) =
        sqlx::query_as("SELECT status FROM files WHERE root_id = ? AND name = 'locked.jpg';")
            .bind(fix.root_id.as_str())
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_eq!(file_status.0, "inaccessible");
}

// 13. Bounded memory: 5,000 files scanned with batch size 500
#[tokio::test]
async fn test_large_tree_5000_files_bounded_memory() {
    let fix = setup_fixture().await;

    // Create 10 subdirectories with 500 files each = 5,000 files
    for folder_idx in 0..10 {
        let dir = fix.root_path.join(format!("folder_{folder_idx:02}"));
        fs::create_dir_all(&dir).expect("create dir");
        for file_idx in 0..500 {
            fs::write(dir.join(format!("img_{file_idx:03}.jpg")), b"x").expect("write file");
        }
    }

    let start = Instant::now();
    let outcome = scan_root(
        &fix.pool,
        &fix.root_id,
        ScanOptions {
            batch_size: 500,
            ..Default::default()
        },
        ScanCancel::new(),
        |_| {},
    )
    .await
    .expect("scan large tree");

    let elapsed = start.elapsed();
    let report = outcome.report();
    assert_eq!(report.counts.new, 5000);
    assert_eq!(report.counts.folders_seen, 10);
    println!(
        "Scanned 5,000 files in {elapsed:?} ({:.1} files/s)",
        5000.0 / elapsed.as_secs_f64()
    );
}

// 14. Part D: real-folder report (owner runs on real photos)
#[tokio::test]
#[ignore]
async fn scan_report() {
    let scan_path_str = match std::env::var("PM_SCAN_PATH") {
        Ok(p) => p,
        Err(_) => {
            eprintln!("PM_SCAN_PATH environment variable not set; skipping scan_report");
            return;
        }
    };
    let scan_path = PathBuf::from(&scan_path_str);
    if !scan_path.is_dir() {
        eprintln!("PM_SCAN_PATH '{scan_path_str}' is not a directory; skipping");
        return;
    }

    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join("catalogue.db");
    let app_data = temp.path().join("app_data");
    fs::create_dir_all(&app_data).expect("create app_data");

    let pool = init_pool(&db_path).await.expect("init_pool");
    let ctx = test_context(&app_data);
    let dialog_guard = DialogGuard::default();

    let target = scan_path.clone();
    let add_res = add_root_with_picker(&pool, &ctx, &dialog_guard, None, move || {
        let t = target.clone();
        async move { Some(t) }
    })
    .await
    .expect("add_root_with_picker");

    let root_id = RootId::from(add_res.root_id.expect("root_id"));

    println!("=== PASS 1 (Initial Scan) ===");
    let start1 = Instant::now();
    let outcome1 = scan_root(
        &pool,
        &root_id,
        ScanOptions::default(),
        ScanCancel::new(),
        |_| {},
    )
    .await
    .expect("pass 1 scan");
    let dur1 = start1.elapsed();
    let rep1 = outcome1.report();
    let fps1 = rep1.counts.files_seen as f64 / dur1.as_secs_f64().max(0.0001);

    println!("Elapsed: {:?}", dur1);
    println!("Files/s: {:.1}", fps1);
    println!("Counts: {:?}", rep1.counts);
    println!("Warnings (total: {}):", rep1.warning_count);
    for w in &rep1.warnings {
        println!("  - [{}]: {}", w.code, w.path_display);
    }

    println!("\n=== PASS 2 (Unchanged Rescan) ===");
    let start2 = Instant::now();
    let outcome2 = scan_root(
        &pool,
        &root_id,
        ScanOptions::default(),
        ScanCancel::new(),
        |_| {},
    )
    .await
    .expect("pass 2 scan");
    let dur2 = start2.elapsed();
    let rep2 = outcome2.report();
    let fps2 = rep2.counts.files_seen as f64 / dur2.as_secs_f64().max(0.0001);

    println!("Elapsed: {:?}", dur2);
    println!("Files/s: {:.1}", fps2);
    println!("Counts: {:?}", rep2.counts);

    let mut conn = pool.acquire().await.unwrap();
    let status_counts: Vec<(String, i64)> =
        sqlx::query_as("SELECT status, COUNT(*) FROM files WHERE root_id = ? GROUP BY status;")
            .bind(root_id.as_str())
            .fetch_all(&mut *conn)
            .await
            .unwrap();

    println!("\n=== ROWS PER STATUS ===");
    for (status, count) in status_counts {
        println!("  {status}: {count}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn test_cancellation_deadlock_prevention() {
    let fix = setup_fixture().await;

    // A tree of at least 300 files
    for i in 0..300 {
        fs::write(
            fix.root_path.join(format!("photo_{i:03}.jpg")),
            b"test image content",
        )
        .expect("write photo");
    }

    let cancel = ScanCancel::new();
    let cancel_clone = cancel.clone();
    let batch_count = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let batch_count_clone = batch_count.clone();

    // Progress callback sleeps 20ms and cancels after the first batch
    let scan_fut = scan_root(
        &fix.pool,
        &fix.root_id,
        ScanOptions {
            batch_size: 1,
            ..Default::default()
        },
        cancel,
        move |_p| {
            let n = batch_count_clone.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                std::thread::sleep(Duration::from_millis(20));
                cancel_clone.cancel();
            }
        },
    );

    let res = tokio::time::timeout(Duration::from_secs(10), scan_fut).await;
    let outcome = res
        .expect("scan_root timed out: deadlock occurred on cancellation")
        .expect("scan_root failed");
    assert!(matches!(outcome, ScanOutcome::Cancelled(_)));

    // Second scan_root for the same root starts (the guard was released)
    let outcome2 = scan_root(
        &fix.pool,
        &fix.root_id,
        ScanOptions::default(),
        ScanCancel::new(),
        |_| {},
    )
    .await
    .expect("second scan should start cleanly because guard was released");
    assert!(matches!(outcome2, ScanOutcome::Completed(_)));
}

#[test]
fn test_warning_codes_sync_with_typescript_and_scanner_sources() {
    use crate::scanner::ALL_WARNING_CODES;

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root_dir = manifest_dir.parent().expect("workspace root");
    let ts_path = root_dir.join("src").join("lib").join("scanText.ts");

    assert!(
        ts_path.exists(),
        "TypeScript scanText.ts file must exist at {:?}",
        ts_path
    );

    let ts_content = std::fs::read_to_string(&ts_path).expect("read scanText.ts");

    let mut ts_keys = std::collections::HashSet::new();
    let dict_section = ts_content
        .split("SCAN_WARNING_MESSAGES: Record<string, string> = {")
        .nth(1)
        .expect("find dictionary start")
        .split("};")
        .next()
        .expect("find dictionary end");

    for line in dict_section.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }
        if let Some((key, _)) = trimmed.split_once(':') {
            let clean_key = key.trim().trim_matches(|c| c == '\'' || c == '"');
            if !clean_key.is_empty() {
                ts_keys.insert(clean_key.to_string());
            }
        }
    }

    let rust_codes: std::collections::HashSet<&str> = ALL_WARNING_CODES.iter().copied().collect();
    assert_eq!(
        rust_codes.len(),
        ALL_WARNING_CODES.len(),
        "No duplicate codes in ALL_WARNING_CODES"
    );

    for &code in ALL_WARNING_CODES {
        assert!(
            ts_keys.contains(code),
            "Warning code '{code}' in Rust ALL_WARNING_CODES is missing from TypeScript SCAN_WARNING_MESSAGES"
        );
    }
    for ts_key in &ts_keys {
        assert!(
            rust_codes.contains(ts_key.as_str()),
            "TypeScript warning key '{ts_key}' is missing from Rust ALL_WARNING_CODES"
        );
    }

    let walk_src =
        std::fs::read_to_string(manifest_dir.join("src").join("scanner").join("walk.rs"))
            .expect("read walk.rs");
    let mod_src = std::fs::read_to_string(manifest_dir.join("src").join("scanner").join("mod.rs"))
        .expect("read mod.rs");

    for (file_name, content) in [("walk.rs", walk_src), ("mod.rs", mod_src)] {
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("code:") {
                if let Some(first_quote) = trimmed.find('"') {
                    if let Some(second_quote) = trimmed[first_quote + 1..].find('"') {
                        let literal_code =
                            &trimmed[first_quote + 1..first_quote + 1 + second_quote];
                        assert!(
                            rust_codes.contains(literal_code),
                            "Literal warning code '{literal_code}' in {file_name} is missing from ALL_WARNING_CODES"
                        );
                    }
                }
            }
        }
    }
}

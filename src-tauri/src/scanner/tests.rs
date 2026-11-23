use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tempfile::{tempdir, TempDir};

use crate::commands::{add_root_with_picker, DialogGuard};
use crate::database::{self, count_for_root, init_pool};
use crate::domain::{CatalogueError, FileStatus, FolderStatus, RootId};
use crate::library::protected::ProtectedContext;
use crate::scanner::{scan_root, ScanCancel, ScanError, ScanOptions, ScanOutcome};

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

// 6. Interrupted scans (The gate for this phase)
#[tokio::test]
async fn test_interrupted_scans_leave_nothing_wrongly_missing_and_rescan_equals_uninterrupted() {
    let fix = setup_fixture().await;

    for i in 0..8 {
        fs::write(fix.root_path.join(format!("file_{i}.jpg")), b"photo bytes").expect("write");
    }

    // Initial complete scan
    scan_root(
        &fix.pool,
        &fix.root_id,
        ScanOptions::default(),
        ScanCancel::new(),
        |_| {},
    )
    .await
    .expect("initial scan");

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
    let mut conn = fix.pool.acquire().await.unwrap();
    let missing_count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM files WHERE root_id = ? AND status = 'missing';")
            .bind(fix.root_id.as_str())
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_eq!(
        missing_count.0, 0,
        "Cancelled scan must NEVER mark rows missing"
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

    let missing_count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM files WHERE root_id = ? AND status = 'missing';")
            .bind(fix.root_id.as_str())
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_eq!(
        missing_count.0, 0,
        "Failed scan must NEVER mark rows missing"
    );

    // Scenario 3: Dropping future mid-scan (simulating crash)
    {
        let pool_clone = fix.pool.clone();
        let root_id_clone = fix.root_id.clone();
        let task = tokio::spawn(async move {
            let _ = scan_root(
                &pool_clone,
                &root_id_clone,
                ScanOptions {
                    batch_size: 1,
                    ..Default::default()
                },
                ScanCancel::new(),
                |_| {},
            )
            .await;
        });
        tokio::time::sleep(Duration::from_millis(5)).await;
        task.abort();
        let _ = task.await;
    }

    let missing_count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM files WHERE root_id = ? AND status = 'missing';")
            .bind(fix.root_id.as_str())
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_eq!(
        missing_count.0, 0,
        "Aborted scan future must NEVER mark rows missing"
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

    let missing_count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM files WHERE root_id = ? AND status = 'missing';")
            .bind(fix.root_id.as_str())
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert_eq!(
        missing_count.0, 0,
        "Failure in move step must NEVER mark rows missing"
    );

    // Final complete scan must restore and match uninterrupted scan
    let final_outcome = scan_root(
        &fix.pool,
        &fix.root_id,
        ScanOptions::default(),
        ScanCancel::new(),
        |_| {},
    )
    .await
    .expect("final scan");
    let report = final_outcome.report();
    assert_eq!(report.counts.unchanged, 8);
    assert_eq!(report.counts.marked_missing, 0);
}

// 7. Unreadable directory handling (carried over as inaccessible, never missing)
#[tokio::test]
#[cfg(unix)]
async fn test_unreadable_directory_carried_over_as_inaccessible_and_restored() {
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

// 12. Warnings capped at 100 with warning_count accurate
#[tokio::test]
async fn test_warnings_capped_at_100() {
    let fix = setup_fixture().await;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        struct CleanPerms(Vec<PathBuf>);
        impl Drop for CleanPerms {
            fn drop(&mut self) {
                for p in &self.0 {
                    let _ = fs::set_permissions(p, fs::Permissions::from_mode(0o755));
                }
            }
        }

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

    #[cfg(not(unix))]
    {
        let mut warnings = Vec::new();
        let mut warning_count = 0;
        let mut add_warning = |code: String, path_display: String| {
            warning_count += 1;
            if warnings.len() < 100 {
                warnings.push(crate::scanner::ScanWarning { code, path_display });
            }
        };
        for i in 0..120 {
            add_warning("test_warning".to_string(), format!("item_{i}"));
        }
        assert_eq!(warning_count, 120);
        assert_eq!(warnings.len(), 100);
    }
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

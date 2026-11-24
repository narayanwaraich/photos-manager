use sqlx::SqlitePool;
use std::path::Path;

use crate::database;
use crate::database::models::{FolderRow, RootRow, VolumeRow};
use crate::domain::{
    AddRootOutcome, CaseProbe, CatalogueError, FolderId, FolderStatus, RemoveRootPreview,
    RootError, RootId, RootStatus, RootSummary, RootWarning, VolumeId, VolumeInfo, VolumeKind,
};
use crate::library::canonical::canonicalize_root;
use crate::library::case_probe::{
    probe_case_sensitivity, FileIdentityLookup, RealFileIdentityLookup,
};
use crate::library::overlap::check_overlap;
use crate::library::protected::{check_protected, ProtectedContext};
use crate::library::volume::describe_volume;

/// Maximum length of a library root label (in characters).
pub const MAX_LABEL_LENGTH: usize = 255;

/// Registers a new library root directory into the catalogue (spec §3.5, D29).
///
/// Execution is fully atomic within a single write transaction (BEGIN IMMEDIATE):
/// 1. Path is canonicalized, resolved, and verified as Unicode.
/// 2. Protected folder rules are checked.
/// 3. Volume details are obtained and case sensitivity is probed.
/// 4. Atomic write transaction begins (`BEGIN IMMEDIATE`), acquiring the SQLite write lock upfront.
/// 5. Overlap is checked against all stored roots under the transaction write lock.
/// 6. Volume row is reused if a matching non-null `fs_uuid` exists, or inserted.
/// 7. The `library_roots` entry is created with default status (`active`).
/// 8. The corresponding root-folder row (`parent_id = NULL`) is created.
///
/// Any failure rolls back completely, leaving zero rows behind.
pub async fn add_root(
    pool: &SqlitePool,
    ctx: &ProtectedContext,
    path: &Path,
    label: Option<&str>,
) -> Result<AddRootOutcome, RootError> {
    add_root_with_deps(
        pool,
        ctx,
        path,
        label,
        describe_volume,
        &RealFileIdentityLookup,
    )
    .await
}

/// Parameterized version of `add_root` allowing injection of volume describer and case lookup for testing.
pub async fn add_root_with_deps<D, L>(
    pool: &SqlitePool,
    ctx: &ProtectedContext,
    path: &Path,
    label: Option<&str>,
    describer: D,
    lookup: &L,
) -> Result<AddRootOutcome, RootError>
where
    D: Fn(&Path) -> Result<VolumeInfo, RootError>,
    L: FileIdentityLookup,
{
    // 1. Canonicalize candidate root
    let canon = canonicalize_root(path)?;

    // 2. Validate protected folder rules
    check_protected(&canon, ctx)?;

    // 3. Determine and validate root label (bounded length)
    let raw_label = label
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| canon.last_component());

    let final_label = if raw_label.is_empty() {
        canon.path_display.as_str()
    } else {
        raw_label
    };

    if final_label.chars().count() > MAX_LABEL_LENGTH {
        return Err(RootError::LabelInvalid {
            label: final_label.to_string(),
            reason: "Label exceeds maximum length of 255 characters",
        });
    }

    // 4. Describe volume and probe case sensitivity
    let vol_info = describer(&canon.canonical_path)?;
    let probe = probe_case_sensitivity(&canon.canonical_path, lookup);
    let (case_sensitive, is_unknown) = match probe {
        CaseProbe::CaseSensitive => (true, false),
        CaseProbe::CaseInsensitive => (false, false),
        CaseProbe::Unknown => (false, true),
    };

    let mut warnings = Vec::new();
    if is_unknown {
        warnings.push(RootWarning::CaseSensitivityUnknown);
    }
    if vol_info.kind == VolumeKind::Network {
        warnings.push(RootWarning::NetworkVolume);
    }

    // 5. Atomic write transaction (BEGIN IMMEDIATE)
    let mut tx = pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(CatalogueError::Database)?;

    // Check overlap under the transaction lock
    let stored_roots = database::roots::list_roots_for_overlap(&mut tx).await?;
    check_overlap(&canon, &stored_roots)?;

    let now_ms = database::now_ms()?;

    // 6. Find or insert volume record
    let volume_id = if let Some(ref uuid) = vol_info.fs_uuid {
        if let Some(existing_vol) = database::volumes::find_volume_by_uuid(&mut tx, uuid).await? {
            database::volumes::update_volume_label_and_seen(
                &mut tx,
                &existing_vol.id,
                vol_info.label.as_deref(),
                now_ms,
            )
            .await?;
            existing_vol.id
        } else {
            let new_id = VolumeId::new();
            let vol_row = VolumeRow {
                id: new_id.clone(),
                kind: vol_info.kind,
                label: vol_info.label,
                fs_uuid: Some(uuid.clone()),
                fs_type: vol_info.fs_type,
                case_sensitive,
                last_seen_at_ms: Some(now_ms),
            };
            database::volumes::insert_volume(&mut tx, &vol_row).await?;
            new_id
        }
    } else {
        let new_id = VolumeId::new();
        let vol_row = VolumeRow {
            id: new_id.clone(),
            kind: vol_info.kind,
            label: vol_info.label,
            fs_uuid: None,
            fs_type: vol_info.fs_type,
            case_sensitive,
            last_seen_at_ms: Some(now_ms),
        };
        database::volumes::insert_volume(&mut tx, &vol_row).await?;
        new_id
    };

    // 7. Insert library_roots row
    let root_id = RootId::new();
    let root_row = RootRow {
        id: root_id.clone(),
        volume_id: Some(volume_id.clone()),
        label: final_label.to_string(),
        path_display: canon.path_display.clone(),
        path_native: canon.path_native.clone(),
        status: RootStatus::Active,
        follow_symlinks: false,
        include_hidden: false,
        created_at_ms: now_ms,
        updated_at_ms: now_ms,
        last_scan_at_ms: None,
        last_scan_generation: None,
    };
    database::roots::insert_root(&mut tx, &root_row).await?;

    // 8. Insert root-folder row (parent_id = NULL, empty relative path and key)
    let folder_row = FolderRow {
        id: FolderId::new(),
        root_id: root_id.clone(),
        parent_id: None,
        name: String::new(),
        rel_path_display: String::new(),
        rel_path_native: Vec::new(),
        path_key: Vec::new(),
        status: FolderStatus::Present,
        modified_at_ms: Some(now_ms),
        last_seen_generation: None,
    };
    database::folders::insert_folder(&mut tx, &folder_row).await?;

    // Commit transaction
    tx.commit().await.map_err(CatalogueError::Database)?;

    Ok(AddRootOutcome {
        root_id,
        volume_id,
        warnings,
    })
}

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Default timeout for live reachable status checks (Decision d: 2 seconds).
pub const DEFAULT_REACHABLE_TIMEOUT: Duration = Duration::from_secs(2);

/// RAII guard that removes a `RootId` from the active in-flight set when the blocking
/// inspection thread exits (or panics).
struct InFlightCheckGuard {
    set: Arc<Mutex<HashSet<RootId>>>,
    id: RootId,
}

impl Drop for InFlightCheckGuard {
    fn drop(&mut self) {
        if let Ok(mut set) = self.set.lock() {
            set.remove(&self.id);
        }
    }
}

/// Lists all registered library roots with volume summaries, live reachable status, and counts (D29).
///
/// Bounded by construction (user library folders); not paginated (spec §12.1).
/// Uses a process-wide default in-flight tracker and the 2-second timeout.
pub async fn list_roots(pool: &SqlitePool) -> Result<Vec<RootSummary>, RootError> {
    static DEFAULT_IN_FLIGHT: std::sync::OnceLock<Arc<Mutex<HashSet<RootId>>>> =
        std::sync::OnceLock::new();
    let in_flight = DEFAULT_IN_FLIGHT.get_or_init(|| Arc::new(Mutex::new(HashSet::new())));
    list_roots_with_in_flight(pool, in_flight).await
}

/// Variant of `list_roots` that accepts the shared in-flight check set (e.g. from Tauri managed state).
pub async fn list_roots_with_in_flight(
    pool: &SqlitePool,
    in_flight: &Arc<Mutex<HashSet<RootId>>>,
) -> Result<Vec<RootSummary>, RootError> {
    list_roots_with_checker_and_timeout(pool, in_flight, DEFAULT_REACHABLE_TIMEOUT, |p| p.is_dir())
        .await
}

/// Fully parameterized version of `list_roots` allowing custom in-flight set, timeout,
/// and checker function for unit tests and failure injection (Decision d).
pub async fn list_roots_with_checker_and_timeout<F>(
    pool: &SqlitePool,
    in_flight: &Arc<Mutex<HashSet<RootId>>>,
    timeout: Duration,
    check_fn: F,
) -> Result<Vec<RootSummary>, RootError>
where
    F: Fn(&Path) -> bool + Send + Sync + Clone + 'static,
{
    let mut conn = pool.acquire().await.map_err(CatalogueError::Database)?;
    let records = database::roots::list_roots_summaries(&mut conn).await?;

    let mut check_tasks = Vec::with_capacity(records.len());

    for r in &records {
        let root_id = r.id.clone();
        let native_path = crate::filesystem::blob_to_native(&r.path_native);

        let already_in_flight = {
            let mut set = in_flight.lock().map_err(|_| {
                CatalogueError::Database(sqlx::Error::Configuration(
                    "Active reachable checks mutex poisoned".into(),
                ))
            })?;
            if set.contains(&root_id) {
                true
            } else {
                set.insert(root_id.clone());
                false
            }
        };

        if already_in_flight {
            // Check is already running for this root; do not spawn a second thread.
            // Reports false without starting another blocking thread (Decision d).
            check_tasks.push(tokio::spawn(async move { false }));
        } else {
            let set_clone = Arc::clone(in_flight);
            let id_clone = root_id.clone();
            let checker = check_fn.clone();
            let timeout_dur = timeout;

            let task = tokio::spawn(async move {
                let blocking_handle = tokio::task::spawn_blocking(move || {
                    let _guard = InFlightCheckGuard {
                        set: set_clone,
                        id: id_clone,
                    };
                    checker(&native_path)
                });

                match tokio::time::timeout(timeout_dur, blocking_handle).await {
                    Ok(Ok(is_dir)) => is_dir,
                    _ => false, // timeout, panic, or JoinError
                }
            });

            check_tasks.push(task);
        }
    }

    let mut summaries = Vec::with_capacity(records.len());
    for (r, task) in records.into_iter().zip(check_tasks) {
        let reachable = task.await.unwrap_or(false);

        summaries.push(RootSummary {
            id: r.id,
            label: r.label,
            path_display: r.path_display,
            volume_kind: r.volume_kind,
            volume_label: r.volume_label,
            volume_case_sensitive: r.volume_case_sensitive,
            status: r.status,
            reachable,
            file_count: r.file_count,
            folder_count: r.folder_count,
            last_scan_at_ms: r.last_scan_at_ms,
            last_job: r.last_job,
            active_job_id: None,
        });
    }

    Ok(summaries)
}

/// Retrieves item counts for a library root prior to removal (spec §5.2, D29g).
pub async fn preview_remove_root(
    pool: &SqlitePool,
    root_id: &RootId,
) -> Result<RemoveRootPreview, RootError> {
    let mut conn = pool.acquire().await.map_err(CatalogueError::Database)?;

    let root_opt = database::roots::get_root(&mut conn, root_id).await?;
    if root_opt.is_none() {
        return Err(RootError::NotFound {
            id: root_id.to_string(),
        });
    }

    let file_count = database::roots::count_files(&mut conn, root_id).await?;
    let folder_count = database::roots::count_folders(&mut conn, root_id).await?;
    let asset_count = database::roots::count_assets(&mut conn, root_id).await?;

    Ok(RemoveRootPreview {
        root_id: root_id.clone(),
        file_count,
        folder_count,
        asset_count,
    })
}

/// Deletes a library root from the catalogue after verifying current file count (decision g, D29g).
///
/// Takes the SQLite write lock immediately (`BEGIN IMMEDIATE`) so that count verification
/// and deletion are strictly serialised against concurrent modifications.
/// Removes catalogue rows only via SQLite CASCADE. Never touches user files on disk.
/// The associated volume row is preserved for future reuse.
pub async fn remove_root(
    pool: &SqlitePool,
    root_id: &RootId,
    expected_files: i64,
) -> Result<(), RootError> {
    let mut tx = pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(CatalogueError::Database)?;

    let root_opt = database::roots::get_root(&mut tx, root_id).await?;
    if root_opt.is_none() {
        return Err(RootError::NotFound {
            id: root_id.to_string(),
        });
    }

    let current_files = database::roots::count_files(&mut tx, root_id).await?;
    if current_files != expected_files {
        return Err(RootError::ChangedSincePreview {
            expected: expected_files,
            actual: current_files,
        });
    }

    database::roots::delete_root(&mut tx, root_id).await?;
    tx.commit().await.map_err(CatalogueError::Database)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::init_pool;
    use crate::library::FileIdentity;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use tempfile::tempdir;

    struct TestLookup(HashMap<PathBuf, FileIdentity>);

    impl FileIdentityLookup for TestLookup {
        fn get_identity(&self, path: &Path) -> Result<Option<FileIdentity>, std::io::Error> {
            Ok(self.0.get(path).copied())
        }
    }

    fn test_context(_app_data: &Path) -> ProtectedContext {
        ProtectedContext::synthetic_unix(
            &["tmp", "test_app_data"],
            &["users", "alice"],
            &["users"],
            &["volumes"],
            &[&["system"]],
        )
    }

    #[tokio::test]
    async fn test_add_root_basic_success() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("catalogue.db");
        let pool = init_pool(&db_path).await.expect("init_pool");

        let photos_dir = temp.path().join("Photos");
        std::fs::create_dir_all(&photos_dir).expect("create dir");

        let ctx = test_context(temp.path());
        let outcome = add_root(&pool, &ctx, &photos_dir, Some("My Photos"))
            .await
            .expect("add_root");

        let roots = list_roots(&pool).await.expect("list_roots");
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].id, outcome.root_id);
        assert_eq!(roots[0].label, "My Photos");
        assert!(roots[0].reachable);
        assert_eq!(roots[0].file_count, 0);
        assert_eq!(roots[0].folder_count, 0);
    }

    #[tokio::test]
    async fn test_add_two_roots_on_same_volume_reuses_volume_row() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("catalogue.db");
        let pool = init_pool(&db_path).await.expect("init_pool");

        let dir1 = temp.path().join("Folder1");
        let dir2 = temp.path().join("Folder2");
        std::fs::create_dir_all(&dir1).expect("create dir1");
        std::fs::create_dir_all(&dir2).expect("create dir2");

        let ctx = test_context(temp.path());
        let fake_describer = |_p: &Path| -> Result<VolumeInfo, RootError> {
            Ok(VolumeInfo {
                kind: VolumeKind::Internal,
                label: Some("Shared Disk".to_string()),
                fs_uuid: Some("uuid-shared-1234".to_string()),
                fs_type: Some("apfs".to_string()),
            })
        };
        let lookup = TestLookup(HashMap::new());

        let out1 = add_root_with_deps(&pool, &ctx, &dir1, None, fake_describer, &lookup)
            .await
            .expect("add root 1");
        let out2 = add_root_with_deps(&pool, &ctx, &dir2, None, fake_describer, &lookup)
            .await
            .expect("add root 2");

        assert_eq!(
            out1.volume_id, out2.volume_id,
            "Roots on same volume with identical fs_uuid must share volume_id"
        );

        let mut conn = pool.acquire().await.unwrap();
        let volumes = database::volumes::list_volumes(&mut *conn).await.unwrap();
        assert_eq!(volumes.len(), 1, "Only one volume row must exist");
    }

    #[tokio::test]
    async fn test_add_two_roots_with_different_uuid_creates_two_volume_rows() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("catalogue.db");
        let pool = init_pool(&db_path).await.expect("init_pool");

        let dir1 = temp.path().join("FolderA");
        let dir2 = temp.path().join("FolderB");
        std::fs::create_dir_all(&dir1).expect("create dir1");
        std::fs::create_dir_all(&dir2).expect("create dir2");

        let ctx = test_context(temp.path());
        let lookup = TestLookup(HashMap::new());

        let describer1 = |_p: &Path| -> Result<VolumeInfo, RootError> {
            Ok(VolumeInfo {
                kind: VolumeKind::Internal,
                label: Some("Disk A".to_string()),
                fs_uuid: Some("uuid-disk-AAAA".to_string()),
                fs_type: Some("apfs".to_string()),
            })
        };
        let describer2 = |_p: &Path| -> Result<VolumeInfo, RootError> {
            Ok(VolumeInfo {
                kind: VolumeKind::Removable,
                label: Some("Disk B".to_string()),
                fs_uuid: Some("uuid-disk-BBBB".to_string()),
                fs_type: Some("exfat".to_string()),
            })
        };

        let out1 = add_root_with_deps(&pool, &ctx, &dir1, None, describer1, &lookup)
            .await
            .expect("add 1");
        let out2 = add_root_with_deps(&pool, &ctx, &dir2, None, describer2, &lookup)
            .await
            .expect("add 2");

        assert_ne!(out1.volume_id, out2.volume_id);

        let mut conn = pool.acquire().await.unwrap();
        let volumes = database::volumes::list_volumes(&mut *conn).await.unwrap();
        assert_eq!(volumes.len(), 2);
    }

    #[tokio::test]
    async fn test_add_two_roots_with_null_uuid_creates_two_volume_rows() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("catalogue.db");
        let pool = init_pool(&db_path).await.expect("init_pool");

        let dir1 = temp.path().join("NullUUID1");
        let dir2 = temp.path().join("NullUUID2");
        std::fs::create_dir_all(&dir1).expect("create dir1");
        std::fs::create_dir_all(&dir2).expect("create dir2");

        let ctx = test_context(temp.path());
        let lookup = TestLookup(HashMap::new());

        let describer_null = |_p: &Path| -> Result<VolumeInfo, RootError> {
            Ok(VolumeInfo {
                kind: VolumeKind::Unknown,
                label: None,
                fs_uuid: None,
                fs_type: None,
            })
        };

        let out1 = add_root_with_deps(&pool, &ctx, &dir1, None, describer_null, &lookup)
            .await
            .expect("add 1");
        let out2 = add_root_with_deps(&pool, &ctx, &dir2, None, describer_null, &lookup)
            .await
            .expect("add 2");

        assert_ne!(out1.volume_id, out2.volume_id);

        let mut conn = pool.acquire().await.unwrap();
        let volumes = database::volumes::list_volumes(&mut *conn).await.unwrap();
        assert_eq!(
            volumes.len(),
            2,
            "Null fs_uuid must result in distinct volume rows"
        );
    }

    #[tokio::test]
    async fn test_overlap_and_protected_refusals_leave_zero_rows() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("catalogue.db");
        let pool = init_pool(&db_path).await.expect("init_pool");

        let dir = temp.path().join("Photos");
        std::fs::create_dir_all(&dir).expect("create dir");

        let ctx = test_context(temp.path());
        let _ = add_root(&pool, &ctx, &dir, None)
            .await
            .expect("initial root");

        // Attempting to add exact duplicate -> overlap refusal
        let dup_res = add_root(&pool, &ctx, &dir, None).await;
        assert!(matches!(dup_res, Err(RootError::Overlaps { .. })));

        // Verify count of roots is still 1
        let roots = list_roots(&pool).await.expect("list_roots");
        assert_eq!(roots.len(), 1);

        // Attempting to add a protected folder -> protected refusal
        let protected_ctx = ProtectedContext::synthetic_unix(
            &["tmp", "catalogue"],
            &["users", "alice"],
            &["users"],
            &["volumes"],
            &[&["system"]],
        );
        let sub_dir = temp.path().join("Photos").join("Sub");
        std::fs::create_dir_all(&sub_dir).expect("create sub dir");

        let overlap_sub = add_root(&pool, &protected_ctx, &sub_dir, None).await;
        assert!(matches!(overlap_sub, Err(RootError::Overlaps { .. })));

        let roots_after = list_roots(&pool).await.expect("list_roots");
        assert_eq!(roots_after.len(), 1);
    }

    #[tokio::test]
    async fn test_add_root_rolls_back_completely_on_forced_failure() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("catalogue.db");
        let pool = init_pool(&db_path).await.expect("init_pool");

        let dir = temp.path().join("RollbackDir");
        std::fs::create_dir_all(&dir).expect("create dir");

        let ctx = test_context(temp.path());

        // We can simulate an error during root folder insert by temporarily creating a trigger
        // or inserting a conflicting folder. Let's create an BEFORE INSERT trigger on folders
        // that fails conditionally:
        sqlx::query("CREATE TRIGGER fail_folder_insert BEFORE INSERT ON folders BEGIN SELECT RAISE(FAIL, 'simulated failure'); END;")
            .execute(&pool)
            .await
            .expect("create trigger");

        let res = add_root(&pool, &ctx, &dir, None).await;
        assert!(res.is_err(), "Must fail due to trigger");

        // Verify that zero roots, zero folders, and zero volumes remain
        let mut conn = pool.acquire().await.unwrap();
        let roots = database::roots::list_roots(&mut *conn).await.unwrap();
        assert_eq!(roots.len(), 0, "Roots must be rolled back");
        let volumes = database::volumes::list_volumes(&mut *conn).await.unwrap();
        assert_eq!(volumes.len(), 0, "Volumes must be rolled back");
        let folders: (i64,) = sqlx::query_as("SELECT count(*) FROM folders;")
            .fetch_one(&mut *conn)
            .await
            .unwrap();
        assert_eq!(folders.0, 0, "Folders must be rolled back");
    }

    #[tokio::test]
    async fn test_list_roots_counts_with_hand_inserted_tree() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("catalogue.db");
        let pool = init_pool(&db_path).await.expect("init_pool");

        let dir = temp.path().join("CountsTree");
        std::fs::create_dir_all(&dir).expect("create dir");

        let ctx = test_context(temp.path());
        let outcome = add_root(&pool, &ctx, &dir, None).await.expect("add root");

        // Hand-insert 1 subfolder and 2 files
        let mut conn = pool.acquire().await.unwrap();
        let root_folder_id: (String,) =
            sqlx::query_as("SELECT id FROM folders WHERE root_id = ? AND parent_id IS NULL;")
                .bind(outcome.root_id.as_str())
                .fetch_one(&mut *conn)
                .await
                .unwrap();

        let subfolder_id = FolderId::new();
        sqlx::query(
            "INSERT INTO folders (id, root_id, parent_id, name, rel_path_display, rel_path_native, path_key, status)
             VALUES (?, ?, ?, 'Sub', 'Sub', X'537562', X'737562', 'present');"
        )
        .bind(subfolder_id.as_str())
        .bind(outcome.root_id.as_str())
        .bind(&root_folder_id.0)
        .execute(&mut *conn)
        .await
        .unwrap();

        for i in 1..=2 {
            let file_id = format!("file_{i}");
            let name = format!("photo{i}.jpg");
            let key = vec![i as u8];
            sqlx::query(
                "INSERT INTO files (
                    id, root_id, folder_id, name, extension, rel_path_display, rel_path_native,
                    path_key, size_bytes, modified_at_ms, status, created_in_catalog_at_ms, updated_at_ms
                 ) VALUES (?, ?, ?, ?, 'jpg', ?, ?, ?, 1024, 1000, 'present', 1000, 1000);"
            )
            .bind(&file_id)
            .bind(outcome.root_id.as_str())
            .bind(subfolder_id.as_str())
            .bind(&name)
            .bind(&name)
            .bind(&key)
            .bind(&key)
            .execute(&mut *conn)
            .await
            .unwrap();
        }

        let roots = list_roots(&pool).await.expect("list_roots");
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].file_count, 2);
        assert_eq!(roots[0].folder_count, 1);
    }

    #[tokio::test]
    async fn test_list_roots_reachable_false_after_directory_removed() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("catalogue.db");
        let pool = init_pool(&db_path).await.expect("init_pool");

        let dir = temp.path().join("WillBeDeleted");
        std::fs::create_dir_all(&dir).expect("create dir");

        let ctx = test_context(temp.path());
        let _ = add_root(&pool, &ctx, &dir, None).await.expect("add root");

        let roots_before = list_roots(&pool).await.expect("list_roots");
        assert!(roots_before[0].reachable);
        assert_eq!(roots_before[0].status, RootStatus::Active);

        // Remove the directory from disk
        std::fs::remove_dir(&dir).expect("remove dir");

        let roots_after = list_roots(&pool).await.expect("list_roots");
        assert!(
            !roots_after[0].reachable,
            "Reachable must be false when directory is gone"
        );
        assert_eq!(
            roots_after[0].status,
            RootStatus::Active,
            "Stored status must remain active (decision h)"
        );
    }

    #[tokio::test]
    async fn test_remove_root_with_right_count_and_leaves_disk_untouched() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("catalogue.db");
        let pool = init_pool(&db_path).await.expect("init_pool");

        let dir1 = temp.path().join("Dir1");
        let dir2 = temp.path().join("Dir2");
        std::fs::create_dir_all(&dir1).expect("create dir1");
        std::fs::create_dir_all(&dir2).expect("create dir2");

        // Write sample files on disk in dir1
        let file1_disk = dir1.join("file1.jpg");
        let file1_bytes = b"sample image content 12345";
        std::fs::write(&file1_disk, file1_bytes).expect("write file");

        let ctx = test_context(temp.path());
        let out1 = add_root(&pool, &ctx, &dir1, None)
            .await
            .expect("add root 1");
        let out2 = add_root(&pool, &ctx, &dir2, None)
            .await
            .expect("add root 2");

        // Add 1 file row to root 1
        let mut conn = pool.acquire().await.unwrap();
        let folder1: (String,) = sqlx::query_as("SELECT id FROM folders WHERE root_id = ?;")
            .bind(out1.root_id.as_str())
            .fetch_one(&mut *conn)
            .await
            .unwrap();

        sqlx::query(
            "INSERT INTO files (
                id, root_id, folder_id, name, extension, rel_path_display, rel_path_native,
                path_key, size_bytes, modified_at_ms, status, created_in_catalog_at_ms, updated_at_ms
             ) VALUES ('f1', ?, ?, 'file1.jpg', 'jpg', 'file1.jpg', X'00', X'00', 26, 1000, 'present', 1000, 1000);"
        )
        .bind(out1.root_id.as_str())
        .bind(&folder1.0)
        .execute(&mut *conn)
        .await
        .unwrap();

        // Check preview
        let preview = preview_remove_root(&pool, &out1.root_id)
            .await
            .expect("preview");
        assert_eq!(preview.file_count, 1);

        // Stale count fails
        let stale_res = remove_root(&pool, &out1.root_id, 0).await;
        assert!(matches!(
            stale_res,
            Err(RootError::ChangedSincePreview {
                expected: 0,
                actual: 1
            })
        ));

        // Correct count succeeds
        remove_root(&pool, &out1.root_id, 1)
            .await
            .expect("remove_root");

        // Verify root 1 is gone, root 2 remains
        let roots = list_roots(&pool).await.expect("list_roots");
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].id, out2.root_id);

        // Files on disk are UNTOUCHED
        assert!(file1_disk.exists(), "File on disk must still exist");
        let read_back = std::fs::read(&file1_disk).expect("read file");
        assert_eq!(
            read_back, file1_bytes,
            "File bytes on disk must be unchanged"
        );

        // Removing unknown id returns root_not_found
        let unknown = RootId::new();
        let unknown_res = remove_root(&pool, &unknown, 0).await;
        assert!(matches!(unknown_res, Err(RootError::NotFound { .. })));
    }

    #[tokio::test]
    async fn test_add_root_concurrent_race_serialises_overlap() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("catalogue.db");
        let pool = init_pool(&db_path).await.expect("init_pool");

        let dir = temp.path().join("RaceDir");
        std::fs::create_dir_all(&dir).expect("create dir");

        let ctx = test_context(temp.path());

        let (res1, res2) = tokio::join!(
            add_root(&pool, &ctx, &dir, None),
            add_root(&pool, &ctx, &dir, None),
        );

        match (res1, res2) {
            (Ok(_), Err(RootError::Overlaps { .. })) | (Err(RootError::Overlaps { .. }), Ok(_)) => {
            }
            (r1, r2) => {
                panic!(
                    "Expected exactly one Ok and one Overlaps error, never Catalogue error. Got:\nr1: {:?}\nr2: {:?}",
                    r1, r2
                );
            }
        }
    }

    #[tokio::test]
    async fn test_add_root_waits_for_held_immediate_transaction() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("catalogue.db");
        let pool = init_pool(&db_path).await.expect("init_pool");

        let dir = temp.path().join("DelayedPhotos");
        std::fs::create_dir_all(&dir).expect("create dir");
        let ctx = test_context(temp.path());

        // On another connection, hold a BEGIN IMMEDIATE transaction for 300 ms
        let pool_holder = pool.clone();
        let holder = tokio::spawn(async move {
            let tx = pool_holder
                .begin_with("BEGIN IMMEDIATE")
                .await
                .expect("holder begin");
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            tx.commit().await.expect("holder commit");
        });

        // Ensure holder task starts and acquires the write lock first
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let start = std::time::Instant::now();
        let outcome = add_root(&pool, &ctx, &dir, None)
            .await
            .expect("add_root should wait and succeed");
        let elapsed = start.elapsed();

        holder.await.expect("holder task");

        assert!(
            elapsed >= std::time::Duration::from_millis(200),
            "Expected add_root to wait for held lock, elapsed: {:?}",
            elapsed
        );

        let roots = list_roots(&pool).await.expect("list_roots");
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].id, outcome.root_id);
    }
}

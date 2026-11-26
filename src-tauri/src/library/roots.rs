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

    // 6. Find or insert volume record (decisions g, h, D29e)
    let volume_id = if let Some(ref uuid) = vol_info.fs_uuid {
        let candidates = database::volumes::find_volumes_by_identity(&mut tx, uuid).await?;
        let matched_vol = if crate::platform::is_serial_identity(uuid) {
            candidates.into_iter().find(|v| {
                v.fs_uuid
                    .as_deref()
                    .is_some_and(|s| crate::platform::same_volume_identity(s, uuid))
                    && v.fs_type == vol_info.fs_type
                    && v.label == vol_info.label
            })
        } else {
            candidates.into_iter().next()
        };

        if let Some(existing_vol) = matched_vol {
            // Decision g: refresh case sensitivity when probe is definite; refuse if volume already has files
            if probe != CaseProbe::Unknown && case_sensitive != existing_vol.case_sensitive {
                if database::volumes::volume_has_files(&mut tx, &existing_vol.id).await? {
                    return Err(RootError::VolumeCaseChanged);
                }
                database::volumes::update_volume_case_sensitive(
                    &mut tx,
                    &existing_vol.id,
                    case_sensitive,
                )
                .await?;
            }

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

use crate::library::availability::{
    check_root_availability, AvailabilityOutcome, DEFAULT_REACHABLE_TIMEOUT,
};

/// Read-only listing result pairing a `RootSummary` with the three-valued live availability outcome.
pub struct RootAvailabilityItem {
    pub summary: RootSummary,
    pub availability: AvailabilityOutcome,
}

/// Lists all registered library roots with volume summaries, live reachable status, and counts (D29).
///
/// Bounded by construction (user library folders); not paginated (spec §12.1).
/// Uses a process-wide default in-flight tracker and the 2-second timeout.
/// Stays read-only (never changes stored `status`).
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

/// Fully parameterized read-only version of `list_roots` allowing custom in-flight set, timeout,
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
    let items =
        list_roots_with_availability_deps(pool, in_flight, timeout, check_fn, describe_volume)
            .await?;
    Ok(items.into_iter().map(|item| item.summary).collect())
}

/// Read-only root listing that returns both each `RootSummary` and its three-valued
/// `AvailabilityOutcome` so the command layer can apply definite transitions (TASK-1F decision d).
pub async fn list_roots_with_availability(
    pool: &SqlitePool,
    in_flight: &Arc<Mutex<HashSet<RootId>>>,
) -> Result<Vec<RootAvailabilityItem>, RootError> {
    list_roots_with_availability_deps(
        pool,
        in_flight,
        DEFAULT_REACHABLE_TIMEOUT,
        |p| p.is_dir(),
        describe_volume,
    )
    .await
}

/// Parameterized read-only root listing with injectable `is_dir` checker and volume `describer`.
pub async fn list_roots_with_availability_deps<F, D>(
    pool: &SqlitePool,
    in_flight: &Arc<Mutex<HashSet<RootId>>>,
    timeout: Duration,
    is_dir_fn: F,
    describer: D,
) -> Result<Vec<RootAvailabilityItem>, RootError>
where
    F: Fn(&Path) -> bool + Send + Sync + Clone + 'static,
    D: Fn(&Path) -> Result<VolumeInfo, RootError> + Send + Sync + Clone + 'static,
{
    let mut conn = pool.acquire().await.map_err(CatalogueError::Database)?;
    let records = database::roots::list_roots_summaries(&mut conn).await?;
    drop(conn);

    if in_flight.lock().is_err() {
        return Err(CatalogueError::Database(sqlx::Error::Configuration(
            "Active reachable checks mutex poisoned".into(),
        ))
        .into());
    }

    let mut check_tasks = Vec::with_capacity(records.len());

    for r in &records {
        let root_id = r.id.clone();
        let native_path = crate::filesystem::blob_to_native(&r.path_native);
        let stored_uuid = r.volume_fs_uuid.clone();
        let in_flight_clone = Arc::clone(in_flight);
        let checker = is_dir_fn.clone();
        let vol_describer = describer.clone();

        let task = tokio::spawn(async move {
            check_root_availability(
                &root_id,
                native_path,
                stored_uuid,
                &in_flight_clone,
                timeout,
                checker,
                vol_describer,
            )
            .await
        });

        check_tasks.push(task);
    }

    let mut items = Vec::with_capacity(records.len());
    for (r, task) in records.into_iter().zip(check_tasks) {
        let availability = task.await.unwrap_or(AvailabilityOutcome::Unknown);
        let reachable = availability.is_available();

        items.push(RootAvailabilityItem {
            summary: RootSummary {
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
            },
            availability,
        });
    }

    Ok(items)
}

/// Attaches an `Offline` root at a new path on the same volume (TASK-1F decision f, D33).
///
/// Rules (in order):
/// 1. Root exists (`not_found`), stored status is `Offline` (`root_not_offline`).
/// 2. Acquires and holds `ScanGuard` (`root_busy`).
/// 3. Canonicalises new path, checks protected folders and overlap excluding the root itself.
/// 4. Stored and live volumes both have `fs_uuid` and match via `same_volume_identity` (`relocate_volume_mismatch`).
/// 5. Content spot check on up to 30 `present`/`inaccessible` file rows; at least half must exist (`relocate_content_mismatch`).
/// 6. Single `BEGIN IMMEDIATE` transaction updates `path_native`, `path_display`, `updated_at_ms`,
///    `status = 'active'`, and volume `last_seen_at_ms`. No `files` or `folders` row is written.
pub async fn relocate_root(
    pool: &SqlitePool,
    ctx: &ProtectedContext,
    root_id: &RootId,
    new_path: &Path,
) -> Result<RootSummary, RootError> {
    relocate_root_with_deps(pool, ctx, root_id, new_path, describe_volume).await
}

/// Parameterized version of `relocate_root` allowing an injected volume `describer` for testing.
pub async fn relocate_root_with_deps<D>(
    pool: &SqlitePool,
    ctx: &ProtectedContext,
    root_id: &RootId,
    new_path: &Path,
    describer: D,
) -> Result<RootSummary, RootError>
where
    D: Fn(&Path) -> Result<VolumeInfo, RootError>,
{
    let mut conn = pool.acquire().await.map_err(CatalogueError::Database)?;

    // 1. Verify root exists and is Offline
    let root = database::roots::get_root(&mut conn, root_id)
        .await?
        .ok_or_else(|| {
            RootError::Catalogue(CatalogueError::NotFound {
                entity: "library_root",
                id: root_id.to_string(),
            })
        })?;

    if root.status != RootStatus::Offline {
        return Err(RootError::NotOffline {
            id: root_id.to_string(),
        });
    }

    // 2. Acquire and hold ScanGuard until relocation finishes
    let _scan_guard = crate::scanner::ScanGuard::acquire(root_id).map_err(|_| RootError::Busy)?;

    // 3. Canonicalize and validate protected folders + overlap excluding self
    let canon = canonicalize_root(new_path)?;
    check_protected(&canon, ctx)?;

    let stored_roots = database::roots::list_roots_for_overlap(&mut conn).await?;
    let other_roots: Vec<_> = stored_roots
        .into_iter()
        .filter(|r| &r.id != root_id)
        .collect();
    check_overlap(&canon, &other_roots)?;

    // 4. Verify stored and live volumes both have non-NULL matching fs_uuid
    let stored_volume_id = root
        .volume_id
        .clone()
        .ok_or(RootError::RelocateVolumeMismatch)?;
    let stored_vol = database::volumes::get_volume(&mut conn, &stored_volume_id)
        .await?
        .ok_or(RootError::RelocateVolumeMismatch)?;
    let stored_uuid = stored_vol
        .fs_uuid
        .as_deref()
        .ok_or(RootError::RelocateVolumeMismatch)?;

    let live_vol = describer(&canon.canonical_path)?;
    let live_uuid = live_vol
        .fs_uuid
        .as_deref()
        .ok_or(RootError::RelocateVolumeMismatch)?;
    if !crate::platform::same_volume_identity(stored_uuid, live_uuid) {
        return Err(RootError::RelocateVolumeMismatch);
    }

    // 5. Content spot check: up to 30 present/inaccessible file rows ordered by id; at least half must exist
    let sample_blobs =
        database::roots::list_relocate_spot_check_paths(&mut conn, root_id, 30).await?;
    drop(conn);

    if !sample_blobs.is_empty() {
        let total = sample_blobs.len();
        let candidate_root = canon.canonical_path.clone();
        let matched = tokio::task::spawn_blocking(move || {
            let mut found = 0usize;
            for blob in sample_blobs {
                let rel = crate::filesystem::blob_to_native(&blob);
                if rel.as_os_str().is_empty() || rel.is_absolute() {
                    continue;
                }
                let full_path = candidate_root.join(rel);
                if std::fs::symlink_metadata(&full_path)
                    .map(|m| m.is_file())
                    .unwrap_or(false)
                {
                    found += 1;
                }
            }
            found
        })
        .await
        .map_err(|_| RootError::RelocateContentMismatch)?;

        if matched * 2 < total {
            return Err(RootError::RelocateContentMismatch);
        }
    }

    // 6. Atomic BEGIN IMMEDIATE update of root paths, status='active', and volume last_seen_at_ms
    let mut tx = pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(CatalogueError::Database)?;

    let latest_root = database::roots::get_root(&mut tx, root_id)
        .await?
        .ok_or_else(|| {
            RootError::Catalogue(CatalogueError::NotFound {
                entity: "library_root",
                id: root_id.to_string(),
            })
        })?;
    if latest_root.status != RootStatus::Offline {
        return Err(RootError::NotOffline {
            id: root_id.to_string(),
        });
    }

    let latest_roots = database::roots::list_roots_for_overlap(&mut tx).await?;
    let latest_others: Vec<_> = latest_roots
        .into_iter()
        .filter(|r| &r.id != root_id)
        .collect();
    check_overlap(&canon, &latest_others)?;

    let now_ms = database::now_ms()?;
    database::roots::relocate_root_paths(
        &mut tx,
        root_id,
        &canon.path_display,
        &canon.path_native,
        now_ms,
    )
    .await?;
    database::volumes::update_volume_last_seen(&mut tx, &stored_volume_id, now_ms).await?;

    tx.commit().await.map_err(CatalogueError::Database)?;

    let mut conn = pool.acquire().await.map_err(CatalogueError::Database)?;
    let rec = database::roots::get_root_summary(&mut conn, root_id)
        .await?
        .ok_or_else(|| {
            RootError::Catalogue(CatalogueError::NotFound {
                entity: "library_root",
                id: root_id.to_string(),
            })
        })?;

    Ok(RootSummary {
        id: rec.id,
        label: rec.label,
        path_display: rec.path_display,
        volume_kind: rec.volume_kind,
        volume_label: rec.volume_label,
        volume_case_sensitive: rec.volume_case_sensitive,
        status: rec.status,
        reachable: true,
        file_count: rec.file_count,
        folder_count: rec.folder_count,
        last_scan_at_ms: rec.last_scan_at_ms,
        last_job: rec.last_job,
        active_job_id: None,
    })
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

    #[tokio::test]
    async fn status_changes_only_on_definite_answers() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("catalogue.db");
        let pool = init_pool(&db_path).await.expect("init_pool");

        let dir = temp.path().join("StatusRoot");
        std::fs::create_dir_all(&dir).expect("create dir");
        let ctx = test_context(temp.path());
        let outcome = add_root(&pool, &ctx, &dir, None).await.expect("add_root");

        let mut conn = pool.acquire().await.unwrap();
        let initial_row = database::roots::get_root(&mut conn, &outcome.root_id)
            .await
            .unwrap()
            .unwrap();
        drop(conn);

        // 1. Unknown outcomes leave Active unchanged (and do not touch updated_at_ms)
        let s = crate::library::apply_availability_result(
            &pool,
            &outcome.root_id,
            Some(RootStatus::Active),
            AvailabilityOutcome::Unknown,
        )
        .await
        .unwrap();
        assert_eq!(s, RootStatus::Active);

        // Check real Unknown causes from check_root_availability:
        // (a) Timeout
        let in_flight = Arc::new(Mutex::new(HashSet::new()));
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let rx_arc = Arc::new(Mutex::new(rx));
        let rx_c = Arc::clone(&rx_arc);
        let timeout_out = crate::library::check_root_availability(
            &outcome.root_id,
            dir.clone(),
            None,
            &in_flight,
            Duration::from_millis(30),
            move |_| {
                if let Ok(g) = rx_c.lock() {
                    let _ = g.recv();
                }
                true
            },
            describe_volume,
        )
        .await;
        assert_eq!(timeout_out, AvailabilityOutcome::Unknown);

        // (b) In-flight check (root_id is still inside in_flight while blocked)
        let inflight_out = crate::library::check_root_availability(
            &outcome.root_id,
            dir.clone(),
            None,
            &in_flight,
            Duration::from_millis(30),
            |_| true,
            describe_volume,
        )
        .await;
        assert_eq!(inflight_out, AvailabilityOutcome::Unknown);
        let _ = tx.send(());

        // (c) Panicking checker -> Unknown
        let clean_in_flight = Arc::new(Mutex::new(HashSet::new()));
        let panic_out = crate::library::check_root_availability(
            &outcome.root_id,
            dir.clone(),
            None,
            &clean_in_flight,
            Duration::from_millis(200),
            |_| panic!("simulated checker panic"),
            describe_volume,
        )
        .await;
        assert_eq!(panic_out, AvailabilityOutcome::Unknown);

        // Verify Active was never changed by Unknown
        let mut conn = pool.acquire().await.unwrap();
        let row_after_unknown = database::roots::get_root(&mut conn, &outcome.root_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row_after_unknown.status, RootStatus::Active);
        assert_eq!(row_after_unknown.updated_at_ms, initial_row.updated_at_ms);
        drop(conn);

        // 2. Definite NotAvailable turns Active -> Offline
        let s_off = crate::library::apply_availability_result(
            &pool,
            &outcome.root_id,
            Some(RootStatus::Active),
            AvailabilityOutcome::NotAvailable(crate::library::UnavailableReason::Unreachable),
        )
        .await
        .unwrap();
        assert_eq!(s_off, RootStatus::Offline);

        // Unknown leaves Offline unchanged
        let mut conn = pool.acquire().await.unwrap();
        let offline_row = database::roots::get_root(&mut conn, &outcome.root_id)
            .await
            .unwrap()
            .unwrap();
        drop(conn);
        let s_off_unknown = crate::library::apply_availability_result(
            &pool,
            &outcome.root_id,
            Some(RootStatus::Offline),
            AvailabilityOutcome::Unknown,
        )
        .await
        .unwrap();
        assert_eq!(s_off_unknown, RootStatus::Offline);
        let mut conn = pool.acquire().await.unwrap();
        let offline_row_after = database::roots::get_root(&mut conn, &outcome.root_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(offline_row_after.updated_at_ms, offline_row.updated_at_ms);
        drop(conn);

        // Definite Available turns Offline -> Active
        let s_on = crate::library::apply_availability_result(
            &pool,
            &outcome.root_id,
            Some(RootStatus::Offline),
            AvailabilityOutcome::Available,
        )
        .await
        .unwrap();
        assert_eq!(s_on, RootStatus::Active);

        // 3. Paused and Removed never change under any outcome
        for fixed_status in [RootStatus::Paused, RootStatus::Removed] {
            let mut conn = pool.acquire().await.unwrap();
            database::roots::set_root_status(&mut conn, &outcome.root_id, fixed_status, 123456)
                .await
                .unwrap();
            drop(conn);

            for outcome_val in [
                AvailabilityOutcome::Available,
                AvailabilityOutcome::NotAvailable(crate::library::UnavailableReason::Unreachable),
                AvailabilityOutcome::NotAvailable(crate::library::UnavailableReason::VolumeChanged),
                AvailabilityOutcome::Unknown,
            ] {
                let res = crate::library::apply_availability_result(
                    &pool,
                    &outcome.root_id,
                    Some(fixed_status),
                    outcome_val,
                )
                .await
                .unwrap();
                assert_eq!(res, fixed_status);
                let mut conn = pool.acquire().await.unwrap();
                let row = database::roots::get_root(&mut conn, &outcome.root_id)
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(row.status, fixed_status);
                assert_eq!(row.updated_at_ms, 123456);
            }
        }
    }

    #[tokio::test]
    async fn case_refresh_definite_change_with_files_refused_volume_case_changed() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("catalogue.db");
        let pool = init_pool(&db_path).await.expect("init_pool");

        let dir1 = temp.path().join("CaseRoot1");
        let dir2 = temp.path().join("CaseRoot2");
        std::fs::create_dir_all(&dir1).expect("create dir1");
        std::fs::create_dir_all(&dir2).expect("create dir2");
        let canon1 = std::fs::canonicalize(&dir1).unwrap();
        let canon2 = std::fs::canonicalize(&dir2).unwrap();

        let ctx = test_context(temp.path());
        let describer = |_p: &Path| -> Result<VolumeInfo, RootError> {
            Ok(VolumeInfo {
                kind: VolumeKind::Internal,
                label: Some("CaseDisk".to_string()),
                fs_uuid: Some("uuid-case-test".to_string()),
                fs_type: Some("apfs".to_string()),
            })
        };

        // First probe: CaseInsensitive (dir1 and swapped parent match)
        let mut map1 = HashMap::new();
        map1.insert(
            canon1.clone(),
            FileIdentity {
                dev_or_serial: 1,
                ino_or_index: 100,
            },
        );
        map1.insert(
            canon1
                .parent()
                .unwrap()
                .join(crate::library::case_probe::swap_case("CaseRoot1")),
            FileIdentity {
                dev_or_serial: 1,
                ino_or_index: 100,
            },
        );
        let lookup1 = TestLookup(map1);

        let out1 = add_root_with_deps(&pool, &ctx, &dir1, None, describer, &lookup1)
            .await
            .expect("add root 1");

        // Insert a file row under root 1
        let mut conn = pool.acquire().await.unwrap();
        let root_folder = database::get_root_folder_id(&mut conn, &out1.root_id)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO files (
                id, root_id, folder_id, name, extension, rel_path_display, rel_path_native,
                path_key, size_bytes, modified_at_ms, status, created_in_catalog_at_ms, updated_at_ms
             ) VALUES ('f_case_1', ?, ?, 'a.jpg', 'jpg', 'a.jpg', X'612E6A7067', X'612E6A7067', 10, 1000, 'present', 1000, 1000);",
        )
        .bind(out1.root_id.as_str())
        .bind(root_folder.as_str())
        .execute(&mut *conn)
        .await
        .unwrap();
        drop(conn);

        // Second probe on same volume: definite CaseSensitive -> must fail with volume_case_changed
        let mut map2 = HashMap::new();
        map2.insert(
            canon2.clone(),
            FileIdentity {
                dev_or_serial: 1,
                ino_or_index: 200,
            },
        );
        map2.insert(
            canon2
                .parent()
                .unwrap()
                .join(crate::library::case_probe::swap_case("CaseRoot2")),
            FileIdentity {
                dev_or_serial: 1,
                ino_or_index: 201,
            },
        );
        let lookup2 = TestLookup(map2);

        let err = add_root_with_deps(&pool, &ctx, &dir2, None, describer, &lookup2)
            .await
            .expect_err("definite case change on volume with files must be refused");
        assert_eq!(err.code(), "volume_case_changed");

        let mut conn = pool.acquire().await.unwrap();
        let roots = database::roots::list_roots(&mut conn).await.unwrap();
        assert_eq!(roots.len(), 1, "No second root should be inserted");
        let vol = database::volumes::get_volume(&mut conn, &out1.volume_id)
            .await
            .unwrap()
            .unwrap();
        assert!(!vol.case_sensitive, "Stored case_sensitive must stay false");
    }

    #[tokio::test]
    async fn case_refresh_unknown_probe_changes_nothing() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("catalogue.db");
        let pool = init_pool(&db_path).await.expect("init_pool");

        let dir1 = temp.path().join("UnknownCase1");
        let dir2 = temp.path().join("UnknownCase2");
        std::fs::create_dir_all(&dir1).expect("create dir1");
        std::fs::create_dir_all(&dir2).expect("create dir2");
        let canon1 = std::fs::canonicalize(&dir1).unwrap();

        let ctx = test_context(temp.path());
        let describer = |_p: &Path| -> Result<VolumeInfo, RootError> {
            Ok(VolumeInfo {
                kind: VolumeKind::Internal,
                label: Some("Disk".to_string()),
                fs_uuid: Some("uuid-unknown-case".to_string()),
                fs_type: Some("apfs".to_string()),
            })
        };

        let mut map1 = HashMap::new();
        map1.insert(
            canon1.clone(),
            FileIdentity {
                dev_or_serial: 1,
                ino_or_index: 10,
            },
        );
        map1.insert(
            canon1
                .parent()
                .unwrap()
                .join(crate::library::case_probe::swap_case("UnknownCase1")),
            FileIdentity {
                dev_or_serial: 1,
                ino_or_index: 11,
            },
        );
        let out1 = add_root_with_deps(&pool, &ctx, &dir1, None, describer, &TestLookup(map1))
            .await
            .expect("add root 1");

        // Unknown probe (empty lookup) must succeed and leave case_sensitive = true untouched
        let out2 = add_root_with_deps(
            &pool,
            &ctx,
            &dir2,
            None,
            describer,
            &TestLookup(HashMap::new()),
        )
        .await
        .expect("unknown probe must succeed");
        assert_eq!(out1.volume_id, out2.volume_id);
        assert!(out2.warnings.contains(&RootWarning::CaseSensitivityUnknown));

        let mut conn = pool.acquire().await.unwrap();
        let vol = database::volumes::get_volume(&mut conn, &out1.volume_id)
            .await
            .unwrap()
            .unwrap();
        assert!(
            vol.case_sensitive,
            "Unknown probe must not alter stored value"
        );
    }

    #[tokio::test]
    async fn case_refresh_definite_change_without_files_updates_volume() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("catalogue.db");
        let pool = init_pool(&db_path).await.expect("init_pool");

        let dir1 = temp.path().join("EmptyCase1");
        let dir2 = temp.path().join("EmptyCase2");
        std::fs::create_dir_all(&dir1).expect("create dir1");
        std::fs::create_dir_all(&dir2).expect("create dir2");
        let canon1 = std::fs::canonicalize(&dir1).unwrap();
        let canon2 = std::fs::canonicalize(&dir2).unwrap();

        let ctx = test_context(temp.path());
        let describer = |_p: &Path| -> Result<VolumeInfo, RootError> {
            Ok(VolumeInfo {
                kind: VolumeKind::Internal,
                label: Some("EmptyDisk".to_string()),
                fs_uuid: Some("uuid-empty-case".to_string()),
                fs_type: Some("apfs".to_string()),
            })
        };

        // Root 1: CaseInsensitive, 0 file rows
        let mut map1 = HashMap::new();
        map1.insert(
            canon1.clone(),
            FileIdentity {
                dev_or_serial: 1,
                ino_or_index: 50,
            },
        );
        map1.insert(
            canon1
                .parent()
                .unwrap()
                .join(crate::library::case_probe::swap_case("EmptyCase1")),
            FileIdentity {
                dev_or_serial: 1,
                ino_or_index: 50,
            },
        );
        let out1 = add_root_with_deps(&pool, &ctx, &dir1, None, describer, &TestLookup(map1))
            .await
            .expect("add root 1");

        // Root 2: definite CaseSensitive with no file rows -> updates stored case_sensitive to true
        let mut map2 = HashMap::new();
        map2.insert(
            canon2.clone(),
            FileIdentity {
                dev_or_serial: 1,
                ino_or_index: 60,
            },
        );
        map2.insert(
            canon2
                .parent()
                .unwrap()
                .join(crate::library::case_probe::swap_case("EmptyCase2")),
            FileIdentity {
                dev_or_serial: 1,
                ino_or_index: 61,
            },
        );
        let out2 = add_root_with_deps(&pool, &ctx, &dir2, None, describer, &TestLookup(map2))
            .await
            .expect("definite change without files should update stored value");
        assert_eq!(out1.volume_id, out2.volume_id);

        let mut conn = pool.acquire().await.unwrap();
        let vol = database::volumes::get_volume(&mut conn, &out1.volume_id)
            .await
            .unwrap()
            .unwrap();
        assert!(
            vol.case_sensitive,
            "Stored case_sensitive must be updated to true"
        );
    }

    #[tokio::test]
    async fn serial_identity_reuse_requires_equal_fs_type_and_label_and_supports_legacy_bare() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("catalogue.db");
        let pool = init_pool(&db_path).await.expect("init_pool");

        let dir1 = temp.path().join("Serial1");
        let dir2 = temp.path().join("Serial2");
        let dir3 = temp.path().join("Serial3");
        let dir4 = temp.path().join("Serial4");
        let dir5 = temp.path().join("LegacySerial");
        for d in [&dir1, &dir2, &dir3, &dir4, &dir5] {
            std::fs::create_dir_all(d).expect("create dir");
        }

        let ctx = test_context(temp.path());
        let lookup = TestLookup(HashMap::new());

        let out1 = add_root_with_deps(
            &pool,
            &ctx,
            &dir1,
            None,
            |_| {
                Ok(VolumeInfo {
                    kind: VolumeKind::Removable,
                    label: Some("USB1".to_string()),
                    fs_uuid: Some("serial:AABBCCDD".to_string()),
                    fs_type: Some("exfat".to_string()),
                })
            },
            &lookup,
        )
        .await
        .unwrap();

        // Same serial, different label -> new volume row
        let out2 = add_root_with_deps(
            &pool,
            &ctx,
            &dir2,
            None,
            |_| {
                Ok(VolumeInfo {
                    kind: VolumeKind::Removable,
                    label: Some("USB2".to_string()),
                    fs_uuid: Some("serial:AABBCCDD".to_string()),
                    fs_type: Some("exfat".to_string()),
                })
            },
            &lookup,
        )
        .await
        .unwrap();
        assert_ne!(out1.volume_id, out2.volume_id);

        // Same serial, different fs_type -> new volume row
        let out3 = add_root_with_deps(
            &pool,
            &ctx,
            &dir3,
            None,
            |_| {
                Ok(VolumeInfo {
                    kind: VolumeKind::Removable,
                    label: Some("USB1".to_string()),
                    fs_uuid: Some("serial:AABBCCDD".to_string()),
                    fs_type: Some("ntfs".to_string()),
                })
            },
            &lookup,
        )
        .await
        .unwrap();
        assert_ne!(out1.volume_id, out3.volume_id);
        assert_ne!(out2.volume_id, out3.volume_id);

        // Same serial, equal fs_type and label -> reuses volume 1
        let out4 = add_root_with_deps(
            &pool,
            &ctx,
            &dir4,
            None,
            |_| {
                Ok(VolumeInfo {
                    kind: VolumeKind::Removable,
                    label: Some("USB1".to_string()),
                    fs_uuid: Some("serial:AABBCCDD".to_string()),
                    fs_type: Some("exfat".to_string()),
                })
            },
            &lookup,
        )
        .await
        .unwrap();
        assert_eq!(out1.volume_id, out4.volume_id);

        // Legacy bare 8-hex volume row is reused by matching serial: identity
        let mut conn = pool.acquire().await.unwrap();
        let legacy_vol_id = VolumeId::new();
        database::volumes::insert_volume(
            &mut conn,
            &VolumeRow {
                id: legacy_vol_id.clone(),
                kind: VolumeKind::Removable,
                label: Some("LEGACY".to_string()),
                fs_uuid: Some("11223344".to_string()),
                fs_type: Some("fat32".to_string()),
                case_sensitive: false,
                last_seen_at_ms: Some(1000),
            },
        )
        .await
        .unwrap();
        drop(conn);

        let out5 = add_root_with_deps(
            &pool,
            &ctx,
            &dir5,
            None,
            |_| {
                Ok(VolumeInfo {
                    kind: VolumeKind::Removable,
                    label: Some("LEGACY".to_string()),
                    fs_uuid: Some("serial:11223344".to_string()),
                    fs_type: Some("fat32".to_string()),
                })
            },
            &lookup,
        )
        .await
        .unwrap();
        assert_eq!(out5.volume_id, legacy_vol_id);
    }
}

pub mod classify;
pub mod error;
pub mod guard;
pub mod types;
pub mod walk;

#[cfg(test)]
mod tests;

pub use classify::{classify, is_always_skipped_name, Class};
pub use error::ScanError;
pub use guard::ScanGuard;
pub use types::{
    ScanCancel, ScanCounts, ScanOptions, ScanOutcome, ScanProgress, ScanReport, ScanWarning,
    ALL_WARNING_CODES,
};
pub use walk::{system_time_to_unix_ms, WalkItem, WalkParams};

use sqlx::SqlitePool;
use std::collections::HashMap;
use std::time::{Instant, SystemTime};

use crate::database::{self, FileEntryParams, FileProcessOutcome, FolderEntryParams};
use crate::domain::{CatalogueError, FolderId, RootId};
use crate::filesystem::{blob_to_native, PathKeyPolicy, RelPath};

const MAX_CAPTURED_WARNINGS: usize = 100;

/// Computes the parent `RelPath` for a given relative path.
fn parent_of(path: &RelPath) -> RelPath {
    let comps = path.components();
    if comps.len() <= 1 {
        RelPath::empty()
    } else {
        match RelPath::from_components(&comps[..comps.len() - 1]) {
            Ok(p) => p,
            Err(_) => RelPath::empty(),
        }
    }
}

/// Resolves the parent `FolderId` for a given relative path.
async fn resolve_parent_folder_id(
    conn: &mut sqlx::SqliteConnection,
    root_id: &RootId,
    parent_rel: &RelPath,
    key_policy: PathKeyPolicy,
    folder_map: &mut HashMap<RelPath, FolderId>,
) -> Result<FolderId, CatalogueError> {
    if let Some(pid) = folder_map.get(parent_rel) {
        return Ok(pid.clone());
    }
    let parent_key = parent_rel.path_key(key_policy);
    if let Some(f) = database::find_folder_by_key(conn, root_id, &parent_key).await? {
        folder_map.insert(parent_rel.clone(), f.id.clone());
        return Ok(f.id);
    }
    if let Some(root_fid) = folder_map.get(&RelPath::empty()) {
        return Ok(root_fid.clone());
    }
    Err(CatalogueError::NotFound {
        entity: "folder",
        id: parent_rel.display(),
    })
}

pub(crate) struct BatchApplyState<'a> {
    pub root_id: &'a RootId,
    pub key_policy: PathKeyPolicy,
    pub scan_generation: &'a str,
    pub now_ms: i64,
    pub folder_map: &'a mut HashMap<RelPath, FolderId>,
    pub counts: &'a mut ScanCounts,
    pub warning_count: &'a mut u64,
    pub warnings: &'a mut Vec<ScanWarning>,
}

/// Applies a single batch of `WalkItem` items to the database inside an existing transaction/connection.
pub(crate) async fn apply_batch(
    conn: &mut sqlx::SqliteConnection,
    items: Vec<WalkItem>,
    state: &mut BatchApplyState<'_>,
) -> Result<(), ScanError> {
    for item in items {
        match item {
            WalkItem::SkippedByRule => {
                state.counts.skipped_by_rule += 1;
            }
            WalkItem::Warning(w) => {
                *state.warning_count += 1;
                if state.warnings.len() < MAX_CAPTURED_WARNINGS {
                    state.warnings.push(w);
                }
            }
            WalkItem::Folder {
                rel_path,
                modified_at_ms,
            } => {
                let parent_rel = parent_of(&rel_path);
                let parent_id = resolve_parent_folder_id(
                    conn,
                    state.root_id,
                    &parent_rel,
                    state.key_policy,
                    state.folder_map,
                )
                .await?;

                let path_key = rel_path.path_key(state.key_policy);
                let rel_native = rel_path.native().into_bytes();
                let name = rel_path.file_name().to_string();
                let rel_display = rel_path.display();

                let upsert_res = database::upsert_folder(
                    conn,
                    &FolderEntryParams {
                        root_id: state.root_id,
                        parent_id: &parent_id,
                        name: &name,
                        rel_path_display: &rel_display,
                        rel_path_native: &rel_native,
                        path_key: &path_key,
                        modified_at_ms,
                        scan_generation: state.scan_generation,
                    },
                )
                .await;

                match upsert_res {
                    Ok(fid) => {
                        state.folder_map.insert(rel_path, fid);
                        state.counts.folders_seen += 1;
                    }
                    Err(CatalogueError::PathKeyCollision { .. }) => {
                        *state.warning_count += 1;
                        if state.warnings.len() < MAX_CAPTURED_WARNINGS {
                            state.warnings.push(ScanWarning {
                                code: "path_key_collision".to_string(),
                                path_display: rel_display,
                            });
                        }
                    }
                    Err(e) => return Err(e.into()),
                }
            }
            WalkItem::File {
                rel_path,
                size_bytes,
                modified_at_ms,
                created_at_ms,
                file_identity,
                class,
            } => {
                state.counts.files_seen += 1;

                let parent_rel = parent_of(&rel_path);
                let folder_id = resolve_parent_folder_id(
                    conn,
                    state.root_id,
                    &parent_rel,
                    state.key_policy,
                    state.folder_map,
                )
                .await?;

                let path_key = rel_path.path_key(state.key_policy);
                let rel_native = rel_path.native().into_bytes();
                let name = rel_path.file_name().to_string();
                let rel_display = rel_path.display();
                let extension = std::path::Path::new(&name)
                    .extension()
                    .and_then(|s| s.to_str());

                let (is_supported_media, asset_format) = match &class {
                    Class::Image(fmt) => (true, Some(fmt.as_str())),
                    Class::Raw(fmt) => (true, Some(fmt.as_str())),
                    Class::Video => (false, None),
                    Class::Ignored => (false, None),
                };

                let process_res = database::process_file_entry(
                    conn,
                    &FileEntryParams {
                        root_id: state.root_id,
                        folder_id: &folder_id,
                        name: &name,
                        extension,
                        rel_path_display: &rel_display,
                        rel_path_native: &rel_native,
                        path_key: &path_key,
                        size_bytes,
                        modified_at_ms,
                        created_at_ms,
                        file_identity: file_identity.as_deref(),
                        is_supported_media,
                        asset_format,
                        scan_generation: state.scan_generation,
                        now_ms: state.now_ms,
                    },
                )
                .await;

                match process_res {
                    Ok(FileProcessOutcome::New) => {
                        state.counts.new += 1;
                        if !is_supported_media {
                            state.counts.unsupported += 1;
                        }
                    }
                    Ok(FileProcessOutcome::Changed) => {
                        state.counts.changed += 1;
                    }
                    Ok(FileProcessOutcome::Unchanged) => {
                        state.counts.unchanged += 1;
                    }
                    Ok(FileProcessOutcome::Collision) => {
                        *state.warning_count += 1;
                        if state.warnings.len() < MAX_CAPTURED_WARNINGS {
                            state.warnings.push(ScanWarning {
                                code: "path_key_collision".to_string(),
                                path_display: rel_display,
                            });
                        }
                    }
                    Err(CatalogueError::PathKeyCollision { .. }) => {
                        *state.warning_count += 1;
                        if state.warnings.len() < MAX_CAPTURED_WARNINGS {
                            state.warnings.push(ScanWarning {
                                code: "path_key_collision".to_string(),
                                path_display: rel_display,
                            });
                        }
                    }
                    Err(e) => return Err(e.into()),
                }
            }
            WalkItem::InaccessibleDir { rel_path, warning } => {
                *state.warning_count += 1;
                if state.warnings.len() < MAX_CAPTURED_WARNINGS {
                    state.warnings.push(warning);
                }
                let path_key = rel_path.path_key(state.key_policy);
                database::mark_subtree_inaccessible(
                    conn,
                    state.root_id,
                    &path_key,
                    state.scan_generation,
                )
                .await?;
                state.counts.marked_inaccessible += 1;
            }
            WalkItem::InaccessibleFile { rel_path, warning } => {
                *state.warning_count += 1;
                if state.warnings.len() < MAX_CAPTURED_WARNINGS {
                    state.warnings.push(warning);
                }
                let path_key = rel_path.path_key(state.key_policy);
                database::mark_file_inaccessible(
                    conn,
                    state.root_id,
                    &path_key,
                    state.scan_generation,
                )
                .await?;
                state.counts.marked_inaccessible += 1;
            }
        }
    }
    Ok(())
}

/// Core scanner entry point: acquires the scan guard and runs the guarded scan (spec §6.2).
pub async fn scan_root<P>(
    pool: &SqlitePool,
    root_id: &RootId,
    options: ScanOptions,
    cancel: ScanCancel,
    progress: P,
) -> Result<ScanOutcome, ScanError>
where
    P: FnMut(ScanProgress) + Send,
{
    // Ensure only one scan runs per root at a time (Decision j)
    let guard = ScanGuard::acquire(root_id)?;
    scan_root_guarded(guard, pool, root_id, options, cancel, progress).await
}

/// Core guarded scanner: walks a library root with an already acquired guard, records folders,
/// files and assets, tracks generations, detects moves, and declares missing items only on full completion (spec §6.2, Decision d).
pub async fn scan_root_guarded<P>(
    guard: ScanGuard,
    pool: &SqlitePool,
    root_id: &RootId,
    options: ScanOptions,
    cancel: ScanCancel,
    progress: P,
) -> Result<ScanOutcome, ScanError>
where
    P: FnMut(ScanProgress) + Send,
{
    static DEFAULT_SCAN_IN_FLIGHT: std::sync::OnceLock<
        std::sync::Arc<std::sync::Mutex<std::collections::HashSet<RootId>>>,
    > = std::sync::OnceLock::new();
    let in_flight = DEFAULT_SCAN_IN_FLIGHT.get_or_init(|| {
        std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()))
    });
    scan_root_guarded_with_in_flight(guard, pool, root_id, options, cancel, in_flight, progress)
        .await
}

/// Variant of `scan_root_guarded` accepting the shared in-flight availability check set.
pub async fn scan_root_guarded_with_in_flight<P>(
    _guard: ScanGuard,
    pool: &SqlitePool,
    root_id: &RootId,
    options: ScanOptions,
    cancel: ScanCancel,
    in_flight: &std::sync::Arc<std::sync::Mutex<std::collections::HashSet<RootId>>>,
    mut progress: P,
) -> Result<ScanOutcome, ScanError>
where
    P: FnMut(ScanProgress) + Send,
{
    let mut conn = pool.acquire().await?;

    // Fetch root row from catalogue (never trust path strings from callers)
    let root = match database::get_root(&mut conn, root_id).await? {
        Some(r) => r,
        None => {
            return Err(ScanError::RootUnreachable {
                path: root_id.to_string(),
            });
        }
    };

    // Refuse root if follow_symlinks is enabled (Decision a)
    if root.follow_symlinks {
        return Err(ScanError::OptionUnsupported {
            reason: "follow_symlinks is not supported".to_string(),
        });
    }

    // Fetch volume case sensitivity and stored fs_uuid before releasing DB connection
    let (case_sensitive, stored_fs_uuid) = match &root.volume_id {
        Some(vid) => match database::get_volume(&mut conn, vid).await? {
            Some(vol) => (vol.case_sensitive, vol.fs_uuid),
            None => (false, None),
        },
        None => (false, None),
    };
    let key_policy = PathKeyPolicy { case_sensitive };

    let scan_generation = options
        .generation
        .clone()
        .unwrap_or_else(|| uuid::Uuid::now_v7().to_string());
    let scan_start_time = Instant::now();
    let scan_start_ms = system_time_to_unix_ms(SystemTime::now()).unwrap_or_default();

    // Lookup root-folder row (parent_id IS NULL, 1C)
    let root_folder_id = database::get_root_folder_id(&mut conn, root_id).await?;
    drop(conn);

    // Verify directory existence and volume identity off the async executor with timeout (TASK-1F decision e)
    let root_path = blob_to_native(&root.path_native);
    let availability = crate::library::check_root_availability(
        root_id,
        root_path.clone(),
        stored_fs_uuid,
        in_flight,
        crate::library::SCAN_AVAILABILITY_TIMEOUT,
        |p| p.is_dir(),
        crate::library::volume::describe_volume,
    )
    .await;

    if !availability.is_available() {
        return Err(ScanError::RootUnreachable {
            path: root.path_display,
        });
    }

    let mut folder_map: HashMap<RelPath, FolderId> = HashMap::new();
    folder_map.insert(RelPath::empty(), root_folder_id);

    let mut warnings: Vec<ScanWarning> = Vec::new();
    let mut warning_count: u64 = 0;
    let mut counts = ScanCounts::default();

    // Channel capacity 2 guarantees bounded memory for arbitrarily large roots (Decision b)
    let (tx, mut rx) = tokio::sync::mpsc::channel(2);

    let walk_root_path = root_path.clone();
    let walk_cancel = cancel.clone();
    let include_hidden = root.include_hidden;
    let batch_size = options.batch_size.max(1);

    let walk_handle = tokio::task::spawn_blocking(move || {
        walk::run_walk(
            WalkParams {
                root_path: walk_root_path,
                include_hidden,
                batch_size,
                cancel: walk_cancel,
                #[cfg(test)]
                panic_after_batch: options.panic_in_walker_after_batch,
            },
            tx,
        );
    });

    let mut batches_committed: u64 = 0;

    // Async writer loop applying batches in dedicated write transactions
    while let Some(batch) = rx.recv().await {
        #[cfg(test)]
        if let Some(fail_after) = options.failure_hook_after_batch {
            if batches_committed >= fail_after as u64 {
                return Err(CatalogueError::Database(sqlx::Error::PoolClosed).into());
            }
        }

        if cancel.is_cancelled() {
            break;
        }

        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
        let now_ms = system_time_to_unix_ms(SystemTime::now()).unwrap_or_default();

        let mut batch_state = BatchApplyState {
            root_id,
            key_policy,
            scan_generation: &scan_generation,
            now_ms,
            folder_map: &mut folder_map,
            counts: &mut counts,
            warning_count: &mut warning_count,
            warnings: &mut warnings,
        };

        apply_batch(&mut tx, batch, &mut batch_state).await?;

        tx.commit().await?;
        batches_committed += 1;
        progress(ScanProgress {
            folders_seen: counts.folders_seen,
            files_seen: counts.files_seen,
            batches_committed,
        });

        if cancel.is_cancelled() {
            break;
        }
    }

    // Drop rx before awaiting the walk task so that any blocked sender in run_walk
    // fails immediately with channel disconnection rather than hanging forever.
    drop(rx);

    // Wait for the blocking walk task to terminate; abort if the walker thread panicked (Decision i, J1)
    walk_handle.await.map_err(|_| ScanError::WalkFailed)?;

    // Cancellation check: cancelled scans mark nothing missing (Decision i)
    if cancel.is_cancelled() {
        return Ok(ScanOutcome::Cancelled(ScanReport {
            generation: scan_generation,
            counts,
            warning_count,
            warnings,
            elapsed: scan_start_time.elapsed(),
        }));
    }

    #[cfg(test)]
    if options.failure_in_move_step {
        return Err(CatalogueError::Database(sqlx::Error::PoolClosed).into());
    }

    // Final transaction: move resolution, marking missing, updating root metadata (Decision h, i)
    let mut final_tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let final_now_ms = system_time_to_unix_ms(SystemTime::now()).unwrap_or_default();

    let moved_count = database::detect_and_apply_moves(
        &mut final_tx,
        root_id,
        &scan_generation,
        scan_start_ms,
        final_now_ms,
    )
    .await?;

    counts.moved = moved_count;
    counts.new = counts.new.saturating_sub(moved_count);

    #[cfg(test)]
    if options.failure_in_final_marking {
        return Err(CatalogueError::Database(sqlx::Error::PoolClosed).into());
    }

    let (files_missing, folders_missing) =
        database::finalize_scan_marking(&mut final_tx, root_id, &scan_generation, final_now_ms)
            .await?;

    counts.marked_missing = files_missing + folders_missing;

    final_tx.commit().await?;

    Ok(ScanOutcome::Completed(ScanReport {
        generation: scan_generation,
        counts,
        warning_count,
        warnings,
        elapsed: scan_start_time.elapsed(),
    }))
}

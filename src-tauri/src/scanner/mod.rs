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
};
pub use walk::{system_time_to_unix_ms, WalkItem, WalkParams};

use sqlx::SqlitePool;
use std::collections::HashMap;
use std::time::{Instant, SystemTime};

use crate::database::{self, FileEntryParams, FileProcessOutcome, FolderEntryParams};
use crate::domain::{CatalogueError, FolderId, RootId};
use crate::filesystem::{blob_to_native, PathKeyPolicy, RelPath};

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

/// Core scanner entry point: walks a library root, records folders, files and assets,
/// tracks generations, detects moves, and declares missing items only on full completion (spec §6.2).
pub async fn scan_root<P>(
    pool: &SqlitePool,
    root_id: &RootId,
    options: ScanOptions,
    cancel: ScanCancel,
    mut progress: P,
) -> Result<ScanOutcome, ScanError>
where
    P: FnMut(ScanProgress) + Send,
{
    // Ensure only one scan runs per root at a time (Decision j)
    let _guard = ScanGuard::acquire(root_id)?;

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

    // Resolve canonical path and verify directory existence
    let root_path = blob_to_native(&root.path_native);
    if !root_path.is_dir() {
        return Err(ScanError::RootUnreachable {
            path: root.path_display,
        });
    }

    // Fetch volume case sensitivity for path_key policy
    let case_sensitive = match &root.volume_id {
        Some(vid) => match database::get_volume(&mut conn, vid).await? {
            Some(vol) => vol.case_sensitive,
            None => false,
        },
        None => false,
    };
    let key_policy = PathKeyPolicy { case_sensitive };

    let scan_generation = uuid::Uuid::now_v7().to_string();
    let scan_start_time = Instant::now();
    let scan_start_ms = system_time_to_unix_ms(SystemTime::now()).unwrap_or_default();

    // Lookup root-folder row (parent_id IS NULL, 1C)
    let root_folder_id = database::get_root_folder_id(&mut conn, root_id).await?;
    drop(conn);

    let mut folder_map: HashMap<RelPath, FolderId> = HashMap::new();
    folder_map.insert(RelPath::empty(), root_folder_id);

    let mut warnings: Vec<ScanWarning> = Vec::new();
    let mut warning_count: u64 = 0;

    let mut add_warning = |code: String, path_display: String| {
        warning_count += 1;
        if warnings.len() < 100 {
            warnings.push(ScanWarning { code, path_display });
        }
    };

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
            },
            tx,
        );
    });

    let mut batches_committed: u64 = 0;

    // Async writer loop applying batches in dedicated write transactions
    while let Some(batch) = rx.recv().await {
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

        for item in batch {
            match item {
                WalkItem::SkippedByRule => {
                    counts.skipped_by_rule += 1;
                }
                WalkItem::Warning(w) => {
                    add_warning(w.code, w.path_display);
                }
                WalkItem::Folder {
                    rel_path,
                    modified_at_ms,
                } => {
                    let parent_rel = parent_of(&rel_path);
                    let parent_id = match folder_map.get(&parent_rel) {
                        Some(pid) => pid.clone(),
                        None => {
                            let parent_key = parent_rel.path_key(key_policy);
                            match database::find_folder_by_key(&mut tx, root_id, &parent_key)
                                .await?
                            {
                                Some(f) => {
                                    folder_map.insert(parent_rel.clone(), f.id.clone());
                                    f.id
                                }
                                None => folder_map
                                    .get(&RelPath::empty())
                                    .cloned()
                                    .unwrap_or_else(|| FolderId::from(String::new())),
                            }
                        }
                    };

                    let path_key = rel_path.path_key(key_policy);
                    let rel_native = rel_path.native().into_bytes();
                    let name = rel_path.file_name().to_string();
                    let rel_display = rel_path.display();

                    let upsert_res = database::upsert_folder(
                        &mut tx,
                        &FolderEntryParams {
                            root_id,
                            parent_id: &parent_id,
                            name: &name,
                            rel_path_display: &rel_display,
                            rel_path_native: &rel_native,
                            path_key: &path_key,
                            modified_at_ms,
                            scan_generation: &scan_generation,
                        },
                    )
                    .await;

                    match upsert_res {
                        Ok(fid) => {
                            folder_map.insert(rel_path, fid);
                            counts.folders_seen += 1;
                        }
                        Err(CatalogueError::PathKeyCollision { .. }) => {
                            add_warning("path_key_collision".to_string(), rel_display);
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
                    counts.files_seen += 1;

                    let parent_rel = parent_of(&rel_path);
                    let folder_id = match folder_map.get(&parent_rel) {
                        Some(pid) => pid.clone(),
                        None => {
                            let parent_key = parent_rel.path_key(key_policy);
                            match database::find_folder_by_key(&mut tx, root_id, &parent_key)
                                .await?
                            {
                                Some(f) => {
                                    folder_map.insert(parent_rel.clone(), f.id.clone());
                                    f.id
                                }
                                None => folder_map
                                    .get(&RelPath::empty())
                                    .cloned()
                                    .unwrap_or_else(|| FolderId::from(String::new())),
                            }
                        }
                    };

                    let path_key = rel_path.path_key(key_policy);
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
                        &mut tx,
                        &FileEntryParams {
                            root_id,
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
                            scan_generation: &scan_generation,
                            now_ms,
                        },
                    )
                    .await;

                    match process_res {
                        Ok(FileProcessOutcome::New) => {
                            counts.new += 1;
                            if !is_supported_media {
                                counts.unsupported += 1;
                            }
                        }
                        Ok(FileProcessOutcome::Changed) => {
                            counts.changed += 1;
                        }
                        Ok(FileProcessOutcome::Unchanged) => {
                            counts.unchanged += 1;
                        }
                        Ok(FileProcessOutcome::Collision) => {
                            add_warning("path_key_collision".to_string(), rel_display);
                        }
                        Err(CatalogueError::PathKeyCollision { .. }) => {
                            add_warning("path_key_collision".to_string(), rel_display);
                        }
                        Err(e) => return Err(e.into()),
                    }
                }
                WalkItem::InaccessibleDir { rel_path, warning } => {
                    add_warning(warning.code, warning.path_display);
                    let path_key = rel_path.path_key(key_policy);
                    database::mark_subtree_inaccessible(
                        &mut tx,
                        root_id,
                        &path_key,
                        &scan_generation,
                    )
                    .await?;
                    counts.marked_inaccessible += 1;
                }
                WalkItem::InaccessibleFile { rel_path, warning } => {
                    add_warning(warning.code, warning.path_display);
                    let path_key = rel_path.path_key(key_policy);
                    database::mark_file_inaccessible(&mut tx, root_id, &path_key, &scan_generation)
                        .await?;
                    counts.marked_inaccessible += 1;
                }
            }
        }

        tx.commit().await?;
        batches_committed += 1;
        progress(ScanProgress {
            folders_seen: counts.folders_seen,
            files_seen: counts.files_seen,
            batches_committed,
        });
    }

    // Wait for the blocking walk task to terminate
    let _ = walk_handle.await;

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

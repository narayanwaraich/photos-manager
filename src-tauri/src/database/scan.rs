use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};
use std::collections::HashMap;

use crate::domain::{AssetId, CatalogueError, FileId, FileStatus, FolderId, RootId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileProcessOutcome {
    New,
    Changed,
    Unchanged,
    Collision,
}

/// Finds the root-folder row (`parent_id IS NULL`) for a given library root.
pub async fn get_root_folder_id(
    conn: &mut SqliteConnection,
    root_id: &RootId,
) -> Result<FolderId, CatalogueError> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT id FROM folders WHERE root_id = ? AND parent_id IS NULL LIMIT 1;")
            .bind(root_id.as_str())
            .fetch_optional(&mut *conn)
            .await?;

    match row {
        Some((id,)) => Ok(FolderId::from(id)),
        None => Err(CatalogueError::NotFound {
            entity: "root_folder",
            id: root_id.to_string(),
        }),
    }
}

/// Parameters for recording or stamping a discovered folder.
pub struct FolderEntryParams<'a> {
    pub root_id: &'a RootId,
    pub parent_id: &'a FolderId,
    pub name: &'a str,
    pub rel_path_display: &'a str,
    pub rel_path_native: &'a [u8],
    pub path_key: &'a [u8],
    pub modified_at_ms: Option<i64>,
    pub scan_generation: &'a str,
}

/// Upserts a folder row during a scan.
///
/// If a folder with `(root_id, path_key)` already exists:
/// stamps `last_seen_generation`, sets `status = 'present'`, updates `modified_at_ms`.
/// If new: inserts into `folders`.
pub async fn upsert_folder(
    conn: &mut SqliteConnection,
    params: &FolderEntryParams<'_>,
) -> Result<FolderId, CatalogueError> {
    let existing: Option<(String, Option<String>)> = sqlx::query_as(
        "SELECT id, last_seen_generation FROM folders WHERE root_id = ? AND path_key = ? LIMIT 1;",
    )
    .bind(params.root_id.as_str())
    .bind(params.path_key)
    .fetch_optional(&mut *conn)
    .await?;

    if let Some((id_str, last_seen_gen)) = existing {
        if last_seen_gen.as_deref() == Some(params.scan_generation) {
            return Err(CatalogueError::PathKeyCollision {
                root_id: params.root_id.to_string(),
                path_key: params.path_key.to_vec(),
                existing_id: Some(id_str),
            });
        }

        sqlx::query(
            "UPDATE folders SET last_seen_generation = ?, status = 'present', modified_at_ms = ? WHERE id = ?;",
        )
        .bind(params.scan_generation)
        .bind(params.modified_at_ms)
        .bind(&id_str)
        .execute(&mut *conn)
        .await?;

        return Ok(FolderId::from(id_str));
    }

    let folder_id = FolderId::new();
    let insert_res = sqlx::query(
        "INSERT INTO folders (
           id, root_id, parent_id, name, rel_path_display, rel_path_native, path_key,
           status, modified_at_ms, last_seen_generation
         ) VALUES (?, ?, ?, ?, ?, ?, ?, 'present', ?, ?);",
    )
    .bind(folder_id.as_str())
    .bind(params.root_id.as_str())
    .bind(params.parent_id.as_str())
    .bind(params.name)
    .bind(params.rel_path_display)
    .bind(params.rel_path_native)
    .bind(params.path_key)
    .bind(params.modified_at_ms)
    .bind(params.scan_generation)
    .execute(&mut *conn)
    .await;

    match insert_res {
        Ok(_) => Ok(folder_id),
        Err(err) => {
            if err.as_database_error().map(|e| e.kind())
                == Some(sqlx::error::ErrorKind::UniqueViolation)
            {
                let existing_row: Option<(String,)> = sqlx::query_as(
                    "SELECT id FROM folders WHERE root_id = ? AND path_key = ? LIMIT 1;",
                )
                .bind(params.root_id.as_str())
                .bind(params.path_key)
                .fetch_optional(&mut *conn)
                .await?;

                return Err(CatalogueError::PathKeyCollision {
                    root_id: params.root_id.to_string(),
                    path_key: params.path_key.to_vec(),
                    existing_id: existing_row.map(|(id,)| id),
                });
            }
            Err(CatalogueError::Database(err))
        }
    }
}

/// Parameters for processing a discovered file entry.
pub struct FileEntryParams<'a> {
    pub root_id: &'a RootId,
    pub folder_id: &'a FolderId,
    pub name: &'a str,
    pub extension: Option<&'a str>,
    pub rel_path_display: &'a str,
    pub rel_path_native: &'a [u8],
    pub path_key: &'a [u8],
    pub size_bytes: i64,
    pub modified_at_ms: i64,
    pub created_at_ms: Option<i64>,
    pub file_identity: Option<&'a str>,
    pub is_supported_media: bool,
    pub asset_format: Option<&'a str>,
    pub scan_generation: &'a str,
    pub now_ms: i64,
}

/// Processes a discovered file during a scan (Decision f).
///
/// Unchanged fingerprint: stamps `scan_generation` and `last_verified_at_ms` only.
/// Changed or new: upserts `files` row, creates/updates primary `assets` and `asset_files` row.
pub async fn process_file_entry(
    conn: &mut SqliteConnection,
    params: &FileEntryParams<'_>,
) -> Result<FileProcessOutcome, CatalogueError> {
    let existing_row: Option<SqliteRow> = sqlx::query(
        "SELECT id, size_bytes, modified_at_ms, file_identity, status, scan_generation FROM files WHERE root_id = ? AND path_key = ? LIMIT 1;",
    )
    .bind(params.root_id.as_str())
    .bind(params.path_key)
    .fetch_optional(&mut *conn)
    .await?;

    let target_status = if params.is_supported_media {
        FileStatus::Present
    } else {
        FileStatus::Unsupported
    };

    if let Some(row) = existing_row {
        let file_id_str: String = row.try_get("id")?;
        let existing_gen: Option<String> = row.try_get("scan_generation")?;
        if existing_gen.as_deref() == Some(params.scan_generation) {
            return Err(CatalogueError::PathKeyCollision {
                root_id: params.root_id.to_string(),
                path_key: params.path_key.to_vec(),
                existing_id: Some(file_id_str),
            });
        }

        let existing_size: i64 = row.try_get("size_bytes")?;
        let existing_mtime: i64 = row.try_get("modified_at_ms")?;
        let existing_identity: Option<String> = row.try_get("file_identity")?;
        let existing_status_str: String = row.try_get("status")?;

        let identity_matches = match (params.file_identity, existing_identity.as_deref()) {
            (Some(new_id), Some(old_id)) => new_id == old_id,
            (None, None) => true,
            _ => false,
        };

        let is_unchanged = existing_size == params.size_bytes
            && existing_mtime == params.modified_at_ms
            && identity_matches;

        if is_unchanged {
            let status_to_set = if existing_status_str != target_status.as_str() {
                target_status.as_str()
            } else {
                &existing_status_str
            };

            sqlx::query(
                "UPDATE files SET scan_generation = ?, last_verified_at_ms = ?, status = ? WHERE id = ?;",
            )
            .bind(params.scan_generation)
            .bind(params.now_ms)
            .bind(status_to_set)
            .bind(&file_id_str)
            .execute(&mut *conn)
            .await?;

            return Ok(FileProcessOutcome::Unchanged);
        }

        // Changed fingerprint: update row
        sqlx::query(
            "UPDATE files SET
               folder_id = ?, name = ?, extension = ?, rel_path_display = ?, rel_path_native = ?,
               size_bytes = ?, modified_at_ms = ?, created_at_ms = ?, file_identity = ?,
               status = ?, scan_generation = ?, last_verified_at_ms = ?, updated_at_ms = ?
             WHERE id = ?;",
        )
        .bind(params.folder_id.as_str())
        .bind(params.name)
        .bind(params.extension)
        .bind(params.rel_path_display)
        .bind(params.rel_path_native)
        .bind(params.size_bytes)
        .bind(params.modified_at_ms)
        .bind(params.created_at_ms)
        .bind(params.file_identity)
        .bind(target_status.as_str())
        .bind(params.scan_generation)
        .bind(params.now_ms)
        .bind(params.now_ms)
        .bind(&file_id_str)
        .execute(&mut *conn)
        .await?;

        // If supported media, ensure asset row exists
        if params.is_supported_media {
            let existing_asset: Option<(String,)> =
                sqlx::query_as("SELECT id FROM assets WHERE primary_file_id = ? LIMIT 1;")
                    .bind(&file_id_str)
                    .fetch_optional(&mut *conn)
                    .await?;

            if let Some((asset_id_str,)) = existing_asset {
                sqlx::query(
                    "UPDATE assets SET format = ?, status = 'present', updated_at_ms = ? WHERE id = ?;",
                )
                .bind(params.asset_format)
                .bind(params.now_ms)
                .bind(&asset_id_str)
                .execute(&mut *conn)
                .await?;
            } else {
                let asset_id = AssetId::new();
                sqlx::query(
                    "INSERT INTO assets (
                       id, kind, primary_file_id, format, is_favorite, status,
                       created_in_catalog_at_ms, updated_at_ms
                     ) VALUES (?, 'image', ?, ?, 0, 'present', ?, ?);",
                )
                .bind(asset_id.as_str())
                .bind(&file_id_str)
                .bind(params.asset_format)
                .bind(params.now_ms)
                .bind(params.now_ms)
                .execute(&mut *conn)
                .await?;

                sqlx::query(
                    "INSERT INTO asset_files (asset_id, file_id, role) VALUES (?, ?, 'primary');",
                )
                .bind(asset_id.as_str())
                .bind(&file_id_str)
                .execute(&mut *conn)
                .await?;
            }
        }

        return Ok(FileProcessOutcome::Changed);
    }

    // New file entry
    let file_id = FileId::new();
    let insert_res = sqlx::query(
        "INSERT INTO files (
           id, root_id, folder_id, name, extension, rel_path_display, rel_path_native, path_key,
           size_bytes, modified_at_ms, created_at_ms, file_identity, status,
           scan_generation, last_verified_at_ms, created_in_catalog_at_ms, updated_at_ms
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?);",
    )
    .bind(file_id.as_str())
    .bind(params.root_id.as_str())
    .bind(params.folder_id.as_str())
    .bind(params.name)
    .bind(params.extension)
    .bind(params.rel_path_display)
    .bind(params.rel_path_native)
    .bind(params.path_key)
    .bind(params.size_bytes)
    .bind(params.modified_at_ms)
    .bind(params.created_at_ms)
    .bind(params.file_identity)
    .bind(target_status.as_str())
    .bind(params.scan_generation)
    .bind(params.now_ms)
    .bind(params.now_ms)
    .bind(params.now_ms)
    .execute(&mut *conn)
    .await;

    match insert_res {
        Ok(_) => {}
        Err(err) => {
            if err.as_database_error().map(|e| e.kind())
                == Some(sqlx::error::ErrorKind::UniqueViolation)
            {
                let existing_row: Option<(String,)> = sqlx::query_as(
                    "SELECT id FROM files WHERE root_id = ? AND path_key = ? LIMIT 1;",
                )
                .bind(params.root_id.as_str())
                .bind(params.path_key)
                .fetch_optional(&mut *conn)
                .await?;

                return Err(CatalogueError::PathKeyCollision {
                    root_id: params.root_id.to_string(),
                    path_key: params.path_key.to_vec(),
                    existing_id: existing_row.map(|(id,)| id),
                });
            }
            return Err(CatalogueError::Database(err));
        }
    }

    // Create asset for new image or RAW file
    if params.is_supported_media {
        let asset_id = AssetId::new();
        sqlx::query(
            "INSERT INTO assets (
               id, kind, primary_file_id, format, is_favorite, status,
               created_in_catalog_at_ms, updated_at_ms
             ) VALUES (?, 'image', ?, ?, 0, 'present', ?, ?);",
        )
        .bind(asset_id.as_str())
        .bind(file_id.as_str())
        .bind(params.asset_format)
        .bind(params.now_ms)
        .bind(params.now_ms)
        .execute(&mut *conn)
        .await?;

        sqlx::query("INSERT INTO asset_files (asset_id, file_id, role) VALUES (?, ?, 'primary');")
            .bind(asset_id.as_str())
            .bind(file_id.as_str())
            .execute(&mut *conn)
            .await?;
    }

    Ok(FileProcessOutcome::New)
}

/// Marks a folder and all its descendant folders and files as inaccessible,
/// restamping them with the current scan generation so they are not declared missing (Decision i).
pub async fn mark_subtree_inaccessible(
    conn: &mut SqliteConnection,
    root_id: &RootId,
    path_key: &[u8],
    scan_generation: &str,
) -> Result<(), CatalogueError> {
    let folder_row: Option<(String,)> =
        sqlx::query_as("SELECT id FROM folders WHERE root_id = ? AND path_key = ? LIMIT 1;")
            .bind(root_id.as_str())
            .bind(path_key)
            .fetch_optional(&mut *conn)
            .await?;

    if let Some((root_folder_id,)) = folder_row {
        sqlx::query(
            "WITH RECURSIVE subtree(id) AS (
               SELECT id FROM folders WHERE id = ?
               UNION ALL
               SELECT f.id FROM folders f JOIN subtree s ON f.parent_id = s.id
             )
             UPDATE folders
             SET status = 'inaccessible', last_seen_generation = ?
             WHERE id IN (SELECT id FROM subtree);",
        )
        .bind(&root_folder_id)
        .bind(scan_generation)
        .execute(&mut *conn)
        .await?;

        sqlx::query(
            "WITH RECURSIVE subtree(id) AS (
               SELECT id FROM folders WHERE id = ?
               UNION ALL
               SELECT f.id FROM folders f JOIN subtree s ON f.parent_id = s.id
             )
             UPDATE files
             SET status = 'inaccessible', scan_generation = ?
             WHERE folder_id IN (SELECT id FROM subtree);",
        )
        .bind(&root_folder_id)
        .bind(scan_generation)
        .execute(&mut *conn)
        .await?;
    }

    Ok(())
}

/// Marks a single existing file as inaccessible and restamps it with current generation (Decision i).
pub async fn mark_file_inaccessible(
    conn: &mut SqliteConnection,
    root_id: &RootId,
    path_key: &[u8],
    scan_generation: &str,
) -> Result<(), CatalogueError> {
    sqlx::query(
        "UPDATE files SET status = 'inaccessible', scan_generation = ? WHERE root_id = ? AND path_key = ?;",
    )
    .bind(scan_generation)
    .bind(root_id.as_str())
    .bind(path_key)
    .execute(&mut *conn)
    .await?;

    Ok(())
}

struct StaleFileInfo {
    id: String,
}

struct NewFileInfo {
    id: String,
    folder_id: String,
    name: String,
    extension: Option<String>,
    rel_path_display: String,
    rel_path_native: Vec<u8>,
    path_key: Vec<u8>,
    size_bytes: i64,
    modified_at_ms: i64,
    created_at_ms: Option<i64>,
    status: String,
}

/// Detects moves by file identity after walk completion (spec §6.2 step 7, Decision h).
///
/// Matches stale rows (from previous scans) with new rows created in this scan having
/// an identity occurring exactly once on each side.
pub async fn detect_and_apply_moves(
    conn: &mut SqliteConnection,
    root_id: &RootId,
    current_generation: &str,
    scan_start_ms: i64,
    now_ms: i64,
) -> Result<u64, CatalogueError> {
    let stale_rows = sqlx::query(
        "SELECT id, file_identity FROM files
         WHERE root_id = ? AND (scan_generation IS NULL OR scan_generation != ?) AND file_identity IS NOT NULL AND status != 'missing';",
    )
    .bind(root_id.as_str())
    .bind(current_generation)
    .fetch_all(&mut *conn)
    .await?;

    let new_rows = sqlx::query(
        "SELECT id, file_identity, folder_id, name, extension, rel_path_display,
                rel_path_native, path_key, size_bytes, modified_at_ms, created_at_ms, status
         FROM files
         WHERE root_id = ? AND scan_generation = ? AND file_identity IS NOT NULL
           AND created_in_catalog_at_ms >= ?;",
    )
    .bind(root_id.as_str())
    .bind(current_generation)
    .bind(scan_start_ms)
    .fetch_all(&mut *conn)
    .await?;

    if stale_rows.is_empty() || new_rows.is_empty() {
        return Ok(0);
    }

    let mut stale_by_identity: HashMap<String, Vec<StaleFileInfo>> = HashMap::new();
    for r in stale_rows {
        let id: String = r.try_get("id")?;
        let ident: Option<String> = r.try_get("file_identity")?;
        if let Some(ident_str) = ident {
            stale_by_identity
                .entry(ident_str.clone())
                .or_default()
                .push(StaleFileInfo { id });
        }
    }

    let mut new_by_identity: HashMap<String, Vec<NewFileInfo>> = HashMap::new();
    for r in new_rows {
        let id: String = r.try_get("id")?;
        let ident: Option<String> = r.try_get("file_identity")?;
        if let Some(ident_str) = ident {
            let folder_id: String = r.try_get("folder_id")?;
            let name: String = r.try_get("name")?;
            let extension: Option<String> = r.try_get("extension")?;
            let rel_path_display: String = r.try_get("rel_path_display")?;
            let rel_path_native: Vec<u8> = r.try_get("rel_path_native")?;
            let path_key: Vec<u8> = r.try_get("path_key")?;
            let size_bytes: i64 = r.try_get("size_bytes")?;
            let modified_at_ms: i64 = r.try_get("modified_at_ms")?;
            let created_at_ms: Option<i64> = r.try_get("created_at_ms")?;
            let status: String = r.try_get("status")?;

            new_by_identity
                .entry(ident_str.clone())
                .or_default()
                .push(NewFileInfo {
                    id,
                    folder_id,
                    name,
                    extension,
                    rel_path_display,
                    rel_path_native,
                    path_key,
                    size_bytes,
                    modified_at_ms,
                    created_at_ms,
                    status,
                });
        }
    }

    let mut moves_count: u64 = 0;

    for (identity, stales) in stale_by_identity {
        if stales.len() == 1 {
            if let Some(news) = new_by_identity.get(&identity) {
                if news.len() == 1 {
                    let stale_file = &stales[0];
                    let new_file = &news[0];

                    // Delete the new file row created during this walk.
                    // This cascade deletes the new row's asset and asset_files rows.
                    sqlx::query("DELETE FROM files WHERE id = ?;")
                        .bind(&new_file.id)
                        .execute(&mut *conn)
                        .await?;

                    // Update the stale file row to the new location, stamping current generation.
                    // The stale row's original FileId (and its asset) is preserved.
                    sqlx::query(
                        "UPDATE files SET
                           folder_id = ?, name = ?, extension = ?, rel_path_display = ?, rel_path_native = ?,
                           path_key = ?, size_bytes = ?, modified_at_ms = ?, created_at_ms = ?,
                           status = ?, scan_generation = ?, last_verified_at_ms = ?, updated_at_ms = ?
                         WHERE id = ?;",
                    )
                    .bind(&new_file.folder_id)
                    .bind(&new_file.name)
                    .bind(&new_file.extension)
                    .bind(&new_file.rel_path_display)
                    .bind(&new_file.rel_path_native)
                    .bind(&new_file.path_key)
                    .bind(new_file.size_bytes)
                    .bind(new_file.modified_at_ms)
                    .bind(new_file.created_at_ms)
                    .bind(&new_file.status)
                    .bind(current_generation)
                    .bind(now_ms)
                    .bind(now_ms)
                    .bind(&stale_file.id)
                    .execute(&mut *conn)
                    .await?;

                    moves_count += 1;
                }
            }
        }
    }

    Ok(moves_count)
}

/// Final marking transaction executed ONLY after walk completion and move detection (Decision i).
///
/// Marks stale files and folders as missing and stamps root's `last_scan_at_ms` and `last_scan_generation`.
pub async fn finalize_scan_marking(
    conn: &mut SqliteConnection,
    root_id: &RootId,
    current_generation: &str,
    now_ms: i64,
) -> Result<(u64, u64), CatalogueError> {
    let files_res = sqlx::query(
        "UPDATE files SET status = 'missing', updated_at_ms = ?
         WHERE root_id = ? AND (scan_generation IS NULL OR scan_generation != ?) AND status != 'missing';",
    )
    .bind(now_ms)
    .bind(root_id.as_str())
    .bind(current_generation)
    .execute(&mut *conn)
    .await?;

    let folders_res = sqlx::query(
        "UPDATE folders SET status = 'missing'
         WHERE root_id = ? AND (last_seen_generation IS NULL OR last_seen_generation != ?)
           AND status != 'missing' AND parent_id IS NOT NULL;",
    )
    .bind(root_id.as_str())
    .bind(current_generation)
    .execute(&mut *conn)
    .await?;

    sqlx::query(
        "UPDATE library_roots SET last_scan_at_ms = ?, last_scan_generation = ? WHERE id = ?;",
    )
    .bind(now_ms)
    .bind(current_generation)
    .bind(root_id.as_str())
    .execute(&mut *conn)
    .await?;

    Ok((files_res.rows_affected(), folders_res.rows_affected()))
}

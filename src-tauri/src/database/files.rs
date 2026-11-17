use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};
use std::str::FromStr;

use crate::database::models::FileRow;
use crate::domain::{CatalogueError, FileId, FileStatus, FolderId, RootId};

/// Validates that an unsigned file size fits in SQLite's signed 64-bit INTEGER (spec §5.1).
pub fn validate_file_size(size: u64) -> Result<i64, CatalogueError> {
    if size > i64::MAX as u64 {
        Err(CatalogueError::SizeOutOfRange { size })
    } else {
        Ok(size as i64)
    }
}

/// Inserts a physical file record (spec §5.2, D27).
///
/// If a file with the same `(root_id, path_key)` already exists,
/// returns `CatalogueError::PathKeyCollision`.
pub async fn insert_file(
    conn: &mut SqliteConnection,
    file: &FileRow,
) -> Result<(), CatalogueError> {
    let result = sqlx::query(
        "INSERT INTO files (
           id, root_id, folder_id, name, extension, rel_path_display, rel_path_native, path_key,
           size_bytes, modified_at_ms, created_at_ms, file_identity, status,
           scan_generation, last_verified_at_ms, created_in_catalog_at_ms, updated_at_ms
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?);",
    )
    .bind(file.id.as_str())
    .bind(file.root_id.as_str())
    .bind(file.folder_id.as_str())
    .bind(&file.name)
    .bind(&file.extension)
    .bind(&file.rel_path_display)
    .bind(&file.rel_path_native)
    .bind(&file.path_key)
    .bind(file.size_bytes)
    .bind(file.modified_at_ms)
    .bind(file.created_at_ms)
    .bind(&file.file_identity)
    .bind(file.status.as_str())
    .bind(&file.scan_generation)
    .bind(file.last_verified_at_ms)
    .bind(file.created_in_catalog_at_ms)
    .bind(file.updated_at_ms)
    .execute(&mut *conn)
    .await;

    match result {
        Ok(_) => Ok(()),
        Err(err) => {
            if err.as_database_error().map(|e| e.kind())
                == Some(sqlx::error::ErrorKind::UniqueViolation)
            {
                if let Ok(Some(existing)) =
                    find_file_by_key(conn, &file.root_id, &file.path_key).await
                {
                    return Err(CatalogueError::PathKeyCollision {
                        root_id: file.root_id.to_string(),
                        path_key: file.path_key.clone(),
                        existing_id: Some(existing.id.to_string()),
                    });
                }
            }
            Err(CatalogueError::Database(err))
        }
    }
}

/// Finds a file by library root and path_key.
pub async fn find_file_by_key(
    conn: &mut SqliteConnection,
    root_id: &RootId,
    path_key: &[u8],
) -> Result<Option<FileRow>, CatalogueError> {
    let row: Option<SqliteRow> = sqlx::query(
        "SELECT id, root_id, folder_id, name, extension, rel_path_display, rel_path_native, path_key,
           size_bytes, modified_at_ms, created_at_ms, file_identity, status,
           scan_generation, last_verified_at_ms, created_in_catalog_at_ms, updated_at_ms
         FROM files WHERE root_id = ? AND path_key = ?;",
    )
    .bind(root_id.as_str())
    .bind(path_key)
    .fetch_optional(&mut *conn)
    .await?;

    match row {
        Some(r) => Ok(Some(row_to_file(&r)?)),
        None => Ok(None),
    }
}

/// Counts total files for a given root.
pub async fn count_for_root(
    conn: &mut SqliteConnection,
    root_id: &RootId,
) -> Result<i64, CatalogueError> {
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM files WHERE root_id = ?;")
        .bind(root_id.as_str())
        .fetch_one(&mut *conn)
        .await?;

    Ok(row.0)
}

fn row_to_file(row: &SqliteRow) -> Result<FileRow, CatalogueError> {
    let id_str: String = row.try_get("id")?;
    let root_id_str: String = row.try_get("root_id")?;
    let folder_id_str: String = row.try_get("folder_id")?;
    let name: String = row.try_get("name")?;
    let extension: Option<String> = row.try_get("extension")?;
    let rel_path_display: String = row.try_get("rel_path_display")?;
    let rel_path_native: Vec<u8> = row.try_get("rel_path_native")?;
    let path_key: Vec<u8> = row.try_get("path_key")?;
    let size_bytes: i64 = row.try_get("size_bytes")?;
    let modified_at_ms: i64 = row.try_get("modified_at_ms")?;
    let created_at_ms: Option<i64> = row.try_get("created_at_ms")?;
    let file_identity: Option<String> = row.try_get("file_identity")?;
    let status_str: String = row.try_get("status")?;
    let scan_generation: Option<String> = row.try_get("scan_generation")?;
    let last_verified_at_ms: Option<i64> = row.try_get("last_verified_at_ms")?;
    let created_in_catalog_at_ms: i64 = row.try_get("created_in_catalog_at_ms")?;
    let updated_at_ms: i64 = row.try_get("updated_at_ms")?;

    let status = FileStatus::from_str(&status_str)
        .map_err(|e| CatalogueError::InvalidEnum(e.to_string()))?;

    Ok(FileRow {
        id: FileId::from(id_str),
        root_id: RootId::from(root_id_str),
        folder_id: FolderId::from(folder_id_str),
        name,
        extension,
        rel_path_display,
        rel_path_native,
        path_key,
        size_bytes,
        modified_at_ms,
        created_at_ms,
        file_identity,
        status,
        scan_generation,
        last_verified_at_ms,
        created_in_catalog_at_ms,
        updated_at_ms,
    })
}

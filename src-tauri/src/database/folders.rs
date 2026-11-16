use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};
use std::str::FromStr;

use crate::database::models::FolderRow;
use crate::domain::{CatalogueError, FolderId, FolderStatus, RootId};

/// Inserts a new folder record (spec §5.2, D27).
///
/// If a folder with the same `(root_id, path_key)` already exists,
/// returns `CatalogueError::PathKeyCollision`.
pub async fn insert_folder(
    conn: &mut SqliteConnection,
    folder: &FolderRow,
) -> Result<(), CatalogueError> {
    let result = sqlx::query(
        "INSERT INTO folders (
           id, root_id, parent_id, name, rel_path_display, rel_path_native, path_key,
           status, modified_at_ms, last_seen_generation
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?);",
    )
    .bind(folder.id.as_str())
    .bind(folder.root_id.as_str())
    .bind(folder.parent_id.as_ref().map(|p| p.as_str()))
    .bind(&folder.name)
    .bind(&folder.rel_path_display)
    .bind(&folder.rel_path_native)
    .bind(&folder.path_key)
    .bind(folder.status.as_str())
    .bind(folder.modified_at_ms)
    .bind(&folder.last_seen_generation)
    .execute(&mut *conn)
    .await;

    match result {
        Ok(_) => Ok(()),
        Err(err) => {
            let s = err.to_string();
            if s.contains("UNIQUE constraint failed")
                && (s.contains("path_key") || s.contains("folders.root_id"))
            {
                Err(CatalogueError::PathKeyCollision {
                    root_id: folder.root_id.to_string(),
                    path_key: folder.path_key.clone(),
                })
            } else {
                Err(CatalogueError::Database(err))
            }
        }
    }
}

/// Finds a folder by library root and path_key.
pub async fn find_folder_by_key(
    conn: &mut SqliteConnection,
    root_id: &RootId,
    path_key: &[u8],
) -> Result<Option<FolderRow>, CatalogueError> {
    let row: Option<SqliteRow> = sqlx::query(
        "SELECT id, root_id, parent_id, name, rel_path_display, rel_path_native, path_key,
           status, modified_at_ms, last_seen_generation
         FROM folders WHERE root_id = ? AND path_key = ?;",
    )
    .bind(root_id.as_str())
    .bind(path_key)
    .fetch_optional(&mut *conn)
    .await?;

    match row {
        Some(r) => Ok(Some(row_to_folder(&r)?)),
        None => Ok(None),
    }
}

/// Lists direct children of a folder (or top-level folders if parent_id is None).
pub async fn list_children(
    conn: &mut SqliteConnection,
    root_id: &RootId,
    parent_id: Option<&FolderId>,
) -> Result<Vec<FolderRow>, CatalogueError> {
    let rows: Vec<SqliteRow> =
        match parent_id {
            Some(pid) => sqlx::query(
                "SELECT id, root_id, parent_id, name, rel_path_display, rel_path_native, path_key,
                   status, modified_at_ms, last_seen_generation
                 FROM folders WHERE root_id = ? AND parent_id = ? ORDER BY name;",
            )
            .bind(root_id.as_str())
            .bind(pid.as_str())
            .fetch_all(&mut *conn)
            .await?,
            None => sqlx::query(
                "SELECT id, root_id, parent_id, name, rel_path_display, rel_path_native, path_key,
                   status, modified_at_ms, last_seen_generation
                 FROM folders WHERE root_id = ? AND parent_id IS NULL ORDER BY name;",
            )
            .bind(root_id.as_str())
            .fetch_all(&mut *conn)
            .await?,
        };

    let mut folders = Vec::with_capacity(rows.len());
    for r in rows {
        folders.push(row_to_folder(&r)?);
    }
    Ok(folders)
}

fn row_to_folder(row: &SqliteRow) -> Result<FolderRow, CatalogueError> {
    let id_str: String = row.try_get("id")?;
    let root_id_str: String = row.try_get("root_id")?;
    let parent_id_str: Option<String> = row.try_get("parent_id")?;
    let name: String = row.try_get("name")?;
    let rel_path_display: String = row.try_get("rel_path_display")?;
    let rel_path_native: Vec<u8> = row.try_get("rel_path_native")?;
    let path_key: Vec<u8> = row.try_get("path_key")?;
    let status_str: String = row.try_get("status")?;
    let modified_at_ms: Option<i64> = row.try_get("modified_at_ms")?;
    let last_seen_generation: Option<String> = row.try_get("last_seen_generation")?;

    let status = FolderStatus::from_str(&status_str)
        .map_err(|e| CatalogueError::InvalidEnum(e.to_string()))?;

    Ok(FolderRow {
        id: FolderId::from(id_str),
        root_id: RootId::from(root_id_str),
        parent_id: parent_id_str.map(FolderId::from),
        name,
        rel_path_display,
        rel_path_native,
        path_key,
        status,
        modified_at_ms,
        last_seen_generation,
    })
}

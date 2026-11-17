use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};
use std::str::FromStr;

use crate::database::models::RootRow;
use crate::domain::{CatalogueError, RootId, RootStatus, VolumeId};

/// Inserts a new library root (spec §5.2).
pub async fn insert_root(
    conn: &mut SqliteConnection,
    root: &RootRow,
) -> Result<(), CatalogueError> {
    sqlx::query(
        "INSERT INTO library_roots (
           id, volume_id, label, path_display, path_native, status,
           follow_symlinks, include_hidden, created_at_ms, updated_at_ms,
           last_scan_at_ms, last_scan_generation
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?);",
    )
    .bind(root.id.as_str())
    .bind(root.volume_id.as_ref().map(|v| v.as_str()))
    .bind(&root.label)
    .bind(&root.path_display)
    .bind(&root.path_native)
    .bind(root.status.as_str())
    .bind(if root.follow_symlinks { 1i32 } else { 0i32 })
    .bind(if root.include_hidden { 1i32 } else { 0i32 })
    .bind(root.created_at_ms)
    .bind(root.updated_at_ms)
    .bind(root.last_scan_at_ms)
    .bind(&root.last_scan_generation)
    .execute(&mut *conn)
    .await?;

    Ok(())
}

/// Fetches a library root by its identifier.
pub async fn get_root(
    conn: &mut SqliteConnection,
    id: &RootId,
) -> Result<Option<RootRow>, CatalogueError> {
    let row: Option<SqliteRow> = sqlx::query(
        "SELECT id, volume_id, label, path_display, path_native, status,
           follow_symlinks, include_hidden, created_at_ms, updated_at_ms,
           last_scan_at_ms, last_scan_generation
         FROM library_roots WHERE id = ?;",
    )
    .bind(id.as_str())
    .fetch_optional(&mut *conn)
    .await?;

    match row {
        Some(r) => Ok(Some(row_to_root(&r)?)),
        None => Ok(None),
    }
}

/// Lists all library roots.
pub async fn list_roots(conn: &mut SqliteConnection) -> Result<Vec<RootRow>, CatalogueError> {
    let rows: Vec<SqliteRow> = sqlx::query(
        "SELECT id, volume_id, label, path_display, path_native, status,
           follow_symlinks, include_hidden, created_at_ms, updated_at_ms,
           last_scan_at_ms, last_scan_generation
         FROM library_roots ORDER BY created_at_ms;",
    )
    .fetch_all(&mut *conn)
    .await?;

    let mut roots = Vec::with_capacity(rows.len());
    for r in rows {
        roots.push(row_to_root(&r)?);
    }
    Ok(roots)
}

/// Updates the status and updated_at_ms of a library root.
pub async fn set_root_status(
    conn: &mut SqliteConnection,
    id: &RootId,
    status: RootStatus,
    updated_at_ms: i64,
) -> Result<(), CatalogueError> {
    let result =
        sqlx::query("UPDATE library_roots SET status = ?, updated_at_ms = ? WHERE id = ?;")
            .bind(status.as_str())
            .bind(updated_at_ms)
            .bind(id.as_str())
            .execute(&mut *conn)
            .await?;

    if result.rows_affected() == 0 {
        return Err(CatalogueError::NotFound {
            entity: "library_root",
            id: id.to_string(),
        });
    }

    Ok(())
}

/// Deletes a library root from the catalogue.
///
/// Removes catalogue rows only (cascades in SQLite to folders, files, and assets).
/// Never touches the filesystem.
pub async fn delete_root(conn: &mut SqliteConnection, id: &RootId) -> Result<(), CatalogueError> {
    let result = sqlx::query("DELETE FROM library_roots WHERE id = ?;")
        .bind(id.as_str())
        .execute(&mut *conn)
        .await?;

    if result.rows_affected() == 0 {
        return Err(CatalogueError::NotFound {
            entity: "library_root",
            id: id.to_string(),
        });
    }

    Ok(())
}

/// Counts total file rows belonging to a library root.
pub async fn count_files(conn: &mut SqliteConnection, id: &RootId) -> Result<i64, CatalogueError> {
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM files WHERE root_id = ?;")
        .bind(id.as_str())
        .fetch_one(&mut *conn)
        .await?;

    Ok(row.0)
}

fn row_to_root(row: &SqliteRow) -> Result<RootRow, CatalogueError> {
    let id_str: String = row.try_get("id")?;
    let volume_id_str: Option<String> = row.try_get("volume_id")?;
    let label: String = row.try_get("label")?;
    let path_display: String = row.try_get("path_display")?;
    let path_native: Vec<u8> = row.try_get("path_native")?;
    let status_str: String = row.try_get("status")?;
    let follow_symlinks: i32 = row.try_get("follow_symlinks")?;
    let include_hidden: i32 = row.try_get("include_hidden")?;
    let created_at_ms: i64 = row.try_get("created_at_ms")?;
    let updated_at_ms: i64 = row.try_get("updated_at_ms")?;
    let last_scan_at_ms: Option<i64> = row.try_get("last_scan_at_ms")?;
    let last_scan_generation: Option<String> = row.try_get("last_scan_generation")?;

    let status = RootStatus::from_str(&status_str)
        .map_err(|e| CatalogueError::InvalidEnum(e.to_string()))?;

    Ok(RootRow {
        id: RootId::from(id_str),
        volume_id: volume_id_str.map(VolumeId::from),
        label,
        path_display,
        path_native,
        status,
        follow_symlinks: follow_symlinks != 0,
        include_hidden: include_hidden != 0,
        created_at_ms,
        updated_at_ms,
        last_scan_at_ms,
        last_scan_generation,
    })
}

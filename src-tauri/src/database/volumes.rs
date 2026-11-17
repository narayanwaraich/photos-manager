use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};
use std::str::FromStr;

use crate::database::models::VolumeRow;
use crate::domain::{CatalogueError, VolumeId, VolumeKind};

/// Inserts a new storage volume record (spec §5.2).
pub async fn insert_volume(
    conn: &mut SqliteConnection,
    volume: &VolumeRow,
) -> Result<(), CatalogueError> {
    sqlx::query(
        "INSERT INTO volumes (id, kind, label, fs_uuid, fs_type, case_sensitive, last_seen_at_ms)
         VALUES (?, ?, ?, ?, ?, ?, ?);",
    )
    .bind(volume.id.as_str())
    .bind(volume.kind.as_str())
    .bind(&volume.label)
    .bind(&volume.fs_uuid)
    .bind(&volume.fs_type)
    .bind(if volume.case_sensitive { 1i32 } else { 0i32 })
    .bind(volume.last_seen_at_ms)
    .execute(&mut *conn)
    .await?;

    Ok(())
}

/// Fetches a volume by its identifier.
pub async fn get_volume(
    conn: &mut SqliteConnection,
    id: &VolumeId,
) -> Result<Option<VolumeRow>, CatalogueError> {
    let row: Option<SqliteRow> = sqlx::query(
        "SELECT id, kind, label, fs_uuid, fs_type, case_sensitive, last_seen_at_ms
         FROM volumes WHERE id = ?;",
    )
    .bind(id.as_str())
    .fetch_optional(&mut *conn)
    .await?;

    match row {
        Some(r) => Ok(Some(row_to_volume(&r)?)),
        None => Ok(None),
    }
}

/// Lists all known volumes.
pub async fn list_volumes(conn: &mut SqliteConnection) -> Result<Vec<VolumeRow>, CatalogueError> {
    let rows: Vec<SqliteRow> = sqlx::query(
        "SELECT id, kind, label, fs_uuid, fs_type, case_sensitive, last_seen_at_ms
         FROM volumes ORDER BY id;",
    )
    .fetch_all(&mut *conn)
    .await?;

    let mut volumes = Vec::with_capacity(rows.len());
    for r in rows {
        volumes.push(row_to_volume(&r)?);
    }
    Ok(volumes)
}

fn row_to_volume(row: &SqliteRow) -> Result<VolumeRow, CatalogueError> {
    let id_str: String = row.try_get("id")?;
    let kind_str: String = row.try_get("kind")?;
    let label: Option<String> = row.try_get("label")?;
    let fs_uuid: Option<String> = row.try_get("fs_uuid")?;
    let fs_type: Option<String> = row.try_get("fs_type")?;
    let cs_int: i32 = row.try_get("case_sensitive")?;
    let last_seen_at_ms: Option<i64> = row.try_get("last_seen_at_ms")?;

    let kind =
        VolumeKind::from_str(&kind_str).map_err(|e| CatalogueError::InvalidEnum(e.to_string()))?;

    Ok(VolumeRow {
        id: VolumeId::from(id_str),
        kind,
        label,
        fs_uuid,
        fs_type,
        case_sensitive: cs_int != 0,
        last_seen_at_ms,
    })
}

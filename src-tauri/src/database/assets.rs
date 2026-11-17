use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};
use std::str::FromStr;

use crate::database::models::AssetRow;
use crate::domain::{AssetId, AssetKind, AssetStatus, CatalogueError, FileId};

/// Inserts an asset row along with its primary entry in `asset_files` (spec §5.2, D27).
///
/// Runs in the caller's transaction/connection.
pub async fn insert_asset_with_primary_file(
    conn: &mut SqliteConnection,
    asset: &AssetRow,
    role: Option<&str>,
) -> Result<(), CatalogueError> {
    sqlx::query(
        "INSERT INTO assets (
           id, kind, primary_file_id, format, width, height, orientation,
           date_taken_ms, date_taken_source, date_taken_raw, tz_offset_min,
           camera_make, camera_model, lens_model, iso, aperture, shutter_speed,
           focal_length, latitude, longitude, is_favorite, status,
           created_in_catalog_at_ms, updated_at_ms
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?);",
    )
    .bind(asset.id.as_str())
    .bind(asset.kind.as_str())
    .bind(asset.primary_file_id.as_str())
    .bind(&asset.format)
    .bind(asset.width)
    .bind(asset.height)
    .bind(asset.orientation)
    .bind(asset.date_taken_ms)
    .bind(&asset.date_taken_source)
    .bind(&asset.date_taken_raw)
    .bind(asset.tz_offset_min)
    .bind(&asset.camera_make)
    .bind(&asset.camera_model)
    .bind(&asset.lens_model)
    .bind(asset.iso)
    .bind(asset.aperture)
    .bind(&asset.shutter_speed)
    .bind(asset.focal_length)
    .bind(asset.latitude)
    .bind(asset.longitude)
    .bind(if asset.is_favorite { 1i32 } else { 0i32 })
    .bind(asset.status.as_str())
    .bind(asset.created_in_catalog_at_ms)
    .bind(asset.updated_at_ms)
    .execute(&mut *conn)
    .await?;

    let role_str = role.unwrap_or("primary");
    sqlx::query("INSERT INTO asset_files (asset_id, file_id, role) VALUES (?, ?, ?);")
        .bind(asset.id.as_str())
        .bind(asset.primary_file_id.as_str())
        .bind(role_str)
        .execute(&mut *conn)
        .await?;

    Ok(())
}

/// Fetches an asset by its identifier.
pub async fn get_asset(
    conn: &mut SqliteConnection,
    id: &AssetId,
) -> Result<Option<AssetRow>, CatalogueError> {
    let row: Option<SqliteRow> = sqlx::query(
        "SELECT id, kind, primary_file_id, format, width, height, orientation,
           date_taken_ms, date_taken_source, date_taken_raw, tz_offset_min,
           camera_make, camera_model, lens_model, iso, aperture, shutter_speed,
           focal_length, latitude, longitude, is_favorite, status,
           created_in_catalog_at_ms, updated_at_ms
         FROM assets WHERE id = ?;",
    )
    .bind(id.as_str())
    .fetch_optional(&mut *conn)
    .await?;

    match row {
        Some(r) => Ok(Some(row_to_asset(&r)?)),
        None => Ok(None),
    }
}

fn row_to_asset(row: &SqliteRow) -> Result<AssetRow, CatalogueError> {
    let id_str: String = row.try_get("id")?;
    let kind_str: String = row.try_get("kind")?;
    let primary_file_id_str: String = row.try_get("primary_file_id")?;
    let format: Option<String> = row.try_get("format")?;
    let width: Option<i64> = row.try_get("width")?;
    let height: Option<i64> = row.try_get("height")?;
    let orientation: Option<i64> = row.try_get("orientation")?;
    let date_taken_ms: Option<i64> = row.try_get("date_taken_ms")?;
    let date_taken_source: Option<String> = row.try_get("date_taken_source")?;
    let date_taken_raw: Option<String> = row.try_get("date_taken_raw")?;
    let tz_offset_min: Option<i64> = row.try_get("tz_offset_min")?;
    let camera_make: Option<String> = row.try_get("camera_make")?;
    let camera_model: Option<String> = row.try_get("camera_model")?;
    let lens_model: Option<String> = row.try_get("lens_model")?;
    let iso: Option<i64> = row.try_get("iso")?;
    let aperture: Option<f64> = row.try_get("aperture")?;
    let shutter_speed: Option<String> = row.try_get("shutter_speed")?;
    let focal_length: Option<f64> = row.try_get("focal_length")?;
    let latitude: Option<f64> = row.try_get("latitude")?;
    let longitude: Option<f64> = row.try_get("longitude")?;
    let is_fav_int: i32 = row.try_get("is_favorite")?;
    let status_str: String = row.try_get("status")?;
    let created_in_catalog_at_ms: i64 = row.try_get("created_in_catalog_at_ms")?;
    let updated_at_ms: i64 = row.try_get("updated_at_ms")?;

    let kind =
        AssetKind::from_str(&kind_str).map_err(|e| CatalogueError::InvalidEnum(e.to_string()))?;
    let status = AssetStatus::from_str(&status_str)
        .map_err(|e| CatalogueError::InvalidEnum(e.to_string()))?;

    Ok(AssetRow {
        id: AssetId::from(id_str),
        kind,
        primary_file_id: FileId::from(primary_file_id_str),
        format,
        width,
        height,
        orientation,
        date_taken_ms,
        date_taken_source,
        date_taken_raw,
        tz_offset_min,
        camera_make,
        camera_model,
        lens_model,
        iso,
        aperture,
        shutter_speed,
        focal_length,
        latitude,
        longitude,
        is_favorite: is_fav_int != 0,
        status,
        created_in_catalog_at_ms,
        updated_at_ms,
    })
}

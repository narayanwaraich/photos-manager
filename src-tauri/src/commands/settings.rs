use sqlx::SqlitePool;

use crate::commands::error::IpcError;
use crate::database;
use crate::domain::CatalogueError;

/// Plain async implementation of `get_auto_scan` (TASK-1G decision o).
/// Defaults to `false` when no `auto_scan` row exists in `app_settings`.
pub async fn get_auto_scan_impl(pool: &SqlitePool) -> Result<bool, IpcError> {
    let mut conn = pool
        .acquire()
        .await
        .map_err(CatalogueError::Database)
        .map_err(IpcError::from)?;
    database::get_auto_scan(&mut conn)
        .await
        .map_err(IpcError::from)
}

/// Plain async implementation of `set_auto_scan` (TASK-1G decision o).
pub async fn set_auto_scan_impl(pool: &SqlitePool, enabled: bool) -> Result<bool, IpcError> {
    let now_ms = database::now_ms().map_err(IpcError::from)?;
    let mut conn = pool
        .acquire()
        .await
        .map_err(CatalogueError::Database)
        .map_err(IpcError::from)?;
    database::set_auto_scan(&mut conn, enabled, now_ms)
        .await
        .map_err(IpcError::from)?;
    Ok(enabled)
}

#[tauri::command]
pub async fn get_auto_scan(pool: tauri::State<'_, SqlitePool>) -> Result<bool, IpcError> {
    get_auto_scan_impl(&pool).await
}

#[tauri::command]
pub async fn set_auto_scan(
    pool: tauri::State<'_, SqlitePool>,
    enabled: bool,
) -> Result<bool, IpcError> {
    set_auto_scan_impl(&pool, enabled).await
}

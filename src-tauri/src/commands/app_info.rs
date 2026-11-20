use super::error::IpcError;
use crate::domain::CatalogueError;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppInfoDto {
    pub app_version: String,
    pub platform_encoding: String,
    pub migrations_applied: u32,
}

/// Plain async function implementing the app info retrieval (spec §12.1).
/// Tests call this directly with an in-memory or fixture pool without Tauri runtime.
pub async fn app_info(pool: &SqlitePool) -> Result<AppInfoDto, IpcError> {
    let app_version = env!("CARGO_PKG_VERSION").to_string();

    let mut conn = pool.acquire().await.map_err(CatalogueError::Database)?;

    let row: Option<(String,)> =
        sqlx::query_as("SELECT value_json FROM app_settings WHERE key = 'native_path_encoding';")
            .fetch_optional(&mut *conn)
            .await
            .map_err(CatalogueError::Database)?;

    let platform_encoding = match row {
        Some((value_json,)) => serde_json::from_str(&value_json).unwrap_or(value_json),
        None => "unknown".to_string(),
    };

    let migrations_applied: u32 = {
        let count_row: Option<(i64,)> =
            sqlx::query_as("SELECT COUNT(*) FROM _sqlx_migrations WHERE success = 1;")
                .fetch_optional(&mut *conn)
                .await
                .map_err(CatalogueError::Database)?;

        count_row.map(|(c,)| c as u32).unwrap_or(0)
    };

    Ok(AppInfoDto {
        app_version,
        platform_encoding,
        migrations_applied,
    })
}

#[tauri::command]
pub async fn get_app_info(pool: tauri::State<'_, SqlitePool>) -> Result<AppInfoDto, IpcError> {
    app_info(&pool).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::init_pool;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_app_info_returns_expected_fields() {
        let temp_dir = tempdir().expect("tempdir");
        let db_path = temp_dir.path().join("app_info_test.db");

        let pool = init_pool(&db_path).await.expect("init_pool");
        let info = app_info(&pool).await.expect("app_info");

        assert_eq!(info.app_version, env!("CARGO_PKG_VERSION"));
        #[cfg(windows)]
        assert_eq!(info.platform_encoding, "windows-utf16le");
        #[cfg(not(windows))]
        assert_eq!(info.platform_encoding, "unix-bytes");
        assert!(info.migrations_applied >= 1, "Must have applied migrations");
    }

    #[tokio::test]
    async fn test_app_info_closed_pool_returns_database_error() {
        let temp_dir = tempdir().expect("tempdir");
        let db_path = temp_dir.path().join("app_info_closed_pool.db");

        let pool = init_pool(&db_path).await.expect("init_pool");
        pool.close().await;

        let err = app_info(&pool)
            .await
            .expect_err("app_info on closed pool must return an error");

        assert_eq!(err.code, "database");
        assert_eq!(
            err.message,
            crate::commands::error::fixed_message_for_code("database")
        );
    }
}

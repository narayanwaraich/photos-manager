use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions};
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DatabaseError {
    #[error("SQLx error: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("Migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),
}

/// Creates a SQLite connection pool configured according to spec §5.1:
/// - WAL journal mode
/// - Foreign keys enabled
/// - Busy timeout configured (5000 ms)
/// - One versioned migration run
pub async fn init_pool(db_path: &Path) -> Result<SqlitePool, DatabaseError> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    let options = SqliteConnectOptions::new()
        .filename(db_path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true)
        .busy_timeout(std::time::Duration::from_millis(5000));

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;

    // Run versioned migrations from migrations/ directory
    sqlx::migrate!("./migrations").run(&pool).await?;

    Ok(pool)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_init_pool_and_app_settings_migration() {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let db_path = temp_dir.path().join("test_catalogue.db");

        let pool = init_pool(&db_path)
            .await
            .expect("Failed to initialize database pool");

        // Verify WAL mode
        let journal_mode: (String,) = sqlx::query_as("PRAGMA journal_mode;")
            .fetch_one(&pool)
            .await
            .expect("Failed to fetch journal_mode");
        assert_eq!(journal_mode.0.to_lowercase(), "wal");

        // Verify foreign keys are on
        let foreign_keys: (i32,) = sqlx::query_as("PRAGMA foreign_keys;")
            .fetch_one(&pool)
            .await
            .expect("Failed to fetch foreign_keys");
        assert_eq!(foreign_keys.0, 1);

        // Verify busy timeout
        let busy_timeout: (i32,) = sqlx::query_as("PRAGMA busy_timeout;")
            .fetch_one(&pool)
            .await
            .expect("Failed to fetch busy_timeout");
        assert_eq!(busy_timeout.0, 5000);

        // Insert a row into app_settings
        let key = "app_theme";
        let value_json = "{\"mode\":\"dark\"}";
        let now_ms: i64 = 1772450000000;

        sqlx::query("INSERT INTO app_settings (key, value_json, updated_at_ms) VALUES (?, ?, ?);")
            .bind(key)
            .bind(value_json)
            .bind(now_ms)
            .execute(&pool)
            .await
            .expect("Failed to insert into app_settings");

        // Read the row back
        let row: (String, String, i64) = sqlx::query_as(
            "SELECT key, value_json, updated_at_ms FROM app_settings WHERE key = ?;",
        )
        .bind(key)
        .fetch_one(&pool)
        .await
        .expect("Failed to read from app_settings");

        assert_eq!(row.0, key);
        assert_eq!(row.1, value_json);
        assert_eq!(row.2, now_ms);
    }
}

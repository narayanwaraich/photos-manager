pub mod assets;
pub mod files;
pub mod folders;
pub mod models;
pub mod roots;
pub mod volumes;

pub use assets::*;
pub use files::*;
pub use folders::*;
pub use models::*;
pub use roots::*;
pub use volumes::*;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions};
use sqlx::SqliteConnection;
use std::path::Path;

use crate::domain::CatalogueError;

/// Current platform native path encoding identifier (spec §5.1, D26).
pub const CURRENT_PLATFORM_ENCODING: &str = if cfg!(windows) {
    "windows-utf16le"
} else {
    "unix-bytes"
};

/// Enforces the catalogue platform guard:
/// The first time a database is opened, records `native_path_encoding` in `app_settings`.
/// If opened on a mismatched platform, returns `CatalogueError::PlatformMismatch`.
pub async fn check_or_set_platform_guard(
    conn: &mut SqliteConnection,
    now_ms: i64,
) -> Result<(), CatalogueError> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT value_json FROM app_settings WHERE key = 'native_path_encoding';")
            .fetch_optional(&mut *conn)
            .await?;

    if let Some((value_json,)) = row {
        let stored_val: String = serde_json::from_str(&value_json).unwrap_or(value_json);
        if stored_val != CURRENT_PLATFORM_ENCODING {
            return Err(CatalogueError::PlatformMismatch {
                expected: CURRENT_PLATFORM_ENCODING.to_string(),
                found: stored_val,
            });
        }
    } else {
        let json_val = serde_json::to_string(CURRENT_PLATFORM_ENCODING)
            .map_err(|e| CatalogueError::Database(sqlx::Error::Decode(Box::new(e))))?;
        sqlx::query("INSERT INTO app_settings (key, value_json, updated_at_ms) VALUES (?, ?, ?);")
            .bind("native_path_encoding")
            .bind(json_val)
            .bind(now_ms)
            .execute(&mut *conn)
            .await?;
    }

    Ok(())
}

/// Returns the current Unix epoch timestamp in milliseconds (spec §5.1, §5.2).
pub fn now_ms() -> Result<i64, CatalogueError> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| CatalogueError::Database(sqlx::Error::Decode(Box::new(e))))?
        .as_millis()
        .try_into()
        .map_err(|_| CatalogueError::SizeOutOfRange { size: u64::MAX })
}

/// Creates a SQLite connection pool configured according to spec §5.1:
/// - WAL journal mode
/// - Foreign keys enabled on every connection
/// - Busy timeout configured (5000 ms)
/// - Versioned migrations applied
/// - Platform guard validated
pub async fn init_pool(db_path: &Path) -> Result<SqlitePool, CatalogueError> {
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

    let now_ms = now_ms()?;

    // Check if app_settings table exists before running migrations (look, refuse, only then touch)
    let mut conn = pool.acquire().await?;
    let has_app_settings: Option<(String,)> = sqlx::query_as(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'app_settings';",
    )
    .fetch_optional(&mut *conn)
    .await?;

    if has_app_settings.is_some() {
        // Database already initialized: check platform guard BEFORE running migrations
        check_or_set_platform_guard(&mut conn, now_ms).await?;
    }

    // Release connection borrow before running migrations on the pool
    drop(conn);

    // Run versioned migrations from migrations/ directory
    sqlx::migrate!("./migrations").run(&pool).await?;

    if has_app_settings.is_none() {
        // Fresh database: stamp platform guard after migrations created app_settings
        let mut conn = pool.acquire().await?;
        check_or_set_platform_guard(&mut conn, now_ms).await?;
    }

    Ok(pool)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        AssetId, AssetKind, AssetStatus, FileId, FileStatus, FolderId, FolderStatus, RootId,
        RootStatus, VolumeId, VolumeKind,
    };
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

    // G.4: Migration from previous schema (0A -> Phase 1)
    #[tokio::test]
    async fn test_migration_0a_to_phase1() {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let db_path = temp_dir.path().join("migration_test.db");

        let options = SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .connect_with(options)
            .await
            .expect("Connect failed");

        // Apply only the 0A schema manually
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS app_settings (
                key TEXT PRIMARY KEY,
                value_json TEXT NOT NULL,
                updated_at_ms INTEGER NOT NULL
            );",
        )
        .execute(&pool)
        .await
        .expect("Failed to apply 0A schema");

        // Insert pre-existing app_settings row
        sqlx::query("INSERT INTO app_settings (key, value_json, updated_at_ms) VALUES (?, ?, ?);")
            .bind("existing_key")
            .bind("{\"version\": 0}")
            .bind(1000i64)
            .execute(&pool)
            .await
            .expect("Failed to insert into 0A app_settings");

        // Run the full migrator
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("Migrator failed");

        // Assert existing row survived
        let row: (String, String, i64) = sqlx::query_as(
            "SELECT key, value_json, updated_at_ms FROM app_settings WHERE key = 'existing_key';",
        )
        .fetch_one(&pool)
        .await
        .expect("Pre-existing row must survive migration");
        assert_eq!(row.0, "existing_key");
        assert_eq!(row.1, "{\"version\": 0}");
        assert_eq!(row.2, 1000);

        // Assert the six new tables exist
        let tables = [
            "volumes",
            "library_roots",
            "folders",
            "files",
            "assets",
            "asset_files",
        ];
        for table in tables {
            let exists: (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?;",
            )
            .bind(table)
            .fetch_one(&pool)
            .await
            .expect("Table check query failed");
            assert_eq!(exists.0, 1, "Table '{}' must exist after migration", table);
        }

        // Assert the three indexes exist
        let indexes = ["idx_files_size", "idx_files_identity", "idx_assets_date"];
        for idx in indexes {
            let exists: (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = ?;",
            )
            .bind(idx)
            .fetch_one(&pool)
            .await
            .expect("Index check query failed");
            assert_eq!(exists.0, 1, "Index '{}' must exist after migration", idx);
        }
    }

    // G.5: Foreign keys enabled on every connection in the pool
    #[tokio::test]
    async fn test_foreign_keys_on_every_connection() {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let db_path = temp_dir.path().join("fk_test.db");

        let pool = init_pool(&db_path).await.expect("Failed to init pool");

        // Acquire connection 1 and verify foreign keys
        let mut conn1 = pool.acquire().await.expect("Acquire conn1 failed");
        let fk1: (i32,) = sqlx::query_as("PRAGMA foreign_keys;")
            .fetch_one(&mut *conn1)
            .await
            .expect("Query fk1 failed");
        assert_eq!(fk1.0, 1);

        // Acquire connection 2 and verify foreign keys
        let mut conn2 = pool.acquire().await.expect("Acquire conn2 failed");
        let fk2: (i32,) = sqlx::query_as("PRAGMA foreign_keys;")
            .fetch_one(&mut *conn2)
            .await
            .expect("Query fk2 failed");
        assert_eq!(fk2.0, 1);

        // Inserting file with non-existent root_id must fail foreign key check
        let bad_file = FileRow {
            id: FileId::new(),
            root_id: RootId::new(),     // Non-existent
            folder_id: FolderId::new(), // Non-existent
            name: "test.jpg".into(),
            extension: Some("jpg".into()),
            rel_path_display: "test.jpg".into(),
            rel_path_native: b"test.jpg".to_vec(),
            path_key: b"test.jpg".to_vec(),
            size_bytes: 100,
            modified_at_ms: 1000,
            created_at_ms: None,
            file_identity: None,
            status: FileStatus::Present,
            scan_generation: None,
            last_verified_at_ms: None,
            created_in_catalog_at_ms: 1000,
            updated_at_ms: 1000,
        };

        let result = insert_file(&mut conn1, &bad_file).await;
        assert!(
            result.is_err(),
            "Inserting file with invalid root_id and folder_id must fail FK check"
        );
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("FOREIGN KEY constraint failed"),
            "Error must be foreign key violation, got: {}",
            err_msg
        );
    }

    // G.6: Uniqueness collision on (root_id, path_key)
    #[tokio::test]
    async fn test_uniqueness_path_key_collision() {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let db_path = temp_dir.path().join("unique_test.db");

        let pool = init_pool(&db_path).await.expect("Failed to init pool");
        let mut conn = pool.acquire().await.expect("Acquire failed");

        let root_id = RootId::new();
        let root = RootRow {
            id: root_id.clone(),
            volume_id: None,
            label: "Photos Root".into(),
            path_display: "/photos".into(),
            path_native: b"/photos".to_vec(),
            status: RootStatus::Active,
            follow_symlinks: false,
            include_hidden: false,
            created_at_ms: 1000,
            updated_at_ms: 1000,
            last_scan_at_ms: None,
            last_scan_generation: None,
        };
        insert_root(&mut conn, &root)
            .await
            .expect("Insert root failed");

        // Insert first folder
        let folder1 = FolderRow {
            id: FolderId::new(),
            root_id: root_id.clone(),
            parent_id: None,
            name: "Vacation".into(),
            rel_path_display: "vacation".into(),
            rel_path_native: b"vacation".to_vec(),
            path_key: b"vacation".to_vec(),
            status: FolderStatus::Present,
            modified_at_ms: None,
            last_seen_generation: None,
        };
        insert_folder(&mut conn, &folder1)
            .await
            .expect("First folder insert must succeed");

        // Insert second folder with identical (root_id, path_key)
        let folder2 = FolderRow {
            id: FolderId::new(),
            root_id: root_id.clone(),
            parent_id: None,
            name: "Vacation Duplicate".into(),
            rel_path_display: "vacation".into(),
            rel_path_native: b"vacation".to_vec(),
            path_key: b"vacation".to_vec(),
            status: FolderStatus::Present,
            modified_at_ms: None,
            last_seen_generation: None,
        };
        let err = insert_folder(&mut conn, &folder2)
            .await
            .expect_err("Duplicate folder path_key must fail");
        assert_eq!(err.code(), "path_key_collision");

        // Insert first file
        let file1 = FileRow {
            id: FileId::new(),
            root_id: root_id.clone(),
            folder_id: folder1.id.clone(),
            name: "pic.jpg".into(),
            extension: Some("jpg".into()),
            rel_path_display: "vacation/pic.jpg".into(),
            rel_path_native: b"vacation/pic.jpg".to_vec(),
            path_key: b"vacation/pic.jpg".to_vec(),
            size_bytes: 200,
            modified_at_ms: 1000,
            created_at_ms: None,
            file_identity: None,
            status: FileStatus::Present,
            scan_generation: None,
            last_verified_at_ms: None,
            created_in_catalog_at_ms: 1000,
            updated_at_ms: 1000,
        };
        insert_file(&mut conn, &file1)
            .await
            .expect("First file insert must succeed");

        // Insert second file with identical (root_id, path_key)
        let file2 = FileRow {
            id: FileId::new(),
            root_id: root_id.clone(),
            folder_id: folder1.id.clone(),
            name: "pic2.jpg".into(),
            extension: Some("jpg".into()),
            rel_path_display: "vacation/pic.jpg".into(),
            rel_path_native: b"vacation/pic.jpg".to_vec(),
            path_key: b"vacation/pic.jpg".to_vec(),
            size_bytes: 300,
            modified_at_ms: 2000,
            created_at_ms: None,
            file_identity: None,
            status: FileStatus::Present,
            scan_generation: None,
            last_verified_at_ms: None,
            created_in_catalog_at_ms: 2000,
            updated_at_ms: 2000,
        };
        let file_err = insert_file(&mut conn, &file2)
            .await
            .expect_err("Duplicate file path_key must fail");
        assert_eq!(file_err.code(), "path_key_collision");
    }

    // G.7: Cascade delete root test
    #[tokio::test]
    async fn test_cascade_delete_root() {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let db_path = temp_dir.path().join("cascade_test.db");

        let pool = init_pool(&db_path).await.expect("Failed to init pool");
        let mut conn = pool.acquire().await.expect("Acquire failed");

        // 1. Volume
        let vol_id = VolumeId::new();
        let volume = VolumeRow {
            id: vol_id.clone(),
            kind: VolumeKind::Internal,
            label: Some("Macintosh HD".into()),
            fs_uuid: Some("vol-uuid-1234".into()),
            fs_type: Some("apfs".into()),
            case_sensitive: false,
            last_seen_at_ms: Some(1000),
        };
        insert_volume(&mut conn, &volume)
            .await
            .expect("Insert volume failed");

        // 2. Library root
        let root_id = RootId::new();
        let root = RootRow {
            id: root_id.clone(),
            volume_id: Some(vol_id.clone()),
            label: "Main Library".into(),
            path_display: "/Volumes/Data/Photos".into(),
            path_native: b"/Volumes/Data/Photos".to_vec(),
            status: RootStatus::Active,
            follow_symlinks: false,
            include_hidden: false,
            created_at_ms: 1000,
            updated_at_ms: 1000,
            last_scan_at_ms: None,
            last_scan_generation: None,
        };
        insert_root(&mut conn, &root)
            .await
            .expect("Insert root failed");

        // 3. Root-folder row (empty relative path and key, parent_id NULL, D27)
        let root_folder_id = FolderId::new();
        let root_folder = FolderRow {
            id: root_folder_id.clone(),
            root_id: root_id.clone(),
            parent_id: None,
            name: "".into(),
            rel_path_display: "".into(),
            rel_path_native: Vec::new(),
            path_key: Vec::new(),
            status: FolderStatus::Present,
            modified_at_ms: Some(1000),
            last_seen_generation: None,
        };
        insert_folder(&mut conn, &root_folder)
            .await
            .expect("Insert root folder row failed");

        // 4. Two sub-folders (children of root-folder row)
        let sub1_id = FolderId::new();
        let sub1 = FolderRow {
            id: sub1_id.clone(),
            root_id: root_id.clone(),
            parent_id: Some(root_folder_id.clone()),
            name: "sub1".into(),
            rel_path_display: "sub1".into(),
            rel_path_native: b"sub1".to_vec(),
            path_key: b"sub1".to_vec(),
            status: FolderStatus::Present,
            modified_at_ms: Some(1000),
            last_seen_generation: None,
        };
        insert_folder(&mut conn, &sub1)
            .await
            .expect("Insert sub1 failed");

        let sub2_id = FolderId::new();
        let sub2 = FolderRow {
            id: sub2_id.clone(),
            root_id: root_id.clone(),
            parent_id: Some(root_folder_id.clone()),
            name: "sub2".into(),
            rel_path_display: "sub2".into(),
            rel_path_native: b"sub2".to_vec(),
            path_key: b"sub2".to_vec(),
            status: FolderStatus::Present,
            modified_at_ms: Some(1000),
            last_seen_generation: None,
        };
        insert_folder(&mut conn, &sub2)
            .await
            .expect("Insert sub2 failed");

        // 5. Three files
        let file1_id = FileId::new();
        let file1 = FileRow {
            id: file1_id.clone(),
            root_id: root_id.clone(),
            folder_id: sub1_id.clone(),
            name: "img1.jpg".into(),
            extension: Some("jpg".into()),
            rel_path_display: "sub1/img1.jpg".into(),
            rel_path_native: b"sub1/img1.jpg".to_vec(),
            path_key: b"sub1/img1.jpg".to_vec(),
            size_bytes: 1024,
            modified_at_ms: 1000,
            created_at_ms: None,
            file_identity: None,
            status: FileStatus::Present,
            scan_generation: None,
            last_verified_at_ms: None,
            created_in_catalog_at_ms: 1000,
            updated_at_ms: 1000,
        };
        insert_file(&mut conn, &file1)
            .await
            .expect("Insert file1 failed");

        let file2_id = FileId::new();
        let file2 = FileRow {
            id: file2_id.clone(),
            root_id: root_id.clone(),
            folder_id: sub1_id.clone(),
            name: "img2.jpg".into(),
            extension: Some("jpg".into()),
            rel_path_display: "sub1/img2.jpg".into(),
            rel_path_native: b"sub1/img2.jpg".to_vec(),
            path_key: b"sub1/img2.jpg".to_vec(),
            size_bytes: 2048,
            modified_at_ms: 1000,
            created_at_ms: None,
            file_identity: None,
            status: FileStatus::Present,
            scan_generation: None,
            last_verified_at_ms: None,
            created_in_catalog_at_ms: 1000,
            updated_at_ms: 1000,
        };
        insert_file(&mut conn, &file2)
            .await
            .expect("Insert file2 failed");

        let file3_id = FileId::new();
        let file3 = FileRow {
            id: file3_id.clone(),
            root_id: root_id.clone(),
            folder_id: sub2_id.clone(),
            name: "img3.jpg".into(),
            extension: Some("jpg".into()),
            rel_path_display: "sub2/img3.jpg".into(),
            rel_path_native: b"sub2/img3.jpg".to_vec(),
            path_key: b"sub2/img3.jpg".to_vec(),
            size_bytes: 4096,
            modified_at_ms: 1000,
            created_at_ms: None,
            file_identity: None,
            status: FileStatus::Present,
            scan_generation: None,
            last_verified_at_ms: None,
            created_in_catalog_at_ms: 1000,
            updated_at_ms: 1000,
        };
        insert_file(&mut conn, &file3)
            .await
            .expect("Insert file3 failed");

        // 6. Three assets with asset_files rows
        let make_asset = |aid: AssetId, fid: FileId| AssetRow {
            id: aid,
            kind: AssetKind::Image,
            primary_file_id: fid,
            format: Some("jpeg".into()),
            width: Some(1920),
            height: Some(1080),
            orientation: Some(1),
            date_taken_ms: Some(1700000000000),
            date_taken_source: Some("exif".into()),
            date_taken_raw: Some("2023:11:14 10:00:00".into()),
            tz_offset_min: Some(0),
            camera_make: Some("Canon".into()),
            camera_model: Some("EOS".into()),
            lens_model: None,
            iso: Some(100),
            aperture: Some(2.8),
            shutter_speed: Some("1/250".into()),
            focal_length: Some(50.0),
            latitude: None,
            longitude: None,
            is_favorite: false,
            status: AssetStatus::Present,
            created_in_catalog_at_ms: 1000,
            updated_at_ms: 1000,
        };

        let asset1_id = AssetId::new();
        let asset1 = make_asset(asset1_id.clone(), file1_id.clone());
        insert_asset_with_primary_file(&mut conn, &asset1, None)
            .await
            .expect("Insert asset1 failed");

        let asset2_id = AssetId::new();
        let asset2 = make_asset(asset2_id.clone(), file2_id.clone());
        insert_asset_with_primary_file(&mut conn, &asset2, None)
            .await
            .expect("Insert asset2 failed");

        let asset3_id = AssetId::new();
        let asset3 = make_asset(asset3_id.clone(), file3_id.clone());
        insert_asset_with_primary_file(&mut conn, &asset3, None)
            .await
            .expect("Insert asset3 failed");

        // Verify initial counts
        assert_eq!(count_files(&mut conn, &root_id).await.unwrap(), 3);

        // Delete the root - all child folders, files, and assets must cascade delete
        delete_root(&mut conn, &root_id)
            .await
            .expect("delete_root failed");

        // Verify all dependent rows are gone
        assert_eq!(get_root(&mut conn, &root_id).await.unwrap(), None);
        assert_eq!(count_files(&mut conn, &root_id).await.unwrap(), 0);

        let root_children = list_children(&mut conn, &root_id, None).await.unwrap();
        assert!(root_children.is_empty());

        assert_eq!(get_asset(&mut conn, &asset1_id).await.unwrap(), None);
        assert_eq!(get_asset(&mut conn, &asset2_id).await.unwrap(), None);
        assert_eq!(get_asset(&mut conn, &asset3_id).await.unwrap(), None);

        let asset_files_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM asset_files;")
            .fetch_one(&mut *conn)
            .await
            .unwrap();
        assert_eq!(asset_files_count.0, 0);

        // Volume row must remain
        let vol_check = get_volume(&mut conn, &vol_id).await.unwrap();
        assert!(
            vol_check.is_some(),
            "Volume row must remain after root deletion"
        );
    }

    // G.8: Enum and CHECK sync & size range
    #[tokio::test]
    async fn test_enum_check_sync_and_size_range() {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let db_path = temp_dir.path().join("enum_check_test.db");

        let pool = init_pool(&db_path).await.expect("Failed to init pool");
        let mut conn = pool.acquire().await.expect("Acquire failed");

        // 1. VolumeKind variants
        for kind in VolumeKind::all_variants() {
            let vid = VolumeId::new();
            let vol = VolumeRow {
                id: vid.clone(),
                kind: *kind,
                label: None,
                fs_uuid: None,
                fs_type: None,
                case_sensitive: false,
                last_seen_at_ms: None,
            };
            insert_volume(&mut conn, &vol)
                .await
                .expect("VolumeKind insert failed");
        }
        // Unlisted VolumeKind rejected
        let bad_vol_res = sqlx::query("INSERT INTO volumes (id, kind) VALUES ('bad_v', 'bogus');")
            .execute(&mut *conn)
            .await;
        assert!(bad_vol_res.is_err());
        assert!(bad_vol_res
            .unwrap_err()
            .to_string()
            .contains("CHECK constraint failed"));

        // Setup base root for status tests
        let root_id = RootId::new();
        let base_root = RootRow {
            id: root_id.clone(),
            volume_id: None,
            label: "Test".into(),
            path_display: "/test".into(),
            path_native: b"/test".to_vec(),
            status: RootStatus::Active,
            follow_symlinks: false,
            include_hidden: false,
            created_at_ms: 1000,
            updated_at_ms: 1000,
            last_scan_at_ms: None,
            last_scan_generation: None,
        };
        insert_root(&mut conn, &base_root).await.unwrap();

        // 2. RootStatus variants
        for status in RootStatus::all_variants() {
            set_root_status(&mut conn, &root_id, *status, 2000)
                .await
                .expect("RootStatus update failed");
        }
        let bad_root_res = sqlx::query("UPDATE library_roots SET status = 'bogus' WHERE id = ?;")
            .bind(root_id.as_str())
            .execute(&mut *conn)
            .await;
        assert!(bad_root_res.is_err());
        assert!(bad_root_res
            .unwrap_err()
            .to_string()
            .contains("CHECK constraint failed"));

        // Setup base folder
        let base_folder_id = FolderId::new();
        let base_folder = FolderRow {
            id: base_folder_id.clone(),
            root_id: root_id.clone(),
            parent_id: None,
            name: "root_dir".into(),
            rel_path_display: "".into(),
            rel_path_native: Vec::new(),
            path_key: Vec::new(),
            status: FolderStatus::Present,
            modified_at_ms: None,
            last_seen_generation: None,
        };
        insert_folder(&mut conn, &base_folder).await.unwrap();

        // 3. FolderStatus variants
        for status in FolderStatus::all_variants() {
            let fid = FolderId::new();
            let key = fid.as_str().as_bytes().to_vec();
            let folder = FolderRow {
                id: fid,
                root_id: root_id.clone(),
                parent_id: Some(base_folder_id.clone()),
                name: "f".into(),
                rel_path_display: "f".into(),
                rel_path_native: b"f".to_vec(),
                path_key: key,
                status: *status,
                modified_at_ms: None,
                last_seen_generation: None,
            };
            insert_folder(&mut conn, &folder)
                .await
                .expect("FolderStatus insert failed");
        }
        let bad_folder_res = sqlx::query("UPDATE folders SET status = 'bogus' WHERE id = ?;")
            .bind(base_folder_id.as_str())
            .execute(&mut *conn)
            .await;
        assert!(bad_folder_res.is_err());
        assert!(bad_folder_res
            .unwrap_err()
            .to_string()
            .contains("CHECK constraint failed"));

        // 4. FileStatus variants
        let mut last_file_id = FileId::new();
        for status in FileStatus::all_variants() {
            let fid = FileId::new();
            last_file_id = fid.clone();
            let key = fid.as_str().as_bytes().to_vec();
            let file = FileRow {
                id: fid,
                root_id: root_id.clone(),
                folder_id: base_folder_id.clone(),
                name: "f.jpg".into(),
                extension: Some("jpg".into()),
                rel_path_display: "f.jpg".into(),
                rel_path_native: b"f.jpg".to_vec(),
                path_key: key,
                size_bytes: 100,
                modified_at_ms: 1000,
                created_at_ms: None,
                file_identity: None,
                status: *status,
                scan_generation: None,
                last_verified_at_ms: None,
                created_in_catalog_at_ms: 1000,
                updated_at_ms: 1000,
            };
            insert_file(&mut conn, &file)
                .await
                .expect("FileStatus insert failed");
        }
        let bad_file_res = sqlx::query("UPDATE files SET status = 'bogus' WHERE id = ?;")
            .bind(last_file_id.as_str())
            .execute(&mut *conn)
            .await;
        assert!(bad_file_res.is_err());
        assert!(bad_file_res
            .unwrap_err()
            .to_string()
            .contains("CHECK constraint failed"));

        // 5. AssetKind and AssetStatus variants
        let base_file_id = FileId::new();
        let base_file = FileRow {
            id: base_file_id.clone(),
            root_id: root_id.clone(),
            folder_id: base_folder_id.clone(),
            name: "asset_base.jpg".into(),
            extension: Some("jpg".into()),
            rel_path_display: "asset_base.jpg".into(),
            rel_path_native: b"asset_base.jpg".to_vec(),
            path_key: b"asset_base.jpg".to_vec(),
            size_bytes: 100,
            modified_at_ms: 1000,
            created_at_ms: None,
            file_identity: None,
            status: FileStatus::Present,
            scan_generation: None,
            last_verified_at_ms: None,
            created_in_catalog_at_ms: 1000,
            updated_at_ms: 1000,
        };
        insert_file(&mut conn, &base_file).await.unwrap();

        let mut last_asset_id = AssetId::new();
        for kind in AssetKind::all_variants() {
            for status in AssetStatus::all_variants() {
                let aid = AssetId::new();
                last_asset_id = aid.clone();
                let asset = AssetRow {
                    id: aid,
                    kind: *kind,
                    primary_file_id: base_file_id.clone(),
                    format: Some("jpeg".into()),
                    width: None,
                    height: None,
                    orientation: None,
                    date_taken_ms: None,
                    date_taken_source: None,
                    date_taken_raw: None,
                    tz_offset_min: None,
                    camera_make: None,
                    camera_model: None,
                    lens_model: None,
                    iso: None,
                    aperture: None,
                    shutter_speed: None,
                    focal_length: None,
                    latitude: None,
                    longitude: None,
                    is_favorite: false,
                    status: *status,
                    created_in_catalog_at_ms: 1000,
                    updated_at_ms: 1000,
                };
                insert_asset_with_primary_file(&mut conn, &asset, None)
                    .await
                    .expect("Asset variant insert failed");
            }
        }
        let bad_asset_kind_res = sqlx::query("UPDATE assets SET kind = 'bogus' WHERE id = ?;")
            .bind(last_asset_id.as_str())
            .execute(&mut *conn)
            .await;
        assert!(bad_asset_kind_res.is_err());
        assert!(bad_asset_kind_res
            .unwrap_err()
            .to_string()
            .contains("CHECK constraint failed"));

        let bad_asset_status_res = sqlx::query("UPDATE assets SET status = 'bogus' WHERE id = ?;")
            .bind(last_asset_id.as_str())
            .execute(&mut *conn)
            .await;
        assert!(bad_asset_status_res.is_err());
        assert!(bad_asset_status_res
            .unwrap_err()
            .to_string()
            .contains("CHECK constraint failed"));

        // 6. Size range validation
        // Size above i64::MAX is rejected before SQL
        let oversized: u64 = (i64::MAX as u64) + 1;
        let size_res = crate::database::files::validate_file_size(oversized);
        assert_eq!(size_res.err().map(|e| e.code()), Some("size_out_of_range"));

        // Negative size rejected by SQL CHECK
        let neg_file_res = sqlx::query(
            "INSERT INTO files (
               id, root_id, folder_id, name, rel_path_display, rel_path_native, path_key,
               size_bytes, modified_at_ms, created_in_catalog_at_ms, updated_at_ms
             ) VALUES ('neg_id', ?, ?, 'neg.jpg', 'neg.jpg', X'6E', X'6E', -1, 1000, 1000, 1000);",
        )
        .bind(root_id.as_str())
        .bind(base_folder_id.as_str())
        .execute(&mut *conn)
        .await;

        assert!(neg_file_res.is_err());
        assert!(neg_file_res
            .unwrap_err()
            .to_string()
            .contains("CHECK constraint failed"));
    }

    // G.9: Platform guard rejects mismatch
    #[tokio::test]
    async fn test_platform_guard_rejects_mismatch() {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let db_path = temp_dir.path().join("platform_test.db");

        // Pre-create database with foreign encoding
        let foreign_encoding = if cfg!(windows) {
            "\"unix-bytes\""
        } else {
            "\"windows-utf16le\""
        };

        let options = SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .connect_with(options)
            .await
            .expect("Connect failed");

        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("Migrate failed");

        sqlx::query("INSERT INTO app_settings (key, value_json, updated_at_ms) VALUES (?, ?, ?);")
            .bind("native_path_encoding")
            .bind(foreign_encoding)
            .bind(1000i64)
            .execute(&pool)
            .await
            .expect("Insert foreign stamp failed");

        drop(pool);

        // Now open via init_pool - platform guard must reject it
        let open_res = init_pool(&db_path).await;
        assert!(open_res.is_err());
        let err = open_res.unwrap_err();
        assert_eq!(err.code(), "catalogue_platform_mismatch");
    }

    // G.8: FromStr rejection for all 6 enums
    #[test]
    fn test_enum_from_str_rejection_all_enums() {
        use std::str::FromStr;
        assert!(VolumeKind::from_str("bogus").is_err());
        assert!(RootStatus::from_str("bogus").is_err());
        assert!(FolderStatus::from_str("bogus").is_err());
        assert!(FileStatus::from_str("bogus").is_err());
        assert!(AssetKind::from_str("bogus").is_err());
        assert!(AssetStatus::from_str("bogus").is_err());
    }

    // G.6 & Review item 4: Duplicate primary key with different path_key returns Database error, not PathKeyCollision
    #[tokio::test]
    async fn test_duplicate_primary_key_is_database_error_not_collision() {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let db_path = temp_dir.path().join("dup_pk_test.db");
        let pool = init_pool(&db_path).await.expect("Failed to init pool");
        let mut conn = pool.acquire().await.expect("Acquire failed");

        let vol_id = VolumeId::new();
        let volume = VolumeRow {
            id: vol_id.clone(),
            kind: VolumeKind::Internal,
            label: Some("HD".into()),
            fs_uuid: None,
            fs_type: None,
            case_sensitive: false,
            last_seen_at_ms: None,
        };
        insert_volume(&mut conn, &volume).await.unwrap();

        let root_id = RootId::new();
        let root = RootRow {
            id: root_id.clone(),
            volume_id: Some(vol_id),
            label: "Photos".into(),
            path_display: "/photos".into(),
            path_native: b"/photos".to_vec(),
            status: RootStatus::Active,
            follow_symlinks: false,
            include_hidden: false,
            created_at_ms: 1000,
            updated_at_ms: 1000,
            last_scan_at_ms: None,
            last_scan_generation: None,
        };
        insert_root(&mut conn, &root).await.unwrap();

        // 1. Folders: duplicate ID with DIFFERENT path_key
        let folder_id = FolderId::new();
        let folder1 = FolderRow {
            id: folder_id.clone(),
            root_id: root_id.clone(),
            parent_id: None,
            name: "f1".into(),
            rel_path_display: "f1".into(),
            rel_path_native: b"f1".to_vec(),
            path_key: b"f1".to_vec(),
            status: FolderStatus::Present,
            modified_at_ms: None,
            last_seen_generation: None,
        };
        insert_folder(&mut conn, &folder1).await.unwrap();

        let folder2 = FolderRow {
            id: folder_id.clone(), // Duplicate primary key!
            root_id: root_id.clone(),
            parent_id: None,
            name: "f2".into(),
            rel_path_display: "f2".into(),
            rel_path_native: b"f2".to_vec(),
            path_key: b"f2".to_vec(), // Different path_key!
            status: FolderStatus::Present,
            modified_at_ms: None,
            last_seen_generation: None,
        };
        let f_err = insert_folder(&mut conn, &folder2).await.unwrap_err();
        assert_eq!(f_err.code(), "database");
        assert!(!matches!(f_err, CatalogueError::PathKeyCollision { .. }));

        // 2. Files: duplicate ID with DIFFERENT path_key
        let file_id = FileId::new();
        let file1 = FileRow {
            id: file_id.clone(),
            root_id: root_id.clone(),
            folder_id: folder_id.clone(),
            name: "a.jpg".into(),
            extension: Some("jpg".into()),
            rel_path_display: "f1/a.jpg".into(),
            rel_path_native: b"f1/a.jpg".to_vec(),
            path_key: b"f1/a.jpg".to_vec(),
            size_bytes: 100,
            modified_at_ms: 1000,
            created_at_ms: None,
            file_identity: None,
            status: FileStatus::Present,
            scan_generation: None,
            last_verified_at_ms: None,
            created_in_catalog_at_ms: 1000,
            updated_at_ms: 1000,
        };
        insert_file(&mut conn, &file1).await.unwrap();

        let file2 = FileRow {
            id: file_id.clone(), // Duplicate primary key!
            root_id: root_id.clone(),
            folder_id: folder_id.clone(),
            name: "b.jpg".into(),
            extension: Some("jpg".into()),
            rel_path_display: "f1/b.jpg".into(),
            rel_path_native: b"f1/b.jpg".to_vec(),
            path_key: b"f1/b.jpg".to_vec(), // Different path_key!
            size_bytes: 200,
            modified_at_ms: 2000,
            created_at_ms: None,
            file_identity: None,
            status: FileStatus::Present,
            scan_generation: None,
            last_verified_at_ms: None,
            created_in_catalog_at_ms: 2000,
            updated_at_ms: 2000,
        };
        let file_err = insert_file(&mut conn, &file2).await.unwrap_err();
        assert_eq!(file_err.code(), "database");
        assert!(!matches!(file_err, CatalogueError::PathKeyCollision { .. }));
    }

    // Review item 1: Test 5 repository functions
    // (list_volumes, list_roots, find_folder_by_key, find_file_by_key, count_for_root)
    #[tokio::test]
    async fn test_repository_functions_crud() {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let db_path = temp_dir.path().join("repo_crud_test.db");
        let pool = init_pool(&db_path).await.expect("Failed to init pool");
        let mut conn = pool.acquire().await.expect("Acquire failed");

        // 1. list_volumes (returns what was inserted in stable order ORDER BY id)
        let vol1 = VolumeRow {
            id: VolumeId::from("vol_a"),
            kind: VolumeKind::Internal,
            label: Some("Volume A".into()),
            fs_uuid: None,
            fs_type: None,
            case_sensitive: false,
            last_seen_at_ms: None,
        };
        let vol2 = VolumeRow {
            id: VolumeId::from("vol_b"),
            kind: VolumeKind::Removable,
            label: Some("Volume B".into()),
            fs_uuid: None,
            fs_type: None,
            case_sensitive: true,
            last_seen_at_ms: None,
        };
        insert_volume(&mut conn, &vol2).await.unwrap();
        insert_volume(&mut conn, &vol1).await.unwrap();

        let vol_list = list_volumes(&mut conn).await.unwrap();
        assert_eq!(vol_list.len(), 2);
        assert_eq!(vol_list[0].id.as_str(), "vol_a");
        assert_eq!(vol_list[1].id.as_str(), "vol_b");

        // 2. list_roots (returns what was inserted in stable order ORDER BY created_at_ms)
        let root1 = RootRow {
            id: RootId::new(),
            volume_id: Some(vol1.id.clone()),
            label: "Root 1".into(),
            path_display: "/path/1".into(),
            path_native: b"/path/1".to_vec(),
            status: RootStatus::Active,
            follow_symlinks: false,
            include_hidden: false,
            created_at_ms: 1000,
            updated_at_ms: 1000,
            last_scan_at_ms: None,
            last_scan_generation: None,
        };
        let root2 = RootRow {
            id: RootId::new(),
            volume_id: Some(vol2.id.clone()),
            label: "Root 2".into(),
            path_display: "/path/2".into(),
            path_native: b"/path/2".to_vec(),
            status: RootStatus::Active,
            follow_symlinks: false,
            include_hidden: false,
            created_at_ms: 2000,
            updated_at_ms: 2000,
            last_scan_at_ms: None,
            last_scan_generation: None,
        };
        insert_root(&mut conn, &root2).await.unwrap();
        insert_root(&mut conn, &root1).await.unwrap();

        let root_list = list_roots(&mut conn).await.unwrap();
        assert_eq!(root_list.len(), 2);
        assert_eq!(root_list[0].id, root1.id);
        assert_eq!(root_list[1].id, root2.id);

        // 3. find_folder_by_key (hit and miss)
        let folder = FolderRow {
            id: FolderId::new(),
            root_id: root1.id.clone(),
            parent_id: None,
            name: "vacation".into(),
            rel_path_display: "vacation".into(),
            rel_path_native: b"vacation".to_vec(),
            path_key: b"vacation".to_vec(),
            status: FolderStatus::Present,
            modified_at_ms: None,
            last_seen_generation: None,
        };
        insert_folder(&mut conn, &folder).await.unwrap();

        // Hit
        let found_folder = find_folder_by_key(&mut conn, &root1.id, b"vacation")
            .await
            .unwrap();
        assert!(found_folder.is_some());
        assert_eq!(found_folder.unwrap().id, folder.id);

        // Miss: wrong key
        let miss_folder_key = find_folder_by_key(&mut conn, &root1.id, b"unknown")
            .await
            .unwrap();
        assert!(miss_folder_key.is_none());

        // Miss: wrong root_id
        let miss_folder_root = find_folder_by_key(&mut conn, &root2.id, b"vacation")
            .await
            .unwrap();
        assert!(miss_folder_root.is_none());

        // 4. count_for_root and find_file_by_key (hits, misses, count after inserts and after delete)
        let count0 = count_for_root(&mut conn, &root1.id).await.unwrap();
        assert_eq!(count0, 0);

        let file1 = FileRow {
            id: FileId::new(),
            root_id: root1.id.clone(),
            folder_id: folder.id.clone(),
            name: "beach.jpg".into(),
            extension: Some("jpg".into()),
            rel_path_display: "vacation/beach.jpg".into(),
            rel_path_native: b"vacation/beach.jpg".to_vec(),
            path_key: b"vacation/beach.jpg".to_vec(),
            size_bytes: 1024,
            modified_at_ms: 1000,
            created_at_ms: None,
            file_identity: None,
            status: FileStatus::Present,
            scan_generation: None,
            last_verified_at_ms: None,
            created_in_catalog_at_ms: 1000,
            updated_at_ms: 1000,
        };
        let file2 = FileRow {
            id: FileId::new(),
            root_id: root1.id.clone(),
            folder_id: folder.id.clone(),
            name: "sunset.jpg".into(),
            extension: Some("jpg".into()),
            rel_path_display: "vacation/sunset.jpg".into(),
            rel_path_native: b"vacation/sunset.jpg".to_vec(),
            path_key: b"vacation/sunset.jpg".to_vec(),
            size_bytes: 2048,
            modified_at_ms: 1000,
            created_at_ms: None,
            file_identity: None,
            status: FileStatus::Present,
            scan_generation: None,
            last_verified_at_ms: None,
            created_in_catalog_at_ms: 1000,
            updated_at_ms: 1000,
        };
        insert_file(&mut conn, &file1).await.unwrap();
        insert_file(&mut conn, &file2).await.unwrap();

        let count2 = count_for_root(&mut conn, &root1.id).await.unwrap();
        assert_eq!(count2, 2);

        // find_file_by_key: Hit
        let found_file = find_file_by_key(&mut conn, &root1.id, b"vacation/beach.jpg")
            .await
            .unwrap();
        assert!(found_file.is_some());
        assert_eq!(found_file.unwrap().id, file1.id);

        // find_file_by_key: Miss (wrong key)
        let miss_file_key = find_file_by_key(&mut conn, &root1.id, b"vacation/missing.jpg")
            .await
            .unwrap();
        assert!(miss_file_key.is_none());

        // find_file_by_key: Miss (wrong root)
        let miss_file_root = find_file_by_key(&mut conn, &root2.id, b"vacation/beach.jpg")
            .await
            .unwrap();
        assert!(miss_file_root.is_none());

        // count_for_root after a delete
        sqlx::query("DELETE FROM files WHERE id = ?;")
            .bind(file1.id.as_str())
            .execute(&mut *conn)
            .await
            .unwrap();

        let count_after_delete = count_for_root(&mut conn, &root1.id).await.unwrap();
        assert_eq!(count_after_delete, 1);
    }

    // Review item 6: Pre-migration platform guard rejection
    // Database with only 0A migration and foreign stamp is refused BEFORE migrations run,
    // and sqlite_master has no Phase 1 tables (volumes table does NOT exist).
    #[tokio::test]
    async fn test_platform_guard_pre_migration_rejection() {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let db_path = temp_dir.path().join("pre_migration_guard.db");

        // 1. Create a database with only the 0A schema (app_settings table)
        let options = SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true);
        let pool_0a = SqlitePoolOptions::new()
            .connect_with(options)
            .await
            .expect("Failed to connect 0A pool");

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS app_settings (
               key TEXT PRIMARY KEY NOT NULL,
               value_json TEXT NOT NULL,
               updated_at_ms INTEGER NOT NULL
             );",
        )
        .execute(&pool_0a)
        .await
        .expect("Failed to create 0A table");

        // 2. Insert a foreign platform stamp
        let foreign_encoding = if CURRENT_PLATFORM_ENCODING == "windows-utf16le" {
            "unix-bytes"
        } else {
            "windows-utf16le"
        };
        sqlx::query("INSERT INTO app_settings (key, value_json, updated_at_ms) VALUES (?, ?, ?);")
            .bind("native_path_encoding")
            .bind(serde_json::to_string(foreign_encoding).unwrap())
            .bind(1000i64)
            .execute(&pool_0a)
            .await
            .expect("Failed to insert foreign stamp");

        drop(pool_0a);

        // 3. Attempt to open via init_pool - must be refused before migrations run
        let open_res = init_pool(&db_path).await;
        assert!(matches!(
            open_res,
            Err(CatalogueError::PlatformMismatch { .. })
        ));

        // 4. Verify that Phase 1 migrations were NOT applied (volumes table does NOT exist)
        let check_options = SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(false);
        let check_pool = SqlitePoolOptions::new()
            .connect_with(check_options)
            .await
            .expect("Failed to connect to check db");

        let volumes_table: Option<(String,)> = sqlx::query_as(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'volumes';",
        )
        .fetch_optional(&check_pool)
        .await
        .expect("Failed to query sqlite_master");

        assert!(
            volumes_table.is_none(),
            "Phase 1 'volumes' table must NOT exist when database is refused by platform guard"
        );
    }

    #[tokio::test]
    async fn test_migration_one_root_folder_per_root_invariant() {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let db_path = temp_dir.path().join("migration_3_test.db");

        // 1. Set up database with migrations 1 & 2 applied manually
        let options = SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .connect_with(options)
            .await
            .expect("Failed to connect");

        let m1 = include_str!("../../migrations/20261102000000_create_app_settings.sql");
        let m2 = include_str!("../../migrations/20261116000000_create_phase1_catalogue.sql");
        sqlx::raw_sql(m1).execute(&pool).await.expect("Run m1");
        sqlx::raw_sql(m2).execute(&pool).await.expect("Run m2");

        // Insert root 1 and its initial root-folder row (parent_id IS NULL)
        let root1_id = "root_test_001";
        sqlx::query(
            "INSERT INTO library_roots (
                id, label, path_display, path_native, status,
                follow_symlinks, include_hidden, created_at_ms, updated_at_ms
             ) VALUES (?, 'Root 1', '/photos1', X'2f70686f746f7331', 'active', 0, 0, 1000, 1000);",
        )
        .bind(root1_id)
        .execute(&pool)
        .await
        .expect("Insert root 1");

        let folder1_id = "folder_root_001";
        sqlx::query(
            "INSERT INTO folders (
                id, root_id, parent_id, name, rel_path_display, rel_path_native, path_key, status
             ) VALUES (?, ?, NULL, '', '', X'', X'', 'present');",
        )
        .bind(folder1_id)
        .bind(root1_id)
        .execute(&pool)
        .await
        .expect("Insert root-folder 1");

        // 2. Apply migration 3 on top of the populated database
        let m3 = include_str!("../../migrations/20261121000000_one_root_folder_per_root.sql");
        sqlx::raw_sql(m3)
            .execute(&pool)
            .await
            .expect("Run m3 on existing catalogue");

        // 3. Attempt to insert a second root-folder row (parent_id IS NULL) for root 1
        let second_folder_for_root1 = "folder_root_001_dupe";
        let dupe_res = sqlx::query(
            "INSERT INTO folders (
                id, root_id, parent_id, name, rel_path_display, rel_path_native, path_key, status
             ) VALUES (?, ?, NULL, 'dupe', 'dupe', X'64757065', X'64757065', 'present');",
        )
        .bind(second_folder_for_root1)
        .bind(root1_id)
        .execute(&pool)
        .await;

        assert!(
            dupe_res.is_err(),
            "A second parentless folder for the same root must fail unique constraint"
        );
        let err = dupe_res.unwrap_err();
        let db_err = err.as_database_error().expect("Must be database error");
        assert_eq!(db_err.kind(), sqlx::error::ErrorKind::UniqueViolation);

        // 4. Insert root 2 and its own root-folder row (parent_id IS NULL) - must succeed
        let root2_id = "root_test_002";
        sqlx::query(
            "INSERT INTO library_roots (
                id, label, path_display, path_native, status,
                follow_symlinks, include_hidden, created_at_ms, updated_at_ms
             ) VALUES (?, 'Root 2', '/photos2', X'2f70686f746f7332', 'active', 0, 0, 2000, 2000);",
        )
        .bind(root2_id)
        .execute(&pool)
        .await
        .expect("Insert root 2");

        let folder2_id = "folder_root_002";
        sqlx::query(
            "INSERT INTO folders (
                id, root_id, parent_id, name, rel_path_display, rel_path_native, path_key, status
             ) VALUES (?, ?, NULL, '', '', X'', X'', 'present');",
        )
        .bind(folder2_id)
        .bind(root2_id)
        .execute(&pool)
        .await
        .expect("A second root's own root-folder row must succeed");
    }
}

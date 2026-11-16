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

    // Run versioned migrations from migrations/ directory
    sqlx::migrate!("./migrations").run(&pool).await?;

    // Validate platform guard
    let mut conn = pool.acquire().await?;
    check_or_set_platform_guard(&mut conn, 0).await?;

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
        for status in FileStatus::all_variants() {
            let fid = FileId::new();
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

        for kind in AssetKind::all_variants() {
            for status in AssetStatus::all_variants() {
                let aid = AssetId::new();
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
}

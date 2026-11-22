pub mod commands;
pub mod database;
pub mod domain;
pub mod filesystem;
pub mod image_pipeline;
pub mod library;
pub mod metadata;
pub mod platform;
pub mod smoke;

use image_pipeline::ImageProcessor;
use std::path::{Path, PathBuf};
use tauri::Manager;

/// Startup error covering all failure modes during application initialization (spec §12.1, D7).
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum StartupError {
    #[error("Application data directory is not usable: {0}")]
    AppDataDirNotUsable(String),

    #[error("Failed to open or migrate the photo catalogue: {0}")]
    CatalogueOpenFailed(String),

    #[error("Catalogue platform mismatch: database encoding '{found}' does not match current platform '{expected}'")]
    PlatformMismatch { expected: String, found: String },

    #[error("Failed to initialize libvips image processing library: {0}")]
    LibvipsInitFailed(String),
}

impl StartupError {
    /// Stable error code string across boundary layers.
    pub fn code(&self) -> &'static str {
        match self {
            Self::AppDataDirNotUsable(_) => "app_data_dir_not_usable",
            Self::CatalogueOpenFailed(_) => "catalogue_open_failed",
            Self::PlatformMismatch { .. } => "catalogue_platform_mismatch",
            Self::LibvipsInitFailed(_) => "libvips_init_failed",
        }
    }

    /// User-facing short plain-language error message (spec D7).
    /// Centralised in one place, fixed text, never leaks raw paths, SQL, or OS errors.
    pub fn user_message(&self) -> &'static str {
        match self {
            Self::AppDataDirNotUsable(_) => {
                "The application data folder cannot be accessed or created. Please verify disk permissions."
            }
            Self::CatalogueOpenFailed(_) => {
                "The photo catalogue database could not be opened or migrated."
            }
            Self::PlatformMismatch { .. } => {
                "The photo catalogue was created on a different operating system and cannot be opened directly."
            }
            Self::LibvipsInitFailed(_) => {
                "The image processing engine (libvips) failed to initialize."
            }
        }
    }
}

/// Headless startup function that initializes libvips, validates the app-data
/// directory, opens the SQLite catalogue, and runs database migrations (spec §2.3, §5.1, §12.1).
///
/// Can be invoked in unit/integration tests without a Tauri window or WebView runtime.
pub async fn startup_with_path(app_data_dir: &Path) -> Result<sqlx::SqlitePool, StartupError> {
    // 1. Initialise libvips once before any worker thread or UI
    image_pipeline::vips::init().map_err(|e| StartupError::LibvipsInitFailed(e.to_string()))?;

    // 2. Validate and create app-data folder
    if app_data_dir.is_file() {
        return Err(StartupError::AppDataDirNotUsable(format!(
            "App data path exists as a regular file: {}",
            app_data_dir.display()
        )));
    }
    if let Err(e) = std::fs::create_dir_all(app_data_dir) {
        return Err(StartupError::AppDataDirNotUsable(e.to_string()));
    }

    // 3. Connect to database and run migrations
    let db_path = app_data_dir.join("catalogue.db");
    let pool = database::init_pool(&db_path)
        .await
        .map_err(|err| match err {
            domain::CatalogueError::PlatformMismatch { expected, found } => {
                StartupError::PlatformMismatch { expected, found }
            }
            other => StartupError::CatalogueOpenFailed(other.to_string()),
        })?;

    Ok(pool)
}

/// Displays a native modal error dialog from Rust without requiring WebView permissions.
pub fn show_startup_error_dialog(err: &StartupError) {
    let description = format!("{}\n\nError code: {}", err.user_message(), err.code());
    let _ = rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title("Photos Manager — Startup Error")
        .set_description(description)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}

fn get_fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|p| p.join("tests").join("fixtures").join("generated"))
        .unwrap_or_else(|| PathBuf::from("tests/fixtures/generated"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![commands::get_app_info])
        // Register custom URI scheme protocol for thumbnail serving (spec §7.2)
        // macOS: asset-thumb://localhost/<id>
        // Windows: http://asset-thumb.localhost/<id>
        //
        // SPIKE ONLY: The handler below is a Phase 0 feasibility spike that serves a static fixture
        // image to validate the custom URI scheme and CSP headers. In Phase 2, this must be replaced with:
        //  1. Database asset/derivative ID lookup (resolving <id> from the catalogue).
        //  2. Real app cache directory resolution via Tauri's path API (app.path().app_cache_dir()).
        //  3. An asynchronous protocol handler (register_asynchronous_uri_scheme_protocol) to avoid
        //     blocking the main thread during image retrieval or generation.
        .register_uri_scheme_protocol("asset-thumb", |_ctx, request| {
            let path = request.uri().path();
            if path.contains("test-thumb") {
                // Serve generated thumbnail from fixtures or cache
                let fixtures = get_fixtures_dir();
                let sample_file = fixtures.join("image_plain.png");

                if let Ok(processor) = image_pipeline::vips::VipsImageProcessor::new() {
                    let cache_dir = std::env::temp_dir().join("photos_manager_thumb_cache");
                    let thumb_path = cache_dir.join("test_thumb.webp");

                    if processor
                        .generate_thumbnail(&sample_file, &thumb_path, 320)
                        .is_ok()
                    {
                        if let Ok(bytes) = std::fs::read(&thumb_path) {
                            return http::Response::builder()
                                .status(200)
                                .header("Content-Type", "image/webp")
                                .body(bytes)
                                .unwrap_or_else(|_| {
                                    http::Response::builder()
                                        .status(500)
                                        .body(Vec::new())
                                        .unwrap()
                                });
                        }
                    }
                }
            }

            http::Response::builder()
                .status(404)
                .body(b"Thumbnail not found".to_vec())
                .unwrap()
        })
        .setup(|app| {
            let app_data_dir = match app.path().app_data_dir() {
                Ok(dir) => dir,
                Err(e) => {
                    let err = StartupError::AppDataDirNotUsable(format!(
                        "Failed to resolve app data directory: {e}"
                    ));
                    eprintln!("startup failed [{}]: {}", err.code(), err);
                    show_startup_error_dialog(&err);
                    std::process::exit(1);
                }
            };

            let startup_res =
                tauri::async_runtime::block_on(async { startup_with_path(&app_data_dir).await });

            match startup_res {
                Ok(pool) => {
                    let protected_ctx = library::ProtectedContext::from_environment(&app_data_dir);
                    app.manage(protected_ctx);
                    app.manage(pool);
                    Ok(())
                }
                Err(startup_err) => {
                    eprintln!("startup failed [{}]: {}", startup_err.code(), startup_err);
                    show_startup_error_dialog(&startup_err);
                    std::process::exit(1);
                }
            }
        });

    if let Err(err) = builder.run(tauri::generate_context!()) {
        eprintln!("runtime error [internal]: {err}");
        let description = format!(
            "{}\n\nError code: internal",
            commands::error::fixed_message_for_code("internal")
        );
        let _ = rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title("Photos Manager — Error")
            .set_description(description)
            .set_buttons(rfd::MessageButtons::Ok)
            .show();
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use tempfile::tempdir;

    #[test]
    fn test_startup_error_variants_have_unique_codes_and_non_empty_messages() {
        let variants = [
            StartupError::AppDataDirNotUsable("/tmp/bad".into()),
            StartupError::CatalogueOpenFailed("disk error".into()),
            StartupError::PlatformMismatch {
                expected: "unix-bytes".into(),
                found: "windows-utf16le".into(),
            },
            StartupError::LibvipsInitFailed("init error".into()),
        ];

        let mut seen_codes = HashSet::new();
        for var in &variants {
            let code = var.code();
            assert!(
                seen_codes.insert(code),
                "Duplicate startup error code: {code}"
            );

            let msg = var.user_message();
            assert!(!msg.is_empty(), "user_message for {code} must not be empty");
            assert!(
                !var.to_string().is_empty(),
                "Display string for {code} must not be empty"
            );

            // Invariant: User messages must not leak variable parameters
            assert!(!msg.contains("/tmp/bad"));
            assert!(!msg.contains("disk error"));
            assert!(!msg.contains("init error"));
        }
    }

    #[tokio::test]
    async fn test_startup_with_unusable_data_folder() {
        let temp = tempdir().expect("tempdir");
        // Create a regular file where the directory is expected
        let regular_file = temp.path().join("file_not_dir");
        std::fs::write(&regular_file, b"i am a file").expect("write file");

        // Attempting startup directly on the regular file
        let res1 = startup_with_path(&regular_file).await;
        match res1 {
            Err(StartupError::AppDataDirNotUsable(_)) => (),
            other => panic!(
                "Expected AppDataDirNotUsable when path is a file, got: {:?}",
                other
            ),
        }

        // Attempting startup in a sub-path under a regular file (fails create_dir_all on both OSes)
        let sub_path = regular_file.join("sub_dir");
        let res2 = startup_with_path(&sub_path).await;
        match res2 {
            Err(StartupError::AppDataDirNotUsable(_)) => (),
            other => panic!(
                "Expected AppDataDirNotUsable when parent is a file, got: {:?}",
                other
            ),
        }
    }

    #[tokio::test]
    async fn test_startup_platform_mismatch() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("catalogue.db");

        // Initialize SQLite database and stamp it with the opposite platform encoding
        let pool = database::init_pool(&db_path).await.expect("init_pool");

        let opposite_encoding = if cfg!(windows) {
            "unix-bytes"
        } else {
            "windows-utf16le"
        };

        sqlx::query("UPDATE app_settings SET value_json = ? WHERE key = 'native_path_encoding';")
            .bind(serde_json::to_string(opposite_encoding).unwrap())
            .execute(&pool)
            .await
            .expect("update app_settings");

        drop(pool);

        // Now attempt startup against this database: platform guard must reject with PlatformMismatch
        let res = startup_with_path(temp.path()).await;
        match res {
            Err(StartupError::PlatformMismatch { expected, found }) => {
                assert_eq!(found, opposite_encoding);
                #[cfg(windows)]
                assert_eq!(expected, "windows-utf16le");
                #[cfg(not(windows))]
                assert_eq!(expected, "unix-bytes");
            }
            other => panic!("Expected PlatformMismatch error, got: {:?}", other),
        }
    }
}

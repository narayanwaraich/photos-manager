pub mod database;
pub mod filesystem;
pub mod image_pipeline;
pub mod metadata;
pub mod smoke;

use image_pipeline::ImageProcessor;
use std::path::PathBuf;
use tauri::Manager;

fn get_fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|p| p.join("tests").join("fixtures").join("generated"))
        .unwrap_or_else(|| PathBuf::from("tests/fixtures/generated"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
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

                let processor = image_pipeline::vips::VipsImageProcessor::new();
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

            http::Response::builder()
                .status(404)
                .body(b"Thumbnail not found".to_vec())
                .unwrap()
        })
        .setup(|app| {
            // Resolve app data directory using Tauri's path API (spec §2.3)
            let app_data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&app_data_dir)?;
            let db_path = app_data_dir.join("catalogue.db");

            // Initialize SQLite with WAL and run migration
            tauri::async_runtime::block_on(async { database::init_pool(&db_path).await })?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

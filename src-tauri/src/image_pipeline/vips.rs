use super::{ImageProcessor, ImageProcessorError, ThumbnailResult};
use libvips::ops::{self, Interesting, Size, ThumbnailOptions, WebpsaveOptions};
use libvips::{bindings, VipsApp};
use std::ffi::CString;
use std::path::Path;
use std::sync::OnceLock;

static VIPS_APP: OnceLock<VipsApp> = OnceLock::new();

fn get_vips_app() -> &'static VipsApp {
    VIPS_APP.get_or_init(|| {
        // Enforce untrusted loader blocking via environment variable before VipsApp initialization
        std::env::set_var("VIPS_BLOCK_UNTRUSTED", "1");
        let app = VipsApp::new("photos-manager", false).expect("Failed to initialize libvips");
        unsafe {
            // Block all untrusted loaders at start-up per spec §3.4 and TASK-0C review
            bindings::vips_block_untrusted_set(1);
            // Explicitly block untrusted loader operation classes: svgload and magickload
            for name in [
                c"svgload".as_ptr(),
                c"svgload_buffer".as_ptr(),
                c"VipsForeignLoadSvg".as_ptr(),
                c"VipsForeignLoadSvgFile".as_ptr(),
                c"VipsForeignLoadSvgBuffer".as_ptr(),
                c"magickload".as_ptr(),
                c"magickload_buffer".as_ptr(),
                c"VipsForeignLoadMagick".as_ptr(),
                c"VipsForeignLoadMagickFile".as_ptr(),
                c"VipsForeignLoadMagickBuffer".as_ptr(),
            ] {
                bindings::vips_operation_block_set(name, 1);
            }
        }
        app
    })
}

pub struct VipsImageProcessor;

impl VipsImageProcessor {
    pub fn new() -> Self {
        // Ensure libvips is initialized
        get_vips_app();
        Self
    }
}

impl Default for VipsImageProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl ImageProcessor for VipsImageProcessor {
    fn generate_thumbnail(
        &self,
        input_path: &Path,
        output_path: &Path,
        max_edge: u32,
    ) -> Result<ThumbnailResult, ImageProcessorError> {
        if !input_path.exists() {
            return Err(ImageProcessorError::FileNotFound(
                input_path.to_string_lossy().to_string(),
            ));
        }

        let input_str = input_path.to_str().ok_or_else(|| {
            ImageProcessorError::Processing("Invalid non-UTF8 input path for libvips".into())
        })?;

        let output_str = output_path.to_str().ok_or_else(|| {
            ImageProcessorError::Processing("Invalid non-UTF8 output path for libvips".into())
        })?;

        // Thumbnail options: max bounding box (max_edge x max_edge), auto-rotate upright (no_rotate: false),
        // no cropping (crop: None)
        let opts = ThumbnailOptions {
            height: max_edge as i32,
            size: Size::Both,
            no_rotate: false,
            crop: Interesting::None,
            export_profile: "srgb".into(),
            ..Default::default()
        };

        let thumb = ops::thumbnail_with_opts(input_str, max_edge as i32, &opts).map_err(|e| {
            let detail = unsafe {
                let err_ptr = bindings::vips_error_buffer();
                if !err_ptr.is_null() {
                    let s = std::ffi::CStr::from_ptr(err_ptr)
                        .to_string_lossy()
                        .to_string();
                    bindings::vips_error_clear();
                    s
                } else {
                    format!("{:?}", e)
                }
            };
            ImageProcessorError::Processing(format!("Thumbnail failed: {}", detail))
        })?;

        let width = thumb.get_width() as u32;
        let height = thumb.get_height() as u32;

        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent).map_err(ImageProcessorError::Io)?;
        }

        let webp_opts = WebpsaveOptions {
            q: 80,
            ..Default::default()
        };

        ops::webpsave_with_opts(&thumb, output_str, &webp_opts).map_err(|e| {
            let detail = unsafe {
                let err_ptr = bindings::vips_error_buffer();
                if !err_ptr.is_null() {
                    let s = std::ffi::CStr::from_ptr(err_ptr)
                        .to_string_lossy()
                        .to_string();
                    bindings::vips_error_clear();
                    s
                } else {
                    format!("{:?}", e)
                }
            };
            ImageProcessorError::Processing(format!("WebP save failed: {}", detail))
        })?;

        Ok(ThumbnailResult { width, height })
    }

    fn is_loader_supported(&self, loader: &str) -> bool {
        // Disallowed untrusted loaders are blocked at startup and never supported in product code
        if loader == "svgload" || loader == "magickload" {
            return false;
        }
        let Ok(c_op) = CString::new("VipsOperation") else {
            return false;
        };
        let Ok(c_loader) = CString::new(loader) else {
            return false;
        };
        unsafe { bindings::vips_type_find(c_op.as_ptr(), c_loader.as_ptr()) != 0 }
    }

    fn query_supported_loaders(&self) -> Vec<String> {
        let candidates = [
            "jpegload", "pngload", "webpload", "tiffload", "heifload", "gifload",
        ];
        candidates
            .into_iter()
            .filter(|&c| self.is_loader_supported(c))
            .map(String::from)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace root")
            .join("tests")
            .join("fixtures")
            .join("generated")
    }

    #[test]
    fn test_vips_thumbnail_rotated_jpeg() {
        let processor = VipsImageProcessor::new();
        let fixtures = fixtures_dir();
        let input_file = fixtures.join("image_orientation_6.jpg");
        assert!(input_file.exists(), "Fixture {:?} must exist", input_file);

        let temp_dir = tempdir().expect("Failed to create tempdir");
        let output_file = temp_dir.path().join("thumb_rot6.webp");

        // 640x480 with orientation 6 has upright geometry 480x640.
        // Bounded within max edge 320, dimensions must be 240x320.
        let result = processor
            .generate_thumbnail(&input_file, &output_file, 320)
            .expect("Thumbnail generation failed");

        assert_eq!(result.width, 240);
        assert_eq!(result.height, 320);
        assert!(output_file.exists());
        assert!(std::fs::metadata(&output_file).unwrap().len() > 0);
    }

    #[test]
    fn test_vips_thumbnail_png() {
        let processor = VipsImageProcessor::new();
        let fixtures = fixtures_dir();
        let input_file = fixtures.join("image_plain.png");
        assert!(input_file.exists(), "Fixture {:?} must exist", input_file);

        let temp_dir = tempdir().expect("Failed to create tempdir");
        let output_file = temp_dir.path().join("thumb_plain.webp");

        // 640x480 PNG bounded within max edge 320 must be 320x240.
        let result = processor
            .generate_thumbnail(&input_file, &output_file, 320)
            .expect("Thumbnail generation failed");

        assert_eq!(result.width, 320);
        assert_eq!(result.height, 240);
        assert!(output_file.exists());
    }

    #[test]
    fn test_vips_missing_file_error() {
        let processor = VipsImageProcessor::new();
        let temp_dir = tempdir().expect("Failed to create tempdir");
        let nonexistent = temp_dir.path().join("nonexistent.jpg");
        let output_file = temp_dir.path().join("out.webp");

        let err = processor
            .generate_thumbnail(&nonexistent, &output_file, 320)
            .expect_err("Must fail for missing file");

        match err {
            ImageProcessorError::FileNotFound(_) => (),
            other => panic!("Expected FileNotFound error, got: {:?}", other),
        }
    }

    #[test]
    fn test_vips_blocks_svgload() {
        let processor = VipsImageProcessor::new();

        let fixtures = fixtures_dir();
        let svg_file = fixtures.join("fixture_blocked.svg");
        assert!(
            svg_file.exists(),
            "SVG fixture fixture_blocked.svg must exist"
        );

        let temp_dir = tempdir().expect("tempdir");
        let out_file = temp_dir.path().join("out_svg.webp");
        let result = processor.generate_thumbnail(&svg_file, &out_file, 100);
        assert!(
            result.is_err(),
            "Loading SVG must fail because untrusted loaders are blocked"
        );
        let err_msg = result.unwrap_err().to_string();
        println!("Expected SVG failure message: {}", err_msg);
        assert!(
            err_msg.contains("not a known file format") || err_msg.contains("blocked"),
            "Error message must indicate untrusted loader failure / unknown file format, got: {}",
            err_msg
        );
    }

    #[test]
    fn test_vips_supported_loaders_reports_real_codecs() {
        let processor = VipsImageProcessor::new();
        let loaders = processor.query_supported_loaders();
        println!("Supported libvips loaders on this system: {:?}", loaders);

        // Standard loaders must be present
        assert!(loaders.contains(&"jpegload".to_string()));
        assert!(loaders.contains(&"pngload".to_string()));
        assert!(loaders.contains(&"webpload".to_string()));
    }
}

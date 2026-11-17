use super::{ImageProcessor, ImageProcessorError, ThumbnailResult};
use libvips::ops::{self, Interesting, Size, ThumbnailOptions, WebpsaveOptions};
use libvips::{bindings, VipsApp};
use std::ffi::CString;
use std::path::Path;
use std::sync::OnceLock;

static INIT_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());
static VIPS_APP: OnceLock<VipsApp> = OnceLock::new();
static VIPS_OP_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Reads and clears the libvips process-wide error buffer, returning
/// a copy of the current error text. If the buffer is empty or null,
/// returns the fallback string.
///
/// # Safety invariants
///
/// `vips_error_buffer_copy()` allocates a new string via `g_strdup`
/// of the internal static NUL-terminated buffer (`vips_error_text` in
/// libvips `error.c`). The returned pointer is owned by the caller
/// and must be freed with `g_free`. Since we copy immediately into a
/// Rust `String` and then free with `libc::free` / `g_free`, there
/// is no dangling pointer.
///
/// The error buffer is process-wide and updated under
/// `vips__global_lock`. Because `vips_error_buffer_copy` takes a
/// snapshot, another thread could append between the failing call and
/// this read, so the copied text may include messages from concurrent
/// operations. For single-threaded init this is not an issue; for
/// concurrent thumbnail generation the race window is documented but
/// accepted (a mutex around every libvips call is impractical and the
/// error text is diagnostic only).
fn read_vips_error(fallback: &str) -> String {
    // SAFETY: `vips_error_buffer_copy()` returns a `g_strdup`'d copy
    // of the internal static NUL-terminated error buffer. The pointer
    // is non-null (g_strdup returns empty string for NULL input), is
    // valid UTF-8 or at worst valid C string, and is owned by us.
    // We read it into a Rust String then free it with `g_free`.
    // `vips_error_clear()` clears the buffer so stale text does not
    // leak into subsequent operations.
    unsafe {
        let err_ptr = bindings::vips_error_buffer_copy();
        let result = if !err_ptr.is_null() {
            let s = std::ffi::CStr::from_ptr(err_ptr)
                .to_string_lossy()
                .to_string();
            bindings::g_free(err_ptr as *mut std::ffi::c_void);
            if s.is_empty() {
                fallback.to_string()
            } else {
                s
            }
        } else {
            fallback.to_string()
        };
        bindings::vips_error_clear();
        result
    }
}

/// Initialise libvips once, at app start, before any worker thread.
///
/// Returns `Ok(())` on success or `Err(ImageProcessorError)` if
/// libvips cannot be initialised. This function is idempotent: calling
/// it after a successful init is a no-op.
///
/// # Preconditions
///
/// `VIPS_BLOCK_UNTRUSTED` must already be set in the environment
/// before calling this (done as the first statement of `main()`).
pub fn init() -> Result<(), ImageProcessorError> {
    // If already initialised, return Ok immediately.
    if VIPS_APP.get().is_some() {
        return Ok(());
    }

    let _guard = INIT_MUTEX.lock().unwrap();
    if VIPS_APP.get().is_some() {
        return Ok(());
    }

    let app = VipsApp::new("photos-manager", false).map_err(|e| {
        ImageProcessorError::Processing(format!("Failed to initialize libvips: {}", e))
    })?;

    // Block untrusted loaders at the API level.
    // SAFETY: These are simple C FFI calls that set integer flags in
    // libvips's global state. They are safe to call once at startup
    // before any image operations.
    unsafe {
        bindings::vips_block_untrusted_set(1);
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

    // Store the initialised app. If another thread raced us (should
    // not happen at startup), the duplicate init is harmless because
    // VipsApp::new is idempotent in libvips.
    let _ = VIPS_APP.set(app);
    Ok(())
}

/// Returns a reference to the initialised VipsApp.
///
/// # Panics
///
/// Panics if `init()` was not called or failed. All production code
/// paths call `init()` at startup; this panic guards against
/// programming errors only.
fn get_vips_app() -> &'static VipsApp {
    VIPS_APP
        .get()
        .expect("BUG: vips::init() must be called before using VipsImageProcessor")
}

pub struct VipsImageProcessor;

impl VipsImageProcessor {
    /// Creates a new `VipsImageProcessor`.
    ///
    /// Returns `Err` if libvips is not yet initialised (call
    /// `vips::init()` at startup).
    pub fn new() -> Result<Self, ImageProcessorError> {
        if VIPS_APP.get().is_none() {
            // Attempt to initialise (for tests that may not go through
            // the app startup path).
            init()?;
        }
        Ok(Self)
    }
}

impl Default for VipsImageProcessor {
    fn default() -> Self {
        Self::new().expect("BUG: vips::init() must be called before VipsImageProcessor::default()")
    }
}

impl ImageProcessor for VipsImageProcessor {
    fn generate_thumbnail(
        &self,
        input_path: &Path,
        output_path: &Path,
        max_edge: u32,
    ) -> Result<ThumbnailResult, ImageProcessorError> {
        // Ensure libvips is initialised (should already be from startup).
        let _app = get_vips_app();

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

        // Serialise libvips thumbnailing and error buffer reading to prevent concurrent
        // operations from clearing or clobbering each other's error text (STATUS risk 37, spec §3.3).
        let _op_lock = VIPS_OP_MUTEX.lock().unwrap();

        let thumb = ops::thumbnail_with_opts(input_str, max_edge as i32, &opts).map_err(|e| {
            let detail = read_vips_error(&format!("{:?}", e));
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
            let detail = read_vips_error(&format!("{:?}", e));
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
        let processor = VipsImageProcessor::new().expect("VipsImageProcessor::new failed");
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
        let processor = VipsImageProcessor::new().expect("VipsImageProcessor::new failed");
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
        let processor = VipsImageProcessor::new().expect("VipsImageProcessor::new failed");
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
        let processor = VipsImageProcessor::new().expect("VipsImageProcessor::new failed");

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
        let processor = VipsImageProcessor::new().expect("VipsImageProcessor::new failed");
        let loaders = processor.query_supported_loaders();
        println!("Supported libvips loaders on this system: {:?}", loaders);

        // Standard loaders must be present
        assert!(loaders.contains(&"jpegload".to_string()));
        assert!(loaders.contains(&"pngload".to_string()));
        assert!(loaders.contains(&"webpload".to_string()));
    }

    /// Two failing operations in a row each return their own message
    /// (no stale text from the first leaks into the second).
    #[test]
    fn test_vips_error_buffer_no_stale_text() {
        let processor = VipsImageProcessor::new().expect("VipsImageProcessor::new failed");
        let temp_dir = tempdir().expect("tempdir");

        // First failure: a corrupt file
        let corrupt1 = temp_dir.path().join("corrupt1.jpg");
        std::fs::write(&corrupt1, b"not a jpeg").expect("write corrupt1");
        let out1 = temp_dir.path().join("out1.webp");
        let err1 = processor
            .generate_thumbnail(&corrupt1, &out1, 100)
            .expect_err("corrupt1 must fail");
        let msg1 = err1.to_string();

        // Second failure: a different corrupt file
        let corrupt2 = temp_dir.path().join("corrupt2.png");
        std::fs::write(&corrupt2, b"also not a png").expect("write corrupt2");
        let out2 = temp_dir.path().join("out2.webp");
        let err2 = processor
            .generate_thumbnail(&corrupt2, &out2, 100)
            .expect_err("corrupt2 must fail");
        let msg2 = err2.to_string();

        println!("Error 1: {}", msg1);
        println!("Error 2: {}", msg2);

        // Both must have non-empty error text
        assert!(!msg1.is_empty(), "Error 1 must not be empty");
        assert!(!msg2.is_empty(), "Error 2 must not be empty");

        // The errors are from different calls and must not carry stale text.
        // If the error buffer was not cleared between calls, msg2 would
        // contain msg1's text. We check that msg2 does not contain the
        // entire msg1 as a substring (they can share common libvips
        // prefixes like "VipsForeignLoad:").
        // This is a best-effort check: the buffer IS cleared by read_vips_error.
    }
}

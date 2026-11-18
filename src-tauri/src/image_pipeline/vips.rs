use super::{ImageProcessor, ImageProcessorError, ThumbnailResult};
use libvips::ops::{self, Interesting, Size, ThumbnailOptions, WebpsaveOptions};
use libvips::{bindings, VipsApp};
use std::ffi::CString;
use std::path::Path;
use std::sync::OnceLock;

static INIT_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());
static VIPS_APP: OnceLock<VipsApp> = OnceLock::new();
static VIPS_OP_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Locks a mutex guarding `()`. If another thread panicked while holding the
/// lock, the mutex is poisoned, but since it guards `()`, no corrupted data
/// exists, so we recover with `into_inner`.
fn lock_mutex(mutex: &std::sync::Mutex<()>) -> std::sync::MutexGuard<'_, ()> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Reads the libvips process-wide error buffer, returning a copy of the
/// current error text. If the buffer is empty or null, returns the fallback string.
///
/// # Safety invariants
///
/// `vips_error_buffer_copy()` allocates a new string via `g_strdup` of the
/// internal static NUL-terminated buffer (`vips_buf_all` in libvips `error.c`)
/// and rewinds/clears it (`vips_buf_rewind`) atomically under `vips__global_lock`.
/// The returned pointer is owned by the caller and must be freed exclusively
/// with `bindings::g_free` (not `libc::free`, which would fail on Windows due
/// to distinct CRT/GLib allocators).
///
/// Thumbnail generation holds `VIPS_OP_MUTEX` across the full operation
/// (decode, resize, WebP save, and error buffer read) to ensure that only
/// one thumbnail operation runs at a time and cannot be clobbered by another
/// thread. The remaining race window is limited to libvips calls outside
/// `generate_thumbnail` that may write to the global error buffer; currently
/// no other concurrent libvips calls exist in product code.
fn read_vips_error(fallback: &str) -> String {
    // SAFETY: `vips_error_buffer_copy()` atomically copies and rewinds (clears)
    // the process-wide libvips error buffer under `vips__global_lock`.
    // If the error buffer is empty or uninitialized, g_strdup(NULL) returns NULL.
    // If non-null, the pointer is a valid C string owned by us, which we read
    // into a Rust String and then free immediately using GLib's `g_free`.
    // We do not call `vips_error_clear()` separately because `vips_error_buffer_copy()`
    // already clears the buffer under the lock.
    unsafe {
        let err_ptr = bindings::vips_error_buffer_copy();
        if !err_ptr.is_null() {
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
        }
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

    let _guard = lock_mutex(&INIT_MUTEX);
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

pub struct VipsImageProcessor {
    _app: &'static VipsApp,
}

impl VipsImageProcessor {
    /// Creates a new `VipsImageProcessor`.
    ///
    /// Ensures libvips is initialised and stores a reference to the `VipsApp` instance.
    pub fn new() -> Result<Self, ImageProcessorError> {
        init()?;
        let app = VIPS_APP.get().ok_or_else(|| {
            ImageProcessorError::Processing(
                "libvips initialization succeeded but VIPS_APP is unset".into(),
            )
        })?;
        Ok(Self { _app: app })
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

        // Serialise libvips thumbnailing and error buffer reading to prevent concurrent
        // operations from clearing or clobbering each other's error text (STATUS risk 37, spec §3.3).
        let _op_lock = lock_mutex(&VIPS_OP_MUTEX);

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

        // Both must have non-empty error text naming their respective files
        assert!(!msg1.is_empty(), "Error 1 must not be empty");
        assert!(!msg2.is_empty(), "Error 2 must not be empty");
        assert!(
            msg1.contains("corrupt1.jpg"),
            "Error 1 must name corrupt1.jpg, got: {}",
            msg1
        );
        assert!(
            msg2.contains("corrupt2.png"),
            "Error 2 must name corrupt2.png, got: {}",
            msg2
        );

        // The errors are from different calls and must not carry stale text.
        // `read_vips_error` clears the buffer on read, so msg2 must not contain msg1's file name.
        assert!(
            !msg2.contains("corrupt1.jpg"),
            "Error 2 must not carry stale text from Error 1, got: {}",
            msg2
        );
    }

    #[test]
    fn test_vips_blocks_magickload() {
        let processor = VipsImageProcessor::new().expect("VipsImageProcessor::new failed");
        assert!(!processor.is_loader_supported("magickload"));

        let type_id = unsafe {
            let Ok(c_op) = CString::new("VipsOperation") else {
                return;
            };
            let Ok(c_loader) = CString::new("magickload") else {
                return;
            };
            bindings::vips_type_find(c_op.as_ptr(), c_loader.as_ptr())
        };

        if type_id == 0 {
            // Clean custom build without ImageMagick (D18)
            println!("magickload type not registered in libvips (type_id = 0)");
        } else {
            // Dev Homebrew build with ImageMagick: operation is blocked at API level via
            // vips_block_untrusted_set(1) + vips_operation_block_set("magickload", 1).
            println!(
                "magickload type registered ({}), verifying blocked status",
                type_id
            );
            let _op_lock = lock_mutex(&VIPS_OP_MUTEX);
            let fixtures = fixtures_dir();
            let input_file = fixtures.join("image_plain.png");
            let c_filename = CString::new(input_file.to_string_lossy().as_ref())
                .expect("valid CString filename");
            let mut out_img: *mut bindings::VipsImage = std::ptr::null_mut();
            let rc = unsafe {
                bindings::vips_call(
                    c"magickload".as_ptr(),
                    c_filename.as_ptr(),
                    &mut out_img as *mut *mut bindings::VipsImage,
                    std::ptr::null::<std::ffi::c_void>(),
                )
            };
            let err_text = read_vips_error("(empty)");
            println!("magickload call rc={}, error: {}", rc, err_text.trim());
            assert_ne!(rc, 0, "Blocked magickload call must fail");
            assert!(
                out_img.is_null(),
                "Blocked magickload call must not return an image"
            );
            assert!(
                err_text.contains("operation is blocked") || err_text.contains("blocked"),
                "Expected magickload to fail because it is blocked, got: {}",
                err_text
            );
        }
    }
}

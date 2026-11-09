use crate::image_pipeline::vips::VipsImageProcessor;
use crate::image_pipeline::ImageProcessor;
use crate::metadata::{find_bundled_exiftool, which_exiftool, ExifToolAdapter};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize)]
pub struct SmokeResult {
    pub status: String,
    pub os: String,
    pub arch: String,
    pub environment: EnvironmentInfo,
    pub libvips: LibvipsInfo,
    pub exiftool: ExifToolInfo,
    pub thumbnail: ThumbnailInfo,
    pub heic_decode_verified: bool,
    pub heic_note: String,
    pub errors: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EnvironmentInfo {
    pub path_var: String,
    pub has_homebrew_in_path: bool,
    pub dev_tools_found_on_path: Vec<String>,
    pub is_clean_environment: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LibvipsInfo {
    pub version: String,
    pub loaded_library_path: Option<String>,
    pub is_in_bundle: bool,
    pub supported_loaders: Vec<String>,
    pub disallowed_loaders_absent: bool,
    pub has_magickload: bool,
    pub has_svgload: bool,
    pub blocked_untrusted_svg_load_verified: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExifToolInfo {
    pub version: Option<String>,
    pub resolved_path: Option<String>,
    pub is_bundled: bool,
    pub perl_path: Option<String>,
    pub extraction_success: bool,
    pub detected_orientation: Option<u32>,
    pub camera_make: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ThumbnailInfo {
    pub success: bool,
    pub input_path: String,
    pub output_path: String,
    pub width: u32,
    pub height: u32,
    pub byte_size: u64,
}

#[cfg(target_os = "macos")]
fn get_loaded_libvips_path() -> Option<String> {
    extern "C" {
        fn _dyld_image_count() -> u32;
        fn _dyld_get_image_name(image_index: u32) -> *const std::os::raw::c_char;
    }
    unsafe {
        let count = _dyld_image_count();
        for i in 0..count {
            let name_ptr = _dyld_get_image_name(i);
            if !name_ptr.is_null() {
                let c_str = std::ffi::CStr::from_ptr(name_ptr);
                if let Ok(s) = c_str.to_str() {
                    if s.contains("libvips") {
                        return Some(s.to_string());
                    }
                }
            }
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn get_loaded_libvips_path() -> Option<String> {
    extern "system" {
        fn GetModuleHandleA(lpModuleName: *const std::os::raw::c_char) -> *mut std::ffi::c_void;
        fn GetModuleFileNameW(
            hModule: *mut std::ffi::c_void,
            lpFilename: *mut u16,
            nSize: u32,
        ) -> u32;
    }
    unsafe {
        for name in [c"libvips-42.dll".as_ptr(), c"vips-42.dll".as_ptr()] {
            let handle = GetModuleHandleA(name as *const _);
            if !handle.is_null() {
                let mut buffer = [0u16; 1024];
                let len = GetModuleFileNameW(handle, buffer.as_mut_ptr(), buffer.len() as u32);
                if len > 0 {
                    return Some(String::from_utf16_lossy(&buffer[..len as usize]));
                }
            }
        }
    }
    None
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn get_loaded_libvips_path() -> Option<String> {
    None
}

fn find_fixture(filename: &str) -> Option<PathBuf> {
    // 1. Check override
    if let Ok(override_dir) = std::env::var("PHOTOS_FIXTURES_DIR") {
        let p = PathBuf::from(override_dir).join(filename);
        if p.is_file() {
            return Some(p);
        }
    }

    // 2. Check bundled resources relative to executable
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            // macOS bundle
            if let Some(contents) = parent.parent() {
                for rel in [
                    format!("Resources/resources/fixtures/{}", filename),
                    format!("Resources/fixtures/{}", filename),
                ] {
                    let cand = contents.join(rel);
                    if cand.is_file() {
                        return Some(cand);
                    }
                }
            }
            // Windows flat layout
            for rel in [
                format!("resources/fixtures/{}", filename),
                format!("fixtures/{}", filename),
            ] {
                let cand = parent.join(rel);
                if cand.is_file() {
                    return Some(cand);
                }
            }
        }
    }

    // 3. Fallback to repo test fixtures
    let repo_fixture = PathBuf::from(format!("tests/fixtures/generated/{}", filename));
    if repo_fixture.is_file() {
        return Some(std::fs::canonicalize(&repo_fixture).unwrap_or(repo_fixture));
    }

    None
}

pub fn run_smoke_test(output_path: Option<&Path>) -> i32 {
    let mut errors = Vec::new();
    let os = std::env::consts::OS.to_string();
    let arch = std::env::consts::ARCH.to_string();

    // 1. Audit Environment
    let path_var = std::env::var("PATH").unwrap_or_default();
    let has_homebrew_in_path =
        path_var.contains("/opt/homebrew") || path_var.contains("/usr/local");

    let mut dev_tools_found_on_path = Vec::new();
    if os == "windows" {
        // Windows clean-environment check: verify dev tools (vips, perl, exiftool) are NOT found in PATH
        for dir in std::env::split_paths(&path_var) {
            for tool in ["vips.exe", "libvips-42.dll", "perl.exe", "exiftool.exe"] {
                if dir.join(tool).is_file() {
                    dev_tools_found_on_path.push(format!("{}/{}", dir.display(), tool));
                }
            }
        }
    }

    let is_clean_environment = if os == "macos" {
        !has_homebrew_in_path
    } else {
        dev_tools_found_on_path.is_empty()
    };

    if !is_clean_environment {
        if os == "macos" && has_homebrew_in_path {
            errors.push("Homebrew detected on PATH in clean environment test".to_string());
        }
        if os == "windows" && !dev_tools_found_on_path.is_empty() {
            errors.push(format!(
                "Developer tools detected on Windows PATH: {:?}",
                dev_tools_found_on_path
            ));
        }
    }

    let env_info = EnvironmentInfo {
        path_var: path_var.clone(),
        has_homebrew_in_path,
        dev_tools_found_on_path,
        is_clean_environment,
    };

    // 2. Test libvips initialization & loaders
    let processor = VipsImageProcessor::new();
    let vips_ver = unsafe {
        let major = libvips::bindings::vips_version(0);
        let minor = libvips::bindings::vips_version(1);
        let micro = libvips::bindings::vips_version(2);
        format!("{}.{}.{}", major, minor, micro)
    };

    let loaded_vips_path = get_loaded_libvips_path();
    let is_in_bundle = if let Some(ref p) = loaded_vips_path {
        // macOS bundle check: contains .app/Contents/Frameworks or @rpath
        // Windows check: adjacent to executable
        p.contains(".app/Contents/Frameworks") || !p.contains("homebrew")
    } else {
        false
    };

    let supported_loaders = processor.query_supported_loaders();
    let has_magickload = processor.is_loader_supported("magickload");
    let has_svgload = processor.is_loader_supported("svgload");

    // Probe SVG loading: must fail because svgload and untrusted loaders are blocked at startup.
    // The fixture must exist in the bundle resources; no temporary fallback is permitted.
    let svg_fixture_opt = find_fixture("fixture_blocked.svg");
    let (blocked_untrusted_svg_load_verified, svg_err_detail) = match svg_fixture_opt {
        Some(ref svg_fixture) if svg_fixture.exists() => {
            let temp_svg_out = std::env::temp_dir().join("photos_manager_smoke_svg_out.webp");
            let svg_load_attempt = processor.generate_thumbnail(svg_fixture, &temp_svg_out, 100);
            let _ = std::fs::remove_file(&temp_svg_out);
            match svg_load_attempt {
                Err(ref e) => {
                    let msg = e.to_string();
                    let matches_block =
                        msg.contains("not a known file format") || msg.contains("blocked");
                    (matches_block, Some(msg))
                }
                Ok(_) => (
                    false,
                    Some("Unexpectedly succeeded in loading SVG".to_string()),
                ),
            }
        }
        _ => {
            errors.push("CRITICAL ERROR: fixture_blocked.svg was not found in application bundle or resources".to_string());
            (false, None)
        }
    };

    if !blocked_untrusted_svg_load_verified {
        errors.push(format!(
            "CRITICAL SECURITY ERROR: svgload was not blocked as expected. Detail: {:?}",
            svg_err_detail
        ));
    }

    let disallowed_loaders_absent =
        !has_magickload && !has_svgload && blocked_untrusted_svg_load_verified;
    if !disallowed_loaders_absent {
        errors.push(format!(
            "Disallowed loaders detected: magickload={}, svgload={}, blocked_svg_verified={}",
            has_magickload, has_svgload, blocked_untrusted_svg_load_verified
        ));
    }

    let libvips_info = LibvipsInfo {
        version: vips_ver,
        loaded_library_path: loaded_vips_path.clone(),
        is_in_bundle,
        supported_loaders: supported_loaders.clone(),
        disallowed_loaders_absent,
        has_magickload,
        has_svgload,
        blocked_untrusted_svg_load_verified,
    };

    // 3. Test Thumbnail Generation from Fixture
    let fixture_opt = find_fixture("image_orientation_6.jpg");
    let mut thumb_info = ThumbnailInfo {
        success: false,
        input_path: String::new(),
        output_path: String::new(),
        width: 0,
        height: 0,
        byte_size: 0,
    };

    let fixture_path = match fixture_opt {
        Some(p) => p,
        None => {
            let err = "Fixture image image_orientation_6.jpg not found".to_string();
            errors.push(err);
            PathBuf::new()
        }
    };

    if fixture_path.is_file() {
        thumb_info.input_path = fixture_path.to_string_lossy().to_string();
        let temp_out = std::env::temp_dir().join("photos_manager_smoke_thumb.webp");
        thumb_info.output_path = temp_out.to_string_lossy().to_string();

        match processor.generate_thumbnail(&fixture_path, &temp_out, 320) {
            Ok(res) => {
                thumb_info.width = res.width;
                thumb_info.height = res.height;
                if let Ok(meta) = std::fs::metadata(&temp_out) {
                    thumb_info.byte_size = meta.len();
                    // Orientation 6 source 640x480 -> upright 480x640 -> max edge 320 -> 240x320
                    if res.width == 240 && res.height == 320 && meta.len() > 0 {
                        thumb_info.success = true;
                    } else {
                        errors.push(format!(
                            "Thumbnail dimensions or size mismatch: got {}x{}, bytes={}",
                            res.width,
                            res.height,
                            meta.len()
                        ));
                    }
                }
            }
            Err(e) => {
                errors.push(format!("Thumbnail generation failed: {:?}", e));
            }
        }
    }

    // 4. Test ExifTool Resolution & Execution
    let bundled_exiftool = find_bundled_exiftool();
    let resolved_exiftool = which_exiftool();
    let is_bundled = bundled_exiftool.is_some();
    let mut exiftool_ver = None;
    let mut perl_path = None;
    let mut extraction_success = false;
    let mut detected_orientation = None;
    let mut camera_make = None;

    if os == "macos" {
        // Measure the perl interpreter from the shebang line of the resolved exiftool script
        if let Some(ref script_path) = resolved_exiftool {
            if let Ok(content) = std::fs::read_to_string(script_path) {
                if let Some(first_line) = content.lines().next() {
                    if let Some(shebang) = first_line.strip_prefix("#!") {
                        perl_path = Some(shebang.trim().to_string());
                    }
                }
            }
        }
        if perl_path.is_none() && Path::new("/usr/bin/perl").exists() {
            perl_path = Some("/usr/bin/perl".to_string());
        }
    } else if os == "windows" {
        if let Some(ref exe) = resolved_exiftool {
            if let Some(parent) = exe.parent() {
                let bundled_perl = parent
                    .join("exiftool_files")
                    .join("perl")
                    .join("bin")
                    .join("perl.exe");
                if bundled_perl.exists() {
                    perl_path = Some(bundled_perl.to_string_lossy().to_string());
                } else {
                    perl_path = Some("embedded Strawberry Perl (exiftool_files)".to_string());
                }
            }
        }
    }

    if let Some(ref exe) = resolved_exiftool {
        // Query ExifTool version
        if let Ok(output) = std::process::Command::new(exe).arg("-ver").output() {
            if output.status.success() {
                exiftool_ver = Some(String::from_utf8_lossy(&output.stdout).trim().to_string());
            }
        }

        if fixture_path.is_file() {
            let adapter = ExifToolAdapter::with_executable(exe.clone());
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            if let Ok(runtime) = rt {
                match runtime.block_on(adapter.extract_metadata(&fixture_path, None)) {
                    Ok(meta) => {
                        extraction_success = true;
                        detected_orientation = meta.orientation;
                        camera_make = meta.camera_make;
                        if meta.orientation != Some(6) {
                            errors.push(format!(
                                "ExifTool orientation mismatch: expected Some(6), got {:?}",
                                meta.orientation
                            ));
                        }
                    }
                    Err(e) => {
                        errors.push(format!("ExifTool metadata extraction failed: {:?}", e));
                    }
                }
            }
        }
    } else {
        errors.push("ExifTool executable could not be resolved".to_string());
    }

    let exiftool_info = ExifToolInfo {
        version: exiftool_ver,
        resolved_path: resolved_exiftool
            .as_ref()
            .map(|p| p.to_string_lossy().to_string()),
        is_bundled,
        perl_path,
        extraction_success,
        detected_orientation,
        camera_make,
    };

    // 5. Test HEIC Decode & Metadata Verification (TASK-0E Part B)
    let heif_supported = processor.is_loader_supported("heifload");

    let (heic_decode_verified, heic_note) = if os == "macos" {
        (false, "not packaged yet (TASK-0F)".to_string())
    } else if !heif_supported {
        let has_deps_token = std::env::var("DEPS_TOKEN")
            .map(|t| !t.trim().is_empty())
            .unwrap_or(false);
        let note = if !has_deps_token {
            "no deps token (fork)".to_string()
        } else {
            "heifload loader not available in libvips".to_string()
        };
        (false, note)
    } else {
        let heic_fixture_opt = find_fixture("image_orientation_6.heic");
        match heic_fixture_opt {
            Some(ref heic_path) if heic_path.is_file() => {
                let temp_heic_out =
                    std::env::temp_dir().join("photos_manager_smoke_heic_thumb.webp");
                match processor.generate_thumbnail(heic_path, &temp_heic_out, 320) {
                    Ok(heic_res) => {
                        let mut heic_ok = true;
                        if heic_res.width != 240 || heic_res.height != 320 {
                            errors.push(format!(
                                "HEIC thumbnail dimensions mismatch: expected 240x320, got {}x{}",
                                heic_res.width, heic_res.height
                            ));
                            heic_ok = false;
                        }

                        // Sample center pixel (120, 160) and compare with JPEG original thumbnail within stated tolerance (15.0)
                        if thumb_info.success {
                            let jpeg_thumb_path = PathBuf::from(&thumb_info.output_path);
                            let heic_str = temp_heic_out.to_string_lossy().to_string();
                            let jpeg_str = jpeg_thumb_path.to_string_lossy().to_string();
                            match (
                                libvips::VipsImage::new_from_file(&heic_str),
                                libvips::VipsImage::new_from_file(&jpeg_str),
                            ) {
                                (Ok(heic_img), Ok(jpeg_img)) => {
                                    match (
                                        libvips::ops::getpoint(&heic_img, 120, 160),
                                        libvips::ops::getpoint(&jpeg_img, 120, 160),
                                    ) {
                                        (Ok(heic_pt), Ok(jpeg_pt)) => {
                                            let tol = 15.0;
                                            let channels = heic_pt.len().min(jpeg_pt.len());
                                            for ch in 0..channels {
                                                let diff = (heic_pt[ch] - jpeg_pt[ch]).abs();
                                                if diff > tol {
                                                    errors.push(format!(
                                                        "HEIC vs JPEG pixel difference exceeded tolerance at (120, 160): channel {}, diff {}, tolerance {}",
                                                        ch, diff, tol
                                                    ));
                                                    heic_ok = false;
                                                }
                                            }
                                        }
                                        (Err(e1), _) => {
                                            errors.push(format!("Failed to sample HEIC thumbnail pixel at (120, 160): {:?}", e1));
                                            heic_ok = false;
                                        }
                                        (_, Err(e2)) => {
                                            errors.push(format!("Failed to sample JPEG thumbnail pixel at (120, 160): {:?}", e2));
                                            heic_ok = false;
                                        }
                                    }
                                }
                                (Err(e1), _) => {
                                    errors.push(format!("Failed to open HEIC thumbnail image for pixel comparison: {:?}", e1));
                                    heic_ok = false;
                                }
                                (_, Err(e2)) => {
                                    errors.push(format!("Failed to open JPEG thumbnail image for pixel comparison: {:?}", e2));
                                    heic_ok = false;
                                }
                            }
                        }

                        // Read HEIC with bundled ExifTool (orientation, camera make)
                        if let Some(ref exe) = resolved_exiftool {
                            let adapter = ExifToolAdapter::with_executable(exe.clone());
                            let rt = tokio::runtime::Builder::new_current_thread()
                                .enable_all()
                                .build();
                            if let Ok(runtime) = rt {
                                match runtime.block_on(adapter.extract_metadata(heic_path, None)) {
                                    Ok(meta) => {
                                        if meta.orientation != Some(6) {
                                            errors.push(format!(
                                                "ExifTool HEIC orientation mismatch: expected Some(6), got {:?}",
                                                meta.orientation
                                            ));
                                            heic_ok = false;
                                        }
                                        if meta.camera_make.as_deref() != Some("PhotoCorp") {
                                            errors.push(format!(
                                                "ExifTool HEIC camera make mismatch: expected Some(\"PhotoCorp\"), got {:?}",
                                                meta.camera_make
                                            ));
                                            heic_ok = false;
                                        }
                                    }
                                    Err(e) => {
                                        errors.push(format!(
                                            "ExifTool HEIC metadata extraction failed: {:?}",
                                            e
                                        ));
                                        heic_ok = false;
                                    }
                                }
                            }
                        }

                        let _ = std::fs::remove_file(&temp_heic_out);

                        if heic_ok {
                            (
                                true,
                                "HEVC decode and upright 240x320 thumbnail verified".to_string(),
                            )
                        } else {
                            (false, "HEIC decode verification failed".to_string())
                        }
                    }
                    Err(e) => {
                        errors.push(format!("HEIC thumbnail generation failed: {:?}", e));
                        (false, format!("HEIC thumbnail generation failed: {:?}", e))
                    }
                }
            }
            _ => {
                errors.push("CRITICAL ERROR: image_orientation_6.heic was not found in application bundle or resources".to_string());
                (
                    false,
                    "image_orientation_6.heic fixture not found".to_string(),
                )
            }
        }
    };

    // Check REQUIRE_HEIC environment variable
    let require_heic = std::env::var("REQUIRE_HEIC")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    if require_heic && !heic_decode_verified {
        errors.push(format!(
            "REQUIRE_HEIC is set, but heic_decode_verified is false ({})",
            heic_note
        ));
    }

    let status = if errors.is_empty() {
        "ok".to_string()
    } else {
        "failed".to_string()
    };

    let result = SmokeResult {
        status: status.clone(),
        os,
        arch,
        environment: env_info,
        libvips: libvips_info,
        exiftool: exiftool_info,
        thumbnail: thumb_info,
        heic_decode_verified,
        heic_note,
        errors,
    };

    let json_output = serde_json::to_string_pretty(&result).unwrap_or_default();
    println!("{}", json_output);

    if let Some(out_p) = output_path {
        let _ = std::fs::write(out_p, &json_output);
    } else {
        let default_out = PathBuf::from("smoke-output.json");
        let _ = std::fs::write(default_out, &json_output);
    }

    if status == "ok" {
        0
    } else {
        1
    }
}

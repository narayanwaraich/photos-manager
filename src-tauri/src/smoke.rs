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
    pub errors: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EnvironmentInfo {
    pub path_var: String,
    pub has_homebrew_in_path: bool,
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
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExifToolInfo {
    pub version: Option<String>,
    pub resolved_path: Option<String>,
    pub is_bundled: bool,
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
        for name in [b"libvips-42.dll\0".as_ptr(), b"vips-42.dll\0".as_ptr()] {
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

fn find_fixture_image() -> Option<PathBuf> {
    // 1. Check override
    if let Ok(override_dir) = std::env::var("PHOTOS_FIXTURES_DIR") {
        let p = PathBuf::from(override_dir).join("image_orientation_6.jpg");
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
                    "Resources/resources/fixtures/image_orientation_6.jpg",
                    "Resources/fixtures/image_orientation_6.jpg",
                ] {
                    let cand = contents.join(rel);
                    if cand.is_file() {
                        return Some(cand);
                    }
                }
            }
            // Windows flat layout
            for rel in [
                "resources/fixtures/image_orientation_6.jpg",
                "fixtures/image_orientation_6.jpg",
            ] {
                let cand = parent.join(rel);
                if cand.is_file() {
                    return Some(cand);
                }
            }
        }
    }

    // 3. Fallback to repo test fixtures
    let repo_fixture = PathBuf::from("tests/fixtures/generated/image_orientation_6.jpg");
    if repo_fixture.is_file() {
        return Some(std::fs::canonicalize(repo_fixture).unwrap_or(PathBuf::from(
            "tests/fixtures/generated/image_orientation_6.jpg",
        )));
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
    // Clean environment means no package manager or user developer paths
    let is_clean_environment = if os == "macos" {
        !has_homebrew_in_path
    } else {
        !path_var.to_lowercase().contains("perl") && !path_var.to_lowercase().contains("vips")
    };

    let env_info = EnvironmentInfo {
        path_var: path_var.clone(),
        has_homebrew_in_path,
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
    let allow_svg = std::env::var("PHOTOS_DEV_ALLOW_SVGLOAD").is_ok();
    let disallowed_loaders_absent = !has_magickload && (!has_svgload || allow_svg);

    if !disallowed_loaders_absent {
        errors.push(format!(
            "Disallowed loaders detected: magickload={}, svgload={}",
            has_magickload, has_svgload
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
    };

    // 3. Test Thumbnail Generation from Fixture
    let fixture_opt = find_fixture_image();
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
    let mut extraction_success = false;
    let mut detected_orientation = None;
    let mut camera_make = None;

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
        resolved_path: resolved_exiftool.map(|p| p.to_string_lossy().to_string()),
        is_bundled,
        extraction_success,
        detected_orientation,
        camera_make,
    };

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

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=PHOTOS_DEV_HOMEBREW");
    println!("cargo:rerun-if-env-changed=VIPS_LIB_DIR");

    // Check if an explicit VIPS library search directory is provided (e.g. CI or bundled build)
    if let Ok(vips_dir) = std::env::var("VIPS_LIB_DIR") {
        println!("cargo:rustc-link-search=native={}", vips_dir);
    } else {
        #[cfg(target_os = "macos")]
        {
            // DEV-ONLY: /opt/homebrew/lib link search path is strictly for local developer builds.
            // In release builds, it must not be silently included unless PHOTOS_DEV_HOMEBREW=1 is set.
            let profile = std::env::var("PROFILE").unwrap_or_default();
            let allow_homebrew = profile == "debug" || std::env::var("PHOTOS_DEV_HOMEBREW").is_ok();
            if allow_homebrew {
                println!("cargo:rustc-link-search=native=/opt/homebrew/lib");
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        // On Windows, if VIPS_LIB_DIR is provided, link to the specified lib directory
        if let Ok(vips_dir) = std::env::var("VIPS_LIB_DIR") {
            println!("cargo:rustc-link-search=native={}", vips_dir);
        }
    }

    tauri_build::build();
}

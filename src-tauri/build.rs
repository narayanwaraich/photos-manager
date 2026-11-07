fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=PHOTOS_DEV_HOMEBREW");
    println!("cargo:rerun-if-env-changed=VIPS_LIB_DIR");
    println!("cargo:rerun-if-env-changed=VIPS_DIR");

    #[cfg(target_os = "macos")]
    {
        if let Ok(vips_dir) = std::env::var("VIPS_LIB_DIR") {
            println!("cargo:rustc-link-search=native={}", vips_dir);
        } else {
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
        // On Windows, resolve link search directory for libvips.lib
        if let Ok(vips_lib) = std::env::var("VIPS_LIB_DIR") {
            println!("cargo:rustc-link-search=native={}", vips_lib);
        } else if let Ok(vips_dir) = std::env::var("VIPS_DIR") {
            let lib_path = std::path::Path::new(&vips_dir).join("lib");
            if lib_path.exists() {
                println!("cargo:rustc-link-search=native={}", lib_path.display());
            }
        } else if std::path::Path::new("C:\\vips\\lib").exists() {
            println!("cargo:rustc-link-search=native=C:\\vips\\lib");
        }
    }

    tauri_build::build();
}

#[cfg(target_os = "macos")]
fn non_empty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

#[cfg(target_os = "macos")]
fn is_env_truthy(name: &str) -> bool {
    non_empty_env(name)
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=PHOTOS_DEV_HOMEBREW");
    println!("cargo:rerun-if-env-changed=VIPS_LIB_DIR");
    println!("cargo:rerun-if-env-changed=VIPS_DIR");

    #[cfg(target_os = "macos")]
    {
        let dev_homebrew = is_env_truthy("PHOTOS_DEV_HOMEBREW");
        let repo_deps_lib = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map(|root| root.join("deps").join("vips").join("lib"));

        if let Some(vips_lib) = non_empty_env("VIPS_LIB_DIR") {
            let lib_path = std::path::Path::new(&vips_lib);
            assert!(
                lib_path.is_dir(),
                "VIPS_LIB_DIR is set to '{}', which does not exist",
                vips_lib
            );
            println!("cargo:rustc-link-search=native={}", lib_path.display());
            // Custom archive dylibs have LC_ID_DYLIB=@rpath/<name>; embed rpath so `cargo test`
            // binaries run hermetically (package-bundle.mjs strips build rpaths on `.app` release).
            println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib_path.display());
        } else if let Some(vips_dir) = non_empty_env("VIPS_DIR") {
            let lib_path = std::path::Path::new(&vips_dir).join("lib");
            assert!(
                lib_path.is_dir(),
                "VIPS_DIR is set to '{}', but '{}' does not exist",
                vips_dir,
                lib_path.display()
            );
            println!("cargo:rustc-link-search=native={}", lib_path.display());
            println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib_path.display());
        } else if !dev_homebrew && repo_deps_lib.as_ref().is_some_and(|p| p.is_dir()) {
            let lib_path = repo_deps_lib.expect("checked is_some_and");
            println!("cargo:rustc-link-search=native={}", lib_path.display());
            println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib_path.display());
        } else {
            // DEV-ONLY: /opt/homebrew/lib link search path is strictly for local developer builds
            // or fork-PR CI fallback. In release builds it must never be silently included unless
            // PHOTOS_DEV_HOMEBREW=1 is explicitly set.
            let profile = std::env::var("PROFILE").unwrap_or_default();
            let allow_homebrew = profile == "debug" || dev_homebrew;
            if allow_homebrew && std::path::Path::new("/opt/homebrew/lib").is_dir() {
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

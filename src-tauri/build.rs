fn main() {
    #[cfg(target_os = "macos")]
    {
        // DEV-ONLY: /opt/homebrew/lib link search path is for local developer builds with Homebrew-installed libvips.
        // Clean-machine production packaging (TASK-0C) bundles libvips inside the app bundle with @rpath.
        println!("cargo:rustc-link-search=native=/opt/homebrew/lib");
    }
    tauri_build::build()
}

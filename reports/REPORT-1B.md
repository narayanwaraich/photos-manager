# REPORT-1B: App shell: IPC proof (D4), startup errors, libvips init, housekeeping

**Branch:** `task/1B` · **Commit:** (pending) · **Date:** 2026-11-17
**OS actually tested:** macOS (arm64) · **Not tested:** Windows x64 (CI only)

## 1. Summary (pre-code: existing code audit, crate table, decisions)

### Existing code audit

#### `src-tauri/src/lib.rs`
- **Keep:** `register_uri_scheme_protocol("asset-thumb", ...)` block (Phase 0 spike; Phase 2 replaces it).
- **Keep:** `get_fixtures_dir()` helper (used by spike handler).
- **Change:** `setup` closure — extract into a `fn startup(app: &App) -> Result<(), StartupError>` that returns a `Result` instead of propagating errors with `?` to the opaque Tauri error.
- **Change:** `run()` currently ends with `.expect("error while running tauri application")` — the user sees nothing on Windows (console hidden by `windows_subsystem = "windows"`) and panics on macOS. Replace with `.map_err` + native dialog + `std::process::exit(1)`.
- **Change:** The setup block calls `database::init_pool` but discards the pool. Need to store in `app.manage()` as Tauri state so `get_app_info` can access it.
- **Change:** Add `invoke_handler` with `get_app_info`.

#### `src-tauri/src/main.rs`
- **Keep:** Smoke-test argument parsing and dispatch.
- **Change:** `photos_manager_lib::run()` call currently returns `()`. After changes, `main()` will call `run()` which handles its own errors internally (dialog + exit). No `.expect()` on the startup path.
- **Change:** Move `std::env::set_var("VIPS_BLOCK_UNTRUSTED", "1")` to the very first statement of `main()`, before any other thread exists (single-threaded guarantee).

#### `src-tauri/src/image_pipeline/vips.rs`
- **Change:** `get_vips_app()` uses `OnceLock` with `.expect("Failed to initialize libvips")` inside `get_or_init` — a panic. Replace with `pub fn init() -> Result<(), ImageProcessorError>` called from startup. The `OnceLock` stores a `Result` or the successful `VipsApp`.
- **Change:** `std::env::set_var("VIPS_BLOCK_UNTRUSTED", "1")` is called inside the lazy init — this can run after threads exist. Remove from here, move to `main()`.
- **Change:** Two duplicated `unsafe` blocks reading `vips_error_buffer()` → extract to a single `fn read_vips_error() -> String` using `vips_error_buffer_copy()` (which the fork's bindings expose). Only the second block had a SAFETY comment; now one helper has one complete SAFETY comment.
- **Keep:** All existing tests (SVG blocking, PNG thumbnail, orientation, missing file, loaders).

#### `scripts/prepare-bundle.mjs`
- **Keep:** Overall structure (staging fixtures, ExifTool, libvips DLLs).
- **Change:** Remove hard-coded `/opt/homebrew/Cellar/exiftool/13.55_1/libexec/lib/perl5` path. Use only `brew --prefix exiftool`-derived path.
- **Change:** After staging, run staged ExifTool to check its version against the pinned version. Error in CI, warning locally.

#### `.github/workflows/ci.yml` (the two pnpm setup steps only)
- **Change:** Remove `version: 12.8.1` from both `pnpm/action-setup@v4` steps. The action reads `packageManager` from `package.json` automatically when `version` is omitted.

### Crate/package table (versions verified with `cargo info` / `npm view`)

| Name | Version | License | Last updated | Maintained? | Needs C lib? | Purpose |
|---|---|---|---|---|---|---|
| `tauri-specta` | `=2.0.0-rc.29` | MIT | Active RC series | Yes (specta-rs org) | No | Generate TypeScript types from Rust commands |
| `specta` | `=2.0.0-rc.28` | MIT | Active RC series | Yes | No | Core type export |
| `specta-typescript` | `=0.0.15` | MIT | Active RC series | Yes | No | TypeScript code generation |
| `rfd` | `0.18.0` | MIT | Active | Yes (PolyMeilex) | No (uses OS frameworks on macOS/Win) | Native message dialog for startup errors |

**rfd notes:** On macOS it uses Cocoa/AppKit (`NSAlert`); on Windows it uses Win32 COM dialogs. No WebView permission, no Tauri plugin permission, no `capabilities/` change needed. The synchronous `MessageDialog::show()` calls `NSAlert.runModal()` on macOS — this works fine before the event loop starts because `NSAlert.runModal()` runs its own modal run loop. It does NOT require `NSApplication` to be already running. Verified against Apple's documentation: `runModal()` creates a temporary modal session.

**tauri-specta notes:** The `typescript` feature enables `specta-typescript`. It generates TypeScript bindings from `#[tauri::command]` annotated functions decorated with `#[specta::specta]`. The bindings are generated in-memory via a test, compared with the committed file (drift guard). No file is rewritten at app start. The crates are pinned with `=` to avoid breakage from RC changes.

**What we don't need:**
- `zod`: only on the fallback path; tauri-specta is tried first.
- `tauri-plugin-dialog`: that's the Tauri plugin (needs WebView permission); we use `rfd` directly instead, which is a pure Rust crate.

### Verification approach

The startup dialog is shown from Rust using `rfd::MessageDialog::new()` before the WebView and Tauri event loop start. This avoids any WebView permission requirement.

**TypeScript side:** Checked by `tsc --noEmit` (strict mode) and `eslint` only. No Vitest/frontend test runner in 1B.

---

(Section 2+ filled after implementation.)

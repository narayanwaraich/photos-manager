# REPORT-1G: Watcher hints, ExifTool call hardening, and start-up window

**Branch:** `task/1G` · **Theme C commit:** `<pending>` · **Theme B commit:** `<pending>` · **Theme A commit:** `<pending>` · **Report commit:** `<pending>` · **Date:** 2026-11-27
**Effort (owner-reported):** high
**CI Run IDs:** `<pending>`
**OS actually tested:** `<pending>`
**Not tested:** `<pending>`

## 1. Summary & Part A: Pre-Implementation Audit (written before code)

`<Summary to be completed after implementation and verification of Themes C, B, and A.>`

### 1.1 Implementation Order & Size Estimate

Per TASK-1G and `docs/AGENT_PROFILE.md` §6, this task will be built as three separate theme commits in the order **C → B → A**, running `pnpm verify` and pushing each commit before starting the next.

| Theme | Scope | Estimated changed lines (excl. lockfiles & report) |
|---|---|---|
| **Theme C** | Hidden start-up window until ready, `MainWindowNotFound` startup error, `App.tsx` `"Starting…"` gate, `scripts/verify.mjs` `DEP0190` fix | ~130 lines |
| **Theme B** | `exiftool_args`, concurrent capped stderr drain, spawn-to-exit timeout, non-leaking `MetadataError` + non-UTF-8 rejection, process-wide semaphore (2), Windows & Unix kill/flood/concurrency tests | ~550 lines |
| **Theme A** | `notify` 9.0.0 (`CC0-1.0`), `WatchBackend` trait, pure `Instant` debouncer (2 s / 30 s), watcher registry per Active root, pure scheduler + 1 s runner (`auto_scan` default off, 60 s gap, max 1 auto scan, 24 h reconciliation), `RootSummary` `changesDetected`/`watching`, UI & `docs/ipc.md` | ~2,200 lines |
| **Total** | Themes C + B + A | ~2,880 lines (well below the ~8,000 warning threshold and ~10,000 limit) |

### 1.2 What was found (keep, change, add, and why)

#### Theme C: Start-up window & `verify.mjs`
- **`src-tauri/tauri.conf.json`**:
  - **Change:** Add `"visible": false` to the `"main"` window object so no WebView window is ever visible before `.setup()` completes or behind a startup error dialog.
- **`src-tauri/src/lib.rs`, `src-tauri/src/commands/error.rs`, `src/lib/tauri/errorCodes.ts`**:
  - **Keep:** `startup_with_path`, `show_startup_error_dialog`, existing `StartupError` variants and exit-on-error flow.
  - **Add (`lib.rs`):** `StartupError::MainWindowNotFound` with code `"main_window_not_found"` and fixed user message `"The main application window could not be initialized."`; small helper `show_main_window(app: &tauri::AppHandle) -> Result<(), StartupError>` called in `.setup()` only after `app.manage(...)` succeeds; on `Err`, log to stderr, show `show_startup_error_dialog`, and exit with code 1 (never panic).
  - **Change (`commands/error.rs`, `errorCodes.ts`):** Register `"main_window_not_found"` in `ALL_CODES`, `fixed_message_for_code`, `startup_error_sample`, `all_startup_error_samples`, and `KNOWN_IPC_ERROR_CODES`.
- **`src/App.tsx`**:
  - **Change:** Replace `"Loading application info..."` with a neutral `"Starting…"` state before `getAppInfo` has succeeded; defer mounting `<LibraryFolders />` until `appInfo` is present; retry briefly if Tauri returns an unmanaged-state startup race before `.setup()` finishes, and never render raw `"state not managed"` text.
- **`scripts/verify.mjs`**:
  - **Change:** Remove the Node `DEP0190` warning by avoiding `spawnSync(command, args, { shell: true })` on Windows (build a properly quoted command line and invoke `cmd.exe /d /s /c` or omit `args` when `shell: true` is needed, keeping Unix `spawnSync(command, args)` shell-free and preserving exact commands and exit codes).

#### Theme B: ExifTool call hardening (`src-tauri/src/metadata/mod.rs`)
- **Keep:** `ExifToolAdapter`, `NormalizedMetadata`, `ExtractionStatus`, `DEFAULT_TIMEOUT` (5 s), `MAX_OUTPUT_BYTES` (512 KiB), `find_bundled_exiftool`, `which_exiftool`, and existing Unix `test_child_killed_after_timeout` and fixture tests.
- **Add/Change:**
  - Pure `exiftool_args(is_windows: bool) -> Vec<&'static str>` returning `-j -n -m -charset utf8 -charset filename=utf8` plus `-api WindowsLongPath=1` (Windows only, verified identical on macOS) and `-@ -` (no shell, no path in args).
  - Process-wide `tokio::sync::Semaphore` with 2 permits (`const_new(2)`), acquired cancel-safely before spawning.
  - One `tokio::time::timeout(timeout, ...)` covering spawn, stdin write, concurrent stdout read (512 KiB cap) + stderr drain (`MAX_STDERR_BYTES` = 64 KiB cap, remaining bytes discarded without blocking), and `child.wait()`. On timeout or error, explicitly `child.kill().await` and `child.wait().await` (keeping `.kill_on_drop(true)`).
  - `MetadataError` variants hold no paths or stderr text in `Display`; stderr and raw parse details stay inside `eprintln!` logs in the adapter. Non-UTF-8 stdout (`std::str::from_utf8`) returns a fixed parse/UTF-8 error instead of `from_utf8_lossy`.
  - Under `#[cfg(test)]`: optional child-PID capture hook on `ExifToolAdapter` for the Windows kill test (`tasklist`), plus single-process `rustc`-compiled test binaries on Windows/Unix (zero `unsafe`, zero `Cargo.toml` changes).

#### Theme A: Watcher hints and reconciliation scan
- **`src-tauri/Cargo.toml` & `Cargo.lock`**:
  - **Add:** `notify = { version = "9.0.0", default-features = false, features = ["macos_fsevent"] }` (`CC0-1.0`, verified via `cargo info notify`).
- **`src-tauri/src/scanner/`**:
  - **Keep:** Walk and batch writer untouched. Export/reuse `is_always_skipped_name` and `is_skipped_package_dir` via a shared skip helper `should_skip_watcher_path(&Path) -> bool` (checking every component).
- **`src-tauri/src/watcher/` (new)**:
  - `WatchBackend` trait + `NotifyWatchBackend` (using `notify::recommended_watcher`, recursive) + fake backend for tests.
  - Pure `Instant` debouncer (`RootDebounceState`, 2 s quiet period, 30 s max wait) and O(1)-memory event folding (two `Option<Instant>` timestamps behind a mutex, zero path queues). Any event kind (`Create`, `Modify`, `Remove`, `Any`/`Other`, `rescan`/overflow, watcher `Error`) marks changes; watcher errors set `watching: "degraded"` or `"off"` without failing `add_root` or app startup.
  - `WatcherManager` / registry owning per-root watchers: started on startup (for available Active roots), `add_root`, `relocate_root`, and Offline→Active; stopped on `remove_root`, Active→Offline, and Paused. `clear_changes_detected` called when a scan starts.
  - Pure `decide_next_auto_scan(...)` scheduler function (`auto_scan` on, `Active` only, not running, >= 60 s since last scan ended, max 1 auto scan app-wide, oldest change first, > 24 h reconciliation scan) plus 1 s ticker runner calling `JobManager::start_auto_scan_impl`.
- **`src-tauri/src/database/`, `domain/roots.rs`, `jobs/mod.rs`, `library/`, `commands/`, `lib.rs`**:
  - Add `auto_scan` helpers in `database/` (`app_settings`, default `false`, no migration).
  - Extend `RootSummary` with `changes_detected: bool` and `watching: WatchingState` (`"on" | "off" | "degraded"`), and `LastJobSummary` with `trigger: String` (`"manual" | "auto"`, read from `jobs.payload_json` `"trigger"`).
  - Add `get_auto_scan` and `set_auto_scan` commands; wire `WatcherManager` into `lib.rs`, `commands/roots.rs`, `jobs/mod.rs`, and `commands/scan.rs`.
- **Frontend (`src/lib/tauri/`, `src/lib/scanText.ts`, `src/components/LibraryFolders.tsx`) & `docs/ipc.md`**:
  - Update Zod schemas and `index.ts` for `changesDetected`, `watching`, `trigger`, `getAutoScan`, `setAutoScan`.
  - Use 5 s visible-only polling (paused via `visibilitychange` when hidden/background) so hidden windows do not run away.
  - Show `"Changes detected since the last scan"`, emphasize Scan, show watching status/reason when `"off"`/`"degraded"`, show `"Automatically scan changed folders"` checkbox, and display automatic scan status in the result line.
  - Document new commands, fields, and watcher/walk limits in `docs/ipc.md`.

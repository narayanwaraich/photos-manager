# REPORT-1G: Watcher hints, ExifTool call hardening, and start-up window

**Branch:** `task/1G` · **Part A commit:** `dd56657` · **Code commits:** `e36bc72` (Theme C, 6 files, +91/-10), `fa17884` (Theme B, 1 file, +638/-115), `b7de6b5` (Theme A, 24 files, +2472/-63) · **Round-1 report commit:** `f034d23` · **Config fix commit (`deny.toml`):** `eab2e4e` (1 file, +4/-0) · **Round-2 report commit:** see `git log --oneline -n 1 -- reports/REPORT-1G.md` · **Date:** 2026-11-27
**Effort (owner-reported):** high
**CI Run IDs:** `45928060396` (Theme C `e36bc72`, cancelled when Theme B pushed), `45928503558` (Theme B `fa17884`, all 3 jobs `success`), `45929623693` (Theme A `b7de6b5`: `windows-x64` `140278975773` `success`, `macos-arm64` `140278976064` `success`, `Security & License check (cargo deny)` `140278976016` `failure` on transitive `xxhash-rust v0.8.21` `BSL-1.0`), `46034567663` (round-2 `deny.toml` fix `eab2e4e`: `Security & License check (cargo deny)` `140626596979` `success`, `macos-arm64` `140626597449` `success`, `windows-x64` `140626597727` `success`)
**OS actually tested:** macOS `27.2.1` (`26B131`, arm64 local `pnpm verify` + 30/30 real-FS watcher smoke test + local `cargo-deny 0.20.2` + macOS CI `macos-arm64`) and Windows (`windows-x64` GitHub Actions CI runner)
**Not tested:** Owner interactive desktop manual checks (`pnpm tauri dev` start-up window delay/unmanaged-state/error-dialog checks, live Desktop drag/drop watcher + auto-scan checkbox UI check, and laptop M4 Windows kill failure demonstration); live 24 h elapsed clock reconciliation (tested via deterministic `decide_next_auto_scan` unit test).

## 1. Summary & Part A: Pre-Implementation Audit (written before code)

Completed TASK-1G in three theme commits on branch `task/1G` in the order **C → B → A**, followed by the round-2 `deny.toml` license-exception fix (`eab2e4e`):

- **Theme C (`e36bc72`):** Hid `"main"` in `src-tauri/tauri.conf.json` (`"visible": false`), added `StartupError::MainWindowNotFound` (`"main_window_not_found"`) and `show_main_window` after `app.manage(...)` in `src-tauri/src/lib.rs`, updated `src/App.tsx` to render `"Starting…"` before `getAppInfo` responds and never expose raw `"state not managed"`, and eliminated Node `DEP0190` in `scripts/verify.mjs` by passing a single quoted command line on Windows without `args` + `shell: true`.
- **Theme B (`fa17884`):** Hardened `src-tauri/src/metadata/mod.rs` with pure `exiftool_args_for_os` (`-j -n -m -charset utf8 -charset filename=utf8 -@ -`, plus `-api WindowsLongPath=1` on Windows only after verifying identical JSON output on macOS), process-wide `EXIFTOOL_SEMAPHORE` (`Semaphore::const_new(2)`), concurrent `tokio::try_join!` stdin write / capped 512 KiB stdout read / capped 64 KiB drained stderr read + `child.wait()` under a single deadline, explicit `child.kill().await` + `child.wait().await` on timeout/error, fixed non-leaking `MetadataError` Display strings + strict `std::str::from_utf8` rejection, and `rustc`-compiled single-process tests covering stderr flood, concurrency limit 2, and Windows `tasklist` kill verification.
- **Theme A (`b7de6b5`):** Added `notify = "9.0.0"` (`CC0-1.0`) behind `WatchBackend`, implemented `src-tauri/src/watcher/` (`debounce.rs`, `scheduler.rs`, `manager.rs`, `backend.rs`, `tests.rs`) with O(1)-memory `RootInbox` folding inside the watcher callback, scanner skip-rule reuse, per-`Active`-root watchers (`WatchingState::On | Off | Degraded`), `auto_scan` in `app_settings` (default `false`), pure scheduler (`>= 60 s` gap, max 1 auto scan, oldest change first, `> 24 h` reconciliation), `LastJobSummary.trigger` (`"manual" | "auto"`), 5 s visible-only polling UI in `src/components/LibraryFolders.tsx`, and `docs/ipc.md` updates.
- **Round-2 `cargo-deny` fix (`eab2e4e`):** Per owner decision (2026-11-27), added a per-crate `BSL-1.0` exception for `xxhash-rust` only in root `deny.toml` (the repository's `deny.toml` sits at repo root, not `src-tauri/deny.toml`) without adding `BSL-1.0` globally or pinning an older `notify`. Local `cargo deny --manifest-path src-tauri/Cargo.toml check` (`cargo-deny v0.20.2`), local `pnpm verify`, local 30/30 real-FS watcher smoke loop, and CI run `46034567663` (`Security & License check (cargo deny)`, `macos-arm64`, and `windows-x64`) all passed. Automated acceptance criteria are met; owner interactive desktop manual checks and the owner's laptop M4 demonstration remain pending.

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

## 2. Acceptance criteria

| Criterion | Result | Evidence |
|---|---|---|
| Three commits in order **C → B → A**; `pnpm verify` + push after each | **Met** | Commits `e36bc72` (C), `fa17884` (B), `b7de6b5` (A) in `git log --oneline -n 6` |
| Theme C: Hidden window until ready, `MainWindowNotFound` (`"main_window_not_found"`), `"Starting…"`, no raw `"state not managed"` | **code met, manual check pending (owner)** | `src-tauri/tauri.conf.json`, `src-tauri/src/lib.rs`, `src-tauri/src/commands/error.rs`, `src/App.tsx`, `test_startup_error_variants_have_unique_codes_and_non_empty_messages`; live Tauri app launch is owner manual check |
| Theme C: `scripts/verify.mjs` eliminates Node `DEP0190` | **Met** | `scripts/verify.mjs`; round-1 `node -e` check on Node `v26.9.0` comparing `spawnSync` with `args` vs single string |
| Theme B: Pinned `exiftool_args` + Windows `-api WindowsLongPath=1` verified on Mac | **Met** | `exiftool_args_are_pinned` in Section 5.1 + round-1 Mac `exiftool` fixture check showing identical JSON with/without `-api WindowsLongPath=1` |
| Theme B: Concurrent capped stderr drain (64 KiB) + spawn-to-exit deadline + explicit `kill`/`wait` | **Met** | `stderr_flood_does_not_block_or_masquerade_as_timeout`, `wait_after_stdout_close_is_inside_the_timeout` (`#[cfg(unix)]`), `test_child_killed_after_timeout` (Section 5.1), `test_child_killed_after_timeout_windows` (CI `46034567663`, Section 5.4); round-1 mutations M2/M3/M4 on Mac |
| Theme B: Non-leaking `MetadataError`, non-UTF-8 error, semaphore limit 2 | **Met** | `error_display_carries_no_stderr_or_path`, `concurrency_limit_is_two` (Section 5.1), `MetadataError::InvalidUtf8` |
| Theme A: `notify` behind `WatchBackend`, O(1) debouncer (2 s / 30 s), scanner skip reuse, no catalogue writes from watcher | **Met** | `src-tauri/src/watcher/`, 15 watcher tests (`debouncer_*`, `ignored_names_do_not_mark_changes`, `event_flood_is_folded_into_one_flag`, `watcher_event_writes_no_catalogue_rows` in Section 5.1), `30/30` `real_watcher_detects_new_file_smoke` (Section 5.2) |
| Theme A: Scheduler (`auto_scan` default `false`, 60 s gap, max 1 auto scan, oldest change first, `> 24 h` reconciliation) + `LastJobSummary.trigger` + UI + `docs/ipc.md` | **code met, manual check pending (owner)** | `scheduler_*` tests, `auto_scan_setting_roundtrip_and_default_off`, `stale_root_is_due_after_24h`, `registry_stops_the_watcher_when_the_root_goes_offline_and_starts_it_when_active`, `watcher_start_failure_shows_off_and_does_not_fail_add_root`, `changes_detected_is_cleared_when_a_scan_starts` (Section 5.1), round-1 mutations M5/M6/M7 |
| `pnpm verify` green locally + Windows & macOS CI green | **Met** | Local `pnpm verify` (`204 passed; 0 failed; 2 ignored`, Section 5.1); CI `46034567663` `windows-x64` (`200 passed; 0 failed; 2 ignored`) and `macos-arm64` (`204 passed; 0 failed; 2 ignored`, Section 5.4) |
| `cargo deny` green locally and on CI | **Met** | Local `cargo deny --manifest-path src-tauri/Cargo.toml check` (`advisories ok, bans ok, licenses ok, sources ok`, Section 5.3); CI `46034567663` job `140626596979` `success` (Section 5.4) |

## 3. Files changed

Output of `git diff --stat main | cat` after writing this report:

```text
 deny.toml                          |    4 +
 docs/ipc.md                        |   22 +
 reports/REPORT-1G.md               | 2394 ++++++++++++++++++++++++++++++++++++
 scripts/verify.mjs                 |   16 +-
 src-tauri/Cargo.lock               |   89 +-
 src-tauri/Cargo.toml               |    1 +
 src-tauri/src/commands/error.rs    |    6 +
 src-tauri/src/commands/mod.rs      |    2 +
 src-tauri/src/commands/roots.rs    |  182 ++-
 src-tauri/src/commands/settings.rs |   45 +
 src-tauri/src/database/jobs.rs     |   15 +-
 src-tauri/src/database/mod.rs      |   37 +
 src-tauri/src/domain/roots.rs      |   12 +
 src-tauri/src/jobs/mod.rs          |   93 +-
 src-tauri/src/lib.rs               |   46 +-
 src-tauri/src/library/roots.rs     |    9 +-
 src-tauri/src/metadata/mod.rs      |  753 ++++++++++--
 src-tauri/src/scanner/classify.rs  |   26 +
 src-tauri/src/scanner/mod.rs       |    5 +-
 src-tauri/src/watcher/backend.rs   |   86 ++
 src-tauri/src/watcher/debounce.rs  |  110 ++
 src-tauri/src/watcher/manager.rs   |  427 +++++++
 src-tauri/src/watcher/mod.rs       |   18 +
 src-tauri/src/watcher/scheduler.rs |  103 ++
 src-tauri/src/watcher/tests.rs     |  974 +++++++++++++++
 src-tauri/tauri.conf.json          |    3 +-
 src/App.tsx                        |   49 +-
 src/components/LibraryFolders.tsx  |  224 +++-
 src/lib/scanText.ts                |   14 +
 src/lib/tauri/errorCodes.ts        |    1 +
 src/lib/tauri/index.ts             |   14 +
 src/lib/tauri/schemas.ts           |    7 +
 32 files changed, 5599 insertions(+), 188 deletions(-)
```

**New files added:**
- `src-tauri/src/commands/settings.rs`: IPC commands `get_auto_scan` / `set_auto_scan` and testable `_impl` helpers backed by `app_settings`.
- `src-tauri/src/watcher/mod.rs`: Public module exports for watcher backend, debouncer, scheduler, manager, and tests.
- `src-tauri/src/watcher/backend.rs`: `WatchBackend` trait, `WatchEvent` enum, and `NotifyWatchBackend` wrapping `notify::RecommendedWatcher`.
- `src-tauri/src/watcher/debounce.rs`: Pure `Instant`-driven `RootDebounceState` (2 s quiet, 30 s max wait) and O(1) `RootInbox` event folding.
- `src-tauri/src/watcher/scheduler.rs`: Pure `decide_next_auto_scan` policy function (auto scan toggle, Active-only, 60 s gap, oldest change first, 24 h reconciliation).
- `src-tauri/src/watcher/manager.rs`: `WatcherManager` per-root registry and 1 s background ticker (`spawn_watcher_and_scheduler`).
- `src-tauri/src/watcher/tests.rs`: 15 unit, integration, and real-filesystem smoke tests for watcher, debouncer, settings, and scheduler.

**Round-2 config fix (`eab2e4e`):**
- `deny.toml` (at repository root; there is no `src-tauri/deny.toml`): Added per-crate license exception `{ crate = "xxhash-rust", allow = ["BSL-1.0"] }` under `[licenses]`.

## 4. Dependencies

- **`notify = { version = "9.0.0", default-features = false, features = ["macos_fsevent"] }`**
  - **Source & license verified:** `cargo info notify` / `https://crates.io/crates/notify/9.0.0` (`version: 9.0.0`, `license: CC0-1.0`, `rust-version: 1.91`, `repository: https://github.com/notify-rs/notify.git`, `+default = [macos_fsevent]`).
  - **Transitive `BSL-1.0` handling:** `notify v9.0.0` pulls in `xxhash-rust v0.8.21` licensed under `BSL-1.0` (Boost Software License 1.0 — permissive, OSI-approved, and FSF Free/Libre, not the Business Source License). Per owner decision (2026-11-27), `deny.toml` permits `BSL-1.0` via a per-crate exception for `xxhash-rust` only (`exceptions = [{ crate = "xxhash-rust", allow = ["BSL-1.0"] }]`), verified with `cargo-deny v0.20.2` locally and in CI.
- **Global tool installed (declared per `AGENTS.md`):** `cargo-deny v0.20.2` via `cargo install cargo-deny --version 0.20.2 --locked` (matching `.github/workflows/ci.yml`).
- No npm packages, Tauri plugins, or `capabilities/` changes were added.

## 5. Commands run

### 5.1 Final local `pnpm verify` (commit `eab2e4e`, macOS `27.2.1` arm64)

Full raw output of `pnpm verify` (also saved in `reports/logs/verify-2026-11-27.txt`):

```text
Photos Manager — pnpm verify verification run
Date: 2026-11-27T16:31:21.589Z
Log output file: /Users/narayanwaraich/Desktop/Projects/photos-manager/reports/logs/verify-2026-11-27.txt

======================================================
STEP: cargo fmt check
COMMAND: cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
CWD: /Users/narayanwaraich/Desktop/Projects/photos-manager
======================================================


✓ PASSED: cargo fmt check

======================================================
STEP: cargo clippy
COMMAND: cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
CWD: /Users/narayanwaraich/Desktop/Projects/photos-manager
======================================================

    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.37s

✓ PASSED: cargo clippy

======================================================
STEP: cargo test
COMMAND: cargo test --manifest-path src-tauri/Cargo.toml
CWD: /Users/narayanwaraich/Desktop/Projects/photos-manager
======================================================


running 206 tests
test commands::error::tests::test_ipc_error_from_startup_error ... ok
test commands::error::tests::test_ipc_error_from_scan_error ... ok
test commands::error::tests::test_ipc_error_from_root_error ... ok
test commands::error::tests::test_ipc_error_mappings_leak_no_data ... ok
test commands::error::tests::test_error_codes_sync_with_typescript ... ok
test commands::app_info::tests::test_app_info_closed_pool_returns_database_error ... ok
test commands::roots::tests::test_add_root_label_too_long_returns_root_label_invalid ... ok
test commands::app_info::tests::test_app_info_returns_expected_fields ... ok
test commands::roots::tests::test_add_root_fake_picker_cancelled_returns_cancelled_and_writes_nothing ... ok
test commands::roots::tests::test_add_root_fake_picker_success_returns_added_and_creates_root_row ... ok
test commands::roots::tests::test_add_root_blank_label_falls_back_to_folder_name ... ok
test commands::roots::tests::test_add_root_missing_folder_returns_root_path_invalid_fixed_message_no_path ... ok
test commands::roots::tests::remove_root_works_when_offline ... ok
test commands::roots::tests::test_add_root_protected_folder_returns_root_protected_fixed_message_no_path ... ok
test commands::roots::tests::test_wire_format_add_root_result_matches_literal_json ... ok
test commands::roots::tests::test_wire_format_ipc_error_matches_literal_json ... ok
test commands::roots::tests::test_wire_format_relocate_root_result_matches_literal_json ... ok
test commands::roots::tests::test_wire_format_remove_root_preview_matches_literal_json ... ok
test commands::roots::tests::test_wire_format_root_summary_matches_literal_json ... ok
test commands::roots::tests::test_dialog_guard_rejects_concurrent_second_add_with_dialog_busy ... ok
test commands::roots::tests::test_add_root_overlapping_folder_returns_root_overlaps_fixed_message_no_path ... ok
test commands::roots::tests::relocate_root_success_updates_only_paths_status_and_volume_seen ... ok
test database::tests::test_enum_from_str_rejection_all_enums ... ok
test commands::roots::tests::offline_round_trip_writes_no_file_or_folder_rows ... ok
test commands::roots::tests::relocate_root_refusals_and_cancelled_dialog ... ok
test database::tests::test_foreign_keys_on_every_connection ... ok
test commands::roots::tests::test_remove_root_with_stale_count_fails_with_root_changed_since_preview_and_deletes_nothing ... ok
test database::tests::test_duplicate_primary_key_is_database_error_not_collision ... ok
test database::tests::test_init_pool_and_app_settings_migration ... ok
test commands::roots::tests::test_preview_and_remove_root_with_matching_count_succeeds ... ok
test database::tests::test_cascade_delete_root ... ok
test database::tests::test_migration_0a_to_phase1 ... ok
test filesystem::tests::test_from_absolute_not_inside_root ... ok
test filesystem::tests::test_from_absolute_root_itself ... ok
test filesystem::tests::test_from_absolute_traversal_in_abs ... ok
test filesystem::tests::test_from_components_and_from_absolute_reject_same_invalid_names ... ok
test filesystem::tests::test_nfc_vs_nfd_key_equality ... ok
test database::tests::test_platform_guard_pre_migration_rejection ... ok
test database::tests::test_enum_check_sync_and_size_range ... ok
test database::tests::test_migration_jobs_table_and_invariants ... ok
test database::tests::test_migration_one_root_folder_per_root_invariant ... ok
test database::tests::test_platform_guard_rejects_mismatch ... ok
test filesystem::tests::test_rejection_table_driven ... ok
test filesystem::tests::test_unix_not_unicode_rejection ... ok
test database::tests::test_uniqueness_path_key_collision ... ok
test database::tests::test_repository_functions_crud ... ok
test filesystem::tests::test_proptest_arbitrary_component_validation ... ok
test filesystem::tests::test_proptest_key_rules_case_policy ... ok
test filesystem::tests::test_proptest_native_roundtrip_unix ... ok
test commands::roots::tests::test_list_roots_never_returning_checker_completes_within_timeout_and_reports_unreachable ... ok
test filesystem::tests::test_proptest_nfc_nfd_key_equality ... ok
test filesystem::tests::test_proptest_display_and_separator_independence ... ok
test image_pipeline::vips::tests::test_vips_supported_loaders_reports_real_codecs ... ok
test image_pipeline::vips::tests::test_vips_missing_file_error ... ok
test image_pipeline::vips::tests::test_vips_blocks_svgload ... ok
test jobs::tests::active_job_id_and_row_agree_until_the_final_write_ends ... ok
test jobs::tests::dead_check_at_scan_start_times_out_and_does_not_block_second_scan ... ok
test jobs::tests::final_write_failure_cancelled_path_emits_recorded_false_and_allows_next_scan ... ok
test jobs::tests::test_cancel_unknown_or_finished_job_returns_job_not_found ... ok
test jobs::tests::final_write_failure_failed_path_emits_recorded_false_and_allows_next_scan ... ok
test jobs::tests::final_write_retry_succeeds_after_first_attempt_fails ... ok
test jobs::tests::final_write_failure_panic_path_emits_recorded_false_and_allows_next_scan ... ok
test jobs::tests::test_dead_channel_does_not_stop_job ... ok
test jobs::tests::test_mark_job_cancelling_only_affects_running_jobs ... ok
test jobs::tests::test_job_lifecycle_root_vanishes_after_preflight_fails_with_scan_root_unreachable ... ok
test jobs::tests::test_job_lifecycle_completed_scan ... ok
test jobs::tests::test_panic_isolation_marks_failed_and_frees_guard ... ok
test jobs::tests::test_refused_start_creates_no_job_row ... ok
test jobs::tests::test_remove_root_while_scanning_returns_root_busy ... ok
test jobs::tests::test_retention_prunes_older_finished_jobs_keeping_newest_20 ... ok
test jobs::tests::test_scan_failure_codes_sync_with_typescript ... ok
test jobs::tests::test_start_scan_unreachable_root_refused_with_root_offline_and_marks_offline ... ok
test image_pipeline::vips::tests::test_vips_blocks_magickload ... ok
test jobs::tests::test_startup_reconciliation_marks_interrupted ... ok
test jobs::tests::test_wire_format_scan_events_matches_literal_json ... ok
test jobs::tests::final_write_failure_completed_path_emits_recorded_false_and_allows_next_scan ... ok
test library::canonical::tests::test_canonicalize_root_accepts_trailing_colon_directory ... ok
test library::canonical::tests::test_canonicalize_root_directory_success ... ok
test library::canonical::tests::test_canonicalize_root_file_instead_of_directory_rejected ... ok
test library::canonical::tests::test_canonicalize_root_missing_path_rejected ... ok
test library::canonical::tests::test_canonicalize_root_non_unicode_name_rejected ... ok
test library::canonical::tests::test_canonicalize_root_relative_path_rejected ... ok
test library::canonical::tests::test_canonicalize_root_symlink_resolves_to_target ... ok
test library::canonical::tests::test_strip_verbatim_prefix ... ok
test library::case_probe::tests::test_case_probe_does_not_fold_case_swapped_has_different_identity ... ok
test library::case_probe::tests::test_case_probe_does_not_fold_case_swapped_is_absent ... ok
test library::case_probe::tests::test_case_probe_folds_case ... ok
test library::case_probe::tests::test_case_probe_lookup_failure_returns_unknown ... ok
test library::case_probe::tests::test_case_probe_no_cased_component_returns_unknown ... ok
test image_pipeline::vips::tests::test_vips_thumbnail_rotated_jpeg ... ok
test library::case_probe::tests::test_case_probe_real_filesystem_read_only_and_correctness ... ok
test library::overlap::tests::test_overlap_rule_differing_only_in_case_rejected ... ok
test library::overlap::tests::test_overlap_rule_differing_only_in_unicode_nfc_nfd_rejected ... ok
test library::overlap::tests::test_overlap_rule_identical_roots_rejected ... ok
test library::overlap::tests::test_overlap_rule_many_roots_table_driven ... ok
test library::case_probe::tests::test_find_cased_child_entry_on_temp_directory ... ok
test library::overlap::tests::test_overlap_rule_new_contains_existing_rejected ... ok
test library::overlap::tests::test_overlap_rule_new_inside_existing_rejected ... ok
test library::overlap::tests::test_overlap_rule_offline_stored_root_still_rejected ... ok
test library::overlap::tests::test_overlap_rule_siblings_with_common_string_prefix_allowed ... ok
test library::protected::tests::test_protected_app_data_and_container_rejected ... ok
test library::protected::tests::test_protected_folder_next_to_protected_allowed ... ok
test library::protected::tests::test_protected_home_dir_itself_rejected ... ok
test library::protected::tests::test_protected_home_parent_rejected ... ok
test library::protected::tests::test_protected_os_system_folder_rejected ... ok
test library::protected::tests::test_protected_photoslibrary_rejected_on_macos_allowed_on_windows ... ok
test library::protected::tests::test_protected_pictures_inside_home_allowed ... ok
test library::protected::tests::test_protected_unix_system_root_and_volumes_photos ... ok
test library::protected::tests::test_protected_volumes_mount_folder_rejected ... ok
test library::protected::tests::test_protected_context_from_environment ... ok
test library::protected::tests::test_protected_windows_c_drive_d_drive_and_unc_share ... ok
test library::protected::tests::test_protected_windows_system_volume_and_recycle_bin_rejected ... ok
test jobs::tests::test_job_lifecycle_cancelled_scan_leaves_zero_missing ... ok
test jobs::tests::volume_guard_refuses_empty_mount_point_and_marks_nothing_missing ... ok
test image_pipeline::vips::tests::test_vips_thumbnail_png ... ok
test image_pipeline::vips::tests::test_vips_error_buffer_no_stale_text ... ok
test library::roots::tests::case_refresh_definite_change_with_files_refused_volume_case_changed ... ok
test library::roots::tests::case_refresh_definite_change_without_files_updates_volume ... ok
test library::roots::tests::case_refresh_unknown_probe_changes_nothing ... ok
test jobs::tests::test_persistence_completed_scan_matches_root_last_scan_generation ... ok
test jobs::tests::test_persistence_before_touching_disk_failure_leaves_null_generation ... ok
test library::roots::tests::serial_identity_reuse_requires_equal_fs_type_and_label_and_supports_legacy_bare ... ok
test library::roots::tests::test_add_root_basic_success ... ok
test library::roots::tests::test_add_root_concurrent_race_serialises_overlap ... ok
test library::roots::tests::test_add_root_rolls_back_completely_on_forced_failure ... ok
test library::roots::tests::test_add_two_roots_with_different_uuid_creates_two_volume_rows ... ok
test library::roots::tests::test_add_two_roots_on_same_volume_reuses_volume_row ... ok
test library::volume::tests::test_describe_volume_on_current_system ... ok
test library::volume::tests::volume_report ... ignored
test library::roots::tests::test_add_two_roots_with_null_uuid_creates_two_volume_rows ... ok
test library::roots::tests::test_list_roots_counts_with_hand_inserted_tree ... ok
test metadata::tests::exiftool_args_are_pinned ... ok
test library::roots::tests::test_list_roots_reachable_false_after_directory_removed ... ok
test library::roots::tests::test_overlap_and_protected_refusals_leave_zero_rows ... ok
test library::roots::tests::test_remove_root_with_right_count_and_leaves_disk_untouched ... ok
test library::roots::tests::status_changes_only_on_definite_answers ... ok
test metadata::tests::test_exiftool_missing_file ... ok
test metadata::tests::test_exiftool_long_path ... ok
test metadata::tests::test_exiftool_rejects_non_absolute_path ... ok
test metadata::tests::test_exiftool_malformed_output ... ok
test metadata::tests::test_exiftool_non_ascii_filename ... ok
test metadata::tests::test_exiftool_success ... ok
test platform::macos::tests::test_parse_statfs_kind_mapping_table ... ok
test platform::tests::test_format_serial_identity_uses_serial_prefix_and_uppercase_hex ... ok
test platform::tests::test_same_volume_identity_handles_exact_and_legacy_bare_serial ... ok
test platform::windows::tests::test_parse_drive_type_mapping_table ... ok
test scanner::classify::tests::test_always_skipped_names ... ok
test scanner::classify::tests::test_classify_all_image_extensions ... ok
test scanner::classify::tests::test_classify_ignored_extensions_and_no_extensions ... ok
test scanner::classify::tests::test_classify_raw_extensions ... ok
test scanner::classify::tests::test_classify_video_extensions ... ok
test scanner::classify::tests::test_extension_lists_have_no_duplicates_and_no_overlap ... ok
test scanner::tests::scan_report ... ignored
test scanner::tests::test_batching_small_batch_commits_transactions_identically ... ok
test library::roots::tests::test_add_root_waits_for_held_immediate_transaction ... ok
test scanner::tests::test_cancellation_deadlock_prevention ... ok
test scanner::tests::test_first_scan_mixed_tree_creates_folders_files_assets ... ok
test scanner::tests::test_changed_deleted_and_restored_file_lifecycle ... ok
test scanner::tests::test_interrupted_scans_leave_nothing_wrongly_missing_and_rescan_equals_uninterrupted ... ok
test scanner::tests::test_move_file_identity_null_never_matched ... ok
test scanner::tests::test_move_rename_folder_preserving_file_ids_and_assets ... ok
test scanner::tests::test_move_rename_inside_same_folder ... ok
test scanner::tests::test_moves_detected_preserving_file_ids_and_hardlink_disambiguation ... ok
test scanner::tests::test_moves_exclude_already_missing_rows_decision_n ... ok
test scanner::tests::test_name_rules_and_path_key_collision_carries_existing_id ... ok
test scanner::tests::test_photoslibrary_package_directory_skipped ... ok
test scanner::tests::test_scan_guard_rejects_concurrent_scan_on_same_root ... ok
test scanner::tests::test_scan_name_rules_non_unicode_and_forbidden_characters_unix ... ok
test scanner::tests::test_scan_name_rules_path_key_collision_through_scan ... ok
test scanner::tests::test_second_scan_unchanged_stamps_generation_and_leaves_rows_intact ... ok
test scanner::tests::test_symlinks_not_followed_and_counted_as_skipped ... ok
test scanner::tests::test_synthetic_inaccessible_dir_marks_subtree_inaccessible_and_never_missing ... ok
test scanner::tests::test_synthetic_inaccessible_file_marked_inaccessible ... ok
test scanner::tests::test_synthetic_warnings_capped_at_100_via_apply_batch ... ok
test scanner::tests::test_unreachable_root_and_symlink_refusal ... ok
test scanner::tests::test_unreadable_directory_carried_over_as_inaccessible_and_restored ... ok
test scanner::tests::test_walk_failure_returns_scan_walk_failed_leaves_zero_missing_and_releases_guard ... ok
test scanner::tests::test_warning_codes_sync_with_typescript_and_scanner_sources ... ok
test jobs::tests::test_throttle_rate_limits_progress_and_job_updates ... ok
test tests::test_startup_error_variants_have_unique_codes_and_non_empty_messages ... ok
test tests::test_startup_platform_mismatch ... ok
test tests::test_startup_with_unusable_data_folder ... ok
test watcher::tests::auto_scan_setting_roundtrip_and_default_off ... ok
test watcher::tests::changes_detected_is_cleared_when_a_scan_starts ... ok
test watcher::tests::debouncer_folds_a_burst_into_one_mark_after_the_quiet_period ... ok
test watcher::tests::debouncer_max_wait_forces_a_mark_during_a_continuous_stream ... ok
test watcher::tests::event_flood_is_folded_into_one_flag ... ok
test watcher::tests::ignored_names_do_not_mark_changes ... ok
test scanner::tests::test_warnings_capped_at_100 ... ok
test watcher::tests::registry_stops_the_watcher_when_the_root_goes_offline_and_starts_it_when_active ... ok
test watcher::tests::scheduler_does_nothing_when_auto_scan_is_off ... ok
test watcher::tests::scheduler_respects_the_60s_gap ... ok
test watcher::tests::scheduler_skips_busy_offline_and_paused_roots ... ok
test watcher::tests::scheduler_starts_one_scan_at_a_time_oldest_change_first ... ok
test watcher::tests::stale_root_is_due_after_24h ... ok
test watcher::tests::watcher_event_writes_no_catalogue_rows ... ok
test watcher::tests::watcher_start_failure_shows_off_and_does_not_fail_add_root ... ok
test metadata::tests::error_display_carries_no_stderr_or_path ... ok
test metadata::tests::test_exiftool_timeout ... ok
test metadata::tests::test_child_killed_after_timeout ... ok
test metadata::tests::stderr_flood_does_not_block_or_masquerade_as_timeout ... ok
test scanner::tests::test_large_tree_5000_files_bounded_memory ... ok
test metadata::tests::concurrency_limit_is_two ... ok
test commands::roots::tests::test_list_roots_second_call_while_blocked_does_not_start_second_check ... ok
test metadata::tests::wait_after_stdout_close_is_inside_the_timeout ... ok
test watcher::tests::real_watcher_detects_new_file_smoke ... ok

test result: ok. 204 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 3.66s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.27s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/photos_manager_lib-8f486d04dd8885ce)
     Running unittests src/main.rs (src-tauri/target/debug/deps/photos_manager-0d89fc71e88de003)
   Doc-tests photos_manager_lib

✓ PASSED: cargo test

======================================================
STEP: frontend typecheck
COMMAND: pnpm exec tsc --noEmit
CWD: /Users/narayanwaraich/Desktop/Projects/photos-manager
======================================================

[WARN] Issue while reading "/Users/narayanwaraich/.npmrc". Operation not permitted (os error 1)

✓ PASSED: frontend typecheck

======================================================
STEP: frontend lint
COMMAND: pnpm exec eslint src/
CWD: /Users/narayanwaraich/Desktop/Projects/photos-manager
======================================================

[WARN] Issue while reading "/Users/narayanwaraich/.npmrc". Operation not permitted (os error 1)

✓ PASSED: frontend lint

======================================================
STEP: production build
COMMAND: pnpm exec vite build
CWD: /Users/narayanwaraich/Desktop/Projects/photos-manager
======================================================

vite v8.3.5 building client environment for production...
transforming...
✓ 32 modules transformed.
rendering chunks...
computing gzip size...
dist/index.html                  0.32 kB │ gzip:  0.23 kB
dist/assets/index-MP1oEFkV.js  301.76 kB │ gzip: 88.58 kB

✓ built in 62ms
[WARN] Issue while reading "/Users/narayanwaraich/.npmrc". Operation not permitted (os error 1)

✓ PASSED: production build

======================================================
STEP: exiftool version check unit tests
COMMAND: node --test scripts/test-exiftool-version.mjs
CWD: /Users/narayanwaraich/Desktop/Projects/photos-manager
======================================================

✔ ci.yml and docs/native-dependencies.md must match PINNED_EXIFTOOL_VERSION and SHA-256 constants (2.884167ms)
✔ checkExifToolVersion: exact match succeeds without failure (0.10975ms)
✔ checkExifToolVersion: mismatch produces warning locally (shouldFail=false) (0.049834ms)
✔ checkExifToolVersion: mismatch in CI produces error (shouldFail=true) (0.05225ms)
✔ checkExifToolVersion: unparsable output produces failure in CI and warning locally (0.037958ms)
ℹ tests 5
ℹ suites 0
ℹ pass 5
ℹ fail 0
ℹ cancelled 0
ℹ skipped 0
ℹ todo 0
ℹ duration_ms 95.503125

✓ PASSED: exiftool version check unit tests

======================================================
STEP: AGENTS.md length check (< 12,000 characters)
======================================================

AGENTS.md character count: 7439
✓ PASSED: AGENTS.md character count is under 12,000

======================================================
ALL VERIFICATION CHECKS PASSED SUCCESSFULLY!
======================================================
```

### 5.2 Real-filesystem watcher smoke test 30/30 loop (commit `eab2e4e`, macOS `27.2.1` arm64)

Command: `for i in $(seq 1 30); do echo "=== Run $i/30 ==="; cargo test --manifest-path src-tauri/Cargo.toml watcher::tests::real_watcher_detects_new_file_smoke -- --exact || exit 1; done`

Full raw output (30/30 passed):

```text
=== Run 1/30 ===
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.13s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/photos_manager_lib-8f486d04dd8885ce)

running 1 test
test watcher::tests::real_watcher_detects_new_file_smoke ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 205 filtered out; finished in 2.37s

     Running unittests src/main.rs (src-tauri/target/debug/deps/photos_manager-0d89fc71e88de003)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

=== Run 2/30 ===
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.13s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/photos_manager_lib-8f486d04dd8885ce)

running 1 test
test watcher::tests::real_watcher_detects_new_file_smoke ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 205 filtered out; finished in 2.36s

     Running unittests src/main.rs (src-tauri/target/debug/deps/photos_manager-0d89fc71e88de003)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

=== Run 3/30 ===
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.13s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/photos_manager_lib-8f486d04dd8885ce)

running 1 test
test watcher::tests::real_watcher_detects_new_file_smoke ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 205 filtered out; finished in 2.36s

     Running unittests src/main.rs (src-tauri/target/debug/deps/photos_manager-0d89fc71e88de003)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

=== Run 4/30 ===
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.15s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/photos_manager_lib-8f486d04dd8885ce)

running 1 test
test watcher::tests::real_watcher_detects_new_file_smoke ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 205 filtered out; finished in 2.37s

     Running unittests src/main.rs (src-tauri/target/debug/deps/photos_manager-0d89fc71e88de003)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

=== Run 5/30 ===
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.13s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/photos_manager_lib-8f486d04dd8885ce)

running 1 test
test watcher::tests::real_watcher_detects_new_file_smoke ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 205 filtered out; finished in 2.36s

     Running unittests src/main.rs (src-tauri/target/debug/deps/photos_manager-0d89fc71e88de003)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

=== Run 6/30 ===
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.13s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/photos_manager_lib-8f486d04dd8885ce)

running 1 test
test watcher::tests::real_watcher_detects_new_file_smoke ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 205 filtered out; finished in 2.36s

     Running unittests src/main.rs (src-tauri/target/debug/deps/photos_manager-0d89fc71e88de003)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

=== Run 7/30 ===
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.13s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/photos_manager_lib-8f486d04dd8885ce)

running 1 test
test watcher::tests::real_watcher_detects_new_file_smoke ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 205 filtered out; finished in 2.37s

     Running unittests src/main.rs (src-tauri/target/debug/deps/photos_manager-0d89fc71e88de003)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

=== Run 8/30 ===
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.13s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/photos_manager_lib-8f486d04dd8885ce)

running 1 test
test watcher::tests::real_watcher_detects_new_file_smoke ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 205 filtered out; finished in 2.36s

     Running unittests src/main.rs (src-tauri/target/debug/deps/photos_manager-0d89fc71e88de003)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

=== Run 9/30 ===
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.13s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/photos_manager_lib-8f486d04dd8885ce)

running 1 test
test watcher::tests::real_watcher_detects_new_file_smoke ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 205 filtered out; finished in 2.36s

     Running unittests src/main.rs (src-tauri/target/debug/deps/photos_manager-0d89fc71e88de003)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

=== Run 10/30 ===
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.13s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/photos_manager_lib-8f486d04dd8885ce)

running 1 test
test watcher::tests::real_watcher_detects_new_file_smoke ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 205 filtered out; finished in 2.37s

     Running unittests src/main.rs (src-tauri/target/debug/deps/photos_manager-0d89fc71e88de003)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

=== Run 11/30 ===
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.13s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/photos_manager_lib-8f486d04dd8885ce)

running 1 test
test watcher::tests::real_watcher_detects_new_file_smoke ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 205 filtered out; finished in 2.36s

     Running unittests src/main.rs (src-tauri/target/debug/deps/photos_manager-0d89fc71e88de003)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

=== Run 12/30 ===
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.13s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/photos_manager_lib-8f486d04dd8885ce)

running 1 test
test watcher::tests::real_watcher_detects_new_file_smoke ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 205 filtered out; finished in 2.36s

     Running unittests src/main.rs (src-tauri/target/debug/deps/photos_manager-0d89fc71e88de003)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

=== Run 13/30 ===
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.13s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/photos_manager_lib-8f486d04dd8885ce)

running 1 test
test watcher::tests::real_watcher_detects_new_file_smoke ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 205 filtered out; finished in 2.36s

     Running unittests src/main.rs (src-tauri/target/debug/deps/photos_manager-0d89fc71e88de003)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

=== Run 14/30 ===
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.14s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/photos_manager_lib-8f486d04dd8885ce)

running 1 test
test watcher::tests::real_watcher_detects_new_file_smoke ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 205 filtered out; finished in 2.37s

     Running unittests src/main.rs (src-tauri/target/debug/deps/photos_manager-0d89fc71e88de003)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

=== Run 15/30 ===
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.13s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/photos_manager_lib-8f486d04dd8885ce)

running 1 test
test watcher::tests::real_watcher_detects_new_file_smoke ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 205 filtered out; finished in 2.36s

     Running unittests src/main.rs (src-tauri/target/debug/deps/photos_manager-0d89fc71e88de003)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

=== Run 16/30 ===
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.13s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/photos_manager_lib-8f486d04dd8885ce)

running 1 test
test watcher::tests::real_watcher_detects_new_file_smoke ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 205 filtered out; finished in 2.37s

     Running unittests src/main.rs (src-tauri/target/debug/deps/photos_manager-0d89fc71e88de003)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

=== Run 17/30 ===
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.13s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/photos_manager_lib-8f486d04dd8885ce)

running 1 test
test watcher::tests::real_watcher_detects_new_file_smoke ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 205 filtered out; finished in 2.36s

     Running unittests src/main.rs (src-tauri/target/debug/deps/photos_manager-0d89fc71e88de003)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

=== Run 18/30 ===
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.13s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/photos_manager_lib-8f486d04dd8885ce)

running 1 test
test watcher::tests::real_watcher_detects_new_file_smoke ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 205 filtered out; finished in 2.36s

     Running unittests src/main.rs (src-tauri/target/debug/deps/photos_manager-0d89fc71e88de003)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

=== Run 19/30 ===
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.13s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/photos_manager_lib-8f486d04dd8885ce)

running 1 test
test watcher::tests::real_watcher_detects_new_file_smoke ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 205 filtered out; finished in 2.36s

     Running unittests src/main.rs (src-tauri/target/debug/deps/photos_manager-0d89fc71e88de003)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

=== Run 20/30 ===
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.13s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/photos_manager_lib-8f486d04dd8885ce)

running 1 test
test watcher::tests::real_watcher_detects_new_file_smoke ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 205 filtered out; finished in 2.36s

     Running unittests src/main.rs (src-tauri/target/debug/deps/photos_manager-0d89fc71e88de003)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

=== Run 21/30 ===
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.13s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/photos_manager_lib-8f486d04dd8885ce)

running 1 test
test watcher::tests::real_watcher_detects_new_file_smoke ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 205 filtered out; finished in 2.36s

     Running unittests src/main.rs (src-tauri/target/debug/deps/photos_manager-0d89fc71e88de003)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

=== Run 22/30 ===
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.13s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/photos_manager_lib-8f486d04dd8885ce)

running 1 test
test watcher::tests::real_watcher_detects_new_file_smoke ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 205 filtered out; finished in 2.36s

     Running unittests src/main.rs (src-tauri/target/debug/deps/photos_manager-0d89fc71e88de003)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

=== Run 23/30 ===
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.13s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/photos_manager_lib-8f486d04dd8885ce)

running 1 test
test watcher::tests::real_watcher_detects_new_file_smoke ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 205 filtered out; finished in 2.37s

     Running unittests src/main.rs (src-tauri/target/debug/deps/photos_manager-0d89fc71e88de003)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

=== Run 24/30 ===
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.13s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/photos_manager_lib-8f486d04dd8885ce)

running 1 test
test watcher::tests::real_watcher_detects_new_file_smoke ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 205 filtered out; finished in 2.54s

     Running unittests src/main.rs (src-tauri/target/debug/deps/photos_manager-0d89fc71e88de003)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

=== Run 25/30 ===
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.13s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/photos_manager_lib-8f486d04dd8885ce)

running 1 test
test watcher::tests::real_watcher_detects_new_file_smoke ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 205 filtered out; finished in 2.36s

     Running unittests src/main.rs (src-tauri/target/debug/deps/photos_manager-0d89fc71e88de003)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

=== Run 26/30 ===
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.13s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/photos_manager_lib-8f486d04dd8885ce)

running 1 test
test watcher::tests::real_watcher_detects_new_file_smoke ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 205 filtered out; finished in 2.37s

     Running unittests src/main.rs (src-tauri/target/debug/deps/photos_manager-0d89fc71e88de003)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

=== Run 27/30 ===
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.13s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/photos_manager_lib-8f486d04dd8885ce)

running 1 test
test watcher::tests::real_watcher_detects_new_file_smoke ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 205 filtered out; finished in 2.37s

     Running unittests src/main.rs (src-tauri/target/debug/deps/photos_manager-0d89fc71e88de003)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

=== Run 28/30 ===
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.17s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/photos_manager_lib-8f486d04dd8885ce)

running 1 test
test watcher::tests::real_watcher_detects_new_file_smoke ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 205 filtered out; finished in 2.37s

     Running unittests src/main.rs (src-tauri/target/debug/deps/photos_manager-0d89fc71e88de003)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

=== Run 29/30 ===
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.13s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/photos_manager_lib-8f486d04dd8885ce)

running 1 test
test watcher::tests::real_watcher_detects_new_file_smoke ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 205 filtered out; finished in 2.37s

     Running unittests src/main.rs (src-tauri/target/debug/deps/photos_manager-0d89fc71e88de003)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

=== Run 30/30 ===
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.14s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/photos_manager_lib-8f486d04dd8885ce)

running 1 test
test watcher::tests::real_watcher_detects_new_file_smoke ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 205 filtered out; finished in 2.37s

     Running unittests src/main.rs (src-tauri/target/debug/deps/photos_manager-0d89fc71e88de003)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

### 5.3 Local `cargo deny` check (commit `eab2e4e`, `cargo-deny v0.20.2`)

Command: `cargo deny --manifest-path src-tauri/Cargo.toml check`

Full raw output:

```text
warning[duplicate]: found 3 duplicate entries for crate 'base64'
   ┌─ /Users/narayanwaraich/Desktop/Projects/photos-manager/src-tauri/Cargo.lock:9:1
   │  
 9 │ ╭ base64 0.21.7 registry+https://github.com/rust-lang/crates.io-index
10 │ │ base64 0.22.1 registry+https://github.com/rust-lang/crates.io-index
11 │ │ base64 0.23.1 registry+https://github.com/rust-lang/crates.io-index
   │ ╰───────────────────────────────────────────────────────────────────┘ lock entries
   │  
   ├ base64 v0.21.7
     └── swift-rs v1.0.9
         └── tauri-utils v2.10.2
             ├── tauri v2.12.5
             │   └── photos-manager v0.1.0
             ├── tauri-build v2.7.4
             │   ├── (build) photos-manager v0.1.0 (*)
             │   └── (build) tauri v2.12.5 (*)
             ├── tauri-codegen v2.7.4
             │   └── tauri-macros v2.7.4
             │       └── tauri v2.12.5 (*)
             ├── tauri-macros v2.7.4 (*)
             ├── tauri-runtime v2.12.2
             │   ├── tauri v2.12.5 (*)
             │   └── tauri-runtime-wry v2.12.3
             │       └── tauri v2.12.5 (*)
             └── tauri-runtime-wry v2.12.3 (*)
   ├ base64 v0.22.1
     └── sqlx-core v0.8.6
         ├── sqlx v0.8.6
         │   └── photos-manager v0.1.0
         ├── sqlx-macros v0.8.6
         │   └── sqlx v0.8.6 (*)
         ├── sqlx-macros-core v0.8.6
         │   └── sqlx-macros v0.8.6 (*)
         └── sqlx-sqlite v0.8.6
             ├── sqlx v0.8.6 (*)
             └── sqlx-macros-core v0.8.6 (*)
   ├ base64 v0.23.1
     ├── plist v1.10.3
     │   ├── tauri v2.12.5
     │   │   └── photos-manager v0.1.0
     │   ├── tauri-codegen v2.7.4
     │   │   └── tauri-macros v2.7.4
     │   │       └── tauri v2.12.5 (*)
     │   └── tauri-utils v2.10.2
     │       ├── tauri v2.12.5 (*)
     │       ├── tauri-build v2.7.4
     │       │   ├── (build) photos-manager v0.1.0 (*)
     │       │   └── (build) tauri v2.12.5 (*)
     │       ├── tauri-codegen v2.7.4 (*)
     │       ├── tauri-macros v2.7.4 (*)
     │       ├── tauri-runtime v2.12.2
     │       │   ├── tauri v2.12.5 (*)
     │       │   └── tauri-runtime-wry v2.12.3
     │       │       └── tauri v2.12.5 (*)
     │       └── tauri-runtime-wry v2.12.3 (*)
     └── tauri-codegen v2.7.4 (*)

warning[license-not-encountered]: license was not encountered
   ┌─ deny.toml:22:6
   │
22 │     "Apache-2.0 WITH LLVM-exception",
   │      ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━ unmatched license allowance

warning[duplicate]: found 2 duplicate entries for crate 'foldhash'
   ┌─ /Users/narayanwaraich/Desktop/Projects/photos-manager/src-tauri/Cargo.lock:80:1
   │  
80 │ ╭ foldhash 0.1.5 registry+https://github.com/rust-lang/crates.io-index
81 │ │ foldhash 0.2.0 registry+https://github.com/rust-lang/crates.io-index
   │ ╰────────────────────────────────────────────────────────────────────┘ lock entries
   │  
   ├ foldhash v0.1.5
     └── hashbrown v0.15.5
         ├── hashlink v0.10.0
         │   └── sqlx-core v0.8.6
         │       ├── sqlx v0.8.6
         │       │   └── photos-manager v0.1.0
         │       ├── sqlx-macros v0.8.6
         │       │   └── sqlx v0.8.6 (*)
         │       ├── sqlx-macros-core v0.8.6
         │       │   └── sqlx-macros v0.8.6 (*)
         │       └── sqlx-sqlite v0.8.6
         │           ├── sqlx v0.8.6 (*)
         │           └── sqlx-macros-core v0.8.6 (*)
         └── sqlx-core v0.8.6 (*)
   ├ foldhash v0.2.0
     └── dom_query v0.28.0
         └── tauri-utils v2.10.2
             ├── tauri v2.12.5
             │   └── photos-manager v0.1.0
             ├── tauri-build v2.7.4
             │   ├── (build) photos-manager v0.1.0 (*)
             │   └── (build) tauri v2.12.5 (*)
             ├── tauri-codegen v2.7.4
             │   └── tauri-macros v2.7.4
             │       └── tauri v2.12.5 (*)
             ├── tauri-macros v2.7.4 (*)
             ├── tauri-runtime v2.12.2
             │   ├── tauri v2.12.5 (*)
             │   └── tauri-runtime-wry v2.12.3
             │       └── tauri v2.12.5 (*)
             └── tauri-runtime-wry v2.12.3 (*)

warning[duplicate]: found 2 duplicate entries for crate 'getrandom'
   ┌─ /Users/narayanwaraich/Desktop/Projects/photos-manager/src-tauri/Cargo.lock:95:1
   │  
95 │ ╭ getrandom 0.3.4 registry+https://github.com/rust-lang/crates.io-index
96 │ │ getrandom 0.4.3 registry+https://github.com/rust-lang/crates.io-index
   │ ╰─────────────────────────────────────────────────────────────────────┘ lock entries
   │  
   ├ getrandom v0.3.4
     └── tauri v2.12.5
         └── photos-manager v0.1.0
   ├ getrandom v0.4.3
     ├── rand v0.10.4
     │   └── proptest v1.12.0
     │       └── (dev) photos-manager v0.1.0
     ├── tempfile v3.27.0
     │   ├── (dev) photos-manager v0.1.0 (*)
     │   ├── proptest v1.12.0 (*)
     │   └── rusty-fork v0.3.1
     │       └── proptest v1.12.0 (*)
     └── uuid v1.29.2
         ├── cfb v0.14.0
         │   └── infer v0.22.0
         │       └── tauri-utils v2.10.2
         │           ├── tauri v2.12.5
         │           │   └── photos-manager v0.1.0 (*)
         │           ├── tauri-build v2.7.4
         │           │   ├── (build) photos-manager v0.1.0 (*)
         │           │   └── (build) tauri v2.12.5 (*)
         │           ├── tauri-codegen v2.7.4
         │           │   └── tauri-macros v2.7.4
         │           │       └── tauri v2.12.5 (*)
         │           ├── tauri-macros v2.7.4 (*)
         │           ├── tauri-runtime v2.12.2
         │           │   ├── tauri v2.12.5 (*)
         │           │   └── tauri-runtime-wry v2.12.3
         │           │       └── tauri v2.12.5 (*)
         │           └── tauri-runtime-wry v2.12.3 (*)
         ├── photos-manager v0.1.0 (*)
         ├── schemars v0.8.22
         │   ├── tauri-build v2.7.4 (*)
         │   └── tauri-utils v2.10.2 (*)
         ├── tauri-codegen v2.7.4 (*)
         └── tauri-utils v2.10.2 (*)

warning[duplicate]: found 3 duplicate entries for crate 'hashbrown'
    ┌─ /Users/narayanwaraich/Desktop/Projects/photos-manager/src-tauri/Cargo.lock:98:1
    │  
 98 │ ╭ hashbrown 0.12.3 registry+https://github.com/rust-lang/crates.io-index
 99 │ │ hashbrown 0.15.5 registry+https://github.com/rust-lang/crates.io-index
100 │ │ hashbrown 0.17.1 registry+https://github.com/rust-lang/crates.io-index
    │ ╰──────────────────────────────────────────────────────────────────────┘ lock entries
    │  
    ├ hashbrown v0.12.3
      └── indexmap v1.9.3
          └── schemars v0.8.22
              ├── tauri-build v2.7.4
              │   ├── (build) photos-manager v0.1.0
              │   └── (build) tauri v2.12.5
              │       └── photos-manager v0.1.0 (*)
              └── tauri-utils v2.10.2
                  ├── tauri v2.12.5 (*)
                  ├── tauri-build v2.7.4 (*)
                  ├── tauri-codegen v2.7.4
                  │   └── tauri-macros v2.7.4
                  │       └── tauri v2.12.5 (*)
                  ├── tauri-macros v2.7.4 (*)
                  ├── tauri-runtime v2.12.2
                  │   ├── tauri v2.12.5 (*)
                  │   └── tauri-runtime-wry v2.12.3
                  │       └── tauri v2.12.5 (*)
                  └── tauri-runtime-wry v2.12.3 (*)
    ├ hashbrown v0.15.5
      ├── hashlink v0.10.0
      │   └── sqlx-core v0.8.6
      │       ├── sqlx v0.8.6
      │       │   └── photos-manager v0.1.0
      │       ├── sqlx-macros v0.8.6
      │       │   └── sqlx v0.8.6 (*)
      │       ├── sqlx-macros-core v0.8.6
      │       │   └── sqlx-macros v0.8.6 (*)
      │       └── sqlx-sqlite v0.8.6
      │           ├── sqlx v0.8.6 (*)
      │           └── sqlx-macros-core v0.8.6 (*)
      └── sqlx-core v0.8.6 (*)
    ├ hashbrown v0.17.1
      └── indexmap v2.14.2
          ├── plist v1.10.3
          │   ├── tauri v2.12.5
          │   │   └── photos-manager v0.1.0
          │   ├── tauri-codegen v2.7.4
          │   │   └── tauri-macros v2.7.4
          │   │       └── tauri v2.12.5 (*)
          │   └── tauri-utils v2.10.2
          │       ├── tauri v2.12.5 (*)
          │       ├── tauri-build v2.7.4
          │       │   ├── (build) photos-manager v0.1.0 (*)
          │       │   └── (build) tauri v2.12.5 (*)
          │       ├── tauri-codegen v2.7.4 (*)
          │       ├── tauri-macros v2.7.4 (*)
          │       ├── tauri-runtime v2.12.2
          │       │   ├── tauri v2.12.5 (*)
          │       │   └── tauri-runtime-wry v2.12.3
          │       │       └── tauri v2.12.5 (*)
          │       └── tauri-runtime-wry v2.12.3 (*)
          └── sqlx-core v0.8.6
              ├── sqlx v0.8.6
              │   └── photos-manager v0.1.0 (*)
              ├── sqlx-macros v0.8.6
              │   └── sqlx v0.8.6 (*)
              ├── sqlx-macros-core v0.8.6
              │   └── sqlx-macros v0.8.6 (*)
              └── sqlx-sqlite v0.8.6
                  ├── sqlx v0.8.6 (*)
                  └── sqlx-macros-core v0.8.6 (*)

warning[duplicate]: found 2 duplicate entries for crate 'indexmap'
    ┌─ /Users/narayanwaraich/Desktop/Projects/photos-manager/src-tauri/Cargo.lock:117:1
    │  
117 │ ╭ indexmap 1.9.3 registry+https://github.com/rust-lang/crates.io-index
118 │ │ indexmap 2.14.2 registry+https://github.com/rust-lang/crates.io-index
    │ ╰─────────────────────────────────────────────────────────────────────┘ lock entries
    │  
    ├ indexmap v1.9.3
      └── schemars v0.8.22
          ├── tauri-build v2.7.4
          │   ├── (build) photos-manager v0.1.0
          │   └── (build) tauri v2.12.5
          │       └── photos-manager v0.1.0 (*)
          └── tauri-utils v2.10.2
              ├── tauri v2.12.5 (*)
              ├── tauri-build v2.7.4 (*)
              ├── tauri-codegen v2.7.4
              │   └── tauri-macros v2.7.4
              │       └── tauri v2.12.5 (*)
              ├── tauri-macros v2.7.4 (*)
              ├── tauri-runtime v2.12.2
              │   ├── tauri v2.12.5 (*)
              │   └── tauri-runtime-wry v2.12.3
              │       └── tauri v2.12.5 (*)
              └── tauri-runtime-wry v2.12.3 (*)
    ├ indexmap v2.14.2
      ├── plist v1.10.3
      │   ├── tauri v2.12.5
      │   │   └── photos-manager v0.1.0
      │   ├── tauri-codegen v2.7.4
      │   │   └── tauri-macros v2.7.4
      │   │       └── tauri v2.12.5 (*)
      │   └── tauri-utils v2.10.2
      │       ├── tauri v2.12.5 (*)
      │       ├── tauri-build v2.7.4
      │       │   ├── (build) photos-manager v0.1.0 (*)
      │       │   └── (build) tauri v2.12.5 (*)
      │       ├── tauri-codegen v2.7.4 (*)
      │       ├── tauri-macros v2.7.4 (*)
      │       ├── tauri-runtime v2.12.2
      │       │   ├── tauri v2.12.5 (*)
      │       │   └── tauri-runtime-wry v2.12.3
      │       │       └── tauri v2.12.5 (*)
      │       └── tauri-runtime-wry v2.12.3 (*)
      └── sqlx-core v0.8.6
          ├── sqlx v0.8.6
          │   └── photos-manager v0.1.0 (*)
          ├── sqlx-macros v0.8.6
          │   └── sqlx v0.8.6 (*)
          ├── sqlx-macros-core v0.8.6
          │   └── sqlx-macros v0.8.6 (*)
          └── sqlx-sqlite v0.8.6
              ├── sqlx v0.8.6 (*)
              └── sqlx-macros-core v0.8.6 (*)

warning[duplicate]: found 2 duplicate entries for crate 'miniz_oxide'
    ┌─ /Users/narayanwaraich/Desktop/Projects/photos-manager/src-tauri/Cargo.lock:133:1
    │  
133 │ ╭ miniz_oxide 0.8.9 registry+https://github.com/rust-lang/crates.io-index
134 │ │ miniz_oxide 0.9.1 registry+https://github.com/rust-lang/crates.io-index
    │ ╰───────────────────────────────────────────────────────────────────────┘ lock entries
    │  
    ├ miniz_oxide v0.8.9
      └── png v0.18.1
          ├── ico v0.6.0
          │   └── tauri-codegen v2.7.4
          │       └── tauri-macros v2.7.4
          │           └── tauri v2.12.5
          │               └── photos-manager v0.1.0
          ├── muda v0.20.0
          │   └── tauri v2.12.5 (*)
          └── tauri-codegen v2.7.4 (*)
    ├ miniz_oxide v0.9.1
      └── flate2 v1.1.10
          └── png v0.18.1
              ├── ico v0.6.0
              │   └── tauri-codegen v2.7.4
              │       └── tauri-macros v2.7.4
              │           └── tauri v2.12.5
              │               └── photos-manager v0.1.0
              ├── muda v0.20.0
              │   └── tauri v2.12.5 (*)
              └── tauri-codegen v2.7.4 (*)

warning[duplicate]: found 2 duplicate entries for crate 'syn'
    ┌─ /Users/narayanwaraich/Desktop/Projects/photos-manager/src-tauri/Cargo.lock:232:1
    │  
232 │ ╭ syn 2.0.119 registry+https://github.com/rust-lang/crates.io-index
233 │ │ syn 3.0.9 registry+https://github.com/rust-lang/crates.io-index
    │ ╰───────────────────────────────────────────────────────────────┘ lock entries
    │  
    ├ syn v2.0.119
      ├── derive_more-impl v2.1.1
      │   └── derive_more v2.1.1
      │       └── selectors v0.38.0
      │           └── dom_query v0.28.0
      │               └── tauri-utils v2.10.2
      │                   ├── tauri v2.12.5
      │                   │   └── photos-manager v0.1.0
      │                   ├── tauri-build v2.7.4
      │                   │   ├── (build) photos-manager v0.1.0 (*)
      │                   │   └── (build) tauri v2.12.5 (*)
      │                   ├── tauri-codegen v2.7.4
      │                   │   └── tauri-macros v2.7.4
      │                   │       └── tauri v2.12.5 (*)
      │                   ├── tauri-macros v2.7.4 (*)
      │                   ├── tauri-runtime v2.12.2
      │                   │   ├── tauri v2.12.5 (*)
      │                   │   └── tauri-runtime-wry v2.12.3
      │                   │       └── tauri v2.12.5 (*)
      │                   └── tauri-runtime-wry v2.12.3 (*)
      ├── num-derive v0.4.2
      │   └── libvips v1.7.2
      │       └── photos-manager v0.1.0 (*)
      ├── phf_macros v0.13.1
      │   └── phf v0.13.1
      │       ├── cssparser v0.37.0
      │       │   ├── dom_query v0.28.0 (*)
      │       │   └── selectors v0.38.0 (*)
      │       ├── selectors v0.38.0 (*)
      │       ├── tauri-utils v2.10.2 (*)
      │       └── web_atoms v0.2.6
      │           └── markup5ever v0.39.0
      │               └── html5ever v0.39.0
      │                   └── dom_query v0.28.0 (*)
      ├── schemars_derive v0.8.22
      │   └── schemars v0.8.22
      │       ├── tauri-build v2.7.4 (*)
      │       └── tauri-utils v2.10.2 (*)
      ├── serde_derive_internals v0.29.1
      │   └── schemars_derive v0.8.22 (*)
      ├── serialize-to-javascript-impl v0.1.2
      │   └── serialize-to-javascript v0.1.2
      │       └── tauri v2.12.5 (*)
      ├── sqlx-macros v0.8.6
      │   └── sqlx v0.8.6
      │       └── photos-manager v0.1.0 (*)
      ├── sqlx-macros-core v0.8.6
      │   └── sqlx-macros v0.8.6 (*)
      ├── tauri-codegen v2.7.4 (*)
      ├── tauri-macros v2.7.4 (*)
      ├── tracing-attributes v0.1.31
      │   └── tracing v0.1.44
      │       ├── softbuffer v0.4.8
      │       │   └── tauri-runtime-wry v2.12.3 (*)
      │       ├── sqlx-core v0.8.6
      │       │   ├── sqlx v0.8.6 (*)
      │       │   ├── sqlx-macros v0.8.6 (*)
      │       │   ├── sqlx-macros-core v0.8.6 (*)
      │       │   └── sqlx-sqlite v0.8.6
      │       │       ├── sqlx v0.8.6 (*)
      │       │       └── sqlx-macros-core v0.8.6 (*)
      │       └── sqlx-sqlite v0.8.6 (*)
      ├── webview2-com-macros v0.8.1
      │   └── webview2-com v0.39.1
      │       ├── tauri v2.12.5 (*)
      │       ├── tauri-runtime v2.12.2 (*)
      │       ├── tauri-runtime-wry v2.12.3 (*)
      │       └── wry v0.57.1
      │           └── tauri-runtime-wry v2.12.3 (*)
      ├── windows-implement v0.60.2
      │   └── windows-core v0.62.2
      │       ├── tao v0.37.2
      │       │   └── tauri-runtime-wry v2.12.3 (*)
      │       ├── webview2-com v0.39.1 (*)
      │       ├── webview2-com-sys v0.39.1
      │       │   └── webview2-com v0.39.1 (*)
      │       ├── windows v0.62.2
      │       │   ├── tao v0.37.2 (*)
      │       │   ├── tauri v2.12.5 (*)
      │       │   ├── tauri-runtime v2.12.2 (*)
      │       │   ├── tauri-runtime-wry v2.12.3 (*)
      │       │   ├── webview2-com v0.39.1 (*)
      │       │   ├── webview2-com-sys v0.39.1 (*)
      │       │   └── wry v0.57.1 (*)
      │       ├── windows-collections v0.3.2
      │       │   └── windows v0.62.2 (*)
      │       ├── windows-future v0.3.2
      │       │   └── windows v0.62.2 (*)
      │       ├── windows-numerics v0.3.1
      │       │   └── windows v0.62.2 (*)
      │       └── wry v0.57.1 (*)
      └── windows-interface v0.59.3
          └── windows-core v0.62.2 (*)
    ├ syn v3.0.9
      ├── cssparser-macros v0.7.1
      │   └── cssparser v0.37.0
      │       ├── dom_query v0.28.0
      │       │   └── tauri-utils v2.10.2
      │       │       ├── tauri v2.12.5
      │       │       │   └── photos-manager v0.1.0
      │       │       ├── tauri-build v2.7.4
      │       │       │   ├── (build) photos-manager v0.1.0 (*)
      │       │       │   └── (build) tauri v2.12.5 (*)
      │       │       ├── tauri-codegen v2.7.4
      │       │       │   └── tauri-macros v2.7.4
      │       │       │       └── tauri v2.12.5 (*)
      │       │       ├── tauri-macros v2.7.4 (*)
      │       │       ├── tauri-runtime v2.12.2
      │       │       │   ├── tauri v2.12.5 (*)
      │       │       │   └── tauri-runtime-wry v2.12.3
      │       │       │       └── tauri v2.12.5 (*)
      │       │       └── tauri-runtime-wry v2.12.3 (*)
      │       └── selectors v0.38.0
      │           └── dom_query v0.28.0 (*)
      ├── darling_core v0.24.1
      │   ├── darling v0.24.1
      │   │   └── serde_with_macros v3.24.0
      │   │       └── serde_with v3.24.0
      │   │           └── tauri-utils v2.10.2 (*)
      │   └── darling_macro v0.24.1
      │       └── darling v0.24.1 (*)
      ├── darling_macro v0.24.1 (*)
      ├── displaydoc v0.2.7
      │   ├── icu_collections v2.4.0
      │   │   ├── icu_normalizer v2.4.0
      │   │   │   └── idna_adapter v1.2.2
      │   │   │       └── idna v1.1.0
      │   │   │           └── url v2.5.9
      │   │   │               ├── schemars v0.8.22
      │   │   │               │   ├── tauri-build v2.7.4 (*)
      │   │   │               │   └── tauri-utils v2.10.2 (*)
      │   │   │               ├── sqlx-core v0.8.6
      │   │   │               │   ├── sqlx v0.8.6
      │   │   │               │   │   └── photos-manager v0.1.0 (*)
      │   │   │               │   ├── sqlx-macros v0.8.6
      │   │   │               │   │   └── sqlx v0.8.6 (*)
      │   │   │               │   ├── sqlx-macros-core v0.8.6
      │   │   │               │   │   └── sqlx-macros v0.8.6 (*)
      │   │   │               │   └── sqlx-sqlite v0.8.6
      │   │   │               │       ├── sqlx v0.8.6 (*)
      │   │   │               │       └── sqlx-macros-core v0.8.6 (*)
      │   │   │               ├── sqlx-macros-core v0.8.6 (*)
      │   │   │               ├── sqlx-sqlite v0.8.6 (*)
      │   │   │               ├── tao v0.37.2
      │   │   │               │   └── tauri-runtime-wry v2.12.3 (*)
      │   │   │               ├── tauri v2.12.5 (*)
      │   │   │               ├── tauri-codegen v2.7.4 (*)
      │   │   │               ├── tauri-runtime v2.12.2 (*)
      │   │   │               ├── tauri-runtime-wry v2.12.3 (*)
      │   │   │               ├── tauri-utils v2.10.2 (*)
      │   │   │               ├── urlpattern v0.6.0
      │   │   │               │   └── tauri-utils v2.10.2 (*)
      │   │   │               └── wry v0.57.1
      │   │   │                   └── tauri-runtime-wry v2.12.3 (*)
      │   │   └── icu_properties v2.4.0
      │   │       ├── idna_adapter v1.2.2 (*)
      │   │       └── urlpattern v0.6.0 (*)
      │   ├── icu_locale_core v2.4.0
      │   │   ├── icu_properties v2.4.0 (*)
      │   │   └── icu_provider v2.4.0
      │   │       ├── icu_normalizer v2.4.0 (*)
      │   │       └── icu_properties v2.4.0 (*)
      │   ├── icu_properties v2.4.0 (*)
      │   ├── icu_provider v2.4.0 (*)
      │   ├── tinystr v0.8.5
      │   │   └── icu_locale_core v2.4.0 (*)
      │   └── zerotrie v0.2.5
      │       ├── icu_properties v2.4.0 (*)
      │       └── icu_provider v2.4.0 (*)
      ├── foreign-types-macros v0.2.4
      │   └── foreign-types v0.5.0
      │       └── core-graphics v0.25.0
      │           └── tao v0.37.2 (*)
      ├── serde_derive v1.0.229
      │   ├── serde v1.0.229
      │   │   ├── cargo-platform v0.1.9
      │   │   │   └── cargo_metadata v0.19.2
      │   │   │       └── tauri-utils v2.10.2 (*)
      │   │   ├── cargo_metadata v0.19.2 (*)
      │   │   ├── cargo_toml v1.0.1
      │   │   │   └── tauri-build v2.7.4 (*)
      │   │   ├── dpi v0.1.2
      │   │   │   ├── muda v0.20.0
      │   │   │   │   └── tauri v2.12.5 (*)
      │   │   │   ├── tao v0.37.2 (*)
      │   │   │   ├── tauri-runtime v2.12.2 (*)
      │   │   │   └── wry v0.57.1 (*)
      │   │   ├── either v1.19.0
      │   │   │   ├── sqlx-core v0.8.6 (*)
      │   │   │   └── sqlx-macros-core v0.8.6 (*)
      │   │   ├── indexmap v1.9.3
      │   │   │   └── schemars v0.8.22 (*)
      │   │   ├── json-patch v4.2.0
      │   │   │   ├── tauri-build v2.7.4 (*)
      │   │   │   ├── tauri-codegen v2.7.4 (*)
      │   │   │   └── tauri-utils v2.10.2 (*)
      │   │   ├── jsonptr v0.7.1
      │   │   │   └── json-patch v4.2.0 (*)
      │   │   ├── keyboard-types v0.8.3
      │   │   │   └── muda v0.20.0 (*)
      │   │   ├── muda v0.20.0 (*)
      │   │   ├── photos-manager v0.1.0 (*)
      │   │   ├── plist v1.10.3
      │   │   │   ├── tauri v2.12.5 (*)
      │   │   │   ├── tauri-codegen v2.7.4 (*)
      │   │   │   └── tauri-utils v2.10.2 (*)
      │   │   ├── schemars v0.8.22 (*)
      │   │   ├── serde_urlencoded v0.7.1
      │   │   │   └── sqlx-sqlite v0.8.6 (*)
      │   │   ├── serialize-to-javascript v0.1.2
      │   │   │   └── tauri v2.12.5 (*)
      │   │   ├── sqlx-core v0.8.6 (*)
      │   │   ├── sqlx-macros-core v0.8.6 (*)
      │   │   ├── sqlx-sqlite v0.8.6 (*)
      │   │   ├── swift-rs v1.0.9
      │   │   │   └── tauri-utils v2.10.2 (*)
      │   │   ├── tauri v2.12.5 (*)
      │   │   ├── tauri-build v2.7.4 (*)
      │   │   ├── tauri-codegen v2.7.4 (*)
      │   │   ├── tauri-runtime v2.12.2 (*)
      │   │   ├── tauri-utils v2.10.2 (*)
      │   │   ├── url v2.5.9 (*)
      │   │   └── urlpattern v0.6.0 (*)
      │   └── url v2.5.9 (*)
      ├── serde_repr v0.1.21
      │   └── tauri v2.12.5 (*)
      ├── serde_with_macros v3.24.0 (*)
      ├── synstructure v0.14.0
      │   ├── yoke-derive v0.8.4
      │   │   └── yoke v0.8.4
      │   │       ├── icu_collections v2.4.0 (*)
      │   │       ├── icu_provider v2.4.0 (*)
      │   │       ├── zerotrie v0.2.5 (*)
      │   │       └── zerovec v0.11.9
      │   │           ├── icu_collections v2.4.0 (*)
      │   │           ├── icu_locale_core v2.4.0 (*)
      │   │           ├── icu_normalizer v2.4.0 (*)
      │   │           ├── icu_properties v2.4.0 (*)
      │   │           ├── icu_provider v2.4.0 (*)
      │   │           ├── potential_utf v0.1.6
      │   │           │   └── icu_collections v2.4.0 (*)
      │   │           └── tinystr v0.8.5 (*)
      │   └── zerofrom-derive v0.1.8
      │       └── zerofrom v0.1.8
      │           ├── icu_collections v2.4.0 (*)
      │           ├── icu_provider v2.4.0 (*)
      │           ├── yoke v0.8.4 (*)
      │           ├── zerotrie v0.2.5 (*)
      │           └── zerovec v0.11.9 (*)
      ├── thiserror-impl v2.0.21
      │   └── thiserror v2.0.21
      │       ├── cargo_metadata v0.19.2 (*)
      │       ├── json-patch v4.2.0 (*)
      │       ├── muda v0.20.0 (*)
      │       ├── photos-manager v0.1.0 (*)
      │       ├── sqlx-core v0.8.6 (*)
      │       ├── sqlx-sqlite v0.8.6 (*)
      │       ├── tauri v2.12.5 (*)
      │       ├── tauri-codegen v2.7.4 (*)
      │       ├── tauri-runtime v2.12.2 (*)
      │       ├── tauri-utils v2.10.2 (*)
      │       ├── (build) webview2-com-sys v0.39.1
      │       │   └── webview2-com v0.39.1
      │       │       ├── tauri v2.12.5 (*)
      │       │       ├── tauri-runtime v2.12.2 (*)
      │       │       ├── tauri-runtime-wry v2.12.3 (*)
      │       │       └── wry v0.57.1 (*)
      │       └── wry v0.57.1 (*)
      ├── tokio-macros v2.7.2
      │   └── tokio v1.54.1
      │       ├── photos-manager v0.1.0 (*)
      │       ├── sqlx-core v0.8.6 (*)
      │       ├── sqlx-macros-core v0.8.6 (*)
      │       ├── tauri v2.12.5 (*)
      │       └── tokio-stream v0.1.19
      │           └── sqlx-core v0.8.6 (*)
      ├── yoke-derive v0.8.4 (*)
      ├── zerofrom-derive v0.1.8 (*)
      └── zerovec-derive v0.11.7
          └── zerovec v0.11.9 (*)

advisories ok, bans ok, licenses ok, sources ok
```

### 5.4 CI Runs (`gh run list`, `gh run view 46034567663`, job conclusions, and run logs)

Raw output of `gh run list --branch task/1G --limit 3`:

```text
Working...
STATUS  TITLE        WORKFLOW    BRANCH   EVENT  ID          ELAPSED  AGE       
✓       chore(1G...  CI & Pa...  task/1G  push   4603456...  6m51s    about 7...
X       feat(1G)...  CI & Pa...  task/1G  push   4592962...  8m34s    about 1...
✓       feat(1G)...  CI & Pa...  task/1G  push   4592850...  7m24s    about 1...
```

Raw output of `gh run list --branch task/1G --limit 6`:

```text
Working...
STATUS  TITLE        WORKFLOW    BRANCH   EVENT  ID          ELAPSED  AGE       
✓       chore(1G...  CI & Pa...  task/1G  push   4603456...  6m51s    about 7...
X       feat(1G)...  CI & Pa...  task/1G  push   4592962...  8m34s    about 1...
✓       feat(1G)...  CI & Pa...  task/1G  push   4592850...  7m24s    about 1...
X       feat(1G)...  CI & Pa...  task/1G  push   4592806...  4m52s    about 1...
```

Raw output of `gh run view 46034567663`:

```text
✓ task/1G CI & Packaging Matrix · 46034567663
Triggered via push about 7 minutes ago

JOBS
✓ Security & License check (cargo deny) in 52s (ID 140626596979)
✓ macos-arm64 in 6m47s (ID 140626597449)
✓ windows-x64 in 6m44s (ID 140626597727)

ANNOTATIONS
! Node.js 20 is deprecated. The following actions target Node.js 20 but are being forced to run on Node.js 24: actions/checkout@v4. For more information see: https://github.blog/changelog/2025-09-19-deprecation-of-node-20-on-github-actions-runners/
Security & License check (cargo deny): .github#2

! Node.js 20 is deprecated. The following actions target Node.js 20 but are being forced to run on Node.js 24: actions/checkout@v4, actions/setup-node@v4, actions/setup-python@v5, actions/upload-artifact@v4, pnpm/action-setup@v4. For more information see: https://github.blog/changelog/2025-09-19-deprecation-of-node-20-on-github-actions-runners/
windows-x64: .github#3

! Node.js 20 is deprecated. The following actions target Node.js 20 but are being forced to run on Node.js 24: actions/checkout@v4, actions/setup-node@v4, actions/setup-python@v5, actions/upload-artifact@v4, pnpm/action-setup@v4. For more information see: https://github.blog/changelog/2025-09-19-deprecation-of-node-20-on-github-actions-runners/
macos-arm64: .github#2

- Due to capacity constraints, jobs targeting macOS arm64 runners may experience longer queue times.
macos-arm64: .github#1

ARTIFACTS
verify-logs-macos-arm64
photos-manager-macos-arm64
smoke-report-macos-arm64
smoke-report-windows-x64
verify-logs-windows-x64
photos-manager-windows-x64

For more information about a job, try: gh run view --job=<job-id>
View this run on GitHub: https://github.com/narayanwaraich/photos-manager/actions/runs/46034567663
```

Job conclusion lines from `gh run view 46034567663 --json jobs --jq '.jobs[] | "\(.name) (ID \(.databaseId)): conclusion=\(.conclusion)"'`:

```text
Security & License check (cargo deny) (ID 140626596979): conclusion=success
macos-arm64 (ID 140626597449): conclusion=success
windows-x64 (ID 140626597727): conclusion=success
```

Extracted verification lines from CI run `46034567663` jobs (`140626596979`, `140626597727`, `140626597449`):

```text
Security & License check (cargo deny)	Security & License policy check	2026-11-27T16:33:49.1834339Z warning[license-not-encountered]: license was not encountered
Security & License check (cargo deny)	Security & License policy check	2026-11-27T16:33:49.4330840Z advisories ok, bans ok, licenses ok, sources ok
---
windows-x64	Run verification suite (pnpm verify)	2026-11-27T16:36:38.5020396Z running 202 tests
windows-x64	Run verification suite (pnpm verify)	2026-11-27T16:36:38.5095880Z test metadata::tests::test_child_killed_after_timeout_windows ... ok
windows-x64	Run verification suite (pnpm verify)	2026-11-27T16:36:38.5123546Z test watcher::tests::real_watcher_detects_new_file_smoke ... ok
windows-x64	Run verification suite (pnpm verify)	2026-11-27T16:36:38.5123938Z test result: ok. 200 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 30.90s
windows-x64	Run verification suite (pnpm verify)	2026-11-27T16:36:38.5124207Z running 0 tests
windows-x64	Run verification suite (pnpm verify)	2026-11-27T16:36:38.5124459Z test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
windows-x64	Run verification suite (pnpm verify)	2026-11-27T16:36:38.5124709Z running 0 tests
windows-x64	Run verification suite (pnpm verify)	2026-11-27T16:36:38.5124959Z test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
windows-x64	Run verification suite (pnpm verify)	2026-11-27T16:36:55.8554470Z AGENTS.md character count: 7510
windows-x64	Run verification suite (pnpm verify)	2026-11-27T16:36:55.8555692Z ALL VERIFICATION CHECKS PASSED SUCCESSFULLY!
---
macos-arm64	Run verification suite (pnpm verify)	2026-11-27T16:35:31.4900280Z running 206 tests
macos-arm64	Run verification suite (pnpm verify)	2026-11-27T16:35:31.5500870Z test watcher::tests::real_watcher_detects_new_file_smoke ... ok
macos-arm64	Run verification suite (pnpm verify)	2026-11-27T16:35:31.5504430Z test result: ok. 204 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 7.61s
macos-arm64	Run verification suite (pnpm verify)	2026-11-27T16:35:31.5510070Z running 0 tests
macos-arm64	Run verification suite (pnpm verify)	2026-11-27T16:35:31.5520970Z test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
macos-arm64	Run verification suite (pnpm verify)	2026-11-27T16:35:31.5524180Z running 0 tests
macos-arm64	Run verification suite (pnpm verify)	2026-11-27T16:35:31.5525800Z test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
macos-arm64	Run verification suite (pnpm verify)	2026-11-27T16:35:37.0126160Z AGENTS.md character count: 7439
macos-arm64	Run verification suite (pnpm verify)	2026-11-27T16:35:37.0131690Z ALL VERIFICATION CHECKS PASSED SUCCESSFULLY!
```

Historical round-1 failed `cargo deny` error from CI run `45929623693` (commit `b7de6b5`, job `140278976016`, prior to `eab2e4e`):

```text
error[rejected]: failed to satisfy license requirements
   ┌─ /usr/local/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/xxhash-rust-0.8.21/Cargo.toml:42:12
42 │ license = "BSL-1.0"
   │            ━━━━━━━
   │            │
   │            rejected: license is not explicitly allowed
   ├ BSL-1.0 - Boost Software License 1.0:
   ├   - OSI approved
   ├   - FSF Free/Libre
   ├ xxhash-rust v0.8.21
     └── notify v9.0.0
advisories ok, bans ok, licenses FAILED, sources ok
```

### 5.5 Specific Theme C & Theme B verification commands (round-1 summary, NOT raw output)

*(Note: carried over from round-1 commits `e36bc72` and `fa17884`; no source files changed in round 2.)*

- **Node `DEP0190` check (`node v26.9.0`, round-1 summary, NOT raw output):** `spawnSync("echo", ["hi"], { shell: true })` printed `[DEP0190] DeprecationWarning: Passing args to a child process with shell option true`; `spawnSync("echo hi", { shell: true })` produced no warning.
- **ExifTool `-api WindowsLongPath=1` on macOS (round-1 summary, NOT raw output):** Run on `tests/fixtures/generated/image_orientation_6.jpg` with and without `-api WindowsLongPath=1` yielded identical JSON output (`ImageWidth: 640`, `ImageHeight: 480`, `Orientation: 6`, `Make: "PhotoCorp"`, `Model: "CameraPro 1"`, `DateTimeOriginal: "2026:06:15 14:30:00"`).

### 5.6 Mutation tests (M1–M7, round-1 condensed summary, NOT raw output)

*(Note: M2–M7 were run in round 1 on `fa17884` and `b7de6b5` and reverted; no source files changed in round 2.)*

| Mutation | Temporary change (all reverted after test) | Observed failing test & message (condensed summary, NOT evidence) |
|---|---|---|
| **M1 (C)** | N/A | None per TASK-1G decision d (manual desktop check + code inspection). |
| **M2 (B)** | Removed `"-m"` from `exiftool_args_for_os` | `test metadata::tests::exiftool_args_are_pinned` failed at `src/metadata/mod.rs:512:9` (`left` missing `"-m"` vs `right`). |
| **M3 (B)** | Sequential `write_stdin.await?; read_stdout.await?; drain_stderr.await?;` | `test metadata::tests::stderr_flood_does_not_block_or_masquerade_as_timeout` failed at `src/metadata/mod.rs:582:14`: `stderr flood must not block or time out: Timeout(5s)` (5.09 s). |
| **M4 (B)** | Removed `.kill_on_drop(true)` and `child.kill().await; child.wait().await` | `test metadata::tests::test_child_killed_after_timeout` failed on Mac at `src/metadata/mod.rs:1009:9`: `Child process 80589 must no longer exist after timeout and kill / left: 0 / right: -1` (PID 80589 cleaned up manually). **The Windows test `test_child_killed_after_timeout_windows` was only seen passing in CI (`45928503558`, `45929623693`, `46034567663`); the owner demonstrates it failing on the laptop.** |
| **M5 (A)** | Ignored `auto_scan_enabled` in `decide_next_auto_scan` | `test watcher::tests::scheduler_does_nothing_when_auto_scan_is_off` failed at `src/watcher/tests.rs:521:5` (`left: Some(AutoScanCandidate)` vs `right: None`). |
| **M6 (A)** | Wrote `database::roots::set_root_status(..., RootStatus::Paused, 9999)` on `WatchEvent::Changed` | `test watcher::tests::watcher_event_writes_no_catalogue_rows` failed at `src/watcher/tests.rs:486:5` (`status: "active"` vs `"paused"`). |
| **M7 (A)** | Removed `last_scan_ended_at_ms` 60 s gap filter in `decide_next_auto_scan` | `test watcher::tests::scheduler_respects_the_60s_gap` failed at `src/watcher/tests.rs:591:5` (`left: Some(AutoScanCandidate)` vs `right: None`). |

### 5.7 Code-hygiene greps (final code tree `eab2e4e`)

This report was written with `grep` because `rg` was restricted.

Full raw output of all 6 hygiene grep commands:

```text
$ grep -rnE "unwrap\(\)|expect\(" src-tauri/src/watcher src-tauri/src/metadata src-tauri/src/library src-tauri/src/jobs src-tauri/src/commands/scan.rs
src-tauri/src/watcher/tests.rs:56:        self.active.lock().unwrap().contains_key(root_id)
src-tauri/src/watcher/tests.rs:63:            .unwrap()
src-tauri/src/watcher/tests.rs:74:            .unwrap()
src-tauri/src/watcher/tests.rs:83:            .unwrap()
src-tauri/src/watcher/tests.rs:97:        self.starts.lock().unwrap().push(root_id.clone());
src-tauri/src/watcher/tests.rs:100:            .unwrap()
src-tauri/src/watcher/tests.rs:106:        self.stops.lock().unwrap().push(root_id.clone());
src-tauri/src/watcher/tests.rs:107:        self.active.lock().unwrap().remove(root_id);
src-tauri/src/watcher/tests.rs:331:    let temp = tempdir().expect("tempdir");
src-tauri/src/watcher/tests.rs:334:    std::fs::create_dir_all(&root_dir).unwrap();
src-tauri/src/watcher/tests.rs:336:    let pool = init_pool(&db_path).await.unwrap();
src-tauri/src/watcher/tests.rs:337:    let mut conn = pool.acquire().await.unwrap();
src-tauri/src/watcher/tests.rs:353:    .unwrap();
src-tauri/src/watcher/tests.rs:374:    .unwrap();
src-tauri/src/watcher/tests.rs:393:    .unwrap();
src-tauri/src/watcher/tests.rs:418:    .unwrap();
src-tauri/src/watcher/tests.rs:433:    .unwrap();
src-tauri/src/watcher/tests.rs:438:        .unwrap();
src-tauri/src/watcher/tests.rs:442:        .unwrap();
src-tauri/src/watcher/tests.rs:446:        .unwrap();
src-tauri/src/watcher/tests.rs:450:        .unwrap();
src-tauri/src/watcher/tests.rs:467:        .unwrap();
src-tauri/src/watcher/tests.rs:471:        .unwrap();
src-tauri/src/watcher/tests.rs:475:        .unwrap();
src-tauri/src/watcher/tests.rs:479:        .unwrap();
src-tauri/src/watcher/tests.rs:606:    let decision = decide_next_auto_scan(true, false, &ready, now_inst, now_ms).unwrap();
src-tauri/src/watcher/tests.rs:652:    let decision = decide_next_auto_scan(true, false, &roots, now_inst, now_ms).unwrap();
src-tauri/src/watcher/tests.rs:687:    let decision = decide_next_auto_scan(true, false, &stale, now_inst, now_ms).unwrap();
src-tauri/src/watcher/tests.rs:694:    let temp = tempdir().expect("tempdir");
src-tauri/src/watcher/tests.rs:697:    std::fs::create_dir_all(&app_data).unwrap();
src-tauri/src/watcher/tests.rs:698:    std::fs::create_dir_all(&root_dir).unwrap();
src-tauri/src/watcher/tests.rs:700:    let pool = init_pool(&app_data.join("cat.db")).await.unwrap();
src-tauri/src/watcher/tests.rs:719:    .unwrap();
src-tauri/src/watcher/tests.rs:720:    let root_id = RootId::from(add_res.root_id.unwrap());
src-tauri/src/watcher/tests.rs:724:    std::fs::remove_dir_all(&root_dir).unwrap();
src-tauri/src/watcher/tests.rs:727:        .unwrap();
src-tauri/src/watcher/tests.rs:733:    std::fs::create_dir_all(&root_dir).unwrap();
src-tauri/src/watcher/tests.rs:736:        .unwrap();
src-tauri/src/watcher/tests.rs:743:        let mut conn = pool.acquire().await.unwrap();
src-tauri/src/watcher/tests.rs:746:            .unwrap();
src-tauri/src/watcher/tests.rs:750:        .unwrap();
src-tauri/src/watcher/tests.rs:757:    std::fs::create_dir_all(&new_dir).unwrap();
src-tauri/src/watcher/tests.rs:759:        let mut conn = pool.acquire().await.unwrap();
src-tauri/src/watcher/tests.rs:762:            .unwrap();
src-tauri/src/watcher/tests.rs:775:    .unwrap();
src-tauri/src/watcher/tests.rs:785:        .unwrap();
src-tauri/src/watcher/tests.rs:793:    let temp = tempdir().expect("tempdir");
src-tauri/src/watcher/tests.rs:796:    std::fs::create_dir_all(&app_data).unwrap();
src-tauri/src/watcher/tests.rs:797:    std::fs::create_dir_all(&root_dir).unwrap();
src-tauri/src/watcher/tests.rs:799:    let pool = init_pool(&app_data.join("cat.db")).await.unwrap();
src-tauri/src/watcher/tests.rs:824:    .expect("watcher start failure must never fail add_root");
src-tauri/src/watcher/tests.rs:827:    let root_id = RootId::from(res.root_id.unwrap());
src-tauri/src/watcher/tests.rs:835:        .unwrap();
src-tauri/src/watcher/tests.rs:843:    let temp = tempdir().expect("tempdir");
src-tauri/src/watcher/tests.rs:846:    std::fs::create_dir_all(&app_data).unwrap();
src-tauri/src/watcher/tests.rs:847:    std::fs::create_dir_all(&root_dir).unwrap();
src-tauri/src/watcher/tests.rs:848:    std::fs::write(root_dir.join("a.jpg"), b"img").unwrap();
src-tauri/src/watcher/tests.rs:850:    let pool = init_pool(&app_data.join("cat.db")).await.unwrap();
src-tauri/src/watcher/tests.rs:869:    .unwrap();
src-tauri/src/watcher/tests.rs:870:    let root_id = RootId::from(add_res.root_id.unwrap());
src-tauri/src/watcher/tests.rs:890:                if let Some(tx) = done_tx.lock().unwrap().take() {
src-tauri/src/watcher/tests.rs:896:        .unwrap();
src-tauri/src/watcher/tests.rs:907:    let mut conn = pool.acquire().await.unwrap();
src-tauri/src/watcher/tests.rs:910:        .unwrap()
src-tauri/src/watcher/tests.rs:911:        .unwrap();
src-tauri/src/watcher/tests.rs:912:    let payload: serde_json::Value = serde_json::from_str(&job_row.1).unwrap();
src-tauri/src/watcher/tests.rs:917:        .unwrap()
src-tauri/src/watcher/tests.rs:918:        .unwrap();
src-tauri/src/watcher/tests.rs:924:    let temp = tempdir().expect("tempdir");
src-tauri/src/watcher/tests.rs:925:    let pool = init_pool(&temp.path().join("settings.db")).await.unwrap();
src-tauri/src/watcher/tests.rs:928:    assert!(!get_auto_scan_impl(&pool).await.unwrap());
src-tauri/src/watcher/tests.rs:931:    assert!(set_auto_scan_impl(&pool, true).await.unwrap());
src-tauri/src/watcher/tests.rs:932:    assert!(get_auto_scan_impl(&pool).await.unwrap());
src-tauri/src/watcher/tests.rs:935:    assert!(!set_auto_scan_impl(&pool, false).await.unwrap());
src-tauri/src/watcher/tests.rs:936:    assert!(!get_auto_scan_impl(&pool).await.unwrap());
src-tauri/src/watcher/tests.rs:941:    let temp = tempdir().expect("tempdir");
src-tauri/src/watcher/tests.rs:943:    std::fs::create_dir_all(&root_dir).unwrap();
src-tauri/src/watcher/tests.rs:944:    let canon_root = std::fs::canonicalize(&root_dir).unwrap();
src-tauri/src/watcher/tests.rs:956:    std::fs::write(canon_root.join("smoke_photo.jpg"), b"jpeg-bytes").unwrap();
src-tauri/src/metadata/mod.rs:475:            .expect("workspace root")
src-tauri/src/metadata/mod.rs:480:            .expect("canonicalize fixtures dir")
src-tauri/src/metadata/mod.rs:488:        std::fs::write(&src_path, source).expect("write fake rs source");
src-tauri/src/metadata/mod.rs:500:            .expect("invoke rustc for test fake");
src-tauri/src/metadata/mod.rs:556:        let temp_dir = tempdir().expect("tempdir");
src-tauri/src/metadata/mod.rs:567:        err.write_all(&block).expect("write stderr");
src-tauri/src/metadata/mod.rs:569:    err.flush().expect("flush stderr");
src-tauri/src/metadata/mod.rs:575:        std::fs::write(&dummy_input, b"dummy").expect("write dummy");
src-tauri/src/metadata/mod.rs:581:            .expect("stderr flood must not block or time out");
src-tauri/src/metadata/mod.rs:593:        let temp_dir = tempdir().expect("tempdir");
src-tauri/src/metadata/mod.rs:597:            .expect("write script");
src-tauri/src/metadata/mod.rs:599:            .expect("metadata")
src-tauri/src/metadata/mod.rs:602:        std::fs::set_permissions(&script_path, perms).expect("set_permissions");
src-tauri/src/metadata/mod.rs:605:        std::fs::write(&dummy_input, b"dummy").expect("write dummy");
src-tauri/src/metadata/mod.rs:627:        let temp_dir = tempdir().expect("tempdir");
src-tauri/src/metadata/mod.rs:629:        std::fs::create_dir_all(&secret_dir).expect("create secret dir");
src-tauri/src/metadata/mod.rs:631:        std::fs::write(&secret_file, b"dummy").expect("write secret file");
src-tauri/src/metadata/mod.rs:671:    writeln!(err, "SECRET_STDERR_TOKEN_ABC /Users/alice/secret_album_dir_999/private.jpg").unwrap();
src-tauri/src/metadata/mod.rs:693:        let temp_dir = tempdir().expect("tempdir");
src-tauri/src/metadata/mod.rs:695:        std::fs::create_dir_all(&state_dir).expect("create state dir");
src-tauri/src/metadata/mod.rs:713:    std::io::stdin().read_to_string(&mut stdin_buf).expect("read stdin");
src-tauri/src/metadata/mod.rs:714:    let input_path = stdin_buf.lines().next().expect("stdin path").trim();
src-tauri/src/metadata/mod.rs:718:        .expect("stem")
src-tauri/src/metadata/mod.rs:738:    std::fs::write(&cur_file, cur.to_string()).expect("write cur");
src-tauri/src/metadata/mod.rs:739:    std::fs::write(&max_file, cur.max(prev_max).to_string()).expect("write max");
src-tauri/src/metadata/mod.rs:740:    std::fs::write(&active_marker, "1").expect("write active");
src-tauri/src/metadata/mod.rs:754:    std::fs::write(&cur_file, cur.to_string()).expect("write cur dec");
src-tauri/src/metadata/mod.rs:767:                std::fs::write(&p, b"dummy").expect("write input");
src-tauri/src/metadata/mod.rs:804:            .expect("one task must still be waiting");
src-tauri/src/metadata/mod.rs:819:        .expect("release first task");
src-tauri/src/metadata/mod.rs:838:        .expect("release second task");
src-tauri/src/metadata/mod.rs:840:            .expect("release third task");
src-tauri/src/metadata/mod.rs:845:                .expect("task join")
src-tauri/src/metadata/mod.rs:846:                .expect("extract_metadata must succeed");
src-tauri/src/metadata/mod.rs:852:            .expect("read max.txt")
src-tauri/src/metadata/mod.rs:855:            .expect("parse max");
src-tauri/src/metadata/mod.rs:865:        let adapter = ExifToolAdapter::new().expect("ExifTool must be available for testing");
src-tauri/src/metadata/mod.rs:872:            .expect("Metadata extraction must succeed");
src-tauri/src/metadata/mod.rs:885:        let adapter = ExifToolAdapter::new().expect("ExifTool must be available");
src-tauri/src/metadata/mod.rs:901:        let adapter = ExifToolAdapter::new().expect("ExifTool must be available");
src-tauri/src/metadata/mod.rs:902:        let temp_dir = tempdir().expect("tempdir");
src-tauri/src/metadata/mod.rs:918:        let adapter = ExifToolAdapter::new().expect("ExifTool must be available");
src-tauri/src/metadata/mod.rs:946:        let adapter = ExifToolAdapter::new().expect("ExifTool must be available");
src-tauri/src/metadata/mod.rs:968:        let temp_dir = tempdir().expect("tempdir");
src-tauri/src/metadata/mod.rs:978:        std::fs::write(&script_path, script_content).expect("Failed to write script");
src-tauri/src/metadata/mod.rs:980:            .expect("metadata")
src-tauri/src/metadata/mod.rs:983:        std::fs::set_permissions(&script_path, perms).expect("set_permissions");
src-tauri/src/metadata/mod.rs:987:        std::fs::write(&dummy_input, b"dummy content").expect("write dummy");
src-tauri/src/metadata/mod.rs:1002:        let pid_str = std::fs::read_to_string(&pid_file).expect("read pid file");
src-tauri/src/metadata/mod.rs:1003:        let pid: i32 = pid_str.trim().parse().expect("valid pid integer");
src-tauri/src/metadata/mod.rs:1027:        let temp_dir = tempdir().expect("tempdir");
src-tauri/src/metadata/mod.rs:1036:    std::fs::write("{pid_file_escaped}", std::process::id().to_string()).expect("write pid");
src-tauri/src/metadata/mod.rs:1046:        std::fs::write(&dummy_input, b"dummy content").expect("write dummy");
src-tauri/src/metadata/mod.rs:1059:            .expect("pid lock")
src-tauri/src/metadata/mod.rs:1060:            .expect("hook must capture spawned child PID");
src-tauri/src/metadata/mod.rs:1062:        let pid_str = std::fs::read_to_string(&pid_file).expect("read pid file");
src-tauri/src/metadata/mod.rs:1063:        let file_pid: u32 = pid_str.trim().parse().expect("valid pid integer");
src-tauri/src/metadata/mod.rs:1072:                .expect("run tasklist");
src-tauri/src/metadata/mod.rs:1089:        let adapter = ExifToolAdapter::new().expect("ExifTool must be available");
src-tauri/src/metadata/mod.rs:1094:        let temp_dir = tempdir().expect("tempdir");
src-tauri/src/metadata/mod.rs:1098:        std::fs::copy(&src_file, &target_file).expect("copy fixture to non-ascii path");
src-tauri/src/metadata/mod.rs:1103:            .expect("Metadata extraction for non-ASCII path should succeed");
src-tauri/src/metadata/mod.rs:1112:        let adapter = ExifToolAdapter::new().expect("ExifTool must be available");
src-tauri/src/metadata/mod.rs:1117:        let temp_dir = tempdir().expect("tempdir");
src-tauri/src/metadata/mod.rs:1123:        std::fs::create_dir_all(&deep_dir).expect("create deep dirs");
src-tauri/src/metadata/mod.rs:1125:        std::fs::copy(&src_file, &long_file).expect("copy to long path");
src-tauri/src/metadata/mod.rs:1138:            .expect("Metadata extraction for long path (>260 chars) should succeed");
src-tauri/src/library/protected.rs:602:        let temp = tempdir().expect("tempdir");
src-tauri/src/library/protected.rs:604:        std::fs::create_dir_all(&app_data).expect("create app data");
src-tauri/src/library/roots.rs:655:        let temp = tempdir().expect("tempdir");
src-tauri/src/library/roots.rs:657:        let pool = init_pool(&db_path).await.expect("init_pool");
src-tauri/src/library/roots.rs:660:        std::fs::create_dir_all(&photos_dir).expect("create dir");
src-tauri/src/library/roots.rs:665:            .expect("add_root");
src-tauri/src/library/roots.rs:667:        let roots = list_roots(&pool).await.expect("list_roots");
src-tauri/src/library/roots.rs:678:        let temp = tempdir().expect("tempdir");
src-tauri/src/library/roots.rs:680:        let pool = init_pool(&db_path).await.expect("init_pool");
src-tauri/src/library/roots.rs:684:        std::fs::create_dir_all(&dir1).expect("create dir1");
src-tauri/src/library/roots.rs:685:        std::fs::create_dir_all(&dir2).expect("create dir2");
src-tauri/src/library/roots.rs:700:            .expect("add root 1");
src-tauri/src/library/roots.rs:703:            .expect("add root 2");
src-tauri/src/library/roots.rs:710:        let mut conn = pool.acquire().await.unwrap();
src-tauri/src/library/roots.rs:711:        let volumes = database::volumes::list_volumes(&mut *conn).await.unwrap();
src-tauri/src/library/roots.rs:717:        let temp = tempdir().expect("tempdir");
src-tauri/src/library/roots.rs:719:        let pool = init_pool(&db_path).await.expect("init_pool");
src-tauri/src/library/roots.rs:723:        std::fs::create_dir_all(&dir1).expect("create dir1");
src-tauri/src/library/roots.rs:724:        std::fs::create_dir_all(&dir2).expect("create dir2");
src-tauri/src/library/roots.rs:748:            .expect("add 1");
src-tauri/src/library/roots.rs:751:            .expect("add 2");
src-tauri/src/library/roots.rs:755:        let mut conn = pool.acquire().await.unwrap();
src-tauri/src/library/roots.rs:756:        let volumes = database::volumes::list_volumes(&mut *conn).await.unwrap();
src-tauri/src/library/roots.rs:762:        let temp = tempdir().expect("tempdir");
src-tauri/src/library/roots.rs:764:        let pool = init_pool(&db_path).await.expect("init_pool");
src-tauri/src/library/roots.rs:768:        std::fs::create_dir_all(&dir1).expect("create dir1");
src-tauri/src/library/roots.rs:769:        std::fs::create_dir_all(&dir2).expect("create dir2");
src-tauri/src/library/roots.rs:785:            .expect("add 1");
src-tauri/src/library/roots.rs:788:            .expect("add 2");
src-tauri/src/library/roots.rs:792:        let mut conn = pool.acquire().await.unwrap();
src-tauri/src/library/roots.rs:793:        let volumes = database::volumes::list_volumes(&mut *conn).await.unwrap();
src-tauri/src/library/roots.rs:803:        let temp = tempdir().expect("tempdir");
src-tauri/src/library/roots.rs:805:        let pool = init_pool(&db_path).await.expect("init_pool");
src-tauri/src/library/roots.rs:808:        std::fs::create_dir_all(&dir).expect("create dir");
src-tauri/src/library/roots.rs:813:            .expect("initial root");
src-tauri/src/library/roots.rs:820:        let roots = list_roots(&pool).await.expect("list_roots");
src-tauri/src/library/roots.rs:832:        std::fs::create_dir_all(&sub_dir).expect("create sub dir");
src-tauri/src/library/roots.rs:837:        let roots_after = list_roots(&pool).await.expect("list_roots");
src-tauri/src/library/roots.rs:843:        let temp = tempdir().expect("tempdir");
src-tauri/src/library/roots.rs:845:        let pool = init_pool(&db_path).await.expect("init_pool");
src-tauri/src/library/roots.rs:848:        std::fs::create_dir_all(&dir).expect("create dir");
src-tauri/src/library/roots.rs:858:            .expect("create trigger");
src-tauri/src/library/roots.rs:864:        let mut conn = pool.acquire().await.unwrap();
src-tauri/src/library/roots.rs:865:        let roots = database::roots::list_roots(&mut *conn).await.unwrap();
src-tauri/src/library/roots.rs:867:        let volumes = database::volumes::list_volumes(&mut *conn).await.unwrap();
src-tauri/src/library/roots.rs:872:            .unwrap();
src-tauri/src/library/roots.rs:878:        let temp = tempdir().expect("tempdir");
src-tauri/src/library/roots.rs:880:        let pool = init_pool(&db_path).await.expect("init_pool");
src-tauri/src/library/roots.rs:883:        std::fs::create_dir_all(&dir).expect("create dir");
src-tauri/src/library/roots.rs:886:        let outcome = add_root(&pool, &ctx, &dir, None).await.expect("add root");
src-tauri/src/library/roots.rs:889:        let mut conn = pool.acquire().await.unwrap();
src-tauri/src/library/roots.rs:895:                .unwrap();
src-tauri/src/library/roots.rs:907:        .unwrap();
src-tauri/src/library/roots.rs:928:            .unwrap();
src-tauri/src/library/roots.rs:931:        let roots = list_roots(&pool).await.expect("list_roots");
src-tauri/src/library/roots.rs:939:        let temp = tempdir().expect("tempdir");
src-tauri/src/library/roots.rs:941:        let pool = init_pool(&db_path).await.expect("init_pool");
src-tauri/src/library/roots.rs:944:        std::fs::create_dir_all(&dir).expect("create dir");
src-tauri/src/library/roots.rs:947:        let _ = add_root(&pool, &ctx, &dir, None).await.expect("add root");
src-tauri/src/library/roots.rs:949:        let roots_before = list_roots(&pool).await.expect("list_roots");
src-tauri/src/library/roots.rs:954:        std::fs::remove_dir(&dir).expect("remove dir");
src-tauri/src/library/roots.rs:956:        let roots_after = list_roots(&pool).await.expect("list_roots");
src-tauri/src/library/roots.rs:970:        let temp = tempdir().expect("tempdir");
src-tauri/src/library/roots.rs:972:        let pool = init_pool(&db_path).await.expect("init_pool");
src-tauri/src/library/roots.rs:976:        std::fs::create_dir_all(&dir1).expect("create dir1");
src-tauri/src/library/roots.rs:977:        std::fs::create_dir_all(&dir2).expect("create dir2");
src-tauri/src/library/roots.rs:982:        std::fs::write(&file1_disk, file1_bytes).expect("write file");
src-tauri/src/library/roots.rs:987:            .expect("add root 1");
src-tauri/src/library/roots.rs:990:            .expect("add root 2");
src-tauri/src/library/roots.rs:993:        let mut conn = pool.acquire().await.unwrap();
src-tauri/src/library/roots.rs:998:            .unwrap();
src-tauri/src/library/roots.rs:1010:        .unwrap();
src-tauri/src/library/roots.rs:1015:            .expect("preview");
src-tauri/src/library/roots.rs:1031:            .expect("remove_root");
src-tauri/src/library/roots.rs:1034:        let roots = list_roots(&pool).await.expect("list_roots");
src-tauri/src/library/roots.rs:1040:        let read_back = std::fs::read(&file1_disk).expect("read file");
src-tauri/src/library/roots.rs:1054:        let temp = tempdir().expect("tempdir");
src-tauri/src/library/roots.rs:1056:        let pool = init_pool(&db_path).await.expect("init_pool");
src-tauri/src/library/roots.rs:1059:        std::fs::create_dir_all(&dir).expect("create dir");
src-tauri/src/library/roots.rs:1082:        let temp = tempdir().expect("tempdir");
src-tauri/src/library/roots.rs:1084:        let pool = init_pool(&db_path).await.expect("init_pool");
src-tauri/src/library/roots.rs:1087:        std::fs::create_dir_all(&dir).expect("create dir");
src-tauri/src/library/roots.rs:1096:                .expect("holder begin");
src-tauri/src/library/roots.rs:1098:            tx.commit().await.expect("holder commit");
src-tauri/src/library/roots.rs:1107:            .expect("add_root should wait and succeed");
src-tauri/src/library/roots.rs:1110:        holder.await.expect("holder task");
src-tauri/src/library/roots.rs:1118:        let roots = list_roots(&pool).await.expect("list_roots");
src-tauri/src/library/roots.rs:1125:        let temp = tempdir().expect("tempdir");
src-tauri/src/library/roots.rs:1127:        let pool = init_pool(&db_path).await.expect("init_pool");
src-tauri/src/library/roots.rs:1130:        std::fs::create_dir_all(&dir).expect("create dir");
src-tauri/src/library/roots.rs:1132:        let outcome = add_root(&pool, &ctx, &dir, None).await.expect("add_root");
src-tauri/src/library/roots.rs:1134:        let mut conn = pool.acquire().await.unwrap();
src-tauri/src/library/roots.rs:1137:            .unwrap()
src-tauri/src/library/roots.rs:1138:            .unwrap();
src-tauri/src/library/roots.rs:1149:        .unwrap();
src-tauri/src/library/roots.rs:1204:        let mut conn = pool.acquire().await.unwrap();
src-tauri/src/library/roots.rs:1207:            .unwrap()
src-tauri/src/library/roots.rs:1208:            .unwrap();
src-tauri/src/library/roots.rs:1221:        .unwrap();
src-tauri/src/library/roots.rs:1225:        let mut conn = pool.acquire().await.unwrap();
src-tauri/src/library/roots.rs:1228:            .unwrap()
src-tauri/src/library/roots.rs:1229:            .unwrap();
src-tauri/src/library/roots.rs:1238:        .unwrap();
src-tauri/src/library/roots.rs:1240:        let mut conn = pool.acquire().await.unwrap();
src-tauri/src/library/roots.rs:1243:            .unwrap()
src-tauri/src/library/roots.rs:1244:            .unwrap();
src-tauri/src/library/roots.rs:1256:        .unwrap();
src-tauri/src/library/roots.rs:1261:            let mut conn = pool.acquire().await.unwrap();
src-tauri/src/library/roots.rs:1264:                .unwrap();
src-tauri/src/library/roots.rs:1280:                .unwrap();
src-tauri/src/library/roots.rs:1282:                let mut conn = pool.acquire().await.unwrap();
src-tauri/src/library/roots.rs:1285:                    .unwrap()
src-tauri/src/library/roots.rs:1286:                    .unwrap();
src-tauri/src/library/roots.rs:1295:        let temp = tempdir().expect("tempdir");
src-tauri/src/library/roots.rs:1297:        let pool = init_pool(&db_path).await.expect("init_pool");
src-tauri/src/library/roots.rs:1301:        std::fs::create_dir_all(&dir1).expect("create dir1");
src-tauri/src/library/roots.rs:1302:        std::fs::create_dir_all(&dir2).expect("create dir2");
src-tauri/src/library/roots.rs:1303:        let canon1 = std::fs::canonicalize(&dir1).unwrap();
src-tauri/src/library/roots.rs:1304:        let canon2 = std::fs::canonicalize(&dir2).unwrap();
src-tauri/src/library/roots.rs:1328:                .unwrap()
src-tauri/src/library/roots.rs:1339:            .expect("add root 1");
src-tauri/src/library/roots.rs:1342:        let mut conn = pool.acquire().await.unwrap();
src-tauri/src/library/roots.rs:1345:            .unwrap();
src-tauri/src/library/roots.rs:1356:        .unwrap();
src-tauri/src/library/roots.rs:1371:                .unwrap()
src-tauri/src/library/roots.rs:1385:        let mut conn = pool.acquire().await.unwrap();
src-tauri/src/library/roots.rs:1386:        let roots = database::roots::list_roots(&mut conn).await.unwrap();
src-tauri/src/library/roots.rs:1390:            .unwrap()
src-tauri/src/library/roots.rs:1391:            .unwrap();
src-tauri/src/library/roots.rs:1397:        let temp = tempdir().expect("tempdir");
src-tauri/src/library/roots.rs:1399:        let pool = init_pool(&db_path).await.expect("init_pool");
src-tauri/src/library/roots.rs:1403:        std::fs::create_dir_all(&dir1).expect("create dir1");
src-tauri/src/library/roots.rs:1404:        std::fs::create_dir_all(&dir2).expect("create dir2");
src-tauri/src/library/roots.rs:1405:        let canon1 = std::fs::canonicalize(&dir1).unwrap();
src-tauri/src/library/roots.rs:1428:                .unwrap()
src-tauri/src/library/roots.rs:1437:            .expect("add root 1");
src-tauri/src/library/roots.rs:1449:        .expect("unknown probe must succeed");
src-tauri/src/library/roots.rs:1453:        let mut conn = pool.acquire().await.unwrap();
src-tauri/src/library/roots.rs:1456:            .unwrap()
src-tauri/src/library/roots.rs:1457:            .unwrap();
src-tauri/src/library/roots.rs:1466:        let temp = tempdir().expect("tempdir");
src-tauri/src/library/roots.rs:1468:        let pool = init_pool(&db_path).await.expect("init_pool");
src-tauri/src/library/roots.rs:1472:        std::fs::create_dir_all(&dir1).expect("create dir1");
src-tauri/src/library/roots.rs:1473:        std::fs::create_dir_all(&dir2).expect("create dir2");
src-tauri/src/library/roots.rs:1474:        let canon1 = std::fs::canonicalize(&dir1).unwrap();
src-tauri/src/library/roots.rs:1475:        let canon2 = std::fs::canonicalize(&dir2).unwrap();
src-tauri/src/library/roots.rs:1499:                .unwrap()
src-tauri/src/library/roots.rs:1508:            .expect("add root 1");
src-tauri/src/library/roots.rs:1522:                .unwrap()
src-tauri/src/library/roots.rs:1531:            .expect("definite change without files should update stored value");
src-tauri/src/library/roots.rs:1534:        let mut conn = pool.acquire().await.unwrap();
src-tauri/src/library/roots.rs:1537:            .unwrap()
src-tauri/src/library/roots.rs:1538:            .unwrap();
src-tauri/src/library/roots.rs:1547:        let temp = tempdir().expect("tempdir");
src-tauri/src/library/roots.rs:1549:        let pool = init_pool(&db_path).await.expect("init_pool");
src-tauri/src/library/roots.rs:1557:            std::fs::create_dir_all(d).expect("create dir");
src-tauri/src/library/roots.rs:1579:        .unwrap();
src-tauri/src/library/roots.rs:1598:        .unwrap();
src-tauri/src/library/roots.rs:1618:        .unwrap();
src-tauri/src/library/roots.rs:1639:        .unwrap();
src-tauri/src/library/roots.rs:1643:        let mut conn = pool.acquire().await.unwrap();
src-tauri/src/library/roots.rs:1658:        .unwrap();
src-tauri/src/library/roots.rs:1677:        .unwrap();
src-tauri/src/library/case_probe.rs:318:        let temp = tempdir().expect("tempdir");
src-tauri/src/library/case_probe.rs:320:        std::fs::create_dir_all(&test_dir).expect("create dir");
src-tauri/src/library/case_probe.rs:325:            for entry in std::fs::read_dir(dir).expect("read_dir") {
src-tauri/src/library/case_probe.rs:326:                let entry = entry.expect("dir entry");
src-tauri/src/library/case_probe.rs:327:                let meta = entry.metadata().expect("metadata");
src-tauri/src/library/case_probe.rs:328:                let mtime = meta.modified().expect("mtime");
src-tauri/src/library/case_probe.rs:341:            .expect("meta")
src-tauri/src/library/case_probe.rs:343:            .expect("mtime");
src-tauri/src/library/case_probe.rs:351:            .expect("meta")
src-tauri/src/library/case_probe.rs:353:            .expect("mtime");
src-tauri/src/library/case_probe.rs:376:        let temp = tempdir().expect("tempdir");
src-tauri/src/library/case_probe.rs:383:        std::fs::write(&uncased_file, b"content").expect("write file");
src-tauri/src/library/case_probe.rs:388:        std::fs::write(&cased_file, b"photo").expect("write cased");
src-tauri/src/library/case_probe.rs:391:        let (found_path, found_name) = found.unwrap();
src-tauri/src/library/canonical.rs:127:        let temp = tempdir().expect("tempdir");
src-tauri/src/library/canonical.rs:140:        let temp = tempdir().expect("tempdir");
src-tauri/src/library/canonical.rs:142:        std::fs::write(&file_path, b"content").expect("write file");
src-tauri/src/library/canonical.rs:147:                assert_eq!(p, std::fs::canonicalize(&file_path).unwrap());
src-tauri/src/library/canonical.rs:155:        let temp = tempdir().expect("tempdir");
src-tauri/src/library/canonical.rs:157:        std::fs::create_dir_all(&dir).expect("create dir");
src-tauri/src/library/canonical.rs:159:        let root = canonicalize_root(&dir).expect("canonicalize should succeed");
src-tauri/src/library/canonical.rs:160:        assert_eq!(root.canonical_path, std::fs::canonicalize(&dir).unwrap());
src-tauri/src/library/canonical.rs:169:        let temp = tempdir().expect("tempdir");
src-tauri/src/library/canonical.rs:171:        std::fs::create_dir_all(&target_dir).expect("create target dir");
src-tauri/src/library/canonical.rs:174:        std::os::unix::fs::symlink(&target_dir, &symlink_path).expect("create symlink");
src-tauri/src/library/canonical.rs:176:        let root = canonicalize_root(&symlink_path).expect("canonicalize symlink");
src-tauri/src/library/canonical.rs:177:        let expected_canon = std::fs::canonicalize(&target_dir).unwrap();
src-tauri/src/library/canonical.rs:188:        let temp = tempdir().expect("tempdir");
src-tauri/src/library/canonical.rs:202:        let temp = tempdir().expect("tempdir");
src-tauri/src/library/canonical.rs:204:        std::fs::create_dir_all(&dir).expect("create dir with colon");
src-tauri/src/library/canonical.rs:206:        let root = canonicalize_root(&dir).expect("directory named 'Trip: 2024' must be accepted");
src-tauri/src/library/volume.rs:44:        let temp = tempfile::tempdir().expect("tempdir");
src-tauri/src/library/volume.rs:45:        let vol = describe_volume(temp.path()).expect("describe_volume must succeed on temp dir");
src-tauri/src/jobs/tests.rs:44:    let temp = tempdir().expect("create tempdir");
src-tauri/src/jobs/tests.rs:47:    fs::create_dir_all(&app_data).expect("create app_data");
src-tauri/src/jobs/tests.rs:49:    let pool = init_pool(&db_path).await.expect("init_pool");
src-tauri/src/jobs/tests.rs:54:    fs::create_dir_all(&root_path).expect("create library root dir");
src-tauri/src/jobs/tests.rs:62:    .expect("add_root");
src-tauri/src/jobs/tests.rs:64:    let root_id = RootId::from(res.root_id.expect("root_id in result"));
src-tauri/src/jobs/tests.rs:80:    fs::write(fix.root_path.join("photo1.jpg"), b"fake_jpeg_1").expect("write photo1");
src-tauri/src/jobs/tests.rs:81:    fs::write(fix.root_path.join("photo2.png"), b"fake_png_2").expect("write photo2");
src-tauri/src/jobs/tests.rs:82:    fs::create_dir_all(fix.root_path.join("vacation")).expect("create subdir");
src-tauri/src/jobs/tests.rs:87:    .expect("write photo3");
src-tauri/src/jobs/tests.rs:98:            let mut list = events_sink.lock().unwrap();
src-tauri/src/jobs/tests.rs:106:        .expect("start_scan_impl");
src-tauri/src/jobs/tests.rs:119:    let recorded = events.lock().unwrap().clone();
src-tauri/src/jobs/tests.rs:129:    let last_event = recorded.last().unwrap();
src-tauri/src/jobs/tests.rs:136:    let mut conn = fix.pool.acquire().await.unwrap();
src-tauri/src/jobs/tests.rs:139:        .unwrap()
src-tauri/src/jobs/tests.rs:140:        .expect("job row must exist");
src-tauri/src/jobs/tests.rs:148:        .unwrap()
src-tauri/src/jobs/tests.rs:149:        .unwrap();
src-tauri/src/jobs/tests.rs:165:    fs::write(fix.root_path.join("file1.jpg"), b"data1").unwrap();
src-tauri/src/jobs/tests.rs:166:    fs::write(fix.root_path.join("file2.jpg"), b"data2").unwrap();
src-tauri/src/jobs/tests.rs:167:    fs::write(fix.root_path.join("file3.jpg"), b"data3").unwrap();
src-tauri/src/jobs/tests.rs:181:        .unwrap();
src-tauri/src/jobs/tests.rs:190:    let mut conn = fix.pool.acquire().await.unwrap();
src-tauri/src/jobs/tests.rs:191:    assert_eq!(count_for_root(&mut conn, &fix.root_id).await.unwrap(), 3);
src-tauri/src/jobs/tests.rs:201:            let mut list = events_sink.lock().unwrap();
src-tauri/src/jobs/tests.rs:209:        .unwrap();
src-tauri/src/jobs/tests.rs:214:    manager.cancel_scan_impl(&job_id).await.unwrap();
src-tauri/src/jobs/tests.rs:228:    let recorded = events.lock().unwrap().clone();
src-tauri/src/jobs/tests.rs:229:    let last = recorded.last().unwrap();
src-tauri/src/jobs/tests.rs:238:        .unwrap()
src-tauri/src/jobs/tests.rs:239:        .unwrap();
src-tauri/src/jobs/tests.rs:248:            .unwrap();
src-tauri/src/jobs/tests.rs:261:    fs::remove_dir_all(&fix.root_path).expect("delete root dir");
src-tauri/src/jobs/tests.rs:264:    let mut conn = fix.pool.acquire().await.unwrap();
src-tauri/src/jobs/tests.rs:265:    let jobs_before = database::count_total_jobs(&mut conn).await.unwrap();
src-tauri/src/jobs/tests.rs:275:    let mut conn = fix.pool.acquire().await.unwrap();
src-tauri/src/jobs/tests.rs:276:    let jobs_after = database::count_total_jobs(&mut conn).await.unwrap();
src-tauri/src/jobs/tests.rs:284:        .unwrap()
src-tauri/src/jobs/tests.rs:285:        .unwrap();
src-tauri/src/jobs/tests.rs:296:    fs::remove_dir_all(&fix.root_path).expect("delete root dir");
src-tauri/src/jobs/tests.rs:298:    let mut conn = fix.pool.acquire().await.unwrap();
src-tauri/src/jobs/tests.rs:301:        .unwrap()
src-tauri/src/jobs/tests.rs:302:        .unwrap();
src-tauri/src/jobs/tests.rs:306:            .unwrap()
src-tauri/src/jobs/tests.rs:333:            let mut list = events_sink.lock().unwrap();
src-tauri/src/jobs/tests.rs:341:        .unwrap();
src-tauri/src/jobs/tests.rs:353:    let recorded = events.lock().unwrap().clone();
src-tauri/src/jobs/tests.rs:354:    let last = recorded.last().unwrap();
src-tauri/src/jobs/tests.rs:367:    let mut conn = fix.pool.acquire().await.unwrap();
src-tauri/src/jobs/tests.rs:370:        .unwrap()
src-tauri/src/jobs/tests.rs:371:        .unwrap();
src-tauri/src/jobs/tests.rs:377:        .unwrap()
src-tauri/src/jobs/tests.rs:378:        .unwrap();
src-tauri/src/jobs/tests.rs:393:    let guard = ScanGuard::acquire(&fix.root_id).expect("acquire guard");
src-tauri/src/jobs/tests.rs:395:    let mut conn = fix.pool.acquire().await.unwrap();
src-tauri/src/jobs/tests.rs:396:    let initial_job_count = database::count_total_jobs(&mut conn).await.unwrap();
src-tauri/src/jobs/tests.rs:402:    let err = res.err().unwrap();
src-tauri/src/jobs/tests.rs:406:    let count_after = database::count_total_jobs(&mut conn).await.unwrap();
src-tauri/src/jobs/tests.rs:425:    fs::write(fix.root_path.join("file1.jpg"), b"fake_data_1").unwrap();
src-tauri/src/jobs/tests.rs:443:            let mut list = events_sink.lock().unwrap();
src-tauri/src/jobs/tests.rs:451:        .expect("start_scan_with_options");
src-tauri/src/jobs/tests.rs:467:    let mut conn = fix.pool.acquire().await.unwrap();
src-tauri/src/jobs/tests.rs:472:        .unwrap()
src-tauri/src/jobs/tests.rs:473:        .expect("job row must exist");
src-tauri/src/jobs/tests.rs:477:        serde_json::from_str(&job_row.1).expect("payload_json must be valid JSON");
src-tauri/src/jobs/tests.rs:487:        .unwrap()
src-tauri/src/jobs/tests.rs:488:        .expect("root must exist");
src-tauri/src/jobs/tests.rs:501:    fs::write(fix.root_path.join("file1.jpg"), b"fake_data_1").unwrap();
src-tauri/src/jobs/tests.rs:512:            let mut list = events_sink.lock().unwrap();
src-tauri/src/jobs/tests.rs:520:        .expect("start_scan_impl");
src-tauri/src/jobs/tests.rs:536:    let mut conn = fix.pool.acquire().await.unwrap();
src-tauri/src/jobs/tests.rs:541:        .unwrap()
src-tauri/src/jobs/tests.rs:542:        .expect("job row must exist");
src-tauri/src/jobs/tests.rs:546:        serde_json::from_str(&job_row.1).expect("payload_json must be valid JSON");
src-tauri/src/jobs/tests.rs:549:        .expect("payload must contain generation string");
src-tauri/src/jobs/tests.rs:555:        .unwrap()
src-tauri/src/jobs/tests.rs:556:        .expect("root must exist");
src-tauri/src/jobs/tests.rs:581:    .unwrap();
src-tauri/src/jobs/tests.rs:594:    .unwrap();
src-tauri/src/jobs/tests.rs:601:    let now_ms = database::now_ms().unwrap();
src-tauri/src/jobs/tests.rs:603:    let mut conn = fix.pool.acquire().await.unwrap();
src-tauri/src/jobs/tests.rs:617:    .unwrap();
src-tauri/src/jobs/tests.rs:646:            events_sink.lock().unwrap().push(ev);
src-tauri/src/jobs/tests.rs:672:    let recorded = events.lock().unwrap().clone();
src-tauri/src/jobs/tests.rs:696:            .unwrap();
src-tauri/src/jobs/tests.rs:706:    let mut conn = fix.pool.acquire().await.unwrap();
src-tauri/src/jobs/tests.rs:709:        .unwrap()
src-tauri/src/jobs/tests.rs:710:        .expect("job row must exist");
src-tauri/src/jobs/tests.rs:715:    let term = recorded.last().unwrap();
src-tauri/src/jobs/tests.rs:728:    fs::write(fix.root_path.join("file1.jpg"), b"data1").unwrap();
src-tauri/src/jobs/tests.rs:729:    fs::write(fix.root_path.join("file2.jpg"), b"data2").unwrap();
src-tauri/src/jobs/tests.rs:746:        .unwrap();
src-tauri/src/jobs/tests.rs:751:    let mut conn = fix.pool.acquire().await.unwrap();
src-tauri/src/jobs/tests.rs:770:    let mut conn = fix.pool.acquire().await.unwrap();
src-tauri/src/jobs/tests.rs:788:    .unwrap();
src-tauri/src/jobs/tests.rs:812:        .unwrap();
src-tauri/src/jobs/tests.rs:818:        .unwrap();
src-tauri/src/jobs/tests.rs:825:            .unwrap()
src-tauri/src/jobs/tests.rs:826:            .unwrap();
src-tauri/src/jobs/tests.rs:835:        .unwrap()
src-tauri/src/jobs/tests.rs:836:        .unwrap();
src-tauri/src/jobs/tests.rs:842:        .unwrap();
src-tauri/src/jobs/tests.rs:848:        .unwrap()
src-tauri/src/jobs/tests.rs:849:        .unwrap();
src-tauri/src/jobs/tests.rs:860:    let guard = ScanGuard::acquire(&fix.root_id).unwrap();
src-tauri/src/jobs/tests.rs:862:    let now_ms = database::now_ms().unwrap();
src-tauri/src/jobs/tests.rs:864:    let mut conn = fix.pool.acquire().await.unwrap();
src-tauri/src/jobs/tests.rs:878:    .unwrap();
src-tauri/src/jobs/tests.rs:909:            let mut list = events_sink.lock().unwrap();
src-tauri/src/jobs/tests.rs:921:    let recorded = events.lock().unwrap().clone();
src-tauri/src/jobs/tests.rs:929:    let mut conn = fix.pool.acquire().await.unwrap();
src-tauri/src/jobs/tests.rs:932:        .unwrap()
src-tauri/src/jobs/tests.rs:933:        .unwrap();
src-tauri/src/jobs/tests.rs:951:    let guard = ScanGuard::acquire(&fix.root_id).unwrap();
src-tauri/src/jobs/tests.rs:955:    assert_eq!(res.err().unwrap().code, "root_busy");
src-tauri/src/jobs/tests.rs:970:    let mut conn = fix.pool.acquire().await.unwrap();
src-tauri/src/jobs/tests.rs:990:        .unwrap();
src-tauri/src/jobs/tests.rs:1007:        .unwrap();
src-tauri/src/jobs/tests.rs:1024:    .unwrap();
src-tauri/src/jobs/tests.rs:1029:        .unwrap();
src-tauri/src/jobs/tests.rs:1038:            .unwrap();
src-tauri/src/jobs/tests.rs:1044:        .unwrap();
src-tauri/src/jobs/tests.rs:1050:        let row = database::get_job_by_id(&mut conn, &job_id).await.unwrap();
src-tauri/src/jobs/tests.rs:1057:        let row = database::get_job_by_id(&mut conn, &job_id).await.unwrap();
src-tauri/src/jobs/tests.rs:1070:    let err = res.err().unwrap();
src-tauri/src/jobs/tests.rs:1082:    let started_json = serde_json::to_string(&started).unwrap();
src-tauri/src/jobs/tests.rs:1095:    let progress_json = serde_json::to_string(&progress).unwrap();
src-tauri/src/jobs/tests.rs:1125:    let finished_json = serde_json::to_string(&finished).unwrap();
src-tauri/src/jobs/tests.rs:1137:    let failed_json = serde_json::to_string(&failed).unwrap();
src-tauri/src/jobs/tests.rs:1147:    let res_json = serde_json::to_string(&res).unwrap();
src-tauri/src/jobs/tests.rs:1155:    let mut conn = fix.pool.acquire().await.unwrap();
src-tauri/src/jobs/tests.rs:1156:    let now_ms = database::now_ms().unwrap();
src-tauri/src/jobs/tests.rs:1172:    .unwrap();
src-tauri/src/jobs/tests.rs:1176:        .unwrap();
src-tauri/src/jobs/tests.rs:1181:        .unwrap()
src-tauri/src/jobs/tests.rs:1182:        .unwrap();
src-tauri/src/jobs/tests.rs:1199:    .unwrap();
src-tauri/src/jobs/tests.rs:1203:        .unwrap();
src-tauri/src/jobs/tests.rs:1208:        .unwrap()
src-tauri/src/jobs/tests.rs:1209:        .unwrap();
src-tauri/src/jobs/tests.rs:1221:    fs::write(fix.root_path.join("photo1.jpg"), b"img1").unwrap();
src-tauri/src/jobs/tests.rs:1222:    fs::write(fix.root_path.join("photo2.jpg"), b"img2").unwrap();
src-tauri/src/jobs/tests.rs:1225:    let mut conn = fix.pool.acquire().await.unwrap();
src-tauri/src/jobs/tests.rs:1228:        .unwrap()
src-tauri/src/jobs/tests.rs:1229:        .unwrap();
src-tauri/src/jobs/tests.rs:1230:    let vol_id = root_row.volume_id.clone().unwrap();
src-tauri/src/jobs/tests.rs:1233:        .unwrap()
src-tauri/src/jobs/tests.rs:1234:        .unwrap();
src-tauri/src/jobs/tests.rs:1240:            .unwrap();
src-tauri/src/jobs/tests.rs:1253:    .unwrap();
src-tauri/src/jobs/tests.rs:1256:    fs::remove_file(fix.root_path.join("photo1.jpg")).unwrap();
src-tauri/src/jobs/tests.rs:1257:    fs::remove_file(fix.root_path.join("photo2.jpg")).unwrap();
src-tauri/src/jobs/tests.rs:1272:    let mut conn = fix.pool.acquire().await.unwrap();
src-tauri/src/jobs/tests.rs:1273:    let jobs_before = database::count_total_jobs(&mut conn).await.unwrap();
src-tauri/src/jobs/tests.rs:1282:    let mut conn = fix.pool.acquire().await.unwrap();
src-tauri/src/jobs/tests.rs:1283:    let jobs_after = database::count_total_jobs(&mut conn).await.unwrap();
src-tauri/src/jobs/tests.rs:1288:        .unwrap()
src-tauri/src/jobs/tests.rs:1289:        .unwrap();
src-tauri/src/jobs/tests.rs:1297:            .unwrap();
src-tauri/src/jobs/tests.rs:1351:    let mut conn = fix.pool.acquire().await.unwrap();
src-tauri/src/jobs/tests.rs:1352:    assert_eq!(database::count_total_jobs(&mut conn).await.unwrap(), 0);
src-tauri/src/jobs/tests.rs:1355:        .unwrap()
src-tauri/src/jobs/tests.rs:1356:        .unwrap();
src-tauri/src/jobs/tests.rs:1397:    fs::write(fix.root_path.join("a.jpg"), b"img").unwrap();
src-tauri/src/jobs/tests.rs:1413:            events_c.lock().unwrap().push(ev);
src-tauri/src/jobs/tests.rs:1419:        .unwrap();
src-tauri/src/jobs/tests.rs:1422:    let recorded_events = events.lock().unwrap().clone();
src-tauri/src/jobs/tests.rs:1429:        recorded_events.last().unwrap(),
src-tauri/src/jobs/tests.rs:1449:        .expect("new scan must start cleanly after unrecorded terminal event");
src-tauri/src/jobs/tests.rs:1463:    let now_ms = database::now_ms().unwrap();
src-tauri/src/jobs/tests.rs:1464:    let mut conn = fix.pool.acquire().await.unwrap();
src-tauri/src/jobs/tests.rs:1478:    .unwrap();
src-tauri/src/jobs/tests.rs:1481:    let guard = ScanGuard::acquire(&fix.root_id).unwrap();
src-tauri/src/jobs/tests.rs:1505:            events_c.lock().unwrap().push(ev);
src-tauri/src/jobs/tests.rs:1510:    let recorded_events = events.lock().unwrap().clone();
src-tauri/src/jobs/tests.rs:1531:    fs::write(fix.root_path.join("fail.jpg"), b"img").unwrap();
src-tauri/src/jobs/tests.rs:1554:                events_c.lock().unwrap().push(ev);
src-tauri/src/jobs/tests.rs:1561:        .unwrap();
src-tauri/src/jobs/tests.rs:1564:    let recorded_events = events.lock().unwrap().clone();
src-tauri/src/jobs/tests.rs:1571:        recorded_events.last().unwrap(),
src-tauri/src/jobs/tests.rs:1597:    let now_ms = database::now_ms().unwrap();
src-tauri/src/jobs/tests.rs:1598:    let mut conn = fix.pool.acquire().await.unwrap();
src-tauri/src/jobs/tests.rs:1612:    .unwrap();
src-tauri/src/jobs/tests.rs:1615:    let guard = ScanGuard::acquire(&fix.root_id).unwrap();
src-tauri/src/jobs/tests.rs:1641:            events_c.lock().unwrap().push(ev);
src-tauri/src/jobs/tests.rs:1646:    let recorded_events = events.lock().unwrap().clone();
src-tauri/src/jobs/tests.rs:1667:    fs::write(fix.root_path.join("retry.jpg"), b"img").unwrap();
src-tauri/src/jobs/tests.rs:1683:            events_c.lock().unwrap().push(ev);
src-tauri/src/jobs/tests.rs:1689:        .unwrap();
src-tauri/src/jobs/tests.rs:1692:    let recorded_events = events.lock().unwrap().clone();
src-tauri/src/jobs/tests.rs:1694:        recorded_events.last().unwrap(),
src-tauri/src/jobs/tests.rs:1702:    let mut conn = fix.pool.acquire().await.unwrap();
src-tauri/src/jobs/tests.rs:1705:        .unwrap()
src-tauri/src/jobs/tests.rs:1706:        .unwrap();
src-tauri/src/jobs/tests.rs:1713:    fs::write(fix.root_path.join("gate.jpg"), b"img").unwrap();
src-tauri/src/jobs/tests.rs:1723:            let entered = entered_opt.lock().unwrap().take();
src-tauri/src/jobs/tests.rs:1724:            let release = release_opt.lock().unwrap().take();
src-tauri/src/jobs/tests.rs:1762:        .unwrap();
src-tauri/src/jobs/tests.rs:1768:        .expect("supervisor must enter final write gate");
src-tauri/src/jobs/tests.rs:1776:    let mut conn = fix.pool.acquire().await.unwrap();
src-tauri/src/jobs/tests.rs:1779:        .unwrap()
src-tauri/src/jobs/tests.rs:1780:        .unwrap();
src-tauri/src/jobs/tests.rs:1795:    let mut conn = fix.pool.acquire().await.unwrap();
src-tauri/src/jobs/tests.rs:1798:        .unwrap()
src-tauri/src/jobs/tests.rs:1799:        .unwrap();
src-tauri/src/jobs/tests.rs:1811:    let root_dir = manifest_dir.parent().expect("workspace root");
src-tauri/src/jobs/tests.rs:1813:    let ts_content = std::fs::read_to_string(&ts_path).expect("read scanText.ts");
src-tauri/src/jobs/tests.rs:1818:        .expect("find SCAN_FAILURE_MESSAGES start")
src-tauri/src/jobs/tests.rs:1821:        .expect("find SCAN_FAILURE_MESSAGES end");
---
$ grep -rn "panic!" src-tauri/src/watcher src-tauri/src/metadata src-tauri/src/library src-tauri/src/jobs src-tauri/src/commands
src-tauri/src/metadata/mod.rs:895:            other => panic!("Expected NonAbsolutePath, got: {:?}", other),
src-tauri/src/metadata/mod.rs:912:            other => panic!("Expected FileNotFound, got: {:?}", other),
src-tauri/src/metadata/mod.rs:932:            Err(e) => panic!("Expected Ok(empty) or ParseError, got: {:?}", e),
src-tauri/src/metadata/mod.rs:959:            other => panic!("Expected Timeout error, got: {:?}", other),
src-tauri/src/metadata/mod.rs:997:            other => panic!("Expected Timeout, got: {:?}", other),
src-tauri/src/library/roots.rs:1072:                panic!(
src-tauri/src/library/roots.rs:1197:            |_| panic!("simulated checker panic"),
src-tauri/src/library/canonical.rs:121:            other => panic!("Expected PathNotAbsolute, got {other:?}"),
src-tauri/src/library/canonical.rs:134:            other => panic!("Expected PathNotFound, got {other:?}"),
src-tauri/src/library/canonical.rs:149:            other => panic!("Expected PathNotDirectory, got {other:?}"),
src-tauri/src/jobs/tests.rs:890:        panic!("worker panicked inside test!");
src-tauri/src/jobs/tests.rs:1622:        panic!("panic inside worker with final write failure");
src-tauri/src/jobs/tests.rs:1865:            panic!("Failure code '{code}' missing from SCAN_FAILURE_MESSAGES in scanText.ts")
---
$ grep -rn "unsafe" src-tauri/src
src-tauri/src/platform/macos.rs:45:    let stat_res = unsafe { libc::statfs(c_path.as_ptr(), stat.as_mut_ptr()) };
src-tauri/src/platform/macos.rs:53:    let stat = unsafe { stat.assume_init() };
src-tauri/src/platform/macos.rs:56:    let fstype_raw = unsafe { CStr::from_ptr(stat.f_fstypename.as_ptr()) };
src-tauri/src/platform/macos.rs:62:    let mut attr_list: libc::attrlist = unsafe { std::mem::zeroed() };
src-tauri/src/platform/macos.rs:70:    let attr_res = unsafe {
src-tauri/src/platform/macos.rs:82:        let buf = unsafe { attr_buf.assume_init() };
src-tauri/src/platform/macos.rs:99:                let name_bytes = unsafe { std::slice::from_raw_parts(name_ptr, name_len) };
src-tauri/src/platform/windows.rs:30:    let res = unsafe {
src-tauri/src/platform/windows.rs:45:    let drive_type = unsafe { GetDriveTypeW(volume_mount_point.as_ptr()) };
src-tauri/src/platform/windows.rs:52:    let vol_info_res = unsafe {
src-tauri/src/platform/windows.rs:90:    let guid_res = unsafe {
src-tauri/src/platform/windows.rs:151:    let mut info: FILE_ID_INFO = unsafe { std::mem::zeroed() };
src-tauri/src/platform/windows.rs:155:    let ok = unsafe {
src-tauri/src/library/case_probe.rs:49:            let handle = unsafe {
src-tauri/src/library/case_probe.rs:72:            let ok = unsafe { GetFileInformationByHandle(handle, info.as_mut_ptr()) };
src-tauri/src/library/case_probe.rs:74:            unsafe { CloseHandle(handle) };
src-tauri/src/library/case_probe.rs:81:            let info = unsafe { info.assume_init() };
src-tauri/src/image_pipeline/vips.rs:47:    unsafe {
src-tauri/src/image_pipeline/vips.rs:94:    unsafe {
src-tauri/src/image_pipeline/vips.rs:210:        unsafe { bindings::vips_type_find(c_op.as_ptr(), c_loader.as_ptr()) != 0 }
src-tauri/src/image_pipeline/vips.rs:394:        let type_id = unsafe {
src-tauri/src/image_pipeline/vips.rs:420:            let rc = unsafe {
src-tauri/src/smoke.rs:68:    unsafe {
src-tauri/src/smoke.rs:95:    unsafe {
src-tauri/src/smoke.rs:217:    let vips_ver = unsafe {
src-tauri/src/main.rs:16:            unsafe {
src-tauri/src/metadata/mod.rs:1013:        let res = unsafe { kill(pid, 0) };
---
$ grep -rnE "failure_hook|failure_in|panic_in|panic_after|fail_final|test_deps|last_child_pid|set_changes_detected_for_test" src-tauri/src
src-tauri/src/watcher/manager.rs:220:    pub fn set_changes_detected_for_test(&self, root_id: &RootId, at: Instant) {
src-tauri/src/jobs/mod.rs:241:    test_deps: Mutex<Option<JobManagerTestDeps>>,
src-tauri/src/jobs/mod.rs:269:            test_deps: Mutex::new(None),
src-tauri/src/jobs/mod.rs:274:    pub fn set_test_deps(&self, deps: JobManagerTestDeps) {
src-tauri/src/jobs/mod.rs:275:        let mut guard = match self.test_deps.lock() {
src-tauri/src/jobs/mod.rs:283:    fn get_test_deps(&self) -> Option<JobManagerTestDeps> {
src-tauri/src/jobs/mod.rs:284:        let guard = match self.test_deps.lock() {
src-tauri/src/jobs/mod.rs:475:        let test_deps = self.get_test_deps();
src-tauri/src/jobs/mod.rs:477:        let timeout = test_deps
src-tauri/src/jobs/mod.rs:487:                let is_dir_fn = test_deps
src-tauri/src/jobs/mod.rs:491:                let describer = test_deps
src-tauri/src/jobs/mod.rs:922:        let hook = manager.get_test_deps().and_then(|d| d.final_write_hook);
src-tauri/src/jobs/tests.rs:313:    manager.set_test_deps(crate::jobs::JobManagerTestDeps {
src-tauri/src/jobs/tests.rs:432:        failure_hook_after_batch: Some(0),
src-tauri/src/jobs/tests.rs:1260:    manager.set_test_deps(crate::jobs::JobManagerTestDeps {
src-tauri/src/jobs/tests.rs:1318:    manager.set_test_deps(crate::jobs::JobManagerTestDeps {
src-tauri/src/jobs/tests.rs:1400:    manager.set_test_deps(crate::jobs::JobManagerTestDeps {
src-tauri/src/jobs/tests.rs:1439:    manager.set_test_deps(crate::jobs::JobManagerTestDeps::default());
src-tauri/src/jobs/tests.rs:1457:    manager.set_test_deps(crate::jobs::JobManagerTestDeps {
src-tauri/src/jobs/tests.rs:1521:    manager.set_test_deps(crate::jobs::JobManagerTestDeps::default());
src-tauri/src/jobs/tests.rs:1534:    manager.set_test_deps(crate::jobs::JobManagerTestDeps {
src-tauri/src/jobs/tests.rs:1549:                failure_hook_after_batch: Some(0),
src-tauri/src/jobs/tests.rs:1580:    manager.set_test_deps(crate::jobs::JobManagerTestDeps::default());
src-tauri/src/jobs/tests.rs:1591:    manager.set_test_deps(crate::jobs::JobManagerTestDeps {
src-tauri/src/jobs/tests.rs:1657:    manager.set_test_deps(crate::jobs::JobManagerTestDeps::default());
src-tauri/src/jobs/tests.rs:1670:    manager.set_test_deps(crate::jobs::JobManagerTestDeps {
src-tauri/src/jobs/tests.rs:1721:    manager.set_test_deps(crate::jobs::JobManagerTestDeps {
src-tauri/src/scanner/types.rs:105:    pub failure_hook_after_batch: Option<usize>,
src-tauri/src/scanner/types.rs:107:    pub failure_in_move_step: bool,
src-tauri/src/scanner/types.rs:109:    pub failure_in_final_marking: bool,
src-tauri/src/scanner/types.rs:111:    pub panic_in_walker_after_batch: Option<usize>,
src-tauri/src/scanner/types.rs:120:            failure_hook_after_batch: None,
src-tauri/src/scanner/types.rs:122:            failure_in_move_step: false,
src-tauri/src/scanner/types.rs:124:            failure_in_final_marking: false,
src-tauri/src/scanner/types.rs:126:            panic_in_walker_after_batch: None,
src-tauri/src/scanner/mod.rs:409:                panic_after_batch: options.panic_in_walker_after_batch,
src-tauri/src/scanner/mod.rs:420:        if let Some(fail_after) = options.failure_hook_after_batch {
src-tauri/src/scanner/mod.rs:478:    if options.failure_in_move_step {
src-tauri/src/scanner/mod.rs:499:    if options.failure_in_final_marking {
src-tauri/src/scanner/tests.rs:570:    fail_opts.failure_hook_after_batch = Some(1);
src-tauri/src/scanner/tests.rs:642:    move_fail_opts.failure_in_move_step = true;
src-tauri/src/scanner/tests.rs:768:    panic_opts.panic_in_walker_after_batch = Some(1);
src-tauri/src/scanner/walk.rs:80:    pub panic_after_batch: Option<usize>,
src-tauri/src/scanner/walk.rs:326:                if let Some(panic_after) = params.panic_after_batch {
src-tauri/src/scanner/walk.rs:327:                    if batches_sent >= panic_after {
src-tauri/src/scanner/walk.rs:342:                if let Some(panic_after) = params.panic_after_batch {
src-tauri/src/scanner/walk.rs:343:                    if batches_sent >= panic_after {
src-tauri/src/metadata/mod.rs:115:    last_child_pid: Option<std::sync::Arc<std::sync::Mutex<Option<u32>>>>,
src-tauri/src/metadata/mod.rs:125:            last_child_pid: None,
src-tauri/src/metadata/mod.rs:133:            last_child_pid: None,
src-tauri/src/metadata/mod.rs:144:            last_child_pid: Some(recorder),
src-tauri/src/metadata/mod.rs:185:        if let Some(ref recorder) = self.last_child_pid {
---
$ grep -rnE "UPDATE files|UPDATE folders|INSERT INTO files|INSERT INTO folders|DELETE FROM" src-tauri/src/watcher
---
$ grep -rn "@tauri-apps/api" src
src/lib/tauri/index.ts:1:import { Channel, invoke } from "@tauri-apps/api/core";
---
```

## 6. Decisions made

- **Theme C — Startup window & unmanaged-state retry:** Set `"visible": false` in `tauri.conf.json` and call `show_main_window` in `.setup()` right after `app.manage(...)` succeeds so errors exit cleanly via `show_startup_error_dialog` instead of exposing a dead/broken window. `src/App.tsx` renders `"Starting…"` until `getAppInfo` responds and retries unmanaged-state errors briefly (never showing raw `"state not managed"`). `scripts/verify.mjs` avoids `spawnSync(cmd, args, { shell: true })` on Windows by passing one quoted command string when `isWindows` is true.
- **Theme B — Single-process `rustc` test fakes & OS-split args:** Used `exiftool_args_for_os(is_windows)` with `-api WindowsLongPath=1` gated on Windows after confirming identical JSON on macOS. Compiled single-process fake executables with `rustc` (no `unsafe`, no `Cargo.toml` changes, no `.cmd` grandchild shell ambiguity on Windows) so `tasklist /FI "PID eq <pid>"` accurately verifies process termination on Windows CI.
- **Theme A — O(1) `RootInbox`, visible-only polling, `LastJobSummary.trigger`:** Folded watcher events immediately inside `NotifyWatchBackend`'s callback into `RootInbox` (`first_event_at`, `last_event_at`, `error_seen`) using `should_skip_event_path` (no path queue in memory). Added `WatchingState` (`on` / `off` / `degraded`) on `RootSummary`, persisted `auto_scan` in `app_settings` (`false` default, no migration), derived `LastJobSummary.trigger` (`"manual" | "auto"`) from `jobs.payload_json`, and used `visibilitychange`-gated 5 s polling in `LibraryFolders.tsx` without touching `capabilities/`.

## Owner decisions

- BSL-1.0 handled by a per-crate exception for xxhash-rust (owner decision, 2026-11-27)

## 7. Deviations

- **Per-crate `BSL-1.0` exception in root `deny.toml` (`eab2e4e`):** Added `[licenses]` `exceptions = [{ crate = "xxhash-rust", allow = ["BSL-1.0"] }]` in `deny.toml` (at repository root; there is no `src-tauri/deny.toml`) per owner decision (2026-11-27), and installed `cargo-deny v0.20.2` globally (`cargo install cargo-deny --version 0.20.2 --locked`) to verify locally before pushing.
- **`wait_after_stdout_close_is_inside_the_timeout` is `#[cfg(unix)]`:** Without `unsafe` Win32 handle calls, a Rust binary cannot reliably close its own inherited stdout handle on Windows, and `.cmd` wrappers leave grandchildren. The overall spawn-to-exit deadline covers `child.wait()` identically in shared code (`metadata/mod.rs:257-268`), while `test_child_killed_after_timeout_windows` tests Windows timeout + `tasklist` kill directly in CI.
- **M4 Windows failure not demonstrated in CI:** The Windows test `test_child_killed_after_timeout_windows` was only seen passing in CI (`45928503558`, `45929623693`, `46034567663`); the owner demonstrates it failing on the laptop.
- **Tools used:** This report was written with `grep` because `rg` was restricted (`operation not permitted`).
- **New command module `src-tauri/src/commands/settings.rs`:** Placed `get_auto_scan` / `set_auto_scan` cleanly under `commands/` (within allowed `src-tauri/src/`).

## 8. Known gaps and risks

- **Per-theme size summary:** Theme C = `+91 / -10` (6 files), Theme B = `+638 / -115` (1 file), Theme A = `+2472 / -63` (24 files; incl. `Cargo.lock` +89 / `Cargo.toml` +1), Round-2 `deny.toml` fix = `+4 / -0` (1 file).
- **Hardest part / implementation risk:** Coordinating concurrent ExifTool pipes (so stderr floods or closed-stdout lingering processes cannot deadlock or escape the deadline cleanly) and ensuring watcher + scheduler ownership stays non-blocking, DB-free on watcher callbacks, and single-scan constrained.
- **Theme C & Theme A manual desktop verification pending (owner):** No automated Playwright/WebView launch exists for the `"main"` window visibility transition, startup error dialog, or live Explorer/Finder drag-and-drop UI checks; the owner needs to run those checks interactively (`pnpm tauri dev`) and demonstrate M4 on the Windows laptop.
- **Watcher best-effort & walk limits (`docs/ipc.md`):** OS watchers (`FSEvents`, `ReadDirectoryChangesW`) can drop events under extreme load or on network shares; the 24 h reconciliation scan (when automatic scanning is enabled) mitigates missed events. Walk follow-up risk remains documented in `docs/STATUS.md` (dead network share detection).

## 9. Questions for the human / orchestrator

1. **Owner manual checks (Theme C, Theme A UI, and laptop M4):** Please run the interactive desktop checks (`Starting…` + hidden-until-ready start-up window + error dialog with mismatched platform stamp, creating/deleting a photo inside an Active folder to verify `"Changes detected since the last scan"` / `"Automatically scan changed folders"`) and the M4 Windows kill failure demonstration on the laptop.

## 10. Proposed next task

Not proposed per task rules.

## 10a. Self-audit: Claims table

| Claim | Source |
|---|---|
| Commits `dd56657`, `e36bc72`, `fa17884`, `b7de6b5`, `f034d23`, `eab2e4e` exist in order C → B → A + report + `deny.toml` | `git log --oneline -n 6` and `git show --stat` outputs (run in this session) |
| Local `pnpm verify` passed (`204 passed; 0 failed; 2 ignored`, 5 Node tests, `AGENTS.md` 7439 chars) | Section 5.1 (full raw output) and `reports/logs/verify-2026-11-27.txt` |
| `real_watcher_detects_new_file_smoke` passed 30/30 consecutive runs | Section 5.2 (full raw 30/30 loop output) |
| Local `cargo deny --manifest-path src-tauri/Cargo.toml check` passed (`advisories ok, bans ok, licenses ok, sources ok`) | Section 5.3 (full raw `cargo-deny v0.20.2` output) |
| CI `45928503558` green (3 jobs); CI `45929623693` failed `cargo deny` on `xxhash-rust v0.8.21` (`BSL-1.0`); CI `46034567663` passed all 3 jobs (`Security & License check (cargo deny)` `140626596979`, `macos-arm64` `140626597449`, `windows-x64` `140626597727`) | Section 5.4 (raw `gh run list`, `gh run view 46034567663`, `--json jobs` conclusion lines, and job logs) |
| Node `DEP0190` warning removed by single-string shell call on Windows | Round-1 agent sandbox log, not reproducible by the owner (condensed summary in Section 5.5) |
| `-api WindowsLongPath=1` is a no-op on macOS against `image_orientation_6.jpg` | Round-1 agent sandbox log, not reproducible by the owner (condensed summary in Section 5.5) |
| Mutations M2–M7 (Mac for M4; Windows `test_child_killed_after_timeout_windows` only seen passing in CI, owner demonstrates failure on laptop) | Round-1 agent sandbox log, not reproducible by the owner (condensed summary in Section 5.6, NOT evidence) |
| Zero production `unwrap`/`expect`/`panic!`/`unsafe`, all hooks `#[cfg(test)]`, zero watcher SQL writes, single `@tauri-apps/api` import | Section 5.7 (full raw `grep` output on `eab2e4e`) |
| Diff stat matches 32 changed files (`5599` insertions, `188` deletions) against `main` | Section 3 (`git diff --stat main \| cat` on final saved tree) |

## Concepts used

- **Concurrent futures without deadlocking (`tokio::try_join!`):** Driving stdin writing, stdout reading, and capped stderr draining side by side stops a noisy child process from filling the stderr pipe buffer and stalling `read_stdout` (`src-tauri/src/metadata/mod.rs:258`).
- **Static async semaphore (`tokio::sync::Semaphore::const_new`):** Holding a compile-time `Semaphore::const_new(2)` bounds concurrent ExifTool processes to two across the whole app and returns permits safely when a future finishes or is dropped (`src-tauri/src/metadata/mod.rs:14`).
- **Trait-object dependency injection (`Arc<dyn WatchBackend>`):** Storing a trait object behind `Arc` lets `WatcherManager` use `NotifyWatchBackend` in production while unit tests swap in a fake backend with zero filesystem sleeps (`src-tauri/src/watcher/manager.rs:40`).
- **Pure `Instant` state machine (`RootDebounceState`):** Separating `record_event(now)` and `poll_ready(now)` out of async timers lets burst debouncing and 30 s max-wait logic be verified deterministically in unit tests (`src-tauri/src/watcher/debounce.rs:23-55`).
- **Startup error propagation (`show_main_window` + fixed codes):** Calling `show_main_window(app.handle())` after `app.manage(...)` uses `Result` propagation (`?`) to turn missing-window initialization into a fixed error code and dialog instead of a panic (`src-tauri/src/lib.rs:240-244`).

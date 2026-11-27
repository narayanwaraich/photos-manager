# REPORT-1G: Watcher hints, ExifTool call hardening, and start-up window

**Branch:** `task/1G` · **Part A commit:** `dd56657` · **Code commits:** `e36bc72` (Theme C, 6 files, +91/-10), `fa17884` (Theme B, 1 file, +638/-115), `b7de6b5` (Theme A, 24 files, +2472/-63) · **Report commit:** `<HEAD commit adding this completed report>` · **Date:** 2026-11-27
**Effort (owner-reported):** high
**CI Run IDs:** `45928060396` (Theme C `e36bc72`, cancelled when Theme B pushed), `45928503558` (Theme B `fa17884`, all 3 jobs green), `45929623693` (Theme A `b7de6b5`: `windows-x64` `140278975773` passed, `macos-arm64` `140278976064` passed, `Security & License check (cargo deny)` `140278976016` failed on transitive `xxhash-rust v0.8.21` `BSL-1.0` license)
**OS actually tested:** macOS `27.2.1` (`26B131`, arm64 local `pnpm verify` + 30/30 real-FS watcher smoke test + macOS CI `macos-arm64`) and Windows (`windows-x64` GitHub Actions CI runner)
**Not tested:** Owner interactive desktop manual checks (start-up window delay/unmanaged-state race check, live Desktop drag/drop watcher + auto-scan checkbox UI check); local `cargo deny` (`cargo-deny` is not installed locally and is run by CI); live 24 h elapsed clock reconciliation (tested via deterministic `decide_next_auto_scan` unit test).

## 1. Summary & Part A: Pre-Implementation Audit (written before code)

Completed TASK-1G in three commits on branch `task/1G` in the order **C → B → A**, running `pnpm verify` and pushing after each commit:

- **Theme C (`e36bc72`):** Hid `"main"` in `src-tauri/tauri.conf.json` (`"visible": false`), added `StartupError::MainWindowNotFound` (`"main_window_not_found"`) and `show_main_window` after `app.manage(...)` in `src-tauri/src/lib.rs`, updated `src/App.tsx` to render `"Starting…"` before `getAppInfo` responds and never expose raw `"state not managed"`, and eliminated Node `DEP0190` in `scripts/verify.mjs` by passing a single quoted command line on Windows without `args` + `shell: true`.
- **Theme B (`fa17884`):** Hardened `src-tauri/src/metadata/mod.rs` with pure `exiftool_args_for_os` (`-j -n -m -charset utf8 -charset filename=utf8 -@ -`, plus `-api WindowsLongPath=1` on Windows only after verifying identical JSON output on macOS), process-wide `EXIFTOOL_SEMAPHORE` (`Semaphore::const_new(2)`), concurrent `tokio::try_join!` stdin write / capped 512 KiB stdout read / capped 64 KiB drained stderr read + `child.wait()` under a single deadline, explicit `child.kill().await` + `child.wait().await` on timeout/error, fixed non-leaking `MetadataError` Display strings + strict `std::str::from_utf8` rejection, and `rustc`-compiled single-process tests covering stderr flood, concurrency limit 2, and Windows `tasklist` kill verification.
- **Theme A (`b7de6b5`):** Added `notify = "9.0.0"` (`CC0-1.0`) behind `WatchBackend`, implemented `src-tauri/src/watcher/` (`debounce.rs`, `scheduler.rs`, `manager.rs`, `backend.rs`, `tests.rs`) with O(1)-memory `RootInbox` folding inside the watcher callback, scanner skip-rule reuse, per-`Active`-root watchers (`WatchingState::On | Off | Degraded`), `auto_scan` in `app_settings` (default `false`), pure scheduler (`>= 60 s` gap, max 1 auto scan, oldest change first, `> 24 h` reconciliation), `LastJobSummary.trigger` (`"manual" | "auto"`), 5 s visible-only polling UI in `src/components/LibraryFolders.tsx`, and `docs/ipc.md` updates.

**Stop Condition Encountered on CI Run `45929623693` (`b7de6b5`):**
Both `windows-x64` (`200 passed; 0 failed; 2 ignored`) and `macos-arm64` (`204 passed; 0 failed; 2 ignored`) passed `pnpm verify` and Tauri packaging, **but** `Security & License check (cargo deny)` failed (`advisories ok, bans ok, licenses FAILED, sources ok`) because `notify v9.0.0` pulls in transitive dependency `xxhash-rust v0.8.21` licensed under `BSL-1.0` (Boost Software License 1.0), which is not in `deny.toml` (`licenses.allow`). Because editing `deny.toml` is forbidden by TASK-1G (a Stop Condition), work stopped here to report the conflict cleanly without touching `deny.toml`.

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
| Three commits in order **C → B → A**; `pnpm verify` + push after each | **Met** | Commits `e36bc72` (C), `fa17884` (B), `b7de6b5` (A) in `git log --oneline -n 5` |
| Theme C: Hidden window until ready, `MainWindowNotFound` (`"main_window_not_found"`), `"Starting…"`, no raw `"state not managed"` | **Met (code + unit tests); desktop UI launch not run** | `src-tauri/tauri.conf.json`, `src-tauri/src/lib.rs`, `src-tauri/src/commands/error.rs`, `src/App.tsx`, `test_startup_error_variants_have_unique_codes_and_non_empty_messages`; live Tauri app launch is owner manual check |
| Theme C: `scripts/verify.mjs` eliminates Node `DEP0190` | **Met** | `scripts/verify.mjs`; `node -e` check on Node `v26.9.0` comparing `spawnSync` with `args` vs single string |
| Theme B: Pinned `exiftool_args` + Windows `-api WindowsLongPath=1` verified on Mac | **Met** | `exiftool_args_are_pinned` + Mac `exiftool` fixture test showing identical JSON with/without `-api WindowsLongPath=1` |
| Theme B: Concurrent capped stderr drain (64 KiB) + spawn-to-exit deadline + explicit `kill`/`wait` | **Met** | `stderr_flood_does_not_block_or_masquerade_as_timeout`, `wait_after_stdout_close_is_inside_the_timeout` (`#[cfg(unix)]`), `test_child_killed_after_timeout`, `test_child_killed_after_timeout_windows`; mutations M2/M3/M4 failed on Mac |
| Theme B: Non-leaking `MetadataError`, non-UTF-8 error, semaphore limit 2 | **Met** | `error_display_carries_no_stderr_or_path`, `concurrency_limit_is_two`, `MetadataError::InvalidUtf8` |
| Theme A: `notify` behind `WatchBackend`, O(1) debouncer (2 s / 30 s), scanner skip reuse, no catalogue writes from watcher | **Met** | `src-tauri/src/watcher/`, 15 watcher tests (`debouncer_*`, `ignored_names_do_not_mark_changes`, `event_flood_is_folded_into_one_flag`, `watcher_event_writes_no_catalogue_rows`), `30/30` `real_watcher_detects_new_file_smoke` |
| Theme A: Scheduler (`auto_scan` default `false`, 60 s gap, max 1 auto scan, oldest change first, `> 24 h` reconciliation) + `LastJobSummary.trigger` + UI + `docs/ipc.md` | **Met (code + unit tests + CI); desktop UI manual check not run** | `scheduler_*` tests, `auto_scan_setting_roundtrip_and_default_off`, `stale_root_is_due_after_24h`, `registry_stops_...`, `watcher_start_failure_...`, `changes_detected_is_cleared_when_a_scan_starts`, mutations M5/M6/M7 |
| `pnpm verify` green locally + Windows & macOS CI green | **Met** | Local `pnpm verify` (`204 passed; 0 failed; 2 ignored`); CI `45929623693` `windows-x64` (`200 passed; 0 failed; 2 ignored`) and `macos-arm64` (`204 passed; 0 failed; 2 ignored`) |
| `cargo deny` green on CI | **Not met (Stop Condition)** | CI `45929623693` job `140278976016` failed: `xxhash-rust v0.8.21` (`BSL-1.0`, transitive under `notify v9.0.0`) rejected by `deny.toml`; `deny.toml` was not modified per task rules |

## 3. Files changed

Output of `git diff --stat main | cat` after writing this report:

```text
 docs/ipc.md                        |  22 +
 reports/REPORT-1G.md               | 320 +++++++++++
 scripts/verify.mjs                 |  16 +-
 src-tauri/Cargo.lock               |  89 +++-
 src-tauri/Cargo.toml               |   1 +
 src-tauri/src/commands/error.rs    |   6 +
 src-tauri/src/commands/mod.rs      |   2 +
 src-tauri/src/commands/roots.rs    | 182 ++++++-
 src-tauri/src/commands/settings.rs |  45 ++
 src-tauri/src/database/jobs.rs     |  15 +-
 src-tauri/src/database/mod.rs      |  37 ++
 src-tauri/src/domain/roots.rs      |  12 +
 src-tauri/src/jobs/mod.rs          |  93 +++-
 src-tauri/src/lib.rs               |  46 +-
 src-tauri/src/library/roots.rs     |   9 +-
 src-tauri/src/metadata/mod.rs      | 753 +++++++++++++++++++++++-----
 src-tauri/src/scanner/classify.rs  |  26 +
 src-tauri/src/scanner/mod.rs       |   5 +-
 src-tauri/src/watcher/backend.rs   |  86 ++++
 src-tauri/src/watcher/debounce.rs  | 110 +++++
 src-tauri/src/watcher/manager.rs   | 427 ++++++++++++++++
 src-tauri/src/watcher/mod.rs       |  18 +
 src-tauri/src/watcher/scheduler.rs | 103 ++++
 src-tauri/src/watcher/tests.rs     | 974 +++++++++++++++++++++++++++++++++++++
 src-tauri/tauri.conf.json          |   3 +-
 src/App.tsx                        |  49 +-
 src/components/LibraryFolders.tsx  | 224 +++++++--
 src/lib/scanText.ts                |  14 +
 src/lib/tauri/errorCodes.ts        |   1 +
 src/lib/tauri/index.ts             |  14 +
 src/lib/tauri/schemas.ts           |   7 +
 31 files changed, 3521 insertions(+), 188 deletions(-)
```

**New files added:**
- `src-tauri/src/commands/settings.rs`: IPC commands `get_auto_scan` / `set_auto_scan` and testable `_impl` helpers backed by `app_settings`.
- `src-tauri/src/watcher/mod.rs`: Public module exports for watcher backend, debouncer, scheduler, manager, and tests.
- `src-tauri/src/watcher/backend.rs`: `WatchBackend` trait, `WatchEvent` enum, and `NotifyWatchBackend` wrapping `notify::RecommendedWatcher`.
- `src-tauri/src/watcher/debounce.rs`: Pure `Instant`-driven `RootDebounceState` (2 s quiet, 30 s max wait) and O(1) `RootInbox` event folding.
- `src-tauri/src/watcher/scheduler.rs`: Pure `decide_next_auto_scan` policy function (auto scan toggle, Active-only, 60 s gap, oldest change first, 24 h reconciliation).
- `src-tauri/src/watcher/manager.rs`: `WatcherManager` per-root registry and 1 s background ticker (`spawn_watcher_and_scheduler`).
- `src-tauri/src/watcher/tests.rs`: 15 unit, integration, and real-filesystem smoke tests for watcher, debouncer, settings, and scheduler.

## 4. Dependencies

- **`notify = { version = "9.0.0", default-features = false, features = ["macos_fsevent"] }`**
  - **Source & license verified:** `cargo info notify` / `https://crates.io/crates/notify/9.0.0` (`version: 9.0.0`, `license: CC0-1.0`, `rust-version: 1.91`, `repository: https://github.com/notify-rs/notify.git`, `+default = [macos_fsevent]`).
  - **Transitive license conflict found in CI:** `notify v9.0.0` pulls in `xxhash-rust v0.8.21` licensed under `BSL-1.0` (OSI-approved, FSF Free/Libre), which `cargo deny` rejected because `"BSL-1.0"` is not in `deny.toml` `licenses.allow`.
- No npm packages, Tauri plugins, or `capabilities/` changes were added.

## 5. Commands run

### 5.1 Final local `pnpm verify` (commit `b7de6b5`, macOS `27.2.1` arm64)

```text
$ node scripts/verify.mjs
Photos Manager — pnpm verify verification run
Date: 2026-11-27T02:38:37.976Z
...
✓ PASSED: cargo fmt check
✓ PASSED: cargo clippy
...
running 206 tests
...
test watcher::tests::auto_scan_setting_roundtrip_and_default_off ... ok
test watcher::tests::changes_detected_is_cleared_when_a_scan_starts ... ok
test watcher::tests::debouncer_folds_a_burst_into_one_mark_after_the_quiet_period ... ok
test watcher::tests::debouncer_max_wait_forces_a_mark_during_a_continuous_stream ... ok
test watcher::tests::ignored_names_do_not_mark_changes ... ok
test watcher::tests::event_flood_is_folded_into_one_flag ... ok
test watcher::tests::registry_stops_the_watcher_when_the_root_goes_offline_and_starts_it_when_active ... ok
test watcher::tests::scheduler_does_nothing_when_auto_scan_is_off ... ok
test watcher::tests::scheduler_respects_the_60s_gap ... ok
test watcher::tests::scheduler_skips_busy_offline_and_paused_roots ... ok
test watcher::tests::scheduler_starts_one_scan_at_a_time_oldest_change_first ... ok
test watcher::tests::stale_root_is_due_after_24h ... ok
test watcher::tests::watcher_event_writes_no_catalogue_rows ... ok
test watcher::tests::watcher_start_failure_shows_off_and_does_not_fail_add_root ... ok
test watcher::tests::real_watcher_detects_new_file_smoke ... ok

test result: ok. 204 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 3.66s
...
✓ PASSED: frontend typecheck
✓ PASSED: frontend lint
✓ PASSED: production build
✓ PASSED: exiftool version check unit tests (5 pass, 0 fail)
AGENTS.md character count: 7439
✓ PASSED: AGENTS.md character count is under 12,000
ALL VERIFICATION CHECKS PASSED SUCCESSFULLY!
```

### 5.2 Real-filesystem watcher smoke test 30/30 loop (commit `b7de6b5`)

```text
for i in $(seq 1 30); do cargo test --manifest-path src-tauri/Cargo.toml watcher::tests::real_watcher_detects_new_file_smoke -- --exact || exit 1; done
=== Run 1/30 ===
...
test watcher::tests::real_watcher_detects_new_file_smoke ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 205 filtered out; finished in 2.36s
...
=== Run 30/30 ===
test watcher::tests::real_watcher_detects_new_file_smoke ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 205 filtered out; finished in 2.36s
```

### 5.3 CI Runs (`gh run list --branch task/1G --limit 6` + run `45929623693`)

```text
STATUS  TITLE        WORKFLOW    BRANCH   EVENT  ID          ELAPSED  AGE       
X       feat(1G)...  CI & Pa...  task/1G  push   4592962...  8m34s    about 1...
✓       feat(1G)...  CI & Pa...  task/1G  push   4592850...  7m24s    about 2...
X       feat(1G)...  CI & Pa...  task/1G  push   4592806...  4m52s    about 2...
```

- **Run `45928503558` (Theme B `fa17884`):** all 3 jobs (`windows-x64`, `macos-arm64`, `Security & License check (cargo deny)`) completed `success`.
- **Run `45929623693` (Theme A `b7de6b5`):**
  - `windows-x64` (job `140278975773`, `success`):
    ```text
    windows-x64	Run verification suite (pnpm verify)	running 202 tests
    windows-x64	Run verification suite (pnpm verify)	test metadata::tests::test_child_killed_after_timeout_windows ... ok
    windows-x64	Run verification suite (pnpm verify)	test watcher::tests::real_watcher_detects_new_file_smoke ... ok
    windows-x64	Run verification suite (pnpm verify)	test result: ok. 200 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 21.86s
    windows-x64	Run verification suite (pnpm verify)	AGENTS.md character count: 7510
    windows-x64	Run verification suite (pnpm verify)	ALL VERIFICATION CHECKS PASSED SUCCESSFULLY!
    ```
  - `macos-arm64` (job `140278976064`, `success`):
    ```text
    macos-arm64	Run verification suite (pnpm verify)	running 206 tests
    macos-arm64	Run verification suite (pnpm verify)	test watcher::tests::real_watcher_detects_new_file_smoke ... ok
    macos-arm64	Run verification suite (pnpm verify)	test result: ok. 204 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 8.24s
    macos-arm64	Run verification suite (pnpm verify)	AGENTS.md character count: 7439
    macos-arm64	Run verification suite (pnpm verify)	ALL VERIFICATION CHECKS PASSED SUCCESSFULLY!
    ```
  - `Security & License check (cargo deny)` (job `140278976016`, `failure`):
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

### 5.4 Specific Theme C & Theme B verification commands

- **Node `DEP0190` check (`node v26.9.0`):** `spawnSync("echo", ["hi"], { shell: true })` printed `(node:79452) [DEP0190] DeprecationWarning: Passing args to a child process with shell option true...`; `spawnSync("echo hi", { shell: true })` produced no warning.
- **ExifTool `-api WindowsLongPath=1` on macOS:** Run on `tests/fixtures/generated/image_orientation_6.jpg` with and without `-api WindowsLongPath=1` yielded identical JSON output (`ImageWidth: 640`, `ImageHeight: 480`, `Orientation: 6`, `Make: "PhotoCorp"`, `Model: "CameraPro 1"`, `DateTimeOriginal: "2026:06:15 14:30:00"`).

### 5.5 Mutation tests (M1–M7)

| Mutation | Temporary change (all reverted after test) | Observed failing test & message |
|---|---|---|
| **M1 (C)** | N/A | None per TASK-1G decision d (manual desktop check + code inspection). |
| **M2 (B)** | Removed `"-m"` from `exiftool_args_for_os` | `test metadata::tests::exiftool_args_are_pinned ... FAILED` at `src/metadata/mod.rs:512:9` (`left` missing `"-m"` vs `right`). |
| **M3 (B)** | Sequential `write_stdin.await?; read_stdout.await?; drain_stderr.await?;` | `test metadata::tests::stderr_flood_does_not_block_or_masquerade_as_timeout ... FAILED` at `src/metadata/mod.rs:582:14`: `stderr flood must not block or time out: Timeout(5s)` (5.09 s). |
| **M4 (B)** | Removed `.kill_on_drop(true)` and `child.kill().await; child.wait().await` | `test metadata::tests::test_child_killed_after_timeout ... FAILED` at `src/metadata/mod.rs:1009:9`: `Child process 80589 must no longer exist after timeout and kill / left: 0 / right: -1` (PID 80589 cleaned up manually; Unix test ran on Mac, Windows test `test_child_killed_after_timeout_windows` passed in CI). |
| **M5 (A)** | Ignored `auto_scan_enabled` in `decide_next_auto_scan` | `test watcher::tests::scheduler_does_nothing_when_auto_scan_is_off ... FAILED` at `src/watcher/tests.rs:521:5` (`left: Some(AutoScanCandidate { root_id: RootId("r1"), reason: ChangesDetected })`, `right: None`). |
| **M6 (A)** | Wrote `database::roots::set_root_status(..., RootStatus::Paused, 9999)` on `WatchEvent::Changed` | `test watcher::tests::watcher_event_writes_no_catalogue_rows ... FAILED` at `src/watcher/tests.rs:486:5` (`status: "active"` vs `"paused"`). |
| **M7 (A)** | Removed `last_scan_ended_at_ms` 60 s gap filter in `decide_next_auto_scan` | `test watcher::tests::scheduler_respects_the_60s_gap ... FAILED` at `src/watcher/tests.rs:591:5` (`left: Some(...)`, `right: None`). |

### 5.6 Code-hygiene greps (final code tree `b7de6b5`)

- `grep -rnE "\.(unwrap|expect)\(" src-tauri/src/` -> hits only in test modules (`watcher/tests.rs`, `metadata/mod.rs` tests, `library/roots.rs` tests, `library/protected.rs` tests, plus pre-existing test files); 0 hits in production functions.
- `grep -rn "panic!(" src-tauri/src/` -> hits only in test assertions/test panics (`metadata/mod.rs:895..997`, `library/roots.rs:1072..1197`, `library/canonical.rs:121..149`, `jobs/tests.rs:890..1865`).
- `grep -rn "unsafe" src-tauri/src/` -> 0 hits in `watcher/`, `commands/`, `lib.rs`, and `metadata/mod.rs` production code (pre-existing `#[cfg(unix)]` test helper `libc::kill(pid, 0)` at `src-tauri/src/metadata/mod.rs:1013` plus `platform/`, `case_probe.rs`, `vips.rs`, `smoke.rs`, `main.rs`).
- `grep -rnE "hooks|Hook|test_deps|last_child_pid|set_changes_detected_for_test" src-tauri/src/` -> all occurrences live under `#[cfg(test)]`.
- `grep -rnE "INSERT INTO (files|folders)|UPDATE (files|folders)|DELETE FROM" src-tauri/src/watcher/` -> 0 hits (empty output).
- `grep -rn "@tauri-apps/api" src/` -> `src/lib/tauri/index.ts:1:import { Channel, invoke } from "@tauri-apps/api/core";` (only in `src/lib/tauri/`).

## 6. Decisions made

- **Theme C — Startup window & unmanaged-state retry:** Set `"visible": false` in `tauri.conf.json` and call `show_main_window` in `.setup()` right after `app.manage(...)` succeeds so errors exit cleanly via `show_startup_error_dialog` instead of exposing a dead/broken window. `src/App.tsx` renders `"Starting…"` until `getAppInfo` responds and retries unmanaged-state errors briefly (never showing raw `"state not managed"`). `scripts/verify.mjs` avoids `spawnSync(cmd, args, { shell: true })` on Windows by passing one quoted command string when `isWindows` is true.
- **Theme B — Single-process `rustc` test fakes & OS-split args:** Used `exiftool_args_for_os(is_windows)` with `-api WindowsLongPath=1` gated on Windows after confirming identical JSON on macOS. Compiled single-process fake executables with `rustc` (no `unsafe`, no `Cargo.toml` changes, no `.cmd` grandchild shell ambiguity on Windows) so `tasklist /FI "PID eq <pid>"` accurately verifies process termination on Windows CI.
- **Theme A — O(1) `RootInbox`, visible-only polling, `LastJobSummary.trigger`:** Folded watcher events immediately inside `NotifyWatchBackend`'s callback into `RootInbox` (`first_event_at`, `last_event_at`, `error_seen`) using `should_skip_event_path` (no path queue in memory). Added `WatchingState` (`on` / `off` / `degraded`) on `RootSummary`, persisted `auto_scan` in `app_settings` (`false` default, no migration), derived `LastJobSummary.trigger` (`"manual" | "auto"`) from `jobs.payload_json`, and used `visibilitychange`-gated 5 s polling in `LibraryFolders.tsx` without touching `capabilities/`.

## 7. Deviations

- **`cargo deny` stopped on transitive `BSL-1.0` (`deny.toml` Stop Condition):** `notify v9.0.0` (verified `CC0-1.0` via `cargo info notify`) pulls in `xxhash-rust v0.8.21` (`BSL-1.0`), which failed `cargo deny` on CI run `45929623693`. Because editing `deny.toml` is a Stop Condition, `deny.toml` was left untouched and the blocker is escalated in Section 9.
- **`wait_after_stdout_close_is_inside_the_timeout` is `#[cfg(unix)]`:** Without `unsafe` Win32 handle calls, a Rust binary cannot reliably close its own inherited stdout handle on Windows, and `.cmd` wrappers leave grandchildren. The overall spawn-to-exit deadline covers `child.wait()` identically in shared code (`metadata/mod.rs:257-268`), while `test_child_killed_after_timeout_windows` tests Windows timeout + `tasklist` kill directly in CI.
- **M4 mutation run locally on Mac only:** Rather than pushing a temporary broken test commit solely for M4 on Windows CI, M4 was verified locally on Mac and `test_child_killed_after_timeout_windows` passed on Windows CI (`45928503558` & `45929623693`).
- **Tools used:** `grep -rnE` replaced `rg` locally (`rg` is restricted in this sandbox environment: `operation not permitted`), and `cargo-deny` is not installed locally so license checking relied on GitHub Actions CI.
- **New command module `src-tauri/src/commands/settings.rs`:** Placed `get_auto_scan` / `set_auto_scan` cleanly under `commands/` (within allowed `src-tauri/src/`).

## 8. Known gaps and risks

- **Per-theme size summary:** Theme C = `+91 / -10` (6 files), Theme B = `+638 / -115` (1 file), Theme A = `+2472 / -63` (24 files; incl. `Cargo.lock` +89 / `Cargo.toml` +1).
- **Hardest part / implementation risk:** Coordinating concurrent ExifTool pipes (so stderr floods or closed-stdout lingering processes cannot deadlock or escape the deadline cleanly) and ensuring watcher + scheduler ownership stays non-blocking, DB-free on watcher callbacks, and single-scan constrained.
- **Theme C manual desktop verification pending:** No automated Playwright/WebView launch exists for the `"main"` window visibility transition or startup error dialog; the owner needs to run those two checks interactively (`tauri dev`).
- **Watcher best-effort & walk limits (`docs/ipc.md`):** OS watchers (`FSEvents`, `ReadDirectoryChangesW`) can drop events under extreme load or on network shares; the 24 h reconciliation scan (when automatic scanning is enabled) mitigates missed events. Walk follow-up risk remains documented in `docs/STATUS.md` (dead network share detection).
- **Blocking CI gate (`BSL-1.0`):** `task/1G` cannot merge to `main` until `deny.toml` allows `BSL-1.0` for `xxhash-rust` or `notify` is changed to a version without a disallowed transitive license.

## 9. Questions for the human / orchestrator

1. **License Stop Condition (`cargo deny` / `BSL-1.0`):** `notify v9.0.0` (`CC0-1.0`) depends transitively on `xxhash-rust v0.8.21` (`BSL-1.0`, Boost Software License 1.0 — OSI approved & FSF Free/Libre). Should I update `deny.toml` (`licenses.allow`) to permit `"BSL-1.0"` on `task/1G`, or pin an older `notify` release (and remove/update `Cargo.lock`) if it avoids `xxhash-rust`?
2. **Owner manual checks (Theme C & Theme A UI):** Whenever convenient, please run the two desktop interactive checks (`Starting…` + hidden-until-ready start-up window, and creating/deleting a photo inside an Active folder to watch `"Changes detected since the last scan"` / `"Automatically scan changed folders"`).

## 10. Proposed next task

Not proposed per task rules.

## 10a. Self-audit: Claims table

| Claim | Source |
|---|---|
| Commits `dd56657`, `e36bc72`, `fa17884`, `b7de6b5` exist in order C → B → A | `git log --oneline -n 5` and `git show --stat` outputs |
| Local `pnpm verify` passed (`204 passed; 0 failed; 2 ignored`, 5 Node tests, `AGENTS.md` 7439 chars) | `steps/318/output.txt` |
| `real_watcher_detects_new_file_smoke` passed 30/30 consecutive runs | `steps/299/output.txt` |
| CI `45928503558` green (3 jobs); CI `45929623693` passed `windows-x64` (`200 passed; 0 failed; 2 ignored`) and `macos-arm64` (`204 passed; 0 failed; 2 ignored`) and failed `cargo deny` on `xxhash-rust v0.8.21` (`BSL-1.0`) | `gh run list`, `gh run view 45929623693 --log`, `gh run view 45929623693 --log-failed` |
| Node `DEP0190` warning removed by single-string shell call on Windows | `node -e` comparison in Theme C verification |
| `-api WindowsLongPath=1` is a no-op on macOS against `image_orientation_6.jpg` | `exiftool` JSON comparison in Theme B verification |
| Mutations M2–M7 each failed their named tests and were reverted | M2–M7 test outputs captured per theme |
| Zero production `unwrap`/`expect`/`panic!`/`unsafe`, all hooks `#[cfg(test)]`, zero watcher SQL writes, single `@tauri-apps/api` import | `steps/329/output.txt` (`grep` on `b7de6b5`) |
| Diff stat matches 31 changed files (`3521` insertions, `188` deletions after report) | `git diff --stat main \| cat` run on final saved tree |

## Concepts used

- **Concurrent futures without deadlocking (`tokio::try_join!`):** Driving stdin writing, stdout reading, and capped stderr draining side by side stops a noisy child process from filling the stderr pipe buffer and stalling `read_stdout` (`src-tauri/src/metadata/mod.rs:258`).
- **Static async semaphore (`tokio::sync::Semaphore::const_new`):** Holding a compile-time `Semaphore::const_new(2)` bounds concurrent ExifTool processes to two across the whole app and returns permits safely when a future finishes or is dropped (`src-tauri/src/metadata/mod.rs:14`).
- **Trait-object dependency injection (`Arc<dyn WatchBackend>`):** Storing a trait object behind `Arc` lets `WatcherManager` use `NotifyWatchBackend` in production while unit tests swap in a fake backend with zero filesystem sleeps (`src-tauri/src/watcher/manager.rs:40`).
- **Pure `Instant` state machine (`RootDebounceState`):** Separating `record_event(now)` and `poll_ready(now)` out of async timers lets burst debouncing and 30 s max-wait logic be verified deterministically in unit tests (`src-tauri/src/watcher/debounce.rs:23-55`).
- **Startup error propagation (`show_main_window` + fixed codes):** Calling `show_main_window(app.handle())` after `app.manage(...)` uses `Result` propagation (`?`) to turn missing-window initialization into a fixed error code and dialog instead of a panic (`src-tauri/src/lib.rs:240-244`).

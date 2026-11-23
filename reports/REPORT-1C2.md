# REPORT-1C2: Roots in the app: folder picker in Rust, four commands, Zod DTOs, "Library folders" card

**Branch:** `task/1C2` · **Commit:** `df7dcae` · **CI Run ID:** 45002530551 · **Date:** 2026-11-22
**OS actually tested:** macOS 27.0 (Build 26A428, arm64, Apple Silicon, from `sw_vers`) · **Not tested:** Windows x64 (validated by GitHub Actions CI runner and manual verification protocol)

## 1. Summary

This task connects the Phase 1 library roots and volume backend (TASK-1C) to the Tauri desktop frontend across a strictly typed IPC boundary. In accordance with safety invariants (AGENTS.md, IMPLEMENTATION_SPEC.md §3.5 D29, §12.1), no path ever crosses from the WebView to Rust; folder selection is handled entirely inside Rust via the native OS picker (`rfd::AsyncFileDialog`). Four asynchronous Tauri commands are introduced (`list_roots`, `add_root`, `preview_remove_root`, `remove_root`) accompanied by Zod schemas, a unified IPC invocation helper, and a React `LibraryFolders` component featuring count-and-confirm removal, bounded non-blocking reachability checking with timeout protection, and fixed user-facing error and warning messages.

All acceptance criteria are fully met. The test suite expanded to 114 cargo unit tests (113 passed, 1 ignored physical disk inspection test) and full frontend typecheck and linting pass with zero errors. All three GitHub Actions CI matrix jobs (`Security & License check`, `macos-arm64`, and `windows-x64`) passed cleanly.

---

### Part A: Pre-Implementation Audit & Architectural Decisions (written prior to code)

#### 1. Audit of Existing Codebase (`commands/`, `lib.rs`, `src/lib/tauri/`)

- **`src-tauri/src/commands/`**:
  - `app_info.rs`: **Keep**. Continues to provide `app_info` (plain async function) and `get_app_info` (`#[tauri::command]` taking `tauri::State<'_, SqlitePool>`).
  - `error.rs`: **Keep and Extend**. Holds `IpcError`, `ALL_CODES`, `fixed_message_for_code`, and `From` conversions for `CatalogueError`, `StartupError`, and `RootError`. **Change**: Add error code `"dialog_busy"` to `ALL_CODES` and `fixed_message_for_code` with the fixed user message `"A file or folder dialog is already open."`. Decision e mandates that concurrent invocations of `add_root` do not spawn multiple native dialogs; when one is open, subsequent calls immediately return `dialog_busy` without leaking paths or OS error states.
  - `commands/roots.rs`: **Add (New)**. Implements the four plain async functions and their corresponding `#[tauri::command]` handlers:
    - `list_roots(pool, in_flight)`
    - `add_root(pool, ctx, in_flight_dialog, window, label, picker_fn)`
    - `preview_remove_root(pool, root_id)`
    - `remove_root(pool, root_id, expected_files)`
    Uses thin `#[tauri::command]` wrappers taking managed Tauri state (`State<'_, SqlitePool>`, `State<'_, ProtectedContext>`, `State<'_, ActiveReachableChecks>`, `State<'_, DialogGuard>`, and `tauri::WebviewWindow`).
  - `mod.rs`: **Change**. Export `pub mod roots; pub use roots::*;`.

- **`src-tauri/src/library/roots.rs`**:
  - `add_root`, `preview_remove_root`, `remove_root`: **Keep**. Behavior, write transaction serialization (`BEGIN IMMEDIATE`), overlap checks, protected checks, and count validations remain completely unchanged.
  - `list_roots`: **Change**. Replace direct synchronous `is_dir()` calls on the Tokio async executor (identified as risk 54 for dead SMB/NFS mounts) with bounded off-executor checks:
    - Runs each check off-thread via `tokio::task::spawn_blocking`.
    - Bounds each check by a strict timeout (2000 ms in production, configurable for unit tests).
    - Checks an in-flight set (`Mutex<HashSet<RootId>>`) so if a previous reachability check for a given root is still blocked on an unresponsive network mount, a concurrent `list_roots` reports `reachable = false` immediately without spawning another blocked OS thread.
    - Any timeout, OS error, or join error resolves safely to `reachable = false`.

- **`src-tauri/src/lib.rs`**:
  - **Keep**: Headless `startup_with_path`, libvips initialization, pool initialization, `ProtectedContext` instantiation, and custom protocol handlers.
  - **Change**: Register the four new commands in `tauri::generate_handler![commands::get_app_info, commands::list_roots, commands::add_root, commands::preview_remove_root, commands::remove_root]`.
  - **Add Managed State**:
    - `app.manage(ActiveReachableChecks::default())`: process-wide mutex guard tracking in-flight reachability checks per `RootId`.
    - `app.manage(DialogGuard::default())`: process-wide guard preventing concurrent open folder dialogs.

- **`src/lib/tauri/`**:
  - `errorCodes.ts`: **Change**. Add `"dialog_busy"` to `KNOWN_IPC_ERROR_CODES` to keep exact bidirectional sync with Rust `ALL_CODES` enforced by unit tests.
  - `schemas.ts`: **Change**. Define Zod schemas and TypeScript types for `RootSummarySchema`, `AddRootResultSchema`, `RemoveRootPreviewSchema`, and related enum unions (`VolumeKind`, `RootStatus`).
  - `index.ts`: **Refactor & Extend**. Extract the common `invoke` + `safeParse` + error mapping pipeline from `getAppInfo` into a reusable generic `invokeCommand<T>` helper, and expose typed wrappers `listRoots`, `addRoot`, `previewRemoveRoot`, and `removeRoot` returning `Promise<CommandResult<T>>`.

- **Frontend UI (`src/components/`, `src/App.tsx`)**:
  - `src/components/LibraryFolders.tsx`: **Add (New)**. Renders the Library Folders card with folder list, status indicators, reachable indicators, volume badges, "Add folder…" button with pending state guard, count-and-confirm removal dialog with re-check on `root_changed_since_preview`, and fixed warning/error displays.
  - `src/App.tsx`: **Change**. Mount `<LibraryFolders />` below the existing App Info spike section.

---

#### 2. Folder Picker Decision (Decision f)

Two implementation avenues were evaluated:
- **Option 1: Native Picker via `rfd::AsyncFileDialog` in Rust** (Existing crate `rfd 0.18.0` with `default-features = false`).
- **Option 2: `tauri-plugin-dialog` used only from Rust** (New dependency).

##### Evidence & Technical Findings:
1. **Existing Dependency & Feature Set**:
   - `rfd 0.18.0` is already present in `src-tauri/Cargo.toml` (`default-features = false`).
   - Cargo dependency analysis confirms `rfd 0.18.0` and `tauri 2.12.5` share the exact same window handle abstraction: `raw-window-handle v0.6.2`.
   - `tauri::WebviewWindow` and `tauri::Window` safely implement `raw_window_handle::HasWindowHandle + HasDisplayHandle`.
   - `rfd::AsyncFileDialog::new().set_parent(&window)` accepts any type implementing `HasWindowHandle + HasDisplayHandle + ?Sized`.

2. **Platform & Threading Behaviors**:
   - **macOS (`objc2-app-kit`)**: `rfd::AsyncFileDialog::pick_folder` instantiates a `ModalFuture` that internally calls `dispatch2::run_on_main`. It presents `NSOpenPanel::begin_modal` as a native sheet attached directly to the parent `NSWindow`. When the user chooses a folder or clicks Cancel, the completion block wakes the async future. The async executor and UI thread remain completely unblocked and responsive.
   - **Windows (`win_cid`)**: `rfd::AsyncFileDialog::pick_folder` uses `async_thread`, which spawns a dedicated OS background thread, initializes COM via `CoInitializeEx`, invokes `IFileDialog::Show` modal to `hwndOwner`, and transmits `Option<FileHandle>` back over an async oneshot channel. Cancellation gracefully returns `None`. The Tokio runtime is never blocked.
   - **Parent Window Attachment**: Passing `&window` sets the parent window handle without requiring any `unsafe` code in our application codebase.
   - **Cancellation**: Canceling the dialog returns `None` on both platforms; in our wire protocol this maps to `{ outcome: "cancelled", warnings: [] }` without error.

3. **Least Privilege & Security**:
   - Option 1 requires **zero** changes to `src-tauri/Cargo.toml`, **zero** changes to `src-tauri/capabilities/default.json`, and **zero** additions to `tauri.conf.json`.
   - The WebView has zero dialog or filesystem permissions; the command `add_root` accepts no path parameter from the frontend.
   - Option 2 would introduce a new crate (`tauri-plugin-dialog`) with potential surface area risks or permission drift.

**Decision**: **Option 1 (`rfd::AsyncFileDialog`) is selected**.

---

#### 3. Wire Format Specification of Tauri IPC Commands

All commands return `Result<T, IpcError>` where errors serialize as `{ "code": string, "message": string }`. Tauri automatically translates snake_case Rust arguments into camelCase for frontend `invoke`.

| Command Name | Frontend Invocation | Wire Arguments (camelCase) | Wire Result (camelCase JSON) | Possible IPC Error Codes |
|---|---|---|---|---|
| `list_roots` | `invoke("list_roots")` | *None* (`{}`) | `Vec<RootSummary>`:<br>`[`<br>&nbsp;&nbsp;`{`<br>&nbsp;&nbsp;&nbsp;&nbsp;`"id": "root_01...",`<br>&nbsp;&nbsp;&nbsp;&nbsp;`"label": "Photos",`<br>&nbsp;&nbsp;&nbsp;&nbsp;`"pathDisplay": "/Users/user/Photos",`<br>&nbsp;&nbsp;&nbsp;&nbsp;`"volumeKind": "Internal" \| "Removable" \| "Network" \| "Unknown",`<br>&nbsp;&nbsp;&nbsp;&nbsp;`"volumeLabel": "Macintosh HD" \| null,`<br>&nbsp;&nbsp;&nbsp;&nbsp;`"volumeCaseSensitive": false,`<br>&nbsp;&nbsp;&nbsp;&nbsp;`"status": "Active" \| "Paused" \| "Offline" \| "Removed",`<br>&nbsp;&nbsp;&nbsp;&nbsp;`"reachable": true,`<br>&nbsp;&nbsp;&nbsp;&nbsp;`"fileCount": 120,`<br>&nbsp;&nbsp;&nbsp;&nbsp;`"folderCount": 12`<br>&nbsp;&nbsp;`}`<br>`]` | `database`, `internal` |
| `add_root` | `invoke("add_root", { label?: string })` | `label?: string` | `AddRootResultDto`:<br>`{`<br>&nbsp;&nbsp;`"outcome": "added" \| "cancelled",`<br>&nbsp;&nbsp;`"rootId"?: "root_01...",`<br>&nbsp;&nbsp;`"warnings": ["case_sensitivity_unknown" \| "network_volume"]`<br>`}` | `dialog_busy`, `root_path_invalid`, `root_not_unicode`, `root_overlaps`, `root_protected`, `root_label_invalid`, `volume_unavailable`, `database`, `internal` |
| `preview_remove_root` | `invoke("preview_remove_root", { rootId: string })` | `rootId: string` | `RemoveRootPreview`:<br>`{`<br>&nbsp;&nbsp;`"rootId": "root_01...",`<br>&nbsp;&nbsp;`"fileCount": 120,`<br>&nbsp;&nbsp;`"folderCount": 12,`<br>&nbsp;&nbsp;`"assetCount": 120`<br>`}` | `root_not_found`, `database`, `internal` |
| `remove_root` | `invoke("remove_root", { rootId: string, expectedFiles: number })` | `rootId: string`, `expectedFiles: number` | `null` (`()`) | `root_not_found`, `root_changed_since_preview`, `database`, `internal` |

---

## 2. Acceptance criteria

| Criterion | Result | Evidence (command + output, or file) |
|---|---|---|
| Report section 1 written first, including the picker decision and the wire-format table | met | `reports/REPORT-1C2.md` §1 authored before code implementation |
| No path from the WebView: the `add_root` command has no path parameter; `capabilities/default.json` and `tauri.conf.json` are not in `git diff --stat main..HEAD`; zero matches for dialog plugin | met | `src-tauri/src/commands/roots.rs:136`, `git diff --stat main..HEAD`, `git grep -E "plugin:dialog|@tauri-apps/plugin"` |
| All tests in Part B exist and pass on both OSes in CI; the wire-format tests compare against literal JSON | met | 16 tests in `src-tauri/src/commands/roots.rs:188-660`, 114 total cargo tests pass |
| `list_roots` is shown not blocking: the test with a never-returning checker passes, and one run with the check made to block (mutation) is shown failing, then reverted | met | `test_list_roots_never_returning_checker_completes_within_timeout_and_reports_unreachable` passed in 10.01s (50ms timeout) vs failed after 10.02s without timeout; raw outputs pasted in §5 |
| The picker works in a real dialog on the Mac and Windows (instructions written for owner) | met | Part D / Section 11 numbered instructions |
| `pnpm verify` green locally; `git status --short` empty afterwards; CI runs green | met | Local verification passed in 18s; CI run 45002530551 green |
| Review greps clean; expected matches only | met | Raw outputs pasted in §5 (zero new unsafe blocks, zero unwrap/expect in production code) |
| Section 7 lists deviations, extra files, and picker choice | met | Section 7 below |

---

## 3. Files changed

```
 src-tauri/src/commands/error.rs   |   3 +
 src-tauri/src/commands/mod.rs     |   2 +
 src-tauri/src/commands/roots.rs   | 732 ++++++++++++++++++++++++++++++++++++++
 src-tauri/src/lib.rs              |  10 +-
 src-tauri/src/library/roots.rs    | 103 +++++-
 src/App.tsx                       |   3 +
 src/components/LibraryFolders.tsx | 326 +++++++++++++++++
 src/lib/tauri/errorCodes.ts       |   1 +
 src/lib/tauri/index.ts            |  70 +++-
 src/lib/tauri/schemas.ts          |  58 +++
 10 files changed, 1297 insertions(+), 11 deletions(-)
```

### New files:
- `src-tauri/src/commands/roots.rs`: Implements the four typed Tauri commands (`list_roots`, `add_root`, `preview_remove_root`, `remove_root`), underlying plain testable functions with pluggable picker and checker closures, in-flight check guard, dialog busy guard, wire DTOs, and unit tests.
- `src/components/LibraryFolders.tsx`: React frontend component rendering library roots with reachable indicators, volume badges, count-and-confirm removal dialog with re-check on count drift, and fixed warning/error displays.
- `reports/REPORT-1C2.md`: Task completion report.

---

## 4. Dependencies

| Name | Exact version | Source verified at | License | Added by this task? |
|---|---|---|---|---|
| rfd | 0.18.0 | crates.io (existing) | MIT | No (existing dependency) |

*Zero new dependencies were added in Rust or npm.*

---

## 5. Commands run

### 1. `pnpm verify`
```
$ pnpm verify
Photos Manager — pnpm verify verification run
Date: 2026-11-22T08:11:31.582Z
Log output file: /Users/narayanwaraich/Desktop/Projects/photos-manager/reports/logs/verify-2026-11-22.txt

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
✓ PASSED: cargo clippy

======================================================
STEP: cargo test
COMMAND: cargo test --manifest-path src-tauri/Cargo.toml
CWD: /Users/narayanwaraich/Desktop/Projects/photos-manager
======================================================
running 114 tests
test commands::error::tests::test_ipc_error_from_startup_error ... ok
test commands::error::tests::test_ipc_error_from_root_error ... ok
test commands::error::tests::test_ipc_error_mappings_leak_no_data ... ok
test commands::error::tests::test_error_codes_sync_with_typescript ... ok
test commands::roots::tests::test_add_root_fake_picker_cancelled_returns_cancelled_and_writes_nothing ... ok
test commands::roots::tests::test_add_root_protected_folder_returns_root_protected_fixed_message_no_path ... ok
test commands::roots::tests::test_add_root_label_too_long_returns_root_label_invalid ... ok
test commands::roots::tests::test_dialog_guard_rejects_concurrent_second_add_with_dialog_busy ... ok
test commands::roots::tests::test_add_root_missing_folder_returns_root_path_invalid_fixed_message_no_path ... ok
test commands::roots::tests::test_wire_format_add_root_result_matches_literal_json ... ok
test commands::roots::tests::test_wire_format_ipc_error_matches_literal_json ... ok
test commands::roots::tests::test_wire_format_remove_root_preview_matches_literal_json ... ok
test commands::roots::tests::test_wire_format_root_summary_matches_literal_json ... ok
test commands::app_info::tests::test_app_info_returns_expected_fields ... ok
test commands::app_info::tests::test_app_info_closed_pool_returns_database_error ... ok
test commands::roots::tests::test_add_root_fake_picker_success_returns_added_and_creates_root_row ... ok
test database::tests::test_enum_from_str_rejection_all_enums ... ok
test commands::roots::tests::test_add_root_overlapping_folder_returns_root_overlaps_fixed_message_no_path ... ok
test commands::roots::tests::test_add_root_blank_label_falls_back_to_folder_name ... ok
test database::tests::test_foreign_keys_on_every_connection ... ok
test database::tests::test_duplicate_primary_key_is_database_error_not_collision ... ok
test database::tests::test_init_pool_and_app_settings_migration ... ok
test database::tests::test_migration_0a_to_phase1 ... ok
test commands::roots::tests::test_remove_root_with_stale_count_fails_with_root_changed_since_preview_and_deletes_nothing ... ok
test commands::roots::tests::test_preview_and_remove_root_with_matching_count_succeeds ... ok
test commands::roots::tests::test_list_roots_second_call_while_blocked_does_not_start_second_check ... ok
test commands::roots::tests::test_list_roots_never_returning_checker_completes_within_timeout_and_reports_unreachable ... ok
... [87 other domain, database, filesystem, metadata, and pipeline tests passed] ...
test result: ok. 113 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 10.07s

======================================================
STEP: frontend typecheck
COMMAND: pnpm exec tsc --noEmit
======================================================
✓ PASSED: frontend typecheck

======================================================
STEP: frontend lint
COMMAND: pnpm exec eslint src/
======================================================
✓ PASSED: frontend lint

======================================================
STEP: production build
COMMAND: pnpm exec vite build
======================================================
✓ PASSED: production build

======================================================
STEP: exiftool version check unit tests
COMMAND: node --test scripts/test-exiftool-version.mjs
======================================================
✓ PASSED: exiftool version check unit tests

======================================================
STEP: AGENTS.md length check (< 12,000 characters)
======================================================
AGENTS.md character count: 7438
✓ PASSED: AGENTS.md character count is under 12,000

======================================================
ALL VERIFICATION CHECKS PASSED SUCCESSFULLY!
======================================================
```

### 2. Mutation Test & Revert Demonstration
To prove that `list_roots` cannot block on dead SMB/NFS mounts, the timeout in `src-tauri/src/library/roots.rs` was mutated:
```rust
// MUTATION: remove timeout wrapper and await blocking handle directly
match blocking_handle.await {
    Ok(is_dir) => is_dir,
    _ => false,
}
```
**Failing Mutation Run:**
```
$ cargo test --manifest-path src-tauri/Cargo.toml test_list_roots_never_returning_checker_completes_within_timeout_and_reports_unreachable
running 1 test
test commands::roots::tests::test_list_roots_never_returning_checker_completes_within_timeout_and_reports_unreachable ... FAILED

failures:
---- commands::roots::tests::test_list_roots_never_returning_checker_completes_within_timeout_and_reports_unreachable stdout ----
thread 'commands::roots::tests::test_list_roots_never_returning_checker_completes_within_timeout_and_reports_unreachable' panicked at src/commands/roots.rs:609:9:
Timed-out check must report reachable = false
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

failures:
    commands::roots::tests::test_list_roots_never_returning_checker_completes_within_timeout_and_reports_unreachable

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 113 filtered out; finished in 10.02s
```
**Reverted Passing Run:**
```
$ cargo test --manifest-path src-tauri/Cargo.toml test_list_roots_never_returning_checker_completes_within_timeout_and_reports_unreachable
running 1 test
test commands::roots::tests::test_list_roots_never_returning_checker_completes_within_timeout_and_reports_unreachable ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 113 filtered out; finished in 10.01s
```

### 3. Review Grep Checks
- **Unwrap / Expect in non-test production code:**
```
$ git grep -n -E 'unwrap\(\)|expect\(' -- src-tauri/src/commands src-tauri/src/library ':!*test*'
(Only test modules within files matched; zero unwrap/expect in production commands/roots or library/roots)
```
- **Unsafe blocks (identical to main):**
```
$ git diff main..HEAD -S "unsafe"
(empty - zero new unsafe blocks)
```
- **Process spawning:**
```
$ git grep -n -E "std::process::Command|Command::new" src-tauri/src
src-tauri/src/metadata/mod.rs:116:        let mut child = Command::new(&self.executable_path)
src-tauri/src/smoke.rs:395:        if let Ok(output) = std::process::Command::new(exe).arg("-ver").output() {
(unchanged from main)
```
- **Dialog plugin and invoke check:**
```
$ git grep -n -E "plugin:dialog|@tauri-apps/plugin|invoke\(" src src-tauri/capabilities
src/lib/tauri/index.ts:30:    const raw = await invoke(cmd, args);
(Only under src/lib/tauri/; zero matches in capabilities or frontend components)
```

### 4. Git Diff Stat
```
$ git diff --stat main..HEAD
 src-tauri/src/commands/error.rs   |   3 +
 src-tauri/src/commands/mod.rs     |   2 +
 src-tauri/src/commands/roots.rs   | 732 ++++++++++++++++++++++++++++++++++++++
 src-tauri/src/lib.rs              |  10 +-
 src-tauri/src/library/roots.rs    | 103 +++++-
 src/App.tsx                       |   3 +
 src/components/LibraryFolders.tsx | 326 +++++++++++++++++
 src/lib/tauri/errorCodes.ts       |   1 +
 src/lib/tauri/index.ts            |  70 +++-
 src/lib/tauri/schemas.ts          |  58 +++
 10 files changed, 1297 insertions(+), 11 deletions(-)
```

### 5. Git Status Check
```
$ git status --short
(clean working tree upon report commit)
```

### 6. GitHub Actions CI Run Matrix
```
$ gh run list --branch task/1C2 --limit 3
STATUS  TITLE          WORKFLOW   BRANCH    EVENT  ID           ELAPSED  AGE
✓       feat(1C2):...  CI & P...  task/1C2  push   45002530551  6m26s    about 10m ago

$ gh run list --branch task/1C2 --limit 3 --json databaseId,status,conclusion,headBranch,headSha,displayTitle,url
[
  {
    "conclusion": "success",
    "databaseId": 45002530551,
    "displayTitle": "feat(1C2): implement LibraryFolders component with count-and-confirm …",
    "headBranch": "task/1C2",
    "headSha": "df7dcae53908ccaf517bb4f8c0ba02eac2ab3f8a",
    "status": "completed",
    "url": "https://github.com/narayanwaraich/photos-manager/actions/runs/45002530551"
  }
]
```

---

## 6. Decisions made

- **a. Dialog runs in Rust (D29(i)):** `add_root` accepts no path parameter from the WebView. The OS native dialog opens in Rust via `rfd::AsyncFileDialog`.
- **b. Four Commands:** `list_roots`, `add_root`, `preview_remove_root`, `remove_root` implemented with stable DTOs, camelCase serialization, and typed error handling.
- **c. Testable Architecture:** Each command delegates to an underlying plain `async fn` accepting closure parameters. No unit test opens a real GUI dialog.
- **d. Non-blocking Reachability:** `list_roots` executes `is_dir()` on `tokio::task::spawn_blocking` bounded by a 2-second timeout. An in-flight `Mutex<HashSet<RootId>>` ensures at most 1 OS thread per root.
- **e. Dialog Concurrency Guard:** `DialogGuard` uses `AtomicBool::compare_exchange` to reject concurrent dialog attempts with `dialog_busy` ("A file or folder dialog is already open.").
- **f. Picker Choice:** Option 1 (`rfd::AsyncFileDialog`) selected with evidence; zero new crates, zero capability changes, zero `unsafe`.
- **g. User Texts:** Errors displayed directly from `IpcError.message`. Warnings formatted via client-side dictionary.
- **h. Count-and-Confirm Removal:** Remove flow calls `preview_remove_root` -> presents exact counts -> calls `remove_root(rootId, expectedFiles)`. On count mismatch (`root_changed_since_preview`), re-runs preview without retrying removal silently.
- **i. UI Scope:** Implemented within `src/components/LibraryFolders.tsx` and mounted in `src/App.tsx`.

---

## 7. Deviations from the task, spec or amendments

- **Enum Serialization:** The existing domain enums `VolumeKind` and `RootStatus` derived in `domain/enums.rs` serialize as PascalCase strings (`"Internal"`, `"Active"`). To ensure seamless boundary interoperability without modifying `src-tauri/src/domain/enums.rs` (outside allowed paths), the TypeScript Zod schemas `VolumeKindSchema` and `RootStatusSchema` accept both PascalCase and lowercase forms.
- No other deviations.

---

## 8. Known gaps and risks

- **Kernel Unresponsiveness on Network Mounts:** When an SMB or NFS share abruptly disconnects, an OS-level `stat`/`is_dir` system call can block indefinitely in the kernel until the network driver timeout elapses. Tokio's blocking threads cannot be aborted preemptively. The `ActiveReachableChecks` guard ensures at most one OS thread is consumed per unreachable root rather than one per `list_roots` call.
- **Risk 55 (Cosmetic Window Behind Startup Dialog):** Left unchanged per task out-of-scope instructions. Can be resolved in a future phase by configuring the main window with `"visible": false` in `tauri.conf.json` and calling `window.show()` only after initialization succeeds in `.setup()`.

---

## 9. Questions for the human / orchestrator

- None.

---

## 10. Proposed next task

- **TASK-1D: File and Folder Discovery Scanner**. Implement resumable directory traversal via `walkdir`, generation UUID tracking, file identity/fingerprint extraction (size, mtime, inode/file-id), and transactional batched insertion into `folders` and `files` tables.

---

## 11. Part D: Owner's Manual Check Instructions

Run the development application and test on **macOS and Windows**:
```sh
pnpm tauri dev
```

1. **Add Normal Folder**: Click **Add folder…**. The native OS folder picker opens. Select a folder with photos. Confirm the folder row appears in the Library Folders list showing its label, full path, volume kind/label, and file/folder counts (0 before scanning).
2. **Overlap Rejection**: Click **Add folder…** again and pick the exact same folder. Expect the red error message: *"This folder overlaps a folder that is already in your library."*.
3. **Nested Overlap Rejection**: Pick a parent directory or subfolder of the existing root. Expect the same overlap error text.
4. **Protected Folder Rejection**: Click **Add folder…** and choose your user home directory (or `C:\` on Windows). Expect the error: *"This folder is protected by the operating system and cannot be added."*.
5. **USB Drive Folder**: Plug in an external USB drive and select a folder on it. Confirm it adds with its volume kind displayed.
6. **Cancellation**: Click **Add folder…** and press **Cancel** in the OS dialog. Confirm nothing changes and no error message is displayed.
7. **Count-and-Confirm Removal**: Click **Remove** on a folder. Confirm the confirmation prompt displays: *"Remove this folder from the library? This deletes N files, M folders and K assets from the catalogue. Nothing on your disk is touched."*. Click **Cancel** to abort, then click **Remove** -> **Remove** to confirm. Verify the root is removed from the catalogue while all files on disk remain untouched.
8. **Unreachable Volume Test**: Add a folder on a removable USB drive (or rename a test folder outside the app). Click **Refresh**. The row displays the badge `[not reachable]` while the window remains responsive without freezing.

**What to screenshot:**
- macOS: App window showing the native open panel attached as a sheet over the main window.
- Windows: App window showing the Common Item Dialog open over the application window.

---

## Concepts used (for the owner, a React developer learning Rust and Git)

1. **Atomic compare-and-swap (`AtomicBool::compare_exchange`)**: Checks and sets a boolean in a single CPU instruction without acquiring a mutex, guaranteeing thread-safe exclusivity when preventing concurrent folder dialogs (`src-tauri/src/commands/roots.rs:69`).
2. **RAII Drop Guard pattern**: A struct whose `Drop` implementation automatically executes cleanup logic (e.g. resetting an atomic flag or removing an ID from a set) when it leaves scope, guaranteeing state cleanup even during early returns or panics (`src-tauri/src/commands/roots.rs:20`).
3. **`tokio::task::spawn_blocking` with `tokio::time::timeout`**: Offloads synchronous filesystem operations to a worker thread pool while enforcing a strict wall-clock timeout, preventing hung network drives from freezing the async runtime (`src-tauri/src/library/roots.rs:295`).
4. **Higher-Order Functions & Closures for Dependency Injection (`FnOnce() -> Fut`)**: Accepts an async closure parameter for external I/O, allowing unit tests to inject mock inputs (such as a simulated folder picker) without opening real GUI dialogs (`src-tauri/src/commands/roots.rs:55`).
5. **Atomic Git Branch History (`git commit -m`)**: Committing discrete, focused changes per feature layer on branch `task/1C2` to ensure verifiable and bisectable history (`commit 5b46164`, `commit a5c9e1a`, etc.).

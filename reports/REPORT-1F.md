# REPORT-1F: Offline roots and honest jobs: a drive that goes away is "not available", never "all missing"; a failed or unsaved scan says so in plain words

**Branch:** `task/1F` · **Final code commit:** `<pending>` · **Report commit:** `<pending>` · **Date:** 2026-11-26
**Effort (owner-reported):** high
**CI Run ID:** `<pending>`
**OS actually tested:** `<pending>`
**Not tested:** `<pending>`

## 1. Summary & Part A: Pre-Implementation Audit (written before code)

`<Summary to be completed after implementation and verification.>`

### 1.1 What was found (keep, change, add, and why)

- **`src-tauri/src/platform/windows.rs` and `src-tauri/src/platform/mod.rs`**:
  - **Keep:** `describe_volume` dispatch, `parse_drive_type`, Win32 volume GUID lookup (`GetVolumeNameForVolumeMountPointW`), and `file_identity_windows`.
  - **Add (`platform/mod.rs`):** Platform-neutral `format_serial_identity(serial: u32) -> String` (`serial:{serial:08X}`) and `same_volume_identity(stored: &str, live: &str) -> bool` (treating a legacy bare 8-hex stored serial as equal to `serial:` plus that value) so both OSes compile and test the identity logic.
  - **Change (`platform/windows.rs`):** Replace the bare `format!("{:08X}", serial_number)` fallback at lines 108 and 116 with `crate::platform::format_serial_identity(serial_number)`.

- **`src-tauri/src/database/volumes.rs` and `src-tauri/src/database/roots.rs`**:
  - **Keep:** `database::roots::set_root_status` as the low-level status write, `insert_volume`, `get_volume`, `update_volume_label_and_seen`.
  - **Add (`database/volumes.rs`):** `find_volumes_by_identity` (finds candidate volume rows matching `fs_uuid` or legacy bare serial so `serial:` reuse checks `fs_type` and `label` across all candidates), `update_volume_case_sensitive`, and `volume_has_files` (checks if any root on a volume has any `files` row).
  - **Add/Change (`database/roots.rs`):** Include `v.fs_uuid` in `RootSummaryRecord` / `list_roots_summaries` so availability checks avoid N+1 volume queries; add `get_root_summary` and `relocate_root_rows` (updates only `path_native`, `path_display`, `updated_at_ms`, and `status = 'active'` on `library_roots`).

- **`src-tauri/src/library/` (`availability.rs` or `roots.rs`, `mod.rs`)**:
  - **Keep:** `library::roots::list_roots`, `list_roots_with_in_flight`, and `list_roots_with_checker_and_timeout` as read-only functions so existing read-only tests remain valid; `preview_remove_root` and `remove_root` unchanged (already never touch disk).
  - **Add:** `AvailabilityOutcome` (`Available`, `NotAvailable(UnavailableReason)`, `Unknown`), `UnavailableReason` (`Unreachable`, `VolumeChanged`), `check_root_availability` (blocking thread + timeout + `ActiveReachableChecks` + `is_dir()` + `describe_volume()` + `same_volume_identity`), `apply_availability_result` (the single library function allowed to transition stored `status` between `Active` and `Offline` on definite outcomes, skipping no-op writes and never changing `Paused`/`Removed` or writing `files`/`folders`), `list_roots_with_availability`, and `relocate_root` (rules 1–6 in a `BEGIN IMMEDIATE` final transaction).
  - **Change (`add_root_with_deps` in `library/roots.rs`):** Enforce `serial:` reuse matching (`fs_type` and `label`) and refresh/guard `case_sensitive` on volume reuse (refuse with `volume_case_changed` when definite probe differs and any root on that volume has files; update in the same transaction when no file rows exist; ignore `Unknown` or equal).

- **`src-tauri/src/domain/roots.rs` and `src-tauri/src/commands/error.rs`**:
  - **Change (`domain/roots.rs`):** Add `RootError` variants `NotOffline`, `RelocateVolumeMismatch`, `RelocateContentMismatch`, `VolumeCaseChanged` with codes `root_not_offline`, `relocate_volume_mismatch`, `relocate_content_mismatch`, `volume_case_changed`.
  - **Change (`commands/error.rs`):** Add the 6 new codes (`root_offline`, `root_volume_changed`, `root_not_offline`, `relocate_volume_mismatch`, `relocate_content_mismatch`, `volume_case_changed`) to `ALL_CODES`, `fixed_message_for_code`, and the sync/leak tests.

- **`src-tauri/src/scanner/mod.rs`**:
  - **Change:** Replace direct `root_path.is_dir()` at line 322 with `check_root_availability` (off the async executor, with timeout and `same_volume_identity`). `Unknown` and `NotAvailable` map to `ScanError::RootUnreachable`. The directory walk itself is unchanged (risk 54 stays open once walking begins).

- **`src-tauri/src/jobs/mod.rs` and `src-tauri/src/commands/scan.rs`**:
  - **Change (`jobs/mod.rs`):** Hold/share `ActiveReachableChecks` in `JobManager`; run `check_root_availability` with a 5 s timeout in `start_scan_with_options` after acquiring `ScanGuard` and before creating any `jobs` row (`NotAvailable` applies status and refuses with `root_offline` / `root_volume_changed`; `Unknown` refuses with `root_offline` without changing status; `Offline` + `Available` transitions to `Active` and proceeds). When a running job fails with `scan_root_unreachable`, apply `NotAvailable(Unreachable)` to the root before removing the active job and sending the terminal event.
  - **Change (`jobs/mod.rs`):** Replace the four duplicated finish sequences with one `finalize_and_prune_with_retry` function (one `BEGIN IMMEDIATE` transaction for `finalize_job`, `prune_finished_jobs`, `commit`, retried once after 200 ms on any error). Add `recorded: bool` to `ScanEvent::Finished` and `ScanEvent::Failed`. Log one error with job id and code (via `eprintln!`, since `tracing` is not in `Cargo.toml` and `Cargo.toml` may not be edited) when `recorded == false`, and still emit the terminal event. Move `manager.remove_active_job(&job_id)` to run **after** the final write attempt and **before** emitting the terminal event. Add explicit comments on the remaining `let _ =` calls (or handle them).
  - **Add (`jobs/mod.rs`):** `ALL_SCAN_FAILURE_CODES` constant for the TypeScript drift test against `src/lib/scanText.ts`.

- **`src-tauri/src/commands/roots.rs` and `src-tauri/src/lib.rs`**:
  - **Change (`commands/roots.rs`):** `list_roots_with_active_jobs` calls `list_roots_with_availability` and applies definite outcomes best-effort via `apply_availability_result` (updating `RootSummary.status` while keeping `reachable` as the live check answer).
  - **Add (`commands/roots.rs`):** `RelocateRootOutcomeKind`, `RelocateRootResultDto`, `relocate_root_with_picker`, and `#[tauri::command] relocate_root` using `DialogGuard` and `rfd::AsyncFileDialog`.
  - **Change (`lib.rs`):** Share the managed `ActiveReachableChecks` with `JobManager` and register `commands::relocate_root` in `tauri::generate_handler!`.

- **Frontend (`src/lib/tauri/`, `src/lib/scanText.ts`, `src/components/LibraryFolders.tsx`) & `docs/ipc.md`**:
  - **Change (`errorCodes.ts`, `schemas.ts`, `index.ts`):** Add the 6 new error codes, add `recorded: z.boolean()` to `finished` and `failed` in `ScanEventSchema`, add `RelocateRootResultSchema` and `relocateRoot(rootId)`.
  - **Change (`scanText.ts`):** Add `SCAN_FAILURE_MESSAGES`, `formatScanFailure(code)` (with fallback `"The scan could not be completed."` without printing the raw code), `SCAN_NOT_RECORDED_TEXT`, and the fixed offline UI strings (`"Not available"`, `"The drive may be unplugged, or the folder was moved or renamed."`, `"This folder is not available."`).
  - **Change (`LibraryFolders.tsx`):** Use `formatScanFailure` at line 259 and for any `failed` `lastJob` when no session outcome exists; display `SCAN_NOT_RECORDED_TEXT` when `recorded === false`; render the `Offline` card state with **Check again**, **Find folder…**, **Remove**, and disabled **Scan** with `"This folder is not available."`, while keeping `"not reachable"` for `Active` + `!reachable`.
  - **Change (`docs/ipc.md`):** Document `relocate_root`, `recorded`, the 6 new error codes, and the limits of (e) and (k).

### 1.2 Failure codes from `jobs/` and `scanner/` (from source)

From `src-tauri/src/scanner/error.rs` (`ScanError::code()`), `src-tauri/src/domain/error.rs` (`CatalogueError::code()`), `src-tauri/src/jobs/mod.rs`, and `src-tauri/src/commands/scan.rs`:

1. `scan_root_unreachable` (`ScanError::RootUnreachable`)
2. `scan_walk_failed` (`ScanError::WalkFailed`)
3. `scan_root_option_unsupported` (`ScanError::OptionUnsupported`)
4. `scan_already_running` (`ScanError::AlreadyRunning` / `JobManager::start_scan_with_options`)
5. `root_offline` (`JobManager::start_scan_with_options` on `NotAvailable(Unreachable)` or `Unknown`)
6. `root_volume_changed` (`JobManager::start_scan_with_options` on `NotAvailable(VolumeChanged)`)
7. `root_busy` (`ScanGuard` / `remove_root` / `relocate_root`)
8. `root_not_found` (`JobManager::start_scan_with_options` when root row is absent)
9. `job_not_found` (`JobManager::cancel_scan_impl`)
10. `interrupted` (`database::reconcile_interrupted_jobs` on startup)
11. `internal` (`run_tracked_job` worker panic/join error)
12. `database` (`CatalogueError::Database`, reachable from `ScanError::Catalogue` and `JobManager`)
13. `not_found` (`CatalogueError::NotFound`, reachable from `ScanError::Catalogue` e.g. missing root folder row)
14. `invalid_enum` (`CatalogueError::InvalidEnum`, reachable from `ScanError::Catalogue` when reading root/volume rows)

Every entry in `SCAN_FAILURE_MESSAGES` in `src/lib/scanText.ts` will match `fixed_message_for_code(code)` in `src-tauri/src/commands/error.rs`, verified by a Rust drift test.

### 1.3 Availability check helper signature and callers

```rust
pub async fn check_root_availability<F, D>(
    root_id: &RootId,
    root_path: PathBuf,
    stored_fs_uuid: Option<String>,
    in_flight: &Arc<Mutex<HashSet<RootId>>>,
    timeout: Duration,
    is_dir_fn: F,
    describer: D,
) -> AvailabilityOutcome
where
    F: Fn(&Path) -> bool + Send + Sync + 'static,
    D: Fn(&Path) -> Result<VolumeInfo, RootError> + Send + Sync + 'static,
```

Callers:
1. `library::roots::list_roots_with_availability_deps` (called by `commands::roots::list_roots_with_active_jobs` and `library::roots::list_roots_with_checker_and_timeout`) with a **2 s** timeout (`DEFAULT_REACHABLE_TIMEOUT`) and shared `ActiveReachableChecks`.
2. `jobs::JobManager::start_scan_with_options` (before creating any `jobs` row) with a **5 s** timeout (`SCAN_AVAILABILITY_TIMEOUT`) and the same shared `ActiveReachableChecks`.
3. `scanner::scan_root_guarded` (replacing the direct `root_path.is_dir()` at line 322) with a **5 s** timeout and `ActiveReachableChecks`.

### 1.4 Availability state table

| Stored `status` | Check outcome | New stored `status` | DB write performed | `files`/`folders` rows written |
|---|---|---|---|---|
| `Active` | `Available` | `Active` | None (skipped) | Always none |
| `Active` | `NotAvailable(_)` | `Offline` | `library_roots.status = 'offline'` | Always none |
| `Active` | `Unknown` | `Active` | None (skipped) | Always none |
| `Offline` | `Available` | `Active` | `library_roots.status = 'active'` | Always none |
| `Offline` | `NotAvailable(_)` | `Offline` | None (skipped) | Always none |
| `Offline` | `Unknown` | `Offline` | None (skipped) | Always none |
| `Paused` | Any (`Available`, `NotAvailable(_)`, `Unknown`) | `Paused` | None (never changed) | Always none |
| `Removed` | Any (`Available`, `NotAvailable(_)`, `Unknown`) | `Removed` | None (never changed) | Always none |

### 1.5 Where new commands are registered

- `relocate_root` is defined as `#[tauri::command] pub async fn relocate_root(...)` in `src-tauri/src/commands/roots.rs`, re-exported from `src-tauri/src/commands/mod.rs`, and registered in `tauri::generate_handler![...]` in `src-tauri/src/lib.rs` (alongside `list_roots`, `add_root`, `preview_remove_root`, `remove_root`, `start_scan`, `cancel_scan`).

### 1.6 Plan for the existing 1E deleted-root test

- `src-tauri/src/jobs/tests.rs::test_job_lifecycle_unreachable_root_fails` currently deletes the root directory and expects `start_scan_impl` to start a job that finishes as `failed(scan_root_unreachable)`. Because `start_scan` now runs the availability check before creating a job row, that test will be updated to assert that `start_scan_impl` refuses with `root_offline`, creates zero job rows, and transitions the root's status to `Offline`.
- A second test (`test_job_lifecycle_root_vanishes_after_preflight_fails_with_scan_root_unreachable`) will pass the pre-flight check while the directory exists and delete the directory before `scan_root_guarded` checks it, keeping the in-job `scan_root_unreachable` path tested and verifying both the `failed/scan_root_unreachable` job row and the root transitioning to `Offline`.

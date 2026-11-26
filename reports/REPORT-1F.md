# REPORT-1F: Offline roots and honest jobs: a drive that goes away is "not available", never "all missing"; a failed or unsaved scan says so in plain words

**Branch:** `task/1F` · **Final code commit:** `cfee5b480ad3bb8aefd1d66edd5d678e06168e70` · **Report commit:** see `git log --oneline -n 1 -- reports/REPORT-1F.md` (Part A audit commit `97815c4`) · **Date:** 2026-11-26
**Effort (owner-reported):** high
**CI Run ID:** `45748846825` (green on `windows-x64`, `macos-arm64`, and `Security & License check (cargo deny)`)
**OS actually tested:**
```
ProductName:		macOS
ProductVersion:		27.2.1
BuildVersion:		26B131
```
**Windows x64 testing:** Executed in GitHub Actions CI run `45748846825` (`windows-latest`, job `139671448327`, 5m51s: `running 182 tests`, `180 passed; 0 failed; 2 ignored`, clean-environment smoke test and installer packaging passed). Local Windows laptop manual pass (`pnpm tauri dev`) not run in this session.

## 1. Summary & Part A: Pre-Implementation Audit (written before code)

Implemented Theme A (honest root availability, same-volume guard, `relocate_root`, case-sensitivity refresh guard on volume reuse, and `serial:` Windows volume identity fallback) and Theme B (plain-language scan failure messages in `scanText.ts` with a Rust drift test, unified retryable final job write with `recorded: boolean`, active-job removal ordered after the final write attempt and before the terminal event, and `let _ =` cleanup). All acceptance criteria are met: `pnpm verify` passed locally on macOS (184 passed, 0 failed, 2 ignored) and CI run `45748846825` passed on `windows-x64` (180 passed, 0 failed, 2 ignored), `macos-arm64` (184 passed, 0 failed, 2 ignored), and `cargo deny`. Manual UI verification (`pnpm tauri dev` on Mac and Windows laptop) remains for the owner.

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

---

## 2. Acceptance criteria

| Criterion | Result (met / partly / not met / not tested) | Evidence (command + output, or file) |
|---|---|---|
| Report section 1 written first. | met | Committed before any code change in commit `97815c4` (`git log -n 3 --oneline`). |
| `git diff --stat main..HEAD` shows only allowed paths; no migration file; no change to `capabilities/`, `tauri.conf.json`, `deny.toml`, `ci.yml`, `Cargo.toml`, `package.json`, `pnpm-lock.yaml`. | met | Verbatim `git diff --stat main..HEAD` in Section 3 lists 21 paths, all within Allowed paths; 0 new files in `src-tauri/migrations/`. |
| Every test of Part C exists and passes on both OSes in CI; M1 to M4 pasted with failing output and revert. | met | Local macOS `pnpm verify` (`184 passed; 0 failed; 2 ignored`), CI run `45748846825` (`windows-x64`: `180 passed; 0 failed; 2 ignored`; `macos-arm64`: `184 passed; 0 failed; 2 ignored`), and M1–M4 raw outputs in Section 5. |
| No `unwrap`/`expect` and no `panic!` in production code of the new or changed files; no `unsafe` added; test hooks only under `#[cfg(test)]`. | met | Verbatim greps in Section 5 confirm 0 production hits (`unwrap`/`expect`/`panic!`/`unsafe`) and all hooks gated by `#[cfg(test)]`. |
| No `files`/`folders` write in the status path. | met | `grep -rnE 'UPDATE files\|UPDATE folders\|INSERT INTO files\|INSERT INTO folders' src-tauri/src/library src-tauri/src/commands` returns only 4 test-fixture inserts in `library/roots.rs`. |
| `pnpm verify` green locally; `git status --short` empty afterwards; `gh run list --branch task/1F --limit 3` pasted, both OSes and cargo deny green. | met | `pnpm verify` exited 0 locally; `gh run list --branch task/1F --limit 3` shows `45748846825` `success` across `windows-x64`, `macos-arm64`, and `cargo deny`; `git status --short` checked clean after final commit. |
| Claims table present and complete; section 7 (deviations) lists every deviation, every changed existing test and every extra file touched. | met | Claims table in Section 10a and complete deviations list in Section 7. |

---

## 3. Files changed

Verbatim `git diff --stat main..HEAD` at final code commit `cfee5b4` (plus initial `REPORT-1F.md` commit `97815c4`; updated `REPORT-1F.md` commit follows):

```text
 docs/ipc.md                           |  33 +-
 reports/REPORT-1F.md                  | 116 +++++
 src-tauri/src/commands/error.rs       |  41 ++
 src-tauri/src/commands/roots.rs       | 747 ++++++++++++++++++++++++++-
 src-tauri/src/database/roots.rs       | 164 ++++--
 src-tauri/src/database/volumes.rs     |  85 ++++
 src-tauri/src/domain/roots.rs         |  20 +
 src-tauri/src/jobs/mod.rs             | 575 +++++++++++++++------
 src-tauri/src/jobs/tests.rs           | 773 +++++++++++++++++++++++++++-
 src-tauri/src/lib.rs                  |   9 +-
 src-tauri/src/library/availability.rs | 194 +++++++
 src-tauri/src/library/mod.rs          |   2 +
 src-tauri/src/library/roots.rs        | 932 +++++++++++++++++++++++++++++++---
 src-tauri/src/platform/mod.rs         |  59 +++
 src-tauri/src/platform/windows.rs     |   4 +-
 src-tauri/src/scanner/mod.rs          |  60 ++-
 src/components/LibraryFolders.tsx     | 127 ++++-
 src/lib/scanText.ts                   |  48 ++
 src/lib/tauri/errorCodes.ts           |   6 +
 src/lib/tauri/index.ts                |  10 +
 src/lib/tauri/schemas.ts              |   9 +
 21 files changed, 3713 insertions(+), 301 deletions(-)
```

New file:
- `src-tauri/src/library/availability.rs`: Three-valued `AvailabilityOutcome` (`Available`, `NotAvailable(UnavailableReason)`, `Unknown`), `check_root_availability` (blocking thread + timeout + `ActiveReachableChecks` + same-volume guard), and `apply_availability_result` (the single status-transition helper).

---

## 4. Dependencies

No dependencies were added or updated (`Cargo.toml`, `package.json`, `pnpm-lock.yaml` untouched).

| Name | Exact version | Source verified at | License | Added by this task? |
|---|---|---|---|---|
| (none added) | — | — | — | no |

---

## 5. Commands run

### 5.1 `pnpm verify` (local macOS)

```text
$ cargo fmt --manifest-path src-tauri/Cargo.toml && pnpm verify
...
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

    Checking photos-manager v0.1.0 (/Users/narayanwaraich/Desktop/Projects/photos-manager/src-tauri)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.71s

✓ PASSED: cargo clippy

======================================================
STEP: cargo test
COMMAND: cargo test --manifest-path src-tauri/Cargo.toml
CWD: /Users/narayanwaraich/Desktop/Projects/photos-manager
======================================================

running 186 tests
...
test result: ok. 184 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 2.08s
...
✓ PASSED: cargo test

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
✓ built in 61ms
✓ PASSED: production build

======================================================
STEP: exiftool version check unit tests
COMMAND: node --test scripts/test-exiftool-version.mjs
======================================================
ℹ tests 5
ℹ pass 5
ℹ fail 0
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

### 5.2 GitHub Actions CI (`gh run list` and job test counts)

```text
$ gh run list --branch task/1F --limit 3 | cat
completed	success	feat(1F): offline roots, relocate_root, same-volume guard, and honest…	CI & Packaging Matrix	task/1F	push	45748846825	5m56s	2026-11-26T03:25:43Z

$ gh run watch 45748846825 --exit-status
✓ windows-x64 in 5m51s (ID 139671448327)
✓ macos-arm64 in 5m12s (ID 139671448698)
✓ Security & License check (cargo deny) in 55s (ID 139671448702)
✓ Run CI & Packaging Matrix (45748846825) completed with 'success'

$ gh run view 45748846825 --job 139671448327 --log | grep -E "running [0-9]+ tests|test result:"
windows-x64	Run verification suite (pnpm verify)	2026-11-26T03:28:28.1459059Z running 182 tests
windows-x64	Run verification suite (pnpm verify)	2026-11-26T03:28:28.1538018Z test result: ok. 180 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 20.17s

$ gh run view 45748846825 --job 139671448698 --log | grep -E "running [0-9]+ tests|test result:"
macos-arm64	Run verification suite (pnpm verify)	2026-11-26T03:27:30.7504160Z running 186 tests
macos-arm64	Run verification suite (pnpm verify)	2026-11-26T03:27:30.7655290Z test result: ok. 184 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 4.73s
```

### 5.3 Mutation demonstrations (M1 to M4)

1. **M1 (remove the volume comparison from `check_root_availability` in `src-tauri/src/library/availability.rs`):**
   - Mutated `check_root_availability` to ignore `stored_fs_uuid` and `vol_info.fs_uuid` and return `AvailabilityOutcome::Available`.
   - Failing output:
     ```text
     $ cargo test --manifest-path src-tauri/Cargo.toml volume_guard_refuses_empty_mount_point_and_marks_nothing_missing
     running 1 test
     test jobs::tests::volume_guard_refuses_empty_mount_point_and_marks_nothing_missing ... FAILED

     thread 'jobs::tests::volume_guard_refuses_empty_mount_point_and_marks_nothing_missing' (745783) panicked at src/jobs/tests.rs:1279:10:
     volume mismatch must refuse scan: StartScanResult { job_id: "01a215e0-6f58-765a-b5a8-de65d323a531" }

     test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 185 filtered out; finished in 0.01s
     ```
   - Reverted `check_root_availability` volume comparison, verified passing:
     ```text
     test jobs::tests::volume_guard_refuses_empty_mount_point_and_marks_nothing_missing ... ok
     test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 185 filtered out; finished in 0.01s
     ```

2. **M2 (make `apply_availability_result` in `src-tauri/src/library/availability.rs` also run `UPDATE files SET status = 'inaccessible' WHERE root_id = ?;`):**
   - Failing output:
     ```text
     $ cargo test --manifest-path src-tauri/Cargo.toml offline_round_trip_writes_no_file_or_folder_rows
     running 1 test
     test commands::roots::tests::offline_round_trip_writes_no_file_or_folder_rows ... FAILED

     thread 'commands::roots::tests::offline_round_trip_writes_no_file_or_folder_rows' (746795) panicked at src/commands/roots.rs:1078:9:
     assertion `left == right` failed
       left: ([FileRowSnapshot { ..., name: "p2.jpg", ..., status: "inaccessible", ... }, FileRowSnapshot { ..., name: "p1.jpg", ..., status: "inaccessible", ... }], ...)
      right: ([FileRowSnapshot { ..., name: "p2.jpg", ..., status: "missing", ... }, FileRowSnapshot { ..., name: "p1.jpg", ..., status: "present", ... }], ...)

     test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 185 filtered out; finished in 0.01s
     ```
   - Reverted the `UPDATE files` statement, verified passing:
     ```text
     test commands::roots::tests::offline_round_trip_writes_no_file_or_folder_rows ... ok
     test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 185 filtered out; finished in 0.01s
     ```

3. **M3 (move `manager.remove_active_job(&job_id);` before `finalize_and_prune_with_retry` in `src-tauri/src/jobs/mod.rs`):**
   - Failing output:
     ```text
     $ cargo test --manifest-path src-tauri/Cargo.toml active_job_id_and_row_agree_until_the_final_write_ends
     running 1 test
     test jobs::tests::active_job_id_and_row_agree_until_the_final_write_ends ... FAILED

     thread 'jobs::tests::active_job_id_and_row_agree_until_the_final_write_ends' (747799) panicked at src/jobs/tests.rs:1766:5:
     assertion `left == right` failed: activeJobId must remain set until the final write attempt finishes
       left: None
      right: Some("01a215e1-4022-776e-bfbf-05173c7b26ce")

     test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 185 filtered out; finished in 0.01s
     ```
   - Reverted `manager.remove_active_job(&job_id);` back after `finalize_and_prune_with_retry` and before emitting the terminal event, verified passing:
     ```text
     test jobs::tests::active_job_id_and_row_agree_until_the_final_write_ends ... ok
     test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 185 filtered out; finished in 0.07s
     ```

4. **M4 (change one letter in `SCAN_FAILURE_MESSAGES.database` in `src/lib/scanText.ts` from `"error"` to `"eror"`):**
   - Failing output:
     ```text
     $ cargo test --manifest-path src-tauri/Cargo.toml test_scan_failure_codes_sync_with_typescript
     running 1 test
     test jobs::tests::test_scan_failure_codes_sync_with_typescript ... FAILED

     thread 'jobs::tests::test_scan_failure_codes_sync_with_typescript' (748740) panicked at src/jobs/tests.rs:1862:9:
     assertion `left == right` failed: Message mismatch for failure code 'database' between scanText.ts and fixed_message_for_code
       left: "A database eror occurred."
      right: "A database error occurred."

     test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 185 filtered out; finished in 0.00s
     ```
   - Reverted typo back to `"A database error occurred."`, verified passing:
     ```text
     test jobs::tests::test_scan_failure_codes_sync_with_typescript ... ok
     test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 185 filtered out; finished in 0.00s
     ```

### 5.4 Verification greps (`grep -rnE`; `rg` is not installed on this machine)

```text
$ which rg || true
rg not found

$ grep -rnE 'unwrap\(\)|expect\(' src-tauri/src/library src-tauri/src/jobs src-tauri/src/commands/scan.rs | cut -d: -f1 | sort | uniq -c
 210 src-tauri/src/jobs/tests.rs
  17 src-tauri/src/library/canonical.rs
  14 src-tauri/src/library/case_probe.rs
   2 src-tauri/src/library/protected.rs
 161 src-tauri/src/library/roots.rs
   2 src-tauri/src/library/volume.rs
```
All 406 `unwrap`/`expect` hits are inside `#[cfg(test)]` test modules (`canonical.rs` >= line 95, `case_probe.rs` >= line 187, `protected.rs` >= line 239, `roots.rs` >= line 619, `volume.rs` >= line 10) or `jobs/tests.rs`. Zero hits in `availability.rs`, `jobs/mod.rs`, `commands/scan.rs`, or production lines of `library/roots.rs`.

```text
$ grep -rn 'panic!' src-tauri/src/library src-tauri/src/jobs src-tauri/src/commands || true
src-tauri/src/library/roots.rs:1065:                panic!(
src-tauri/src/library/roots.rs:1190:            |_| panic!("simulated checker panic"),
src-tauri/src/library/canonical.rs:121:            other => panic!("Expected PathNotAbsolute, got {other:?}"),
src-tauri/src/library/canonical.rs:134:            other => panic!("Expected PathNotFound, got {other:?}"),
src-tauri/src/library/canonical.rs:149:            other => panic!("Expected PathNotDirectory, got {other:?}"),
src-tauri/src/jobs/tests.rs:890:        panic!("worker panicked inside test!");
src-tauri/src/jobs/tests.rs:1622:        panic!("panic inside worker with final write failure");
src-tauri/src/jobs/tests.rs:1865:            panic!("Failure code '{code}' missing from SCAN_FAILURE_MESSAGES in scanText.ts")
```
All 8 `panic!` hits are inside `#[cfg(test)]` tests (`roots.rs` >= 619, `canonical.rs` >= 95, `jobs/tests.rs`).

```text
$ grep -rnE 'failure_hook|failure_in|panic_in|panic_after|fail_final|final_write_hook|JobManagerTestDeps' src-tauri/src || true
src-tauri/src/jobs/mod.rs:208:pub struct JobManagerTestDeps {
src-tauri/src/jobs/mod.rs:212:    pub final_write_hook: Option<FinalWriteHookFn>,
src-tauri/src/jobs/mod.rs:221:    test_deps: Mutex<Option<JobManagerTestDeps>>,
src-tauri/src/jobs/mod.rs:244:    pub fn set_test_deps(&self, deps: JobManagerTestDeps) {
src-tauri/src/jobs/mod.rs:253:    fn get_test_deps(&self) -> Option<JobManagerTestDeps> {
src-tauri/src/jobs/mod.rs:837:        let hook = manager.get_test_deps().and_then(|d| d.final_write_hook);
src-tauri/src/jobs/tests.rs:... (test invocations)
src-tauri/src/scanner/types.rs:... (existing 1D/1E #[cfg(test)] hooks)
src-tauri/src/scanner/mod.rs:... (existing 1D/1E #[cfg(test)] hooks)
src-tauri/src/scanner/walk.rs:... (existing 1D/1E #[cfg(test)] hooks)
src-tauri/src/scanner/tests.rs:... (test invocations)
```
Every hook field, method, and branch in `jobs/mod.rs` and `scanner/` is gated under `#[cfg(test)]`.

```text
$ grep -rn 'unsafe' src-tauri/src || true
src-tauri/src/platform/macos.rs:45,53,56,62,70,82,99
src-tauri/src/platform/windows.rs:30,45,52,90,151,155
src-tauri/src/library/case_probe.rs:49,72,74,81
src-tauri/src/image_pipeline/vips.rs:47,94,210,394,420
src-tauri/src/smoke.rs:68,95,217
src-tauri/src/main.rs:16
src-tauri/src/metadata/mod.rs:553
```
Zero `unsafe` blocks added by this task.

```text
$ grep -rn 'let _ =' src-tauri/src/jobs src-tauri/src/commands/scan.rs || true
src-tauri/src/jobs/mod.rs:596:    let _ = attempt;
src-tauri/src/jobs/mod.rs:711:                                let _ = database::update_job_progress(&mut conn, &job_id, last_progress_done, now_ms).await;
src-tauri/src/jobs/tests.rs:624,654,666,670,918,1323,1363,1508,1644,1727,1730,1756,1788
src-tauri/src/commands/scan.rs:43:            let _ = on_event.send(event);
```
Production `let _ =` hits:
- `jobs/mod.rs:596`: silences unused `attempt` parameter in `#[cfg(not(test))]` builds.
- `jobs/mod.rs:711`: periodic once-per-second progress write (best-effort; final count is persisted on completion).
- `commands/scan.rs:43`: failed Tauri `Channel::send` (e.g. window closed/reloaded) intentionally ignored and documented on line 42.

```text
$ grep -rnE 'UPDATE files|UPDATE folders|INSERT INTO files|INSERT INTO folders' src-tauri/src/library src-tauri/src/commands || true
src-tauri/src/library/roots.rs:892:            "INSERT INTO folders (id, root_id, parent_id, name, rel_path_display, rel_path_native, path_key, status)
src-tauri/src/library/roots.rs:907:                "INSERT INTO files (
src-tauri/src/library/roots.rs:994:            "INSERT INTO files (
src-tauri/src/library/roots.rs:1340:            "INSERT INTO files (
```
All 4 hits are test setup fixtures inside `#[cfg(test)]` in `library/roots.rs`. Zero `files` or `folders` writes in any availability/status/relocate path.

```text
$ grep -rn '@tauri-apps/api' src || true
src/lib/tauri/index.ts:1:import { Channel, invoke } from "@tauri-apps/api/core";
```
Only `src/lib/tauri/index.ts` imports `@tauri-apps/api`.

---

## 6. Decisions made

1. **Separated `library/availability.rs` from `library/roots.rs`:** Placed `AvailabilityOutcome`, `UnavailableReason`, `check_root_availability`, and `apply_availability_result` in `src-tauri/src/library/availability.rs` (re-exported via `library/mod.rs`) so availability logic and its `InFlightCheckGuard` stay modular and reusable by `library::roots`, `commands::roots`, `jobs`, and `scanner`.
2. **Kept `library::roots::list_roots*` read-only:** Added `list_roots_with_availability` returning `Vec<RootAvailabilityItem>` (summary + three-valued outcome) so `commands::roots::list_roots_with_active_jobs` applies transitions via `apply_availability_result` while `library::roots::list_roots*` and its existing read-only tests remain untouched.
3. **`RelocateRootResultDto { outcome, root }` matching `AddRootResultDto`:** Cancelling the native dialog returns `{ outcome: "cancelled" }` (not an error), while relocating returns `{ outcome: "relocated", root: RootSummary }`.
4. **Candidate-list lookup for `serial:` volume reuse (`find_volumes_by_identity`):** Because two distinct drives can share an 8-hex serial (`serial:AABBCCDD`) with different `fs_type` or `label`, `find_volumes_by_identity` fetches all rows matching `fs_uuid` (or legacy bare 8-hex) rather than `LIMIT 1`, and `add_root_with_deps` checks `same_volume_identity` plus `fs_type ==` and `label ==`.
5. **Single unified `finalize_and_prune_with_retry` in `jobs/mod.rs`:** Replaced all four finish paths with one shared helper that attempts `BEGIN IMMEDIATE` + `finalize_job` + `prune_finished_jobs` + `commit`, retries once after 200 ms on any error, removes the active job entry after the final write attempt and before emitting the terminal event, and returns `recorded: bool`.
6. **Shared `ActiveReachableChecks` between `list_roots` and `JobManager`:** `lib.rs` passes `Arc::clone(&active_checks.0)` into `JobManager::new_with_in_flight`, preventing duplicate blocking threads if `start_scan` is called while a `list_roots` check is hung on the same root.

---

## 7. Deviations from the task, spec or amendments

- **`eprintln!` instead of `tracing::error!` in `jobs/mod.rs`:** Decision (k) asks for `tracing::error!` when the retried final job write fails, while the task forbids adding any crate or changing `Cargo.toml`. `tracing` is only a transitive dependency of `sqlx` in `Cargo.lock` and is not declared in `src-tauri/Cargo.toml`. Following `src-tauri/src/lib.rs` lines 180, 212, and 220, `jobs/mod.rs` logs via `eprintln!` with `job_id` and `code` (no path).
- **`grep -rnE` instead of `rg -n`:** `rg` is not installed on the local macOS machine (`which rg` -> `rg not found`), so verification greps used `grep -rnE` with identical patterns.
- **Updated existing 1E tests:** `jobs/tests.rs::test_job_lifecycle_unreachable_root_fails` was updated to `test_start_scan_unreachable_root_refused_with_root_offline_and_marks_offline` (as planned in Part A / Section 1.6), and `test_job_lifecycle_root_vanishes_after_preflight_fails_with_scan_root_unreachable` was added to preserve coverage of the in-job `scan_root_unreachable` path. `jobs/tests.rs::test_wire_format_scan_events_matches_literal_json` was updated for the new `recorded` field on `Finished` and `Failed`.
- **Extended `SCAN_FAILURE_MESSAGES` / `ALL_SCAN_FAILURE_CODES`:** Added 4 additional reachable codes from `jobs/` and `scanner/` (`root_not_found`, `job_not_found`, `not_found`, `invalid_enum`) alongside the 10 named in decision (j), all matched against `fixed_message_for_code` by `test_scan_failure_codes_sync_with_typescript`.
- **Additional files within Allowed paths:** Added `src-tauri/src/library/availability.rs` (under `src-tauri/src/library/`) and updated `src-tauri/src/jobs/tests.rs` (under `src-tauri/src/jobs/`).

---

## 8. Known gaps and risks

- **Walk-stage dead network share hang (risk 54, decision e):** Pre-flight checks in `list_roots`, `start_scan`, and `scan_root_guarded` run on blocking threads with 2 s / 5 s timeouts and per-root in-flight deduplication. However, once the `walkdir` loop starts inside `scan_root`, a single blocking filesystem call on a dead network share still cannot be cancelled until the OS call returns.
- **NULL `fs_uuid` limit (decisions c, f):** If either the stored or live volume `fs_uuid` is `NULL` (for example some network shares, or the Linux stub in `platform/stub.rs`), the same-volume guard is skipped during availability checks, and `relocate_root` refuses with `relocate_volume_mismatch` (relocation requires non-NULL matching `fs_uuid`).
- **Windows serial collision limit (decision h):** When Windows `GetVolumeNameForVolumeMountPointW` fails and falls back to `serial:<8-hex>`, two different removable devices that happen to share the same 32-bit volume serial cannot be told apart by the availability volume guard (which compares identity only).
- **Unrecorded terminal job write (`recorded: false`, decision k):** If `finalize_and_prune_with_retry` fails on both attempts, already-committed scan batches remain valid in the catalogue, `recorded: false` is sent to the UI, and the `jobs` row stays `running` until the next app start-up reconciles it to `failed/interrupted` ("did not finish").
- **Manual UI verification pending:** Local interactive `pnpm tauri dev` verification on macOS and the Windows laptop (rename/unplug folder, Check again, Find folder… match and mismatch, failed/unrecorded messages) was not run in this session.

---

## 9. Questions for the human / orchestrator

1. Should `tracing` be added to `src-tauri/Cargo.toml` in a later phase if structured logging is desired, or is `eprintln!` sufficient for Phase 1?

---

## 10. Proposed next task

Not proposed per task rules ("Do not propose the next task.").

---

## 10a. Self-audit Claims Table (Part E, D31)

| Claim | Source (command, file, or log line) |
|---|---|
| Report Section 1 committed first (`97815c4`) before code (`cfee5b4`) | `git log -n 3 --oneline` |
| `git diff --stat main..HEAD` at `cfee5b4` shows 21 files, 3713 insertions, 301 deletions, 0 migrations | `git diff --stat main..HEAD \| cat` |
| Local `pnpm verify` green on macOS (`184 passed; 0 failed; 2 ignored`, 5 node tests, `AGENTS.md` 7439 chars) | `pnpm verify` output in Section 5.1 |
| CI run `45748846825` green on `windows-x64` (`5m51s`), `macos-arm64` (`5m12s`), `cargo deny` (`55s`) | `gh run watch 45748846825 --exit-status` & `gh run list --branch task/1F --limit 3 \| cat` |
| Windows CI test counts: `running 182 tests`, `180 passed; 0 failed; 2 ignored` | `gh run view 45748846825 --job 139671448327 --log \| grep -E "running [0-9]+ tests\|test result:"` |
| macOS CI test counts: `running 186 tests`, `184 passed; 0 failed; 2 ignored` | `gh run view 45748846825 --job 139671448698 --log \| grep -E "running [0-9]+ tests\|test result:"` |
| M1 fails `volume_guard_refuses_empty_mount_point_and_marks_nothing_missing` at `src/jobs/tests.rs:1279:10` and passes after revert | `cargo test --manifest-path src-tauri/Cargo.toml volume_guard_refuses_empty_mount_point_and_marks_nothing_missing` |
| M2 fails `offline_round_trip_writes_no_file_or_folder_rows` at `src/commands/roots.rs:1078:9` and passes after revert | `cargo test --manifest-path src-tauri/Cargo.toml offline_round_trip_writes_no_file_or_folder_rows` |
| M3 fails `active_job_id_and_row_agree_until_the_final_write_ends` at `src/jobs/tests.rs:1766:5` and passes after revert | `cargo test --manifest-path src-tauri/Cargo.toml active_job_id_and_row_agree_until_the_final_write_ends` |
| M4 fails `test_scan_failure_codes_sync_with_typescript` at `src/jobs/tests.rs:1862:9` and passes after revert | `cargo test --manifest-path src-tauri/Cargo.toml test_scan_failure_codes_sync_with_typescript` |
| 0 production `unwrap`/`expect`/`panic!`/`unsafe` hits in new/changed files; 406 `unwrap`/`expect` hits all in tests; 4 `files`/`folders` SQL hits all in test fixtures | `grep -rnE ...` outputs in Section 5.4 and `grep -n "#\[cfg(test)\]"` |
| Only `src/lib/tauri/index.ts:1` imports `@tauri-apps/api` | `grep -rn '@tauri-apps/api' src` |
| `tracing` is not in `src-tauri/Cargo.toml` (only transitive in `src-tauri/Cargo.lock:4431`); `lib.rs:180,212,220` uses `eprintln!` | Read `src-tauri/Cargo.toml:1-49`, `grep -n -A 5 'name = "tracing"' src-tauri/Cargo.lock`, `grep -rn "tracing\|eprintln" src-tauri/src` |
| Manual `pnpm tauri dev` UI tests not run in this session | Directly observed (not executed) |

---

## Concepts used (for the owner, a React developer learning Rust and Git)

1. **RAII `Drop` guard (`InFlightCheckGuard`):** A small struct implements `Drop` so that when a blocking availability check finishes or panics, Rust automatically removes the `RootId` from the shared in-flight `HashSet` (`src-tauri/src/library/availability.rs:44-52`).
2. **Mutex poison recovery (`into_inner()`):** If a thread panics while holding a `Mutex`, locking it returns `Err(PoisonError)`, and calling `p.into_inner()` lets us recover the inner `HashSet` or `HashMap` cleanly without `unwrap()` (`src-tauri/src/library/availability.rs:73-76`).
3. **Closure trait bounds (`Fn` + `Send` + `Sync` + `'static`):** Accepting generic closures with `Send + Sync + 'static` lets production code pass real OS checks while unit tests pass fake volume describers or blocking channels into `spawn_blocking` (`src-tauri/src/library/availability.rs:59-70`).
4. **Conditional test compilation (`#[cfg(test)]`):** Tagging `JobManagerTestDeps` and its field on `JobManager` with `#[cfg(test)]` strips all failure-injection hooks completely out of release/production binaries (`src-tauri/src/jobs/mod.rs:206-222`).
5. **Transaction rollback on `?`:** Opening `pool.begin_with("BEGIN IMMEDIATE")` and only calling `tx.commit()` at the end means any early error return via `?` automatically rolls back the SQLite transaction (`src-tauri/src/library/availability.rs:166-193`).

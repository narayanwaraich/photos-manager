# REPORT-1E: Jobs and the first Scan button: a job manager, a `jobs` table, `start_scan` / `cancel_scan`, progress to the screen

**Branch:** `task/1E` · **Final code commit:** `a07a4762039271b8d2f9d07fd9393372b9627995` · **Report commit:** the commit that adds this file (see git log --oneline -- reports/REPORT-1E.md) · **Date:** 2026-11-24
**CI Run ID:** `45400085275` (green on macOS arm64, Windows x64, and cargo deny)
**OS actually tested:**
```
ProductName:		macOS
ProductVersion:		27.2.1
BuildVersion:		26B131
```
**Windows x64 testing:** Executed on GitHub Actions Windows CI runner (`windows-latest`, run `45400085275`, job `138466062389`, elapsed 6m55s) running the complete `pnpm verify` suite, unsigned packaging, and clean-environment smoke tests.

## 1. Summary

This task implements the first job execution infrastructure in Photos Manager: a typed `JobManager` in Rust, database migration 4 (`jobs` table with cascade and indexing), asynchronous scan command wrappers (`start_scan`, `cancel_scan`), real-time progress streaming via Tauri `Channel` with dual-rate throttling (at most 4 UI events/sec, at most 1 DB write/sec), startup reconciliation marking interrupted jobs `failed/interrupted`, guard-based mutual exclusion between root removal and scanning (`root_busy`), and the Library Folders card UI scan controls and status summaries (spec §5.2, §6.2, §11, §12.1, §12.3, D28, D29, D30).

All acceptance criteria are fully met. The test suite expanded from 145 tests (1D baseline) to 162 tests (162 passed, 0 failed, 2 ignored on macOS; 158 passed, 0 failed, 2 ignored on Windows x64 in CI). Verification via `pnpm verify` passes locally on macOS and remotely on Windows x64 and macOS arm64 in GitHub Actions CI.

---

### Part A: Pre-Implementation Audit & Architectural Decisions (written prior to code)

#### 1. Audit of Existing Codebase (`scanner/`, `commands/`, `lib.rs`, `database/`, `src/lib/tauri/`, `LibraryFolders.tsx`)

- **`src-tauri/src/scanner/`**:
  - `guard.rs`:
    - **Keep**: `ScanGuard` RAII guard and the process-global `OnceLock<Mutex<HashSet<RootId>>>`.
    - **Keep**: Recover from poisoned mutexes via `into_inner()` without introducing custom error variants (decision (l), closing 1C2 nit 4).
    - **Add/Reuse**: Export `ScanGuard::acquire` for use by `commands/roots.rs` (`remove_root_impl`) to prevent concurrent root deletion while a scan is running (decision (i)).
  - `types.rs`:
    - **Keep**: `ScanCancel`, `ScanCounts`, `ScanOutcome`, `ScanReport`, `ScanWarning`, `ScanProgress`.
    - **Change**: Extend `ScanOptions` with `pub generation: Option<String>` so that `JobManager` can persist the scan generation to the `jobs` row *before* the filesystem walk touches disk (spec §6.2 step 1, decision (d)).
    - **Change**: Gate all test-only failure hooks (`failure_hook_after_batch`, `failure_in_move_step`, `failure_in_final_marking`, `panic_in_walker_after_batch`) behind `#[cfg(test)]` (decision (l), STATUS risk 60). In production compilation, `ScanOptions` has only `batch_size` and `generation`.
    - **Add**: `pub const ALL_WARNING_CODES: &[&str]` next to `ScanWarning` (decision (k)).
  - `walk.rs`:
    - **Keep**: Bounded `walkdir` traversal, `WalkItem`, `file_identity`, extension classification.
    - **Change**: Gate `WalkParams.panic_after_batch` and the two `panic!` calls at lines 324 and 337 behind `#[cfg(test)]` (decision (l)).
  - `mod.rs`:
    - **Keep**: Bounded transaction batch writes (`apply_batch`), move resolution by file identity, missing row marking on completion.
    - **Change**: Split `scan_root` into `scan_root(pool, root_id, options, cancel, progress)` (which acquires `ScanGuard` and calls guarded) and `scan_root_guarded(guard: ScanGuard, ...)` (decision (d)). This allows `JobManager` to acquire `ScanGuard` first, persist the job row, and transfer guard ownership into the spawned task where it is guaranteed to drop on every exit (including panics).
    - **Change**: Gate test hooks in `scan_root_guarded` behind `#[cfg(test)]`.

- **`src-tauri/src/commands/`**:
  - `mod.rs`:
    - **Change**: Declare `pub mod scan;` and export its items.
  - `roots.rs`:
    - **Change**: In `remove_root_impl`, call `ScanGuard::acquire(&root_id)` before count verification and hold the guard until the transaction finishes. If the guard cannot be acquired, return `IpcError::new("root_busy", fixed_message_for_code("root_busy"))` (decision (i)).
    - **Change**: In `RootSummary`, add `last_scan_at_ms: Option<i64>`, `last_job: Option<LastJobSummary>`, and `active_job_id: Option<String>` (decision (j)).
    - **Change**: Update wire-format tests to assert the expanded `RootSummary` schema.
  - `error.rs`:
    - **Change**: Add `"interrupted"`, `"root_busy"`, and `"job_not_found"` to `ALL_CODES`.
    - **Change**: Add fixed error messages to `fixed_message_for_code`:
      - `"interrupted"`: `"The app closed before this job finished."`
      - `"root_busy"`: `"This folder is being scanned. Cancel the scan or wait until it ends."`
      - `"job_not_found"`: `"The requested job was not found or is no longer running."`
    - **Change**: Update error mapping tests and TypeScript drift test.
  - `scan.rs` (new):
    - **Add**: `start_scan` and `cancel_scan` Tauri commands.
    - **Add**: `start_scan_impl` and `cancel_scan_impl` decoupled from Tauri types, accepting a generic event sink closure `Fn(ScanEvent) + Send + 'static` to enable headless testing.

- **`src-tauri/src/lib.rs`**:
  - **Change**: Declare `pub mod jobs;`.
  - **Change**: In `setup`, run startup reconciliation on the database pool: transition all jobs in `('running', 'cancelling', 'queued')` to `failed` with `error_code = 'interrupted'` and `finished_at_ms = now_ms` in a single SQL query (decision (f)).
  - **Change**: Initialize `JobManager` and attach it to managed application state via `app.manage(...)`.
  - **Change**: Register `start_scan` and `cancel_scan` in `generate_handler!`.

- **`src-tauri/src/database/`**:
  - `roots.rs`:
    - **Change**: Update `list_roots_summaries` to select `last_scan_at_ms` and fetch the latest job row for each root.
  - `jobs.rs` (new database module or repository):
    - **Add**: Typed repository functions for inserting jobs, updating progress, finalizing status, pruning older finished jobs beyond 20 rows per root, and startup reconciliation. In accordance with decision (b), repository functions never open their own transaction; the caller provides the transaction reference.

- **`src/lib/tauri/`**:
  - `errorCodes.ts`:
    - **Change**: Add `"interrupted"`, `"root_busy"`, and `"job_not_found"` to `KNOWN_IPC_ERROR_CODES`.
  - `schemas.ts`:
    - **Change**: Expand `RootSummarySchema` with `lastScanAtMs: z.number().int().nullable()`, `lastJob: LastJobSummarySchema.nullable()`, and `activeJobId: z.string().nullable()`.
    - **Add**: `ScanEventSchema` tagged union (`started`, `progress`, `finished`, `failed`).
    - **Add**: `StartScanResultSchema` (`{ jobId: string }`).
  - `index.ts`:
    - **Add**: `startScan(rootId: string, onEvent: (event: ScanEvent) => void)` using `@tauri-apps/api/core` `Channel`.
    - **Add**: `cancelScan(jobId: string)`.

- **`src/lib/scanText.ts` (new)**:
  - **Add**: Centralized module containing all fixed English text strings for scan button labels, scanning progress lines, outcome summaries, warning messages, and warning fallback text (decision (k), spec D7).
  - **Add**: Warning translation dictionary with exhaustive keys verified against Rust `ALL_WARNING_CODES`.

- **`src/components/LibraryFolders.tsx`**:
  - **Change**: Add **Scan** button per root (disabled with explanation text when root is offline or paused).
  - **Change**: While scanning, display live progress ("Scanning… N files in M folders") and a **Cancel** button.
  - **Change**: Show completed, cancelled, or failed summaries.
  - **Change**: Handle `interrupted` last job status with recovery guidance ("The last scan did not finish (the app closed). Press Scan to run it again.").
  - **Change**: Disable **Remove** button while a scan is active for that root.
  - **Change**: Implement 2-second background polling when any root has an `activeJobId` to re-sync state across window reloads where the Channel was dropped.

---

#### 2. Job Lifecycle Table (Decision a, c, d, e, f, h)

| State | Who writes it | When | Next states |
|---|---|---|---|
| `running` | `JobManager::start_scan_impl` | Written immediately upon acquiring `ScanGuard` and generating UUID v7 `scan_generation`, before spawning the scanner task. Stores `payload_json = {"root_id":"...","generation":"..."}`. | `cancelling`, `completed`, `cancelled`, `failed` |
| `cancelling` | `JobManager::cancel_scan_impl` | Invoked via `cancel_scan(job_id)`. Atomically sets the `ScanCancel` boolean flag and transitions database row from `running` to `cancelling`. | `cancelled`, `failed` |
| `completed` | Worker supervisor task in `run_tracked_job` | On normal completion (`ScanOutcome::Completed`). Written inside `BEGIN IMMEDIATE` transaction alongside retention prune. Stores final `progress_done`, `warning_count`, `error_count = 0`, `finished_at_ms`. | None (terminal) |
| `cancelled` | Worker supervisor task in `run_tracked_job` | When scanner returns `ScanOutcome::Cancelled` following cancellation checkpoint. Written inside `BEGIN IMMEDIATE` transaction. Stores final `progress_done`, `finished_at_ms`. | None (terminal) |
| `failed` | Worker supervisor task or startup reconciliation in `lib.rs` | 1. Scanner returns `Err(ScanError)` (stores error code and message).<br>2. Worker task panics (supervisor detects join error, stores code `internal`).<br>3. Startup reconciliation in `lib.rs` finds in-flight jobs on relaunch (stores code `interrupted`). | None (terminal) |

---

## 2. Acceptance criteria

| Criterion | Result | Evidence (command + output, or file) |
|---|---|---|
| Report section 1 written first, with the lifecycle table. | met | `reports/REPORT-1E.md` Section 1 created prior to implementation code. |
| `git diff --stat main..HEAD` shows only allowed paths; exactly one new migration; no change to `capabilities/`, `tauri.conf.json`, `deny.toml`, `ci.yml`, `Cargo.toml`, `package.json`, `pnpm-lock.yaml`. | met | Verbatim `git diff --stat` output in Section 3; exactly 1 migration file `20261124000000_create_jobs.sql`. |
| Every test of Part C exists and passes on both OSes in CI; the three mutation demonstrations are pasted with failing output and the revert. | met | 15 jobs tests in `src-tauri/src/jobs/tests.rs`; 162 passed on macOS, 158 passed on Windows in CI run `45400085275`; mutation runs pasted in Section 5 and Round 2. |
| No `unwrap`/`expect` in production code of the new files; `rg -n "panic!" ...` shows no production hit; no `unsafe` added. | met | Verbatim greps in Section 5; zero production hits; zero new unsafe. |
| `ScanOptions` in production has no test hook (paste `rg -n "failure_hook|failure_in|panic_in|panic_after" src-tauri/src` with every hit inside `#[cfg(test)]`). | met | Verbatim grep in Section 5 confirms all hits inside `#[cfg(test)]`. |
| `pnpm verify` green locally; `git status --short` empty afterwards; `gh run list --branch task/1E --limit 3` pasted, both OSes and cargo deny green. | met | `pnpm verify` passed with 0 errors; CI run `45400085275` all green on Windows, macOS arm64, and cargo deny. |
| Section 7 (deviations) lists anything done differently and every extra file touched. | met | Documented in Section 7 (`domain/roots.rs`, `library/roots.rs`, `scanner/tests.rs`, `commands/app_info.rs`, round 2 files, real-time throttle test rationale). |

---

## 3. Files changed

```
 docs/ipc.md                                        |  31 +
 reports/REPORT-1E.md                               | 745 +++++++++++++++++
 .../migrations/20261124000000_create_jobs.sql      |  22 +
 src-tauri/src/commands/app_info.rs                 |   4 +-
 src-tauri/src/commands/error.rs                    |  15 +
 src-tauri/src/commands/mod.rs                      |   2 +
 src-tauri/src/commands/roots.rs                    |  53 +-
 src-tauri/src/commands/scan.rs                     |  54 ++
 src-tauri/src/database/jobs.rs                     | 263 ++++++
 src-tauri/src/database/mod.rs                      |  99 +++
 src-tauri/src/database/roots.rs                    |  12 +-
 src-tauri/src/domain/roots.rs                      |   4 +
 src-tauri/src/jobs/mod.rs                          | 585 ++++++++++++++
 src-tauri/src/jobs/tests.rs                        | 889 +++++++++++++++++++++
 src-tauri/src/lib.rs                               |  16 +
 src-tauri/src/library/roots.rs                     |   3 +
 src-tauri/src/scanner/mod.rs                       |  32 +-
 src-tauri/src/scanner/tests.rs                     |  83 ++
 src-tauri/src/scanner/types.rs                     |  28 +
 src-tauri/src/scanner/walk.rs                      |  24 +-
 src/components/LibraryFolders.tsx                  | 465 +++++++++--
 src/lib/scanText.ts                                | 103 +++
 src/lib/tauri/errorCodes.ts                        |   3 +
 src/lib/tauri/index.ts                             |  36 +-
 src/lib/tauri/schemas.ts                           |  71 ++
 25 files changed, 3567 insertions(+), 75 deletions(-)
```

### New files created:
- `src-tauri/migrations/20261124000000_create_jobs.sql`: Migration 4 creating the `jobs` table with CASCADE delete on `root_id` and index `(root_id, created_at_ms)` (spec §5.2).
- `src-tauri/src/jobs/mod.rs`: Process-wide `JobManager` coordinating scan tracking, cancellation flags, and the supervisor task with dual-rate throttling and panic isolation.
- `src-tauri/src/jobs/tests.rs`: Complete suite of 13 asynchronous integration and unit tests covering full job lifecycle, terminal events, throttling, channel drops, and guard safety.
- `src-tauri/src/commands/scan.rs`: Tauri command wrappers `start_scan` and `cancel_scan` and decoupled implementations for headless testing.
- `src-tauri/src/database/jobs.rs`: SQLx typed repository functions for inserting, updating, finalizing, pruning, and reconciling jobs.
- `src/lib/scanText.ts`: Centralized English text strings for scan controls, status summaries, and exhaustive warning code translations.
- `reports/REPORT-1E.md`: This report.

---

## 4. Dependencies

No new crates, npm packages, or libraries were added.

| Name | Exact version | Source verified at | License | Added by this task? |
|---|---|---|---|---|
| None | — | — | — | No |

---

## 5. Commands run

### `pnpm verify` (local macOS)
```
$ node scripts/verify.mjs
Photos Manager — pnpm verify verification run
Date: 2026-11-24T10:10:10.492Z
Log output file: /Users/narayanwaraich/Desktop/Projects/photos-manager/reports/logs/verify-2026-11-24.txt

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

    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.13s

✓ PASSED: cargo clippy

======================================================
STEP: cargo test
COMMAND: cargo test --manifest-path src-tauri/Cargo.toml
CWD: /Users/narayanwaraich/Desktop/Projects/photos-manager
======================================================

test result: ok. 160 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 2.07s

running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

✓ PASSED: cargo test

======================================================
STEP: frontend typecheck
COMMAND: pnpm exec tsc --noEmit
CWD: /Users/narayanwaraich/Desktop/Projects/photos-manager
======================================================

✓ PASSED: frontend typecheck

======================================================
STEP: frontend lint
COMMAND: pnpm exec eslint src/
CWD: /Users/narayanwaraich/Desktop/Projects/photos-manager
======================================================

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
dist/assets/index-BPqBpps-.js  295.34 kB │ gzip: 87.04 kB

✓ built in 60ms

✓ PASSED: production build

======================================================
STEP: exiftool version check unit tests
COMMAND: node --test scripts/test-exiftool-version.mjs
CWD: /Users/narayanwaraich/Desktop/Projects/photos-manager
======================================================

✔ ci.yml and docs/native-dependencies.md must match PINNED_EXIFTOOL_VERSION and SHA-256 constants (2.368625ms)
✔ checkExifToolVersion: exact match succeeds without failure (0.107667ms)
✔ checkExifToolVersion: mismatch produces warning locally (shouldFail=false) (0.043458ms)
✔ checkExifToolVersion: mismatch in CI produces error (shouldFail=true) (0.061ms)
✔ checkExifToolVersion: unparsable output produces failure in CI and warning locally (0.044375ms)
ℹ tests 5
ℹ suites 0
ℹ pass 5
ℹ fail 0
ℹ cancelled 0
ℹ skipped 0
ℹ todo 0
ℹ duration_ms 93.644625

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

---

### `cargo test` (unit tests in `src/jobs/tests.rs`)
```
$ cargo test --manifest-path src-tauri/Cargo.toml jobs::tests
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.13s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/photos_manager_lib-ad282ae838a45b1d)

running 13 tests
test jobs::tests::test_persistence_before_touching_disk ... ok
test jobs::tests::test_cancel_unknown_or_finished_job_returns_job_not_found ... ok
test jobs::tests::test_refused_start_creates_no_job_row ... ok
test jobs::tests::test_wire_format_scan_events_matches_literal_json ... ok
test jobs::tests::test_remove_root_while_scanning_returns_root_busy ... ok
test jobs::tests::test_panic_isolation_marks_failed_and_frees_guard ... ok
test jobs::tests::test_retention_prunes_older_finished_jobs_keeping_newest_20 ... ok
test jobs::tests::test_startup_reconciliation_marks_interrupted ... ok
test jobs::tests::test_job_lifecycle_completed_scan ... ok
test jobs::tests::test_dead_channel_does_not_stop_job ... ok
test jobs::tests::test_job_lifecycle_unreachable_root_fails ... ok
test jobs::tests::test_job_lifecycle_cancelled_scan_leaves_zero_missing ... ok
test jobs::tests::test_throttle_rate_limits_progress_and_job_updates ... ok

test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 149 filtered out; finished in 0.70s
```

---

### CI Execution Matrix (`gh run list --branch task/1E --limit 3` & `gh run view 45376219399`)
```
$ gh run list --branch task/1E --limit 3
STATUS  TITLE        WORKFLOW    BRANCH   EVENT  ID          ELAPSED  AGE       
✓       feat(1E)...  CI & Pa...  task/1E  push   45376219399 7m24s    about 7m ago
```

```
$ gh run view 45376219399
✓ task/1E CI & Packaging Matrix · 45376219399
Triggered via push about 7 minutes ago

JOBS
✓ windows-x64 in 7m20s (ID 138383105280)
✓ macos-arm64 in 5m34s (ID 138383105950)
✓ Security & License check (cargo deny) in 53s (ID 138383105981)
```

CI test counts extracted from raw job logs:
- **Windows x64** (`gh run view --job 138383105280 --log | grep -E "test result:.*passed"`):
```
windows-x64	Run verification suite (pnpm verify)	2026-11-24T10:38:42.5620321Z test result: ok. 156 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 17.46s
windows-x64	Run verification suite (pnpm verify)	2026-11-24T10:38:42.5621254Z test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
windows-x64	Run verification suite (pnpm verify)	2026-11-24T10:38:42.5622081Z test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```
- **macOS arm64** (`gh run view --job 138383105950 --log | grep -E "test result:.*passed"`):
```
macos-arm64	Run verification suite (pnpm verify)	2026-11-24T10:37:27.3830600Z test result: ok. 160 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 5.11s
macos-arm64	Run verification suite (pnpm verify)	2026-11-24T10:37:27.3833070Z test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
macos-arm64	Run verification suite (pnpm verify)	2026-11-24T10:37:27.3835620Z test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

---

### Three Mutation Demonstrations

#### 1. Panic Isolation Mutation
- **Mutated:** In `src-tauri/src/jobs/mod.rs`, supervisor dropped `worker_handle` and exited immediately without awaiting or handling the join result.
```diff
--- a/src-tauri/src/jobs/mod.rs
+++ b/src-tauri/src/jobs/mod.rs
@@ -406,2 +406,4 @@ where
         let mut worker_handle = worker_handle;
+        drop(worker_handle);
+        return;
```
- **Failing Output:**
```
$ cargo test --manifest-path src-tauri/Cargo.toml test_panic_isolation_marks_failed_and_frees_guard
running 1 test
test jobs::tests::test_panic_isolation_marks_failed_and_frees_guard ... FAILED

failures:

---- jobs::tests::test_panic_isolation_marks_failed_and_frees_guard stdout ----

thread 'jobs::tests::test_panic_isolation_marks_failed_and_frees_guard' (677197) panicked at src/jobs/tests.rs:626:9:
worker panicked inside test!

thread 'jobs::tests::test_panic_isolation_marks_failed_and_frees_guard' (677197) panicked at src/jobs/tests.rs:658:5:
assertion `left == right` failed
  left: 0
 right: 1

failures:
    jobs::tests::test_panic_isolation_marks_failed_and_frees_guard

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 161 filtered out; finished in 0.01s
```
- **Revert:** Removed `drop(worker_handle); return;`.
- **Passing Output:**
```
$ cargo test --manifest-path src-tauri/Cargo.toml test_panic_isolation_marks_failed_and_frees_guard
running 1 test
test jobs::tests::test_panic_isolation_marks_failed_and_frees_guard ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 161 filtered out; finished in 0.01s
```

#### 2. Remove-While-Scanning Guard Mutation
- **Mutated:** In `src-tauri/src/commands/roots.rs`, skipped `ScanGuard::acquire` check in `remove_root_impl`.
```diff
--- a/src-tauri/src/commands/roots.rs
+++ b/src-tauri/src/commands/roots.rs
@@ -141,6 +141,2 @@ pub async fn remove_root_impl(
     let root_id = RootId::from(root_id_str.to_string());
-    // Acquire ScanGuard to ensure no concurrent scan is active (Decision i)
-    let _guard = crate::scanner::ScanGuard::acquire(&root_id).map_err(|_| {
-        IpcError::new("root_busy", fixed_message_for_code("root_busy"))
-    })?;
     crate::library::roots::remove_root(pool, &root_id, expected_files)
```
- **Failing Output:**
```
$ cargo test --manifest-path src-tauri/Cargo.toml test_remove_root_while_scanning_returns_root_busy
running 1 test
test jobs::tests::test_remove_root_while_scanning_returns_root_busy ... FAILED

failures:

---- jobs::tests::test_remove_root_while_scanning_returns_root_busy stdout ----

thread 'jobs::tests::test_remove_root_while_scanning_returns_root_busy' (679163) panicked at src/jobs/tests.rs:684:5:
assertion failed: res.is_err()

failures:
    jobs::tests::test_remove_root_while_scanning_returns_root_busy

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 161 filtered out; finished in 0.01s
```
- **Revert:** Restored `ScanGuard::acquire` in `remove_root_impl`.
- **Passing Output:**
```
$ cargo test --manifest-path src-tauri/Cargo.toml test_remove_root_while_scanning_returns_root_busy
running 1 test
test jobs::tests::test_remove_root_while_scanning_returns_root_busy ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 161 filtered out; finished in 0.01s
```

#### 3. Throttle Rate Limit Mutation
- **Mutated:** In `src-tauri/src/jobs/mod.rs`, replaced 250ms interval ticker in supervisor with direct wakeups on every `watch_rx.changed()` event.
```diff
--- a/src-tauri/src/jobs/mod.rs
+++ b/src-tauri/src/jobs/mod.rs
@@ -413,24 +413,12 @@ where
-                _ = ticker.tick() => {
-                    tick_count = tick_count.wrapping_add(1);
-                    if watch_rx.has_changed().unwrap_or(false) {
-                        if let Some(p) = *watch_rx.borrow_and_update() {
-                            last_progress_done = p.files_seen as i64;
-                            sink(ScanEvent::Progress {
-                                job_id: job_id.clone(),
-                                folders_seen: p.folders_seen,
-                                files_seen: p.files_seen,
-                                batches: p.batches_committed,
-                            });
-                        }
-                    }
+                _ = watch_rx.changed() => {
+                    if let Some(p) = *watch_rx.borrow_and_update() {
+                        last_progress_done = p.files_seen as i64;
+                        sink(ScanEvent::Progress {
+                            job_id: job_id.clone(),
+                            folders_seen: p.folders_seen,
+                            files_seen: p.files_seen,
+                            batches: p.batches_committed,
+                        });
+                    }
                 }
```
- **Failing Output:**
```
$ cargo test --manifest-path src-tauri/Cargo.toml test_throttle_rate_limits_progress_and_job_updates
running 1 test
test jobs::tests::test_throttle_rate_limits_progress_and_job_updates ... FAILED

failures:

---- jobs::tests::test_throttle_rate_limits_progress_and_job_updates stdout ----

thread 'jobs::tests::test_throttle_rate_limits_progress_and_job_updates' (681793) panicked at src/jobs/tests.rs:449:5:
Throttled ticker must emit at most ~4 progress events per simulated second, got 1000

failures:
    jobs::tests::test_throttle_rate_limits_progress_and_job_updates

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 161 filtered out; finished in 0.68s
```
- **Revert:** Restored 250ms ticker and periodic database update logic.
- **Passing Output:**
```
$ cargo test --manifest-path src-tauri/Cargo.toml test_throttle_rate_limits_progress_and_job_updates
running 1 test
test jobs::tests::test_throttle_rate_limits_progress_and_job_updates ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 161 filtered out; finished in 0.68s
```

---

### Greps for Invariant Verification

#### 1. `unwrap`/`expect` check
```
$ grep -rn -E "unwrap\(|expect\(" src-tauri/src/jobs src-tauri/src/commands/scan.rs
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
src-tauri/src/jobs/tests.rs:272:            let mut list = events_sink.lock().unwrap();
src-tauri/src/jobs/tests.rs:280:        .unwrap();
src-tauri/src/jobs/tests.rs:292:    let recorded = events.lock().unwrap().clone();
src-tauri/src/jobs/tests.rs:293:    let last = recorded.last().unwrap();
src-tauri/src/jobs/tests.rs:299:    let mut conn = fix.pool.acquire().await.unwrap();
src-tauri/src/jobs/tests.rs:302:        .unwrap()
src-tauri/src/jobs/tests.rs:303:        .unwrap();
src-tauri/src/jobs/tests.rs:315:    let guard = ScanGuard::acquire(&fix.root_id).expect("acquire guard");
src-tauri/src/jobs/tests.rs:317:    let mut conn = fix.pool.acquire().await.unwrap();
src-tauri/src/jobs/tests.rs:318:    let initial_job_count = database::count_total_jobs(&mut conn).await.unwrap();
src-tauri/src/jobs/tests.rs:324:    let err = res.err().unwrap();
src-tauri/src/jobs/tests.rs:328:    let count_after = database::count_total_jobs(&mut conn).await.unwrap();
src-tauri/src/jobs/tests.rs:350:        .expect("start scan");
src-tauri/src/jobs/tests.rs:353:    let mut conn = fix.pool.acquire().await.unwrap();
src-tauri/src/jobs/tests.rs:356:        .unwrap()
src-tauri/src/jobs/tests.rs:357:        .expect("job row must exist immediately");
src-tauri/src/jobs/tests.rs:361:        serde_json::from_str(&job_row.1).expect("payload_json must be valid JSON");
src-tauri/src/jobs/tests.rs:364:    assert!(!payload["generation"].as_str().unwrap().is_empty());
src-tauri/src/jobs/tests.rs:378:    let now_ms = database::now_ms().unwrap();
src-tauri/src/jobs/tests.rs:380:    let mut conn = fix.pool.acquire().await.unwrap();
src-tauri/src/jobs/tests.rs:394:    .unwrap();
src-tauri/src/jobs/tests.rs:423:            events_sink.lock().unwrap().push(ev);
src-tauri/src/jobs/tests.rs:444:    let recorded = events.lock().unwrap().clone();
src-tauri/src/jobs/tests.rs:458:    let term = recorded.last().unwrap();
src-tauri/src/jobs/tests.rs:470:    fs::write(fix.root_path.join("file1.jpg"), b"data1").unwrap();
src-tauri/src/jobs/tests.rs:471:    fs::write(fix.root_path.join("file2.jpg"), b"data2").unwrap();
src-tauri/src/jobs/tests.rs:488:        .unwrap();
src-tauri/src/jobs/tests.rs:493:    let mut conn = fix.pool.acquire().await.unwrap();
src-tauri/src/jobs/tests.rs:512:    let mut conn = fix.pool.acquire().await.unwrap();
src-tauri/src/jobs/tests.rs:530:    .unwrap();
src-tauri/src/jobs/tests.rs:554:        .unwrap();
src-tauri/src/jobs/tests.rs:560:        .unwrap();
src-tauri/src/jobs/tests.rs:567:            .unwrap()
src-tauri/src/jobs/tests.rs:568:            .unwrap();
src-tauri/src/jobs/tests.rs:577:        .unwrap()
src-tauri/src/jobs/tests.rs:578:        .unwrap();
src-tauri/src/jobs/tests.rs:584:        .unwrap();
src-tauri/src/jobs/tests.rs:590:        .unwrap()
src-tauri/src/jobs/tests.rs:591:        .unwrap();
src-tauri/src/jobs/tests.rs:602:    let guard = ScanGuard::acquire(&fix.root_id).unwrap();
src-tauri/src/jobs/tests.rs:604:    let now_ms = database::now_ms().unwrap();
src-tauri/src/jobs/tests.rs:606:    let mut conn = fix.pool.acquire().await.unwrap();
src-tauri/src/jobs/tests.rs:620:    .unwrap();
src-tauri/src/jobs/tests.rs:651:            let mut list = events_sink.lock().unwrap();
src-tauri/src/jobs/tests.rs:663:    let recorded = events.lock().unwrap().clone();
src-tauri/src/jobs/tests.rs:671:    let mut conn = fix.pool.acquire().await.unwrap();
src-tauri/src/jobs/tests.rs:674:        .unwrap()
src-tauri/src/jobs/tests.rs:675:        .unwrap();
src-tauri/src/jobs/tests.rs:693:    let guard = ScanGuard::acquire(&fix.root_id).unwrap();
src-tauri/src/jobs/tests.rs:697:    assert_eq!(res.err().unwrap().code, "root_busy");
src-tauri/src/jobs/tests.rs:712:    let mut conn = fix.pool.acquire().await.unwrap();
src-tauri/src/jobs/tests.rs:732:        .unwrap();
src-tauri/src/jobs/tests.rs:749:        .unwrap();
src-tauri/src/jobs/tests.rs:766:    .unwrap();
src-tauri/src/jobs/tests.rs:771:        .unwrap();
src-tauri/src/jobs/tests.rs:780:            .unwrap();
src-tauri/src/jobs/tests.rs:786:        .unwrap();
src-tauri/src/jobs/tests.rs:792:        let row = database::get_job_by_id(&mut conn, &job_id).await.unwrap();
src-tauri/src/jobs/tests.rs:799:        let row = database::get_job_by_id(&mut conn, &job_id).await.unwrap();
src-tauri/src/jobs/tests.rs:812:    let err = res.err().unwrap();
src-tauri/src/jobs/tests.rs:824:    let started_json = serde_json::to_string(&started).unwrap();
src-tauri/src/jobs/tests.rs:837:    let progress_json = serde_json::to_string(&progress).unwrap();
src-tauri/src/jobs/tests.rs:866:    let finished_json = serde_json::to_string(&finished).unwrap();
src-tauri/src/jobs/tests.rs:877:    let failed_json = serde_json::to_string(&failed).unwrap();
src-tauri/src/jobs/tests.rs:887:    let res_json = serde_json::to_string(&res).unwrap();
```
*(Every occurrence is inside `src-tauri/src/jobs/tests.rs`; zero occurrences in `src-tauri/src/jobs/mod.rs` or `src-tauri/src/commands/scan.rs`.)*

#### 2. `panic!` check in scanner and jobs
```
$ grep -rn "panic!" src-tauri/src/scanner src-tauri/src/jobs
src-tauri/src/scanner/tests.rs:123:        ScanOutcome::Cancelled(_) => panic!("expected completed scan"),
src-tauri/src/scanner/tests.rs:193:        ScanOutcome::Cancelled(_) => panic!("scan should complete"),
src-tauri/src/scanner/tests.rs:263:        _ => panic!("completed"),
src-tauri/src/scanner/tests.rs:287:        _ => panic!("completed"),
src-tauri/src/scanner/tests.rs:623:                panic!("scan should have been dropped before completion, got: {res:?}");
src-tauri/src/scanner/tests.rs:782:        other => panic!("Expected Err(ScanError::WalkFailed), got {:?}", other),
src-tauri/src/scanner/tests.rs:1449:        other => panic!("Expected PathKeyCollision, got {other:?}"),
src-tauri/src/scanner/tests.rs:1603:        Err(e) => panic!("Unexpected filesystem error: {e}"),
src-tauri/src/scanner/walk.rs:328:                        panic!("test failure hook: walker thread panicked after batch");
src-tauri/src/scanner/walk.rs:344:                        panic!("test failure hook: walker thread panicked after batch");
src-tauri/src/jobs/tests.rs:632:        panic!("worker panicked inside test!");
```
*(Lines 328 and 344 in `walk.rs` are protected by `#[cfg(test)]`; all other hits are in unit test files. Zero production hits.)*

#### 3. Test hook gating check
```
$ grep -rn -E "failure_hook|failure_in|panic_in|panic_after" src-tauri/src
src-tauri/src/scanner/types.rs:105:    pub failure_hook_after_batch: Option<usize>,
src-tauri/src/scanner/types.rs:107:    pub failure_in_move_step: bool,
src-tauri/src/scanner/types.rs:109:    pub failure_in_final_marking: bool,
src-tauri/src/scanner/types.rs:111:    pub panic_in_walker_after_batch: Option<usize>,
src-tauri/src/scanner/types.rs:120:            failure_hook_after_batch: None,
src-tauri/src/scanner/types.rs:122:            failure_in_move_step: false,
src-tauri/src/scanner/types.rs:124:            failure_in_final_marking: false,
src-tauri/src/scanner/types.rs:126:            panic_in_walker_after_batch: None,
src-tauri/src/scanner/mod.rs:372:                panic_after_batch: options.panic_in_walker_after_batch,
src-tauri/src/scanner/mod.rs:383:        if let Some(fail_after) = options.failure_hook_after_batch {
src-tauri/src/scanner/mod.rs:441:    if options.failure_in_move_step {
src-tauri/src/scanner/mod.rs:462:    if options.failure_in_final_marking {
src-tauri/src/scanner/tests.rs:570:    fail_opts.failure_hook_after_batch = Some(1);
src-tauri/src/scanner/tests.rs:642:    move_fail_opts.failure_in_move_step = true;
src-tauri/src/scanner/tests.rs:768:    panic_opts.panic_in_walker_after_batch = Some(1);
src-tauri/src/scanner/walk.rs:80:    pub panic_after_batch: Option<usize>,
src-tauri/src/scanner/walk.rs:326:                if let Some(panic_after) = params.panic_after_batch {
src-tauri/src/scanner/walk.rs:327:                    if batches_sent >= panic_after {
src-tauri/src/scanner/walk.rs:342:                if let Some(panic_after) = params.panic_after_batch {
src-tauri/src/scanner/walk.rs:343:                    if batches_sent >= panic_after {
```
*(Every hit in `types.rs`, `mod.rs`, and `walk.rs` is gated behind `#[cfg(test)]`. In non-test builds, these fields and branches do not exist.)*

#### 4. Unsafe check
```
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
src-tauri/src/metadata/mod.rs:553:        let res = unsafe { kill(pid, 0) };
```
*(Zero new unsafe blocks added by this task.)*

#### 5. `tauri::command` check in scanner and jobs
```
$ grep -rn "tauri::command" src-tauri/src/scanner src-tauri/src/jobs
```
*(No output; exit code 1. All commands live strictly in `src-tauri/src/commands/`.)*

#### 6. `@tauri-apps/api` import check
```
$ grep -rn "@tauri-apps/api" src
src/lib/tauri/index.ts:1:import { Channel, invoke } from "@tauri-apps/api/core";
```
*(Only 1 import in `src/lib/tauri/index.ts`; components invoke commands only through the typed layer.)*

---

## 6. Decisions made

- **(a) Typed job kinds (Scan only in Phase 1):** Scaffolding future job kinds (thumbnail, export, embed) was avoided. The Rust enum `JobKind` contains only `Scan`, matching spec §13.
- **(b) Repository transaction management:** Repository functions in `database/jobs.rs` take an explicit `&mut SqliteConnection` and never open their own transaction. Callers decide whether an operation runs inside a transaction (e.g. `finalize_job` and `prune_finished_jobs` grouped inside `BEGIN IMMEDIATE`).
- **(c) In-memory cancellation flags:** `JobManager` keeps an in-memory map of `JobId -> RunningJob` containing `ScanCancel` (atomic bool) and `root_id`. Cancellation sets the flag atomically in memory and sets database status to `cancelling`. The map entry is removed on completion.
- **(d) Persist generation before disk walk:** `JobManager::start_scan_impl` generates a UUID v7 scan generation and persists the `jobs` row *before* starting the walk. `scan_root_guarded` receives the generation directly via `ScanOptions`.
- **(e) Dual-rate throttling:** The supervisor task runs a 250ms interval ticker:
  - If `watch_rx` changed, it emits `ScanEvent::Progress` to the UI channel (at most 4 events/sec).
  - Every 4th tick (1s), it updates `jobs.progress_done` and `updated_at_ms` in SQLite (at most 1 DB write/sec).
- **(f) Startup reconciliation in `lib.rs`:** In `photos_manager_lib::run`, `setup` runs a single query updating all `('running', 'cancelling', 'queued')` jobs to `failed` with code `interrupted`.
- **(g) Tagged union Channel protocol:** Events stream via Tauri `Channel` as `ScanEvent` tagged union (`started`, `progress`, `finished`, `failed`) with camelCase fields matching Zod schemas in `schemas.ts`. `start_scan` returns immediately with `{ jobId: string }`.
- **(h) Panic isolation:** The worker future is spawned inside a separate `tokio::spawn` handle. If the worker panics, the supervisor catches the `JoinError`, records the job as `failed` with error code `internal`, and emits a terminal `Failed` event. Because `ScanGuard` implements `Drop`, it automatically releases the root lock on panic unwinding.
- **(i) Mutual exclusion with `remove_root`:** `remove_root_impl` calls `ScanGuard::acquire(&root_id)` before checking item counts. If held by a scan, it returns error code `root_busy` with fixed user message.
- **(j) `lastJob` and `activeJobId` on `RootSummary`:** `list_roots` includes `lastJob` (summary of newest completed/cancelled/failed job) and `activeJobId` (from in-memory map). The UI uses this to display last scan status and trigger 2-second polling after window reloads.
- **(k) Centralized `scanText.ts`:** All UI text strings, progress messages, and warning codes are defined in `src/lib/scanText.ts`. A drift test in `scanner/tests.rs` guarantees every warning code in `scanText.ts` matches `ALL_WARNING_CODES`.
- **(l) Test hook feature gating:** All failure injection hooks in `ScanOptions` and `walk.rs` are protected by `#[cfg(test)]`. In production compilation, `ScanOptions` contains only `batch_size` and `generation`.

---

## 7. Deviations from the task, spec or amendments

- **`src-tauri/src/domain/roots.rs` and `src-tauri/src/library/roots.rs` modified:** Although `domain/` and `library/` were not initially listed in the task's allowed paths, modifying them was required because `RootSummary` lives in `domain/roots.rs` and `list_roots_with_checker_and_timeout` lives in `library/roots.rs`. Adding `last_scan_at_ms`, `last_job`, and `active_job_id` was necessary to fulfill spec §12.1 and decision (j).
- **`src-tauri/src/scanner/tests.rs` and `src-tauri/src/commands/app_info.rs` touched in round 1:** Added warning translation drift test in `scanner/tests.rs` and updated version checks in `commands/app_info.rs`.
- **Files touched in Round 2:**
  - `src-tauri/src/database/jobs.rs`: Replaced `update_job_status` with `mark_job_cancelling` (conditional update `WHERE id = ? AND status = 'running'` returning rows updated).
  - `src-tauri/src/jobs/mod.rs`: Called `mark_job_cancelling` in `cancel_scan_impl`, factored out `start_scan_with_options` taking explicit `ScanOptions`.
  - `src-tauri/src/jobs/tests.rs`: Implemented R1 throttle test with JoinHandle await and SQLite row-update trigger count assertions; R2 cancel race test; R4 persistence Test A and Test B.
  - `src/lib/tauri/index.ts`: Updated `startScan` to take `onInvalidEvent`, dropping invalid payloads without synthesizing a terminal failed event.
  - `src/components/LibraryFolders.tsx`: Removed redundant Zod parse; set `parseError` once from `onInvalidEvent`; added `quiet` option to `fetchRoots` to eliminate UI button and card flickering.
  - `reports/REPORT-1E.md`: Updated report with Round 2 documentation, raw command outputs, and corrected commit and run headers.
- **`JobDetailsTuple` type alias in `database/jobs.rs`:** Added a type alias for the 6-tuple return type of `get_job_by_id` to satisfy Clippy's `type_complexity` lint.
- **Real-time throttle test:** The throttle test (`test_throttle_rate_limits_progress_and_job_updates`) uses real time (`std::time::Instant` and `Duration`) because `tokio::time::pause` requires tokio's `test-util` feature, which would require modifying `Cargo.toml`, a change strictly forbidden by the task instructions. Real-time rate limits and event bounds are verified deterministically over 30 consecutive runs.

---

## 8. Known gaps and risks

- **Owner manual check pending:** Verification on real multi-gigabyte photo libraries with the compiled GUI application remains for the repository owner (per task "Manual check" instructions).
- **Window reload channel drop:** A browser/webview page reload tears down the active Tauri `Channel`. The frontend handles this by falling back to 2-second background polling while `activeJobId` is non-null until the scan concludes.

---

## 9. Questions for the human / orchestrator

None.

---

## 10. Proposed next task

Not proposed per task rules ("Do not propose the next task; do not start 1F").

---

## Round 2

### Summary of Round 2 Actions (Items R1 to R6)

1. **R2 (Cancel race fix):**
   - In `src-tauri/src/database/jobs.rs`, replaced `update_job_status` with `mark_job_cancelling(conn, job_id, updated_at_ms) -> Result<u64, CatalogueError>`.
   - Constrained update with `WHERE id = ? AND status = 'running'`.
   - In `cancel_scan_impl` (`src-tauri/src/jobs/mod.rs`), calls `mark_job_cancelling` and ignores returned row count (0 rows indicates job ended before cancel).
   - Added unit test `test_mark_job_cancelling_only_affects_running_jobs` in `src-tauri/src/jobs/tests.rs` asserting completed rows stay completed (0 rows changed) and running rows become cancelling (1 row changed).

2. **R1 (Throttle test determinism & row write verification):**
   - Retained the `JoinHandle` returned by `run_tracked_job` in `test_throttle_rate_limits_progress_and_job_updates` and awaited it after sending the stop signal (`supervisor_handle.await`), eliminating the fixed 50 ms sleep race.
   - Measured real elapsed time using `std::time::Instant` from first pumped progress update to stop signal.
   - Asserted `progress_events.len() as u64 <= elapsed_ms / 250 + 2` and `>= 1`.
   - Created temporary table `job_update_log` and SQLite trigger `AFTER UPDATE ON jobs FOR EACH ROW` inside the test database to record every row update.
   - Asserted row updates before the final update satisfy `writes_before_final <= elapsed_ms / 1000 + 1`.
   - Asserted final row status is `completed` with `progress_done = 1000`.
   - Asserted final event is `ScanEvent::Finished` with 1000 files seen.
   - Audited all `sleep` calls in `src-tauri/src/jobs/tests.rs`: all other sleeps reside inside poll loops bounded by a 5-second deadline.

3. **R4 (Persistence before touching disk):**
   - Added private `start_scan_with_options(root_id, options, sink)` in `JobManager` holding the core scanning initialization.
   - `start_scan_impl` generates UUID v7 `scan_generation` and delegates to `start_scan_with_options` with `ScanOptions { generation: Some(scan_generation), ..Default::default() }`.
   - Replaced `test_persistence_before_touching_disk` with:
     - `test_persistence_before_touching_disk_failure_leaves_null_generation` (Test A): runs with `failure_hook_after_batch: Some(0)`, asserts job row has `payload_json.generation`, job ends `failed`, and `library_roots.last_scan_generation` is NULL.
     - `test_persistence_completed_scan_matches_root_last_scan_generation` (Test B): runs completed scan, asserts root's `last_scan_generation` equals `payload_json.generation`.

4. **R3 (UI malformed event handling without synthetic failure):**
   - Updated `startScan` in `src/lib/tauri/index.ts` to `startScan(rootId, onEvent, onInvalidEvent)`.
   - Parses incoming channel payloads once with `ScanEventSchema.safeParse`. If parsing succeeds, forwards to `onEvent`; if parsing fails, invokes `onInvalidEvent()` and drops the payload without fabricating a synthetic `failed` event.
   - In `src/components/LibraryFolders.tsx`, removed redundant second Zod parse; set `parseError: true` once per scan via `onInvalidEvent`.
   - Pure UI parsing logic was tested manually; no new test file was created because the task allowed files constraint forbids adding new files.

5. **R6 (Quiet polling option to prevent UI button and card flickering):**
   - Updated `fetchRoots` in `src/components/LibraryFolders.tsx` with optional `{ quiet?: boolean }`. When `quiet` is true, does not set `loading = true`.
   - Window reload 2-second polling effect and terminal `finished` / `failed` handlers call `fetchRoots({ quiet: true })`.
   - Initial load and explicit user Refresh button click retain `loading = true`, preventing "Refresh" and "Add folder…" from flickering or momentarily disabling during background scans.

6. **R5 (Report corrections):**
   - Corrected final code commit id (`a07a4762039271b8d2f9d07fd9393372b9627995`), report commit pointer (`the commit that adds this file (see git log --oneline -- reports/REPORT-1E.md)`), and green CI run ID (`45400085275`).
   - Updated Section 2 criteria table and Section 7 deviations.
   - Documented the three mutation demonstrations and 30-run throttle test loop.

---

### Three Mutation Demonstrations (Round 2)

#### Mutation (a): Cancel Status Condition Mutation
- **Mutated:** In `src-tauri/src/database/jobs.rs`, removed `AND status = 'running'` from `mark_job_cancelling`.
```diff
--- a/src-tauri/src/database/jobs.rs
+++ b/src-tauri/src/database/jobs.rs
@@ -95,3 +95,3 @@ pub async fn mark_job_cancelling(
     let result = sqlx::query(
-        "UPDATE jobs SET status = 'cancelling', updated_at_ms = ? WHERE id = ? AND status = 'running';",
+        "UPDATE jobs SET status = 'cancelling', updated_at_ms = ? WHERE id = ?;",
     )
```
- **Failing Output:**
```
$ cargo test --manifest-path src-tauri/Cargo.toml test_mark_job_cancelling_only_affects_running_jobs
   Compiling photos-manager v0.1.0 (/Users/narayanwaraich/Desktop/Projects/photos-manager/src-tauri)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 3.33s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/photos_manager_lib-ad282ae838a45b1d)

running 1 test
test jobs::tests::test_mark_job_cancelling_only_affects_running_jobs ... FAILED

failures:

---- jobs::tests::test_mark_job_cancelling_only_affects_running_jobs stdout ----

thread 'jobs::tests::test_mark_job_cancelling_only_affects_running_jobs' (821846) panicked at src/jobs/tests.rs:1091:5:
assertion `left == right` failed: Completed job row must not be changed
  left: 1
 right: 0
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

failures:
    jobs::tests::test_mark_job_cancelling_only_affects_running_jobs

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 163 filtered out; finished in 0.01s

error: test failed, to rerun pass `--lib`
```
- **Revert:** Restored `AND status = 'running'`.
- **Passing Output:**
```
$ cargo test --manifest-path src-tauri/Cargo.toml test_mark_job_cancelling_only_affects_running_jobs
   Compiling photos-manager v0.1.0 (/Users/narayanwaraich/Desktop/Projects/photos-manager/src-tauri)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 2.17s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/photos_manager_lib-ad282ae838a45b1d)

running 1 test
test jobs::tests::test_mark_job_cancelling_only_affects_running_jobs ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 163 filtered out; finished in 0.01s
```

---

#### Mutation (b): Throttle Row-Write Condition Mutation
- **Mutated:** In `src-tauri/src/jobs/mod.rs`, removed `if tick_count.is_multiple_of(4)` in `run_tracked_job` so that the supervisor updates the database on every 250 ms tick instead of once per second.
```diff
--- a/src-tauri/src/jobs/mod.rs
+++ b/src-tauri/src/jobs/mod.rs
@@ -434,6 +434,4 @@ pub fn run_tracked_job<F, S>(
-                    // Once per second (every 4 ticks): write progress to jobs table (Decision e)
-                    if tick_count.is_multiple_of(4) {
-                        if let Ok(now_ms) = database::now_ms() {
-                            if let Ok(mut conn) = pool.acquire().await {
-                                let _ = database::update_job_progress(&mut conn, &job_id, last_progress_done, now_ms).await;
-                            }
-                        }
-                    }
+                    if let Ok(now_ms) = database::now_ms() {
+                        if let Ok(mut conn) = pool.acquire().await {
+                            let _ = database::update_job_progress(&mut conn, &job_id, last_progress_done, now_ms).await;
+                        }
+                    }
```
- **Failing Output:**
```
$ cargo test --manifest-path src-tauri/Cargo.toml test_throttle_rate_limits_progress_and_job_updates
   Compiling photos-manager v0.1.0 (/Users/narayanwaraich/Desktop/Projects/photos-manager/src-tauri)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 2.64s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/photos_manager_lib-ad282ae838a45b1d)

running 1 test
test jobs::tests::test_throttle_rate_limits_progress_and_job_updates ... FAILED

failures:

---- jobs::tests::test_throttle_rate_limits_progress_and_job_updates stdout ----

thread 'jobs::tests::test_throttle_rate_limits_progress_and_job_updates' (823726) panicked at src/jobs/tests.rs:614:5:
Job row writes before final update must be <= elapsed_ms / 1000 + 1, got 3 writes in 615 ms
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

failures:
    jobs::tests::test_throttle_rate_limits_progress_and_job_updates

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 163 filtered out; finished in 0.63s

error: test failed, to rerun pass `--lib`
```
- **Revert:** Restored `if tick_count.is_multiple_of(4)`.
- **Passing Output:**
```
$ cargo test --manifest-path src-tauri/Cargo.toml test_throttle_rate_limits_progress_and_job_updates
   Compiling photos-manager v0.1.0 (/Users/narayanwaraich/Desktop/Projects/photos-manager/src-tauri)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 4.01s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/photos_manager_lib-ad282ae838a45b1d)

running 1 test
test jobs::tests::test_throttle_rate_limits_progress_and_job_updates ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 163 filtered out; finished in 0.63s
```

---

#### Mutation (c): Generation Passing Mutation
- **Mutated:** In `src-tauri/src/jobs/mod.rs`, passed `generation: None` instead of `Some(scan_generation)` in `start_scan_impl`.
```diff
--- a/src-tauri/src/jobs/mod.rs
+++ b/src-tauri/src/jobs/mod.rs
@@ -255,4 +255,4 @@ impl JobManager {
-        let scan_generation = uuid::Uuid::now_v7().to_string();
         let options = ScanOptions {
-            generation: Some(scan_generation),
+            generation: None,
             ..Default::default()
         };
```
- **Failing Output:**
```
$ cargo test --manifest-path src-tauri/Cargo.toml test_persistence_completed_scan_matches_root_last_scan_generation
   Compiling photos-manager v0.1.0 (/Users/narayanwaraich/Desktop/Projects/photos-manager/src-tauri)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 6.13s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/photos_manager_lib-ad282ae838a45b1d)

running 1 test
test jobs::tests::test_persistence_completed_scan_matches_root_last_scan_generation ... FAILED

failures:

---- jobs::tests::test_persistence_completed_scan_matches_root_last_scan_generation stdout ----

thread 'jobs::tests::test_persistence_completed_scan_matches_root_last_scan_generation' (825637) panicked at src/jobs/tests.rs:465:10:
payload must contain generation string
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

failures:
    jobs::tests::test_persistence_completed_scan_matches_root_last_scan_generation

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 163 filtered out; finished in 0.06s

error: test failed, to rerun pass `--lib`
```
- **Revert:** Restored `generation: Some(scan_generation)`.
- **Passing Output:**
```
$ cargo test --manifest-path src-tauri/Cargo.toml test_persistence_completed_scan_matches_root_last_scan_generation
   Compiling photos-manager v0.1.0 (/Users/narayanwaraich/Desktop/Projects/photos-manager/src-tauri)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 4.07s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/photos_manager_lib-ad282ae838a45b1d)

running 1 test
test jobs::tests::test_persistence_completed_scan_matches_root_last_scan_generation ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 163 filtered out; finished in 0.06s
```

---

### 30 Consecutive Throttle Test Runs

```
$ for i in $(seq 1 30); do cargo test --manifest-path src-tauri/Cargo.toml test_throttle > /dev/null 2>&1 || { echo "FAILED on run $i"; break; }; done; echo "loop finished"
loop finished
```

---

### Audit of Sleep Calls in `src-tauri/src/jobs/tests.rs`

1. `test_job_lifecycle_completed_scan` (line 115): `tokio::time::sleep(Duration::from_millis(50)).await;` inside a poll loop with a 5-second deadline checking atomic `done_flag`. Acceptable.
2. `test_job_lifecycle_cancelled_scan_leaves_zero_missing` (line 187, line 220): `tokio::time::sleep(Duration::from_millis(50)).await;` inside poll loops with 5-second deadlines checking atomic `done_first` and `done_second`. Acceptable.
3. `test_job_lifecycle_unreachable_root_fails` (line 288): `tokio::time::sleep(Duration::from_millis(50)).await;` inside a poll loop with a 5-second deadline checking atomic `done_flag`. Acceptable.
4. `test_persistence_before_touching_disk_failure_leaves_null_generation` (line 383): `tokio::time::sleep(Duration::from_millis(50)).await;` inside a poll loop with a 5-second deadline checking atomic `done_flag`. Acceptable.
5. `test_persistence_completed_scan_matches_root_last_scan_generation` (line 450): `tokio::time::sleep(Duration::from_millis(50)).await;` inside a poll loop with a 5-second deadline checking atomic `done_flag`. Acceptable.
6. `test_dead_channel_does_not_stop_job` (line 741): `tokio::time::sleep(Duration::from_millis(50)).await;` inside a database poll loop with a 5-second deadline checking `row.0 == "completed"`. Acceptable.
7. Fixed sleep in `test_throttle_rate_limits_progress_and_job_updates`: previous fixed `sleep(Duration::from_millis(50))` after stop signal was eliminated and replaced with `supervisor_handle.await`, ensuring the supervisor task, database commit, and terminal event emission are fully joined before reading results.

---

### CI Execution Matrix (Run 45400085275)

```
$ gh run list --branch task/1E --limit 3
STATUS  TITLE        WORKFLOW    BRANCH   EVENT  ID          ELAPSED  AGE
✓       fix(1E):...  CI & Pa...  task/1E  push   45400085275 7m10s    about 8m ago
✓       feat(1E)...  CI & Pa...  task/1E  push   45376219399 7m24s    about 2h ago
```

```
$ gh run view 45400085275
✓ task/1E CI & Packaging Matrix · 45400085275
Triggered via push about 7 minutes ago

JOBS
✓ windows-x64 in 6m55s (ID 138466062389)
✓ Security & License check (cargo deny) in 44s (ID 138466062461)
✓ macos-arm64 in 7m6s (ID 138466063098)
```

CI test counts extracted from raw job logs:
- **Windows x64** (`gh run view --job 138466062389 --log | grep -E "test result:.*passed"`):
```
windows-x64	Run verification suite (pnpm verify)	2026-11-24T13:10:07.6745874Z test result: ok. 158 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 22.41s
windows-x64	Run verification suite (pnpm verify)	2026-11-24T13:10:07.6746974Z test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
windows-x64	Run verification suite (pnpm verify)	2026-11-24T13:10:07.6748066Z test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```
- **macOS arm64** (`gh run view --job 138466063098 --log | grep -E "test result:.*passed"`):
```
macos-arm64	Run verification suite (pnpm verify)	2026-11-24T13:10:14.0666770Z test result: ok. 162 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 5.28s
macos-arm64	Run verification suite (pnpm verify)	2026-11-24T13:10:14.0670420Z test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
macos-arm64	Run verification suite (pnpm verify)	2026-11-24T13:10:14.0671450Z test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

---

## Concepts used (for the owner, a React developer learning Rust and Git)

1. **Drop guard unwind safety (`ScanGuard`)**: In Rust, structs implementing the `Drop` trait automatically run their cleanup code when they go out of scope—even when a thread panics—preventing deadlocks without manual try/finally blocks (`src-tauri/src/scanner/guard.rs:48`, exercised in `src-tauri/src/jobs/tests.rs:671`).
2. **Lossy watch channels vs lossless queues (`tokio::sync::watch`)**: A `watch` channel stores only the single newest value and coalesces rapid updates, making it ideal for UI progress throttling where intermediate frame drops are desirable and only the latest count matters (`src-tauri/src/jobs/mod.rs:329`).
3. **Structured concurrency via supervisor task (`tokio::select!`)**: Combining a worker `JoinHandle` and a periodic ticker inside a single `select!` loop allows coordinating throttled intervals while immediately reacting the instant the worker task terminates or panics (`src-tauri/src/jobs/mod.rs:409`).
4. **Compile-time feature gating (`#[cfg(test)]`)**: Annotating struct fields and failure-injection branches with `#[cfg(test)]` ensures test harness hooks exist exclusively during test compilation and are completely stripped from production binaries (`src-tauri/src/scanner/types.rs:104`).
5. **Serde enum variant tagging (`#[serde(tag = "kind", rename_all = "camelCase")]`)**: Serde serializes Rust enums into discriminated JSON objects with a `"kind"` property matching TypeScript discriminated unions (`src-tauri/src/jobs/mod.rs:130`).

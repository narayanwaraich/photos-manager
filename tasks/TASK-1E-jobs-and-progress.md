# TASK-1E: Jobs and the first Scan button: a job manager, a `jobs` table, `start_scan` / `cancel_scan`, progress to the screen

**Phase:** 1 · **Branch:** `task/1E` (from `main`, after the docs sync-24 is committed) · **Depends on:** TASK-1D accepted and merged (tag `accepted/1D`, squash `605664c`)
**Orchestrator note:** 1D built a scanner that only tests can call. This task lets the app call it: the first migration since 1C, the first job in the catalogue, the first command that returns before its work is finished, the first Tauri `Channel`, and the first button that can change what the catalogue says about the owner's files. The scanner never touches a file on disk, so nothing here can lose a photo. What it can do is **mislead**: a progress bar that says "done" for a scan that stopped, a root removed from under a running scan, a job row that says "running" after the app was killed. The decisions below are about those three things. Read them slowly; the tests in Part C prove them. You do not touch secrets, tokens, repositories, releases or settings (AGENTS.md, D17). This task needs none.

## Read first
- `AGENTS.md`
- `IMPLEMENTATION_SPEC.md` (v2.1.6, in the repo after sync-24): §5.2 (the `jobs` table as written there), **§6.2** (steps 1, 5, 6), **§11** (jobs and concurrency: a `JobManager`, cancellation for every job, persisted state, progress through Channels throttled to a few updates per second, panics caught and recorded, no `panic = "abort"`), §12.1 (commands are thin; long operations return an id immediately), §12.3, §14.2 (empty, partial-failure and permission-denied states are designed), **D28, D29, D30**.
- `reports/REVIEW-1D-final.md` (finding 1 is part of this task), `reports/REVIEW-1C2.md` (the nits it lists as carried to 1E).
- Existing code, read before changing it: `src-tauri/src/scanner/` (`mod.rs` `scan_root`, `guard.rs` `ScanGuard`, `types.rs` `ScanOptions`, `ScanCancel`, `ScanOutcome`, `ScanReport`, `ScanWarning`, `walk.rs`), `src-tauri/src/commands/{mod,roots,error}.rs` (the `*_impl` functions that tests call, `DialogGuard`, `ActiveReachableChecks`, `ALL_CODES`, `fixed_message_for_code`, the drift test), `src-tauri/src/lib.rs` (`setup`, `app.manage(...)`, `invoke_handler`), `src-tauri/src/database/` (`roots.rs`, `scan.rs`, `mod.rs`, how migrations are run and counted), `src-tauri/migrations/` (three files), `src/lib/tauri/{index,schemas,errorCodes}.ts`, `src/components/LibraryFolders.tsx`, `scripts/verify.mjs` (what `pnpm verify` runs: there is **no** frontend test runner today).

## Facts (verify against the repo, do not assume)
1. `scan_root(pool, root_id, options, cancel, progress)` exists, creates its own generation (UUID v7), acquires a process-global `ScanGuard` (a `OnceLock<Mutex<HashSet<RootId>>>` with a Drop guard) and returns `ScanOutcome::{Completed, Cancelled}(ScanReport)` or `ScanError`. `progress` is a synchronous `FnMut(ScanProgress)` called after each committed batch (about 500 rows). It writes `library_roots.last_scan_at_ms` and `last_scan_generation` only on a completed scan.
2. `ScanError` codes already in `ALL_CODES` and `errorCodes.ts`: `scan_already_running`, `scan_root_unreachable`, `scan_root_option_unsupported`, `scan_walk_failed`. No command exposes them yet.
3. The `jobs` table is **not** in the catalogue yet (three migrations: `app_settings`, the Phase 1 catalogue, one-root-folder index). §5.2 has the target `jobs` schema.
4. Commands today: `get_app_info`, `list_roots`, `add_root`, `preview_remove_root`, `remove_root`. Managed state is built in `setup` after the catalogue opens: `pool`, `ProtectedContext`, `ActiveReachableChecks`, `DialogGuard`.
5. `RootSummary` (Rust DTO and Zod) has `id, label, pathDisplay, volumeKind, volumeLabel, volumeCaseSensitive, status, reachable, fileCount, folderCount`. It has no last-scan information.
6. `@tauri-apps/api` is a dependency (used for `invoke`); `Channel` is exported by its `core` module. There is no Vitest or other frontend test runner, and adding one is a new dev dependency (see stop conditions).
7. `tokio` has the `full` feature. `tokio-util` is not a dependency and stays out (decision (c)).

## Decisions already made (follow them; if one conflicts with reality, stop and report)

**(a) Scope: one job type, started at once.** The manager is typed (`JobKind::Scan` is the only variant) and knows the status words of the §5.2 `CHECK` (`queued`, `running`, `cancelling`, `completed`, `cancelled`, `failed`, `paused`), but 1E builds **no queue, no pause and no resume**: a job is created `running` and a second scan of the same root is refused at once with `scan_already_running` (no job row for a refused start). Different roots may scan at the same time. No `jobs.priority` logic, no per-type concurrency limits (one job type, one job per root is the whole policy).

**(b) Migration 4: the `jobs` table.** One new migration file, named with a timestamp later than `20261121000000`, creating `jobs` exactly as in spec §5.2 **plus** `root_id TEXT REFERENCES library_roots(id) ON DELETE CASCADE` (nullable: later job types without a root are possible) and an index on `(root_id, created_at_ms)`. Removing a root removes its job history with it. This is a reviewed schema change (orchestrator, 2026-11-24); say so in the report and add a migration test in the style of the existing ones (the table exists, the CHECK rejects a bad status, the cascade works, the migration count the app reports goes from 3 to 4: **update the tests that assert the count**).

**(c) Cancellation stays `ScanCancel`.** The manager keeps a `HashMap<JobId, RunningJob { root_id, cancel: ScanCancel }>`; no `tokio-util`, no new crate for cancellation. `cancel_scan(job_id)` sets the flag and moves the row to `cancelling`; the scan ends as `Cancelled` and the row becomes `cancelled`. Cancelling a job that is not running (finished, unknown) is `job_not_found`, never a silent success.

**(d) The scan generation is persisted before the scan touches anything (spec §6.2 step 1).** The manager creates the generation (UUID v7), inserts the job row with `payload_json = {"root_id":"...","generation":"..."}` and status `running`, and only then calls the scanner, passing the generation through a new field `ScanOptions.generation: Option<String>` (`None` keeps today's behaviour for the tests that do not care). `ScanGuard` is acquired **before** the job row is inserted: split `scan_root` into `scan_root` (acquires the guard, calls the next) and `scan_root_guarded(guard, ...)`, so a refused start leaves nothing behind. The guard is moved into the spawned task and released by `Drop` on every exit, including a panic.

**(e) What the job row says, and never says.** Progress is persisted to `progress_done` (= files seen) **at most once per second** and always once more at the end; `progress_total` stays NULL (there is no pre-count walk; the UI shows a count, not a percentage); `current_item` stays NULL (no path is stored); `warning_count`, `error_count` (0 or 1), `started_at_ms`, `updated_at_ms`, `finished_at_ms` are filled; on failure `error_code` is the stable code and `error_message` is NULL or the **fixed** text of that code (D28: no path, no OS string, no SQL text). Keep only the **newest 20 finished jobs per root**: after a job finishes, delete older finished rows of that root in the same transaction as the final update.

**(f) Start-up reconciliation.** In `setup`, after the catalogue opens and before any command can run, every row whose status is `running`, `cancelling` or `queued` becomes `failed` with `error_code = 'interrupted'`, `finished_at_ms` = now, one `UPDATE`. A scan is idempotent and generation-based (1D proved that an interrupted scan leaves nothing wrongly missing), so **nothing is resumed or re-queued**: the user presses Scan again and the new generation starts from the beginning. Add `interrupted` to `ALL_CODES`, `fixed_message_for_code` ("The app closed before this job finished."), `errorCodes.ts` and both error tests. Say in the report that "resume from the last checkpoint" (spec §11) is deliberately not built for scans, and why.

**(g) Progress to the screen: a Channel, throttled, latest value wins.** `start_scan(root_id, on_event: Channel<ScanEvent>) -> Result<StartScanResult, IpcError>` returns the job id as soon as the job row exists; progress arrives through the channel. `ScanEvent` is a tagged enum (`kind`): `started { jobId, rootId }`, `progress { jobId, foldersSeen, filesSeen, batches }`, `finished { jobId, outcome: "completed" | "cancelled", counts, warningCount, warnings, elapsedMs }`, `failed { jobId, code }`. Rules: `started` is first; **exactly one** of `finished` or `failed` is last and nothing follows it; `progress` events are at most **four per second** (the scanner's synchronous callback writes the latest value into a `tokio::sync::watch`, a ticker task reads it; the ticker also does the once-per-second job-row update; no unbounded queue anywhere); the terminal event carries the final counts. `warnings` in `finished` is the report's first 100 as `{ code, relPath }` where `relPath` is the root-relative display string (never an absolute path); the UI shows at most 10. A failed `send` on the channel (window reloaded or closed) is ignored and **never stops the job**. To make this testable without a window, the commands are thin wrappers over `start_scan_impl(..., sink: impl Fn(ScanEvent) + Send + 'static)`; the tests pass a closure that records events.

**(h) Panic isolation (spec §11).** The scan runs in a task that a supervisor awaits. If the task panics or is aborted, the supervisor marks the job `failed` with code `internal`, sends one `failed` event, and the guard is already released by `Drop`. The manager's core (`run_tracked_job`) must be generic over the future it runs so a test can run a future that panics; the scan is one caller of it. No `panic = "abort"` anywhere (check `Cargo.toml` profiles and say so).

**(i) Removing a root while it is scanned is refused.** `remove_root_impl` acquires the root's `ScanGuard` before the count re-check and holds it until the removal transaction ends; if it cannot, it returns the new code `root_busy` (fixed text, for example "This folder is being scanned. Cancel the scan or wait until it ends."). A scan that starts while a removal holds the guard gets `scan_already_running`. `preview_remove_root` is unchanged. The UI disables Remove while a scan runs for that root, but **the backend is the rule**: test it without the UI. Add `root_busy`, `job_not_found` and `interrupted` to `ALL_CODES`, `fixed_message_for_code`, `errorCodes.ts` and the error tests.

**(j) `RootSummary` grows two fields.** `lastScanAtMs: number | null` (from `library_roots.last_scan_at_ms`) and `lastJob: { jobId, status, errorCode, finishedAtMs } | null` (the newest job row of the root; `errorCode` is `null` unless `failed`), plus `activeJobId: string | null` (from the manager's map). Rust DTO, Zod schema and the wire-format tests change together. After a window reload the UI has lost its channel: if any root has an `activeJobId`, it shows "A scan is running" with Cancel and refreshes the list **once every two seconds** until none has (the only polling in the app; say so in the report).

**(k) UI (Library Folders card), designed states, fixed English texts in one file.** Per root: a **Scan** button (disabled, with the reason as visible text, when the root is not `Active` or not `reachable`); while running, a line "Scanning… N files in M folders" and a **Cancel** button; after `finished/completed` a one-line summary (new, changed, unchanged, moved, marked missing, skipped by rule, warnings); when the marked-missing count is above zero the summary adds one plain sentence, "Missing means: not found on disk during this scan; the files are not deleted by the app"; after `finished/cancelled`: "Scan cancelled. Nothing was marked missing."; after `failed`: the fixed text of the code, never a raw code; for `lastJob` with `interrupted`: "The last scan did not finish (the app closed). Press Scan to run it again." Remove is disabled while the root scans. All texts live in one module (`src/lib/scanText.ts`, spec D7). **Warning texts:** one fixed English sentence per warning code, and the unknown-code fallback is a generic sentence that **does not print the code** (carried nit from 1C2). Add `ALL_WARNING_CODES` in Rust (next to `ScanWarning`) and a drift test that it equals the keys of the TypeScript table, in the style of the error-code drift test; also assert that every `code:` string in `scanner/walk.rs` and `scanner/mod.rs` that builds a `ScanWarning` is in the list (a test that reads the two source files is acceptable).

**(l) Poison and hooks (carried nits).** A poisoned `Mutex` in the manager is recovered with `into_inner()` as `guard.rs` does, and **no new error variant is added** (this closes the 1C2 nit: the answer is "recover, do not invent a variant"; say so in the report). The test hooks of 1D (`failure_hook_after_batch`, `failure_in_move_step`, `failure_in_final_marking`, `panic_in_walker_after_batch` in `ScanOptions`, `panic_after_batch` in `WalkParams`, and the two `panic!` calls in `walk.rs`) are put behind `#[cfg(test)]`, so the production `ScanOptions` has only `batch_size` and `generation`. `start_scan_impl` builds `ScanOptions` without any hook by construction. Check with `cargo clippy` and `cargo test` that nothing else broke, and show `rg -n "panic!" src-tauri/src/scanner src-tauri/src/jobs` in the report with each hit labelled test or production.

## Goal
The owner clicks **Scan** on a library folder in the app and sees the count rise; **Cancel** stops it and says nothing was marked missing; closing the app mid-scan and reopening shows "did not finish" instead of a stuck "running"; a folder being scanned cannot be removed; and a job that panics is a failed job, not a dead app or a stuck button.

## In scope

### Part A: report section 1 (before any code)
What you found in `scanner/`, `commands/`, `lib.rs`, `database/`, `src/lib/tauri/` and `LibraryFolders.tsx` (keep, change, add, and why); the exact job lifecycle as a table (state, who writes it, when); the throttling design (watch, ticker, the two rates); the migration; how `remove_root_impl` and the guard interact; what you will do about the `ScanOptions` hooks.

### Part B: Rust
1. `src-tauri/migrations/<timestamp>_create_jobs.sql` (decision (b)).
2. `src-tauri/src/jobs/` (new module; create it now, §4 lists it): `mod.rs` (`JobManager`, `JobId`, `JobKind`, `JobStatus`, `run_tracked_job`), `repo.rs` or additions under `database/` for the `jobs` rows (a repository function never opens its own transaction; the caller does). `lib.rs` declares the module, manages `JobManager`, runs the reconciliation (f) in `setup`, registers the new commands.
3. `src-tauri/src/scanner/`: `scan_root_guarded`, `ScanOptions.generation`, `ALL_WARNING_CODES`, the `#[cfg(test)]` gating (l). No change to what the scan does.
4. `src-tauri/src/commands/`: new `scan.rs` with `start_scan`, `cancel_scan` (thin wrappers) and `start_scan_impl`, `cancel_scan_impl`; `roots.rs`: `RootSummary` fields (j) and `remove_root_impl` (i); `error.rs`: the three codes, the fixed texts, the tests.
5. No new crate. If you believe one is needed, stop and report.

### Part C: tests (names that say what they prove; temp directories; both OSes in CI)
Generated trees only; the manager is tested through `start_scan_impl` with a recording sink.
- **Lifecycle:** a completed scan writes `started`, some `progress`, `finished(completed)`; the row goes `running` to `completed` with counts and times; `last_scan_at_ms` is set. A cancelled scan (cancel from the sink or from a second task) gives `finished(cancelled)`, the row `cancelled`, **zero rows `missing` that exist on disk**. A scan of a deleted root path gives `failed(scan_root_unreachable)` and the row `failed` with that code.
- **Exactly one terminal event, last, nothing after it**, in all three outcomes and in the panic case; `started` is always first.
- **Refused start:** a second `start_scan` for a root that is running returns `scan_already_running` **and creates no job row** (count rows before and after); after the first ends a new start works.
- **Persistence before touching:** with a hook (test-only) that stops the scan before the first batch, the job row already holds the generation in `payload_json` and the root has no `last_scan_generation`.
- **Throttle:** feed the watch 1,000 values in a fast loop with paused tokio time: at most four `progress` events per simulated second, the **last** value is never lost (the terminal event carries the final counts), and the job row is written at most once per simulated second plus the final write.
- **Dead channel:** a sink that starts failing after the second event does not stop the job; the row still ends `completed`.
- **Reconciliation:** a database with rows `running`, `cancelling`, `queued` and `completed` → after the start-up step the first three are `failed/interrupted` with `finished_at_ms`, the last is untouched; running it twice changes nothing; a root's `lastJob` shows `interrupted`.
- **Panic isolation:** `run_tracked_job` with a future that panics: the row becomes `failed/internal`, one `failed` event is sent, the `ScanGuard` for that root is free (a new scan starts), and the test process is still alive. Show it failing when the supervisor is replaced by a plain `tokio::spawn` that ignores the result (name what you mutated).
- **`remove_root` while scanning:** returns `root_busy` while a scan holds the guard (use a blocked scan, as the 1D guard test does); succeeds after the scan ends; a scan that starts during a removal gets `scan_already_running`. Show the busy test failing with the guard acquisition removed from `remove_root_impl`.
- **Retention:** after 25 finished jobs for one root, 20 rows remain and they are the newest; running jobs are never deleted.
- **Cancel unknown:** `cancel_scan` for an unknown or finished job is `job_not_found`.
- **Wire format:** `ScanEvent` and the new `RootSummary` fields serialise to the camelCase shape the Zod schemas parse (a test that serialises a sample of each variant and compares it with a literal JSON string, as the existing wire-format tests do); `ALL_CODES` equals `errorCodes.ts` (existing drift test, extended); `ALL_WARNING_CODES` equals the TypeScript warning table.
- **Migration:** as in decision (b); the existing app-info test that asserts the migration count is updated to 4 and still fails if a migration file is removed.
- `pnpm exec tsc --noEmit` and `eslint src/` clean. If you can test the pure part of the UI state (a function from events to display state) without a new dev dependency, do; if not, say so in the report instead of adding a runner.

**Show these failing and passing, with the mutation named and reverted:** (1) the panic supervisor replaced by a plain spawn; (2) the guard acquisition removed from `remove_root_impl`; (3) the throttle removed (send every watch value) so the rate test fails.

### Part D: UI
`src/lib/tauri/schemas.ts` (event union, new `RootSummary` fields, `StartScanResult`), `src/lib/tauri/index.ts` (typed `startScan(rootId, onEvent)`, `cancelScan(jobId)`; the only place that calls `invoke` and creates a `Channel`; the rest of the app never imports `@tauri-apps/api`), `src/lib/tauri/errorCodes.ts`, `src/lib/scanText.ts` (all texts and the warning table), `src/components/LibraryFolders.tsx` (decision (k)). Parse every event with Zod before using it; an event that fails to parse is dropped and shown as "Something went wrong while reporting progress" at most once per scan, never crashes the card.

### Part E: carried fixes (before the report)
1. The decision (l) items (poison, hooks).
2. `docs/ipc.md`: add the commands, the event shape and the rules of decision (g) in the same style as the existing entries (this is the one doc you may edit).

### Part F: report
`reports/REPORT-1E.md` from the template, with the rules below.

## Out of scope (do not do)
- A queue, priorities, pause, resume, persisted checkpoints beyond the row updates above, auto-scan at start-up or on a schedule, scan-all, retry (the owner's decision when he wants any of these).
- Metadata, thumbnails, hashing, any other job type; offline roots, drive-returned handling, a root's `status` changing (1F); watchers (1F); showing absolute paths of scan results.
- A new crate or npm package, a frontend test runner, a change to `capabilities/`, `tauri.conf.json`, `deny.toml`, `ci.yml`, `AGENTS.md`, the spec, `docs/STATUS.md`, `docs/NEXT_STEPS.md`.
- New `unsafe`. The start-up window issue (STATUS risk 55) stays as it is.

## Allowed paths
`src-tauri/migrations/` (one new file), `src-tauri/src/jobs/` (new), `src-tauri/src/scanner/`, `src-tauri/src/commands/`, `src-tauri/src/database/` (jobs repository, `mod.rs`, the roots query for `lastJob`), `src-tauri/src/lib.rs`, `src-tauri/Cargo.toml` (only to confirm no `panic = "abort"`: do not change it), `src/lib/tauri/`, `src/lib/scanText.ts` (new), `src/components/LibraryFolders.tsx`, `src/App.tsx` (only if the card's props change), `docs/ipc.md`, `reports/`.

## Acceptance criteria
- [ ] Report section 1 written first, with the lifecycle table.
- [ ] `git diff --stat main..HEAD` shows only allowed paths; exactly one new migration; no change to `capabilities/`, `tauri.conf.json`, `deny.toml`, `ci.yml`, `Cargo.toml`, `package.json`, `pnpm-lock.yaml`.
- [ ] Every test of Part C exists and passes on both OSes in CI; the three mutation demonstrations are pasted with failing output and the revert.
- [ ] No `unwrap`/`expect` in production code of the new files; `rg -n "panic!" ...` shows no production hit; no `unsafe` added.
- [ ] `ScanOptions` in production has no test hook (paste `rg -n "failure_hook|failure_in|panic_in|panic_after" src-tauri/src` with every hit inside `#[cfg(test)]`).
- [ ] `pnpm verify` green locally; `git status --short` empty afterwards; `gh run list --branch task/1E --limit 3` pasted, both OSes and cargo deny green; a red run is explained from its failed log (the owner supplies it; remember STATUS risk 59: an advisory can turn it red with no code change).
- [ ] Section 7 (deviations) lists anything done differently and every extra file touched.

## Verification commands
```
pnpm verify
cargo test --manifest-path src-tauri/Cargo.toml
git status --short
gh run list --branch task/1E --limit 3
rg -n 'unwrap\(\)|expect\(' src-tauri/src/jobs src-tauri/src/commands/scan.rs
rg -n "panic!" src-tauri/src/scanner src-tauri/src/jobs
rg -n "failure_hook|failure_in|panic_in|panic_after" src-tauri/src
rg -n "unsafe" src-tauri/src
rg -n "tauri::command" src-tauri/src/scanner src-tauri/src/jobs
rg -n "@tauri-apps/api" src
git diff --stat main..HEAD
```
(`unwrap`/`expect`/`panic!` hits inside `#[cfg(test)]` are fine; say which hits are tests. `@tauri-apps/api` must appear only under `src/lib/tauri/`.)

## Report rules (STATUS risk 26: the reports have contained invented text)
Anything quoted as output must be raw output of a command you ran in this task; anything else is labelled "from source, not run". **Every test count must come from a line of a log you pasted; a number you did not see is written "inferred" and says from what.** Put the final commit id, the report commit id (the real one, not `HEAD`) and the last CI run id in the header, taken from `git log` and `gh run list`; every CI id cited anywhere must come from a `gh run list` you ran. Paste `git diff --stat` verbatim including the report itself, without `...`. Do not write a macOS or Windows version into the header unless it is pasted raw `sw_vers` or `ver` output. Paste every grep as the raw command and its raw output (an empty result is the command followed by nothing). Say what you mutated in every failure demonstration and show the revert. "Concepts used": at most 5, each with a `file:line` you checked in the final tree. Do **not** propose the next task. The header, the files-changed block and the "all criteria met" line must match the final state of the branch, and "all criteria met" is only allowed next to a green latest run.

## Stop conditions
Stop and report (do not guess, do not work around) if: a new crate or npm package seems needed (including a frontend test runner and `tokio-util`); a change to `tauri.conf.json`, `capabilities/` or `ci.yml` seems needed (for example for the Channel); the guard cannot be made to cover `remove_root` without a deadlock; a migration beyond the one `jobs` file seems needed; `Channel` cannot be used from the pinned Tauri version; you find a conflict between this file, the spec and AGENTS.md; or an acceptance criterion cannot be met on one OS.

## Manual check (the owner, after the agent: Mac and Windows laptop)
With the app (`pnpm tauri dev`): add a folder with photos; press **Scan** and watch the count rise (first scan of the owner's `Pictures` takes about 4 seconds on the laptop, so cancel may be too late there: try it on the largest folder you have, and say if it was too fast); **Cancel** shows "Scan cancelled. Nothing was marked missing."; scan again to the end and read the summary; try **Remove** during a scan (it is disabled; after the scan it works and shows the counts); start a scan and **close the app window**, reopen, and read "The last scan did not finish (the app closed)". Screenshots of the card in three states (scanning, summary, did-not-finish) on each OS. The orchestrator will give the exact commands.

## Deliverable
`reports/REPORT-1E.md` (use `reports/_REPORT_TEMPLATE.md`; raw outputs, a run log from `gh run list`, "Concepts used" at most 5). Then stop. Do not start 1F.

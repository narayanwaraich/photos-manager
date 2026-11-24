# REVIEW-1E: jobs and the first Scan button, round 1

**Date:** 2026-11-24 · **Reviewer:** orchestrator · **Verdict: FIX (round 1).** The design is right and the code I read does what the decisions say. Four things must change before the merge: one test that fails on your Windows laptop (a race in the test, not in the app), one real race in the cancel path, one UI path that reports a failed scan while the scan is still running, and two tests that do not prove what the task asked. Every fix is small. No fix touches the scanner, the migration or the guard.

## What I read, and from where

- A fresh clone of the public repository: `origin/task/1E` at `c8e202f` (report) on `eab69f4` (the code) on `331b3af` (your sync-24 commit). 25 files, +3567 / -75 (your `git diff --stat main..HEAD`).
- **Read in full:** `jobs/mod.rs` (585 lines), `database/jobs.rs`, the migration `20261124000000_create_jobs.sql`, `commands/scan.rs`, `components/LibraryFolders.tsx`, `lib/scanText.ts`; the diffs of `scanner/{mod,types,walk}.rs`, `commands/{roots,error,app_info}.rs`, `lib.rs`, `database/roots.rs`, `library/roots.rs`, `domain/roots.rs`, `src/lib/tauri/{schemas,index,errorCodes}.ts`.
- **Tests in `jobs/tests.rs` read:** all of them except the wire-format test (lines 830 to 889) and the test fixture header (lines 1 to 27).
- **Report:** sections 1 (partly), 5 (the first mutation), 6, 7, 8, 9, "Concepts used".
- **Your evidence:** the laptop `verify-1E.txt`, the two empty status files, the six screenshots, the Mac terminal output you sent last (`pnpm verify`, `gh run list`, three `pnpm tauri dev` starts), `verify-1D-final.txt` and `scan-report-1D-final.txt`.

**Not done by me:** I ran no Rust test and no UI (this session cannot build the project). I did not read `docs/ipc.md`, the 83 added lines of `scanner/tests.rs`, or the migration test in `database/mod.rs` (their tests are green in your Mac log; I did not check what they assert). I have no job log of the green CI run `45376219399`; its test counts (156 Windows, 160 macOS) come from the agent's report, and the Mac count (160 passed, 2 ignored) is also in your own terminal.

## The laptop failure: a race in the test

`jobs::tests::test_throttle_rate_limits_progress_and_job_updates` failed on the laptop at `src\jobs\tests.rs:459` ("Terminal event must carry final 1000 count"); `155 passed; 1 failed; 2 ignored` (your `verify-1E.txt`, lines 183 to 203). The same test passed on your Mac (160 passed) and on the Windows CI runner (156 passed, run `45376219399`).

Why it fails, from the code (I did not reproduce it):

1. The test starts `run_tracked_job` and **drops the `JoinHandle` it returns**. It then sends the stop signal and does `sleep(50 ms)` and reads `recorded.last()`.
2. After the stop signal the supervisor still has to leave its loop, take a pool connection, open `BEGIN IMMEDIATE`, update the row, prune, **commit to disk**, and only then call `sink(Finished)` (`jobs/mod.rs` lines 553 to 581). A commit that takes longer than 50 ms (a laptop disk, other tests writing at the same time) leaves a `Progress` event as the last event. That is exactly the message in the log: the last event exists but is not a `Finished` with 1000 files.
3. The test cannot fail the other way: the worker returns 1000 files by construction, so a `Finished` with a wrong count is impossible.

So the **app** is right (the terminal event is sent after the row is committed, which is the order the UI needs: when it asks for the list again, `lastJob` is already final). The **test** guessed how long the commit takes. It passes on three machines and fails on a fourth; on CI it is a red run waiting for a slow day. The agent's own panic test already does the right thing (`supervisor_handle.await`), so the fix is known.

## Findings

### Blocking (round 2)

**R1. The throttle test waits on the clock, and tests less than the task asked.**
- Fixed `sleep(50 ms)` after stop (above). Fix: keep the `JoinHandle` and `await` it, then read the events.
- The task asked for "at most four `progress` events per simulated second" and "the job row is written at most once per simulated second plus the final write". The test uses real time, and asserts `<= 5` events for about 650 ms of real time, which is not a rate. And it **never checks the row writes at all**: nothing in the file counts them. (`tokio::time::pause` needs tokio's `test-util` feature, i.e. a `Cargo.toml` change, which the task forbids; real time is acceptable, but the deviation must be in section 7 and the checks must still be real.)
- Fix: measure the elapsed time between the first pumped value and the stop signal, and assert `progress_events <= elapsed_ms / 250 + 2` and at least 1; count the row writes with a SQLite trigger created inside the test on its own temporary database (an `AFTER UPDATE ON jobs` trigger inserting into a log table) and assert `writes_before_final <= elapsed_ms / 1000 + 1`; assert the final row (`progress_done` 1000, status `completed`). No fixed sleep after the stop signal.

**R2. Cancel can overwrite a finished job's status (real bug, narrow window).** `cancel_scan_impl` finds the job in the map, sets the flag, **then** runs `UPDATE jobs SET status = 'cancelling' ... WHERE id = ?` (`database/jobs.rs` lines 87 to 101) with no condition on the current status. If the scan finishes and commits `completed` or `cancelled` between those two steps (the map entry is removed before the final commit, and a three-second scan makes a late Cancel click likely), the update writes `cancelling` over the final status. The row then stays `cancelling` until the next start-up turns it into `failed/interrupted`, a false "did not finish" for a scan that did. Fix: `... WHERE id = ? AND status = 'running'`, ignore "0 rows changed" in `cancel_scan_impl` (the flag was set; the job ended by itself). Tests: a repository test (a `completed` row stays `completed` after the call; a `running` row becomes `cancelling`).

**R3. A malformed progress event is reported as a failed scan (the UI does not follow Part D).** In `src/lib/tauri/index.ts` an event that fails Zod is turned into `{ kind: "failed", jobId: "unknown", code: "internal" }` and passed to the handler, which then shows "Scan failed (internal)", drops the live progress line, and the 2 s polling then shows "A scan is running…" under it, while the scan is still running. Part D says: the event is dropped and "Something went wrong while reporting progress" is shown at most once per scan; the `parseError` branch in `LibraryFolders.tsx` (the text, lines 576 to 580) is therefore unreachable today. Fix: `startScan(rootId, onEvent, onInvalidEvent)`; no fabricated terminal event; the handler sets `parseError` once; parse once, not twice.

**R4. The "persistence before touching" test does not test that.** `test_persistence_before_touching_disk` starts a scan, and reads the row after `start_scan_impl` returned. The row is inserted before the function returns, so the test passes with or without the ordering that decision (d) is about; it also does not compare the persisted generation with the one the scanner stamps. The task asked for a hook that stops the scan before the first batch and then checks that the row holds the generation and the root has no `last_scan_generation`. Fix: a private `start_scan_with_options(root_id, options, sink)`, called by `start_scan_impl` with `ScanOptions { generation, ..Default::default() }`, so a test can pass `failure_hook_after_batch: Some(0)` (that field exists in test builds); test A: one file in the root, the scan fails before the first batch, the row has `payload_json.generation` and the root's `last_scan_generation` is NULL; test B: a completed scan leaves `last_scan_generation` equal to the `payload_json.generation` of its row.

**R5. Report corrections** (the agent updates the file; same rules as in the task file). The header names report commit `8950fbd`, which does not exist in the repository (the real id is the commit that adds the report; after round 2, the real latest report commit). Section 7 lacks: the throttle test uses real time and why; `scanner/tests.rs` and `commands/app_info.rs` touched (allowed paths, but list every file); the files added in round 2. The criteria table says "met" for the throttle test and for persistence, which R1 and R4 show is too strong.

### Small, in the same round

**R6. The 2 s polling flickers the card.** `fetchRoots` sets `loading` to true on every call; the polling effect calls it every 2 s, so while a scan runs the Refresh button flashes "Checking…" and "Add folder…" is disabled for a moment each time. Polling must not touch `loading` (a quiet refresh).

### Carried, not in this round (STATUS risks 62 and 63)

- **Silent failure of the final write.** In `run_tracked_job` the four finish paths use `let _ =` on `begin_with`, `finalize_job`, `prune_finished_jobs` and `commit`. If any of them fails (database busy, disk full), the row stays `running` while the UI was told `finished`, and the next start-up marks it `failed/interrupted`. The window is small and nothing else is lost (the scan's own batches are committed), so I do not widen this round; 1F decides whether to retry or report it.
- **The dead-channel test cannot fail.** The sink type is `Fn(ScanEvent)` (decision g), so a "failing" sink cannot be written; the test's sink does nothing after the second event. The real swallow is `let _ = on_event.send(event)` in `commands/scan.rs`, which is correct and from source only. The test name overstates; I accept it with this note.
- `list_roots` runs one `lastJob` query per root (N+1; a handful of roots). `start_scan` has an unused `_pool` parameter. Nits.
- "A scan that starts during a removal gets `scan_already_running`" is not tested on its own; it is the same `ScanGuard` and is covered by the 1D guard test.

## Decisions (a) to (l) against the code

| | Decision | State |
|---|---|---|
| a | One job type, started at once, no queue | met (`JobKind::Scan`; refused start creates no row, tested) |
| b | Migration 4, one file, `root_id` cascade, index | met (22 lines; `Migrations Applied: 4` in your screenshots; migration test is green) |
| c | Cancellation via `ScanCancel`, `job_not_found` | met, with **R2** |
| d | Generation persisted first; guard before the row | met in code (`start_scan_impl` order is right); the test is **R4** |
| e | Row content, 1 write per second, retention in the final transaction | met in code (retention is in the same `BEGIN IMMEDIATE` as the final update); the write-rate test is missing (**R1**) |
| f | Start-up reconciliation | met (`lib.rs`; test runs it twice and checks `lastJob`) |
| g | Channel, throttled, latest wins, one terminal event last | met in code (`watch` plus 250 ms ticker; terminal event after the commit); tests **R1**; UI **R3** |
| h | Panic isolation, generic `run_tracked_job` | met; the panic test awaits the supervisor and checks the guard is free |
| i | `remove_root_impl` takes the guard first | met (`_guard` held for the whole function; `root_busy` tested) |
| j | `RootSummary` grows | met (wire test with and without values; 2 s polling only while an `activeJobId` exists), with **R6** |
| k | UI texts in one file, warning drift test | met (`scanText.ts`, `ALL_WARNING_CODES`, drift test green) |
| l | Poison recovered, hooks behind `#[cfg(test)]` | met: every hook field, the walker `panic!` and the three hook branches are under `#[cfg(test)]` in the diff; poisoned mutexes use `into_inner()` (no new variant). **STATUS risk 60 is closed in code**; it closes for good with the merge |

Also checked: no `unwrap`/`expect` in the production code of the new files (read, not grepped); `Cargo.toml`, `package.json`, `pnpm-lock.yaml`, `ci.yml`, `tauri.conf.json`, `capabilities/` are not in the diffstat; exactly one new migration file; `domain/roots.rs` and `library/roots.rs` are outside the allowed paths but were unavoidable (`RootSummary` lives there) and the report says so: accepted.

## What your manual check showed (and did not)

The screenshots show: the progress line and a disabled Remove during a scan; a completed summary; the "last scan did not finish" message; `Migrations Applied: 4`. They do **not** show the Cancel result ("Scan cancelled. Nothing was marked missing.") or Remove working after a scan. Both are asked again, briefly, in NEXT_STEPS.

## Evidence summary

| Claim | Source | State |
|---|---|---|
| Mac `pnpm verify` green on `c8e202f` (160 passed, 2 ignored), status clean | your terminal | seen |
| CI run `45376219399` green, 7m24s | your `gh run list` | seen (no job log) |
| CI Windows 156 passed, macOS 160 passed | agent's report | not seen in a log by me |
| **Laptop `pnpm verify` red: throttle test, 155 passed 1 failed** | `verify-1E.txt` | seen |
| The laptop failure is a test race, not an app bug | my reading of `jobs/mod.rs` and the test | **read, not reproduced** |
| R2 (cancel overwrites a finished status) | my reading of `cancel_scan_impl` and `update_job_status` | read, not reproduced; round 2 adds the test |
| 1D on the laptop: 141 passed 2 ignored; `scan_report` 2469 files, 953 files/s then 2474 files/s; status file empty | `verify-1D-final.txt`, `scan-report-1D-final.txt`, `status-after-verify6.txt` | seen: closes the open items of 1D (STATUS risk 56) |

## Round 2: what the agent does

Items R1 to R6 only, on branch `task/1E`, then a new report section "Round 2" and the corrected header. No other change. The exact prompt is in NEXT_STEPS. Acceptance for round 2: `pnpm verify` green on the Mac **and on the laptop**, the throttle test run 30 times in a row on each machine without a failure, the latest CI run green, and the files-changed block, header and run ids matching the final branch.

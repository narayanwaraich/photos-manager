# REVIEW-1E, round 2: jobs and the first Scan button

**Date:** 2026-11-25 · **Reviewer:** orchestrator · **Verdict: ACCEPT the code. Merge held until the laptop has run the round-2 code.** All six round-1 items (R1 to R6) are done and match my reading of the diff. The one gate that is still open is evidence, not code: the laptop files that came with this session were produced by the round-1 code, so the machine that found the throttle-test race has not yet run the fixed test.

## What I read, and from where

- A fresh clone of the public repository: `origin/task/1E` at `b021571` (report) on `a07a476` (round-2 code) on `38770fd` (your sync-25 merge). Round-2 diff `38770fd..HEAD`: 6 files, +704 / -157 (`database/jobs.rs`, `jobs/mod.rs`, `jobs/tests.rs`, `LibraryFolders.tsx`, `lib/tauri/index.ts`, `REPORT-1E.md`). All six are allowed paths; no `Cargo.toml`, `package.json`, `ci.yml`, `tauri.conf.json` or `capabilities/` change.
- **Read in full:** the diffs of `database/jobs.rs`, `jobs/mod.rs`, `index.ts` and `LibraryFolders.tsx`; the round-2 throttle test (`jobs/tests.rs` lines 488 to 640); `update_job_progress`; report sections 6 to 10 and the whole "Round 2" section.
- **Read only in part:** `jobs/tests.rs` outside the throttle test (I read the names and the mutation outputs of the cancel and persistence tests, not their full bodies).
- **Your evidence:** the agent's closing summary, `REPORT-1E.md`, your Mac terminal (sync-25 commit, merge into `task/1E`, `pnpm verify`, `gh run list`, the 30-run loop), the laptop `verify-1E-round2.txt`, `throttle-loop-1E.txt` and the empty `status-after-verify8.txt`.
- **Not done by me:** I ran no Rust test and no UI. I did not read the 83 lines of `scanner/tests.rs`, `docs/ipc.md` or the migration test (unchanged since round 1). **No screenshots reached this session.** The manual check is your statement that all screens came out ok.

## Round-1 items against the code

| Item | State | Evidence |
|---|---|---|
| R1 throttle test | **done** | `JoinHandle` kept and awaited, no fixed sleep after the stop signal; elapsed time measured with `Instant`; `progress_events <= elapsed_ms / 250 + 2` and `>= 1`; an `AFTER UPDATE ON jobs` trigger fills a log table; `writes_before_final <= elapsed_ms / 1000 + 1`; final row `completed` with `progress_done` 1000; last event `Finished` with 1000 files. The bounds hold at any machine speed: tokio's interval never fires more ticks than the elapsed time allows (it catches up missed ticks, it does not add any), and a row write needs every fourth tick (my reasoning, not run). Mutation (b) (write on every tick) fails at `tests.rs:614` with "3 writes in 615 ms" |
| R2 cancel race | **done** | `mark_job_cancelling ... WHERE id = ? AND status = 'running'`, returns the rows changed; `cancel_scan_impl` keeps the `?` on a database error and ignores the count; `update_job_progress` touches no status column (read). Test plus mutation (a): without the condition the completed row is changed (`left: 1 right: 0`, `tests.rs:1091`) |
| R3 malformed event | **done** | `startScan(rootId, onEvent, onInvalidEvent)`; one `safeParse`; a bad payload calls `onInvalidEvent()` and is dropped; the card sets `parseError` once per scan; the second parse is gone. The path cannot be reached from the real app, so no test exists (see N2) |
| R4 persistence | **done** | private `start_scan_with_options`; `start_scan_impl` passes `generation: Some(..)` and no hook; the row payload uses `options.generation`. Test A (hook before the first batch: row has the generation, row `failed`, root `last_scan_generation` NULL), Test B (completed scan: root generation equals the row's). Mutation (c) (`generation: None`) fails Test B. Test A itself was not mutated (moving the insert after the scan): I believe it fails then, from reading, not run |
| R5 report | **mostly done** | header: code commit `a07a476` matches `git log`, CI id `45400085275` matches your `gh run list`; section 7 lists the round-2 files and says why the throttle test uses real time. Slips: see N3 |
| R6 flicker | **done** | `fetchRoots({ quiet: true })` in the 2 s polling and in both terminal handlers; the first load and the Refresh button keep the loading state |

Scope: your `git diff --stat main..HEAD` lists the same 25 files as round 1 (4117 insertions, 78 deletions); no new path.

## Evidence: what is closed and what is not

| Claim | Source | State |
|---|---|---|
| Mac `pnpm verify` green on round-2 code: 164 tests (162 passed, 2 ignored), the three round-2 test names are in the list, `git status --short` printed nothing | your terminal | **seen** |
| Mac 30 runs of the throttle test, no failure | your terminal (`loop finished`, no `FAILED`) | seen; the output is discarded, so it shows no failure, not that the test ran (the test name is in the verify list) |
| CI run `45400085275` green on Windows, macOS and cargo deny | your `gh run list` | **seen** (jobs and ids from the agent's `gh run view`, not seen by me elsewhere) |
| CI test counts 158 (Windows) and 162 (macOS) | agent's pasted job lines | Mac 162 is in your own log; Windows 158 is not seen by me, but it is consistent: round 1 had 156, and round 2 added two tests |
| **Laptop `pnpm verify` and the 30-run loop on the round-2 code** | `verify-1E-round2.txt`, `throttle-loop-1E.txt` | **NOT met.** The laptop ran the **round-1** code (see below) |
| Manual check, four items | your statement ("all screens came out ok") | accepted as your word; no screenshots seen; OS, scan duration and whether any scan lasted over 2 s are not stated |

**Why I say the laptop files are round-1 code** (all from the files):
1. `verify-1E-round2.txt` says `running 158 tests` and `156 passed; 2 ignored`. Round 2 adds two tests; the Windows CI run of the round-2 code shows 158 passed (160 tests). Round 1 on the laptop was 155 passed + 1 failed + 2 ignored = 158 tests: the same total as this file.
2. The file lists `test_persistence_before_touching_disk` (the round-1 name, replaced in round 2) and has **no** `test_mark_job_cancelling_only_affects_running_jobs`, no `..._failure_leaves_null_generation`, no `..._matches_root_last_scan_generation`. The Mac log has all three.
3. `throttle-loop-1E.txt` says `157 filtered out` in all 30 runs. With the round-2 test count it would be 159.
4. The file's timestamp is `2026-11-24T12:58:18Z`. The CI job of the round-2 push logs its test result at `13:10:07Z` in a job of 6m55s, so that job started at about `13:03Z`: the laptop run is several minutes older than the push (my inference from the timestamps; the laptop's `git log` is not in the files).

What the files still tell us: the round-1 throttle test passed 30 times in a row on the laptop this time, so the original race did not show again on that code. That says nothing about the round-2 test, which is why the run has to be repeated.

## Findings (none blocks the code)

**N1. A failed scan prints the raw code (mine to own; carried to 1F, first item).** `LibraryFolders.tsx:259` builds ``Scan failed (${validEvent.code}).`` for every failure except `interrupted`. Decision (k) says "the fixed text of the code, never a raw code". My round-1 table marked (k) as met; I read `scanText.ts` and the warning texts and missed this line. It shows only when a scan fails (for example a folder that went away mid-scan), it leaks nothing (codes are fixed strings), and round 2 did not touch it. Fix in 1F: one TypeScript table from code to the same fixed texts as `fixed_message_for_code`, with a drift test in the style of the warning table.

**N2. The report says the R3 path was "tested manually".** Round 2 section: "Pure UI parsing logic was tested manually". No output backs this, and the path cannot be reached from the app. It should have said "from source, not run". The code is right; the sentence is a report slip of the old kind (risk 26).

**N3. Other report slips.** The summary says "All acceptance criteria are fully met" while the laptop gate of this review was open; "from 145 tests (1D baseline) to 162 tests" mixes a total with a passed count (162 passed + 2 ignored = 164 on the Mac); the header names the report commit by a pointer ("the commit that adds this file"), which is ambiguous after a second report commit (the latest is `b021571`). Accepted, not worth a third round.

**N4. "The last value is never lost" is still weak by construction.** The terminal event carries the counts the fake worker returns, so the test cannot fail on a lost last progress value. This is as in round 1 and is accepted (the task's own wording asks only that the terminal event carries the final counts).

## Decision

- **The code of `task/1E` at `a07a476` (report `b021571`) is accepted.** No third agent round.
- **Merge after one more check, on the laptop** (round-1 acceptance criterion: green on the Mac **and on the laptop**, 30 throttle runs on each). The commands are in NEXT_STEPS, with the three markers that prove the right code was tested (`160 tests`, the new test names, `159 filtered out`).
- If the laptop is green: squash-merge, tag `accepted/1E`, then I write the sync (STATUS, this review, lesson, spec v2.1.7: `jobs.root_id` in §5.2, job states and start-up reconciliation in §11) and TASK-1F, with N1 as its first item.
- If the laptop is red: nothing is merged; send the three files and I read the failing line. A red test on a slow disk after the round-2 fix would be a new finding, not a repeat.

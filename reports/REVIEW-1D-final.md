# REVIEW-1D-final: the last commits of TASK-1D, read after the merge

**Date:** 2026-11-24 · **Reviewer:** orchestrator · **Verdict:** 1D stays ACCEPTED. No new defect that needs a fix commit. Four small carried items (below).

## What I read, and from where

- A fresh clone of the public repository (`https://github.com/narayanwaraich/photos-manager.git`), not the agent's report.
- The three final commits on `task/1D`, as one diff: `git diff 728e33b..dc940d4` (`728e33b` is your sync-23 merge). 9 files, +403 / -71:
  - `a483d22` J1 and N1: `scanner/{mod,error,types,tests,walk}.rs`, `commands/error.rs`, `src/lib/tauri/errorCodes.ts`.
  - `00cd903` `cookie` 0.18.2 to 0.18.3: `src-tauri/Cargo.lock` only (4 lines).
  - `dc940d4` the report.
- The squash on `main` (`605664c`, 18 files, +5065 / -94) against the branch head: `git diff dc940d4 main -- src-tauri src` is **empty**, so the code you merged is exactly the code I read.
- `scanner/walk.rs` and `scanner/mod.rs` on `main`, for every way the walker can stop.
- Your Mac terminal (merge, `pnpm verify`, squash, tag, `gh run list --branch main`), the Windows job log of run `45139709422`, the failed log of `45102343589`.

**Not done by me:** I ran no Rust test (this session cannot build the project). I saw no log of the green runs `45184722248` (branch) or `45191956736` (main); the main run's result is yours (`✓ 17m2s`), the branch run's is the agent's pasted `gh run list`.

## J1: the walker that dies (the one open correctness item): closed

`scan_root` now ends the writer loop, drops `rx`, and then does

```rust
walk_handle.await.map_err(|_| ScanError::WalkFailed)?;
```

before the cancellation check and before the final transaction. A panic in the walker thread is a `JoinError`; it becomes `ScanError::WalkFailed` (code `scan_walk_failed`) and the function returns before `detect_and_apply_moves` and `finalize_scan_marking`, so no row is touched after the last committed batch. The `ScanGuard` is a Drop guard, so the root is released (the test proves it).

I also checked the other ways the walker can end, because J1 only helps if there is no fourth, silent one. In `walk.rs` the only exits are: the loop finishing; `break` when the cancel flag is set (the writer then returns `Cancelled`, nothing marked); and `return` when `blocking_send` fails, which can only happen after the writer has dropped `rx` (an error path that never reaches the final transaction). Everything else inside the walk is a warning plus `continue`. So the only silent way left was the panic, and it is now an error.

The test (`test_walk_failure_returns_scan_walk_failed_leaves_zero_missing_and_releases_guard`) is a real test of the right thing: it calls `scan_root` (no re-implemented logic), scans a three-file tree, deletes one file on disk, scans again with batch size 1 and a walker panic after the first batch, and asserts (1) `Err(ScanError::WalkFailed)`, (2) zero `missing` files and zero `missing` folders, (3) a third scan starts and completes and marks exactly the deleted file. The agent's mutation demo (the old `let _ = walk_handle.await;` restored) fails with `marked_missing: 2`, which is exactly the harm J1 describes (two unreached rows declared missing). The failing assertion is the first one, so assertion (2) is never seen failing on its own; the `marked_missing: 2` in the same output covers that.

`scan_walk_failed` is in all four places: `ALL_CODES`, `fixed_message_for_code` (fixed text, no path), `errorCodes.ts`, and both error tests (the exhaustive `match` and the leak test with an empty secret list, which is right for a variant with no data).

## N1 and the lock file

- N1: `FolderStatus` is imported inside the Unix-only test, so the Windows warning at `scanner/tests.rs:11` (seen in the Windows job log) is gone. The CI warning does not fail the build (`-D warnings` applies to clippy, which passed), so this was hygiene.
- `cookie` 0.18.3: RUSTSEC-2026-0594. The branch run `45181104275` was red for this advisory, not for J1 (the report's run list shows it directly under the J1 commit, which looks like blame; it is not). Lock-file only, no source change.

## Findings

All small. None needs another agent round on 1D.

1. **Test hooks are compiled into production types (carried to 1E, STATUS risk 60).** `ScanOptions` has `failure_hook_after_batch`, `failure_in_move_step`, `failure_in_final_marking` and now `panic_in_walker_after_batch`; `WalkParams` has `panic_after_batch`; `walk.rs` contains two `panic!` calls reachable through them. Nothing can set them today (no command exists), and `panic!` is not an `unwrap`/`expect`, so the task's grep did not look for it. 1E adds the first caller that is not a test, so 1E must gate the fields and the `panic!` with `#[cfg(test)]`, or construct `ScanOptions::default()` only.
2. **The report header is not final.** It names `HEAD` as the report commit; the id is `dc940d4`. Cosmetic.
3. **The Windows count for the last run is inferred, not seen.** The report says 141 passed on Windows for run `45184722248` and says "all 141 ... passed" in its Windows paragraph. The only Windows log in evidence is run `45139709422`, which shows 140 passed, 2 ignored. 141 is plausible (the Mac went 144 to 145 and the J1 test is not OS-specific), but nobody has shown it. The summary also mixes totals (142 for Windows, 147 for the Mac). I recorded 141 as "inferred" in STATUS.
4. **Process, not code:** the squash commit message on `main` says "accepted (squash of task/1D)", which is the convention; nothing to change.

## Evidence summary (what is solid, what is not)

| Claim | Source | State |
|---|---|---|
| The merged code is the reviewed code | my `git diff dc940d4 main -- src-tauri src` (empty) | seen |
| J1 is implemented as described | the diff and `mod.rs` on `main` | read |
| J1 test is real and fails without the fix | the diff; the agent's pasted failing output (`marked_missing: 2`) | read; mutation output is the agent's |
| `pnpm verify` green on the merged tree (145 passed, 2 ignored) | your Mac terminal | yours |
| Main CI green for the squash | `gh run list --branch main`: `45191956736` ✓ 17m2s | yours |
| Branch CI green (`45184722248`) | the agent's pasted `gh run list` | agent's |
| Windows 141 tests, final run | nothing | inferred |
| Laptop: final code, `pnpm verify` and `scan_report` | nothing yet | **open** (NEXT_STEPS step 3) |

## Decision on the carried items

Findings 1 and 2 go to the 1E task file (Part F). Finding 3 is a rule in the 1E report rules: every test count must come from a line in a log you pasted, or be marked "inferred".

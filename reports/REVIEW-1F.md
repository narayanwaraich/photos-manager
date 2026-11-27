# REVIEW-1F: offline roots and honest jobs (first Gemini 4 Hydrogen task)

Reviewer: orchestrator, 2026-11-26. Branch `task/1F` at `57fd8da` (docs-only) on code `cfee5b4` on part A `97815c4` on `main` at `8d9fc70` (sync-26).

## Verdict: ACCEPT the code (round 1)

Merge after one owner check: the offline flow on the Mac (rename a folder, "Not available", **Find folder…**, wrong folder refused). Why this one blocks the merge: the whole task is about screens that nobody has seen yet (the screenshots you sent show the 1E Cancel text and the Remove counts, not one 1F state). It takes five minutes. The laptop check is not blocking; it becomes part of the Windows pass that closes Phase 1.

The report slips below are real but small. They do not need another agent round; they are carried (see "Carried").

## What I checked, and how

Evidence is labelled. "Read" = I read it in a fresh read-only clone of `origin/task/1F` at `57fd8da`. "Not run" = I ran no Rust test, no mutation, no UI.

| Check | Result | Source |
|---|---|---|
| Scope | 21 files, +4116 / -301 (matches the report's block except its own line, see R2). No `migrations/`, `Cargo.toml`, `package.json`, `ci.yml`, `tauri.conf.json`, `capabilities/` file in `git diff --name-only origin/main..HEAD` | my clone, read |
| Size | about 3,900 changed lines without the report (4116 + 301 − 519). Under the 6,000 cap, two themes | arithmetic on my diffstat |
| Production `unwrap`/`expect`/`panic!`/`unsafe` | none. Every `unwrap`/`expect` in the added lines is after the `#[cfg(test)]` line of its file (`library/roots.rs:619`, `commands/roots.rs:335`, `platform/mod.rs:55`); `availability.rs`, `database/roots.rs`, `database/volumes.rs` have none; added lines containing `unsafe`: 0 | my greps, read |
| Test hooks | `JobManagerTestDeps`, `set_test_deps`, `get_test_deps`, the `hook` parameter, its use at `jobs/mod.rs:837` and the `test_deps` field are all under `#[cfg(test)]` (risk 60 stays closed). `let _ = attempt;` is under `#[cfg(not(test))]`: it is not a leak, only a parameter that exists for the test hook | `jobs/mod.rs` 199–253, 593–640, 836–843, read |
| Status function (decision a, b) | `apply_availability_result` writes only through `set_root_status`; Active+NotAvailable→Offline, Offline+Available→Active, `Unknown` returns the stored status unchanged (it does not even open a write transaction), Paused and Removed fall into `_ => None`. No `files`/`folders` write in `library/availability.rs`, `commands/roots.rs`, `database/roots.rs`; the four `INSERT INTO files/folders` hits in `library/roots.rs` (lines 892, 907, 994, 1340) all lie after line 619, i.e. inside the test module (fixtures) | `library/availability.rs` 120–194, grep, read |
| Final write (decision k, risk 63) | one `try_finalize_once` (`BEGIN IMMEDIATE`, `finalize_job`, `prune_finished_jobs`, `commit`) and one `finalize_and_prune_with_retry` with exactly one retry after 200 ms, returns `bool`; the four old copies are gone (one call site at line 839). Logging is `eprintln!` (see D-1 below) | `jobs/mod.rs` 588–660, read |
| Relocate (decision f) | `ScanGuard::acquire` before anything else and held for the whole function (`library/roots.rs:421`); content check is `matched * 2 < total` → refuse, so exactly half passes (matches the task) | `library/roots.rs` 376–485, read (lines 422–481 not read line by line) |
| Scan pre-flight (decision e) | the direct `root_path.is_dir()` is removed from `scan_root` and replaced by `check_root_availability` with the stored `fs_uuid` | `scanner/mod.rs` diff, read |
| Serial identity (decision h) | `format_serial_identity` gives `serial:` + 8 uppercase hex; `same_volume_identity` accepts the legacy bare serial; both live in `platform/mod.rs`, so both OSes test them | `platform/mod.rs` diff, read |
| UI (decision j and Part D) | `formatScanFailure` replaces the raw code; Offline badge, detail line, **Check again**, **Find folder…**, Scan disabled with the reason; `recorded === false` adds the "could not be saved" line on all three terminal paths; a `failed` last job shows its sentence, not only `interrupted`; relocate does not start a scan | `LibraryFolders.tsx` diff, read |
| Gates | CI run `45748846825` green: Windows 182 tests (180 passed, 2 ignored), macOS 186 (184 passed, 2 ignored), cargo deny. Mac `pnpm verify` at `57fd8da` green (184 passed). **Laptop `pnpm verify` at `57fd8da` green** (`verify-1F.txt`: 180 passed, 2 ignored, every step PASSED; `status-after-verify10.txt` empty). Header and CI id agree: the CI run is for code commit `cfee5b4`; `57fd8da` is docs-only | owner's output |
| Part C names | every test name of Part C is in both logs (list in the session notes: the volume guard, the round trip, the three-outcome test, relocate success and refusals, remove while offline, three case-refresh tests, serial tests, dead check at scan start, in-job vanish, four final-write failure paths, the retry test, the active-job test, the drift and wire tests) | both logs |
| Mutations M1 to M4 | each has a named mutation, the failing output and the revert in the report; M2's output is shortened with `...` (R4). I did not re-run them | report, not run |

Not checked by me (so not part of this verdict): the bodies of most Part C tests (I read the opening of `status_changes_only_on_definite_answers` and nothing else, so "no hollow test" is **not** established for the rest), `relocate_root_with_deps` lines 422–481, the 932-line `library/roots.rs` diff beyond the parts above, `database/volumes.rs`, `platform/windows.rs`, the Zod and error-code files, `docs/ipc.md`. The agent's mutation demos are the evidence that the four most important gates can fail; they are not re-run.

## Decisions

- **D-1: `eprintln!` instead of `tracing::error!`: accepted.** This is my defect, not the agent's: decision (k) required `tracing::error!`, while the same task forbade `Cargo.toml` changes, and `tracing` is only a transitive dependency. The agent chose the stop-free reading, said so in section 7 and asked in section 9. Answer: keep `eprintln!` (the file already uses it at `lib.rs:180, 212, 220`); a direct `tracing` dependency, with a subscriber, is a decision for a task that names it. Not now.
- **D33 (offline roots) → Accepted with 1F**, subject to the owner's check.
- **Risk 63** (final job write): closed in code. Known gap, stated in the report and accepted: if even the retry fails, the row stays `running` until the next start-up turns it into `failed/interrupted`; the UI says so (`recorded: false`).
- **Risk 54**: scan-start half closed (the pre-flight cannot stall an executor thread); the walk itself is still not cancellable inside one blocking call. Stays open for that half, as the task said.

## Findings (all non-blocking; none needs the agent)

- **R1** Header: "Report commit: see `git log --oneline -n 1 -- reports/REPORT-1F.md`" is not an id (real: `57fd8da`), and the summary says "all criteria met" while the manual UI checks were not run (the old class: "all criteria met" next to an open item).
- **R2** Section 3's diffstat is stale: it shows `reports/REPORT-1F.md | 116 +++++`; the final report is 519 lines (my own stat: 519). The task said "verbatim including the report itself".
- **R3** Section 5.4 greps contain literal `...` and `(test invocations)` placeholders and hand-condensed `unsafe` line lists; the `unwrap` grep is post-processed with `cut | sort | uniq`; "inside `#[cfg(test)]`" is argued with line-number thresholds in prose, not with the raw `grep -n "#\[cfg(test)\]"` output (my own greps above confirm the claim is true).
- **R4** M2's failing output is cut with `...` inside struct dumps without saying so.
- **R5** `jobs/mod.rs:596`: a parameter (`attempt`) that only the test hook uses is in the production signature. Harmless; a nit.
- **R6** No sentence in the report says "tested manually" without output, and none of the quoted outputs I could compare was invented. The Claims table exists and its ids match the owner's terminal (`57fd8da`, `cfee5b4`, `45748846825`, 184/180 passed).

## Calibration record for Gemini 4 Hydrogen on 1F (docs/AGENT_PROFILE.md §4)

| Item | 1F |
|---|---|
| Rounds to accept | 1 (pending the owner's Mac check; no code finding) |
| Blocking findings by class | none. Hollow or weakened tests: none seen in what I read (not all read). Real defects found only by review: none found by me. Report statements without a source: four slips (R1 to R4), no invented output seen |
| Failures only on the laptop | none (180 passed, status empty) |
| Stop condition | none hit; one honest deviation (`eprintln!`) caused by my inconsistent task text; the agent ran Part A first and committed it separately (`97815c4`) |
| Forbidden paths or secrets | none (my `git diff --name-only`); D17 not in play |
| Size | about 3,900 changed lines, two risk themes, 21 files, no migration, no crate |
| Baseline (3.8 Flash) | about 2 rounds per task; every task had report claims without a source and most had hollow tests; real defects were found only by review |

Hypotheses: **H1** (fewer unsupported statements): supported on this one task (slips are formatting and staleness, not invention). **H2** (fewer hollow tests): not contradicted; not established (I read few test bodies). **H3** (respects stop conditions and D17): supported (one question asked instead of a silent workaround). **H4** (handles the larger scope in ≤2 rounds): supported by this task: 1 round. **H5** (stale cutoff still appears): no sign here (no new crate or API). One task is one data point; the agent profile now says how to use it.

## What it says about going much bigger (the owner's question)

My view, then the decision it leads to (recorded in `docs/AGENT_PROFILE.md` §6):

1. **One clean round on about 3,900 lines is enough to stop treating 6,000 as a hard cap, and not enough to know where the real limit is.** The cheapest way to find it is the one you proposed: go big and watch for the first sign of trouble.
2. **The binding limit is probably my review, not the agent's work.** A 10,000-line task is one I read in two sessions, from a clone, and the slips I find are in the places I did not read. So big tasks must come with their own evidence (named gates, mutation demos shown failing, a per-theme commit that is green on its own) and I will say plainly in each REVIEW what I did not read.
3. **Big means more themes, not looser rules.** Each theme keeps its gate tests and mutation demos; the evidence rules, stop conditions, D17 and the Claims table do not change.
4. **Where "go big" does not apply:** work that moves or deletes your photos (Phase 5: journal, execute, recovery, undo, Trash) stays designed first and split; a task never crosses a phase gate or the Windows manual pass; a native-dependency build is not mixed with feature code.
5. **Shrink triggers** (so the experiment teaches something instead of just failing): 3 or more rounds; any invented evidence; a weakened or hollow test found; a stop condition ignored; a forbidden path or secret touched; the agent leaving a theme half done to start another. One trigger means the next task goes back one size; two in one task means back to the 1F size.
6. I changed one variable at a time: TASK-1G is bigger but the effort stays `high`. If it goes well, `xhigh` is a separate later experiment on a concurrency-heavy task.

## Carried

R1 to R5 stay in `reports/REPORT-1F.md` as they are. The 1F manual check (Mac blocking, laptop later). The DEP0190 warning from `scripts/verify.mjs` (Node: `shell` option with args; harmless) goes into 1G. Known 1F limits (from the report): a root with a NULL `fs_uuid` skips the guard and cannot be relocated; Windows serial collisions between two devices cannot be told apart; the walk stage on a dead share is not cancellable.

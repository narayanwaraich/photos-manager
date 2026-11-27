# TASK-1G: Watcher hints, ExifTool call hardening, start-up window (the first "go big" task)

**Phase:** 1 · **Branch:** `task/1G` (from `main` after the 1F merge and sync-27) · **Depends on:** TASK-1F accepted and merged (tag `accepted/1F`)
**Effort (set by the owner in the picker): `high`.** You cannot see it; write "Effort (owner-reported): high" in the report header.
**Orchestrator note (D34, `docs/AGENT_PROFILE.md` §6).** The owner decided to give you really big tasks first and find your limit from there. This task is the old 1G plus the old 1H in one, with **three themes**, about 20 decisions and a size limit of about 10,000 changed lines. The rules do not get looser: every theme keeps its gate tests and its mutation demo, the Claims table is required, the stop conditions and D17 are unchanged. **Build it as three commits, one per theme, in the order C, B, A, each with `pnpm verify` green and pushed before you start the next.** If you must stop (size, a stop condition, a blocker), stop after a green theme commit and say exactly what is left: the owner can then merge a smaller task. Never leave a theme half done and start another.

Nothing in this task touches a photo on disk. You do not touch secrets, tokens, repositories, releases or settings (AGENTS.md, D17). This task needs none.

## Read first (only these; do not load the whole spec)
- `AGENTS.md`; `IMPLEMENTATION_SPEC.md` (v2.1.8 after sync-27): §3.3 (ExifTool call rules), §6.1, §6.2, §11, §12.1, D30, D32, D33, D35; `docs/STATUS.md` risks 16, 54, 55.
- Code, read before changing it (verify every line number; they are from the orchestrator's read of `main` at `c61dad7` and of `task/1F` at `57fd8da`):
  - `src-tauri/src/metadata/mod.rs` (the whole file: `ExifToolAdapter::extract_metadata`, lines 94 to 205; `DEFAULT_TIMEOUT` 5 s, `MAX_OUTPUT_BYTES` 512 KiB; the Unix-only `test_child_killed_after_timeout`).
  - `src-tauri/src/lib.rs` (`run`, the `.setup` closure, `show_startup_error_dialog`), `src-tauri/tauri.conf.json` (the single `main` window), `src-tauri/capabilities/default.json` (`core:default` only), `src/App.tsx`.
  - `src-tauri/src/library/availability.rs`, `library/roots.rs` (`list_roots*`, `add_root*`, `relocate_root*`), `commands/roots.rs`, `commands/scan.rs`, `jobs/mod.rs` (`JobManager`, `start_scan_impl`), `scanner/walk.rs` (the always-skipped names and the `.photoslibrary` rule), `database/mod.rs` (`app_settings` access).
  - `src/components/LibraryFolders.tsx`, `src/lib/scanText.ts`, `src/lib/tauri/{index,schemas,errorCodes}.ts`, `scripts/verify.mjs`.

## Decisions already made (follow them; if one conflicts with reality, stop and report)

### Theme C: the start-up window (STATUS risk 55) and one small script fix. Do this first.
**(a)** `tauri.conf.json`: the `main` window gets `"visible": false`. In `.setup()`, after the state is managed and the catalogue is ready, the window is shown (and focused). The failure paths are unchanged: a start-up error shows the native dialog and exits, and no window has ever been visible. If the `main` window cannot be found, that is a start-up error with a fixed code and the dialog, never a panic. Put the "show the window" step in one small function so a reader can see it.
**(b)** `src/App.tsx`: before `getAppInfo` has answered, show a neutral "Starting…" state (it may already; read it). Never show the raw "state not managed" text.
**(c)** `scripts/verify.mjs`: remove the Node `DEP0190` deprecation warning (a `shell` option together with an args array) without changing what `pnpm verify` runs or how it fails. Show the warning gone in the pasted output of `pnpm verify`.
**(d) Be honest about testing.** `tauri::test` needs a Cargo feature; do not add one. This theme has **no automated gate**; its gate is the owner's manual check (below) and your reading of the code. Say so in the report. If the page never loads while the window is hidden on one OS in the owner's check, that is a stop condition.

### Theme B: ExifTool call hardening (STATUS risk 16), adapter only
The adapter is not used by the scanner and stays unused (Phase 3). Two defects are already visible in the code: stderr is piped but read only after stdout ends and only on failure, so a child that writes a lot to stderr blocks on a full pipe until the timeout (reported as `Timeout`, the wrong cause); and `child.wait()` after stdout ended is outside the timeout.
**(e)** One pure function builds the argument vector (`exiftool_args`) and one test pins it: it contains `-m` (ignore minor errors, spec §3.3), keeps `-j -n -charset utf8 -charset filename=utf8 -@ -`, and has no shell and no path in the arguments (the path goes over stdin, as today). `-api WindowsLongPath=1`: keep it only on Windows if you can show (a test or a run on the Mac) that it changes nothing elsewhere; otherwise keep it as is and say why.
**(f)** Caps and waiting: stdout cap stays (512 KiB); stderr is **drained concurrently** with stdout into a capped buffer (at most 64 KiB kept, the rest discarded, never unbounded and never able to block the child); the wait for exit is inside the timeout; one overall deadline covers spawn to exit.
**(g)** Errors: every adapter error maps to a fixed variant. **No variant may carry stderr text or a path into anything that can reach IPC** (D28); where stderr text is kept for logs, it stays in the adapter and is not part of `Display` of the public error. Non-UTF-8 output is an error, not a panic or a lossy success.
**(h)** Concurrency: a process-wide limit of 2 simultaneous ExifTool processes (the laptop has 8 GB); a waiter is cancel-safe (dropping the future releases nothing it does not hold). Decide the mechanism yourself (a `tokio::sync::Semaphore` is expected; no new crate).
**(i)** The Windows kill-on-timeout test (risk 16): a Windows variant of `test_child_killed_after_timeout`. The process must be proven gone through the OS, not through your own bookkeeping. You may use a `cfg(test)` hook that exposes the spawned child's id, and the real staged `exiftool.exe` (CI has it; the test may use a very short timeout), or a fake executable that does not go through `cmd.exe` (a `.cmd` file leaves a grandchild after the kill; if you choose a fake, say why it is a single process). To check that the process is gone you may run `tasklist` from the test; **do not add `unsafe`, a crate, or a `windows-sys` feature** (a `Cargo.toml` change is a stop condition). If that cannot be done within these limits, stop and report; do not weaken the test to a timeout-only check.
**(j)** Tests (names say what they prove): `exiftool_args_are_pinned`, `stderr_flood_does_not_block_or_masquerade_as_timeout` (a fake executable that writes far more than the pipe buffer to stderr and then JSON to stdout: success), `wait_after_stdout_close_is_inside_the_timeout` (a fake that closes stdout and keeps running), `error_display_carries_no_stderr_or_path`, `concurrency_limit_is_two` (a fake that records overlap through a lock file; assert at most 2 overlapped and that a third waits), the Windows kill test (i) and the existing Unix kill test kept. Fakes must be real child processes (a script on Unix; for Windows say what you did); no test waits on a guessed time (lesson 13): wait for the process, a flag or a poll loop with a deadline.

### Theme A: watcher hints and the reconciliation scan (D35). New crate `notify`.
**(k) Hints, not truth.** A watcher event never changes a catalogue row. It only sets an in-memory "changes detected" mark for a root. Only a scan changes rows, with every 1D, 1E and 1F safeguard unchanged (nothing is missing unless the walk completed; the volume guard; one scan per root).
**(l) The crate.** Add exactly one crate: `notify` (latest stable at the time; **verify the version and its feature flags from crates.io or its documentation, do not use memory** and write the source in the report; default features off if that is possible; the recommended watcher per OS; recursive). `cargo deny` must pass (licence and advisories); an advisory you cannot fix with `cargo update -p` is a stop condition. Keep the backend behind a small trait (`WatchBackend`) so the rest is tested with a fake backend that you drive by hand.
**(m) One watcher per Active root.** Started after the catalogue is ready, after `add_root`, after `relocate_root`, and when a root goes Offline→Active; stopped on `remove_root`, when the root goes Offline, and when it is Paused. A watcher that fails to start, or reports an error, never fails `add_root` and never ends the app: the root shows `watching: off` (or `degraded`) with a fixed reason text, and a watcher error counts as a change (mark it). Never watch a root while it is not available (1F's check).
**(n) The debouncer is a pure state machine** that takes `Instant` values as arguments (no timers or clocks inside): a burst of events becomes one "changes detected" after a quiet period of 2 s, and never later than 30 s after the first event of a continuous stream. Any event kind (create, modify, remove, rename, other, overflow, error) is a change; events for always-skipped names and `.photoslibrary` content are dropped by the same rule function the scanner uses (reuse it; do not copy it). The callback given to `notify` only folds the event into the state (an atomic flag or a capacity-1 channel): **a flood of events must not grow memory** (test it with a million synthetic events through the folding function).
**(o) What "changes detected" does.** (1) `RootSummary` gains `changesDetected: boolean` and `watching: "on" | "off" | "degraded"`, in memory only, cleared when a scan of that root starts; the card shows "Changes detected since the last scan" and the **Scan** button stands out. (2) An app setting `auto_scan` (stored in `app_settings`, **default off**; no migration; two commands `get_auto_scan` and `set_auto_scan(enabled)`, with Zod and fixed error texts) lets the scheduler start the scan itself. The UI has one checkbox for it in the Library Folders card.
**(p) The scheduler is a pure decision function plus a thin runner.** The function takes the full state (roots with status, `changesDetected`, running jobs, last scan end times, the setting, now) and returns what to start. Rules: only when `auto_scan` is on; only an `Active` root; never while a job runs for that root; at least 60 s after that root's last scan ended; **at most one automatic scan at a time in the whole app** (the others wait, oldest change first); and a root whose last finished scan ended more than 24 h ago is due even with no event (the reconciliation scan: events can be lost). An automatic scan goes through the same `start_scan` path as the button (same availability check, same guard, same job row; it records `"trigger": "auto"` in `jobs.payload_json`, not in a new column) and the result line says it started automatically. The runner is a 1 s ticker calling the function; it is the only part that touches the clock.
**(q) The UI learns of changes** either from a Tauri event listened to in `src/lib/tauri/` (only if `core:default` already allows it; **a change to `capabilities/` is a stop condition**) or from a 5 s refresh of the list while the card is visible. Choose, say which and why, and show that the window being hidden or the app in the background does not make it run away.
**(r) Honest limits** go into `docs/ipc.md` and the report: the watcher is best effort (network shares, iCloud-managed folders, event buffer overflow on Windows, event coalescing on macOS); a manual Scan always works; the 24 h reconciliation exists for lost events; the walk stage on a dead share is still not cancellable (risk 54).
**(s)** Tests (names say what they prove): `debouncer_folds_a_burst_into_one_mark_after_the_quiet_period`, `debouncer_max_wait_forces_a_mark_during_a_continuous_stream`, `event_flood_is_folded_into_one_flag`, `ignored_names_do_not_mark_changes`, `watcher_event_writes_no_catalogue_rows` (snapshot every column of `files`, `folders`, `roots`, `jobs` before and after a mark), `scheduler_does_nothing_when_auto_scan_is_off`, `scheduler_skips_busy_offline_and_paused_roots`, `scheduler_respects_the_60s_gap`, `scheduler_starts_one_scan_at_a_time_oldest_change_first`, `stale_root_is_due_after_24h`, `registry_stops_the_watcher_when_the_root_goes_offline_and_starts_it_when_active` (fake backend), `watcher_start_failure_shows_off_and_does_not_fail_add_root`, `changes_detected_is_cleared_when_a_scan_starts`, `auto_scan_setting_roundtrip_and_default_off`, the wire-format tests for the new fields and commands, `ALL_CODES` and `errorCodes.ts` drift tests extended if you add codes. **One** real-filesystem smoke test per OS (create a file in a temp root; poll with a deadline of 30 s for the mark); it must pass 30 of 30 runs on the Mac and on the laptop before you report (paste the loop output); if it is not reliable on a CI runner, mark it `#[ignore]` with the reason and an owner-run command, and say so in section 7.

### Mutation demonstrations (each: the mutation named, the failing output pasted, the revert shown)
- **M1 (C):** none. Say so (decision d).
- **M2 (B):** remove `-m` from the argument vector → `exiftool_args_are_pinned` fails.
- **M3 (B):** replace the concurrent stderr drain by the old read-after-stdout → `stderr_flood_does_not_block_or_masquerade_as_timeout` fails (on Windows too if you can show it from CI; the Mac is enough).
- **M4 (B):** remove the `kill()` calls and `kill_on_drop` → the kill test fails on the Mac; the Windows kill test fails in CI (a throwaway commit on `task/1G` that you revert is allowed; paste the run line and the failing assertion).
- **M5 (A):** make the scheduler ignore the `auto_scan` setting → `scheduler_does_nothing_when_auto_scan_is_off` fails.
- **M6 (A):** make the watcher callback call a row-writing function (for example the status function) → `watcher_event_writes_no_catalogue_rows` fails.
- **M7 (A):** remove the 60 s gap → `scheduler_respects_the_60s_gap` fails.

## Goal
Start the app: the window appears only when the catalogue is ready (and never behind an error dialog). Change a photo folder from Finder or Explorer: within seconds the card says "Changes detected" and Scan stands out; with the auto-scan box ticked the app scans by itself, one root at a time, without ever marking a photo missing unless the walk was complete. The ExifTool adapter cannot hang on stderr, cannot leave a process behind on either OS, and runs at most two at once.

## Out of scope (do not do)
- Using ExifTool or its output anywhere in the scan, the database or the UI (Phase 3); thumbnails, the grid, the viewer (Phase 2); a timer-driven availability check; a watcher for offline roots; marking anything `missing` from an event; changes to the walk (risk 54, walk stage).
- A migration, any new crate besides `notify`, a new npm package, a frontend test runner, `unsafe`, a `windows-sys` or other Cargo feature change; changes to `capabilities/`, `deny.toml`, `ci.yml`, `AGENTS.md`, the spec, `docs/STATUS.md`, `docs/NEXT_STEPS.md`.
- Network share support beyond today.

## Allowed paths
`src-tauri/src/watcher/` (new), `src-tauri/src/metadata/`, `src-tauri/src/library/`, `src-tauri/src/commands/`, `src-tauri/src/jobs/`, `src-tauri/src/scanner/` (only to export the skip-rule function), `src-tauri/src/database/` (settings and queries only), `src-tauri/src/domain/`, `src-tauri/src/lib.rs`, `src-tauri/Cargo.toml` and `Cargo.lock` (**only** the `notify` line and its lock entries), `src-tauri/tauri.conf.json` (**only** the window's `visible`), `src/App.tsx`, `src/components/LibraryFolders.tsx`, `src/lib/`, `scripts/verify.mjs`, `docs/ipc.md`, `reports/`.

## What you may decide alone
Names, internal structure, file layout, the semaphore mechanism, how fakes are built, the shape of the state machine, event versus polling (q) with your reason. If a decision above is unclear, **stop and ask** (the owner relays); do not guess. Do not stop for a name or a layout.

## Size
About 10,000 changed lines excluding lockfiles and the report is the limit. Write in Part A your estimate per theme. At about 8,000 lines, stop at the next green theme commit if the remaining theme would pass the limit, and report what is left.

## Acceptance criteria
- [ ] Report section 1 written first (Part A), including the size estimate per theme and the order C, B, A.
- [ ] Three theme commits, each with `pnpm verify` green; each pushed; the `gh run list` line for each is in the report.
- [ ] `git diff --stat main..HEAD` shows only allowed paths; no migration; `Cargo.toml` changed only by the `notify` line; `tauri.conf.json` changed only by `visible`; no change to `capabilities/`, `deny.toml`, `ci.yml`, `package.json`, `pnpm-lock.yaml`.
- [ ] Every test above exists and passes on both OSes in CI; M2 to M7 pasted; the real-filesystem smoke test passes 30 of 30 on the Mac (the laptop run is the owner's).
- [ ] No `unwrap`/`expect`/`panic!` in production code of new or changed files; no `unsafe` added; test hooks only under `#[cfg(test)]` (greps below).
- [ ] No `files`/`folders` write in `src-tauri/src/watcher/` (grep below).
- [ ] `pnpm verify` green locally with the `DEP0190` warning gone; `git status --short` empty afterwards; `gh run list --branch task/1G --limit 6` pasted, both OSes and cargo deny green (a red run is explained from its failed log, which the owner supplies; STATUS risk 59).
- [ ] Claims table complete; section 7 (deviations) lists every deviation, every changed existing test and every extra file touched; section 8 lists, per theme, its size in changed lines and what was hard.

## Verification commands
```
pnpm verify
cargo test --manifest-path src-tauri/Cargo.toml
git status --short
gh run list --branch task/1G --limit 6
rg -n 'unwrap\(\)|expect\(' src-tauri/src/watcher src-tauri/src/metadata src-tauri/src/library src-tauri/src/jobs src-tauri/src/commands/scan.rs
rg -n "panic!" src-tauri/src/watcher src-tauri/src/metadata src-tauri/src/library src-tauri/src/jobs src-tauri/src/commands
rg -n "unsafe" src-tauri/src
rg -n "failure_hook|failure_in|panic_in|panic_after|fail_final|test_deps" src-tauri/src
rg -n "UPDATE files|UPDATE folders|INSERT INTO files|INSERT INTO folders|DELETE FROM" src-tauri/src/watcher
rg -n "@tauri-apps/api" src
git diff --stat main..HEAD
cargo deny --manifest-path src-tauri/Cargo.toml check
```
(Hits inside `#[cfg(test)]` are fine; label each hit test or production. Every hook hit must be inside `#[cfg(test)]`. `@tauri-apps/api` only under `src/lib/tauri/`. If `cargo deny` is not installed locally, say so; CI runs it.)

## Report rules (STATUS risk 26)
As in TASK-1F, in full: anything quoted as output is raw output of a command you ran in this task, anything else is labelled "from source, not run"; every test count comes from a pasted log line, otherwise "inferred, from X"; header ids (final code commit, the real report commit, last CI run) come from `git log` and `gh run list` you ran; paste `git diff --stat` verbatim including the report, without `...`; paste every grep raw (an empty result is the command and nothing); name the mutation in every failure demo and show the revert and **do not shorten failing output without saying so**; "Concepts used": at most 5, each with a `file:line` checked in the final tree; do **not** propose the next task; "all criteria met" only next to a green latest run **and** only if the manual checks are marked as the owner's; "tested manually" needs the output. Self-audit: the **Claims table** (`claim | source`) covers every result sentence in sections 3 to 8. Add to the header the line count of each theme commit (from `git show --stat`).

## Stop conditions
Stop and report (do not guess, do not work around) if: any crate besides `notify` seems needed; a migration seems needed; a change to `capabilities/`, `ci.yml`, `deny.toml` or `Cargo.toml` beyond the `notify` line (including a feature flag) seems needed; `notify` brings an advisory you cannot fix with `cargo update -p`; the Windows kill test (i) cannot be written within the limits; the page does not load while the window is hidden on one OS; a watcher event path needs a row write to work; you find a conflict between this file, the spec and AGENTS.md; an acceptance criterion cannot be met on one OS; or the work passes the size limit.

## Manual check (the owner, after the agent; the orchestrator will give the exact commands for each OS)
With `pnpm tauri dev`: (1) the window appears once and shows "Starting…" at most for a moment, never raw error text; (2) with the catalogue's platform stamp deliberately wrong, only the native error dialog shows and no window is ever visible behind it (the orchestrator gives the stamp commands, verified from the code, when the agent is done); (3) add a small folder, add a photo to it from Finder or Explorer: "Changes detected" appears within about 5 s; press Scan: the mark clears; (4) tick **Automatically scan changed folders**, copy another photo into the folder: a scan starts by itself after the 60 s gap and the result says it started automatically; untick it and repeat: nothing starts; (5) rename the folder: it goes "Not available" (the 1F behaviour) and no scan starts; Find folder… attaches it again. Screenshots of (1), (3), (4), (5) on each OS.

## Deliverable
`reports/REPORT-1G.md` (template, raw outputs, a run log from `gh run list`, the Claims table, "Concepts used" at most 5). Then stop. Do not start the next task.

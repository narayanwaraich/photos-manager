# TASK-1F: Offline roots and honest jobs: a drive that goes away is "not available", never "all missing"; a failed or unsaved scan says so in plain words

**Phase:** 1 · **Branch:** `task/1F` (from `main`, after the docs sync-26 is committed) · **Depends on:** TASK-1E accepted and merged (tag `accepted/1E`, squash `c61dad7`)
**Effort (set by the owner in the picker): `high`.** You cannot see it; write "Effort (owner-reported): high" in the report header.
**Orchestrator note:** this is the first task for Gemini 4 Hydrogen (D31, `docs/AGENT_PROFILE.md`), so it is somewhat larger than earlier tasks and its task file is shorter. It has **two themes**, each with its own gate tests and mutation demos:
- **Theme A, availability.** The app can scan, but it cannot yet tell "the drive is unplugged" from "the photos are gone". One mistake here makes the catalogue say that a whole library is missing. Everything in A is about never writing that lie.
- **Theme B, honest jobs.** Two places where a job tells the user less than the truth: a failed scan prints a raw code (finding N1 of the 1E review), and the final write of a job's row can fail without anyone knowing (STATUS risk 63).

Nothing in this task touches a photo on disk. You do not touch secrets, tokens, repositories, releases or settings (AGENTS.md, D17). This task needs none.

## Read first (only these; do not load the whole spec)
- `AGENTS.md`.
- `IMPLEMENTATION_SPEC.md` (v2.1.7 after sync-26): **§6.1** (offline roots), §6.2 steps 6 and 7, §11 ("Phase 1 as built"), §12.1, **D29, D30, D32, D33**. Hydrogen's long context is not a reason to read more.
- `reports/REVIEW-1E-round2.md` finding N1, STATUS risks 53, 54, 63 (the orchestrator will paste nothing else; read them in `docs/STATUS.md` in the repo).
- Code, read before changing it (line numbers are from the orchestrator's read of `main` at `c61dad7`; verify them, they are facts to check, not to trust):
  - `src-tauri/src/library/roots.rs`: `add_root_with_deps` (volume find-or-insert around lines 118 to 160), `list_roots_with_checker_and_timeout` (about 245 to 325: `reachable` from `is_dir()` on a blocking thread, a 2 s timeout, an in-flight set; a root whose check is still running reports `false`).
  - `src-tauri/src/database/roots.rs`: `set_root_status` (line 78) already exists; `database/volumes.rs`: `find_volume_by_uuid`, `update_volume_label_and_seen`.
  - `src-tauri/src/scanner/mod.rs` about line 322: `if !root_path.is_dir()` runs **directly on the async task** (STATUS risk 54).
  - `src-tauri/src/platform/windows.rs` lines 98 to 118: the volume GUID, else a bare 8-hex volume serial as `fs_uuid`.
  - `src-tauri/src/jobs/mod.rs` lines 420 to 600: four finish paths, each with `let _ = database::finalize_job(...)`, `prune_finished_jobs`, `tx.commit()`, and `manager.remove_active_job(&job_id)` **before** the final write.
  - `src-tauri/src/commands/roots.rs`: `add_root_with_picker`, `DialogGuard`, `ActiveReachableChecks`, `list_roots_with_active_jobs`; `commands/error.rs`: `ALL_CODES`, `fixed_message_for_code`, the drift tests.
  - `src/components/LibraryFolders.tsx` (line 259: `` `Scan failed (${validEvent.code}).` ``; lines 434 to 440: when Scan is disabled; line 666: the `interrupted` notice), `src/lib/scanText.ts`, `src/lib/tauri/{index,schemas,errorCodes}.ts`.

## Decisions already made (follow them; if one conflicts with reality, stop and report)

### Theme A: availability

**(a) Stored status.** A root's stored `status` is changed by **one** function (`database::roots::set_root_status` may stay the low-level write; add one library function that applies a live check result): `Active` with a definite "not available" result becomes `Offline`; `Offline` with a definite "available and same volume" result becomes `Active`. `Paused` and `Removed` are **never** changed by this function. A check returns three outcomes, not two: `Available`, `NotAvailable`, `Unknown` (timeout, a check still in flight, a panicked check). **Only a definite outcome changes the stored status**; `Unknown` changes nothing. The write is skipped when the status would not change.

**(b) No file or folder row is written when a root goes offline or returns** (D33, spec §6.1). Readers treat every row of an offline root as unavailable. This is the core safety rule of the task: after any sequence of offline and online transitions the `files` and `folders` tables are byte-for-byte what they were.

**(c) Same-volume guard before every scan, and in every availability check.** The availability check is: (1) `is_dir()` on the root path; (2) `describe_volume(path)` and compare its `fs_uuid` with the stored volume's `fs_uuid`. If both are non-NULL and differ, the result is `NotAvailable` with reason "volume changed" (the path exists but it is another drive: an empty mount point left behind, a new drive letter). If either is NULL the comparison is skipped (state this limit in the report). Both calls run on a blocking thread with a timeout, never on the async executor; one in-flight check per root (reuse `ActiveReachableChecks`). **Why:** a complete walk of an empty folder marks every photo `missing` (lesson 14); the 1D rule "nothing is missing unless the walk completed" does not cover a walk of the wrong place.

**(d) Where the check runs.** (1) `list_roots` (the command layer, so `library::roots::list_roots*` stay read-only and their tests stay valid): after computing the live results it applies them with (a), best effort (an error is logged, never fails the list); `RootSummary.status` shows the stored status after the write and `reachable` stays the live answer. (2) `start_scan`: before the job row exists. `NotAvailable` or `Unknown` refuses the start: `root_offline` (reason "unreachable") or `root_volume_changed` (reason "volume changed"); **no job row is created**; `NotAvailable` also applies (a). Timeout for the scan-start check: 5 s (a sleeping external disk may need it); for the list: the existing 2 s. (3) At the end of a scan that failed with `scan_root_unreachable` (the folder vanished after the pre-flight): apply `NotAvailable` to the root. (4) Not at start-up and not on a timer: the 2 s polling exists only while a scan runs.

**(e) The scanner's own `is_dir()` (risk 54, scan start).** Replace the direct `root_path.is_dir()` in `scan_root` by the same check helper from (c) (blocking thread, timeout), so a dead network share cannot stall an executor thread; `Unknown` maps to `ScanError::RootUnreachable`. **The walk itself is not changed:** a single filesystem call that blocks on a dead share still cannot be cancelled once the walk has started. Say so in the report and in `docs/ipc.md`; risk 54 stays open for that part.

**(f) `relocate_root(root_id)`: attach a returned root that is at a new path.** A Tauri command, dialog run in Rust exactly as `add_root` does it (same `DialogGuard`, no path from the WebView, same cancel behaviour). Rules, in this order, each with its fixed code:
1. the root exists, else `not_found`; its stored status is `Offline`, else `root_not_offline`;
2. no scan holds the root's `ScanGuard` (take it and hold it until the end), else `root_busy`;
3. the chosen path is canonicalised and passes the same checks as `add_root` (absolute, directory, valid Unicode, protected folders, overlap with **every other** root; the root itself is excluded from the overlap check);
4. the volume under the new path has an `fs_uuid`, the stored volume has one, and they are equal, else `relocate_volume_mismatch` (a root on a volume with no `fs_uuid`, for example some network shares, cannot be relocated in this version: say so);
5. **content spot check:** if the root has any file rows, take up to 30 `files` rows with status `present` or `inaccessible`, ordered by `id`, join each `rel_path_native` to the new path, and test existence with a metadata call on a blocking thread (no file is read, no content hashed); at least half must exist, else `relocate_content_mismatch`. If the root has no file rows the check is skipped;
6. one `BEGIN IMMEDIATE` transaction: update the root's `path_native`, `path_display`, `updated_at_ms` and `status = 'active'`, and the volume's `last_seen_at_ms`; commit. **Nothing else is written** (b).
It returns the refreshed `RootSummary`. `remove_root` of an offline root keeps working and must not touch the disk.

**(g) Volume reuse refreshes case sensitivity, safely.** In `add_root_with_deps` the probe already runs before the volume lookup. When a stored volume is reused: if the probe result is **definite** (not `Unknown`) and **differs** from the stored `case_sensitive`, and any root of that volume has any `files` row, refuse with `volume_case_changed` (changing it would make the stored `path_key`s wrong, D26); if no file row exists on that volume, update the stored value in the same transaction; an `Unknown` probe or an equal value changes nothing.

**(h) The Windows `fs_uuid` fallback.** When the volume GUID call fails, the fallback is stored as `serial:` plus the 8-hex serial (today it is the bare serial). Put the identity logic in one **platform-neutral** function so both OSes test it: `format_serial_identity`, and `same_volume_identity(stored, live)`, which treats a stored bare 8-hex value (legacy) as equal to `serial:` plus that value. Reuse of a volume row found by a `serial:` identity additionally requires equal `fs_type` and equal `label`, else a new volume row (serials can repeat across devices). The guard in (c) compares the identity only. Known limit to state: two devices with the same serial cannot be told apart.

**(i) New codes, fixed texts (D28: no path, no OS text).** Add each to `ALL_CODES`, `fixed_message_for_code`, `errorCodes.ts` and the drift tests, with exactly these texts:
- `root_offline`: "This folder is not available. The drive may be unplugged, or the folder may have been moved or renamed."
- `root_volume_changed`: "This path is now on a different drive than the one this folder was added from. Nothing was changed."
- `root_not_offline`: "Only a folder that is not available can be looked for again."
- `relocate_volume_mismatch`: "The chosen folder is on a different drive than the one this folder was added from."
- `relocate_content_mismatch`: "The chosen folder does not contain the photos this folder had. Nothing was changed."
- `volume_case_changed`: "This drive now treats upper and lower case in names differently than when it was first added. Remove its folders from the app first, then add them again."

### Theme B: honest jobs

**(j) N1: a failed scan never shows a raw code.** `scanText.ts` gets `SCAN_FAILURE_MESSAGES` (code to sentence) and `formatScanFailure(code)`; the unknown-code fallback is "The scan could not be completed." and **does not print the code**. Cover every code a scan job or its start can end with: `scan_root_unreachable`, `scan_walk_failed`, `scan_root_option_unsupported`, `scan_already_running`, `root_offline`, `root_volume_changed`, `root_busy`, `interrupted`, `internal`, `database`, plus any other the `jobs`/`scanner` code can emit (list them in report section 1 from the source). **The text must equal `fixed_message_for_code(code)` in Rust.** Add a drift test in the style of the warning-table test (a Rust test that reads `src/lib/scanText.ts`; a simple regex parse is acceptable, say so) asserting the key set and each text. `LibraryFolders.tsx` uses `formatScanFailure` at line 259 and also shows the text of a `failed` `lastJob` (not only `interrupted`) when no outcome exists from this session.

**(k) Risk 63: the final job write.** Replace the four copies of the finish sequence in `jobs/mod.rs` by **one** function that does `BEGIN IMMEDIATE`, `finalize_job`, `prune_finished_jobs`, `commit`, retries the whole sequence **once** after 200 ms on any error, and returns whether the row was recorded. When it was not recorded: log one `tracing::error!` with the job id and the code (no path), and still send the terminal event. The terminal events `finished` and `failed` gain `recorded: boolean`; Rust, Zod and the wire-format tests change together. The UI, when `recorded` is `false`, adds one line to the result: "The result of this scan could not be saved to the history. The catalogue itself is fine." (the next start will show the old row as "did not finish": say so in `docs/ipc.md`). **Order:** the manager's active-job entry is removed **after** the final write attempt and **before** the terminal event, so `activeJobId` and the row never disagree (today the entry goes first). Every other `let _ =` in `jobs/` and `commands/scan.rs` gets either real handling or a one-line comment saying why ignoring is right (the progress write and the failed channel send are the two valid cases).

## Goal
Unplug a drive, press Refresh: the folder says "Not available", Scan says why, nothing in the catalogue changed. Plug it back: the folder is Active again and a scan finds nothing missing. Rename the folder instead: "Not available", then **Find folder…** attaches it again with every photo still in the catalogue. A scan that fails says what happened in a sentence, and a scan whose result could not be saved says that too.

## In scope

### Part A: report section 1 (before any code)
What you found (keep, change, add and why); the list of failure codes from (j) taken from the source; the check helper's signature and who calls it; the availability state table (status, check outcome, new status, rows written: always none); where the new commands are registered; what you will do about the existing 1E test that asserts a scan of a deleted root path ends as `failed(scan_root_unreachable)` (it changes: the start is now refused with `root_offline`, and a second test keeps the in-job path alive by making the folder vanish after the pre-flight).

### Part B: Rust
1. The check helper and the status function (a), (c), (e); `list_roots` and `start_scan` wiring (d); the scanner pre-flight (e).
2. `relocate_root` and its `*_impl` / picker-injected variant so tests need no window (f).
3. Volume reuse (g); the serial identity (h) including `platform/windows.rs`.
4. The finish function and `recorded` (k); codes (i).
5. No migration. No new crate. No `unsafe`. No `unwrap`/`expect` in production code.

### Part C: tests (names that say what they prove; temp directories; both OSes in CI; inject describers and checkers, no real drive needed)
**Theme A gates (each gate shown failing without its protection, see the demonstrations):**
- `volume_guard_refuses_empty_mount_point_and_marks_nothing_missing`: scan a real tree so rows exist; empty the folder (it still exists) and make the injected volume describer report another `fs_uuid`; `start_scan` returns `root_volume_changed`, **no job row exists**, the root is `Offline`, and every `files` row still has its old status.
- `offline_round_trip_writes_no_file_or_folder_rows`: snapshot the `files` and `folders` tables (every column), go offline, come back, go offline again; the snapshots are equal each time (compare whole rows, as the 1D snapshot test does).
- `status_changes_only_on_definite_answers`: `Unknown` (timeout, in-flight, panicking checker) leaves `Active` and `Offline` unchanged; `Paused` and `Removed` never change under any outcome.
- Relocate: success changes only the root's path columns, status and the volume's `last_seen_at_ms` (snapshot of everything else equal); refused for each of `root_not_offline`, `root_busy`, `relocate_volume_mismatch` (also when either identity is NULL), `relocate_content_mismatch` (below half; and exactly half passes), overlap with another root, a protected folder; a cancelled dialog changes nothing.
- `remove_root_works_when_offline`: the counts come from the catalogue and the removal succeeds with the path gone.
- Case refresh: a definite change on a volume with files is refused with `volume_case_changed` and nothing is written; `Unknown` changes nothing; a volume with no file rows updates the stored value.
- Serial identity (pure functions, run on both OSes): prefix, legacy compatibility in `same_volume_identity`, reuse needs equal `fs_type` and `label`.
- Dead check at scan start: a checker that never returns does not block: `start_scan` returns `root_offline` within the test's deadline (use an injected short timeout) and a second `start_scan` is not stuck behind the first.
- `scan_root_unreachable` inside the job (the folder vanishes after the pre-flight): the job row is `failed/scan_root_unreachable` and the root becomes `Offline`.

**Theme B gates:**
- Final-write failure on **each of the four finish paths** (completed, cancelled, failed, panic): inject a failure that persists through the retry; exactly one terminal event, last, with `recorded: false`; no panic; a new scan of the root starts afterwards. One more test where the first attempt fails and the retry succeeds: `recorded: true`. Inject the failure however you like, but test hooks exist only under `#[cfg(test)]` (STATUS risk 60).
- `active_job_id_and_row_agree_until_the_final_write_ends`: while the final write is held back (a test-only gate), the root's `activeJobId` is still set and the row still says `running`; when the terminal event arrives the entry is gone and the row is final.
- Failure-text drift (j), `recorded` and the new error codes in the wire-format tests, `ALL_CODES` equals `errorCodes.ts` (existing test, extended).
- Existing tests that now change (the 1E deleted-root test, tests that assert `RootSummary` shapes or terminal events) are updated and listed in report section 7.

**Show these four failing and then passing, each with the mutation named, the failing output pasted, and the revert shown:**
1. **M1** remove the volume comparison from the check: `volume_guard_refuses_empty_mount_point_and_marks_nothing_missing` fails (without the guard the scan runs and marks the rows missing).
2. **M2** make the status function also run `UPDATE files SET status = 'inaccessible' WHERE root_id = ?`: `offline_round_trip_writes_no_file_or_folder_rows` fails.
3. **M3** move `remove_active_job` back before the final write: `active_job_id_and_row_agree_until_the_final_write_ends` fails.
4. **M4** change one letter of one text in `scanText.ts`: the failure-text drift test fails.

### Part D: UI (`src/lib/tauri/`, `scanText.ts`, `LibraryFolders.tsx`)
- Zod and the typed wrapper for `relocateRoot(rootId)` (the only place that calls `invoke`); the `recorded` field; the new codes in `errorCodes.ts`.
- Per root with status `Offline`, as designed states with fixed texts in `scanText.ts`: a badge "Not available"; one line "The drive may be unplugged, or the folder was moved or renamed."; **Check again** (the existing Refresh for that root's list); **Find folder…** (relocate; shows the fixed text of any refusal code); **Remove** still enabled. Scan is disabled with the visible reason "This folder is not available." (the existing disabled-reason line). A root that is `Active` but not `reachable` (an unknown check) keeps today's "not reachable" line.
- After a successful relocate the card shows the refreshed list; no scan is started automatically.
- Parse every response with Zod as before.

### Part E: self-audit, then the report
**Self-audit (new rule for this task, D31).** Before writing the report, go through the report's sections 3 to 8 and for **every sentence that states a result** (a count, an id, a line number, "passes", "fails", "matches") write down in a **Claims table** the command or log line it came from: `claim | source (command, file, or log line)`. A claim without a source is deleted or marked "inferred, from X". Header ids (commit, report commit, CI run) come only from `git log` and `gh run list` that you ran.

`reports/REPORT-1F.md` from `reports/_REPORT_TEMPLATE.md`, with the rules below. Also update `docs/ipc.md` (the new command, the `recorded` field, the codes, the limits of (e) and (k)); it is the one doc you may edit.

## Out of scope (do not do)
- Watchers, notify hints, a reconciliation scan, ExifTool call hardening (1G); the start-up window issue, risk 55, `tauri.conf.json` (1H).
- Marking files `inaccessible` or `missing` because a root is offline; automatic relocation without the owner choosing the folder; automatic scan on return; a timer or focus-driven availability check.
- A migration, a new crate or npm package, a frontend test runner, `unsafe`; changes to `capabilities/`, `tauri.conf.json`, `deny.toml`, `ci.yml`, `Cargo.toml`, `package.json`, `AGENTS.md`, the spec, `docs/STATUS.md`, `docs/NEXT_STEPS.md`.
- Network share support beyond today (a root with no `fs_uuid` is not relocatable and skips the guard; say so).

## Allowed paths
`src-tauri/src/library/`, `src-tauri/src/commands/`, `src-tauri/src/database/` (roots and volumes queries only), `src-tauri/src/jobs/`, `src-tauri/src/scanner/mod.rs` (the pre-flight only), `src-tauri/src/platform/windows.rs` and `platform/mod.rs` (the identity fallback), `src-tauri/src/domain/`, `src-tauri/src/lib.rs` (command registration), `src/lib/tauri/`, `src/lib/scanText.ts`, `src/components/LibraryFolders.tsx`, `src/App.tsx` (only if props change), `docs/ipc.md`, `reports/`.

## What you may decide alone
Names, internal structure, file layout, how the failure is injected in tests, how the check helper is shaped, whether the status function lives in `library/` or `database/`. If a decision above is unclear, **stop and ask** (the owner relays); do not guess. Do not stop for a name or a layout.

## Size
Aim for at most about 6,000 changed lines excluding lockfiles and the report (1E was about 4,100 including its report). If you see the work going past that, stop at a coherent point and report instead of continuing.

## Acceptance criteria
- [ ] Report section 1 written first.
- [ ] `git diff --stat main..HEAD` shows only allowed paths; **no migration file**; no change to `capabilities/`, `tauri.conf.json`, `deny.toml`, `ci.yml`, `Cargo.toml`, `package.json`, `pnpm-lock.yaml`.
- [ ] Every test of Part C exists and passes on both OSes in CI; M1 to M4 pasted with failing output and revert.
- [ ] No `unwrap`/`expect` and no `panic!` in production code of the new or changed files; no `unsafe` added; test hooks only under `#[cfg(test)]` (paste the greps below).
- [ ] No `files`/`folders` write in the status path: paste `rg -n "UPDATE files|UPDATE folders|INSERT INTO files|INSERT INTO folders" src-tauri/src/library src-tauri/src/commands` and label each hit (the scanner's and add-root's writes are not in these folders; an unexpected hit needs a sentence).
- [ ] `pnpm verify` green locally; `git status --short` empty afterwards; `gh run list --branch task/1F --limit 3` pasted, both OSes and cargo deny green (a red run is explained from its failed log, which the owner supplies; STATUS risk 59: an advisory can turn it red with no code change).
- [ ] Claims table present and complete; section 7 (deviations) lists every deviation, every changed existing test and every extra file touched.

## Verification commands
```
pnpm verify
cargo test --manifest-path src-tauri/Cargo.toml
git status --short
gh run list --branch task/1F --limit 3
rg -n 'unwrap\(\)|expect\(' src-tauri/src/library src-tauri/src/jobs src-tauri/src/commands/scan.rs
rg -n "panic!" src-tauri/src/library src-tauri/src/jobs src-tauri/src/commands
rg -n "failure_hook|failure_in|panic_in|panic_after|fail_final" src-tauri/src
rg -n "unsafe" src-tauri/src
rg -n "let _ =" src-tauri/src/jobs src-tauri/src/commands/scan.rs
rg -n "UPDATE files|UPDATE folders|INSERT INTO files|INSERT INTO folders" src-tauri/src/library src-tauri/src/commands
rg -n "@tauri-apps/api" src
git diff --stat main..HEAD
```
(`unwrap`/`expect`/`panic!` hits inside `#[cfg(test)]` are fine; label each hit test or production. Every hook hit must be inside `#[cfg(test)]`. `@tauri-apps/api` only under `src/lib/tauri/`.)

## Report rules (STATUS risk 26: the reports have contained invented text)
Anything quoted as output must be raw output of a command you ran in this task; anything else is labelled "from source, not run". **Every test count comes from a log line you pasted; a number you did not see is written "inferred" and says from what.** The header holds the final code commit, the real report commit (not `HEAD`) and the last CI run id, from `git log` and `gh run list`; every CI id cited anywhere comes from a `gh run list` you ran. Paste `git diff --stat` verbatim including the report itself, without `...`. No macOS or Windows version in the header unless it is pasted raw `sw_vers` or `ver` output. Paste every grep as the raw command and its raw output (an empty result is the command followed by nothing). Say what you mutated in every failure demonstration and show the revert. "Concepts used": at most 5, each with a `file:line` you checked in the final tree. Do **not** propose the next task. The header, the files-changed block and "all criteria met" must match the final branch, and "all criteria met" is only allowed next to a green latest run. A sentence that says something was "tested manually" needs the output; otherwise write "from source, not run".

## Stop conditions
Stop and report (do not guess, do not work around) if: a new crate or npm package seems needed; a migration seems needed; a change to `tauri.conf.json`, `capabilities/` or `ci.yml` seems needed; the volume identity cannot be read without new `unsafe`; the same-volume guard produces a false "volume changed" on a real volume on one OS (the owner's Mac or laptop output would show it); the root's `ScanGuard` cannot be taken for `relocate_root` without a deadlock; you find a conflict between this file, the spec and AGENTS.md; an acceptance criterion cannot be met on one OS; or the work is going past the size note.

## Manual check (the owner, after the agent, Mac and Windows laptop; no USB drive needed)
With `pnpm tauri dev`: add a small folder of photos (for example a copy in `~/Pictures/test-a`), press Scan and read the summary. (1) In Finder or Explorer rename the folder to `test-b`, press Refresh: the card says "Not available" and Scan says why. (2) Press **Find folder…**, choose `test-b`: the card is Active again with the same counts; Scan again: nothing new, nothing marked missing. (3) Press **Find folder…** on a second offline folder and choose a **wrong** folder: the card shows the "does not contain the photos" text and nothing changes. (4) Optional, with the USB drive: unplug it, Refresh ("Not available"), plug it in, Refresh (Active), Scan (0 missing). Screenshots of the card in the "Not available" and "Active again" states on each OS. The orchestrator will give the exact commands.

## Deliverable
`reports/REPORT-1F.md` (template, raw outputs, a run log from `gh run list`, the Claims table, "Concepts used" at most 5). Then stop. Do not start 1G.

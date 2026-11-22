# REVIEW-1C: Roots and volumes (backend)

Reviewer: orchestrator (Claude) · Date: 2026-11-22 · Branch `task/1C`, HEAD `54fb2a5` (report-only commit; the last code commit is `575fceb`) · Report: `reports/REPORT-1C.md` · CI run `44935233670` (green on both OSes, from the owner's `gh run list`)

## Verdict: **FIX** (small, one round; then ACCEPT if the five code fixes and the report corrections are in)

The design is sound and most of it is better than the task asked for: component-wise overlap with a real mutation proof, a read-only probe with an injected lookup, an exhaustive leak test for the new error codes, a migration test on a populated catalogue. Nothing here is dangerous. I still send it back because of one claim that is false in the code and repeated four times in the report, and because I found two rules in my own D29 that are wrong and are cheaper to fix now than after the commands and the UI sit on top of them.

| # | What | Why it blocks |
|---|---|---|
| C1 | `add_root` and `remove_root` use `pool.begin()`, which sends a plain `BEGIN` (deferred), not `BEGIN IMMEDIATE`. The doc comment, the report (§ on serialisation, "Concepts used" 1) and the acceptance table all say IMMEDIATE | A false safety claim is a FIX under checklist 4. The behaviour is safe but wrong in one case (below) |
| C2 | macOS `unsafe`: three sites have no SAFETY comment, and the name read trusts an offset from the kernel | AGENTS.md rule and report section 1 promised a SAFETY text for each |
| C3 | **My D29(c) is inconsistent:** a Windows drive root (`D:\`) is refused as a "filesystem root", while `/Volumes/My Passport` is allowed on macOS | Fix the rule now (spec v2.1.4), before the picker lets a user try it |
| C4 | The probe returns `Unknown` for every drive root (no cased path component) | Follows from C3: whole drives are the common Windows case |
| C5 | The root path is refused if an ancestor name has a backslash or a trailing colon, with the wrong error code | The rule protects paths that never cross OSes (D29(j), new) |

Everything else is a report correction (R1 to R7) or goes forward to 1C2 or later. Details and the exact fix prompt are below.

## Acceptance criteria against evidence

| Criterion (task file) | Result | What I saw |
|---|---|---|
| Report section 1 first (decisions, crates, `unsafe` count and SAFETY texts) | **Partly**: written first, but the count is wrong (R3) | 15 production `unsafe` sites in the new code, report says 7 and lists 6 |
| Migration 3 on a populated catalogue; index refuses a second parentless folder; `migrations_applied` is 3 | **Met** | `test_migration_one_root_folder_per_root_invariant` builds migrations 1 and 2 by hand, inserts a root and its folder, applies migration 3, then a second parentless folder fails; `app_info` test now asserts 3 |
| Overlap, protected, canonical rules tested; one mutation shown failing | **Met** | Report §5 mutates `check_overlap` to `starts_with` on strings and shows `/photos` vs `/photos2` failing; I read `overlap.rs` (component-wise, NFC then lower-case) and it is right |
| Probe wrote nothing (before/after) and decides both ways with the fake lookup | **Met in the test, thin in the report** (R5) | `test_case_probe_real_filesystem_read_only_and_correctness` compares the directory listing and the mtime of the directory before and after, and asserts `CaseInsensitive` on the runners; five fake-lookup tests cover fold, absent, different identity, no cased name, failing lookup |
| `describe_volume` works on the runner's OS; mapping tables tested | **Met** | `test_describe_volume_on_current_system` on both CI OSes; `parse_statfs_kind` and `parse_drive_type` tables |
| `add_root` atomicity (forced failure leaves zero rows); serialisation stated | **Atomicity met; serialisation stated wrongly** (C1) | A trigger forces the folder insert to fail and the test asserts zero roots and zero volumes afterwards (its comment also promises zero folders, but there is no assertion for folders; add one in C1's commit) |
| `remove_root`: right count deletes the tree and nothing else; stale count deletes nothing; bytes unchanged | **Met** | Tests in `roots.rs`; the bytes of a file on disk are compared before and after |
| `RootError` mapping: code, fixed text, leak test; `ALL_CODES` and `errorCodes.ts` agree | **Met** | Eight new codes; the leak test is exhaustive (no `_` arm); the drift test passes. The 1B nit ("build the samples without the template argument") is still open: `catalogue_error_sample(template)` and the `templates` array are unchanged; it stays a carried nit |
| `pnpm verify` green locally; status empty; `gh run list` pasted, both OSes green | **Met** | Mac: 94 tests (93 passed, 1 ignored `volume_report`), status clean. CI: three runs, the last two green; the first failed for a real reason (below) |
| Real-disk evidence from the owner (Mac internal and external, laptop) | **Mac met; laptop not yet** | See the next section. The laptop has not run `volume_report` |
| Stop; do not start 1C2 | **Met** | The agent stopped and wrote the next task into the report |

## The owner's evidence

- **Mac, `volume_report`** (owner's terminal, 2026-11-21): the internal data volume: `Internal`, label `Data`, `fs_uuid b9934167-…`, `apfs`, probe `CaseInsensitive`. The external drive `/Volumes/My Passport/Pictures`: `Removable`, label `My Passport`, `fs_uuid d8ccf56c-…`, `ntfs`, probe `CaseSensitive`. The first line proves `MNT_REMOVABLE` works on a real USB drive. The second is plausible but surprising: the macOS NTFS driver looks names up case-sensitively. It is harmless: overlap is lower-cased on every volume anyway, and a mis-labelled volume only changes how a future `path_key` is cased. To be sure, `ls "/Volumes/My Passport/pictures"` (lower-case `p`): "No such file or directory" means the probe is right.
- **CI**: `gh run list --branch task/1C`: `44933354630` red (docs commit; the code at that point used the unstable `MetadataExt::file_index` on Windows and left the macOS module ungated), `44934383719` green (`fbae3c1`: Windows identity through `windows-sys`, macOS module gated), `44935233670` green (the last commit). The report's run log shows only the first two (R2).
- **Laptop** (`verify-main3.txt`, 2026-11-21): this is `main` at `09f6573`, **not** the 1C code. `pnpm verify` is green end to end with the `PATH` fix: 47 tests, 5 node tests, `AGENTS.md` 7438. `status-after-verify3.txt` is empty. That closes risk 49 (no test deleted a tracked fixture). `pnpm tauri dev` printed `Running target\debug\photos-manager.exe` and then `[ELIFECYCLE] Command failed with exit code -1`, with no DLL error and no panic text. I cannot tell from that whether the window opened and was closed (closing the window often ends `tauri dev` with a non-zero code on Windows) or whether the program died. Risk 18 stays open until you tell me what you saw (NEXT_STEPS step 4).
- **Not seen**: any Windows run of the 1C code on the laptop (the volume report on `C:` and the USB drive, the start-up check with the new `ProtectedContext`). CI covers the unit tests only.

## Findings

### C1. `begin()` is a deferred transaction (code and report disagree)

`roots.rs:105-106` (`add_root`) and `roots.rs:261` (`remove_root`) call `pool.begin()`. In sqlx 0.8.6 `Pool::begin` issues `BEGIN` (I read `begin_ansi_transaction_sql` in `sqlx-core`); `Pool::begin_with("BEGIN IMMEDIATE")` is the call that takes the write lock first. The comment at `roots.rs:23` and `:105`, report §1 (point 1), §6 and "Concepts used" 1, and the "serialisation stated" row of the acceptance table all say IMMEDIATE.

What happens without it, which I reproduced with the SQLite library directly: connection A and B both run `BEGIN`, both read (no overlap yet), A inserts and commits, and B's first write fails at once with `database is locked` (the read snapshot is stale, so `busy_timeout` does not help in WAL mode). So the loser never commits a bad overlap (safe), but the user sees `database_error` instead of `root_overlaps`. With `BEGIN IMMEDIATE` the second caller waits (up to the 5 s `busy_timeout` in `database/mod.rs:84`), then reads the committed row and gets `root_overlaps`. A single-user app makes the race rare, which is why this is a correction, not an emergency. The same holds for `remove_root` (count, then delete).

### C2. macOS `unsafe`

`platform/macos.rs` has 7 `unsafe` sites and 4 SAFETY comments. Missing: `CStr::from_ptr(stat.f_fstypename…)` (line 55), `mem::zeroed()` for `attrlist` (60), and `slice::from_raw_parts(name_ptr, name_len)` (87). The last one is the one that matters: `name_ptr` and `name_len` come from `attr_dataoffset` and `attr_length` that the kernel wrote, and nothing checks that they stay inside the buffer. The kernel is trusted here, but a wrong offset would be a read past the buffer, so check `0 <= offset` and `offset + len <= size_of::<VolAttrBuf>()` (the offset is relative to the `attrreference_t` itself) before building the slice, and return no label otherwise. The struct layout (`name` before `uuid`, packed 4) matches the order the kernel writes volume attributes, and the real run on two disks confirms it; keep that as the first sentence of the SAFETY text. The Windows `platform/windows.rs` and `case_probe.rs` sites all have SAFETY comments and are correct (the handle is closed on every path; access 0 with `FILE_FLAG_BACKUP_SEMANTICS` is the right way to open a directory for identity only).

### C3. Whole-drive roots (my rule, now amended)

`protected.rs` refuses any root whose normalised path has one component on Windows (`D:\`) and none on Unix (`/`). D29(c) said "a filesystem root", and the agent implemented it as written. But macOS allows `/Volumes/My Passport` (a whole external drive), and a user's photos on Windows are very often a whole drive. The scan algorithm (spec §6.2 step 2) already always skips `System Volume Information` and `$RECYCLE.BIN`, which only makes sense if drive roots are allowed. The real protection is already there: `C:\` and `/` contain the app-data folder and the home folder, and the containment rules (`protected.rs:195-210`) refuse them. **Decision (amended D29(c), spec v2.1.4):** remove the separate filesystem-root test; the system volume's root stays refused by containment; any other drive root is allowed. You can veto this before the fix prompt is sent (NEXT_STEPS, decision 1).

### C4. The probe on a drive root

`probe_case_sensitivity` walks `path.ancestors()` and uses the first name with a cased letter. For `D:\` (and `\\?\D:\`) no ancestor has a name, so the result is `Unknown`, stored as case-insensitive with a warning. Once C3 allows drive roots that warning would appear on almost every Windows drive. Amended D29(d): when the path has no cased component, use the first entry directly inside the root with a cased character (a read-only `read_dir`, bounded to the first 64 entries); only if there is none, or the lookup fails, is it `Unknown`.

### C5. The root path and the "round-trip" name rule

`canonicalize_root` (lines 85-90) rejects a root if any component contains `\` or ends with `:`, with `RootError::PathNotUnicode` (so the user sees `root_not_unicode`, which is the wrong reason). The rule comes from D26, which protects names **under** a root so that a catalogue can move between OSes. A root's own path never moves between OSes (only root-relative paths are remapped), so the rule over-rejects: `/Users/a/Trip: 2024` is a legal folder on macOS and would be refused. Amended D29(j): the rule applies under a root, not to the root path. Remove it from `canonicalize_root`, from the `path-model.md` sentence ("On Unix, backslashes and trailing colons in components are rejected") and from its tests; also collapse the two identical `NotFound`/`_` arms at lines 71-72 into one.

### Smaller points, no action now

- **Scope:** three files outside the task's allowed list changed: `database/mod.rs` (the `now_ms` helper moved out of a function, and the migration test, which the task asked for), `commands/app_info.rs` (the `migrations_applied == 3` assertion the task asked for) and `lib.rs` (the `setup` closure was restructured: the "cannot resolve the app-data folder" case now shows the dialog and exits inside the `match` instead of falling through; same visible behaviour). All three are reasonable; the report should have listed them (R6).
- **Dependencies:** `libc 0.2` on Unix and `windows-sys 0.61.2` (features `Win32_Storage_FileSystem`, `Win32_Foundation`) on Windows. Both are real, widely used, and already in the lockfile through Tauri; the lockfile gains two lines. Fine.
- **Windows volume kind:** `GetDriveTypeW` reports many USB hard disks as `DRIVE_FIXED`, which maps to `Internal`. Not a bug in the mapping; the owner's laptop report for the USB drive will show what his drives report. D29(e) says kind is informational.
- **`fs_uuid` fallback to the volume serial** (Windows, when the volume GUID is unavailable): serials are not unique across devices, so a reused row could be wrong in rare cases. Acceptable now; revisit in 1F with the offline/remount logic.
- **`case_sensitive` is not refreshed** when an existing volume row is reused. Carry to 1F.
- **`preview_remove_root` runs without a transaction.** Fine, it is advisory; `remove_root` re-checks.

## Carried to 1C2 (add to its task file)

1. `list_roots` calls `native_path.is_dir()` for every root inside an `async fn`. On a dead network share that call can block for tens of seconds. In 1C2 the command runs it on a blocking thread, one task per root, with a short timeout (2 s) after which `reachable` is `false`.
2. The folder dialog runs in Rust (D29(i)); the command returns outcomes and warnings only; no path comes from the WebView.
3. The `RootWarning` texts (case-insensitive assumed, volume not identified) must be shown to the user, not dropped.

## Report corrections (report only, in the same commit series)

R1. **Serialisation claims** (§1 point 1, §6, "Concepts used" 1, the acceptance row): after C1 they become true; keep them, and add the evidence: the concurrent test passing, and the same test failing with `pool.begin()` (paste both runs, as for the overlap mutation).
R2. **Header and run log.** The header says commit `57cc0dc` and run `44935233670`; the last code commit was `575fceb` and the HEAD `54fb2a5`. The run log from `gh run list --json` must list all three runs with what each changed; the red run's cause (unstable `MetadataExt::file_index`, ungated macOS module) is in the report body but not in the log. After the fix round, the header names the new final commit and run.
R3. **`unsafe` section.** "7 (3 macOS, 4 Windows)" is wrong, and the pasted `rg` is from an earlier revision (it shows 6 macOS lines and omits `case_probe.rs`). The final count is 15 in the new code (7 macOS, 4 Windows `platform/windows.rs`, 4 Windows `case_probe.rs`), plus the 10 that existed before. Paste the final `rg -n unsafe src-tauri/src` and the count of `SAFETY` comments (every new site has one).
R4. **"macOS 15.3".** I cannot find it in any output you sent. Replace it with the `sw_vers` output, or "version not captured".
R5. **Probe evidence.** The read-only proof is the assertion inside `test_case_probe_real_filesystem_read_only_and_correctness`. Say so, quote its passing line from the test run, and drop any wording that suggests a pasted before/after listing exists if it does not.
R6. **Scope.** List `database/mod.rs`, `commands/app_info.rs` and `lib.rs` as deviations from the allowed list with the one-line reasons above. The file count (24 files, +3120) was taken before the report was added; paste the final `git diff --stat 6b2e67b..HEAD`.
R7. **"Concepts used" lines.** #1 cites `roots.rs:43` (that is not the transaction; it is at 105 to 106 now, after the fix at its new line); #2 cites `macos.rs:35` (the `MaybeUninit` is at 42). Re-read from the final files.

## What I did not check

I did not run the Rust tests or CI (my session cannot reach the Actions logs), did not run anything on Windows, and did not read all 704 lines of the `roots.rs` tests or the whole `protected.rs` test table (612 lines): I read the production code of every module, the migration test, the probe tests, the rollback and removal tests' assertions, and `error.rs`. The grep for `unwrap`/`expect` over production code in `library/`, `platform/`, `database/roots.rs`, `database/volumes.rs` and `domain/roots.rs` finds none outside `#[cfg(test)]`; no `set_var`, `Command::new` or file writes in `library/` or `platform/`.

## How I checked

Read-only clone of the public repo, `origin/task/1C`, HEAD `54fb2a5`. Read in full: `library/roots.rs` (production part), `canonical.rs`, `overlap.rs`, `protected.rs` (production part), `case_probe.rs` (code and tests), `volume.rs`, `platform/macos.rs`, `platform/windows.rs`, `domain/roots.rs`, the diffs of `commands/error.rs`, `lib.rs`, `database/mod.rs`, `Cargo.toml` and `docs/path-model.md`, the migration, the owner's Mac terminal output, `verify-main3.txt` and the report's sections 1, 5, 6 and "Concepts used". Commands I ran: `git diff --stat 6b2e67b..HEAD` (scope), the unsafe, SAFETY and unwrap greps above, a Python test of two deferred SQLite transactions in WAL mode (the second writer failed with `database is locked`), and a read of `sqlx-core 0.8.6` for `begin()`.

## Fix prompt for the agent

> TASK-1C, review round 1 (`reports/REVIEW-1C.md`). Read it and spec v2.1.4 §3.5 D29 first (the owner has committed it). Stay on branch `task/1C`. Allowed paths are the task's list plus `docs/path-model.md`; change nothing else.
>
> **Code, in this order, one commit each:**
> **C1.** In `library/roots.rs`, replace both `pool.begin()` calls (`add_root`, `remove_root`) with `pool.begin_with("BEGIN IMMEDIATE")` (check that sqlx 0.8.6 exposes it on `Pool`; if the name differs, use the equivalent and say so). Fix the doc comments. Add a test with two `add_root` calls on the same directory started together (`tokio::join!`, a pool with at least 2 connections): exactly one is `Ok` and the other is `RootError::Overlaps`, never `RootError::Catalogue`. Add a second test that holds a `BEGIN IMMEDIATE` transaction on another connection for 300 ms while `add_root` runs: it waits and succeeds. Show the first test **failing** with `pool.begin()` (revert after) and passing with the fix; paste both runs.
> **C2.** In `platform/macos.rs` add a SAFETY comment to the three sites without one (`CStr::from_ptr`, `mem::zeroed`, `from_raw_parts`) and check, before building the slice, that the offset is non-negative and `offset + length` is inside the buffer; if not, label is `None`. Name the layout assumption (volume name before UUID, packed 4) in the SAFETY text.
> **C3.** In `library/protected.rs` remove the separate filesystem-root test. Keep every other rule. Tests: with the synthetic Windows context (app data under `C:\`), `C:\` is `root_protected` (it contains app data) and `D:\` is allowed; with the synthetic Unix context, `/` is `root_protected` and `/Volumes/Photos` is allowed; a UNC share root such as `\\srv\share` is allowed unless it contains a protected folder. Update the sentence in `docs/path-model.md` §6.
> **C4.** In `library/case_probe.rs`, when no ancestor has a cased name, take the first entry inside the path (read-only `read_dir`, at most 64 entries) whose file name has a cased character, and run the same swap-and-compare on it; if there is none, or listing fails, return `Unknown`. Keep the injected `FileIdentityLookup`; put the directory listing behind a small function so a unit test can call it on a temp directory. Add a Windows-only test that probes `C:\` on the runner (expected `CaseInsensitive`).
> **C5.** In `library/canonical.rs` remove the backslash and trailing-colon check on components (D29(j)); collapse the identical `NotFound`/`_` arms. Update `docs/path-model.md` and the tests. Add a Unix-only test that a directory named `Trip: 2024` is accepted.
>
> **Report (`reports/REPORT-1C.md` only), after the code:** do R1 to R7 exactly as written in the review. Then run `pnpm verify` on the Mac, `git status --short` (must be empty), push, and paste `gh run list --branch task/1C --limit 4` once both OS runs are green. Put the final commit and run in the header. Do not start 1C2.

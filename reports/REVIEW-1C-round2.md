# REVIEW-1C, round 2: Roots and volumes (backend)

Reviewer: orchestrator (Claude) · Date: 2026-11-22 · Branch `task/1C`, HEAD `7a360a5` (docs-only commit: the report header) · Last code commit `8811955` (clippy), report commit `515242f` · CI run `44989685964` (green, cited by the report for `515242f`; I did not see the run list for `7a360a5`) · Report: `reports/REPORT-1C.md` (814 lines) · Round 1: `reports/REVIEW-1C.md`

## Verdict: **ACCEPT** (merge when ready; four small report nits are listed and do not block)

All five code fixes (C1 to C5) are in the code, each with the tests I asked for. The report corrections R1 to R7 are done, with the nits below. The owner's real-machine evidence agrees with the code: Mac `pnpm verify` green (98 tests: 97 passed, 1 ignored), the laptop starts the app, and the platform-stamp mismatch shows the start-up dialog and stops.

## Round 1 findings, one by one

| # | Asked | Result | What I read |
|---|---|---|---|
| C1 | `BEGIN IMMEDIATE` in `add_root` and `remove_root`; race test; wait test; show the race test failing with `pool.begin()` | **Done** | `roots.rs` calls `pool.begin_with("BEGIN IMMEDIATE")` in both functions; the doc comment now lists the probe before the transaction. `test_add_root_concurrent_race_serialises_overlap` runs two `add_root` calls on the same directory with `tokio::join!` and asserts exactly one `Ok` and one `RootError::Overlaps`. `test_add_root_waits_for_held_immediate_transaction` holds an IMMEDIATE transaction for 300 ms and asserts `add_root` waited at least 200 ms and then succeeded. The report shows the race test failing with `pool.begin()` as `SqliteError { code: 5, message: "database is locked" }`, then passing after the revert. The rollback test now also asserts zero folders |
| C2 | SAFETY text on the three macOS sites; bounds check before `from_raw_parts` | **Done** | `platform/macos.rs`: 7 `unsafe` sites and 7 SAFETY comments; the name is read only if `offset >= 0` and `ref_offset + offset + name_len` fits in the buffer, otherwise the label is `None`. The layout assumption (name before UUID, packed 4) is in the SAFETY text |
| C3 | Remove the separate filesystem-root test; `D:\` and `/Volumes/X` allowed; `C:\` and `/` refused by containment | **Done** | `is_fs_root` is gone from `protected.rs`; tests `test_protected_unix_system_root_and_volumes_photos` and `test_protected_windows_c_drive_d_drive_and_unc_share`; `docs/path-model.md` sentence updated |
| C4 | Probe fallback for a root with no cased component | **Done** | `case_probe.rs`: `find_cased_child_entry` (read-only `read_dir`, `take(64)`, first name with a cased character); only that first entry is tried, and a failed lookup gives `Unknown`. Tests on a temp directory, and a Windows-only test that probes `C:\` on the runner |
| C5 | Drop the backslash and trailing-colon rule for the root path | **Done** | `canonical.rs`: the Unix loop is gone, the identical `NotFound` arms are collapsed; `test_canonicalize_root_accepts_trailing_colon_directory` (Unix) |
| R1 to R7 | Report corrections | **Done, four nits** (below) | Unsafe inventory 15/10/25 with SAFETY texts; serialisation section; four-run CI log; race and probe demonstrations; deviations section |

The diff from my round-1 merge (`14c22bd`) to `7a360a5` touches 7 files, all inside the allowed paths. No `#[tauri::command]`, no dialog, no React change: the agent stopped before 1C2, as told.

## The owner's evidence

| Evidence | What it shows |
|---|---|
| Mac: sync-19 committed `0a1b00b`, merged into `task/1C` as `14c22bd`; `pnpm verify` 98 tests green; `gh run list` | The branch is the one I reviewed, and the checks run on the Mac, not only in CI |
| Mac: `ls` on the NTFS USB drive | A case-swapped name is not found: macOS's NTFS driver is case-sensitive, so the probe's `CaseSensitive` was right and my surprise was wrong (risk 53) |
| Laptop: `volume_report` on `C:` and on a folder on the USB drive `D:` | `C:` is `Internal` and `CaseInsensitive`. The USB drive (a WD easystore) is reported `Internal`, as I predicted: Windows often calls a USB hard disk "fixed", and `kind` is informational (D29(e)). The drive-root fallback (`D:\` itself) has run only in the unit tests and the CI runner's `C:\`, not on a real external drive |
| Laptop: `pnpm tauri dev` | The app window opened normally and the owner closed it. Risk 18 (`tauri dev` half) is closed |
| Laptop: platform stamp changed to `unix-bytes` with Python, then `tauri dev` | The start-up error dialog appeared and the app stopped; restoring `windows-utf16le` brought App Info back with `windows-utf16le` and 2 migrations. The dialog check passed on Windows |

One thing in the screenshots is new (risk 55, cosmetic): while the dialog is open the WebView window is already visible behind it and shows Tauri's raw text "state not managed for field `pool`", because the page asks for state that was never set up. The app exits when the owner presses OK, so nothing is lost, but a user would see an ugly message for a moment. A later fix: create the window hidden or after the catalogue is ready. Not for 1C.

## Report nits (fix in the next report; do not reopen 1C)

1. **Header, macOS version.** The header says "macOS 27.0 (Build 26A428 ...) from sw_vers" without the raw output; I cannot confirm it from anything I have. The owner runs `sw_vers` once (NEXT_STEPS) and pastes it; I record it.
2. **Files-changed block (§3).** It contains a literal `...` and says 4165 insertions; the real figure is 4243.
3. **§2 verify row** still cites the old CI run `44935233670` instead of `44989685964`.
4. **HEAD `7a360a5`** is a docs-only commit. Its CI run is not in anything I saw; the code is identical to `515242f`, which is green. The owner's `gh run list` after merging will show it.

## Carried forward (add to TASK-1C2, done in its file)

- `list_roots` still calls the blocking `is_dir()` inside an async function (risk 54). 1C2 moves it to blocking threads with a 2 s timeout.
- The 1B nit about building the error-leak samples without the template argument is still open; take it when `error.rs` is next touched.
- `case_sensitive` is not refreshed when a volume row is reused, and `fs_uuid` on Windows is a per-machine volume GUID (1F).

## What I did not check

- I did not run any Rust test, build or CI job (this session has no libvips). Every "done" above is from reading the code and the report, plus the owner's terminal and laptop output.
- I did not read the CI logs, only the run id and result the report and the owner's `gh run list` give.
- The `D:\` fallback is untested on real hardware (see above).

## How I checked

Fetched `origin/task/1C` into a read-only clone, diffed `14c22bd..7a360a5`, read the changed functions and tests in full, counted `unsafe` and SAFETY lines in `macos.rs`, compared the report's claims about each fix with the code, and read the owner's Mac terminal, the two volume reports, the laptop transcript and the three screenshots.

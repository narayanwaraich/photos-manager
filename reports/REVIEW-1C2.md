# REVIEW-1C2: Roots in the app (folder picker in Rust, four commands, Zod DTOs, "Library Folders" card)

Reviewer: orchestrator (Claude) · Date: 2026-11-22 · Branch `task/1C2`, HEAD `e323e8a` (docs-only: REPORT-1C2) · Last code commit `df7dcae` · CI run `45002530551` (green; the owner's `gh run list`; the report cites the same id) · Report: `reports/REPORT-1C2.md` (410 lines)

## Verdict: **ACCEPT** (merge when ready; report nits and three small code nits are listed, none blocks)

The task did what it was written to do. A person presses "Add folder…", a native dialog opens on both OSes, the folder appears with its volume, overlap and protected choices show the fixed error text, removing shows the counts first, and the list cannot freeze on a dead share. The path never comes from the WebView. The owner's manual check passed on the Mac and on the Windows laptop (all eight items, screenshots of the dialogs on both OSes), and CI is green on both OSes.

## What I checked, decision by decision

I read `src-tauri/src/commands/roots.rs` (732 lines, production part and all 14 tests), the `library/roots.rs`, `lib.rs`, `error.rs`, `mod.rs` and `errorCodes.ts` diffs, `schemas.ts`, `index.ts`, `App.tsx` and `LibraryFolders.tsx` in a read-only clone at `origin/task/1C2`.

| Decision | Result | Evidence in the code |
|---|---|---|
| a. No path from the WebView | **Met** | `add_root(pool, ctx, dialog_guard, window, label: Option<String>)` has no path parameter; `rfd::AsyncFileDialog::new().set_parent(&window).pick_folder().await` runs in Rust. `rg "plugin:dialog\|@tauri-apps/plugin" src src-tauri/capabilities` prints nothing; `invoke(` appears only under `src/lib/tauri/`; `capabilities/`, `tauri.conf.json`, `Cargo.toml`, `deny.toml`, `ci.yml` are not in the diffstat |
| b. Four commands and wire format | **Met** | `list_roots`, `add_root`, `preview_remove_root`, `remove_root` registered in `generate_handler!`; four wire-format tests compare the serialised JSON of `RootSummary`, the add result (added with warnings, cancelled without `rootId`), the preview and `IpcError` with literal strings |
| c. Testable shape | **Met** | `add_root_with_picker(pool, ctx, guard, label, picker)` takes `FnOnce() -> Future<Output = Option<PathBuf>>`; the command is a seven-line wrapper. No test opens a dialog |
| d. `reachable` off the executor | **Met, with limits (below)** | `list_roots_with_checker_and_timeout`: one `tokio::spawn` per root, inside it `spawn_blocking` for the check and `tokio::time::timeout` around the join; timeout, panic and join error give `false`; the in-flight set is a `Mutex<HashSet<RootId>>` cleaned by a Drop guard inside the blocking closure; a root already in flight reports `false` without a second thread |
| e. One dialog at a time | **Met** | `compare_exchange` on an `AtomicBool`, a Drop guard that resets it (also when the future is dropped), code `dialog_busy` with fixed text added to `ALL_CODES`, the drift test's emitted set and `errorCodes.ts`. The test holds a first dialog open with two oneshot channels, asserts `dialog_busy` for the second call, then a third call succeeds after the first closes |
| f. Picker by evidence | **Met** | Option 1 (`rfd::AsyncFileDialog`, parent window), no new crate, no new `unsafe` (`git diff` adds none), no capability change. Real dialogs on both OSes in the owner's screenshots (macOS sheet; Windows Common Item Dialog with the button showing "Opening Dialog…") |
| g. Warning texts | **Met, one nit** | `formatWarning` has the two sentences from the task. The fallback line prints the code in brackets (`unknown ... (${code})`); the task said never raw text. The code is a fixed backend string, so no leak, but it is not what the task asked (nit 4) |
| h. Count-and-confirm | **Met** | Preview → dialog with counts and "Nothing on your disk is touched." → `removeRoot(rootId, fileCount)`. On `root_changed_since_preview` the message is shown and `previewRemoveRoot` runs again; no silent retry. Any other error closes the confirm. Backend tests: right count removes, stale count returns `root_changed_since_preview` and deletes nothing |
| i. One component | **Met** | `LibraryFolders.tsx` (326 lines, inline styles), rendered from `App.tsx`; Add and Refresh disabled while a call is pending; "not reachable" mark; empty state |

Error handling: the tests for protected, overlapping and missing folders and for a label over 255 characters assert the fixed message and that the serialised error contains no path. The blank label falls back to the folder name (test present).

`errorCodes.ts` and `ALL_CODES` agree (the drift test is green in CI). No production `unwrap`/`expect` was added (grep of the diff).

## The reachable check: what it does and what it does not do

Read in `list_roots_with_checker_and_timeout`:

1. **A hung check keeps its thread until the OS call returns.** The timeout stops the wait, not the thread. The in-flight set stops a second thread for the same root, so a dead share costs one blocked thread per root, not one per refresh. That is what decision d asked for, and the report says so.
2. **A root whose earlier check is still running shows "not reachable"** even if it has become healthy meanwhile, until that check ends. Acceptable (it is the same answer a hung share would give), but 1F should know it.
3. **Two in-flight sets exist:** a process-wide static one inside `library::roots::list_roots` and the managed one the commands use. Only the managed one is reachable from the app. Nit, harmless.
4. A poisoned mutex is reported as `CatalogueError::Database(sqlx::Error::Configuration(...))`, the wrong variant for the situation. It cannot happen without a panic while holding the lock, and it maps to a generic code. Nit.

**Risk 54 closes** for `list_roots`. The scanner in 1D has the same hazard (a walk can block on a dead share and cannot be cancelled mid-call); 1D's task text says so.

## The mutation demonstration

The report shows the never-returning-checker test failing after 10.02 s with the timeout wrapper removed and passing in 10.01 s after the revert. Both times are about 10 s because the test's checker sleeps 10 s on a blocking thread and Tokio waits for blocking threads when the test runtime shuts down; the assertion itself (`elapsed < 500 ms`) is what tells the two runs apart, and it does. The demonstration is valid. The side effect is that the suite now carries a 10 s test and a 2 s test (the in-flight test). Nit 3: release the checker through a channel at the end of the test.

## The owner's evidence

| Evidence | What it shows |
|---|---|
| Mac: `pnpm verify` 114 tests (113 passed, 1 ignored), status clean; `gh run list --branch task/1C2` run `45002530551` green | The branch I read is the one that passed on the Mac and in CI |
| Mac: `sw_vers` | macOS 27.0, build 26A428: the header of REPORT-1C2 (and the one in REPORT-1C) now has a real source. 1C nit 1 is closed |
| Mac: sync-20 committed `8a41817`, 1C squash-merged `0edc778`, tag `accepted/1C` pushed | 1C is on `main`. **The main CI result for `0edc778` was not in the output** (the `gh run list --branch main` lines showed only old runs, probably taken before the run registered); asked again in NEXT_STEPS |
| Windows: fast-forward to `0edc778`, `volume_report`, `pnpm tauri dev` on `main` and on `task/1C2` | The app compiles and starts on the laptop with the 1C2 code. `pnpm verify` of the 1C2 code was not shown; CI's Windows job covers it |
| Manual check, Mac and Windows (all eight items ok: normal folder, same folder again, parent or subfolder overlap, protected folder, USB drive folder, cancel, count-and-confirm removal, unreachable) | The behaviour the task asked for, on both OSes. The protected-folder text on screen is "This folder is protected by the operating system and cannot be added." |
| `volume-usb-root.txt` (laptop, `PM_VOLUME_PATH=D:\`) | The drive-root fallback of 1C ran on a real external drive: kind `Internal` (Windows says "fixed" for this USB disk, as predicted), label `easystore`, filesystem NTFS, `fs_uuid` the volume GUID, case probe **CaseInsensitive**, not `Unknown`. This closes the last untested piece of risk 53 |
| Screenshots | Library Folders with the Downloads root (`Internal · Data`, 0 files · 0 folders) and Migrations Applied: 3; the overlap error banner; the macOS folder sheet; the Windows dialog open with the button disabled |

## Deviation and nits

**Deviation (listed by the agent, accepted):** the Zod enums for `volumeKind` and `status` accept PascalCase and lowercase, because `domain/enums.rs` (outside the allowed paths) serialises PascalCase. The agent was right to stop at the boundary. But the lowercase half is dead code that nothing can produce, and the report's closing "no other deviations" sits next to it. **Decision (orchestrator):** the wire format stays PascalCase (it is what serde derives and the tests freeze); 1D narrows the Zod enums to the real values and removes the lowercase ones (TASK-1D Part F).

Report nits (do not reopen 1C2; 1D's report rules cover them):

1. The header names commit `df7dcae` (the last code commit); the branch HEAD is `e323e8a` (docs-only). Fine, but say "last code commit".
2. The files-changed block omits `reports/REPORT-1C2.md` (the real diffstat has 11 files, +1707 / -11).
3. The greps are described in parentheses instead of pasted raw; line numbers in "Concepts used" (`roots.rs:69`, `:20`, `:55`, `library/roots.rs:295`) do not match the file as it stands; concept 5 cites commits, not a concept.
4. Section 10 proposes the next task. Tasks are written by the orchestrator (the same slip as in earlier reports).
5. The 10 s mutation timing is not explained (see above).

Code nits carried (small, none blocks):

- **N1 (1D, Part F):** Zod enums narrowed to PascalCase.
- **N2 (1D, Part F):** the never-returning-checker test released by a channel instead of a 10 s sleep.
- **N3 (next time `LibraryFolders.tsx` is touched, 1E):** unknown warning code shows a generic line without the code in brackets; the mutex-poison variant (nit 4 above).
- **Risk 55** (window visible behind the start-up dialog) stays open. The agent's one-paragraph idea (`"visible": false` in `tauri.conf.json`, then `window.show()` in `setup` after the catalogue is ready, with the dialog path unchanged) is reasonable; it needs a `tauri.conf.json` change, so it gets its own small task later.
- **Spec §12.3** still says "The dialog plugin is allowed for folder selection only"; the app uses `rfd` from Rust and no plugin. Fix at the next spec edit.

## What I did not check

- I did not run any Rust test, build or CI job (this session has no libvips). Every "met" above is from reading the code and the report plus the owner's terminal output, screenshots and files.
- I did not read the CI logs, only the run id and result the owner's `gh run list` shows.
- Windows `pnpm verify` of the 1C2 code and the main CI run for the 1C squash `0edc778` were not shown.

## How I checked

Fetched `origin/task/1C2` into a read-only clone, read the diffstat (11 files, +1707 / -11), the changed Rust and TypeScript files, the tests (names and bodies for the guard, in-flight, wire-format, removal and picker tests), re-ran the review greps (production `unwrap`/`expect` in the diff, `unsafe` added, `plugin:dialog`, `invoke(` outside `src/lib/tauri/`, `Command::new`, changes under `capabilities/` and `tauri.conf.json`), and compared the report with the code and with the owner's output.

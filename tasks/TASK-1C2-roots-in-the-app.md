# TASK-1C2: Roots in the app: folder picker in Rust, four commands, Zod DTOs, "Library folders" card

**Phase:** 1 · **Branch:** `task/1C2` (from `main`, after TASK-1C is squash-merged and the docs sync-20 is committed) · **Depends on:** TASK-1C accepted and merged (tag `accepted/1C`)
**Orchestrator note:** 1C built and proved the rules; this task connects them to the screen. It is the first task where a user chooses a folder, so the rules that matter are about **what crosses the boundary**: no path comes from the WebView, no error text contains a path, and a dead network drive cannot freeze the list. It is small in logic and big in wiring. Take it in the order of the Parts. You do not touch secrets, tokens, repositories, releases or settings (AGENTS.md, D17). This task needs none.

## Read first
- `AGENTS.md`
- `IMPLEMENTATION_SPEC.md` §3.5 (D4, D28, **D29** (accepted; (i) is this task)), §12.1 (command rules), §6.1, §16.2.
- Existing code, read before changing it: `src-tauri/src/commands/{mod,app_info,error}.rs` (the pattern: a plain `async fn` that tests call, plus a thin `#[tauri::command]` taking `tauri::State<'_, SqlitePool>`), `src-tauri/src/library/roots.rs` (`add_root`, `list_roots`, `preview_remove_root`, `remove_root`), `src-tauri/src/domain/roots.rs` (the DTO-shaped types, already `serde` camelCase), `src-tauri/src/lib.rs` (start-up, `app.manage(pool)`, `app.manage(protected_ctx)`, `generate_handler!`), `src/lib/tauri/{index,schemas,errorCodes}.ts`, `src/App.tsx`, `src-tauri/capabilities/default.json`.

## Facts (verify against the repo, do not assume)
1. `library::roots` has `add_root(pool, ctx, path, label)`, `list_roots(pool)`, `preview_remove_root(pool, &RootId)` and `remove_root(pool, &RootId, expected_files)`. `RootError` already maps to `IpcError` with eight codes (`root_path_invalid`, `root_not_unicode`, `root_overlaps`, `root_protected`, `root_label_invalid`, `root_not_found`, `root_changed_since_preview`, `volume_unavailable`); `ALL_CODES` and `errorCodes.ts` already contain them and a test keeps them equal.
2. `ProtectedContext` and the `SqlitePool` are already managed Tauri state.
3. `rfd 0.18.0` (`default-features = false`) is already a direct dependency (the start-up `MessageDialog`). There is no `tauri-plugin-dialog`. `capabilities/default.json` holds `core:default` only, and stays that way.
4. `list_roots` computes `reachable` with `Path::is_dir()` on the async executor (risk 54). On a dead SMB or NFS share this can block for tens of seconds.
5. The front end is a spike (`App.tsx` with inline styles, `useEffect` + `getAppInfo`); there is no UI library, no router, no TanStack Query, no frontend test runner. Do not add any of them.
6. 1B's rule stands: `IpcError` text is fixed per code and never contains a path, an id taken from user data, SQL or an OS error string.

## Decisions already made (follow them; if one conflicts with reality, stop and report)
a. **The dialog runs in Rust (D29(i)).** The WebView never sends a path. `add_root` takes no path argument: Rust opens the native folder picker, gets a path or "cancelled", and passes the path to `library::roots::add_root`. The only argument from the front end is an optional label.
b. **Four commands**, all `async`, all returning `Result<_, IpcError>`:
   - `list_roots()` → `Vec<RootSummary>`
   - `add_root(label: Option<String>)` → `{ outcome: "added" | "cancelled", rootId?: string, warnings: string[] }` (`warnings` are the `RootWarning` codes `case_sensitivity_unknown` and `network_volume`; empty when cancelled). Cancelling is **not** an error.
   - `preview_remove_root(rootId: string)` → `{ rootId, fileCount, folderCount, assetCount }`
   - `remove_root(rootId: string, expectedFiles: number)` → nothing (`null`)
   Tauri turns snake_case Rust arguments into camelCase for `invoke` (`root_id` is `rootId`); check this on a run and say so in the report.
c. **Testable shape.** Each command is a thin wrapper over a plain `async fn` (the 1B pattern). The dialog sits behind a parameter (a closure or a small trait that returns `Option<PathBuf>`), so tests inject a fake picker. No test opens a real dialog.
d. **`reachable` is computed off the async executor, with a time limit.** One `spawn_blocking` task per root, run together, each given **2 seconds** (the limit is a parameter so a test can use 50 ms). A timeout, a panic or a join error means `reachable = false` for that root. `list_roots` returns in about 2 s even if every share is dead. The blocked thread cannot be cancelled, so keep **at most one in-flight check per root** (a small `Mutex<HashSet<RootId>>` in managed state, or equivalent): a second `list_roots` call while a root's check is still running reports `false` for that root without starting another thread. Say in the report how you did it and what the limit of the approach is.
e. **No second dialog while one is open.** Keep a guard so `add_root` called twice at once opens one dialog and the second call returns an error. Prefer an existing code if one fits; if none does, add one code (`dialog_busy`, fixed text, in `error.rs`, `ALL_CODES` and `errorCodes.ts`; the drift test must pass). The UI also disables the button while the call is pending.
f. **The picker implementation is an open item; decide with evidence, in report section 1.** Option 1: `rfd::AsyncFileDialog::pick_folder` with the main window as parent (check whether Tauri's window type can be passed as the parent in `rfd` 0.18 with `default-features = false`, and whether the call needs the main thread on macOS). Option 2: `tauri-plugin-dialog` used **only from Rust** (a new dependency; the plugin must not give the WebView any permission: `capabilities/default.json` unchanged and the JS API not callable; prove it). Prefer Option 1 (no new dependency). The call must not freeze the app window, must work on both OSes, and a cancel must return `None`. If neither works on one OS, stop and report (stop conditions).
g. **Texts for the user.** Error text comes from `IpcError.message` (fixed per code, already written). Warning texts are written in the front end, one fixed English sentence per warning code, for example: `case_sensitivity_unknown`: "Could not tell whether this drive treats upper and lower case as different, so it is treated as case-insensitive."; `network_volume`: "This folder is on a network drive. Scanning may be slow and the folder can go offline." Show warnings after a successful add. Unknown warning codes show a generic line, never raw text.
h. **Removal UI is count-and-confirm.** Press Remove → call `preview_remove_root` → show "Remove this folder from the library? This deletes N files, M folders and K assets from the catalogue. Nothing on your disk is touched." with Cancel and Remove → on Remove call `remove_root(rootId, fileCount)`. If the code is `root_changed_since_preview`, show its message and re-run the preview (do not retry silently).
i. **Scope of the UI.** One component, `src/components/LibraryFolders.tsx`, rendered by `App.tsx` under the existing App Info block. Plain inline styles like the rest of the spike (the design pass comes in Phase 2). Rows show: label, `pathDisplay`, volume kind and label, a "not reachable" mark when `reachable` is false, file and folder counts, a Remove button. A header button "Add folder…". Empty state: a one-line hint. Errors from a command show in a visible line with the fixed message.

## Goal
On both OSes, the owner presses "Add folder…", a native dialog opens, a chosen folder appears in the list with its volume, a second overlapping or protected choice shows the fixed error text, removing a root shows the counts first, and a dead or missing folder shows as "not reachable" without freezing the window. In `cargo test`, the same flows are proved without a dialog.

## In scope

### Part A: report section 1 (before any code)
What you found in the existing `commands/`, `lib.rs` and `src/lib/tauri/` (keep, change, add, and why). The picker decision (decision f) with what you read and what you tried: versions, the exact `rfd` call, the parent-window and main-thread facts for macOS and Windows, any `cargo info` output for a new crate. The wire format of the four commands, as a table (name, arguments, result, error codes).

### Part B: Rust
1. `src-tauri/src/commands/roots.rs` (new): the four plain functions and the four `#[tauri::command]` wrappers; DTO types are the existing domain types where they already serialise correctly, or small new ones in this file.
2. `library/roots.rs`: change `list_roots` only as decision d says (the reachable check). No other behaviour change in `library/`.
3. `lib.rs`: register the four commands in `generate_handler!` next to `get_app_info`, and manage any new state (the in-flight set, the dialog guard).
4. Tests (names that say what they prove): wire format frozen by comparing the serialised JSON of each result to a literal string (camelCase fields, snake_case warning codes, `outcome` values); add with a fake picker returns `added` and a root row; the fake picker returning `None` returns `cancelled` and writes nothing; picking a protected folder, an overlapping folder and a missing folder return `root_protected`, `root_overlaps`, `root_path_invalid` with the fixed message and no path in the serialised error; a label longer than 255 characters returns `root_label_invalid`, and a blank label falls back to the folder name (the 1C behaviour); preview and remove with the right and with a stale count (`root_changed_since_preview`, nothing deleted); `list_roots` with a checker that never returns completes within the limit plus slack and reports `reachable = false`; a second call while the first check is still blocked does not start a second check; the dialog guard rejects a concurrent second add.

### Part C: TypeScript (`src/lib/tauri/`, `src/components/`, `src/App.tsx`)
1. `schemas.ts`: Zod schemas and inferred types for `RootSummary`, the add result, the preview, matching the Rust wire format exactly (enums as string unions).
2. `index.ts`: `listRoots()`, `addRoot(label?)`, `previewRemoveRoot(rootId)`, `removeRoot(rootId, expectedFiles)`, each returning `CommandResult<T>` and never throwing. Pull the common `invoke` + `safeParse` + error mapping that `getAppInfo` has into one small helper and use it for all five (a refactor of `getAppInfo` with unchanged behaviour; say so in the report).
3. `errorCodes.ts`: only if decision e adds a code.
4. `LibraryFolders.tsx` and the `App.tsx` change (decisions g, h, i). `tsc` and `eslint` clean; no new dependency.

### Part D: owner's manual check (you write the instructions; the owner runs them)
Add to the report's end a short, numbered list for the owner for **macOS and Windows**: `pnpm tauri dev`; add a normal folder (with photos); add the same folder again (expect the overlap text); add its parent or a subfolder (overlap); try the home folder (expect protected) and, on Windows, `C:\`; add a folder on a USB drive; cancel the dialog (nothing changes); remove a root and read the counts; unplug the USB drive or rename the folder and press Refresh/reload (shows "not reachable", window stays responsive). Say what to screenshot.

## Out of scope (do not do)
- Scanning, walking a root, hashing, thumbnails, metadata (1D and later).
- Changing a root's `status`, marking files `inaccessible`, matching a returned volume, watchers (1F). "Not reachable" is display only.
- Pause/resume, rename or edit a root, `follow_symlinks` or `include_hidden` switches.
- A design system, UI library, router, TanStack Query, a frontend test runner, pagination (the roots list is bounded, §12.1).
- Any change to `capabilities/`, `tauri.conf.json`, `deny.toml`, `ci.yml`, migrations, `AGENTS.md`, the spec, `docs/STATUS.md`, `docs/NEXT_STEPS.md`.
- Fixing the "window is visible behind the start-up dialog" cosmetic issue (STATUS risk 55). You may report how you would fix it, in one paragraph, in section 7.
- Any new `unsafe` code.

## Allowed paths
`src-tauri/src/commands/` (new `roots.rs`, edits to `mod.rs` and `error.rs`), `src-tauri/src/library/roots.rs` (`list_roots` only), `src-tauri/src/lib.rs` (handler list and managed state only), `src-tauri/Cargo.toml` and `Cargo.lock` (only if Part A decision f forces a feature flag; a new crate means stop and report first), `src/lib/tauri/`, `src/components/` (new), `src/App.tsx`, `reports/`.

## Acceptance criteria
- [ ] Report section 1 written first, including the picker decision and the wire-format table.
- [ ] No path from the WebView: the `add_root` command has no path parameter (paste the signature); `capabilities/default.json` and `tauri.conf.json` are **not** in `git diff --stat main..HEAD`; `rg -n "plugin:dialog|@tauri-apps/plugin" src src-tauri/capabilities` shows no match.
- [ ] All tests in Part B exist and pass on both OSes in CI; the wire-format tests compare against literal JSON.
- [ ] `list_roots` is shown not blocking: the test with a never-returning checker passes, and one run with the check made to block (mutation: put `is_dir()` back on the executor, or remove the timeout) is shown **failing**, then reverted (both runs pasted).
- [ ] The picker works in a real dialog on the Mac (screenshot by the owner) and on the Windows laptop (screenshot by the owner); the agent cannot do this and says so, then writes the instructions (Part D).
- [ ] `pnpm verify` green locally; `git status --short` empty afterwards; `gh run list --branch task/1C2 --limit 3` pasted, both OSes green; any red run explained with the tail of its failed log (the owner supplies it).
- [ ] Review greps pasted (below); expected matches only.
- [ ] Section 7 (deviations) lists anything done differently, every extra file touched, and the picker choice.

## Verification commands
```
pnpm verify
cargo test --manifest-path src-tauri/Cargo.toml
git status --short
gh run list --branch task/1C2 --limit 3
rg -n 'unwrap\(\)|expect\(' src-tauri/src/commands src-tauri/src/library -g '!*test*'
rg -n "unsafe" src-tauri/src
rg -n "std::process::Command|Command::new" src-tauri/src
rg -n "plugin:dialog|@tauri-apps/plugin|invoke\(" src src-tauri/capabilities
git diff --stat main..HEAD
```
`unsafe` matches must be the same set as on `main` (no new ones). `invoke(` may appear only under `src/lib/tauri/`.

## Report rules (STATUS risk 26: the reports have contained invented text)
Anything quoted as output must be raw output of a command you ran in this task; anything else is labelled "from source, not run". Put the final commit id and the last CI run id in the header, taken from `gh run list`; every CI id cited anywhere in the report must come from a `gh run list` you ran. Paste the `git diff --stat` verbatim, without `...`. Do not write a macOS or Windows version into the header unless it is pasted raw output of `sw_vers` or `ver`. Say what you mutated in every failure demonstration and show the revert. The header, the files-changed block and the "all criteria met" line must match the final state of the branch.

## Stop conditions
Stop and report (do not guess, do not work around) if: the picker needs a new crate, a WebView permission or a change to `capabilities/` or `tauri.conf.json` on either OS (report what you found, with the exact error); the dialog hangs, crashes or freezes the window on one OS; the only way to get a window handle is `unsafe`; `cargo deny` fails on a licence; a command would need to accept a path from the front end; or you find a conflict between this file, the spec and AGENTS.md.

## Deliverable
`reports/REPORT-1C2.md` (use `reports/_REPORT_TEMPLATE.md`; raw outputs, a run log taken from `gh run list --json`, "Concepts used" at most 5 items with `file:line`). Then stop. Do not start 1D.

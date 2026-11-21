# TASK-1C: Roots and volumes (backend only): overlap and protected-folder rules, read-only case probe, volume detection, add/list/remove with counts

**Phase:** 1 · **Branch:** `task/1C` (from `main`, after the docs sync-18 is committed) · **Depends on:** TASK-1A and TASK-1B accepted and merged (both are: `accepted/1A`, `accepted/1B`)
**Orchestrator note:** This is the first task that decides what the app is allowed to index, so most of it is **rules with tests**, not screens. The original plan row 1C (dialog, commands, UI, backend in one task) was too big for one review; it is split. **1C (this file): the Rust backend only, no Tauri command, no dialog, no React.** **1C2 (written after 1C is accepted): the folder dialog, the four IPC commands, the "Library folders" block in the UI.** Provisional plan after that: 1D scanner core; 1E jobs; 1F offline roots, watcher hints, Windows pass. Do only 1C.
You do not touch secrets, tokens, repositories, releases or settings (AGENTS.md, D17). This task needs none.

## Read first
- `AGENTS.md`
- `IMPLEMENTATION_SPEC.md` §3.5 (D26, D27, D28, **D29**: this task implements D29), §5.1, §5.2 (the six tables), §6.1, §6.4, §12.1, §16.2. Where this task file and the spec differ, **this file wins**; the orchestrator folds the differences into the spec later.
- `docs/path-model.md` (D26: the encoding, `path_key`, the platform guard; the root and folder paths you store follow it).
- Existing code to read before changing it: `src-tauri/src/domain/` (types, `CatalogueError`), `src-tauri/src/database/` (pool, repositories, migrations runner), `src-tauri/migrations/` (**never edit an applied migration**), `src-tauri/src/commands/error.rs` (`IpcError`, `ALL_CODES`), `src/lib/tauri/errorCodes.ts`, `src-tauri/src/lib.rs` (start-up, the managed state), `scripts/verify.mjs`.

## Facts (verify against the repo, do not assume)
1. The six Phase 1 tables exist (1A): `volumes`, `library_roots`, `folders`, `files`, `assets`, `asset_files`. D27: every root has exactly one **root-folder row** (empty relative path, empty `path_key`, `parent_id` NULL); `files.folder_id` is `NOT NULL ... ON DELETE CASCADE`; `assets.primary_file_id` cascades. So deleting a `library_roots` row removes everything under it in the database, and never a file on disk. The database does **not yet enforce** "one root-folder row per root" (1A review carry-forward); 1C adds the index.
2. `volumes` has `kind` (`internal|removable|network|unknown`), `label`, `fs_uuid`, `fs_type`, `case_sensitive` (0/1), `last_seen_at_ms`. `library_roots` has `volume_id`, `label`, `path_display`, `path_native` (BLOB, D26 encoding), `status` (`active|paused|offline|removed`), `follow_symlinks`, `include_hidden`, `created_at_ms`, `updated_at_ms`, `last_scan_at_ms`, `last_scan_generation`. Check the real migration; the spec is the target, the migration is the truth.
3. The Mac catalogue is at 2 applied migrations (the owner's App Info check). Your new migration is number 3 and applies on top of an existing catalogue. The 1B test that asserts `migrations_applied` must move from 2 to 3 (change the expected value, say so in the report).
4. `rfd` is already a dependency (1B start-up dialog). 1C does not open any dialog.
5. 1B's rule stands: `IpcError` text is fixed per code and never contains a path, an id taken from user data, SQL or an OS error string; `ALL_CODES` and `src/lib/tauri/errorCodes.ts` are kept equal by a test.

## Decisions already made (D29; follow them; if one conflicts with reality, stop and report)
a. **Canonical path.** A root is stored in canonical form: absolute, symlinks resolved (`std::fs::canonicalize`; on Windows the verbatim `\\?\` form is kept in `path_native` so paths over 260 characters work, and **stripped for `path_display` only**: `\\?\C:\x` shows as `C:\x`, `\\?\UNC\srv\share` as `\\srv\share`). A path that is not a directory, does not exist, is relative, or is not valid Unicode (D26) is rejected. A root that is itself a symlink is stored as its target; the display says the resolved path.
b. **Overlap rule.** Two roots must not be equal, nested one inside the other, or one containing the other. Test with the canonical component lists, each component **NFC-normalised and lower-cased, whatever the volume** (this over-rejects `/Photos` next to `/photos` on a case-sensitive volume and never under-rejects). Compare against **every stored root** whatever its status (`active`, `paused`, `offline`). Component-wise comparison, never string prefix (`/a/photos` and `/a/photos2` do not overlap).
c. **Protected folders** (rejected, code `root_protected`): a filesystem root (`/`, `C:\`, a UNC share root); the user's home folder itself (subfolders such as `Pictures` are fine); the parent of home (`/Users`, `C:\Users`) and `/Volumes`; the **app-data folder and any folder inside it or containing it** (the catalogue must never be scanned); OS system folders and anything inside them: macOS `/System`, `/Library`, `/usr`, `/bin`, `/sbin`, `/etc`, `/var`, `/private`, `/dev`, `/cores`, `/Applications`; Windows `%SystemRoot%`, `%ProgramFiles%`, `%ProgramFiles(x86)%`, `%ProgramData%`, and any `System Volume Information` or `$RECYCLE.BIN`; on macOS any folder equal to or inside a `*.photoslibrary` package (spec: other managers' libraries are ignored and never modified). The list is built once at start-up from the OS (home, app-data, environment variables) and **passed into the check as a value**, so tests inject synthetic folders and run identically on both OSes. The orchestrator proposed this list; the owner may veto items.
d. **Case sensitivity is probed read-only.** No file is created, renamed or written, in the root, in the app-data folder or anywhere else (D27c). Method: walk the canonical path from the root upwards to the first component whose name contains a cased letter; build the same name with its case swapped; ask the OS for the file identity of both (`stat`; Windows: file index or volume serial plus file id). Same identity: the volume folds case (`case_sensitive = 0`). The swapped name is absent, or has a different identity: it does not (`case_sensitive = 1`). **No component has a cased letter, or any `stat` fails unexpectedly: result `Unknown`, stored as `case_sensitive = 0`** (the safe side: more collisions reported, never fewer) **and `AddRootOutcome` carries the warning `case_sensitivity_unknown`.** The decision is a pure function over an injected "identity lookup", so both outcomes are tested without a case-sensitive disk; a real-filesystem test covers the runner's own volume. Per-directory case sensitivity on Windows (WSL flag) is not detected; say so in the report.
e. **Volumes.** The volume of a root is described by OS calls, not by parsing the text output of a command in the user's language. Fields: `kind` (network when the OS says remote; removable when the OS says removable; internal otherwise; `unknown` if the OS does not say), `label`, `fs_uuid` (macOS: the volume UUID; Windows: the volume GUID path `\\?\Volume{...}\` or the serial number if the GUID is unavailable; NULL if neither can be read), `fs_type`. **A volume row is reused when a root is added and an existing row has the same non-NULL `fs_uuid`** (then label and `last_seen_at_ms` are refreshed); otherwise a new row is made. Two roots on one volume with a NULL `fs_uuid` get two volume rows (limit, stated in the report; matching by user confirmation is 1F). A `network` volume is allowed with the warning `network_volume` (spec §6.1: read-only browsing and scanning, no watcher guarantee).
f. **`add_root` is one transaction:** validate, check overlap against the stored roots, find or insert the volume, insert the `library_roots` row, insert its root-folder row. The overlap check and the inserts happen in the same write transaction (use `BEGIN IMMEDIATE` or an equivalent; say which and why), so two adds cannot both pass the check. A failure at any step leaves no volume, root or folder row behind (test it by making the last insert fail).
g. **Removal is count-and-confirm, and the count is checked again.** `preview_remove_root(root_id)` returns counts (files, folders, assets). `remove_root(root_id, expected_files)` deletes the root row (everything else goes by cascade) **only if** the current file count equals `expected_files`; otherwise it fails with `root_changed_since_preview` and deletes nothing. The `volumes` row stays (reused later by `fs_uuid`). `status = 'removed'` is never stored in v1. **No file or folder on disk is ever touched**: a test removes a root over a temp tree and asserts every file still exists with unchanged bytes.
h. **Offline is detected, not acted on.** `list_roots` returns each root with a live `reachable: bool` (is the stored path a reachable directory now). 1C does **not** change `status`, mark files `inaccessible`, or match returned volumes; that is 1F.
i. **Scope of the IPC surface in 1C: none.** No `#[tauri::command]`, no `invoke`, no React change. The functions are `pub` items of the library crate and are called by tests. Do not add `#[allow(dead_code)]`; if clippy objects, make the item `pub` where it belongs.
j. **Lists bounded by construction** (the roots list) are not paginated; v2.1.3 of the spec says so in §12.1. Do not add pagination.

## Goal
`cargo test` proves, on macOS and Windows, that: a folder becomes a root only if it passes the canonical, overlap and protected rules; the volume is described and reused correctly; the case probe is read-only and decides correctly in both directions; `add_root` is atomic; the root-folder invariant is enforced by the database; and removing a root with its counts deletes catalogue rows only. The owner then runs one ignored test on real disks (internal, and an external drive if he has one) and pastes the output.

## In scope

### Part A: report section 1 (before any code)
Short: what you found in the existing `database/`, `domain/`, `lib.rs` and migrations (keep, change, add, and why); the exact crate and API choice for volume details on each OS with version, licence, maintenance state and what it needs (`cargo info`): expected the `libc` crate (macOS `statfs`, `getattrlist`) and `windows-sys` (`GetVolumeInformationW`, `GetVolumePathNameW`, `GetDriveTypeW`, `GetVolumeNameForVolumeMountPointW`), but verify; if the OS API cannot give `fs_uuid` on macOS without a shell command, you may call `diskutil info -plist <mount>` (its plist output does not depend on the user's language) **as a stated deviation with the reason**. A crate that needs a C compiler or system library is not allowed (AGENTS.md). `cargo deny` stays green: on a licence failure stop and report; do not edit `deny.toml`. Say how many `unsafe` blocks you add and give each a `// SAFETY:` comment that states what is guaranteed.

### Part B: migration 3 (the invariant)
1. New migration `src-tauri/migrations/<next number>_one_root_folder_per_root.sql`: `CREATE UNIQUE INDEX idx_folders_one_root_folder ON folders(root_id) WHERE parent_id IS NULL;` (name may differ; say so). Never edit migrations 1 and 2.
2. Tests: the migration applies on a database that already holds a root and its root-folder row; a second parentless folder for the same root is refused with a unique violation; a second root's own root-folder row is accepted; the 1B `migrations_applied` expectation is moved to 3.

### Part C: the pure rules (`library/`)
1. `canonicalize_root(path) -> Result<CanonicalRoot, RootError>` (canonical native path, display string, components) per decision a. Tests: relative path, missing path, a file instead of a directory, a symlink to a directory (Unix test; Windows junction or symlink test only if the runner can create one, else say so), Windows verbatim prefix stripped for display only (a pure function over strings, tested on both OSes), non-Unicode name (Unix test, as in 1A).
2. `check_overlap(new: &CanonicalRoot, existing: &[StoredRootPath]) -> Result<(), RootError>` per decision b. Table-driven tests: equal; new inside existing; new contains existing; siblings with a common string prefix (`photos`, `photos2`: allowed); differing only in case; differing only in NFC/NFD form; an `offline` root still counts; many roots.
3. `check_protected(new: &CanonicalRoot, ctx: &ProtectedContext) -> Result<(), RootError>` per decision c. Table-driven with synthetic folders for each category, on both OSes (build the synthetic context with the platform's path syntax: a test helper, not `cfg`-skipped tests); a folder next to a protected one is allowed; `Pictures` inside the synthetic home is allowed; home itself is refused; a folder containing the synthetic app-data folder is refused; a `*.photoslibrary` path is refused on macOS and allowed on Windows (assert the platform behaviour).
4. `ProtectedContext::from_environment(app_data: &Path) -> ProtectedContext` builds the real list once at start-up. A test calls it and asserts the list is non-empty and contains the app-data folder; print the list in the report for each OS (from CI or the owner's run).

### Part D: volumes and the case probe (`platform/`, `library/`)
1. `describe_volume(path) -> Result<VolumeInfo, VolumeError>` per decision e, one implementation per OS behind one signature. The OS code is thin; the mapping from raw values to `VolumeInfo` is a pure function with tests (a table of raw values to `kind`).
2. `probe_case_sensitivity(path, lookup) -> CaseProbe` per decision d, with a trait or function argument for the identity lookup and one real implementation. Tests with a fake lookup: folds case; does not fold; swapped name has a different identity; no cased component; the lookup fails. A real-filesystem test on a temp directory: the directory listing and modification times are identical before and after the probe (it wrote nothing), and the result equals the expected value of the runner's default volume (APFS and NTFS runners fold case; if a runner differs, assert only "no write" and print the result).
3. `#[ignore]` test `volume_report`: reads the folder in the environment variable `PM_VOLUME_PATH` and prints `describe_volume` and the case probe as plain lines. The owner runs it on real disks:
```
PM_VOLUME_PATH=/Volumes/SomeDrive cargo test --manifest-path src-tauri/Cargo.toml volume_report -- --ignored --nocapture
```
Write the exact Windows command (PowerShell and cmd) in the report; verify its syntax yourself, do not guess.

### Part E: the operations (`library/roots.rs`)
1. `add_root(pool, ctx, path, label) -> Result<AddRootOutcome, RootError>`; `AddRootOutcome { root_id, volume_id, warnings: Vec<RootWarning> }` (`case_sensitivity_unknown`, `network_volume`). `label` defaults to the folder's last component when empty; its length is bounded (say the bound). Defaults on insert: `status = 'active'`, `follow_symlinks = 0`, `include_hidden = 0`, timestamps from the existing clock helper (1B moved the clock errors to the right variants; reuse it).
2. `list_roots(pool) -> Result<Vec<RootSummary>, RootError>`: id, label, `path_display`, volume summary (kind, label, `case_sensitive`), `status`, `reachable` (decision h), counts of files and folders (one query for all roots, not one per root). Ordered by creation time then id.
3. `preview_remove_root(pool, root_id)` and `remove_root(pool, root_id, expected_files)` per decision g.
4. Tests (temp directories and an in-memory or temp-file database, as the 1A tests do): add one root; add two on one fake volume (one volume row, same `volume_id`); two different `fs_uuid` (two rows); NULL `fs_uuid` (two rows); overlap and protected refusals leave **zero** new rows; the transaction rolls back completely when the root-folder insert fails; `list_roots` counts with a hand-inserted tree of files; `reachable` false after the temp directory is removed (the stored status stays `active`); `remove_root` with the right count deletes the root, its folders, files and assets and leaves a second root's rows untouched; `remove_root` with a stale count returns `root_changed_since_preview` and deletes nothing; the files on disk are untouched after removal (bytes compared); removing an unknown id returns `root_not_found`.
5. Do not run a scan, read a directory listing beyond what the probe needs, or insert any `folders` row other than the root-folder row.

### Part F: errors
1. `RootError` (thiserror) with variants for: not absolute, not found, not a directory, not Unicode, overlaps, protected, already added is **the same as overlaps** (do not add a separate code), too long label, root not found, root changed since preview, volume details unavailable, and a wrapper for `CatalogueError`. Stable codes (suggested, say if you change them): `root_path_invalid`, `root_not_unicode`, `root_overlaps`, `root_protected`, `root_label_invalid`, `root_not_found`, `root_changed_since_preview`, `volume_unavailable`.
2. `From<RootError> for IpcError` with **fixed text per code** (decision 5 of the Facts). Add the codes to `ALL_CODES` and to `src/lib/tauri/errorCodes.ts` (a one-line TypeScript change per code, and the only React-side change in 1C); the existing drift test must pass and the existing table-driven leak test must cover every new variant with sample values such as `/Users/alice/secret.jpg` and a `path_key`.
3. Every user-facing message states what happened and what to do, in plain language (`root_overlaps`: "This folder overlaps a folder that is already in your library."). The strings live in the central place 1B created (spec D7).

### Part G: hygiene
`pnpm verify` green on the Mac and in CI on both OSes; `git status --short` empty afterwards; the review greps below pasted; the test counts before and after stated (the baseline is 46 on the Mac, from the 1B round-2 report; the Windows count was not run by the reviewer).

## Out of scope (do not do)
- The native folder dialog, any `#[tauri::command]`, any `invoke` or React component, Zod DTOs for roots (1C2).
- Scanning, walking a root, inserting `folders` rows other than the root-folder row, hashing, thumbnails, metadata (1D and later).
- Changing a root's `status`, marking files `inaccessible`, matching a returned volume, watchers (1F).
- Pause/resume of a root, editing `follow_symlinks` or `include_hidden` after adding (later; the columns keep their defaults).
- Changes to `src-tauri/capabilities/`, `tauri.conf.json`, `deny.toml`, `ci.yml`, migrations 1 and 2, `AGENTS.md`, the spec, `docs/STATUS.md`, `docs/NEXT_STEPS.md`.
- Spawning a shell to read volume details (except the one stated `diskutil -plist` fallback in Part A).
- A frontend test runner (still not now; the TypeScript side is checked by `tsc` and `eslint`).

## Allowed paths
`src-tauri/src/library/` (new), `src-tauri/src/platform/` (new), `src-tauri/src/domain/` (new types and error variants only), `src-tauri/src/database/` (new repository functions only), `src-tauri/src/commands/error.rs` (the `From<RootError>` mapping and `ALL_CODES` only), `src-tauri/src/lib.rs` (module declarations and building the `ProtectedContext` once at start-up; no behaviour change), `src-tauri/migrations/` (one new file), `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, `src/lib/tauri/errorCodes.ts`, `docs/path-model.md` (a short "Roots" section: canonical form, overlap, the `path_display` rule), `reports/`.

## Acceptance criteria
- [ ] Report section 1 (existing-code decisions, crate and API table with `cargo info` output, `unsafe` count and SAFETY texts) written first.
- [ ] Migration 3 applies on a catalogue that already has data; the one-root-folder index refuses a second parentless folder (test output pasted); the `migrations_applied` expectation is 3.
- [ ] Overlap, protected and canonical rules: every case listed in Part C has a test with a name that says what it proves; one test was shown failing against a deliberately broken rule (for example, string-prefix comparison instead of component comparison) and reverted (both runs pasted).
- [ ] The case probe wrote nothing (listing and mtimes equal before and after, pasted) and decides correctly in both directions with the fake lookup; the `Unknown` path stores `0` and reports the warning.
- [ ] `describe_volume` works on the runner's OS and the mapping table is tested; CI output of the ignored `volume_report` is **not** expected (the owner runs it on real disks); the commands for the owner are written and syntax-checked.
- [ ] `add_root` atomicity shown (the forced failure leaves zero rows); how the overlap check and the inserts are serialised is stated.
- [ ] `remove_root`: right count deletes the tree and nothing else; stale count deletes nothing; files on disk unchanged (bytes compared).
- [ ] `RootError` mapping: every variant has a code, fixed text and a leak test; `ALL_CODES` and `errorCodes.ts` agree.
- [ ] `pnpm verify` green locally; `git status --short` empty afterwards; `gh run list --branch task/1C --limit 3` pasted with both OSes green; any red run explained with the tail of its failed log (the owner supplies it).
- [ ] Review greps pasted (below). Expected matches only: the two Phase 0 spike lines in `vips.rs` and your new `unsafe` blocks with their SAFETY comments.
- [ ] Section 7 (deviations) is honest: list anything you did differently from this file, including any extra file you touched and the `diskutil` fallback if you used it.

## Verification commands
```
pnpm verify
cargo test --manifest-path src-tauri/Cargo.toml
git status --short
gh run list --branch task/1C --limit 3
rg -n 'unwrap\(\)|expect\(' src-tauri/src -g '!*test*'
rg -n "std::env::set_var" src-tauri/src src
rg -n "unsafe" src-tauri/src
rg -n "std::process::Command|Command::new" src-tauri/src
rg -n "File::create|fs::write|create_dir|OpenOptions|fs::rename|remove_file|remove_dir" src-tauri/src/library src-tauri/src/platform -g '!*test*'
git diff --stat main..HEAD
```
The last-but-one grep must show **no match** in `library/` and `platform/` production code (the probe and the rules write nothing). Explain any match.

## Report rules (STATUS risk 26: the reports have contained invented text)
Anything quoted as output must be raw output of a command you ran in this task; anything else is labelled "from source, not run". Put the final commit id and the last CI run id in the header. Say what you mutated in every failure demonstration and show the revert. The header, the files-changed block and the "all criteria met" line must match the final state of the branch (they were stale in 1A and 1B).

## Stop conditions
Stop and report (do not guess, do not work around) if: a crate or API needs a C library, a new WebView permission or a change to `capabilities/` or `tauri.conf.json`; `cargo deny` fails on a licence; the case probe cannot be made read-only on one OS; a volume's `fs_uuid` cannot be obtained without a shell on macOS and the `diskutil -plist` fallback does not work either; the only way to serialise `add_root` correctly is outside the allowed paths; the real migration differs from the spec in a way that breaks a rule above; or you find a conflict between this file, the spec and AGENTS.md.

## Deliverable
`reports/REPORT-1C.md` (use `reports/_REPORT_TEMPLATE.md`; raw outputs, a run log taken from `gh run list --json`, "Concepts used" at most 5 items with `file:line`). Then stop. Do not start 1C2.

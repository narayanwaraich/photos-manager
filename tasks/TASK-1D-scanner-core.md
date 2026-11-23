# TASK-1D: Scanner core: walk a root, record folders and files, generations, mark missing only after a complete walk

**Phase:** 1 · **Branch:** `task/1D` (from `main`, after TASK-1C2 is squash-merged and the docs sync-21 is committed) · **Depends on:** TASK-1C2 accepted and merged (tag `accepted/1C2`)
**Orchestrator note:** 1C and 1C2 put roots into the catalogue. This task makes a root's contents appear in it. It is **backend only**: no Tauri command, no UI, no jobs (1E adds the job manager, the `scan_root` command and progress). The risk in this task is not the walk, it is **the moment a file is declared missing**. A scan that is cancelled, that crashes, or that cannot read one folder must never make good files look deleted, and a file that was merely moved must keep its id. Read the Decisions slowly; the tests in Part C prove them. You do not touch secrets, tokens, repositories, releases or settings (AGENTS.md, D17). This task needs none.

## Read first
- `AGENTS.md`
- `IMPLEMENTATION_SPEC.md` §5.1, §5.2 (the `folders` and `files` tables), §5.3 (fingerprint, `scan_generation`), **§6.2 (the scan algorithm; step 7 is settled by decision (h) below)**, §6.4, §6.5, §11 (bounded memory, panics), §15 (scan target), §16.1, **D26 and D27 and D29** (§3.5), `docs/path-model.md`.
- Existing code, read before changing it: `src-tauri/src/domain/` (file, folder, root types), `src-tauri/src/database/{files,folders,roots,volumes,assets,models,mod}.rs` (`insert_file`, `find_file_by_key`, `insert_folder`, `find_folder_by_key`, `list_children`, `count_for_root`, the `PathKeyCollision` error and its test), `src-tauri/src/filesystem/` (native path BLOB, `PathKey`, the validators), `src-tauri/src/library/{roots,canonical,volume}.rs` (how a root and its root-folder row are created; `volume.case_sensitive`), `src-tauri/src/platform/{mod,windows,macos,stub}.rs` (existing `unsafe` and its SAFETY comments), `src-tauri/src/commands/error.rs` and `src/lib/tauri/errorCodes.ts` (how an error enum maps to codes and the drift test), `src-tauri/Cargo.toml`.

## Facts (verify against the repo, do not assume)
1. After 1C, a root has a `library_roots` row, a `volumes` row and exactly one root-folder row (`parent_id IS NULL`, empty relative path, empty key; the partial unique index from migration 3 enforces it). There are six catalogue tables plus `app_settings`: no `jobs`, `derivatives`, `file_hashes`, tags or albums yet.
2. `files` carries `file_identity TEXT`, `scan_generation TEXT`, `last_verified_at_ms`, `status IN ('present','missing','inaccessible','unsupported','error')`; `folders` carries `status IN ('present','missing','inaccessible')` and `last_seen_generation`; `library_roots` carries `last_scan_at_ms` and `last_scan_generation`. `files.folder_id` is `NOT NULL ... ON DELETE CASCADE` (D27(d)): **deleting a folder row deletes its files, so folders are only ever marked `missing`, never deleted.**
3. `walkdir` is **not** a dependency yet; `tokio-util` is not either. `windows-sys 0.61.2` with `Win32_Storage_FileSystem` and `Win32_Foundation` is (used by 1C for volumes). `unicode-normalization`, `uuid` (v7), `time`-style helpers and `tempfile` (tests) exist.
4. The CI runners and the owner's machines have plain folders only: no real photos in the repository, ever. All test trees are generated into temp directories by the tests.
5. The Mac has Unix-only and the laptop has Windows-only tests (as in 1A); a test that needs a Unix inode is `#[cfg(unix)]`, and so on.

## Decisions already made (follow them; if one conflicts with reality, stop and report)

**(a) Shape.** A library function `scan_root(pool, ctx?, root_id, options, cancel, progress) -> Result<ScanOutcome, ScanError>` in a new module `src-tauri/src/scanner/`. It takes the root from the catalogue by id (never a path), refuses a root whose `follow_symlinks = 1` (not implemented: `ScanError`, fixed code) and a root that is missing or not a directory (`ScanError::RootUnreachable`; **a scan never changes a root's `status`**, offline handling is 1F). `options` has the batch size (default 500, a parameter so tests can use 3), and a hook for failure injection in tests (see Part C). `progress` is an `FnMut(ScanProgress)` called after every committed batch. `ScanOutcome` is `Completed(ScanReport)` or `Cancelled(ScanReport)`; `ScanReport` holds the generation, counts (folders seen, files seen, new, changed, unchanged, moved, marked missing, marked inaccessible, unsupported, skipped by rule), the warning count, the first 100 warnings (code, a root-relative display string, never an absolute path or an OS error string in the *code* field) and the elapsed time.

**(b) Walk and memory.** The walk is blocking I/O: it runs on a blocking thread (`spawn_blocking`), never on the async executor. It sends **bounded batches** (about 500 entries) over a bounded channel (capacity 2) to an async writer that applies each batch in **one write transaction (`BEGIN IMMEDIATE`, as in D29(k))**. No `Vec` of the whole tree anywhere; memory stays bounded for a 100,000-file root. `walkdir` is the walker (spec §3.2): add it with an exact version, say so in the report, and run `cargo deny`; **any other new crate means stop and report first.** Directory order is not relied on; do not sort the whole tree.

**(c) Classification: by extension only in 1D** (magic-byte checks are not "cheap" enough to justify a read per file here; the spec's "where cheap" is satisfied by not doing it). Extensions are compared case-insensitively.
- Images: `jpg jpeg png webp tif tiff bmp gif heic heif avif` → `files.status='present'` and an `assets` row (`kind='image'`, `format` from a small tested mapping to a lower-case name such as `jpeg`, `png`, `heic`, dimensions and dates NULL) plus one `asset_files` row (`role='primary'`). One asset per file (§5.3).
- RAW (Tier 1, D15): `dng cr2 cr3 nef arw raf orf rw2` → the same as images (`format` is the extension); the preview comes in Phase 2.
- Video (`mp4 mov m4v avi mkv`): recorded with `files.status='unsupported'`, **no asset row** (§6.5, §13.5: recognised, not indexed in v1).
- Everything else is **ignored**: not recorded, only counted in `skipped by rule`.
- A tested function `classify(name) -> Class { Image(fmt) | Raw(ext) | Video | Ignored }`; the extension lists are constants with a test that each list has no duplicates and no overlap.

**(d) Skip rules (§6.2 step 2).** Symlinks are not followed and a symlink entry is not recorded (count it). Names always skipped: `.DS_Store`, `Thumbs.db`, `@eaDir`, `.Trashes`, `System Volume Information`, `$RECYCLE.BIN` (compare case-insensitively on every OS: the same list on both OSes). Hidden entries are skipped unless `library_roots.include_hidden = 1`: a name starting with `.` on both OSes, and on Windows also the `FILE_ATTRIBUTE_HIDDEN` bit from `std::os::windows::fs::MetadataExt::file_attributes()` (stable, safe). A skipped directory is not entered.

**(e) Names and keys.** Use the 1A path model for everything: native name BLOB, display string, `path_key` from the root-relative components (NFC on every volume, lower-cased when `volumes.case_sensitive = 0`). A name that is not valid Unicode → skipped with warning `path_not_unicode`. A name that cannot round-trip between the OSes (a backslash, a trailing colon; D26, §5.1) → skipped with a warning (use the existing 1A validator and its code). Two names with the same `path_key` in one folder → the second is skipped with warning `path_key_collision`; this is the first time the scanner meets `PathKeyCollision`: **add the missing assertion that the error carries `existing_id`** (carried from the 1A review). Windows reserved names and trailing dots are not special-cased in 1D (they cannot exist on NTFS as ordinary files; say so in the report).

**(f) Fingerprint and the unchanged path.** Fingerprint = size + modified time (ms) + file identity (§5.3). For an existing row with the same fingerprint: stamp `scan_generation` and `last_verified_at_ms` only (one `UPDATE`, no other column). Changed or new: upsert the `files` row and its asset. Later tables (`derivatives`, `file_hashes`, signatures, embeddings) are keyed by `source_fingerprint`, so a changed fingerprint invalidates them by itself: **do not add an invalidation function or any code for tables that do not exist** (spec: no stubs); say this in the report. Times are UTC Unix ms; a modified time that is before 1970 or that the OS does not give: do not invent a value; skip the file with a warning (`file_time_invalid`) and say in the report what you did (the CI runners will not produce one, so this is a one-line rule plus one test with an injected metadata function if it is cheap, otherwise reasoned in the report). `created_at_ms` is best-effort (NULL when the OS does not give it).

**(g) File identity.** Unix: `"<dev>:<ino>"` from `std::os::unix::fs::MetadataExt` (safe). Windows: the volume serial number plus the 128-bit file id (`GetFileInformationByHandleEx` with `FileIdInfo`) through the **already-present `windows-sys`** (`Win32_Storage_FileSystem`; a feature flag in `Cargo.toml` is allowed if the call needs one, a new crate is not), in `platform/windows.rs`, one small function with a `// SAFETY:` text for each `unsafe` block, opened with read-only share-all access, `None` on any failure (identity is best-effort and **never fails a scan**). std's `file_index()` is unstable, which is why this needs the API. Format `"<serial hex>:<file id hex>"`. A Windows-only test checks that two paths to one hard link give the same identity and two files give different ones. **If this cannot be done without a new crate or more than that one function of new `unsafe`, stop and report the options** (the fallback is: identity `None` on Windows in 1D, move detection then works on the Mac only, and 1F revisits). Show the cost: the `scan_report` run (Part D) on the laptop says what the per-file open costs in files per second.

**(h) Moves: identity only, decided after the walk (spec §6.2 step 7, orchestrator's proposal; hash-based moves wait for Phase 4).** Do **not** match moves while walking (a hard link has two live paths with one identity; stealing a row mid-walk would swap ids). After the walk **completed**, in one transaction, before marking anything missing: for this root, take the rows that were **not** seen in this scan (`scan_generation` differs) and the rows that **were** seen with a `created_in_catalog_at_ms` later than that stale row's `last_verified_at_ms`. If a file identity occurs **exactly once on each side**, it is a move: keep the stale row (its id, and later its tags, albums, favourite), update its folder, name, display path, native path, `path_key`, size and times to the new location, stamp the generation, and remove the duplicate row that the walk created (and its asset). **Ambiguous cases (an identity twice on either side, which is what a hard link looks like) are not moves**: the new row stays new, the old one becomes missing. A row with identity NULL is never matched. The rule is deliberately conservative: a wrong "move" would give one photo another photo's future tags, a wrong "new + missing" costs only history. If you find a flaw in the criteria above, propose the fix in the report and stop on it.

**(i) The one rule that matters: nothing is marked missing unless the walk completed, and nothing is marked missing under a folder that could not be read.**
- Marking happens only after the walk has finished successfully **and** the move step has run, in a final transaction: rows of this root whose generation differs from this scan's become `missing` (files, and folders). A cancelled, crashed, errored or failure-injected scan marks nothing. The root's `last_scan_at_ms` and `last_scan_generation` are written only then.
- **A directory that cannot be listed** (permission denied, vanished mid-walk): its folder row becomes `inaccessible`, and **every folder and file row below it is carried over**, not marked missing: restamped with this generation, files set to `inaccessible`. (Otherwise one locked folder makes its whole subtree look deleted. This is the case the Windows laptop and a permission-denied folder on the Mac will meet.) A file whose `stat` fails the same way: the row, if it exists, is stamped and set `inaccessible`; if it does not exist, a warning only. Each such case is a warning with a stable code (`entry_unreadable`, no OS text) and **never aborts the scan**.
- A file or folder that reappears becomes `present` (or `unsupported` for video) again and is restamped.
- A folder is never deleted (D27(d)).
- **Unreadable root itself** (the root path cannot be listed at all): `ScanError::RootUnreachable`, no row changes.

**(j) One scan per root at a time.** Two concurrent scans of one root would interleave generations and the first to finish would mark the other's rows missing. A small in-memory guard (`Mutex<HashSet<RootId>>` with a Drop guard, the 1C2 pattern) makes the second call return `ScanError::AlreadyRunning` immediately. 1E replaces this with the job manager. Cancellation in 1D is an `Arc<AtomicBool>` wrapper (`ScanCancel`), checked between entries and between batches (no `tokio-util`; 1E can swap in the job token). **Limit to state in the report:** a single filesystem call that blocks on a dead network share cannot be cancelled; the walk notices the flag only when that call returns (the same hazard as risk 54; 1F).

**(k) Errors.** `ScanError` (thiserror) with stable codes through `IpcError` like every other enum (D28): fixed text, no path, no OS string; add the codes to `ALL_CODES`, `errorCodes.ts` and the drift test's emitted set, with test cases like the existing ones. Needed: `scan_already_running`, `scan_root_unreachable` and, if you add it, `scan_root_option_unsupported`; database failures map to the existing codes. No Tauri command exists, so nothing else crosses the boundary.

**(l) Bounded warnings.** Keep at most 100 warnings in the report; count the rest (`warning_count`). A scan of a root with thousands of unreadable names must not grow memory or the report.

## Goal
`scan_root` on a folder of images, RAW files and videos produces folder rows, file rows and asset rows; a second scan of the unchanged folder is a cheap stamp; deleting, renaming, moving, restoring and changing files outside the app is reflected correctly; and every way a scan can stop early leaves the catalogue with **no wrong "missing"**.

## In scope

### Part A: report section 1 (before any code)
What you found in `database/`, `filesystem/`, `library/` and `platform/` (keep, change, add, and why); which repository functions you reuse and which you add (batch upsert, stamp, mark-missing, move step); the writer/producer design (channel, batch, transaction); the Windows identity decision (g) with what you read in `windows-sys` and the exact call; the walker and skip-rule tables; how the scan interacts with the existing `UNIQUE (root_id, path_key)` constraints.

### Part B: Rust
1. `src-tauri/src/scanner/` (new: `mod.rs` and, as you see fit, `classify.rs`, `walk.rs`, `moves.rs`): the function, the guard, `ScanCancel`, the DTOs, the classification, the skip rules, the producer, the writer, the move step, the final marking.
2. `src-tauri/src/database/`: new repository functions as needed (new file `scan.rs` or additions to `files.rs` / `folders.rs`); a repository function never opens its own transaction (the caller does). `database/mod.rs` declares the new file.
3. `src-tauri/src/platform/windows.rs`: the identity function (g). `platform/macos.rs` and `stub.rs`: nothing unless a trait seam needs it.
4. `src-tauri/src/lib.rs`: `mod scanner;` only. **No command is registered and nothing is managed.**
5. `src-tauri/src/commands/error.rs`, `src/lib/tauri/errorCodes.ts`: the new codes (k) and tests. Also fix the 1B nit in the error-leak test here (build the leak samples with the template argument, as the nit says: REVIEW-1B-round2), since `error.rs` is touched anyway.
6. `src-tauri/Cargo.toml` and `Cargo.lock`: `walkdir` (exact version) and, only if needed, one `windows-sys` feature flag.

### Part C: tests (names that say what they prove; temp directories; both OSes in CI)
Generated trees only. At least:
- classification table (each extension class; upper-case extensions; no extension; hidden and always-skipped names; a symlink is not recorded and not followed (Unix));
- first scan of a mixed tree: folder rows with correct parents and keys, file rows, one asset and one `primary` asset_file per image and RAW, video `unsupported` without asset, other files absent; counts in the report match the database;
- second scan unchanged: counts say unchanged, the only columns that changed are the generation and `last_verified_at_ms`;
- changed file (size or mtime) updated; deleted file becomes `missing` **only on a completed scan**; the file comes back → `present` again, same id;
- **batching:** with batch size 3 and 10 files, the writer commits several transactions and the final database is identical to the one from a large batch size;
- **interrupted scans (the gate for this phase), each ending with "no row is `missing` that exists on disk" and "a later complete scan gives the same database as a scan that was never interrupted":** (1) cancel after N entries; (2) failure injected after batch k (the hook makes the producer return an error mid-walk); (3) the future dropped after k batches (simulating a crash: simply stop awaiting the scan and start a new one); (4) a scan that fails in the move step or the final marking leaves nothing marked (use the hook);
- **unreadable directory** (Unix: `chmod 000` a subfolder in the temp tree, restored at the end of the test even on failure; Windows: if you cannot make a folder unreadable portably, say so and test the carry-over logic through the injected hook): the folder is `inaccessible`, its files are carried over as `inaccessible`, nothing below it is `missing`, the scan reports a warning and completes; then the permission is restored, rescan, and everything is `present` again;
- **moves (Unix; and Windows if (g) is done):** rename a file inside a folder, move it to another folder, rename a folder (all files inside): the file ids are kept, the old row is updated, no `missing` rows remain, and no duplicate rows; **hard links** (two names, one inode, one of them removed or both kept): never a wrong move, ids are not swapped; an identity NULL never matches;
- name rules: non-Unicode name (Unix, `OsStr` from bytes) → skipped with `path_not_unicode`; a name with a trailing colon or a backslash (Unix) → skipped with the 1A code; two names that collide by key (NFC and NFD forms of the same name, or case on a case-insensitive volume) → `path_key_collision`, the first one wins, the second is reported, and the assertion that the repository error carries `existing_id` exists;
- guard: a second `scan_root` for the same root while the first is blocked returns `ScanAlreadyRunning`; after the first ends a new scan works;
- unreachable root (path deleted) → `RootUnreachable` and no row changes; a root with `follow_symlinks = 1` → refused;
- warnings capped at 100 with the count correct when more occur;
- `ScanError` code mapping and the drift test.

**Show the gate test failing:** mutate the final marking so it runs even when the walk failed (or runs inside the producer), show the interrupted-scan test **failing** with the output, and revert; paste both runs and say what you mutated. Do the same for the move step: make it match identities without the "exactly once on each side" rule and show the hard-link test failing.

### Part D: real-folder report (the owner runs it)
An `#[ignore]` test `scan_report` (like `volume_report`): reads `PM_SCAN_PATH`, creates a temp catalogue and a temp app-data folder, registers the folder as a root through `library::roots::add_root` (so the real protected-folder rules apply), runs `scan_root` twice, and prints for each run the counts, the warnings (code and relative display only) and the elapsed time and files per second; then prints the number of rows per `status`. It only reads the folder (the owner points it at a copy or a real photo folder; the test never writes there). Add the exact commands for the Mac and for the Windows laptop (cmd, one `set` per line) to the report's end. The owner will run it on his real photos on both machines; the numbers are compared with the spec's 300 files per second first-pass target (§15), which is a stat-only walk here (no metadata yet).

### Part E: report
`reports/REPORT-1D.md` from the template, with the rules below.

### Part F: two small carried fixes (before the report)
1. `src/lib/tauri/schemas.ts`: `VolumeKindSchema` and `RootStatusSchema` accept only the values the backend sends (PascalCase: `Internal | Removable | Network | Unknown`, `Active | Paused | Offline | Removed`); remove the lowercase twins (REVIEW-1C2 N1). `tsc` and `eslint` clean.
2. `src-tauri/src/commands/roots.rs` (test module only): the never-returning-checker test releases its checker through a channel at the end instead of sleeping 10 s, so the suite is not held for 10 s (REVIEW-1C2 N2). The test must still fail when the timeout wrapper is removed (say how you checked).

## Out of scope (do not do)
- A Tauri command, a UI change, progress through Channels, the `jobs` table, job persistence (all 1E); `scan_root` is called only from tests and `scan_report`.
- Metadata (ExifTool), thumbnails, hashing, dimensions, dates (later phases); `assets.width`, `height` and dates stay NULL.
- Watching (1F), offline roots and changing a root's `status` (1F), matching a returned volume, refreshing `volumes.case_sensitive` (1F).
- Following symlinks, a UI switch for hidden files, per-root options beyond the existing columns.
- Hash-based move detection (Phase 4).
- Magic-byte sniffing, RAW+JPEG pairing, XMP sidecars, Live Photos.
- Any change to `capabilities/`, `tauri.conf.json`, `deny.toml`, `ci.yml`, migrations, `AGENTS.md`, the spec, `docs/STATUS.md`, `docs/NEXT_STEPS.md`. **No new migration.** If you believe the scanner needs one (a column or index), stop and report why.
- New `unsafe` anywhere except the one Windows identity function in (g).

## Allowed paths
`src-tauri/src/scanner/` (new), `src-tauri/src/database/` (new `scan.rs`, edits to `files.rs`, `folders.rs`, `mod.rs`), `src-tauri/src/platform/windows.rs`, `src-tauri/src/lib.rs` (`mod scanner;` only), `src-tauri/src/commands/error.rs` (codes, drift test, the leak-sample fix), `src-tauri/src/commands/roots.rs` (test module only, Part F.2), `src-tauri/Cargo.toml` and `Cargo.lock` (`walkdir`, at most one `windows-sys` feature flag), `src/lib/tauri/errorCodes.ts`, `src/lib/tauri/schemas.ts` (Part F.1), `reports/`.

## Acceptance criteria
- [ ] Report section 1 written first, including the Windows identity decision and the producer/writer design.
- [ ] `git diff --stat main..HEAD` shows only allowed paths; no `migrations/`, `capabilities/`, `tauri.conf.json`, `deny.toml`, `ci.yml`; `rg -n "tauri::command" src-tauri/src/scanner` prints nothing; `lib.rs` changed by the `mod` line only.
- [ ] Every test in Part C exists and passes on both OSes in CI (the Unix-only and Windows-only ones are named and counted per OS, as in 1A); the interrupted-scan tests assert both "nothing wrongly missing" and "a later complete scan equals an uninterrupted one".
- [ ] The two mutation demonstrations (final marking; move rule) are pasted with the failing output and the revert.
- [ ] No `unwrap`/`expect` in production code of the new files; `unsafe` matches are the same set as on `main` plus only the Windows identity function(s), each with a SAFETY comment (paste the greps).
- [ ] Memory is bounded: the report says how (channel capacity, batch size) and one test or a printed measurement shows a tree of at least 5,000 generated files scanned with batch size 500 without collecting the tree.
- [ ] `pnpm verify` green locally; `git status --short` empty afterwards; `gh run list --branch task/1D --limit 3` pasted, both OSes green; any red run explained with the tail of its failed log (the owner supplies it).
- [ ] `scan_report` exists and runs (the agent runs it on a generated or a small tree and pastes the raw output; the owner runs it on real folders).
- [ ] Section 7 (deviations) lists anything done differently, every extra file touched, every new dependency with exact version and licence, and the Windows identity outcome.

## Verification commands
```
pnpm verify
cargo test --manifest-path src-tauri/Cargo.toml
git status --short
gh run list --branch task/1D --limit 3
rg -n 'unwrap\(\)|expect\(' src-tauri/src/scanner src-tauri/src/database
rg -n "unsafe" src-tauri/src
rg -n "std::process::Command|Command::new" src-tauri/src
rg -n "tauri::command" src-tauri/src/scanner
git diff --stat main..HEAD
```
(`unwrap`/`expect` hits inside `#[cfg(test)]` modules are fine; say which hits are tests.)

## Report rules (STATUS risk 26: the reports have contained invented text)
Anything quoted as output must be raw output of a command you ran in this task; anything else is labelled "from source, not run". Put the final commit id and the last CI run id in the header, taken from `gh run list`; every CI id cited anywhere in the report must come from a `gh run list` you ran. Paste the `git diff --stat` verbatim, **including `reports/REPORT-1D.md` itself**, without `...`. Do not write a macOS or Windows version into the header unless it is pasted raw output of `sw_vers` or `ver`. Paste every grep as the raw command and its raw output (an empty result is shown as the command followed by nothing). Say what you mutated in every failure demonstration and show the revert. "Concepts used": at most 5, each a concept with a `file:line` you checked in the final tree. Do **not** propose the next task (the orchestrator writes tasks). The header, the files-changed block and the "all criteria met" line must match the final state of the branch.

## Stop conditions
Stop and report (do not guess, do not work around) if: the Windows identity needs a new crate or more than the one function of new `unsafe` (g); a migration seems needed; the move criteria in (h) have a flaw you can show; a test needs the real filesystem outside a temp directory; the walk cannot be kept off the async executor with bounded memory without a new crate other than `walkdir`; `cargo deny` fails on a licence; you find a conflict between this file, the spec and AGENTS.md; or an acceptance criterion cannot be met on one OS.

## Deliverable
`reports/REPORT-1D.md` (use `reports/_REPORT_TEMPLATE.md`; raw outputs, a run log taken from `gh run list --json`, "Concepts used" at most 5). Then stop. Do not start 1E.

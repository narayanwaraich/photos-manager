# TASK-1A: Catalogue core: path model, Phase 1 schema, repositories

**Phase:** 1 · **Branch:** `task/1A` (from `main`, after the sync-13 docs are committed) · **Depends on:** Phase 0 complete (it is)
**Orchestrator note:** First Phase 1 task. Pure Rust and tests: no Tauri commands, no UI, no scanning, no file changes, no libvips or ExifTool. It builds the two things every later task stands on: the **path model** (how a file's location is stored and compared) and the **catalogue tables** with their repositories. Provisional Phase 1 plan (each task is written when the previous one is accepted): 1A this; 1B app shell (IPC types D4, startup errors, libvips init); 1C roots and volumes; 1D scanner core; 1E jobs; 1F offline roots, watcher hints, Windows pass. Do only 1A.
You do not touch secrets, tokens, repositories, releases or settings (AGENTS.md, D17). This task needs none.

## Read first
- `AGENTS.md`
- `IMPLEMENTATION_SPEC.md` §5 (all), §6.1, §6.4, §12.1 (ids), §16.1. The spec in the repo is v2.1.2. Where this task file and the spec differ, **this file wins**; the differences are decisions D26 and D27 below, and the orchestrator folds them into the spec (v2.1.3) when this task is accepted, with your evidence.
- Existing code to read before changing it: `src-tauri/src/filesystem/mod.rs`, `src-tauri/src/database/mod.rs`, `src-tauri/migrations/`, `src-tauri/src/lib.rs`, `src-tauri/Cargo.toml`.

## Facts (verify against the repo, do not assume)
1. From REPORT-0A (read it in the repo, then the code): 0A left `src-tauri/src/filesystem/mod.rs` (151 lines: native-path BLOB conversion, display strings, an NFC/NFD probe; tests `test_roundtrip_non_ascii_path`, `test_roundtrip_non_utf8_path_unix`, `test_macos_nfc_vs_nfd_normalization_probe`, and `test_windows_cases_note`, which is a note and not a real test), `src-tauri/src/database/mod.rs` (pool with WAL, foreign keys, busy timeout; one test) and one migration, `20261102000000_create_app_settings.sql`. The report is a description; where the code differs, the code is true and you say so in report section 1.
2. CI runs `pnpm verify` (so `cargo test`) on macOS arm64 **and** Windows x64 for every push to `task/**`. Tests marked `#[cfg(windows)]` therefore run only in CI; you cannot run them locally and must say so.
3. Repositories are Phase 1 code that the scanner (1D) will call, so until then nothing in the app calls them. `cargo clippy -- -D warnings` runs in `pnpm verify`: if unused-code warnings appear, solve it with `pub` items in the library crate (check how `lib.rs` is organised) or a narrowly scoped, commented `#[allow(dead_code)]` that 1D removes; do not delete the code and do not weaken the lint globally. Do not write scanner-facing batch APIs now (see Out of scope).

## Decisions already made (follow them; if one conflicts with the spec or with reality, stop and report)
**D26, paths.**
- **Native BLOB encoding:** Unix = the raw bytes of the name; Windows = UTF-16LE code units. Both are lossless for any OS name.
- **Not valid Unicode (Unix: invalid UTF-8; Windows: unpaired surrogate) means "skip and report":** the catalogue constructor returns `PathError::NotUnicode` (stable code `path_not_unicode`); nothing lossy is ever stored (spec §5.1). The raw native conversion helpers stay lossless for any input (0A already tests this on Unix).
- **`path_key`:** the root-relative path, **every component NFC-normalised on every volume**, **lower-cased when the volume is case-insensitive**, components joined by `/`, as UTF-8 bytes. The key never depends on the OS separator. A volume's `case_sensitive` flag selects the case rule. Case folding uses `str::to_lowercase` after NFC; document its limits (for example Greek final sigma, Turkish İ) in `docs/path-model.md`. A false collision is reported, never silently merged; a collision the file system itself would not allow cannot occur.
- **Key collision:** inserting a second row with the same `(root_id, path_key)` returns `CatalogueError::PathKeyCollision` (code `path_key_collision`). It is a normal outcome the scanner will report as a warning, not a panic.
- **Catalogue platform guard:** the first time a database is opened, store `native_path_encoding` (`"unix-bytes"` or `"windows-utf16le"`) in `app_settings`. Opening it on the other kind of OS returns `CatalogueError::PlatformMismatch` (code `catalogue_platform_mismatch`), never a panic. This protects the BLOB encoding if a database file is ever copied between the Mac and the laptop.

**D27, schema corrections found by reviewing the spec before implementation.**
- `assets.primary_file_id` is `REFERENCES files(id) ON DELETE CASCADE`. As written in spec v2.1.2 (no action), deleting a root cascades to its files and then fails on the foreign key from `assets`; "remove a root" would be impossible. v1 has one asset per file, so cascading is correct. (RAW+JPEG pairs, spec §13.1, will re-point the primary before deleting.)
- **Each root has exactly one root-folder row:** empty relative path, empty `path_key`, `parent_id` NULL, `name` empty. Top-level folders have it as parent. **Every file has a non-NULL `folder_id`**, so the repository insert takes a `FolderId`, not an `Option`. (This makes the folder tree uniform and avoids the "NULL means in the root" ambiguity.)
- Case sensitivity of a volume is **never probed by writing into a root** (spec v2.1.2 said "test file in the app data dir", which cannot measure another volume). The probe is task 1C. In 1A the flag is simply a parameter.

**Other choices.**
- IDs are text UUIDs generated in Rust only, version 7 if the `uuid` crate offers it behind a feature (verify the feature name; v7 is time-ordered and keeps SQLite indexes tidy at 100k+ rows), otherwise v4 (say so). One function `new_id()`.
- SQLx **runtime** queries (`sqlx::query(...).bind(...)`, `query_as`), not the compile-time `query!` macros: those need a prepared offline cache committed to the repository. The price is that a typo in SQL shows up only in a test, so **every repository function gets a test**.
- Repository functions take `&mut SqliteConnection` so the scanner can later write about 500 rows in one transaction (spec §6.2 step 5). Callers pass a pooled connection or `&mut *tx`.
- Timestamps are `i64` UTC milliseconds passed in as parameters (`now_ms`), never read inside repositories, so tests are deterministic.

## Goal
`cargo test` proves, on macOS and Windows, that: paths convert to native bytes, display strings and keys by rules that survive property tests; the Phase 1 tables exist after one additive migration, reject bad data with the right error kind, and cascade correctly; roots can be removed without any file system call; and a database cannot be reopened on a different kind of OS.

## In scope

### Part A: report section 1 (before any code)
Short: what you found in the existing modules (list the functions you keep, change or remove and why); the exact versions, licences and maintenance state of every new crate (`cargo info`, registry page): expected candidates `unicode-normalization` (NFC), `proptest` (dev), `uuid` with a v7 feature, `tempfile` (dev, likely present). A crate that needs a C compiler or system library is not allowed (AGENTS.md).

### Part B: path model (`src-tauri/src/filesystem/`)
1. `NativePath` (bytes) with lossless `from_os_str` and `to_os_string` (Windows: odd byte length is an error, not a panic).
2. `RelPath`: a validated root-relative path made of normal components only. Built by `RelPath::from_absolute(root: &Path, abs: &Path)` and by `RelPath::from_components`. Rejects: any `..`, `.`, root or drive-prefix component, an empty component, a NUL character, "not inside root". It never touches the file system and never canonicalises.
3. From a `RelPath`: `native()` (bytes with the OS separator), `display()` (UTF-8, always `/`), `file_name()`, and `path_key(policy)`.
4. `PathKeyPolicy { case_sensitive: bool }`, key rules exactly as in D26.
5. Replace or remove 0A's note test and keep the intent of its NFC/NFD test as a real assertion.
6. Windows specifics that belong here: a verbatim root (`\\?\C:\...`) and a verbatim file path work; mixing a verbatim root with a non-verbatim file path returns a clean error (no panic). Reserved names, trailing dots and the 260-character limit are Phase 5 (plans), not now.

### Part C: domain types (`src-tauri/src/domain/`, new)
- Id newtypes `VolumeId`, `RootId`, `FolderId`, `FileId`, `AssetId` so a file id cannot be passed where an asset id is expected. Hand-written or one short, commented `macro_rules!` (the owner reads this code).
- Enums for every status/kind column of the Phase 1 tables (`VolumeKind`, `RootStatus`, `FolderStatus`, `FileStatus`, `AssetKind`, `AssetStatus`), each with `as_str()` and a fallible parse. A test inserts **every variant** of each enum into its column and checks that an unlisted string is rejected, so the Rust enums and the SQL `CHECK` lists cannot drift apart.
- `CatalogueError` (thiserror) with `code() -> &'static str` for the codes named above plus `not_found`, `size_out_of_range`, `database`. Sizes above `i64::MAX` are rejected before SQL (spec §5.1).

### Part D: migration (`src-tauri/migrations/`)
One new additive migration file, timestamp later than the existing one, creating **only**: `volumes`, `library_roots`, `folders`, `files`, `assets`, `asset_files` and the indexes `idx_files_size`, `idx_files_identity`, `idx_assets_date`. Copy the definitions from spec §5.2 with the D27 changes. No `jobs`, `derivatives`, hashes, tags, FTS or any other table (additive per phase; AGENTS.md forbids empty tables for later work). Never edit the applied 0A migration.

### Part E: repositories (`src-tauri/src/database/`)
Only what the tests need to prove the schema: volumes (insert, get, list); roots (insert, get, list, set_status, delete, `count_files`); folders (insert, `find_by_key`, `list_children`); files (insert, `find_by_key`, `count_for_root`); assets (`insert_with_primary_file`: asset row plus its `primary` `asset_files` row, in the caller's transaction, and `get`). Deleting a root removes catalogue rows only; the repository module imports no `std::fs`, `tokio::fs` or OS-path APIs (the review greps for it).

### Part F: platform guard (`database/mod.rs`)
As specified under D26; the pool initialiser returns an error, never panics.

### Part G: tests (all in `cargo test`; temporary directories only, no real photos)
1. **Native round trip:** property test (proptest) over arbitrary bytes on Unix and arbitrary `u16` sequences on Windows.
2. **Key rules (property tests):** NFC and NFD spellings of the same name give the same key; a key does not change with the separator; on a case-insensitive policy `A.JPG` and `a.jpg` give one key and on a case-sensitive policy two; a generated component list never produces a `RelPath` containing `..`, a prefix or a NUL; display is always valid UTF-8 and equals the components joined by `/`.
3. **Rejections (table-driven):** each rejected shape from Part B returns the specific error variant (assert the variant, not just `is_err()`).
4. **Migration from the previous schema:** build a database with only the 0A migration, insert an `app_settings` row, run the full migrator, assert the row survives and the six new tables exist. (`sqlx::migrate!` checksums: this is why the applied migration is never edited.)
5. **Foreign keys on every connection:** open a pool with at least two connections and assert `PRAGMA foreign_keys` is 1 on each; inserting a file with an unknown `root_id` fails with a foreign-key error kind.
6. **Uniqueness:** a second file or folder with the same `(root_id, path_key)` returns `PathKeyCollision`.
7. **Cascade:** a root with a root-folder row, two sub-folders, three files and three assets (with `asset_files`) is deleted; all dependent rows are gone, the `volumes` row remains, and the call touches no file. **Prove the test can fail:** run it once against the spec v2.1.2 definition of `assets.primary_file_id` (no `ON DELETE CASCADE`), paste the foreign-key failure, restore the migration, paste the green run (the 0F lesson: a guard you have not seen fail proves nothing).
8. **Enum and `CHECK` sync** (Part C) and **size range** (a size above `i64::MAX` is rejected before SQL; a negative stored size is rejected by the `CHECK`).
9. **Platform guard:** a database stamped with the other encoding string fails to open with `PlatformMismatch` (write the stamp with SQL in the test).
10. Windows-only tests (`#[cfg(windows)]`): verbatim root and file path, mixed forms, an unpaired surrogate gives `NotUnicode`. Unix-only: invalid UTF-8 gives `NotUnicode`; NFC/NFD names. State in the report that the Windows ones ran only in CI and give the run id.

### Part H: docs and hygiene
- `docs/path-model.md`, at most one page: the encoding, the key rules, the limits of case folding, the collision behaviour, the root-folder row, the platform guard. Written for the owner (a React developer learning Rust).
- After `pnpm verify`, `git status --short` must print nothing: tests and scripts must not modify tracked files. (On the owner's Windows laptop two tracked fixtures, `tests/fixtures/generated/image_plain.png` and `not_an_image.txt`, showed as deleted after earlier work on branch `task/0B2`; cause unknown. Check whether anything in `scripts/` or the tests removes tracked fixtures, and report what you find; fix it only if it is in our code and inside the allowed paths, otherwise report.)

## Out of scope (do not do)
- Tauri commands, IPC types (D4 is task 1B), UI, `src/` changes, startup error dialog, libvips init changes, ExifTool changes.
- Volume detection from the OS, the read-only case-sensitivity probe, adding or removing real roots, canonicalising real paths (task 1C).
- Walking directories, fingerprints, file identity, scan generations, marking files missing, move detection, batch upserts (task 1D), the `jobs` table (1E), watching (1F).
- Metadata, thumbnails, hashes, derivatives, tags, albums, FTS, any later-phase table or column.
- Editing `AGENTS.md`, the spec, `docs/STATUS.md`, `docs/NEXT_STEPS.md`.

## Allowed paths
`src-tauri/src/database/`, `src-tauri/src/filesystem/`, `src-tauri/src/domain/` (new), `src-tauri/src/lib.rs` (module declarations only), `src-tauri/migrations/` (one new file), `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, `docs/path-model.md`, `reports/`.

## Acceptance criteria
- [ ] Report section 1 (existing code decisions, crate table) written first.
- [ ] Every test in Part G exists and passes; the cascade test shown failing against the old definition and green after the fix, both pasted.
- [ ] The migration test (previous schema to new) passes; only the six tables and three indexes are created (`sqlite3` `.schema` output pasted).
- [ ] Windows tests ran in CI: `gh run list --branch task/1A --limit 3` and the Windows job's `cargo test` lines pasted; macOS and Windows both green.
- [ ] `pnpm verify` green locally; `git status --short` empty afterwards; new dependencies listed with exact version, source, licence.
- [ ] Review greps (paste the output): no `fs::`/`tokio::fs` in `database/`; no `unwrap()`/`expect(` outside tests in the new code (`rg -n 'unwrap\(\)|expect\(' src-tauri/src/database src-tauri/src/filesystem src-tauri/src/domain -g '!*test*'`); no `execute_sql`.
- [ ] `docs/path-model.md` written; the report lists what only CI or only one OS validated, and the fixture-deletion finding.

## Verification commands
```
pnpm verify
cargo test --manifest-path src-tauri/Cargo.toml
git status --short
gh run list --branch task/1A --limit 3
rg -n "std::fs|tokio::fs" src-tauri/src/database
rg -n "execute_sql" src-tauri/src
```

## Stop conditions
Stop and report (do not guess) if: a decision above conflicts with the spec or with SQLite or SQLx behaviour you observe; the migration cannot be made additive; a needed crate requires a C library or is unmaintained; the cascade test cannot be made to pass without changing a decision; or you find that the scanner (1D) would need something the schema cannot express.

## Deliverable
`reports/REPORT-1A.md` (use `reports/_REPORT_TEMPLATE.md`; raw outputs, a run log taken from `gh run list --json`, "Concepts used" at most 5 items with `file:line`). Then stop. Do not start 1B.

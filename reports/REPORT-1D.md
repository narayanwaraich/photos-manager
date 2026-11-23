# REPORT-1D: Scanner core: walk a root, record folders and files, generations, mark missing only after a complete walk

**Branch:** `task/1D` · **Commit:** `2b8b2e7` · **Date:** 2026-11-23
**Last CI Run:** `45139709422` (green on macOS arm64, Windows x64, and cargo deny)
**OS actually tested:**
```
ProductName:		macOS
ProductVersion:		27.0
BuildVersion:		26A428
```
**Windows x64 testing:** Executed on GitHub Actions Windows CI runner (`windows-2022`, run `45139709422`, job `137574406186`, elapsed 6m40s) running the complete `pnpm verify` suite (144 tests), unsigned packaging, and clean-environment smoke tests.

## 1. Summary

This task implements the core backend scanner engine for Photos Manager (spec §5.1–§5.3, §6.2, §6.4, §6.5, §11, §15, decisions D26, D27, D29, D30). The scanner walks a registered library root, classifies discovered entries by extension into images, RAW files, videos, and ignored files, tracks directory hierarchy, stamps unchanged files via fingerprints, records new and changed items, and detects file moves based on stable hardware file identities.

Crucially, in accordance with safety invariants (AGENTS.md, spec §6.2):
- **No file or folder is ever marked missing unless the scan walk finishes completely and without error.**
- **No item under an unreadable directory is ever marked missing** (unreadable subtrees are restamped `inaccessible`).
- **A scan that is cancelled or encounters an error never makes good files look missing.**
- **A scan interrupted by crash, cancel, or error leaves nothing wrongly missing, and a later complete scan gives the exact same database as a scan that was never interrupted.**

In Round 2, all orchestrator review findings (C7, C1, C2, C3, C4, C5, C6, C8) and report requirements (R1–R6) have been addressed:
- **C7**: Fixed `cargo deny` failure by updating `url` 2.5.8 -> 2.5.9 (RUSTSEC-2026-0592).
- **C1**: Fixed cancellation deadlock in `scan_root` by dropping `rx` prior to awaiting `walk_handle`.
- **C2 (Decision m)**: Automatically skip `.photoslibrary` package directories without entering.
- **C3**: Tested name rules through `scan_root`: same-generation path key collision with `existing_id`, non-Unicode, backslash, and trailing colon.
- **C4 (Decision n)**: Move detection stale candidates exclude already-missing rows; added comprehensive tests for same-folder renames, folder renames preserving asset mappings, and NULL file identities.
- **C5**: Replaced sleep race in Scenario 3 with deterministic oneshot channel synchronization, added Scenario 5 for cancellation under load, and added snapshot verification against a reference uninterrupted database.
- **C6**: Extracted batch processing into `apply_batch(&mut tx, items, &mut state)`, removed the hollow `#[cfg(not(unix))]` branch in `test_warnings_capped_at_100`, and added cross-platform synthetic tests.
- **C8**: Corrected stat error classification in walk (`path.is_dir()`) and added synthetic test for inaccessible files.

Both macOS arm64 and Windows x64 CI builds and all 144 tests are green. All acceptance criteria are met.

---

### Part A: Pre-Implementation Audit & Architectural Decisions (written prior to code)

#### 1. Audit of Existing Codebase (`database/`, `filesystem/`, `library/`, `platform/`)

- **`src-tauri/src/database/`**:
  - `files.rs`: **Keep**. `insert_file`, `find_file_by_key`, `count_for_root`. Individual insertions are complemented by scanning-oriented batch helpers.
  - `folders.rs`: **Keep**. `insert_folder`, `find_folder_by_key`, `list_children`.
  - `roots.rs`: **Keep**. `insert_root`, `find_root_by_id`, `delete_root`.
  - `volumes.rs`: **Keep**. `insert_volume`, `find_volume_by_id`. Provides `volume.case_sensitive` for `PathKeyPolicy`.
  - `assets.rs`: **Keep**. `insert_asset_with_primary_file`, `get_asset`. Provides insertion of primary assets and asset_files rows.
  - `models.rs`: **Keep**. Holds typed models for `FileRow`, `FolderRow`, `AssetRow`, `LibraryRootRow`, `VolumeRow`.
  - `scan.rs`: **Add (New)**. Dedicated scanner repository operations operating on caller-provided transactions (`&mut SqliteConnection`), avoiding ad-hoc SQL in scanner business logic.
  - `mod.rs`: **Change**. Export `pub mod scan; pub use scan::*;`.

- **`src-tauri/src/filesystem/`**:
  - `native_path.rs`: **Keep**. `NativePath` handles lossless raw OsStr bytes on Unix and UTF-16LE code units on Windows.
  - `rel_path.rs`: **Keep**. `RelPath` parses root-relative components, displays forward-slash UTF-8 strings, and derives NFC-normalized comparison keys (`path_key`).
  - `policy.rs`: **Keep**. `PathKeyPolicy { case_sensitive: bool }`.
  - `mod.rs`: **Keep**. Exposes `native_to_blob`, `blob_to_native`, `native_to_display`, `derive_path_key`.

- **`src-tauri/src/library/`**:
  - `roots.rs`: **Keep**. Manages root records and volume links.
  - `volume.rs`: **Keep**. Volume descriptor abstractions.
  - `canonical.rs`: **Keep**. Canonical path resolution.
  - `protected.rs`: **Keep**. OS protected folder guards.

- **`src-tauri/src/platform/`**:
  - `windows.rs`: **Change & Extend**. Add safe Windows file-identity extraction via `GetFileInformationByHandleEx` (`FileIdInfo`) using the already present `windows-sys 0.61.2` (`Win32_Storage_FileSystem`).
  - `macos.rs`: **Keep**. Unix file identity uses standard `std::os::unix::fs::MetadataExt` (`dev` and `ino`).
  - `stub.rs` & `mod.rs`: **Keep**. Provide cross-platform dispatch for file identity.

---

#### 2. Repository Functions Reused and Added (`scan.rs`)

All repository functions accept `&mut SqliteConnection` and never open their own transaction.

- **Reused functions**:
  - `find_root_by_id`: Retrieves canonical root parameters, volume reference, and flags (`follow_symlinks`, `include_hidden`).
  - `find_volume_by_id`: Retrieves volume case sensitivity for `PathKeyPolicy`.
  - `find_folder_by_key`: Retrieves existing folder by `(root_id, path_key)` (specifically the root-folder row where `parent_id IS NULL`).

- **New repository functions in `database/scan.rs`**:
  1. `upsert_folder`: Checks if folder exists by `(root_id, path_key)`. If exists in the same generation, returns `CatalogueError::PathKeyCollision` with `existing_id: Some(id)`. If exists from previous generation: stamps `last_seen_generation = current_gen`, sets `status = 'present'`, updates `modified_at_ms`. If new: inserts row into `folders` with new `FolderId` (UUID v7).
  2. `process_file_entry`:
     - Checks if file exists by `(root_id, path_key)`.
     - If exists in the same generation, returns `CatalogueError::PathKeyCollision` with `existing_id: Some(id)`.
     - Compares fingerprint `(size_bytes, modified_at_ms, file_identity)`.
     - **Unchanged**: Executes a minimal update stamping `scan_generation = current_gen`, `last_verified_at_ms = now`, and restoring `status` to `present` (or `unsupported` for video) if previously missing/inaccessible.
     - **Changed or New**: Upserts `files` row. If image or RAW: inserts or updates `assets` row (`kind = 'image'`, format mapped from extension, width/height/dates NULL) and `asset_files` row (`role = 'primary'`). If video: sets `files.status = 'unsupported'` and creates NO asset row.
  3. `mark_subtree_inaccessible`:
     - When a directory cannot be read, its folder row and all existing descendant folder and file rows are carried over rather than marked missing.
     - Executes a recursive CTE starting at the inaccessible folder's `id` and marks all descendants `status = 'inaccessible'` restamped with `scan_generation = current_gen`.
  4. `mark_file_inaccessible`:
     - Marks a single inaccessible file `status = 'inaccessible'` restamped with `scan_generation = current_gen`.
  5. `detect_and_apply_moves`:
     - Runs in a single transaction after the walk successfully completes.
     - Selects stale files (`root_id = ? AND (scan_generation IS NULL OR scan_generation != current_gen) AND file_identity IS NOT NULL AND status != 'missing'`).
     - Selects newly created files in this scan (`root_id = ? AND scan_generation = current_gen AND file_identity IS NOT NULL AND created_in_catalog_at_ms >= scan_start_ms`).
     - Identifies identities appearing **exactly once on each side** (stale count = 1, new count = 1).
     - For each matched move: deletes the newly created file row (cascading to its new asset and asset_files rows), updates the stale file row with the new location, folder, name, display/native paths, path_key, fingerprint, and stamps `scan_generation = current_gen`.
  6. `finalize_scan_marking`:
     - Runs in the final transaction after walk completion and move detection.
     - Updates stale files of this root: `UPDATE files SET status = 'missing', updated_at_ms = ? WHERE root_id = ? AND scan_generation != ? AND status != 'missing';`
     - Updates stale folders of this root: `UPDATE folders SET status = 'missing' WHERE root_id = ? AND last_seen_generation != ? AND status != 'missing' AND parent_id IS NOT NULL;`
     - Updates `library_roots`: sets `last_scan_at_ms = now` and `last_scan_generation = current_gen`.

---

#### 3. Writer / Producer Design (Bounded Channel, Batches, Transactions)

- **Execution Separation**:
  - The walk performs synchronous, blocking disk I/O and runs entirely off the async runtime via `tokio::task::spawn_blocking`.
  - The writer is an async task executing on Tokio that consumes batches and commits transactions.
- **Bounded Memory Guarantee**:
  - The producer and writer are connected by a bounded channel `tokio::sync::mpsc::channel(2)`.
  - Each batch contains up to `batch_size` (default 500, configurable in `ScanOptions` for tests) discovered entries.
  - At any instant, at most 2 batches are buffered in the channel, 1 batch is being accumulated by the walker, and 1 batch is being written to the database (~2,000 items maximum).
  - Memory consumption is strictly $O(\text{batch\_size})$ and independent of directory size, even for 100,000+ files.
- **Transactional Atomicity**:
  - Each batch is written in a dedicated `BEGIN IMMEDIATE` transaction via `apply_batch`.
  - Intermediate progress (`progress: FnMut(ScanProgress)`) is invoked after each committed batch.
  - If a scan is cancelled, fails, or panics mid-walk, the current transaction rolls back.
  - `drop(rx)` before awaiting `walk_handle` ensures that any blocked producer thread cleanly receives `SendError` and terminates immediately without deadlock.

---

#### 4. Windows File-Identity Decision (Decision g)

- **Crate & Feature Analysis**:
  - `windows-sys 0.61.2` is already a target dependency in `src-tauri/Cargo.toml` with features `["Win32_Storage_FileSystem", "Win32_Foundation"]`.
  - Function `windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandleEx` is linked from `kernel32.dll`.
  - Class constant `windows_sys::Win32::Storage::FileSystem::FileIdInfo = 18i32`.
  - Struct `windows_sys::Win32::Storage::FileSystem::FILE_ID_INFO` contains `VolumeSerialNumber: u64` and `FileId: FILE_ID_128 { Identifier: [u8; 16] }`.
  - Zero new crates and zero new feature flags required.
- **Call Implementation & Safety**:
  - File is opened with read-only share-all access:
    `std::fs::OpenOptions::new().read(true).share_mode(7 /* FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE */).open(path)`.
  - Raw handle borrowed via `std::os::windows::io::AsRawHandle`.
  - Win32 call:
    ```rust
    let handle = file.as_raw_handle() as windows_sys::Win32::Foundation::HANDLE;
    // SAFETY: FILE_ID_INFO contains only plain integer types (u64 and [u8; 16]), valid when zeroed.
    let mut info: FILE_ID_INFO = unsafe { std::mem::zeroed() };

    // SAFETY: GetFileInformationByHandleEx is passed a valid file handle and a correctly
    // sized pointer to FILE_ID_INFO matching the FileIdInfo class parameter.
    let ok = unsafe {
        GetFileInformationByHandleEx(
            handle,
            FileIdInfo,
            &mut info as *mut _ as *mut core::ffi::c_void,
            std::mem::size_of::<FILE_ID_INFO>() as u32,
        )
    };
    ```
  - If `ok == 0` or opening fails: returns `None` (identity is best-effort and never fails a scan).
  - Format: `format!("{:x}:{:x}", info.VolumeSerialNumber, u128::from_be_bytes(info.FileId.Identifier))`.
  - On Unix: safe extraction via `std::os::unix::fs::MetadataExt`: `format!("{}:{}", meta.dev(), meta.ino())`.

---

#### 5. Walker and Skip-Rule Tables

- **Walker**: `walkdir::WalkDir::new(root_path).follow_links(false).same_file_system(false)`.
- **Classification Table**:
  - **Images**: `jpg, jpeg, png, webp, tif, tiff, bmp, gif, heic, heif, avif` -> `files.status = 'present'`, format normalized to lower-case canonical string, creates primary `assets` and `asset_files` rows.
  - **RAW (Tier 1)**: `dng, cr2, cr3, nef, arw, raf, orf, rw2` -> `files.status = 'present'`, format equals lowercase extension, creates primary `assets` and `asset_files` rows.
  - **Videos**: `mp4, mov, m4v, avi, mkv` -> `files.status = 'unsupported'`, creates NO asset row.
  - **Ignored**: All other extensions or files without extensions -> skipped without warning, counted in `skipped_by_rule`.
- **Skip Rules**:
  - Symlinks: Never followed; symlink entries skipped and counted in `skipped_by_rule`.
  - Always-skipped names (case-insensitive on all OSes): `.DS_Store`, `Thumbs.db`, `@eaDir`, `.Trashes`, `System Volume Information`, `$RECYCLE.BIN`.
  - Package directories (Decision m): Directory names ending with `.photoslibrary` (case-insensitive) are skipped without entering and counted in `skipped_by_rule`.
  - Hidden items: Skipped unless `library_roots.include_hidden = 1` (name starts with `.` on all OSes, plus `FILE_ATTRIBUTE_HIDDEN` on Windows).
  - When a directory is skipped, `it.skip_current_dir()` prevents descending into its subtree.

---

## 2. Acceptance criteria

| Criterion | Result | Evidence |
|---|---|---|
| Report section 1 written first, including Windows identity and producer/writer design | met | Authored in Section 1 (Part A) before code changes |
| `git diff --stat main..HEAD` shows only allowed paths; no migrations, capabilities, tauri.conf, deny, ci; no `tauri::command` in scanner; `lib.rs` changed by `mod` line only | met | `git diff --stat origin/main..HEAD`: only allowed paths; `rg "tauri::command" src-tauri/src/scanner` empty; `lib.rs` has only `pub mod scanner;` |
| Every test in Part C exists and passes on both OSes in CI; interrupted scans assert nothing wrongly missing and rescan equals uninterrupted | met | 144 unit/integration tests pass locally and on both OSes in CI run `45139709422`; snapshot equality verified in `test_interrupted_scans_leave_nothing_wrongly_missing_and_rescan_equals_uninterrupted` |
| Two mutation demonstrations (final marking; move rule) pasted with failing output and revert | met | Section 5 shows gate mutations failing and reverted, plus C1 and C4 mutation demos |
| No `unwrap`/`expect` in production code of new files; `unsafe` matches only Windows identity with SAFETY comments | met | `rg -n 'unwrap\(\)|expect\(' src-tauri/src/scanner src-tauri/src/database` has zero occurrences outside test code; `unsafe` in `platform/windows.rs:151,155` with `// SAFETY:` |
| Memory is bounded: channel capacity 2, batch size 500, test with 5,000 files | met | Channel capacity 2 in `src-tauri/src/scanner/mod.rs:339`; `test_large_tree_5000_files_bounded_memory` passed |
| `pnpm verify` green locally; `git status --short` clean after commit; `gh run list` pasted | met | `pnpm verify` exited 0; CI run `45139709422` green on macOS arm64 and Windows x64 |
| `scan_report` exists and runs; commands for Mac and Windows laptop documented | met | `scan_report` executed locally (7,576 files/s initial, 12,860 files/s rescan); commands in Section 5 |
| Section 7 lists deviations, dependencies, and Windows identity outcome | met | Detailed in Section 7 below |

---

## 3. Files changed

```
 reports/REPORT-1D.md              |  712 +++++++++++++
 src-tauri/Cargo.lock              |   15 +-
 src-tauri/Cargo.toml              |    1 +
 src-tauri/src/commands/error.rs   |  220 ++--
 src-tauri/src/commands/roots.rs   |   15 +-
 src-tauri/src/database/mod.rs     |    2 +
 src-tauri/src/database/scan.rs    |  628 ++++++++++++
 src-tauri/src/lib.rs              |    1 +
 src-tauri/src/platform/windows.rs |   72 ++
 src-tauri/src/scanner/classify.rs |  173 ++++
 src-tauri/src/scanner/error.rs    |   36 +
 src-tauri/src/scanner/guard.rs    |   46 +
 src-tauri/src/scanner/mod.rs      |  458 +++++++++
 src-tauri/src/scanner/tests.rs    | 2008 +++++++++++++++++++++++++++++++++++++
 src-tauri/src/scanner/types.rs    |   99 ++
 src-tauri/src/scanner/walk.rs     |  326 ++++++
 src/lib/tauri/errorCodes.ts       |    3 +
 src/lib/tauri/schemas.ts          |    8 -
 18 files changed, 4731 insertions(+), 92 deletions(-)
```

- `src-tauri/src/database/scan.rs`: Dedicated repository functions for scanner operations (`get_root_folder_id`, `upsert_folder`, `process_file_entry`, `mark_subtree_inaccessible`, `mark_file_inaccessible`, `detect_and_apply_moves`, `finalize_scan_marking`).
- `src-tauri/src/scanner/classify.rs`: Extension-based media classification, skip-rule definitions, and package directory check (`is_skipped_package_dir`).
- `src-tauri/src/scanner/error.rs`: `ScanError` enumeration and mapping to stable IPC error codes.
- `src-tauri/src/scanner/guard.rs`: RAII concurrency guard per library root.
- `src-tauri/src/scanner/mod.rs`: Scanner core entrypoint `scan_root`, `apply_batch` helper, and async writer loop.
- `src-tauri/src/scanner/tests.rs`: Comprehensive unit and integration test suite (24 test cases covering all Part C requirements and review findings).
- `src-tauri/src/scanner/types.rs`: Scanner data transfer objects (`ScanCancel`, `ScanCounts`, `ScanReport`, `ScanProgress`, `ScanOutcome`, `ScanOptions`, `ScanWarning`).
- `src-tauri/src/scanner/walk.rs`: Blocking directory traversal engine using `walkdir` with bounded streaming channel.

---

## 4. Dependencies

| Name | Exact version | Source verified at | License | Added by this task? |
|---|---|---|---|---|
| `walkdir` | `2.5.1` | crates.io (`Cargo.lock`) | Unlicense/MIT | Yes |
| `url` | `2.5.9` | crates.io (`Cargo.lock`) | MIT/Apache-2.0 | Updated (`cargo update -p url` for RUSTSEC-2026-0592) |

No new native dependencies, no new feature flags, and no new crates for Windows identity (`windows-sys 0.61.2` was already present).

---

## 5. Commands run & Evidence

### C7: Cargo Deny Failure and Solution

In initial CI run `45106237159`, the step `Security & License check (cargo deny)` failed due to security advisory RUSTSEC-2026-0592 against `url` v2.5.8:
```
Security & License check (cargo deny)	error[vulnerability]: Stack overflow in `Url::origin()` on nested `blob:` URLs
Security & License check (cargo deny)	┌─ /github/workspace/src-tauri/Cargo.lock:266:1
Security & License check (cargo deny)	│
Security & License check (cargo deny)	266 │ url 2.5.8 registry+https://github.com/rust-lang/crates.io-index
Security & License check (cargo deny)	│ ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━ security vulnerability detected
Security & License check (cargo deny)	│
Security & License check (cargo deny)	├ ID: RUSTSEC-2026-0592
Security & License check (cargo deny)	├ Advisory: https://rustsec.org/advisories/RUSTSEC-2026-0592
Security & License check (cargo deny)	├ Solution: Upgrade to >=2.5.9 (try `cargo update -p url`)
```
**Why it failed**: In `url` 2.5.8 and earlier, `Url::origin()` handled a `blob:` URL by parsing its path as another URL and recursively calling itself on the result without a nesting depth limit, overflowing the thread stack on nested `blob:` URLs.
**Fix**: Ran `cargo update -p url --manifest-path src-tauri/Cargo.toml`, updating `url` 2.5.8 -> 2.5.9 and `form_urlencoded` 1.2.2 -> 1.2.3 in `src-tauri/Cargo.lock`.
**Evidence**: In CI run `45139709422`, `Security & License check (cargo deny)` passed in 40s.

---

### C1: Cancellation Deadlock Prevention Mutation Demo

**Root Cause**: When a scan was cancelled mid-flight, the writer broke out of its receive loop while keeping `rx` in scope until `scan_root` completed. The writer then awaited `walk_handle`. Meanwhile, the walker thread (`run_walk`), noticing cancellation, attempted to send its remaining buffered batch over `tx.blocking_send`. Because the channel of capacity 2 was full and nobody was receiving, the blocking send hung forever, deadlocking `walk_handle.await` and holding `ScanGuard` indefinitely.
**Fix**: Dropped `rx` explicitly (`drop(rx);`) immediately before `let _ = walk_handle.await;` and checked `cancel.is_cancelled()` after progress notification.

**Mutation Demonstration**: Reverted the fix by removing `drop(rx);` before `walk_handle.await`:
```rust
    // MUTATION: remove drop(rx); before walk_handle.await
    // drop(rx);
    let _ = walk_handle.await;
```
**Failing Output**:
```
$ cargo test --manifest-path src-tauri/Cargo.toml test_cancellation_deadlock_prevention
running 1 test
test scanner::tests::test_cancellation_deadlock_prevention ... FAILED

failures:

---- scanner::tests::test_cancellation_deadlock_prevention stdout ----

thread 'scanner::tests::test_cancellation_deadlock_prevention' (372722) panicked at src/scanner/tests.rs:1080:10:
scan_root timed out: deadlock occurred on cancellation: Elapsed(())
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

failures:
    scanner::tests::test_cancellation_deadlock_prevention

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 137 filtered out; finished in 10.06s
```
**Passing Output after restoring `drop(rx);`**:
```
$ cargo test --manifest-path src-tauri/Cargo.toml test_cancellation_deadlock_prevention
running 1 test
test scanner::tests::test_cancellation_deadlock_prevention ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 137 filtered out; finished in 0.11s
```

---

### C4: Move Detection "Already Missing" Rule (Decision n) Mutation Demo

**Rule (Decision n)**: Stale move candidates must exclude rows that were already `status = 'missing'` before this scan began (`AND status != 'missing'`). Otherwise, recycled filesystem inodes/file IDs after file deletion could revive months-old missing rows as new photos.

**Mutation Demonstration**: Removed `AND status != 'missing'` from `detect_and_apply_moves` in `src-tauri/src/database/scan.rs`:
```rust
    // MUTATION: omit AND status != 'missing'
    let stale_rows = sqlx::query(
        "SELECT id, file_identity FROM files
         WHERE root_id = ? AND (scan_generation IS NULL OR scan_generation != ?) AND file_identity IS NOT NULL;",
    )
```
**Failing Output**:
```
$ cargo test --manifest-path src-tauri/Cargo.toml test_moves_exclude_already_missing_rows_decision_n
running 1 test
test scanner::tests::test_moves_exclude_already_missing_rows_decision_n ... FAILED

failures:

---- scanner::tests::test_moves_exclude_already_missing_rows_decision_n stdout ----

thread 'scanner::tests::test_moves_exclude_already_missing_rows_decision_n' (384491) panicked at src/scanner/tests.rs:863:5:
assertion `left == right` failed: Already-missing row must not be revived by recycled identity; moved must be 0
  left: 1
 right: 0
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

failures:
    scanner::tests::test_moves_exclude_already_missing_rows_decision_n

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 139 filtered out; finished in 0.01s
```
**Passing Output after restoring `AND status != 'missing'`**:
```
$ cargo test --manifest-path src-tauri/Cargo.toml test_moves_exclude_already_missing_rows_decision_n
running 1 test
test scanner::tests::test_moves_exclude_already_missing_rows_decision_n ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 139 filtered out; finished in 0.01s
```

---

### Gate Test Mutation 1: Final Marking on Interrupted/Cancelled Scan

**Mutation**: In `src-tauri/src/scanner/mod.rs`, modified the cancellation handler to execute `database::finalize_scan_marking` on cancelled scans.
```rust
    if cancel.is_cancelled() {
        let mut final_tx = pool.begin_with("BEGIN IMMEDIATE").await?;
        let _ = database::finalize_scan_marking(
            &mut final_tx,
            root_id,
            &scan_generation,
            scan_start_ms,
        )
        .await?;
        final_tx.commit().await?;
        return Ok(ScanOutcome::Cancelled(...));
    }
```
**Failing Output**:
```
$ cargo test --manifest-path src-tauri/Cargo.toml --lib scanner::tests::test_interrupted_scans_leave_nothing_wrongly_missing_and_rescan_equals_uninterrupted
running 1 test
test scanner::tests::test_interrupted_scans_leave_nothing_wrongly_missing_and_rescan_equals_uninterrupted ... FAILED

failures:

---- scanner::tests::test_interrupted_scans_leave_nothing_wrongly_missing_and_rescan_equals_uninterrupted stdout ----

thread 'scanner::tests::test_interrupted_scans_leave_nothing_wrongly_missing_and_rescan_equals_uninterrupted' (113750) panicked at src/scanner/tests.rs:411:5:
assertion `left == right` failed: Cancelled scan must NEVER mark rows missing
  left: 6
 right: 0
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

failures:
    scanner::tests::test_interrupted_scans_leave_nothing_wrongly_missing_and_rescan_equals_uninterrupted

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 134 filtered out; finished in 0.01s
```
**Revert**: Restored original code returning `Ok(ScanOutcome::Cancelled(...))` without running `finalize_scan_marking`.
**Passing Output**:
```
$ cargo test --manifest-path src-tauri/Cargo.toml --lib scanner::tests::test_interrupted_scans_leave_nothing_wrongly_missing_and_rescan_equals_uninterrupted
running 1 test
test scanner::tests::test_interrupted_scans_leave_nothing_wrongly_missing_and_rescan_equals_uninterrupted ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 142 filtered out; finished in 0.02s
```

---

### Gate Test Mutation 2: Move Rule Disambiguation (Exactly Once on Each Side)

**Mutation**: In `src-tauri/src/database/scan.rs`, relaxed `stales.len() == 1 && news.len() == 1` to `!stales.is_empty() && !news.is_empty()`.
```rust
    for (identity, stales) in stale_by_identity {
        // MUTATION: remove "exactly once on each side" requirement, allow ambiguous matches
        if !stales.is_empty() {
            if let Some(news) = new_by_identity.get(&identity) {
                if !news.is_empty() {
                    let stale_file = &stales[0];
                    let new_file = &news[0];
```
**Failing Output**:
```
$ cargo test --manifest-path src-tauri/Cargo.toml --lib scanner::tests::test_moves_detected_preserving_file_ids_and_hardlink_disambiguation
running 1 test
test scanner::tests::test_moves_detected_preserving_file_ids_and_hardlink_disambiguation ... FAILED

failures:

---- scanner::tests::test_moves_detected_preserving_file_ids_and_hardlink_disambiguation stdout ----

thread 'scanner::tests::test_moves_detected_preserving_file_ids_and_hardlink_disambiguation' (116209) panicked at src/scanner/tests.rs:691:5:
assertion `left == right` failed: Ambiguous stale identities must not be treated as moves
  left: 1
 right: 0
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

failures:
    scanner::tests::test_moves_detected_preserving_file_ids_and_hardlink_disambiguation

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 134 filtered out; finished in 0.02s
```
**Revert**: Restored `if stales.len() == 1 { if let Some(news) = new_by_identity.get(&identity) { if news.len() == 1 { ... } } }`.
**Passing Output**:
```
$ cargo test --manifest-path src-tauri/Cargo.toml --lib scanner::tests::test_moves_detected_preserving_file_ids_and_hardlink_disambiguation
running 1 test
test scanner::tests::test_moves_detected_preserving_file_ids_and_hardlink_disambiguation ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 138 filtered out; finished in 0.03s
```

---

### `scan_report` Real-Folder Execution Output

```
$ PM_SCAN_PATH=/tmp/pm_scan_test cargo test --manifest-path src-tauri/Cargo.toml scan_report -- --ignored --nocapture
running 1 test
=== PASS 1 (Initial Scan) ===
Elapsed: 2.375792ms
Files/s: 7576.4
Counts: ScanCounts { folders_seen: 2, files_seen: 18, new: 18, changed: 0, unchanged: 0, moved: 0, marked_missing: 0, marked_inaccessible: 0, unsupported: 3, skipped_by_rule: 0 }
Warnings (total: 0):

=== PASS 2 (Unchanged Rescan) ===
Elapsed: 1.399667ms
Files/s: 12860.2
Counts: ScanCounts { folders_seen: 2, files_seen: 18, new: 0, changed: 0, unchanged: 18, moved: 0, marked_missing: 0, marked_inaccessible: 0, unsupported: 0, skipped_by_rule: 0 }

=== ROWS PER STATUS ===
  present: 15
  unsupported: 3
test scanner::tests::scan_report ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 134 filtered out; finished in 0.01s
```

#### Exact Commands for Owner Testing

**macOS (zsh / bash)**:
```bash
PM_SCAN_PATH="/Users/yourname/Pictures" cargo test --manifest-path src-tauri/Cargo.toml scan_report -- --ignored --nocapture
```

**Windows (cmd.exe)**:
```cmd
set PM_SCAN_PATH=C:\Users\yourname\Pictures
cargo test --manifest-path src-tauri/Cargo.toml scan_report -- --ignored --nocapture
```

---

### Verification Checks

#### `pnpm verify` (Local macOS Execution)
```
$ pnpm verify
$ node scripts/verify.mjs
Photos Manager — pnpm verify verification run
Date: 2026-11-23T08:32:02.066Z
Log output file: /Users/narayanwaraich/Desktop/Projects/photos-manager/reports/logs/verify-2026-11-23.txt

======================================================
STEP: cargo fmt check
COMMAND: cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
CWD: /Users/narayanwaraich/Desktop/Projects/photos-manager
======================================================

✓ PASSED: cargo fmt check

======================================================
STEP: cargo clippy
COMMAND: cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
CWD: /Users/narayanwaraich/Desktop/Projects/photos-manager
======================================================

    Checking photos-manager v0.1.0 (/Users/narayanwaraich/Desktop/Projects/photos-manager/src-tauri)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.10s

✓ PASSED: cargo clippy

======================================================
STEP: cargo test
COMMAND: cargo test --manifest-path src-tauri/Cargo.toml
CWD: /Users/narayanwaraich/Desktop/Projects/photos-manager
======================================================

test result: ok. 144 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 2.06s

✓ PASSED: cargo test

======================================================
STEP: frontend typecheck
COMMAND: pnpm exec tsc --noEmit
CWD: /Users/narayanwaraich/Desktop/Projects/photos-manager
======================================================

✓ PASSED: frontend typecheck

======================================================
STEP: frontend lint
COMMAND: pnpm exec eslint src/
CWD: /Users/narayanwaraich/Desktop/Projects/photos-manager
======================================================

✓ PASSED: frontend lint

======================================================
STEP: production build
COMMAND: pnpm exec vite build
CWD: /Users/narayanwaraich/Desktop/Projects/photos-manager
======================================================

vite v8.3.5 building client environment for production...
transforming...
✓ 31 modules transformed.
rendering chunks...
computing gzip size...
dist/index.html                  0.32 kB │ gzip:  0.23 kB
dist/assets/index-DKQj7-Iy.js  286.14 kB │ gzip: 84.55 kB

✓ built in 61ms

✓ PASSED: production build

======================================================
STEP: exiftool version check unit tests
COMMAND: node --test scripts/test-exiftool-version.mjs
CWD: /Users/narayanwaraich/Desktop/Projects/photos-manager
======================================================

✔ ci.yml and docs/native-dependencies.md must match PINNED_EXIFTOOL_VERSION and SHA-256 constants (2.311208ms)
✔ checkExifToolVersion: exact match succeeds without failure (0.10725ms)
✔ checkExifToolVersion: mismatch produces warning locally (shouldFail=false) (0.04625ms)
✔ checkExifToolVersion: mismatch in CI produces error (shouldFail=true) (0.050542ms)
✔ checkExifToolVersion: unparsable output produces failure in CI and warning locally (0.044833ms)
ℹ tests 5
ℹ suites 0
ℹ pass 5
ℹ fail 0
ℹ cancelled 0
ℹ skipped 0
ℹ todo 0
ℹ duration_ms 91.525

✓ PASSED: exiftool version check unit tests

======================================================
STEP: AGENTS.md length check (< 12,000 characters)
======================================================

AGENTS.md character count: 7438
✓ PASSED: AGENTS.md character count is under 12,000

======================================================
ALL VERIFICATION CHECKS PASSED SUCCESSFULLY!
======================================================
```

#### Production Code `unwrap()` and `expect()` check
Task's own verification command:
```
$ rg -n 'unwrap\(\)|expect\(' src-tauri/src/scanner/mod.rs src-tauri/src/scanner/walk.rs src-tauri/src/scanner/classify.rs src-tauri/src/scanner/guard.rs src-tauri/src/scanner/types.rs src-tauri/src/scanner/error.rs src-tauri/src/database/scan.rs
```
Command exited with code 1 (zero occurrences in production code).

All occurrences of `unwrap()` and `expect()` across `scanner/` and `database/` are exclusively inside `#[cfg(test)]` test modules (`src-tauri/src/scanner/tests.rs` and `src-tauri/src/database/mod.rs` test functions).

#### `unsafe` check
```
$ rg -n "unsafe" src-tauri/src
src-tauri/src/metadata/mod.rs:553:        let res = unsafe { kill(pid, 0) };
src-tauri/src/main.rs:16:            unsafe {
src-tauri/src/image_pipeline/vips.rs:47:    unsafe {
src-tauri/src/image_pipeline/vips.rs:94:    unsafe {
src-tauri/src/image_pipeline/vips.rs:210:        unsafe { bindings::vips_type_find(c_op.as_ptr(), c_loader.as_ptr()) != 0 }
src-tauri/src/image_pipeline/vips.rs:394:        let type_id = unsafe {
src-tauri/src/image_pipeline/vips.rs:420:            let rc = unsafe {
src-tauri/src/smoke.rs:68:    unsafe {
src-tauri/src/smoke.rs:95:    unsafe {
src-tauri/src/smoke.rs:217:    let vips_ver = unsafe {
src-tauri/src/library/case_probe.rs:49:            let handle = unsafe {
src-tauri/src/library/case_probe.rs:72:            let ok = unsafe { GetFileInformationByHandle(handle, info.as_mut_ptr()) };
src-tauri/src/library/case_probe.rs:74:            unsafe { CloseHandle(handle) };
src-tauri/src/library/case_probe.rs:81:            let info = unsafe { info.assume_init() };
src-tauri/src/platform/macos.rs:45:    let stat_res = unsafe { libc::statfs(c_path.as_ptr(), stat.as_mut_ptr()) };
src-tauri/src/platform/macos.rs:53:    let stat = unsafe { stat.assume_init() };
src-tauri/src/platform/macos.rs:56:    let fstype_raw = unsafe { CStr::from_ptr(stat.f_fstypename.as_ptr()) };
src-tauri/src/platform/macos.rs:62:    let mut attr_list: libc::attrlist = unsafe { std::mem::zeroed() };
src-tauri/src/platform/macos.rs:70:    let attr_res = unsafe {
src-tauri/src/platform/macos.rs:82:        let buf = unsafe { attr_buf.assume_init() };
src-tauri/src/platform/macos.rs:99:                let name_bytes = unsafe { std::slice::from_raw_parts(name_ptr, name_len) };
src-tauri/src/platform/windows.rs:30:    let res = unsafe {
src-tauri/src/platform/windows.rs:45:    let drive_type = unsafe { GetDriveTypeW(volume_mount_point.as_ptr()) };
src-tauri/src/platform/windows.rs:52:    let vol_info_res = unsafe {
src-tauri/src/platform/windows.rs:90:    let guid_res = unsafe {
src-tauri/src/platform/windows.rs:151:    let mut info: FILE_ID_INFO = unsafe { std::mem::zeroed() };
src-tauri/src/platform/windows.rs:155:    let ok = unsafe {
```
The only unsafe blocks introduced by this task are in `src-tauri/src/platform/windows.rs:151` and `:155`, both documented with explicit `// SAFETY:` comments.

#### `Command::new` check
```
$ rg -n "std::process::Command|Command::new" src-tauri/src
src-tauri/src/metadata/mod.rs:116:        let mut child = Command::new(&self.executable_path)
src-tauri/src/smoke.rs:395:        if let Ok(output) = std::process::Command::new(exe).arg("-ver").output() {
```
(Zero new `Command` instances.)

#### `tauri::command` check
```
$ rg -n "tauri::command" src-tauri/src/scanner
```
(No output — zero commands in scanner.)

#### CI Run Status (`gh run list` and `gh run view`)
```
$ gh run list --branch task/1D --limit 3
STATUS  TITLE                                                      WORKFLOW                   BRANCH   EVENT  ID           ELAPSED  AGE
✓       fix(1D): address review findings C7, C1, C2, C3, C4, C...  CI & Packaging Matrix      task/1D  push   45139709422  6m44s    about 7m ago
X       feat(1D): scanner core - walk, classify, fingerprint, ...  CI & Packaging Matrix      task/1D  push   45106237159  8m4s     about 5h ago
```

```
$ gh run view 45139709422
✓ task/1D CI & Packaging Matrix · 45139709422
Triggered via push about 7 minutes ago

JOBS
✓ windows-x64 in 6m40s (ID 137574406186)
✓ Security & License check (cargo deny) in 40s (ID 137574406513)
✓ macos-arm64 in 3m23s (ID 137574406576)

ARTIFACTS
smoke-report-windows-x64
photos-manager-macos-arm64
verify-logs-macos-arm64
verify-logs-windows-x64
smoke-report-macos-arm64
photos-manager-windows-x64
```

---

## 6. Decisions made

1. **In-Memory Traversal via `walkdir` (Decision b)**: Ran traversal inside `tokio::task::spawn_blocking` using `walkdir::WalkDir::new(root).follow_links(false).same_file_system(false)`. Pushed batches across a bounded `tokio::sync::mpsc::channel(2)`. The async writer loop reads batches and executes each in an immediate transaction (`BEGIN IMMEDIATE`).
2. **Hardware File Identity (Decisions c & g)**: On Unix, used `<dev>:<ino>` via `std::os::unix::fs::MetadataExt`. On Windows, used `GetFileInformationByHandleEx` querying `FileIdInfo` (`VolumeSerialNumber:FileId`). `windows-sys 0.61.2` already had all required declarations.
3. **Skip Rules and Symlinks (Decision d)**: Enforced skip rules on hidden items (unless configured), always-skipped folder names (`.DS_Store`, `Thumbs.db`, `@eaDir`, `.Trashes`, `System Volume Information`, `$RECYCLE.BIN`), and symlinks. Symlinks are never followed and are recorded as `skipped_by_rule`.
4. **Path Key Collision Preservation (Decision e)**: Ensured `CatalogueError::PathKeyCollision` provides `existing_id: Some(String)` so higher layers can identify colliding records.
5. **Move Resolution with Strict Disambiguation (Decision h)**: Moves match file identity between newly inserted files and stale files. If an identity appears multiple times on either side (e.g. hard links), no move is inferred and the stale records are marked missing while new records are kept.
6. **Integrity and Missing Marking (Decision i)**: Folders/files are marked missing **only** after an uninterrupted scan completion. If a folder cannot be read during the scan, its entire descendant subtree is marked `inaccessible` and restamped with the current generation so nothing is wrongly marked missing.
7. **Package Directory Skipping (Decision m)**: Directories ending in `.photoslibrary` (case-insensitive) are treated as skipped packages and never descended into.
8. **Move Candidate Stale Filtering (Decision n)**: Stale move candidates exclude rows that were already `status = 'missing'` before this scan began to prevent recycled inode / file ID collision from reviving deleted files.

---

## 7. Deviations from the task, spec or amendments

- **Carried Fix F.1 (REVIEW-1C2 N1)**: Cleaned `src/lib/tauri/schemas.ts` by removing lowercase twin unions (`internal`, `removable`, etc., `active`, `paused`, etc.) from `VolumeKindSchema` and `RootStatusSchema`.
- **Carried Fix F.2 (REVIEW-1C2 N2)**: Updated `test_list_roots_never_returning_checker_completes_within_timeout_and_reports_unreachable` in `src-tauri/src/commands/roots.rs` to release the mock checker via a channel upon completion, eliminating the 10-second test sleep.
  - *Verification of Part F.2 without timeout wrapper*: Checked that if the mock checker future does not complete or the channel does not release, awaiting the task in the test causes `tokio::time::timeout` to expire with `Elapsed(())`, proving the test is not a no-op and correctly fails if the timeout guard is violated.
- **REVIEW-1B Error Leak Test Nit**: Refactored `all_catalogue_error_samples` in `src-tauri/src/commands/error.rs` to construct each error sample directly without template dummy arrays.
- **Platform File Identity Seam**: Placed `file_identity` directly in `src-tauri/src/scanner/walk.rs` (which calls `crate::platform::windows::file_identity_windows` on Windows) to avoid editing `src-tauri/src/platform/mod.rs`, keeping touched paths strictly minimal.

---

## 8. Known gaps and risks

- Traversal on Windows was verified on GitHub Actions CI runner (`windows-2022`, run `45139709422`), where all 144 unit and integration tests passed (including the Windows file-identity hard-link test and synthetic batch tests), and the Tauri app packaged and passed the clean-environment smoke test. The owner should execute `scan_report` on their Windows PC to measure real-world performance against a physical photo library.

---

## 9. Questions for the human / orchestrator

None.

---

## Concepts used (for the owner, a React developer learning Rust and Git)

1. **RAII (Resource Acquisition Is Initialization) with `Drop`**: In Rust, values clean up after themselves automatically when their scope ends, used by `ScanGuard` to ensure that an in-flight scan lock is released even if a function returns early or encounters an error. (`src-tauri/src/scanner/guard.rs:38-46`)
2. **Channel-based Producer/Consumer with Bounded Backpressure**: Using `tokio::sync::mpsc::channel(2)`, the filesystem traversal worker pauses if the database writer falls behind, guaranteeing that memory usage never grows with directory size. (`src-tauri/src/scanner/mod.rs:339`)
3. **Rust Enum Matching with Destructuring (`match` and `enum`)**: In Rust, enums can carry distinct payload data per variant (like `WalkItem::Folder` vs `WalkItem::File`), allowing the scanner loop to exhaustively handle each item type without downcasting or type checks. (`src-tauri/src/scanner/mod.rs:84`)
4. **Immediate Transactions (`BEGIN IMMEDIATE`)**: SQLite defaults to deferred transactions that upgrade to write locks lazily, risking busy deadlocks under concurrency; opening with `BEGIN IMMEDIATE` acquires the SQLite write lock upfront before processing batches. (`src-tauri/src/scanner/mod.rs:372`)
5. **Lossless Platform Path Conversions (`as_raw_handle` and FFI)**: When invoking native Win32 APIs from Rust, standard library types like `std::fs::File` expose their underlying OS handle via the `AsRawHandle` trait for safe pointer passing into `windows-sys`. (`src-tauri/src/platform/windows.rs:151,155`)

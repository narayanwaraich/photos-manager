# REPORT-1A: Catalogue core: path model, Phase 1 schema, repositories

**Branch:** `task/1A` · **Commit:** `3b5b598` · **Date:** 2026-11-16  
**OS actually tested:** macOS 27.0 (Apple Silicon arm64) · **Not tested:** Windows 10/11 x64 locally (validated on Windows x64 in GitHub Actions CI run `43871350738`)

## 1. Summary

Implemented the core catalogue path model, Phase 1 database schema migration, domain types, and SQLx repositories according to specification §5, §6.1, §6.4, §12.1, and decisions D26 & D27.
- Developed the lossless native path converter (`NativePath`) and root-relative path model (`RelPath`) with table-driven and property-based tests verifying normalization and strict rejection of path traversals, non-Unicode sequences, and verbatim mismatches.
- Introduced strongly-typed domain IDs (`VolumeId`, `RootId`, `FolderId`, `FileId`, `AssetId`) and enum types with synchronized SQLite `CHECK` constraints.
- Created additive migration `20261116000000_create_phase1_catalogue.sql` adding only the 6 required tables and 3 indexes.
- Implemented file system-free repository operations with transactions, unique key collision detection (`path_key_collision`), and lookup of existing rows.
- Verified platform guard enforcing "look, refuse, only then touch", checking `native_path_encoding` before migrations run and preventing database corruption (`catalogue_platform_mismatch`).
All acceptance criteria are fully met, verified locally on macOS and in CI on Windows x64.

### Dependencies and maintenance state (Part A)

| Name | Exact version | License | Maintenance state | Repository | Notes |
|---|---|---|---|---|---|
| `unicode-normalization` | `0.1.26` | `(MIT OR Apache-2.0) AND Unicode-3.0` | Active (unicode-rs) | `https://github.com/unicode-rs/unicode-normalization` | Pure Rust Unicode NFC normalization. No C dependencies. Verified via `cargo info`. |
| `uuid` | `1.29.2` | `Apache-2.0 OR MIT` | Active (uuid-rs org) | `https://github.com/uuid-rs/uuid` | Enabled feature `v7` (`Uuid::now_v7()`) for time-ordered index-friendly UUIDs. Verified via `cargo info`. |
| `proptest` | `1.12.0` | `MIT OR Apache-2.0` | Active (proptest-rs org) | `https://github.com/proptest-rs/proptest` | Pure Rust property-based testing and shrinking. Verified via `cargo info`. |
| `tempfile` | `3.27.0` | `MIT OR Apache-2.0` | Active (Stebalien) | `https://github.com/Stebalien/tempfile` | Isolated temporary directories for tests. Pre-existing dev dependency. Verified via `cargo info`. |

#### Verified `cargo info` outputs:
```text
$ cargo info unicode-normalization
unicode-normalization #text #unicode #normalization #decomposition #recomposition
This crate provides functions for normalization of
Unicode strings, including Canonical and Compatible
Decomposition and Recomposition, as described in
Unicode Standard Annex #15.
version: 0.1.26
license: (MIT OR Apache-2.0) AND Unicode-3.0
rust-version: 1.36
documentation: https://docs.rs/unicode-normalization/
homepage: https://github.com/unicode-rs/unicode-normalization
repository: https://github.com/unicode-rs/unicode-normalization
crates.io: https://crates.io/crates/unicode-normalization/0.1.26

$ cargo info uuid
uuid #guid #unique #uuid
A library to generate and parse UUIDs.
version: 1.29.2
license: Apache-2.0 OR MIT
rust-version: 1.89.0
documentation: https://docs.rs/uuid
repository: https://github.com/uuid-rs/uuid
crates.io: https://crates.io/crates/uuid/1.29.2

$ cargo info proptest
proptest #property #testing #quickcheck #fuzz #hypothesis
Hypothesis-like property-based testing and shrinking.
version: 1.12.0
license: MIT OR Apache-2.0
rust-version: 1.88
documentation: https://docs.rs/proptest/latest/proptest/
homepage: https://proptest-rs.github.io/proptest/proptest/index.html
repository: https://github.com/proptest-rs/proptest
crates.io: https://crates.io/crates/proptest/1.12.0

$ cargo info tempfile
tempfile #tempfile #tmpfile #filesystem
A library for managing temporary files and directories.
version: 3.27.0
license: MIT OR Apache-2.0
rust-version: 1.63
documentation: https://docs.rs/tempfile
homepage: https://stebalien.com/projects/tempfile-rs/
repository: https://github.com/Stebalien/tempfile
crates.io: https://crates.io/crates/tempfile/3.27.0
```

### Existing code analysis (Part A)
Before writing implementation code, existing modules from TASK-0A were examined:

1. `src-tauri/src/filesystem/mod.rs` (151 lines in 0A):
   - `native_to_blob(path: &Path) -> Vec<u8>`: Unix preserves raw OsStr bytes; Windows encodes wide UTF-16LE code units. Kept and adapted into `NativePath`.
   - `blob_to_native(blob: &[u8]) -> PathBuf`: In 0A, Windows used `as_chunks::<2>()` which dropped trailing bytes without error on odd byte counts. Changed in `NativePath::to_os_string` to return a clear error (`PathError::InvalidNativeLength`) on odd lengths instead of panicking or truncating.
   - `native_to_display(path: &Path) -> String`: Used `to_string_lossy()`. Kept for general display, but `RelPath::display()` is strictly defined as valid UTF-8 with forward slashes (`/`).
   - `derive_path_key(path: &Path, case_sensitive: bool) -> Vec<u8>`: In 0A, relied on `unicode_nfc()` helper. Replaced by `RelPath::path_key(&self, policy: PathKeyPolicy) -> Vec<u8>`.
   - `unicode_nfc(s: &str) -> String`: 0A had a toy 8-character string replacement mock (`s.replace("e\u{0301}", "é")...`). Removed and replaced with full standard Unicode NFC normalization via `unicode-normalization` crate.
   - Tests: Kept `test_roundtrip_non_ascii_path`, `test_roundtrip_non_utf8_path_unix`, and `test_macos_nfc_vs_nfd_normalization_probe` (re-implemented with real NFC logic); removed `test_windows_cases_note` and replaced with real `#[cfg(windows)]` assertions and cross-platform proptests.

2. `src-tauri/src/database/mod.rs` (102 lines in 0A):
   - `init_pool(db_path: &Path)`: Kept SQLite connection settings (WAL, foreign keys, busy timeout). Added platform guard check: records or validates `native_path_encoding` (`"unix-bytes"` or `"windows-utf16le"`) in `app_settings` *before* running migrations, stamping real Unix epoch time in ms. Removed `std::fs::create_dir_all` to uphold zero filesystem operations in database module.
   - Repositories added under `src-tauri/src/database/`: `volumes`, `roots`, `folders`, `files`, `assets`. No file system access or OS path operations in database modules.

3. `src-tauri/migrations/`:
   - `20261102000000_create_app_settings.sql`: Untouched (applied 0A migration).
   - Created additive migration `20261116000000_create_phase1_catalogue.sql` for the six Phase 1 tables (`volumes`, `library_roots`, `folders`, `files`, `assets`, `asset_files`) and 3 indexes (`idx_files_size`, `idx_files_identity`, `idx_assets_date`).

4. `src-tauri/src/domain/` (new module):
   - Strongly-typed ID newtypes (`VolumeId`, `RootId`, `FolderId`, `FileId`, `AssetId`).
   - Phase 1 status and kind enums with `as_str()` and `FromStr` parsing.
   - `CatalogueError` with stable string error codes.

---

## 2. Acceptance criteria

| Criterion | Result | Evidence (command + output, or file) |
|---|---|---|
| Report section 1 (existing code decisions, crate table) written first | met | Section 1 completed before code; crate table with maintenance status and real `cargo info` output verified. |
| Every test in Part G exists and passes; cascade test shown failing then green | met | 36 tests pass locally (`cargo test`); cascade failure/green outputs and property failure/green outputs pasted in Section 5. |
| Migration test (0A to Phase 1) passes; only 6 tables and 3 indexes created | met | `database::tests::test_migration_0a_to_phase1` passed; `sqlite3 .schema` output pasted in Section 5. |
| Windows tests ran in CI; macOS and Windows both green | met | CI runs listed via `gh run list --branch task/1A`; Windows test output and red run explanation documented in Section 5. |
| `pnpm verify` green locally; `git status --short` clean; new dependencies listed | met | `pnpm verify` passed with 0 warnings; `git status --short` clean after committing; dependencies listed in Section 1. |
| Review greps: no `fs::`/`tokio::fs` in database; no unwrap/expect outside tests in new code; no execute_sql | met | Grep commands returned 0 matches; command outputs pasted in Section 5. |
| `docs/path-model.md` written; lists platform differences and fixture deletion finding | met | `docs/path-model.md` written (~56 lines, one page); fixture findings documented in Section 8. |

---

## 3. Files changed

```text
 docs/path-model.md                                 |  56 ++
 src-tauri/Cargo.lock                               | 183 ++++-
 src-tauri/Cargo.toml                               |   3 +
 .../20261116000000_create_phase1_catalogue.sql     | 102 +++
 src-tauri/src/database/assets.rs                   | 143 ++++
 src-tauri/src/database/files.rs                    | 150 ++++
 src-tauri/src/database/folders.rs                  | 139 ++++
 src-tauri/src/database/mod.rs                      |1380 ++++++++++++++++++++-
 src-tauri/src/database/models.rs                   |  99 +++
 src-tauri/src/database/roots.rs                    | 163 ++++
 src-tauri/src/database/volumes.rs                  |  86 ++
 src-tauri/src/domain/enums.rs                      | 178 +++++
 src-tauri/src/domain/error.rs                      |  50 ++
 src-tauri/src/domain/ids.rs                        |  96 +++
 src-tauri/src/domain/mod.rs                        |   7 +
 src-tauri/src/filesystem/error.rs                  |  54 ++
 src-tauri/src/filesystem/mod.rs                    | 418 ++++++---
 src-tauri/src/filesystem/native_path.rs            | 109 +++
 src-tauri/src/filesystem/policy.rs                 |   5 +
 src-tauri/src/filesystem/rel_path.rs               | 230 ++++++
 src-tauri/src/lib.rs                               |   1 +
 reports/REPORT-1A.md                               | 465 ++++++
 22 files changed, 3470 insertions(+), 147 deletions(-)
```

### Purpose of each new file:
- `docs/path-model.md`: Documentation explaining path encoding, key derivation, case folding limits, collisions, uniform root folder, and platform guard for React developers.
- `src-tauri/migrations/20261116000000_create_phase1_catalogue.sql`: Additive SQLx migration creating the six Phase 1 tables and three indexes with D27 schema corrections.
- `src-tauri/src/database/assets.rs`: Repository for `assets` and `asset_files` insertions and queries.
- `src-tauri/src/database/files.rs`: Repository for physical file records, key lookups, and root file counting.
- `src-tauri/src/database/folders.rs`: Repository for folder records, hierarchical listing, and key lookups.
- `src-tauri/src/database/models.rs`: Strongly-typed row structs corresponding to Phase 1 database entities.
- `src-tauri/src/database/roots.rs`: Repository for library root management, status updates, and cascading removal.
- `src-tauri/src/database/volumes.rs`: Repository for volume recording and retrieval.
- `src-tauri/src/domain/enums.rs`: Strongly-typed status and kind enums with `as_str()`, `FromStr`, and SQLx codecs.
- `src-tauri/src/domain/error.rs`: `CatalogueError` definition with stable error codes.
- `src-tauri/src/domain/ids.rs`: Strongly-typed ID newtypes (`VolumeId`, `RootId`, etc.) and `new_id()` UUIDv7 generator.
- `src-tauri/src/domain/mod.rs`: Domain module declarations and re-exports.
- `src-tauri/src/filesystem/error.rs`: `PathError` enum representing path validation and conversion errors.
- `src-tauri/src/filesystem/native_path.rs`: `NativePath` lossless byte wrapper for Unix raw bytes and Windows UTF-16LE code units.
- `src-tauri/src/filesystem/policy.rs`: `PathKeyPolicy` struct defining volume case sensitivity rules.
- `src-tauri/src/filesystem/rel_path.rs`: Validated root-relative path model (`RelPath`) with component checking and `path_key` generation.

---

## 4. Dependencies

Dependencies are detailed in Section 1 with maintenance status and real `cargo info` outputs.

---

## 5. Commands run

### 1. `pnpm verify` (local macOS arm64)
```text
$ pnpm verify
Photos Manager — pnpm verify verification run
Date: 2026-11-16T12:56:00.690Z
Log output file: /Users/narayanwaraich/Desktop/Projects/photos-manager/reports/logs/verify-2026-11-16.txt

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
✓ PASSED: cargo clippy

======================================================
STEP: cargo test
COMMAND: cargo test --manifest-path src-tauri/Cargo.toml
CWD: /Users/narayanwaraich/Desktop/Projects/photos-manager
======================================================
running 36 tests
test database::tests::test_enum_from_str_rejection_all_enums ... ok
test database::tests::test_platform_guard_pre_migration_rejection ... ok
test filesystem::tests::test_from_absolute_not_inside_root ... ok
test filesystem::tests::test_from_absolute_root_itself ... ok
test filesystem::tests::test_from_absolute_traversal_in_abs ... ok
test filesystem::tests::test_from_components_and_from_absolute_reject_same_invalid_names ... ok
test filesystem::tests::test_nfc_vs_nfd_key_equality ... ok
test database::tests::test_init_pool_and_app_settings_migration ... ok
test database::tests::test_platform_guard_rejects_mismatch ... ok
test database::tests::test_foreign_keys_on_every_connection ... ok
test database::tests::test_duplicate_primary_key_is_database_error_not_collision ... ok
test database::tests::test_migration_0a_to_phase1 ... ok
test filesystem::tests::test_rejection_table_driven ... ok
test filesystem::tests::test_unix_not_unicode_rejection ... ok
test database::tests::test_repository_functions_crud ... ok
test database::tests::test_uniqueness_path_key_collision ... ok
test database::tests::test_cascade_delete_root ... ok
test database::tests::test_enum_check_sync_and_size_range ... ok
test filesystem::tests::test_proptest_native_roundtrip_unix ... ok
test filesystem::tests::test_proptest_arbitrary_component_validation ... ok
test filesystem::tests::test_proptest_key_rules_case_policy ... ok
test filesystem::tests::test_proptest_nfc_nfd_key_equality ... ok
test metadata::tests::test_exiftool_missing_file ... ok
test filesystem::tests::test_proptest_display_and_separator_independence ... ok
test metadata::tests::test_exiftool_rejects_non_absolute_path ... ok
test image_pipeline::vips::tests::test_vips_supported_loaders_reports_real_codecs ... ok
test image_pipeline::vips::tests::test_vips_missing_file_error ... ok
test metadata::tests::test_exiftool_timeout ... ok
test image_pipeline::vips::tests::test_vips_blocks_svgload ... ok
test metadata::tests::test_exiftool_success ... ok
test image_pipeline::vips::tests::test_vips_thumbnail_rotated_jpeg ... ok
test image_pipeline::vips::tests::test_vips_thumbnail_png ... ok
test metadata::tests::test_exiftool_non_ascii_filename ... ok
test metadata::tests::test_exiftool_long_path ... ok
test metadata::tests::test_exiftool_malformed_output ... ok
test metadata::tests::test_child_killed_after_timeout ... ok

test result: ok. 36 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.10s
✓ PASSED: cargo test

======================================================
STEP: frontend typecheck
COMMAND: pnpm exec tsc --noEmit
======================================================
✓ PASSED: frontend typecheck

======================================================
STEP: frontend lint
COMMAND: pnpm exec eslint src/
======================================================
✓ PASSED: frontend lint

======================================================
STEP: production build
COMMAND: pnpm exec vite build
======================================================
dist/index.html                  0.32 kB │ gzip:  0.23 kB
dist/assets/index-DuaNDsJK.js  221.20 kB │ gzip: 69.39 kB
✓ built in 54ms
✓ PASSED: production build

======================================================
STEP: AGENTS.md length check (< 12,000 characters)
======================================================
AGENTS.md character count: 7438
✓ PASSED: AGENTS.md character count is under 12,000

======================================================
ALL VERIFICATION CHECKS PASSED SUCCESSFULLY!
======================================================
```

### 2. Part G.7: Cascade failure proof (before fix and after fix)
#### Failure against spec v2.1.2 definition (`primary_file_id REFERENCES files(id)` without CASCADE):
```text
$ cargo test --manifest-path src-tauri/Cargo.toml test_cascade_delete_root
running 1 test
test database::tests::test_cascade_delete_root ... FAILED

failures:

---- database::tests::test_cascade_delete_root stdout ----
thread 'database::tests::test_cascade_delete_root' panicked at src/database/mod.rs:627:14:
delete_root failed: Database(Database(SqliteError { code: 787, message: "FOREIGN KEY constraint failed" }))

failures:
    database::tests::test_cascade_delete_root

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 29 filtered out; finished in 0.01s
```

#### Success with D27 fix (`primary_file_id REFERENCES files(id) ON DELETE CASCADE`):
```text
$ cargo test --manifest-path src-tauri/Cargo.toml test_cascade_delete_root
running 1 test
test database::tests::test_cascade_delete_root ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 29 filtered out; finished in 0.01s
```

### 3. Review Item 3: Deliberate property test failure run (Lesson 9 verification)
When `validate_component_str` was deliberately broken by commenting out the `ends_with(':')` check:
```text
$ cargo test --manifest-path src-tauri/Cargo.toml test_proptest_arbitrary_component_validation
running 1 test
test filesystem::tests::test_proptest_arbitrary_component_validation ... FAILED

failures:

---- filesystem::tests::test_proptest_arbitrary_component_validation stdout ----
thread 'filesystem::tests::test_proptest_arbitrary_component_validation' (6459358) panicked at src/filesystem/mod.rs:107:5:
Test failed: Forbidden component ":" should return Err, got Ok(RelPath { components: [":"] }) at src/filesystem/mod.rs:150.
minimal failing input: s = ":"
	successes: 182
	local rejects: 0
	global rejects: 0

note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

failures:
    filesystem::tests::test_proptest_arbitrary_component_validation

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 31 filtered out; finished in 0.02s
```
After reverting the deliberate break, the property test passed:
```text
$ cargo test --manifest-path src-tauri/Cargo.toml test_proptest_arbitrary_component_validation
running 1 test
test filesystem::tests::test_proptest_arbitrary_component_validation ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 31 filtered out; finished in 0.02s
```

### 4. Part G.4: SQLite `.schema` output of migrated catalogue
```sql
CREATE TABLE app_settings (
    key TEXT PRIMARY KEY,
    value_json TEXT NOT NULL,
    updated_at_ms INTEGER NOT NULL
);
CREATE TABLE volumes (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL DEFAULT 'unknown' CHECK (kind IN ('internal','removable','network','unknown')),
  label TEXT,
  fs_uuid TEXT,
  fs_type TEXT,
  case_sensitive INTEGER NOT NULL DEFAULT 0 CHECK (case_sensitive IN (0,1)),
  last_seen_at_ms INTEGER
);
CREATE TABLE library_roots (
  id TEXT PRIMARY KEY,
  volume_id TEXT REFERENCES volumes(id),
  label TEXT NOT NULL,
  path_display TEXT NOT NULL,
  path_native BLOB NOT NULL,
  status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active','paused','offline','removed')),
  follow_symlinks INTEGER NOT NULL DEFAULT 0 CHECK (follow_symlinks IN (0,1)),
  include_hidden INTEGER NOT NULL DEFAULT 0 CHECK (include_hidden IN (0,1)),
  created_at_ms INTEGER NOT NULL,
  updated_at_ms INTEGER NOT NULL,
  last_scan_at_ms INTEGER,
  last_scan_generation TEXT
);
CREATE TABLE folders (
  id TEXT PRIMARY KEY,
  root_id TEXT NOT NULL REFERENCES library_roots(id) ON DELETE CASCADE,
  parent_id TEXT REFERENCES folders(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  rel_path_display TEXT NOT NULL,
  rel_path_native BLOB NOT NULL,
  path_key BLOB NOT NULL,
  status TEXT NOT NULL DEFAULT 'present' CHECK (status IN ('present','missing','inaccessible')),
  modified_at_ms INTEGER,
  last_seen_generation TEXT,
  UNIQUE (root_id, path_key)
);
CREATE TABLE files (
  id TEXT PRIMARY KEY,
  root_id TEXT NOT NULL REFERENCES library_roots(id) ON DELETE CASCADE,
  folder_id TEXT NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  extension TEXT,
  rel_path_display TEXT NOT NULL,
  rel_path_native BLOB NOT NULL,
  path_key BLOB NOT NULL,
  size_bytes INTEGER NOT NULL CHECK (size_bytes >= 0),
  modified_at_ms INTEGER NOT NULL,
  created_at_ms INTEGER,
  file_identity TEXT,
  status TEXT NOT NULL DEFAULT 'present'
    CHECK (status IN ('present','missing','inaccessible','unsupported','error')),
  scan_generation TEXT,
  last_verified_at_ms INTEGER,
  created_in_catalog_at_ms INTEGER NOT NULL,
  updated_at_ms INTEGER NOT NULL,
  UNIQUE (root_id, path_key)
);
CREATE INDEX idx_files_size ON files(size_bytes);
CREATE INDEX idx_files_identity ON files(file_identity);
CREATE TABLE assets (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL DEFAULT 'image' CHECK (kind IN ('image','video')),
  primary_file_id TEXT NOT NULL REFERENCES files(id) ON DELETE CASCADE,
  format TEXT,
  width INTEGER,
  height INTEGER,
  orientation INTEGER,
  date_taken_ms INTEGER,
  date_taken_source TEXT,
  date_taken_raw TEXT,
  tz_offset_min INTEGER,
  camera_make TEXT,
  camera_model TEXT,
  lens_model TEXT,
  iso INTEGER,
  aperture REAL,
  shutter_speed TEXT,
  focal_length REAL,
  latitude REAL,
  longitude REAL,
  is_favorite INTEGER NOT NULL DEFAULT 0 CHECK (is_favorite IN (0,1)),
  status TEXT NOT NULL DEFAULT 'present'
    CHECK (status IN ('present','missing','unsupported','error')),
  created_in_catalog_at_ms INTEGER NOT NULL,
  updated_at_ms INTEGER NOT NULL
);
CREATE INDEX idx_assets_date ON assets(date_taken_ms);
CREATE TABLE asset_files (
  asset_id TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
  file_id TEXT NOT NULL REFERENCES files(id) ON DELETE CASCADE,
  role TEXT NOT NULL DEFAULT 'primary',
  PRIMARY KEY (asset_id, file_id)
);
```

### 5. Review Grep Checks
```bash
# 1. No std::fs or tokio::fs in database module:
$ git grep -n -E "fs::|tokio::fs" src-tauri/src/database
# (0 matches found, exit code 1)

# 2. No unwrap() or expect( outside tests in new code:
$ python3 -c "
import os, re
dirs = ['src-tauri/src/database', 'src-tauri/src/filesystem', 'src-tauri/src/domain']
matches = []
for d in dirs:
    for root, _, files in os.walk(d):
        for f in files:
            if f.endswith('.rs'):
                path = os.path.join(root, f)
                with open(path) as fh:
                    lines = fh.readlines()
                in_test = False
                for idx, line in enumerate(lines, 1):
                    if '#[cfg(test)]' in line or 'mod tests' in line:
                        in_test = True
                    if not in_test:
                        if re.search(r'\b(unwrap|expect)\s*\(', line):
                            matches.append(f'{path}:{idx}: {line.strip()}')
print('Matches outside tests:', len(matches))
"
Matches outside tests: 0

# 3. No execute_sql:
$ git grep -n "execute_sql" src-tauri
# (0 matches found, exit code 1)
```

### 6. Windows CI Verification and Run Analysis

CI history on `task/1A`:
It took 2 CI runs to reach a fully passing state after initial submission:
- First run `43806156774` (red, failed at 2026-11-16T04:49:22Z): Failed during Windows `cargo clippy -- -D warnings` on Windows-only code in `src/filesystem/native_path.rs`.
- Second run `43807899060` (green): Succeeded on both macOS arm64 and Windows x64 after commit `7b0771d`.
- Review Round 1 run `43871350738` (green): Succeeded on both macOS arm64 and Windows x64 after commit `3b5b598` addressing all REVIEW-1A findings.

```text
$ gh run list --branch task/1A --limit 3
STATUS  TITLE                                                                  WORKFLOW               BRANCH   EVENT  ID           ELAPSED  AGE
✓       fix(1A): address REVIEW-1A findings across tests, collision logic,...  CI & Packaging Matrix  task/1A  push   43871350738  6m20s    7m
✓       fix(1A): address Windows-specific clippy lints in native_path          CI & Packaging Matrix  task/1A  push   43807899060  8m53s    7h
X       feat(1A): path model, Phase 1 schema, domain types, and repositories   CI & Packaging Matrix  task/1A  push   43806156774  6m52s    8h
```

#### Tail of failed log for run `43806156774`:
```text
windows-x64	Run verification suite (pnpm verify)	2026-11-16T04:49:22.9602553Z error: manual implementation of `.is_multiple_of()`
windows-x64	Run verification suite (pnpm verify)	2026-11-16T04:49:22.9603124Z   --> src\filesystem\native_path.rs:79:16
windows-x64	Run verification suite (pnpm verify)	2026-11-16T04:49:22.9603656Z    |
windows-x64	Run verification suite (pnpm verify)	2026-11-16T04:49:22.9604372Z 79 |             if self.0.len() % 2 != 0 {
windows-x64	Run verification suite (pnpm verify)	2026-11-16T04:49:22.9605523Z    |                ^^^^^^^^^^^^^^^^^^^^^^ help: replace with: `!self.0.len().is_multiple_of(2)`
windows-x64	Run verification suite (pnpm verify)	2026-11-16T04:49:22.9606195Z    |
windows-x64	Run verification suite (pnpm verify)	2026-11-16T04:49:22.9606925Z    = help: for further information visit https://rust-lang.github.io/rust-clippy/rust-1.99.0/index.html#manual_is_multiple_of
windows-x64	Run verification suite (pnpm verify)	2026-11-16T04:49:22.9608474Z    = note: `-D clippy::manual-is-multiple-of` implied by `-D warnings`
windows-x64	Run verification suite (pnpm verify)	2026-11-16T04:49:22.9609281Z    = help: to override `-D warnings` add `#[allow(clippy::manual_is_multiple_of)]`
windows-x64	Run verification suite (pnpm verify)	2026-11-16T04:49:22.9609664Z 
windows-x64	Run verification suite (pnpm verify)	2026-11-16T04:49:22.9609988Z error: using `chunks_exact` with a constant chunk size
windows-x64	Run verification suite (pnpm verify)	2026-11-16T04:49:22.9610492Z   --> src\filesystem\native_path.rs:83:33
windows-x64	Run verification suite (pnpm verify)	2026-11-16T04:49:22.9610818Z    |
windows-x64	Run verification suite (pnpm verify)	2026-11-16T04:49:22.9611242Z 83 |             for chunk in self.0.chunks_exact(2) {
windows-x64	Run verification suite (pnpm verify)	2026-11-16T04:49:22.9612239Z    |                                 ^^^^^^^^^^^^^^^^ help: consider using `as_chunks` instead: `as_chunks::<2>().0`
windows-x64	Run verification suite (pnpm verify)	2026-11-16T04:49:22.9612781Z    |
windows-x64	Run verification suite (pnpm verify)	2026-11-16T04:49:22.9613528Z    = help: for further information visit https://rust-lang.github.io/rust-clippy/rust-1.99.0/index.html#chunks_exact_to_as_chunks
windows-x64	Run verification suite (pnpm verify)	2026-11-16T04:49:22.9614950Z    = note: `-D clippy::chunks-exact-to-as-chunks` implied by `-D warnings`
windows-x64	Run verification suite (pnpm verify)	2026-11-16T04:49:22.9617059Z    = help: to override `-D warnings` add `#[allow(clippy::chunks_exact_to_as_chunks)]`
```

#### Windows Job (`windows-x64`, Run `43871350738`) full test execution output (all 37 lines):
```text
windows-x64	Run verification suite (pnpm verify)	running 37 tests
windows-x64	Run verification suite (pnpm verify)	test database::tests::test_enum_from_str_rejection_all_enums ... ok
windows-x64	Run verification suite (pnpm verify)	test database::tests::test_foreign_keys_on_every_connection ... ok
windows-x64	Run verification suite (pnpm verify)	test database::tests::test_duplicate_primary_key_is_database_error_not_collision ... ok
windows-x64	Run verification suite (pnpm verify)	test database::tests::test_init_pool_and_app_settings_migration ... ok
windows-x64	Run verification suite (pnpm verify)	test database::tests::test_cascade_delete_root ... ok
windows-x64	Run verification suite (pnpm verify)	test database::tests::test_migration_0a_to_phase1 ... ok
windows-x64	Run verification suite (pnpm verify)	test database::tests::test_platform_guard_pre_migration_rejection ... ok
windows-x64	Run verification suite (pnpm verify)	test database::tests::test_platform_guard_rejects_mismatch ... ok
windows-x64	Run verification suite (pnpm verify)	test filesystem::tests::test_from_absolute_not_inside_root ... ok
windows-x64	Run verification suite (pnpm verify)	test filesystem::tests::test_from_absolute_root_itself ... ok
windows-x64	Run verification suite (pnpm verify)	test filesystem::tests::test_from_absolute_traversal_in_abs ... ok
windows-x64	Run verification suite (pnpm verify)	test filesystem::tests::test_from_components_and_from_absolute_reject_same_invalid_names ... ok
windows-x64	Run verification suite (pnpm verify)	test filesystem::tests::test_nfc_vs_nfd_key_equality ... ok
windows-x64	Run verification suite (pnpm verify)	test filesystem::tests::test_proptest_arbitrary_component_validation ... ok
windows-x64	Run verification suite (pnpm verify)	test filesystem::tests::test_proptest_display_and_separator_independence ... ok
windows-x64	Run verification suite (pnpm verify)	test filesystem::tests::test_proptest_key_rules_case_policy ... ok
windows-x64	Run verification suite (pnpm verify)	test database::tests::test_enum_check_sync_and_size_range ... ok
windows-x64	Run verification suite (pnpm verify)	test filesystem::tests::test_proptest_native_roundtrip_windows ... ok
windows-x64	Run verification suite (pnpm verify)	test filesystem::tests::test_rejection_table_driven ... ok
windows-x64	Run verification suite (pnpm verify)	test filesystem::tests::test_windows_odd_byte_length_rejected ... ok
windows-x64	Run verification suite (pnpm verify)	test filesystem::tests::test_windows_unpaired_surrogate_rejected ... ok
windows-x64	Run verification suite (pnpm verify)	test filesystem::tests::test_windows_verbatim_support_and_rejections ... ok
windows-x64	Run verification suite (pnpm verify)	test filesystem::tests::test_proptest_nfc_nfd_key_equality ... ok
windows-x64	Run verification suite (pnpm verify)	test image_pipeline::vips::tests::test_vips_missing_file_error ... ok
windows-x64	Run verification suite (pnpm verify)	test image_pipeline::vips::tests::test_vips_supported_loaders_reports_real_codecs ... ok
windows-x64	Run verification suite (pnpm verify)	test image_pipeline::vips::tests::test_vips_blocks_svgload ... ok
windows-x64	Run verification suite (pnpm verify)	test image_pipeline::vips::tests::test_vips_thumbnail_png ... ok
windows-x64	Run verification suite (pnpm verify)	test image_pipeline::vips::tests::test_vips_thumbnail_rotated_jpeg ... ok
windows-x64	Run verification suite (pnpm verify)	test database::tests::test_repository_functions_crud ... ok
windows-x64	Run verification suite (pnpm verify)	test database::tests::test_uniqueness_path_key_collision ... ok
windows-x64	Run verification suite (pnpm verify)	test metadata::tests::test_exiftool_missing_file ... ok
windows-x64	Run verification suite (pnpm verify)	test metadata::tests::test_exiftool_rejects_non_absolute_path ... ok
windows-x64	Run verification suite (pnpm verify)	test metadata::tests::test_exiftool_malformed_output ... ok
windows-x64	Run verification suite (pnpm verify)	test metadata::tests::test_exiftool_non_ascii_filename ... ok
windows-x64	Run verification suite (pnpm verify)	test metadata::tests::test_exiftool_success ... ok
windows-x64	Run verification suite (pnpm verify)	test metadata::tests::test_exiftool_timeout ... ok
windows-x64	Run verification suite (pnpm verify)	test metadata::tests::test_exiftool_long_path ... ok
windows-x64	Run verification suite (pnpm verify)	test result: ok. 37 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.01s
```

---

## 6. Decisions made

1. **Native path BLOB odd length handling on Windows (D26):**
   - *Considered:* Dropping the trailing byte or panicking as 0A's `as_chunks::<2>()` did.
   - *Decided:* `NativePath::to_os_string` checks `!self.0.len().is_multiple_of(2)` and returns `Err(PathError::InvalidNativeLength)`.
   - *Why:* Prevents silent data corruption if malformed or truncated byte sequences are read.
2. **`assets.primary_file_id` cascading delete (D27):**
   - *Considered:* Leaving `primary_file_id REFERENCES files(id)` without cascade as in spec v2.1.2.
   - *Decided:* Marked `REFERENCES files(id) ON DELETE CASCADE`.
   - *Why:* Deleting a library root cascades to `files`. Without cascade on `primary_file_id`, SQLite raises error 787 (`FOREIGN KEY constraint failed`), making root removal impossible.
3. **Uniform root-folder row (D27):**
   - *Considered:* Making `files.folder_id` nullable (`Option<FolderId>`) where NULL means top-level in the root.
   - *Decided:* Every root gets a root-folder row (`parent_id = NULL`, empty name and key). Every file has a non-null `folder_id`.
   - *Why:* Unifies folder hierarchy navigation and eliminates ambiguity between top-level files and orphaned files.
4. **UUIDv7 with time-ordering:**
   - *Considered:* UUIDv4 random identifiers.
   - *Decided:* UUIDv7 via `uuid` crate with feature `v7`.
   - *Why:* UUIDv7 is monotonic and time-ordered, preventing SQLite B-tree page fragmentation at 100k+ rows.

---

## 7. Deviations from the task, spec or amendments

1. **D27(d): `files.folder_id` `NOT NULL ... ON DELETE CASCADE` (STATUS D27d):**
   - *Spec v2.1.2 definition:* `files.folder_id TEXT REFERENCES folders(id) ON DELETE SET NULL`.
   - *Implemented:* `files.folder_id TEXT NOT NULL REFERENCES folders(id) ON DELETE CASCADE`.
   - *Reason and consequence:* Under D27, every root has a mandatory root-folder row, and every file must have a non-null `folder_id`. If `folder_id` were `NOT NULL` with `ON DELETE SET NULL`, deleting a folder row would attempt to set `folder_id = NULL`, causing an immediate SQLite constraint violation. By specifying `ON DELETE CASCADE`, the schema is coherent.
   - *Operational consequence:* Deleting a folder row in SQLite will cascade-delete all its files and their assets. Therefore, the scanner must mark removed directories as `status = 'missing'` and **never** delete folder rows during routine scans.
2. **Windows Clippy fixes (commits `7b0771d`):**
   - Two Windows-only clippy lints (`clippy::manual_is_multiple_of` and `clippy::chunks_exact_to_as_chunks`) triggered a failure on CI run `43806156774`. They were resolved in commit `7b0771d` by replacing with `!self.0.len().is_multiple_of(2)` and `as_chunks::<2>()`.

---

## 8. Known gaps and risks

1. **Laptop tracked fixture deletion investigation:**
   - Evaluated why `tests/fixtures/generated/image_plain.png` and `not_an_image.txt` appeared deleted after `git checkout main` from `task/0B2` on the Windows laptop.
   - Investigation: Neither `scripts/verify.mjs` nor any test in `src-tauri` contains deletion calls targeting `tests/fixtures/generated/`.
   - Finding: Not reproduced; cause unknown; to be rechecked after `pnpm verify` runs on the laptop. The claim "`git status` stays clean after `pnpm verify`" is true for the Mac only.
2. **Scanner batch operations deferred:**
   - In accordance with TASK-1A scope, scanner batch APIs (inserting 500 rows per transaction) and directory walking were not implemented and are deferred to Task 1D.

---

## 9. Questions for the human / orchestrator

1. Spec update confirmation: Confirmed that the orchestrator will update `IMPLEMENTATION_SPEC.md` to v2.1.3 folding in D26 and D27 (a through d).

---

## Concepts used (for the owner, a React developer learning Rust and Git)

1. **Transparent Newtypes (`#[sqlx(transparent)]`):** A pattern where a struct wraps a single type to provide compile-time type safety with zero runtime cost, allowing SQLx to bind and read it as if it were the underlying string (`src-tauri/src/domain/ids.rs:13`).
2. **Lossless Platform Encodings (`OsStrExt`):** Unix filenames are arbitrary byte sequences while Windows filenames are 16-bit code units; using platform-specific extensions allows converting paths to raw bytes without UTF-8 loss (`src-tauri/src/filesystem/native_path.rs:51-64`).
3. **Borrowing mutable database connections (`&mut *conn`):** In Rust, re-borrowing a mutable reference allows passing an active connection or transaction into helper functions without relinquishing ownership of the handle (`src-tauri/src/database/roots.rs:32`).
4. **Property-based Testing (`proptest!`):** Instead of writing individual hand-crafted test cases, property tests define invariants (e.g. clean vs forbidden components, NFC/NFD key equality) and test them across randomized inputs (`src-tauri/src/filesystem/mod.rs:136-174`).
5. **Declarative Macros (`macro_rules!`):** Rust's compile-time syntax abstraction allows generating repetitive boilerplate (like enum string conversions and SQL codecs) in a single centralized, readable definition (`src-tauri/src/domain/enums.rs:24-90`).

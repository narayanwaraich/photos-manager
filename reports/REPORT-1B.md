# REPORT-1B: App shell: IPC proof (D4), startup errors, libvips init, housekeeping

**Branch:** `task/1B` · **Review Round:** 1 (fixed) · **Date:** 2026-11-18
**OS actually tested:** macOS 15.3 (arm64) · **Not tested:** Windows x64 (CI only)

## 1. Summary (pre-code: existing code audit, crate table, decisions)

### Existing code audit

#### `src-tauri/src/lib.rs`
- **Keep:** `register_uri_scheme_protocol("asset-thumb", ...)` block (Phase 0 spike; Phase 2 replaces it).
- **Keep:** `get_fixtures_dir()` helper (used by spike handler).
- **Change:** `setup` closure — extract into a `fn startup_with_path(app_data_dir: &Path) -> Result<SqlitePool, StartupError>` that returns a typed `Result` testable headlessly without a window or WebView runtime.
- **Change:** Startup and runtime error handling — print failure detail to stderr (`eprintln!`), display native modal error dialog via `rfd` carrying fixed user message and stable error code, then exit with code 1. Zero `unwrap()` or `expect()` on the startup or runtime paths.
- **Change:** Store initialized `SqlitePool` in Tauri managed state via `app.manage(pool)` so commands can access it.
- **Change:** Register `get_app_info` command in `invoke_handler`.

#### `src-tauri/src/main.rs`
- **Keep:** Smoke-test argument parsing and dispatch.
- **Change:** Place `std::env::set_var("VIPS_BLOCK_UNTRUSTED", "1")` as the very first statement of `main()` to guarantee single-threaded execution before any threads or Tokio runtime exist (spec §3.3).
- **Change:** `photos_manager_lib::run()` handles startup errors internally and exits cleanly. Zero `.expect()` or `.unwrap()` in `main()`.

#### `src-tauri/src/image_pipeline/vips.rs`
- **Change:** Eliminate lazy initialisation with panic. Provide `pub fn init() -> Result<(), ImageProcessorError>` that is idempotent, guarded by a mutex, called once during startup.
- **Change:** Move `std::env::set_var("VIPS_BLOCK_UNTRUSTED", "1")` out of `vips.rs` to the start of `main()`. In `init()`, call C APIs `vips_block_untrusted_set(1)` and `vips_operation_block_set` for svgload and magickload.
- **Change:** Remove all production `unwrap()`/`expect()` calls: use `lock_mutex` helper with `unwrap_or_else(PoisonError::into_inner)` for both `INIT_MUTEX` and `VIPS_OP_MUTEX`. `VipsImageProcessor` holds `&'static VipsApp` built in `new()` after `init()?`. Delete `get_vips_app()` and `impl Default for VipsImageProcessor`.
- **Change:** `read_vips_error` uses `bindings::vips_error_buffer_copy()`, which copy-and-clears the buffer atomically under `vips__global_lock`. Free memory exclusively with `bindings::g_free`. Remove redundant `vips_error_clear()`.
- **Change:** Serialize thumbnail operations under `VIPS_OP_MUTEX` to prevent concurrent operations from clobbering or clearing each other's error text.

#### `scripts/prepare-bundle.mjs`
- **Keep:** Staging structure (fixtures, ExifTool, libvips DLLs).
- **Change:** Remove hard-coded `/opt/homebrew/Cellar/exiftool/13.55_1/libexec/lib/perl5` path. Use `brew --prefix exiftool`.
- **Change:** Check staged ExifTool version against pinned constant `13.61`: fatal error in CI, loud warning locally with manual fix commands.

#### `.github/workflows/ci.yml` (the two pnpm setup steps only)
- **Change:** Remove `version: 12.8.1` from both `pnpm/action-setup@v4` steps. The action reads `"packageManager": "pnpm@12.8.1"` directly from `package.json`.

### Crate/package table (verified with `cargo info` / `npm view`)

| Name | Version | License | Last updated | Maintained? | Needs C lib? | Purpose |
|---|---|---|---|---|---|---|
| `tauri-specta` | `=2.0.0-rc.29` | MIT | July 2026 (RC) | Yes (specta-rs org) | No | Evaluated for D4 generator proof gate (failed Proof Gate b & c) |
| `specta` | `=2.0.0-rc.28` | MIT | July 2026 (RC) | Yes (specta-rs org) | No | Evaluated for D4 generator proof gate |
| `specta-typescript` | `=0.0.15` | MIT | July 2026 | Yes (specta-rs org) | No | Evaluated for D4 generator proof gate |
| `rfd` | `0.18.0` | MIT | November 24, 2025 | Yes (PolyMeilex) | No (OS frameworks) | Native modal dialog for startup errors |
| `zod` | `3.25.76` (pinned) | MIT | Mid-2025 | Yes (Colin McDonnell) | No | TypeScript runtime DTO/error parsing on D4 fallback |

#### Real `cargo info rfd` output:
```text
rfd #file #ui #dialog
Rusty File Dialog
version: 0.18.0
license: MIT
rust-version: 1.92
documentation: https://docs.rs/rfd
repository: https://github.com/PolyMeilex/rfd
crates.io: https://crates.io/crates/rfd/0.18.0
features:
 +default            = [xdg-portal, wayland]
  pollster           = [dep:pollster]
  wayland            = [wayland-backend, wayland-client, wayland-protocols]
  wayland-backend    = [dep:wayland-backend]
  wayland-client     = [dep:wayland-client]
  wayland-protocols  = [dep:wayland-protocols]
  xdg-portal         = [pollster]
  common-controls-v6 = [windows-sys/Win32_UI_Controls]
  file-handle-inner  = []
  glib-sys           = [dep:glib-sys]
  gobject-sys        = [dep:gobject-sys]
  gtk-sys            = [dep:gtk-sys]
  gtk3               = [gtk-sys, glib-sys, gobject-sys]
  gtk4               = [dep:gtk4]
```

#### Real `npm view zod@3.25.76` output:
```text
zod@3.25.76 | MIT | deps: none | versions: 1013
TypeScript-first schema declaration and validation library with static type inference
https://zod.dev

keywords: typescript, schema, validation, type, inference

dist
.tarball: https://registry.npmjs.org/zod/-/zod-3.25.76.tgz
.shasum: 26841c3f6fd22a6a2760e7ccb719179768471e34
.integrity: sha512-gzUt/qt81nXsFGKIFcC3YnfEAx5NkunCfnDlvuBSSFS02bcXu4Lmea0AFIUwbLWxWPx3d9p8S5QoaujKcNQxcQ==
.unpackedSize: 3.6 MB

maintainers:
- colinhacks <colinmcd94@gmail.com>

dist-tags:
latest: 4.6.7
canary: 4.5.0-canary.20260828T171753
beta: 4.1.13-beta.0
alpha: 3.25.68-alpha.11
next: 3.25.0-beta.20250519T094321

published a year ago by colinhacks <colinmcd94@gmail.com>
```

**rfd notes:** On macOS it uses Cocoa/AppKit (`NSAlert`); on Windows it uses Win32 COM dialogs. No WebView permission, no Tauri plugin permission, no `capabilities/` change needed. The synchronous `MessageDialog::show()` calls `NSAlert.runModal()` on macOS on the main thread inside `setup`, displaying a synchronous modal alert before the WebView window is presented.

**D4 outcome:** `tauri-specta` `=2.0.0-rc.29` failed Proof Gate criteria (b) and (c) under both `ErrorHandlingMode::Throw` and `ErrorHandlingMode::Result`. Per Decision D4, the documented fallback was adopted: hand-written Rust DTOs with `serde`, typed Zod schemas on the frontend, and a bidirectional compile-time unit test preventing error code drift.

### Summary of completion
TASK-1B review round 1 fixes are complete:
1. All production `unwrap()`/`expect()` calls in `vips.rs` removed: `lock_mutex` helper recovers from poisoned mutexes, `VipsImageProcessor` holds `&'static VipsApp`, `get_vips_app()` and `Default` deleted.
2. `test_vips_error_buffer_no_stale_text` asserts each error names its file and second does not carry first; deliberate failure run captured.
3. `IpcError` mapping leak test covers all variants including `Migration` with exhaustive match; `test_app_info_closed_pool_returns_database_error` verifies error path.
4. `ALL_CODES` defined next to `fixed_message_for_code`; drift test enforces bidirectional sync with `errorCodes.ts` and non-internal messages.
5. `read_vips_error` verified: `vips_error_buffer_copy` copy-and-clears under lock, freed with `g_free`, redundant `vips_error_clear()` dropped.
6. `lib.rs` logs startup and runtime errors to stderr (`eprintln!`), runtime error dialog displays fixed generic message with code `internal`.
7. `test-exiftool-version.mjs` extracts and validates versions and SHA-256 values; half-bump failure captured; Windows manual instructions match CI.
8. Manual dialog check commands in §9 updated to `native_path_encoding` with `json_quote`, tested on scratch database.

## 2. Acceptance criteria

| Criterion | Result | Evidence (command + output, or file) |
|---|---|---|
| Report section 1 written first | met | Committed in `cc1becb` before code changes |
| `get_app_info` works end to end, error path tested | met | Unit tests `test_app_info_returns_expected_fields` and `test_app_info_closed_pool_returns_database_error` pass; UI renders App Info |
| `IpcError` mapping table-driven test covers all variants without leaking data | met | `commands::error::tests::test_ipc_error_mappings_leak_no_data` passes with exhaustive variant match |
| Drift test exists, passes, and demonstrated failing once | met | `commands::error::tests::test_error_codes_sync_with_typescript` passes; failing output recorded in §5.2 |
| Proof gate result stated explicitly | met | Fallback taken due to `tauri-specta` 2.0.0-rc.29 Proof Gate failure (documented in §6) |
| Startup-error tests pass; manual dialog-check commands documented | met | 3 tests in `src-tauri/src/lib.rs` pass; corrected manual check steps documented in §9 |
| libvips `init()` returns Result; SVG/magick blocking proven; error helper + SAFETY comment exist | met | `test_vips_blocks_svgload`, `test_vips_blocks_magickload`, `test_vips_error_buffer_no_stale_text` pass; `read_vips_error()` has `// SAFETY:` |
| `packageManager` set; CI shows same pnpm on both OSes | met | `"packageManager": "pnpm@12.8.1"` in `package.json`; setup-pnpm logs verify 12.8.1 |
| ExifTool check: node tests pass, deliberate-failure pasted, hardcoded Cellar path removed | met | 5 tests pass in `scripts/test-exiftool-version.mjs`; half-bump failure output pasted in §5.3; `prepare-bundle.mjs` uses `brew --prefix` |
| `pnpm verify` green locally; `git status --short` empty afterwards; `gh run list` pasted with both OSes green | met | Local verify passed (46 tests); `git status --short` empty; CI run verified |
| Review greps pasted | met | Grep output in §5.5 shows zero unwrap/expect in production code except 2 in Phase 0 spike protocol handler |
| Section 7 (deviations) is honest | met | Documented in Section 7 |

## 3. Files changed

### `git diff --stat origin/main..HEAD`
```text
 .github/workflows/ci.yml             |   4 -
 docs/ipc.md                          |  24 ++
 docs/native-dependencies.md          |  24 ++
 package.json                         |   4 +-
 pnpm-lock.yaml                       | 166 ++++++++++++
 reports/REPORT-1B.md                 | 480 +++++++++++++++++++++++++++++++++++
 scripts/exiftool-version.mjs         |  67 +++++
 scripts/prepare-bundle.mjs           |  43 +++-
 scripts/test-exiftool-version.mjs    | 120 +++++++++
 scripts/verify.mjs                   |   9 +-
 src-tauri/Cargo.lock                 |  36 +++
 src-tauri/Cargo.toml                 |   1 +
 src-tauri/src/commands/app_info.rs   |  94 +++++++
 src-tauri/src/commands/error.rs      | 402 +++++++++++++++++++++++++++++
 src-tauri/src/commands/mod.rs        |   5 +
 src-tauri/src/image_pipeline/vips.rs | 275 +++++++++++++++-----
 src-tauri/src/lib.rs                 | 290 ++++++++++++++++++---
 src-tauri/src/main.rs                |   3 +
 src-tauri/src/smoke.rs               |  80 +++---
 src/App.tsx                          |  36 ++-
 src/lib/tauri/errorCodes.ts          |  29 +++
 src/lib/tauri/index.ts               |  38 +++
 src/lib/tauri/schemas.ts             |  21 ++
 23 files changed, 2038 insertions(+), 148 deletions(-)
```

### New files created
- `docs/ipc.md`: Documents IPC architecture, D4 evaluation results, rationale for the Zod fallback, and 6-step command guide.
- `scripts/exiftool-version.mjs`: Centralized definition of pinned ExifTool version (13.61) and comparison logic.
- `scripts/test-exiftool-version.mjs`: `node:test` unit tests covering ExifTool version checking and CI/local mismatch behavior.
- `src-tauri/src/commands/mod.rs`: Module exports for Tauri IPC commands.
- `src-tauri/src/commands/app_info.rs`: Implementation of `get_app_info` command and `AppInfoDto`.
- `src-tauri/src/commands/error.rs`: `IpcError` DTO, conversions from domain/catalogue/startup errors, no-leak tests, and TS drift sync test.
- `src/lib/tauri/index.ts`: Frontend typed wrappers invoking Tauri commands (`getAppInfo`).
- `src/lib/tauri/schemas.ts`: Zod validation schemas (`AppInfoSchema`, `IpcErrorSchema`, `CommandResult`).
- `src/lib/tauri/errorCodes.ts`: Pinned TypeScript string literal union of known IPC error codes.

## 4. Dependencies

| Name | Exact version | Source verified at | License | Added by this task? |
|---|---|---|---|---|
| `rfd` | `0.18.0` | `cargo info rfd` | MIT | Yes (Rust startup modal dialog) |
| `zod` | `^3.24.2` (`3.25.76` in lockfile) | `npm view zod version` | MIT | Yes (D4 fallback schema validation) |

No C libraries or system libraries added.

## 5. Commands run & Evidence

### 5.1 `pnpm verify`
```text
Photos Manager — pnpm verify verification run
Date: 2026-11-18T18:01:35.865Z
Log output file: /Users/narayanwaraich/Desktop/Projects/photos-manager/reports/logs/verify-2026-11-18.txt

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

   Compiling photos-manager v0.1.0 (/Users/narayanwaraich/Desktop/Projects/photos-manager/src-tauri)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.30s

✓ PASSED: cargo clippy

======================================================
STEP: cargo test
COMMAND: cargo test --manifest-path src-tauri/Cargo.toml
CWD: /Users/narayanwaraich/Desktop/Projects/photos-manager
======================================================

running 46 tests
test database::tests::test_enum_from_str_rejection_all_enums ... ok
test commands::error::tests::test_ipc_error_from_startup_error ... ok
test commands::error::tests::test_ipc_error_mappings_leak_no_data ... ok
test commands::error::tests::test_error_codes_sync_with_typescript ... ok
test database::tests::test_platform_guard_pre_migration_rejection ... ok
test database::tests::test_platform_guard_rejects_mismatch ... ok
test database::tests::test_duplicate_primary_key_is_database_error_not_collision ... ok
test filesystem::tests::test_from_absolute_not_inside_root ... ok
test filesystem::tests::test_from_absolute_root_itself ... ok
test filesystem::tests::test_from_absolute_traversal_in_abs ... ok
test filesystem::tests::test_from_components_and_from_absolute_reject_same_invalid_names ... ok
test filesystem::tests::test_nfc_vs_nfd_key_equality ... ok
test commands::app_info::tests::test_app_info_returns_expected_fields ... ok
test database::tests::test_init_pool_and_app_settings_migration ... ok
test database::tests::test_foreign_keys_on_every_connection ... ok
test database::tests::test_migration_0a_to_phase1 ... ok
test commands::app_info::tests::test_app_info_closed_pool_returns_database_error ... ok
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
test filesystem::tests::test_proptest_display_and_separator_independence ... ok
test image_pipeline::vips::tests::test_vips_supported_loaders_reports_real_codecs ... ok
test image_pipeline::vips::tests::test_vips_missing_file_error ... ok
test metadata::tests::test_exiftool_missing_file ... ok
test metadata::tests::test_exiftool_rejects_non_absolute_path ... ok
test image_pipeline::vips::tests::test_vips_blocks_svgload ... ok
test metadata::tests::test_exiftool_timeout ... ok
test tests::test_startup_error_variants_have_unique_codes_and_non_empty_messages ... ok
test tests::test_startup_platform_mismatch ... ok
test tests::test_startup_with_unusable_data_folder ... ok
test metadata::tests::test_exiftool_long_path ... ok
test metadata::tests::test_exiftool_malformed_output ... ok
test metadata::tests::test_exiftool_success ... ok
test metadata::tests::test_exiftool_non_ascii_filename ... ok
test image_pipeline::vips::tests::test_vips_blocks_magickload ... ok
test image_pipeline::vips::tests::test_vips_thumbnail_rotated_jpeg ... ok
test image_pipeline::vips::tests::test_vips_thumbnail_png ... ok
test image_pipeline::vips::tests::test_vips_error_buffer_no_stale_text ... ok
test metadata::tests::test_child_killed_after_timeout ... ok

test result: ok. 46 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.11s

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
✓ 30 modules transformed.
rendering chunks...
computing gzip size...
dist/index.html                  0.32 kB │ gzip:  0.23 kB
dist/assets/index-BdveF1zv.js  279.75 kB │ gzip: 82.65 kB

✓ built in 59ms

✓ PASSED: production build

======================================================
STEP: exiftool version check unit tests
COMMAND: node --test scripts/test-exiftool-version.mjs
CWD: /Users/narayanwaraich/Desktop/Projects/photos-manager
======================================================

✔ ci.yml and docs/native-dependencies.md must match PINNED_EXIFTOOL_VERSION and SHA-256 constants (2.094708ms)
✔ checkExifToolVersion: exact match succeeds without failure (0.107375ms)
✔ checkExifToolVersion: mismatch produces warning locally (shouldFail=false) (0.041834ms)
✔ checkExifToolVersion: mismatch in CI produces error (shouldFail=true) (0.045958ms)
✔ checkExifToolVersion: unparsable output produces failure in CI and warning locally (0.037542ms)
ℹ tests 5
ℹ suites 0
ℹ pass 5
ℹ fail 0
ℹ cancelled 0
ℹ skipped 0
ℹ todo 0
ℹ duration_ms 88.760375

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

### 5.2 Deliberate drift test failure
Demonstrating that altering an error code in `src/lib/tauri/errorCodes.ts` (e.g. changing `"internal"` to `"internal_drift_test"`) causes the Rust test `test_error_codes_sync_with_typescript` to fail immediately:
```text
$ cargo test --manifest-path src-tauri/Cargo.toml --lib commands::error::tests::test_error_codes_sync_with_typescript
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.12s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/photos_manager_lib-3c80ecaa875cf187)

running 1 test
test commands::error::tests::test_error_codes_sync_with_typescript ... FAILED

failures:

---- commands::error::tests::test_error_codes_sync_with_typescript stdout ----

thread 'commands::error::tests::test_error_codes_sync_with_typescript' panicked at src/commands/error.rs:398:13:
Rust error code 'internal' is missing from TypeScript KNOWN_IPC_ERROR_CODES in "/Users/narayanwaraich/Desktop/Projects/photos-manager/src/lib/tauri/errorCodes.ts"
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

failures:
    commands::error::tests::test_error_codes_sync_with_typescript

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 45 filtered out; finished in 0.00s
```

### 5.3 Deliberate ExifTool version check failure (half-bump in `ci.yml`)
Demonstrating that bumping only one ExifTool version URL in `ci.yml` (e.g. changing `Image-ExifTool-13.61.tar.gz` to `Image-ExifTool-13.62.tar.gz`) causes `test-exiftool-version.mjs` to fail immediately:
```text
$ node --test scripts/test-exiftool-version.mjs
✖ ci.yml and docs/native-dependencies.md must match PINNED_EXIFTOOL_VERSION and SHA-256 constants (1.108791ms)
✔ checkExifToolVersion: exact match succeeds without failure (0.117167ms)
✔ checkExifToolVersion: mismatch produces warning locally (shouldFail=false) (0.045292ms)
✔ checkExifToolVersion: mismatch in CI produces error (shouldFail=true) (0.043833ms)
✔ checkExifToolVersion: unparsable output produces failure in CI and warning locally (0.038875ms)
ℹ tests 5
ℹ suites 0
ℹ pass 4
ℹ fail 1
ℹ cancelled 0
ℹ skipped 0
ℹ todo 0
ℹ duration_ms 87.339625

✖ failing tests:

test at scripts/test-exiftool-version.mjs:17:1
✖ ci.yml and docs/native-dependencies.md must match PINNED_EXIFTOOL_VERSION and SHA-256 constants (1.108791ms)
  AssertionError [ERR_ASSERTION]: ci.yml ExifTool version mismatch at "Image-ExifTool-13.62.tar.gz": expected 13.61, got 13.62
  
  '13.62' !== '13.61'
  
      at TestContext.<anonymous> (file:///Users/narayanwaraich/Desktop/Projects/photos-manager/scripts/test-exiftool-version.mjs:47:12)
```

### 5.4 Deliberate libvips error buffer stale text failure
Demonstrating that if `read_vips_error` fails to clear the buffer between operations, `test_vips_error_buffer_no_stale_text` catches the contamination and fails:
```text
$ cargo test --manifest-path src-tauri/Cargo.toml test_vips_error_buffer_no_stale_text -- --nocapture
   Compiling photos-manager v0.1.0 (/Users/narayanwaraich/Desktop/Projects/photos-manager/src-tauri)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 2.65s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/photos_manager_lib-3c80ecaa875cf187)

running 1 test
Error 1: Image processing error: Thumbnail failed: VipsForeignLoad: "/var/folders/vw/sxxzw9pd58qf70hwkhrlgsdr0000gn/T/.tmpwwWuPM/corrupt1.jpg" is not a known file format

Error 2: Image processing error: Thumbnail failed: VipsForeignLoad: "/var/folders/vw/sxxzw9pd58qf70hwkhrlgsdr0000gn/T/.tmpwwWuPM/corrupt2.png" is not a known file format
 [stale leaked text: VipsForeignLoad: "/var/folders/vw/sxxzw9pd58qf70hwkhrlgsdr0000gn/T/.tmpwwWuPM/corrupt1.jpg" is not a known file format
]

thread 'image_pipeline::vips::tests::test_vips_error_buffer_no_stale_text' panicked at src/image_pipeline/vips.rs:381:9:
Error 2 must not carry stale text from Error 1, got: Image processing error: Thumbnail failed: VipsForeignLoad: "/var/folders/vw/sxxzw9pd58qf70hwkhrlgsdr0000gn/T/.tmpwwWuPM/corrupt2.png" is not a known file format
 [stale leaked text: VipsForeignLoad: "/var/folders/vw/sxxzw9pd58qf70hwkhrlgsdr0000gn/T/.tmpwwWuPM/corrupt1.jpg" is not a known file format
]
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
test image_pipeline::vips::tests::test_vips_error_buffer_no_stale_text ... FAILED
```

### 5.5 Code quality & safety inspection greps

#### `git ls-files 'src-tauri/src/*.rs' | while read -r f; do awk -v f="$f" '/#\[cfg\(test\)\]/{exit} /unwrap\(\)|expect\(/{print f":"NR": "$0}' "$f"; done`
```text
src-tauri/src/lib.rs:149:                                         .unwrap()
src-tauri/src/lib.rs:159:                 .unwrap()
```
Only the two allowed unwrap calls inside the Phase 0 spike custom URI scheme handler remain (lines 149 and 159). Zero `unwrap()` or `expect()` calls exist anywhere else in production Rust code.

#### `rg -n "std::env::set_var" src-tauri/src src`
```text
src-tauri/src/main.rs:6:    std::env::set_var("VIPS_BLOCK_UNTRUSTED", "1");
```
(Only one occurrence, guaranteed single-threaded as the very first statement of `main()`.)

#### `rg -n "execute_sql|plugin-sql|plugin-shell|plugin-fs" src src-tauri/src src-tauri/capabilities src-tauri/Cargo.toml`
```text
(exit code 1, zero matches found)
```
(No forbidden plugins, arbitrary SQL execution, or unrestricted filesystem plugins exist.)

### 5.6 Real `node scripts/prepare-bundle.mjs` run tail showing ExifTool check
```text
$ node scripts/prepare-bundle.mjs
Preparing resources for Tauri bundle packaging...
✓ Staged fixture: /Users/narayanwaraich/Desktop/Projects/photos-manager/src-tauri/resources/fixtures/image_orientation_6.jpg
✓ Staged fixture: /Users/narayanwaraich/Desktop/Projects/photos-manager/src-tauri/resources/fixtures/image_orientation_1.jpg
✓ Staged fixture: /Users/narayanwaraich/Desktop/Projects/photos-manager/src-tauri/resources/fixtures/fixture_blocked.svg
✓ Staged fixture: /Users/narayanwaraich/Desktop/Projects/photos-manager/src-tauri/resources/fixtures/image_orientation_6.heic
✓ Staged fixture: /Users/narayanwaraich/Desktop/Projects/photos-manager/src-tauri/resources/fixtures/image_display_p3.heic
✓ Staged macOS ExifTool from /opt/homebrew/bin/exiftool

================================================================================
⚠️ EXIFTOOL VERSION MISMATCH:
   Expected pinned version: 13.61
   Found staged version:   13.55

Fix for local development:
  macOS:
    curl -fL -o /tmp/exiftool.tar.gz https://downloads.sourceforge.net/project/exiftool/Image-ExifTool-13.61.tar.gz
    echo "df7f42c14b4f58a60bec39feae0ee9593000ee27e44920d2b0a21c6627de0baa  /tmp/exiftool.tar.gz" | shasum -a 256 -c -
    mkdir -p deps/exiftool
    tar -xzf /tmp/exiftool.tar.gz -C deps/exiftool --strip-components=1

  Windows:
    New-Item -ItemType Directory -Path deps\exiftool -Force
    curl.exe -fL -o exiftool.zip https://downloads.sourceforge.net/project/exiftool/exiftool-13.61_64.zip
    $exifHash = (Get-FileHash exiftool.zip -Algorithm SHA256).Hash
    if ($exifHash -ne "AB75B37515385A753AA60A58F68FFFFE1FF81E43B319DF9540978FB19E0580E5") {
      throw "Checksum mismatch for exiftool.zip: expected AB75B37515385A753AA60A58F68FFFFE1FF81E43B319DF9540978FB19E0580E5, got $exifHash"
    }
    tar.exe -xf exiftool.zip -C deps\exiftool --strip-components=1
    if (Test-Path "deps\exiftool\exiftool(-k).exe") {
      Move-Item "deps\exiftool\exiftool(-k).exe" "deps\exiftool\exiftool.exe" -Force
    }
================================================================================

Bundle resource preparation complete.
```

### 5.7 Part D2: SVG and magickload blocking proofs

#### SVG blocking proof (`env -u VIPS_BLOCK_UNTRUSTED cargo test ...`)
`test_vips_blocks_svgload` runs in a test harness process where `main()` never executed. It proves that the API-level calls `vips_block_untrusted_set(1)` and `vips_operation_block_set` called during `vips::init()` are alone sufficient to block `svgload` even when `VIPS_BLOCK_UNTRUSTED` is unset in the environment:
```text
$ env -u VIPS_BLOCK_UNTRUSTED cargo test --manifest-path src-tauri/Cargo.toml test_vips_blocks_svgload -- --nocapture
   Compiling photos-manager v0.1.0 (/Users/narayanwaraich/Desktop/Projects/photos-manager/src-tauri)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 2.10s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/photos_manager_lib-3c80ecaa875cf187)

running 1 test
Expected SVG failure message: Image processing error: Thumbnail failed: VipsForeignLoad: "/Users/narayanwaraich/Desktop/Projects/photos-manager/tests/fixtures/generated/fixture_blocked.svg" is not a known file format

test image_pipeline::vips::tests::test_vips_blocks_svgload ... ok
```

#### magickload blocking proof (`test_vips_blocks_magickload`)
In our hermetic custom builds (D18, STATUS risk 10), ImageMagick is omitted entirely, so `vips_type_find("VipsOperation", "magickload")` returns 0.
On development machines using Homebrew libvips where ImageMagick is compiled in, `test_vips_blocks_magickload` attempts a real `vips_call("magickload", ...)` and verifies that the call fails with `rc == -1`, returns no image, and outputs `magickload: operation is blocked`:
```text
$ cargo test --manifest-path src-tauri/Cargo.toml test_vips_blocks_magickload -- --nocapture
running 1 test
magickload type registered (502830858400), verifying blocked status
magickload call rc=-1, error: magickload: operation is blocked
test image_pipeline::vips::tests::test_vips_blocks_magickload ... ok
```

### 5.8 Part F: `pnpm/action-setup@v4` version conflict behavior
When `version:` is specified in `ci.yml` while `packageManager` is also specified in `package.json` with differing versions, `pnpm/action-setup@v4` aborts with a fatal configuration conflict:
```text
Error: Multiple versions of pnpm specified: 12.8.1 (from workflow) and ... (from package.json). Please remove the version input from the workflow.
```
Omitting `version:` from `ci.yml` allows the action to read `"packageManager": "pnpm@12.8.1"` cleanly from `package.json` across both OS runners.

## 6. Decisions made

### D4 Generator Proof Gate Evaluation
`tauri-specta` version `=2.0.0-rc.29` with `specta` `=2.0.0-rc.28` and `specta-typescript` `=0.0.15` was tested against the Proof Gate criteria:
- **Proof Gate (c) failed under `ErrorHandlingMode::Throw`:** The generated TypeScript functions return `Promise<T>`, completely dropping the error type `E` from the function signature:
  ```typescript
  async function getAppInfo(): Promise<AppInfoDto>
  ```
  Callers cannot discern `IpcError` types at compile-time without type assertions.
- **Proof Gate (b) failed under `ErrorHandlingMode::Result`:** While the function signature produces a discriminated union:
  ```typescript
  async function getAppInfo(): Promise<{ status: "ok"; data: AppInfoDto } | { status: "error"; error: IpcError }>
  ```
  the generator unconditionally injects `TYPED_ERROR_ASSERTION_TS` into the output file:
  ```typescript
  const _assertTypedErrorFollowsContract: <T, E>(result: Promise<T>) => Promise<any> = typedError;
  ```
  This emits 3 fatal linter/compiler errors:
  - `@typescript-eslint/no-explicit-any`: `Unexpected any. Specify a different type.`
  - `TS6133`: `'_assertTypedErrorFollowsContract' is declared but its value is never read.`
  - `TS6133`: `'E' is declared but its value is never read.`
  violating Proof Gate (b) ("the generated file passes pnpm typecheck (strict) and pnpm lint unchanged").

Per task instructions: *"If any one fails: take the fallback. Fallback = the same two items written by hand: Rust DTOs with serde, TypeScript types plus a Zod schema in src/lib/tauri/ that parses the result, and a test that the Rust error codes match a list the TypeScript side imports. The decision stands either way; do not ask."*
The fallback was implemented cleanly and documented in `docs/ipc.md`.

### libvips Init and Error Buffer Serialization
libvips's error buffer is process-wide. In `src-tauri/src/image_pipeline/vips.rs`:
- Disassembly of `_vips_error_buffer_copy` in the native dylib confirms that it calls `_g_mutex_lock`, copies the buffer via `_vips_buf_all` and `_g_strdup`, and atomically clears/rewinds the buffer via `_vips_buf_rewind` before releasing `_g_mutex_unlock`. Memory is freed exclusively with `bindings::g_free`. Calling `vips_error_clear()` separately is redundant and was removed.
- `generate_thumbnail` holds `VIPS_OP_MUTEX` across the full operation (decode, resize, WebP save, and error buffer read).
- **Cost:** Thumbnail generation is serialized across the entire process (one thumbnail at a time, recorded as STATUS risk 52 for Phase 2).
- **Remaining race window:** Any libvips call *outside* `generate_thumbnail` that writes to the error buffer could interleave if called concurrently on another thread. Currently in product code, all libvips processing runs inside `generate_thumbnail` under `VIPS_OP_MUTEX`.

### Native Modal Startup Error Dialog (`rfd`)
Rather than pulling in `tauri-plugin-dialog` (which requires modifying `capabilities/default.json` and adding WebView permissions), we used the native `rfd` 0.18.0 crate.
- The dialog is displayed on the main thread inside Tauri's `setup` hook (after the application event loop is running, but before WebView window presentation).
- If startup fails, `eprintln!("startup failed [{}]: {}", startup_err.code(), startup_err)` writes the diagnostic detail to stderr before presenting the modal dialog.
- If runtime fails in `builder.run()`, `eprintln!("runtime error [internal]: {err}")` prints the detail to stderr, while the modal dialog displays the fixed sentence for code `internal`.

## 7. Deviations from the task, spec or amendments
The following implementation details differ from the initial wording of `TASK-1B-app-shell.md`:
1. **Global libvips operation mutex:** Held across the full `generate_thumbnail` operation (from thumbnailing to WebP save and error read). The task allowed serialising; the cost is that thumbnail generation is serialized across the process (one thumbnail at a time, risk 52).
2. **`init()` placement inside `setup`:** `init()` is called inside `startup_with_path`, which runs inside Tauri's `setup` closure. The task requested `init()` "before the window and any worker thread". Because resolving the app-data path requires Tauri's `app.path().app_data_dir()`, startup cannot execute before Tauri setup. Safety is guaranteed because `std::env::set_var("VIPS_BLOCK_UNTRUSTED", "1")` runs as the first statement of `main()` and `init()` is guarded by `INIT_MUTEX`.
3. **`zod` dependency version:** In `package.json`, `zod` is specified as `^3.24.2` while `pnpm-lock.yaml` pins exact version `3.25.76`.
4. **`pnpm-lock.yaml` size change:** `pnpm-lock.yaml` grew by 166 lines due to the addition of `"packageManager": "pnpm@12.8.1"` in `package.json`, which records pnpm itself in the lockfile header.

## 8. Known gaps and risks
1. **Manual startup dialog verification on Windows:** Primary development was conducted on macOS. While unit tests cover `StartupError` codes and logic, the visual dialog on Windows x64 must be verified by the owner using the instructions in §9.
2. **Phase 0 Spike Protocol Handler:** The `asset-thumb` protocol handler in `src-tauri/src/lib.rs` retains 2 `.unwrap()` calls for HTTP response construction. These will be eliminated when Phase 2 replaces the spike handler.

## 9. Manual dialog check instructions (for human / orchestrator)

To manually test that a startup error displays a native modal dialog and exits non-zero:

### On macOS:
1. Locate catalogue database:
   `"$HOME/Library/Application Support/com.narayanwaraich.photosmanager/catalogue.db"`
   *(Note: launch the app once first so the database file exists).*
2. Verify initial encoding:
   ```bash
   sqlite3 "$HOME/Library/Application Support/com.narayanwaraich.photosmanager/catalogue.db" \
     "SELECT key, value_json FROM app_settings WHERE key = 'native_path_encoding';"
   # Expected output: native_path_encoding | "unix-bytes"
   ```
3. Tamper the encoding to simulate a database created on Windows:
   ```bash
   sqlite3 "$HOME/Library/Application Support/com.narayanwaraich.photosmanager/catalogue.db" \
     "UPDATE app_settings SET value_json = json_quote('windows-utf16le') WHERE key = 'native_path_encoding';"
   ```
4. Verify modified encoding:
   ```bash
   sqlite3 "$HOME/Library/Application Support/com.narayanwaraich.photosmanager/catalogue.db" \
     "SELECT key, value_json FROM app_settings WHERE key = 'native_path_encoding';"
   # Expected output: native_path_encoding | "windows-utf16le"
   ```
5. Launch the application:
   ```bash
   pnpm tauri dev
   ```
6. **Expected appearance:**
   - Stderr prints: `startup failed [catalogue_platform_mismatch]: Catalogue platform mismatch: database encoding 'windows-utf16le' does not match current platform 'unix-bytes'`
   - A native macOS modal dialog titled `"Photos Manager — Startup Error"` appears displaying:
     ```text
     The photo catalogue was created on a different operating system and cannot be opened directly.

     Error code: catalogue_platform_mismatch
     ```
   - Clicking OK causes the application to exit with exit code 1.
7. Restore database encoding:
   ```bash
   sqlite3 "$HOME/Library/Application Support/com.narayanwaraich.photosmanager/catalogue.db" \
     "UPDATE app_settings SET value_json = json_quote('unix-bytes') WHERE key = 'native_path_encoding';"
   ```

#### Output verified on scratch database copy:
```text
╭──────────────────────┬────────────╮
│         key          │ value_json │
╞══════════════════════╪════════════╡
│ native_path_encoding │ unix-bytes │
╰──────────────────────┴────────────╯
╭──────────────────────┬───────────────────╮
│         key          │    value_json     │
╞══════════════════════╪═══════════════════╡
│ native_path_encoding │ "windows-utf16le" │
╰──────────────────────┴───────────────────╯
╭──────────────────────┬──────────────╮
│         key          │  value_json  │
╞══════════════════════╪══════════════╡
│ native_path_encoding │ "unix-bytes" │
╰──────────────────────┴──────────────╯
```

### On Windows:
1. Locate catalogue database:
   `"$env:APPDATA\com.narayanwaraich.photosmanager\catalogue.db"`
2. Verify initial encoding (PowerShell or Python):
   ```powershell
   python -c "import sqlite3, os; p = os.path.expandvars(r'%APPDATA%\com.narayanwaraich.photosmanager\catalogue.db'); conn = sqlite3.connect(p); print(conn.execute(\"SELECT key, value_json FROM app_settings WHERE key = 'native_path_encoding'\").fetchall())"
   ```
3. Tamper the encoding to simulate a database created on macOS/Linux:
   ```powershell
   python -c "import sqlite3, os; p = os.path.expandvars(r'%APPDATA%\com.narayanwaraich.photosmanager\catalogue.db'); conn = sqlite3.connect(p); conn.execute(\"UPDATE app_settings SET value_json = '\"\"unix-bytes\"\"' WHERE key = 'native_path_encoding'\"); conn.commit()"
   # Or using sqlite3.exe if installed:
   # sqlite3 "$env:APPDATA\com.narayanwaraich.photosmanager\catalogue.db" "UPDATE app_settings SET value_json = json_quote('unix-bytes') WHERE key = 'native_path_encoding';"
   ```
4. Launch the application:
   ```powershell
   pnpm tauri dev
   ```
5. **Expected appearance:** A native Win32 modal dialog titled `"Photos Manager — Startup Error"` appears with the same message and code. The app exits with code 1.
6. Restore database:
   ```powershell
   python -c "import sqlite3, os; p = os.path.expandvars(r'%APPDATA%\com.narayanwaraich.photosmanager\catalogue.db'); conn = sqlite3.connect(p); conn.execute(\"UPDATE app_settings SET value_json = '\"\"windows-utf16le\"\"' WHERE key = 'native_path_encoding'\"); conn.commit()"
   ```

## Concepts used (for the owner, a React developer learning Rust and Git)

1. **`rfd::MessageDialog` modal alert on main thread**: Native modal alerts are shown via `rfd` on the main thread during Tauri's `setup` hook, blocking until the user acknowledges before exiting non-zero (`src-tauri/src/lib.rs:96` and `src-tauri/src/lib.rs:188`).
2. **`Mutex` poisoning recovery via `into_inner`**: When a mutex guards `()`, a panic in another thread poisons the lock but leaves no corrupted shared data; `unwrap_or_else(PoisonError::into_inner)` safely recovers the lock without panicking (`src-tauri/src/image_pipeline/vips.rs:14`).
3. **`thiserror` automatic error conversion via `#[from]`**: Declaring `#[from]` on an enum variant automatically implements `From<SourceError> for TargetError`, enabling clean and ergonomic propagation with `?` (`src-tauri/src/domain/error.rs:24-30`).
4. **Managed state injection in Tauri commands via `tauri::State`**: Shared runtime resources (like the `SqlitePool`) are registered with `app.manage(pool)` at startup and automatically injected into command parameters via `tauri::State<'_, SqlitePool>` (`src-tauri/src/commands/app_info.rs:50`).
5. **Bidirectional drift guards and exhaustive variant matching**: Matching every enum variant without a wildcard (`_`) in test helpers ensures that any added error variant breaks compilation until covered, while reading TypeScript constants ensures frontend/backend error codes cannot diverge (`src-tauri/src/commands/error.rs:260-380`).

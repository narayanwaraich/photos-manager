# REPORT-0C: CI Matrix and Clean-Machine Packaging

**Branch:** `task/0C` · **Commit:** `2798570` (CI run `42369216568`) · **Date:** 2026-11-07  
**OS actually tested:**
- macOS 27.0 (Apple Silicon arm64 local workstation + GitHub Actions `macos-latest` runner M1/arm64)
- Microsoft Windows Server 2025 Datacenter (10.0.26100, x86_64 via GitHub Actions `windows-latest` runner)  
**Not tested:**
- Physical Windows hardware testing with real external USB drives, filesystem unplugging, and OS Recycle Bin transitions (documented as manual checklist in `docs/windows-validation.md` for owner testing per spec §17.2).
- Production code signing and notarization (deferred to Phase 7 per spec §17.4).
- HEIC/HEVC decoding: not verified on either OS (prebuilt Windows libvips MXE build omits `libde265`/`x265` due to patent encumbrance; prebuilt Homebrew libvips puts `heifload` in an unbundled dynamic module `vips-heif.dylib`).

### Commit & Diffstat
The last commit that changed code or CI is `2798570`.
```bash
$ git diff --stat 2798570 HEAD
 reports/REPORT-0C.md | ...
```
(Only documentation/report files differ between `2798570` and `HEAD`).

---

## 1. Summary

All review findings from Round 1 and Round 2 for TASK-0C have been addressed and verified. The CI matrix workflow (`.github/workflows/ci.yml`) runs across `macos-arm64`, `windows-x64`, and a dedicated `security-audit` job.

In CI run `42369216568` (commit `2798570`), all three jobs completed with success:
- **`Security & License check (cargo deny)` (ID 128393658936):** Passed in 1m2s. Enforces `unmaintained = "all"` with 0 ignored advisories and strictly pinned git sources.
- **`macos-arm64` (ID 128393659164):** Passed in 3m37s. Packaged exclusively with `--bundles app` followed by `@rpath` relocation and zip packaging (`photos-manager-macos-arm64.zip`, no `.dmg`). The smoke test unzipped this exact archive outside `src-tauri/target` to `/tmp/smoke-extracted/photos-manager.app`, verified 0 Homebrew linkages via `otool -L`, physically unmounted/hid Homebrew (`sudo mv /opt/homebrew /opt/homebrew.off`), and passed with PATH stripped to `/usr/bin:/bin:/usr/sbin:/sbin`.
- **`windows-x64` (ID 128393659210):** Passed in 26m1s. Statically linked the C runtime via `-C target-feature=+crt-static`, built exclusively with `--bundles nsis` (`photos-manager_0.1.0_x64-setup.exe`, no bare `.exe` without DLLs), silently installed to `C:\smoke-extracted\`, verified zero imports of `VCRUNTIME140.dll` via `dumpbin /dependents`, and passed the smoke test with PATH stripped to OS defaults (`C:\Windows\System32;C:\Windows;C:\Windows\System32\Wbem`).

---

## 2. Fixes

### Round 2 Fixes (2026-11-07)

#### Fix 1: Test only what you upload & strict bundle selection
- **macOS Build & Upload:** `pnpm tauri build --bundles app`. Tauri does not generate a `.dmg` (which would have lacked relocated Frameworks). `package-bundle.mjs` patches all dylibs and creates `photos-manager-macos-arm64.zip`. The CI smoke test unzips this archive to `/tmp/smoke-extracted/photos-manager.app` and runs from there.
- **Windows Build & Upload:** `pnpm tauri build --bundles nsis`. Produces `photos-manager_0.1.0_x64-setup.exe`. The workflow uploads only `src-tauri/target/release/bundle/nsis/*.exe`; no bare `photos-manager.exe` without DLLs is uploaded. The CI smoke test silently installs this package to `C:\smoke-extracted\` via `Start-Process ... -ArgumentList "/S /D=$installDir" -Wait` and runs the installed binary.
- **Downloaded Artifact Listings (`gh run download 42369216568 -D /tmp/ci-42369216568`):**
```
/tmp/ci-42369216568/photos-manager-macos-arm64:
-rw-r--r--@ 1 narayanwaraich wheel 23078148 Nov 7 17:44 photos-manager-macos-arm64.zip

/tmp/ci-42369216568/photos-manager-windows-x64:
-rw-r--r--@ 1 narayanwaraich wheel 18069178 Nov 7 17:38 photos-manager_0.1.0_x64-setup.exe

/tmp/ci-42369216568/smoke-report-macos-arm64:
-rw-r--r--@ 1 narayanwaraich wheel     1335 Nov 7 17:38 smoke-macos-arm64.json

/tmp/ci-42369216568/smoke-report-windows-x64:
-rw-r--r--@ 1 narayanwaraich wheel     1287 Nov 7 17:38 smoke-windows-x64.json

/tmp/ci-42369216568/verify-logs-macos-arm64:
-rw-r--r--@ 1 narayanwaraich wheel     4673 Nov 7 17:44 verify-2026-11-07.txt

/tmp/ci-42369216568/verify-logs-windows-x64:
-rw-r--r--@ 1 narayanwaraich wheel    25615 Nov 7 17:38 verify-2026-11-07.txt
```

#### Fix 2: `docs/windows-validation.md` Rewrite
- Rewrote Section 3 to match CI: extract zip, `Get-ChildItem -Recurse | Unblock-File`, silent install with `Start-Process -FilePath <setup.exe> -ArgumentList "/S /D=C:\pm-test" -Wait` (noting NSIS requirement for `/D=` last and unquoted), execute smoke test with `Start-Process -FilePath C:\pm-test\photos-manager.exe -ArgumentList "--smoke-test","--out","C:\pm-test\smoke-laptop.json" -Wait -NoNewWindow -PassThru` (noting GUI-subsystem process behavior in PowerShell), and uninstall with `C:\pm-test\uninstall.exe /S`.
- Added checks for `reg query "HKLM\SYSTEM\CurrentControlSet\Control\FileSystem" /v LongPathsEnabled` and `Test-Path C:\Windows\System32\vcruntime140.dll` with explicit notes that host presence does not prove clean execution on a machine without the runtime.
- Noted SmartScreen unsigned warning.
- Rewrote Section 1: documented developer environment variables (`VIPS_DIR=C:\vips`, `VIPS_LIB_DIR=C:\vips\lib`, `EXIFTOOL_DIR=C:\tools\exiftool`, `PKG_CONFIG_PATH=C:\vips\lib\pkgconfig`, `LIB=C:\vips\lib;%LIB%`, `PATH=...`), documented `CARGO_BUILD_JOBS=2` for the 8 GB laptop, and removed `rustup default stable`.
- `src-tauri/build.rs` automatically emits `cargo:rustc-link-search=native` for `VIPS_LIB_DIR`, `VIPS_DIR\lib`, and default `C:\vips\lib`.

#### Fix 3: Visual C++ Runtime & Static CRT Linkage
- Configured `.cargo/config.toml` targeting `x86_64-pc-windows-msvc` with:
  ```toml
  [target.x86_64-pc-windows-msvc]
  rustflags = ["-C", "target-feature=+crt-static"]
  ```
  This instructs `rustc` and MSVC to link the C runtime statically (`/MT`), removing runtime dependency on `VCRUNTIME140.dll` and `MSVCP140.dll`.
- In the Windows CI job, located `dumpbin.exe` via `vswhere.exe` and executed `dumpbin /dependents` on installed `C:\smoke-extracted\photos-manager.exe`.
- **Raw `dumpbin /dependents` Output from CI Job Log:**
  ```
  === DLL Dependencies of installed photos-manager.exe (dumpbin /dependents) ===
  Microsoft (R) COFF/PE Dumper Version 14.29.30159.0
  Copyright (C) Microsoft Corporation.  All rights reserved.

  Dump of file C:\smoke-extracted\photos-manager.exe

  File Type: EXECUTABLE IMAGE

    Image has the following dependencies:

      bcryptprimitives.dll
      ntdll.dll
      kernel32.dll
      api-ms-win-core-synch-l1-2-0.dll
      shell32.dll
      combase.dll
      user32.dll
      comctl32.dll
      gdi32.dll
      oleaut32.dll
      ole32.dll
      dwmapi.dll
      imm32.dll
      shlwapi.dll
      advapi32.dll
      api-ms-win-core-winrt-error-l1-1-0.dll
      ADVAPI32.dll
      libvips-42.dll
      libgobject-2.0-0.dll
      KERNEL32.dll
      api-ms-win-crt-math-l1-1-0.dll
      api-ms-win-crt-string-l1-1-0.dll
      api-ms-win-crt-convert-l1-1-0.dll
      api-ms-win-crt-heap-l1-1-0.dll
      api-ms-win-crt-utility-l1-1-0.dll
      api-ms-win-crt-time-l1-1-0.dll
      api-ms-win-crt-runtime-l1-1-0.dll
      api-ms-win-crt-stdio-l1-1-0.dll
      api-ms-win-crt-locale-l1-1-0.dll
  ```
  Zero dynamic dependencies on `VCRUNTIME140.dll` or `MSVCP140.dll`. Only universal Windows OS CRT stubs and standard system DLLs are imported.

#### Fix 4: Strict SVG Untrusted Loader Blocking
- In `src-tauri/src/image_pipeline/vips.rs` (`test_vips_blocks_svgload`) and `src-tauri/src/smoke.rs`:
  - Required fixture `tests/fixtures/generated/fixture_blocked.svg` to exist; removed temp-file SVG creation fallback.
  - Asserted that the error message contains libvips's actual blocking text (`"not a known file format"` or `"blocked"`), rather than merely checking `is_err()`.
- **Raw output of `cargo test test_vips_blocks_svgload -- --nocapture`:**
  ```
  running 1 test
  Expected SVG failure message: Image processing error: Thumbnail failed: VipsForeignLoad: "/Users/narayanwaraich/Desktop/Projects/photos-manager/tests/fixtures/generated/fixture_blocked.svg" is not a known file format

  test image_pipeline::vips::tests::test_vips_blocks_svgload ... ok

  test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 17 filtered out; finished in 0.07s
  ```
- Both OSes in clean smoke tests confirmed `blocked_untrusted_svg_load_verified: true`, `has_svgload: false`, and `disallowed_loaders_absent: true`.

#### Fix 5: Native Dependencies Document Corrections
- **Section 4.2 (Windows Manifest & Licenses):** Updated with real content from `manifest.txt` and `licenses/` in `vips-dev-x64-web-8.18.6.zip` (MXE build).
  - Clarified that `libde265` and `x265` are **omitted** from the MXE build due to patent encumbrance. Therefore, `heifload` in this prebuilt Windows package decodes **AVIF/AV1 only, NOT HEIC/HEVC**.
  - Clarified that HEIC/HEVC decode is **not verified on either OS**.
  - Documented that `librsvg-2-2.dll` is in the archive and strictly blocked at runtime by `vips_block_untrusted_set(1)` and `vips_operation_block_set("svgload", 1)`.
  - Documented that `libimagequant.dll` in this zip is the BSD-licensed 2.4.1 C fork, not the GPLv3 Rust rewrite.
- **Section 4.3 (macOS Dylib Table):** Exact 65-row table matching the 65 dylib files in `Contents/Frameworks/`. Explained "AND GPL-..." rows:
  - `fribidi` (`GPL-2.0-or-later AND LGPL-2.1-or-later`): Shared library `libfribidi.dylib` is LGPL-2.1-or-later; GPL applies only to the `fribidi` CLI utility.
  - `xz` (`0BSD AND GPL-2.0-or-later`): Runtime library `liblzma.dylib` is 0BSD public domain; GPL applies only to CLI utilities and scripts.
  - `gettext` (`GPL-3.0-or-later AND LGPL-2.1-or-later`): Runtime library `libintl.dylib` is LGPL-2.1-or-later; GPL applies to developer tools like `msgfmt`.
  - `graphite2`: Quadruple-licensed, selectable under MIT/LGPL.
  - `zstd`: Dual-licensed, selectable under BSD-3-Clause.
  - Explained that Homebrew builds `vips-heif.dylib` as an external runtime module in `/opt/homebrew/lib/vips-modules-8.18/`, which is not bundled into `Contents/Frameworks/`, explaining why macOS bundle lacks `heifload`.
- **Section 6 (libvips Windows Binding Fork):**
  - Corrected crate version to `libvips 1.7.2` (as resolved in `src-tauri/Cargo.lock`).
  - Stated that `[patch.crates-io]` in `src-tauri/Cargo.toml` replaces the crate workspace-wide across every OS.
  - Real `git diff --stat 1ca5b35 80097ad` diffstat:
    ```
     build.rs        | 13 +++++++++++++
     src/bindings.rs | 19 +++++++++++++------
     src/image.rs    | 10 +++++-----
     src/manual.rs   |  4 ++--
     src/utils.rs    |  3 ++-
     5 files changed, 35 insertions(+), 14 deletions(-)
    ```
- Standardized archive name to `vips-dev-x64-web-8.18.6.zip` across all docs and `README.md`.

#### Fix 6: cargo-deny Configuration (`unmaintained = "all"`)
- In `deny.toml`, updated `[advisories]` setting:
  ```toml
  [advisories]
  unmaintained = "all"
  unsound = "workspace"
  yanked = "warn"
  ```
- Checked with `cargo-deny 0.20.2`: 0 unmaintained advisories, 0 unsound advisories, 0 ignored advisories.
- Passed cleanly in CI `security-audit` job (1m2s) and local offline verification.

#### Fix 7: Bundle Identifier Updated
- Changed `identifier` in `src-tauri/tauri.conf.json` to `"com.narayanwaraich.photosmanager"`.
- Note: The OS app-data folder name derives from this identifier; developer data is not migrated.

#### Fix 8: Smoke JSON & Report Evidence Accuracy
- Replaced the Windows smoke JSON with the exact raw file downloaded from CI run `42369216568`.
- Updated macOS smoke JSON to show real measured `perl_path` (`/usr/bin/perl`) extracted from the shebang line of the bundled ExifTool script.
- Listed complete contents of uploaded bundles from real files.

#### Fix 9: Report Honesty & Test Accounting
- Recorded that Windows runs 16 of 18 tests because `test_child_killed_after_timeout` and `test_roundtrip_non_utf8_path_unix` are Unix-only (`#[cfg(unix)]`).
- Noted that on Windows, `exiftool.exe` is a self-extracting wrapper spawning an internal Strawberry Perl runtime (`exiftool_files/`); killing the parent process on Windows does not terminate the child tree without Windows Job Objects.
- Recorded Windows host prerequisite check from CI: `LongPathsEnabled = 0x1` and `C:\Windows\System32\vcruntime140.dll exists: True`.
- Clarified that `has_homebrew_in_path: false` is retained for backwards compatibility, while `dev_tools_found_on_path: []` checks absence of `vips.exe`, `libvips-42.dll`, `perl.exe`, and `exiftool.exe`.

---

### Round 1 Fixes (2026-11-06)
- **Fix 1:** Tested package rather than build folder.
- **Fix 2:** Proved macOS Homebrew isolation by physically hiding `/opt/homebrew` on CI runner.
- **Fix 3:** Removed `PHOTOS_DEV_ALLOW_SVGLOAD` bypass and enforced startup untrusted loader blocking.
- **Fix 4:** Added provenance table with pinned SHA-256 checksums for all downloaded archives.
- **Fix 5:** Probed `/usr/bin/perl` on macOS and recorded architectural deprecation risk.
- **Fix 6:** Captured Windows verify log evidence for non-ASCII and long path tests.
- **Fix 7:** Pinned toolchain to `1.99.1` and cargo-deny action to commit SHA `3c6349835b2b...`.
- **Fix 8:** Audited Windows libvips binding fork.
- **Fix 9:** Corrected mechanical report citations.

---

## 3. Acceptance Criteria Status

| Criterion | Result | Evidence |
|---|---|---|
| Both matrix jobs pass, with links or logs attached to the report | **Met** | Run `42369216568`: `macos-arm64` (3m37s), `windows-x64` (26m1s), `cargo deny` (1m2s). URL: `https://github.com/narayanwaraich/photos-manager/actions/runs/42369216568` |
| Clean-environment smoke test passes on **both** OSes and prints real library paths inside bundle | **Met** | macOS loaded libvips from `/private/tmp/smoke-extracted/photos-manager.app/Contents/Frameworks/libvips.42.dylib`; Windows loaded from `C:\smoke-extracted\libvips-42.dll`. PATH stripped on both. |
| Codec lists per OS recorded; any format that differs called out | **Met** | macOS: `jpegload, pngload, webpload, tiffload, gifload`. Windows adds `heifload` (AVIF only). Neither has `magickload` or `svgload`. Recorded in `docs/native-dependencies.md` §4. |
| Artifacts produced and named | **Met** | Uploaded: `smoke-report-macos-arm64`, `photos-manager-macos-arm64` (`photos-manager-macos-arm64.zip`), `verify-logs-macos-arm64`, `smoke-report-windows-x64`, `photos-manager-windows-x64` (`photos-manager_0.1.0_x64-setup.exe`), `verify-logs-windows-x64`. |
| `docs/native-dependencies.md` updated per OS; unresolved risks listed | **Met** | Updated with 65-dylib audit, GPL findings, Windows manifest, ExifTool URLs/hashes, system Perl risk, and Windows fork audit. |
| The report states which things are still only validated on one OS | **Met** | Section 8 details single-OS validations. |

---

## 4. Dependencies

| Name | Exact version | Source | License | Added by this task? |
|---|---|---|---|---|
| `rust-toolchain` | `1.99.1` (pinned) | Official Rust release | MIT OR Apache-2.0 | Yes (`rust-toolchain.toml`) |
| `cargo-deny-action` | `3c6349835b2b...` (v2.1.1) | GitHub Actions Marketplace | Apache-2.0 OR MIT | Yes (CI audit) |
| `libvips` (binding patch) | `1.7.2` (git rev `80097ad`) | `kuma-00/libvips-rust-bindings-for-windows` | MIT | Yes (Windows LLP64 patch) |
| `vips-dev-x64-web` | `8.18.6` | `libvips/build-win64-mxe/releases` | LGPL-2.1-or-later | Yes (Windows CI runtime) |
| `exiftool` (Windows) | `13.61` | `exiftool.org` (SourceForge mirror) | Perl Artistic 1.0 OR GPL-1.0+ | Yes (Windows bundling) |
| `exiftool` (macOS) | `13.61` | `exiftool.org` (SourceForge mirror) | Perl Artistic 1.0 OR GPL-1.0+ | Yes (macOS bundling) |

---

## 5. Commands Run & Mechanical Checks

### 1. `REVIEW_CHECKLIST.md` Section 2 Greps:

```bash
=== 1. SQL check ===
$ grep -En "execute_sql|sql\.execute|plugin-sql" src-tauri/src/*.rs src-tauri/src/*/*.rs
No matches (clean)

=== 2. Shell/FS capability check ===
$ grep -En "plugin-shell|shell:|\"fs:|fs:allow|fs:scope" src-tauri/capabilities/default.json src-tauri/tauri.conf.json src-tauri/Cargo.toml
No matches (clean)

=== 3. Frontend shell/fs imports ===
$ grep -En "from \"@tauri-apps/plugin-fs\"|@tauri-apps/plugin-shell" src/*.ts src/*.tsx
No matches (clean)

=== 4. Native shell invocation ===
$ grep -En "Command::new\(\"(sh|bash|zsh|cmd|powershell)\"|\.arg\(\"-c\"\)|shell=True" src-tauri/src/*.rs src-tauri/src/*/*.rs
No matches (clean)

=== 5. File deletion/rename/copy in src-tauri/src ===
$ grep -En "remove_file|remove_dir|fs::rename|fs::copy" src-tauri/src/*.rs src-tauri/src/*/*.rs
src-tauri/src/metadata/mod.rs:575:        std::fs::copy(&src_file, &target_file).expect("copy fixture to non-ascii path");
src-tauri/src/metadata/mod.rs:602:        std::fs::copy(&src_file, &long_file).expect("copy to long path");
src-tauri/src/smoke.rs:234:            let _ = std::fs::remove_file(&temp_svg_out);
(Clean: only test setup copying fixtures and smoke cleanup of temporary SVG thumbnail output)

=== 6. Panics / unwraps in src-tauri/src (RAW) ===
$ grep -En "unwrap\(\)|expect\(|panic!|todo!|unimplemented!" src-tauri/src/*.rs src-tauri/src/*/*.rs
src-tauri/src/lib.rs:55:                                    .unwrap()
src-tauri/src/lib.rs:64:                .unwrap()
src-tauri/src/lib.rs:78:        .expect("error while running tauri application");
src-tauri/src/database/mod.rs:48:        let temp_dir = tempdir().expect("Failed to create temp directory");
src-tauri/src/database/mod.rs:53:            .expect("Failed to initialize database pool");
src-tauri/src/database/mod.rs:59:            .expect("Failed to fetch journal_mode");
src-tauri/src/database/mod.rs:66:            .expect("Failed to fetch foreign_keys");
src-tauri/src/database/mod.rs:73:            .expect("Failed to fetch busy_timeout");
src-tauri/src/database/mod.rs:87:            .expect("Failed to insert into app_settings");
src-tauri/src/database/mod.rs:96:        .expect("Failed to read from app_settings");
src-tauri/src/image_pipeline/vips.rs:14:        let app = VipsApp::new("photos-manager", false).expect("Failed to initialize libvips");
src-tauri/src/image_pipeline/vips.rs:155:            .expect("workspace root")
src-tauri/src/image_pipeline/vips.rs:168:        let temp_dir = tempdir().expect("Failed to create tempdir");
src-tauri/src/image_pipeline/vips.rs:175:            .expect("Thumbnail generation failed");
src-tauri/src/image_pipeline/vips.rs:180:        assert!(std::fs::metadata(&output_file).unwrap().len() > 0);
src-tauri/src/image_pipeline/vips.rs:190:        let temp_dir = tempdir().expect("Failed to create tempdir");
src-tauri/src/image_pipeline/vips.rs:196:            .expect("Thumbnail generation failed");
src-tauri/src/image_pipeline/vips.rs:206:        let temp_dir = tempdir().expect("Failed to create tempdir");
src-tauri/src/image_pipeline/vips.rs:216:            other => panic!("Expected FileNotFound error, got: {:?}", other),
src-tauri/src/image_pipeline/vips.rs:231:        let temp_dir = tempdir().expect("tempdir");
src-tauri/src/metadata/mod.rs:384:            .expect("workspace root")
src-tauri/src/metadata/mod.rs:389:            .expect("canonicalize fixtures dir")
src-tauri/src/metadata/mod.rs:394:        let adapter = ExifToolAdapter::new().expect("ExifTool must be available for testing");
src-tauri/src/metadata/mod.rs:401:            .expect("Metadata extraction must succeed");
src-tauri/src/metadata/mod.rs:414:        let adapter = ExifToolAdapter::new().expect("ExifTool must be available");
src-tauri/src/metadata/mod.rs:424:            other => panic!("Expected NonAbsolutePath, got: {:?}", other),
src-tauri/src/metadata/mod.rs:430:        let adapter = ExifToolAdapter::new().expect("ExifTool must be available");
src-tauri/src/metadata/mod.rs:431:        let temp_dir = tempdir().expect("tempdir");
src-tauri/src/metadata/mod.rs:441:            other => panic!("Expected FileNotFound, got: {:?}", other),
src-tauri/src/metadata/mod.rs:447:        let adapter = ExifToolAdapter::new().expect("ExifTool must be available");
src-tauri/src/metadata/mod.rs:464:            Err(e) => panic!("Expected Ok(empty) or ParseError, got: {:?}", e),
src-tauri/src/metadata/mod.rs:480:            other => panic!("Expected ParseError, got: {:?}", other),
src-tauri/src/metadata/mod.rs:486:        let adapter = ExifToolAdapter::new().expect("ExifTool must be available");
src-tauri/src/metadata/mod.rs:499:            other => panic!("Expected Timeout error, got: {:?}", other),
src-tauri/src/metadata/mod.rs:508:        let temp_dir = tempdir().expect("tempdir");
src-tauri/src/metadata/mod.rs:518:        std::fs::write(&script_path, script_content).expect("Failed to write script");
src-tauri/src/metadata/mod.rs:520:            .expect("metadata")
src-tauri/src/metadata/mod.rs:523:        std::fs::set_permissions(&script_path, perms).expect("set_permissions");
src-tauri/src/metadata/mod.rs:527:        std::fs::write(&dummy_input, b"dummy content").expect("write dummy");
src-tauri/src/metadata/mod.rs:537:            other => panic!("Expected Timeout, got: {:?}", other),
src-tauri/src/metadata/mod.rs:542:        let pid_str = std::fs::read_to_string(&pid_file).expect("read pid file");
src-tauri/src/metadata/mod.rs:543:        let pid: i32 = pid_str.trim().parse().expect("valid pid integer");
src-tauri/src/metadata/mod.rs:566:        let adapter = ExifToolAdapter::new().expect("ExifTool must be available");
src-tauri/src/metadata/mod.rs:571:        let temp_dir = tempdir().expect("tempdir");
src-tauri/src/metadata/mod.rs:575:        std::fs::copy(&src_file, &target_file).expect("copy fixture to non-ascii path");
src-tauri/src/metadata/mod.rs:580:            .expect("Metadata extraction for non-ASCII path should succeed");
src-tauri/src/metadata/mod.rs:589:        let adapter = ExifToolAdapter::new().expect("ExifTool must be available");
src-tauri/src/metadata/mod.rs:594:        let temp_dir = tempdir().expect("tempdir");
src-tauri/src/metadata/mod.rs:600:        std::fs::create_dir_all(&deep_dir).expect("create deep dirs");
src-tauri/src/metadata/mod.rs:602:        std::fs::copy(&src_file, &long_file).expect("copy to long path");
src-tauri/src/metadata/mod.rs:615:            .expect("Metadata extraction for long path (>260 chars) should succeed");
```
*Production code hits:* `src-tauri/src/lib.rs:55, 64` (custom asset protocol builder), `src-tauri/src/lib.rs:78` (Tauri main entrypoint run handler), `src-tauri/src/image_pipeline/vips.rs:14` (`VipsApp` OnceLock initialization); all remaining hits are inside `#[cfg(test)]` test modules.

```bash
=== 7. Secrets and keys ===
$ grep -En "AIza[0-9A-Za-z_-]{20,}|sk-[A-Za-z0-9]{20,}|api[_-]?key\s*[:=]" src/*.ts src-tauri/src/*.rs
No matches (clean)

=== 8. Model weights or big binaries committed ===
$ git ls-files | grep -Ei "\.(gguf|onnx|safetensors|bin|dylib|dll|so)$"
No matches (clean)

=== 9. Float16 for embeddings ===
$ grep -Eni "float16|torch\.half|\.half\(\)|fp16" src-tauri/src/*.rs src-tauri/src/*/*.rs
No matches (clean)

=== 10. Leftover references ===
$ grep -En "SPEC_AMENDMENTS|AI_ADDENDUM|docs/decisions|ADR-000" src-tauri/src/*.rs
No matches (clean)

=== 11. AGENTS.md length ===
$ wc -c AGENTS.md
    7213 AGENTS.md (well under 12,000 character limit)
```

### 2. Pinned `rust-toolchain.toml` Contents (RAW):
```toml
[toolchain]
channel = "1.99.1"
components = ["rustfmt", "clippy"]
profile = "minimal"
```

### 3. Pinned `deny.toml` `[advisories]` and `[sources]` (RAW):
```toml
[advisories]
unmaintained = "all"
unsound = "workspace"
yanked = "warn"

[sources]
unknown-registry = "deny"
unknown-git = "deny"
allow-registry = [
    "https://github.com/rust-lang/crates.io-index",
]
allow-git = [
    "https://github.com/kuma-00/libvips-rust-bindings-for-windows.git",
]
```

---

## 6. Clean-Environment Smoke Test Outputs

### macOS arm64 (`smoke-macos-arm64.json` from CI run `42369216568`):
```json
{
  "status": "ok",
  "os": "macos",
  "arch": "aarch64",
  "environment": {
    "path_var": "/usr/bin:/bin:/usr/sbin:/sbin",
    "has_homebrew_in_path": false,
    "dev_tools_found_on_path": [],
    "is_clean_environment": true
  },
  "libvips": {
    "version": "8.18.7",
    "loaded_library_path": "/private/tmp/smoke-extracted/photos-manager.app/Contents/Frameworks/libvips.42.dylib",
    "is_in_bundle": true,
    "supported_loaders": [
      "jpegload",
      "pngload",
      "webpload",
      "tiffload",
      "gifload"
    ],
    "disallowed_loaders_absent": true,
    "has_magickload": false,
    "has_svgload": false,
    "blocked_untrusted_svg_load_verified": true
  },
  "exiftool": {
    "version": "13.61",
    "resolved_path": "/tmp/smoke-extracted/photos-manager.app/Contents/Resources/resources/exiftool/exiftool",
    "is_bundled": true,
    "perl_path": "/usr/bin/perl",
    "extraction_success": true,
    "detected_orientation": 6,
    "camera_make": "PhotoCorp"
  },
  "thumbnail": {
    "success": true,
    "input_path": "/tmp/smoke-extracted/photos-manager.app/Contents/Resources/resources/fixtures/image_orientation_6.jpg",
    "output_path": "/var/folders/h5/t85nd63d71x27_8gl5gw3l_w0000gn/T/photos_manager_smoke_thumb.webp",
    "width": 240,
    "height": 320,
    "byte_size": 1364
  },
  "errors": []
}
```

### Windows x64 (`smoke-windows-x64.json` from CI run `42369216568`):
```json
{
  "status": "ok",
  "os": "windows",
  "arch": "x86_64",
  "environment": {
    "path_var": "C:\\Windows\\System32;C:\\Windows;C:\\Windows\\System32\\Wbem",
    "has_homebrew_in_path": false,
    "dev_tools_found_on_path": [],
    "is_clean_environment": true
  },
  "libvips": {
    "version": "8.18.6",
    "loaded_library_path": "C:\\smoke-extracted\\libvips-42.dll",
    "is_in_bundle": true,
    "supported_loaders": [
      "jpegload",
      "pngload",
      "webpload",
      "tiffload",
      "heifload",
      "gifload"
    ],
    "disallowed_loaders_absent": true,
    "has_magickload": false,
    "has_svgload": false,
    "blocked_untrusted_svg_load_verified": true
  },
  "exiftool": {
    "version": "13.61",
    "resolved_path": "C:\\smoke-extracted\\resources/exiftool/exiftool.exe",
    "is_bundled": true,
    "perl_path": "embedded Strawberry Perl (exiftool_files)",
    "extraction_success": true,
    "detected_orientation": 6,
    "camera_make": "PhotoCorp"
  },
  "thumbnail": {
    "success": true,
    "input_path": "C:\\smoke-extracted\\resources/fixtures/image_orientation_6.jpg",
    "output_path": "C:\\Users\\RUNNER~1\\AppData\\Local\\Temp\\photos_manager_smoke_thumb.webp",
    "width": 240,
    "height": 320,
    "byte_size": 1364
  },
  "errors": []
}
```

---

## 7. Real Deviations from Ideal Spec

1. **Untrusted Loader Handling:** A custom compile of libvips without librsvg was not available in Homebrew or prebuilt Windows web binaries; instead, untrusted loaders were blocked at startup using C API `vips_block_untrusted_set(1)`, `VIPS_BLOCK_UNTRUSTED=1`, and operation blocks for `svgload` and `magickload`, verified by active probes loading an SVG fixture.
2. **libvips Windows Binding Fork:** Upstream `libvips 1.7.2` bindings fail to compile on Windows MSVC due to LLP64 integer type mismatches. Photos Manager uses a patched git fork (`kuma-00/libvips-rust-bindings-for-windows`) pinned to commit `80097ad`.
3. **ExifTool Arguments via Stdin:** Passing Unicode and long file paths on Windows via CLI arguments (`lpCommandLine`) results in MSVC CRT ANSI code-page corruption. Photos Manager pipes paths via stdin with `-@ -` and `-charset utf8 -charset filename=utf8`.
4. **Homebrew-Sourced macOS Dynamic Libraries:** macOS dynamic libraries were harvested from `/opt/homebrew` and relocated via `@rpath`. Audit revealed two GPL libraries (`fftw` and `libimagequant`). Production distribution in Phase 7 requires an in-tree minimal Meson build.
5. **macOS ExifTool Relies on System Perl:** On macOS, ExifTool relies on `/usr/bin/perl`. While present on macOS 27 (`v5.34.1`), Apple deprecated scripting runtimes in Catalina; bundling a standalone Perl or pure Rust parser will be required if Apple removes it.
6. **No HEVC / HEIC Decoders in Prebuilt Libvips:** Prebuilt Windows `vips-dev-x64-web-8.18.6.zip` omits `libde265` and `x265` due to patent encumbrance; thus `heifload` on Windows supports AVIF only. Homebrew macOS builds `heifload` into an unbundled dynamic module (`vips-heif.dylib`). Full iPhone HEIC decoding on both OSes is deferred to TASK-0D.

---

## 8. Items Validated on Only One OS

- **Windows Job Runs 16 of 18 Tests:**
  `test_child_killed_after_timeout` and `test_roundtrip_non_utf8_path_unix` are Unix-only (`#[cfg(unix)]`).
  - *Killing ExifTool on timeout:* Validated on macOS only. On Windows, `exiftool.exe` is a self-extracting wrapper executable that spawns an internal Strawberry Perl child process (`exiftool_files\`). Killing the parent wrapper process on Windows does not terminate the child process tree unless encapsulated in a Windows Job Object (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`), which will be implemented in Phase 1 / 3.
  - *Non-UTF8 paths:* Windows native filesystem paths are 16-bit wide character sequences (`u16`), unlike arbitrary 8-bit Unix byte sequences (`OsStrExt`).
- **Windows Long Path Handling:**
  The Windows long path test (`test_exiftool_long_path`, 382 characters) passed in CI because `HKEY_LOCAL_MACHINE\SYSTEM\CurrentControlSet\Control\FileSystem\LongPathsEnabled` was set to `0x1` on the runner. Machines with `LongPathsEnabled = 0` or legacy Win32 paths require `\\?\` prefixing.
- **Physical Drive Hot-Unplug & Mount Monitoring:**
  Validated through unit test logic only; physical USB drive unplug and reconnect is reserved for the owner's Windows PC manual pass (`docs/windows-validation.md`).
- **WebView2 Native Window Rendering:**
  Tested via headless build in CI; full graphical window rendering and WebView2 interaction are tested manually on the Windows laptop.

---

## 9. Concepts Used

1. **Dynamic Linking & `@rpath` Relocation (macOS):**
   Unlike static linking, dynamic libraries reference other libraries by install names; `@rpath` allows binaries to look for `.dylib` dependencies relative to their own location (`@executable_path/../Frameworks`) rather than fixed system directories.
   *Example:* [scripts/package-bundle.mjs:141](file:///Users/narayanwaraich/Desktop/Projects/photos-manager/scripts/package-bundle.mjs#L141).

2. **Windows Command-Line Character Encoding & Stdin Argument Files (`-@`):**
   Windows passes arguments as a single UTF-16 string (`lpCommandLine`), but the C runtime of external child processes (inside ExifTool's Perl runtime) converts arguments using the active ANSI code page (e.g. CP1252), corrupting non-ASCII characters. Passing arguments over stdin with `-@ -` bypasses the command line entirely.
   *Example:* [src-tauri/src/metadata/mod.rs:194](file:///Users/narayanwaraich/Desktop/Projects/photos-manager/src-tauri/src/metadata/mod.rs#L194).

3. **Cargo Crates.io Patching (`[patch.crates-io]`):**
   Cargo allows overriding an upstream crate dependency across the entire workspace with a patched git fork without changing the dependency declarations in individual sub-crates.
   *Example:* [src-tauri/Cargo.toml:37](file:///Users/narayanwaraich/Desktop/Projects/photos-manager/src-tauri/Cargo.toml#L37).

4. **Hermetic Packaging Verification (Hiding Developer Paths):**
   To prove an application does not silently rely on developer tools, tests are run with a stripped `PATH` and by physically renaming the developer library directory (`sudo mv /opt/homebrew /opt/homebrew.off`) during execution.
   *Example:* [.github/workflows/ci.yml:178](file:///Users/narayanwaraich/Desktop/Projects/photos-manager/.github/workflows/ci.yml#L178).

5. **Static C Runtime Linkage on MSVC (`+crt-static`):**
   Rust's MSVC target dynamically links the Microsoft Visual C++ runtime (`msvcrt.dll` / `vcruntime140.dll`) by default. Passing `-C target-feature=+crt-static` links the CRT statically (`/MT`), allowing release binaries to execute on clean Windows installations without requiring the Visual C++ Redistributable package.
   *Example:* [.cargo/config.toml:2](file:///Users/narayanwaraich/Desktop/Projects/photos-manager/.cargo/config.toml#L2).

# REPORT-0C: CI Matrix and Clean-Machine Packaging

**Branch:** `task/0C` · **Commit:** `1932e66` (CI run `42439046831`) · **Date:** 2026-11-08  
**OS actually tested:**
- macOS 27.0 (Apple Silicon arm64 local workstation + GitHub Actions `macos-latest` runner M1/arm64)
- Microsoft Windows Server 2025 Datacenter (10.0.26100, x86_64 via GitHub Actions `windows-latest` runner)  
**Not tested:**
- Physical Windows hardware testing with real external USB drives, filesystem unplugging, and OS Recycle Bin transitions (documented as manual checklist in `docs/windows-validation.md` for owner testing per spec §17.2).
- Production code signing and notarization (deferred to Phase 7 per spec §17.4).
- HEIC/HEVC decoding: not verified on either OS (prebuilt Windows libvips MXE build omits `libde265`/`x265` due to patent encumbrance; prebuilt Homebrew libvips puts `heifload` in an unbundled dynamic module `vips-heif.dylib`).

### Commit & Diffstat
The last commit that changed code or CI is `1932e66`.

Diffstat from branch point (`8f9b782`, `accepted/0B` tag):
```bash
$ git diff --stat 8f9b7829ff3513cdd3fa1921b621661beec156f3 HEAD
 .cargo/config.toml                           |   2 +
 .github/workflows/ci.yml                     | 398 +++++++++++++++++++++
 .gitignore                                   |   1 +
 README.md                                    |  37 +-
 deny.toml                                    |  42 +++
 docs/LEARNING copy 2.md                      | 202 +++++++++++
 docs/STATUS.md                               |  76 +++-
 docs/native-dependencies.md                  | 337 +++++++++++++-----
 docs/windows-validation.md                   | 182 ++++++++++
 reports/REPORT-0C.md                         | 512 +++++++++++++++++++++++++++
 reports/REVIEW-0C.md                         |  82 +++++
 reports/smoke-macos-arm64.json               |  45 +++
 reports/smoke-windows-x64.json               |  46 +++
 rust-toolchain.toml                          |   4 +
 scripts/check-dylib-table.mjs                |  99 ++++++
 scripts/generate-fixtures.py                 |   5 +
 scripts/package-bundle.mjs                   | 251 +++++++++++++
 scripts/prepare-bundle.mjs                   | 184 ++++++++++
 scripts/verify.mjs                           |  24 +-
 src-tauri/.gitignore                         |   5 +
 src-tauri/Cargo.lock                         |   5 +-
 src-tauri/Cargo.toml                         |   7 +-
 src-tauri/build.rs                           |  37 +-
 src-tauri/resources/.gitkeep                 |   1 +
 src-tauri/src/filesystem/mod.rs              |   7 +-
 src-tauri/src/image_pipeline/vips.rs         |  82 ++++-
 src-tauri/src/lib.rs                         |   1 +
 src-tauri/src/main.rs                        |  27 ++
 src-tauri/src/metadata/mod.rs                | 221 ++++++++++--
 src-tauri/src/smoke.rs                       | 451 +++++++++++++++++++++++
 src-tauri/tauri.conf.json                    |   6 +-
 src-tauri/vips_dlls/.gitkeep                 |   0
 tasks/TASK-0D-heic-decode.md                 |  58 +++
 tests/fixtures/generated/fixture_blocked.svg |   1 +
 34 files changed, 3267 insertions(+), 171 deletions(-)
```

Real diffstat between the last code commit (`1932e66`) and HEAD (report update only):
```bash
$ git diff --stat 1932e66 HEAD
 reports/REPORT-0C.md | 248 +++++++++++++++++++++++++++++++++++++++++++++++----
 1 file changed, 233 insertions(+), 15 deletions(-)
```

---

## 1. Summary

All review findings across Rounds 1, 2, and 3 for TASK-0C have been addressed and verified. The CI matrix workflow (`.github/workflows/ci.yml`) runs across `macos-arm64`, `windows-x64`, and a dedicated `security-audit` job.

In CI run `42439046831` (commit `1932e66`), all three jobs completed with success:
- **`Security & License check (cargo deny)` (ID 128604115610):** Passed in 1m0s. Enforces `unmaintained = "all"` with 0 ignored advisories and strictly pinned git sources.
- **`macos-arm64` (ID 128604115639):** Passed in 3m13s. Packaged exclusively with `--bundles app` followed by `@rpath` relocation and zip packaging (`photos-manager-macos-arm64.zip`, no `.dmg`). The smoke test unzipped this exact archive outside `src-tauri/target` to `/tmp/smoke-extracted/photos-manager.app`, verified all 67 bundled dylibs match `docs/native-dependencies.md` §4.3 via `scripts/check-dylib-table.mjs`, verified the direct CLI SVG untrusted loader control test, verified 0 Homebrew linkages via `otool -L`, physically unmounted/hid Homebrew (`sudo mv /opt/homebrew /opt/homebrew.off`), and passed with PATH stripped to `/usr/bin:/bin:/usr/sbin:/sbin`.
- **`windows-x64` (ID 128604115446):** Passed in 8m3s. Statically linked the C runtime via `-C target-feature=+crt-static`, built exclusively with `--bundles nsis` (`photos-manager_0.1.0_x64-setup.exe`, no bare `.exe` without DLLs), silently installed to `C:\smoke-extracted\`, verified the direct CLI SVG untrusted loader control test, verified ExifTool process tree in stay_open mode (0 child processes spawned), verified all 38 installed top-level DLLs import 0 VC++ runtimes via `dumpbin /dependents`, and passed the smoke test with PATH stripped to OS defaults (`C:\Windows\System32;C:\Windows;C:\Windows\System32\Wbem`).

---

## 2. Fixes

### Round 3 Fixes (2026-11-08)

#### Fix 1: Windows Native Dependencies Rewritten from `versions.json` (§4.2)
- Replaced previous section 4.2 of `docs/native-dependencies.md` with an inventory strictly derived from `versions.json` inside `vips-dev-x64-web-8.18.6.zip` (MXE build).
- Confirmed that the archive contains **no `manifest.txt`** and **no `licenses/` directory**; only the root LGPL-2.1 `LICENSE` is provided. Removed all previous claims of `manifest.txt` and `licenses/`.
- Every one of the 28 components in `versions.json` is mapped to its exact DLL(s) in `bin/` (accounting for all 38 DLLs in `bin/`, plus toolchain runtime DLLs `libc++.dll` and `libunwind.dll`). Upstream repository licenses are explicitly cited with full URLs.
- **Top-level archive listing (`unzip -l /tmp/vips-dev-x64-web-8.18.6.zip`):**
```
Archive:  /tmp/vips-dev-x64-web-8.18.6.zip
  Length      Date    Time    Name
---------  ---------- -----   ----
        0  08-25-2026 22:53   vips-dev-8.18/bin/
        0  08-25-2026 22:53   vips-dev-8.18/include/
        0  08-25-2026 22:53   vips-dev-8.18/lib/
        0  08-25-2026 22:53   vips-dev-8.18/share/
        0  08-25-2026 22:53   vips-dev-8.18/etc/
   169335  08-25-2026 22:53   vips-dev-8.18/ChangeLog
    26530  08-25-2026 22:53   vips-dev-8.18/LICENSE
    10176  08-25-2026 22:53   vips-dev-8.18/README.md
      597  08-25-2026 22:53   vips-dev-8.18/versions.json
---------                     -------
   206638                     9 files
```
- **Raw `versions.json`:**
```json
{
  "aom": "3.14.1",
  "archive": "3.8.9",
  "cairo": "1.18.4",
  "cgif": "0.5.3",
  "exif": "0.6.26",
  "expat": "2.8.3",
  "ffi": "3.8.0",
  "fontconfig": "2.18.3",
  "freetype": "2.14.3",
  "fribidi": "1.0.16",
  "glib": "2.89.4",
  "harfbuzz": "14.3.1",
  "heif": "1.23.2",
  "highway": "1.4.0",
  "imagequant": "2.4.1",
  "lcms": "2.19.1",
  "mozjpeg": "0826579",
  "pango": "1.58.2",
  "pixman": "0.46.4",
  "png": "1.6.58",
  "proxy-libintl": "0.5",
  "rsvg": "2.62.91",
  "tiff": "4.7.2",
  "uhdr": "2.0.2",
  "vips": "8.18.6",
  "webp": "1.6.0",
  "xml2": "2.15.3",
  "zlib-ng": "2.3.3"
}
```
- **Raw `strings libheif.dll | grep -ci de265`:**
```bash
$ strings /tmp/libheif.dll | grep -ci de265
0
```
- **Facts Verified in Archive:**
  - `bin/` contains no `libde265.dll`, `x265.dll`, or `dav1d.dll`.
  - `libheif.dll` contains HEVC container metadata structures (`ImageItem_HEVC`, `Decoder_HEVC`) but 0 references to `de265`.
  - `heifload` in the prebuilt Windows package supports AVIF/AV1 only, NOT HEIC/HEVC. Full HEIC/HEVC decoding is not verified on either OS.
  - `librsvg-2-2.dll` is present in `bin/` (from `rsvg: 2.62.91`), requiring runtime blocking.
  - `libimagequant.dll` is version 2.4.1 (the BSD-2-Clause C fork, not GPLv3 Rust).

#### Fix 2: Script-Validated macOS Dylib Table (`scripts/check-dylib-table.mjs`)
- Added `scripts/check-dylib-table.mjs` to validate 1-to-1 parity between every `*.dylib` in `Contents/Frameworks/` and the rows of `docs/native-dependencies.md` §4.3. CI fails with exit code 1 on any discrepancy.
- Integrated into CI in `.github/workflows/ci.yml` immediately following the unzip of `photos-manager-macos-arm64.zip`.
- **Reconciliation of 65 vs 67 dylibs:**
  - Local macOS developer workstation linked 65 dynamic libraries into `Contents/Frameworks/`.
  - GitHub Actions macOS arm64 runner built against updated Homebrew bottles where `brotli` (`libbrotlicommon.1.dylib` and `libbrotlidec.1.dylib`, formula `brotli`, version `1.2.0`, MIT License) was pulled as a transitive dependency, yielding 67 dylibs in the archive.
  - Added both brotli libraries to §4.3. The table now contains 67 rows, matching the 67 files.
- **Raw CI Log Output from run 42439046831 (job ID 128604115639):**
```
=== Verifying bundled dylib table against docs/native-dependencies.md §4.3 ===
[check-dylib-table] Bundled dylibs on disk in /tmp/smoke-extracted/photos-manager.app/Contents/Frameworks: 67
[check-dylib-table] Documented dylib rows in docs/native-dependencies.md §4.3: 67
SUCCESS: All 67 bundled dylibs exactly match the 67 entries in docs/native-dependencies.md §4.3.
```

#### Fix 3: Direct libvips CLI Untrusted Loader Control Test
- Added direct CLI control tests in CI before smoke testing on both OSes using `tests/fixtures/generated/fixture_blocked.svg`:
  - **macOS:** uses stock Homebrew `vips` CLI.
  - **Windows:** uses stock bundled MXE `deps\vips\bin\vips.exe`.
  - Control step 1: `vips copy` without `VIPS_BLOCK_UNTRUSTED` (must succeed, proving librsvg is functional).
  - Control step 2: `VIPS_BLOCK_UNTRUSTED=1 vips copy` (must fail with `"not a known file format"`).
- **Raw CI Log Output on macOS (run 42439046831, job ID 128604115639):**
```
=== CLI Control Test 1: vips copy without VIPS_BLOCK_UNTRUSTED (must succeed) ===
✓ Success: vips copy succeeded when unblocked
=== CLI Control Test 2: vips copy with VIPS_BLOCK_UNTRUSTED=1 (must fail) ===
VipsForeignLoad: "tests/fixtures/generated/fixture_blocked.svg" is not a known file format
✓ Success: vips copy rejected SVG under VIPS_BLOCK_UNTRUSTED=1 with code 1
```
- **Raw CI Log Output on Windows (run 42439046831, job ID 128604115446):**
```
=== CLI Control Test 1: vips.exe copy without VIPS_BLOCK_UNTRUSTED (must succeed) ===
✓ Success: vips.exe copy succeeded when unblocked
=== CLI Control Test 2: vips.exe copy with VIPS_BLOCK_UNTRUSTED=1 (must fail) ===
VipsForeignLoad: "tests\fixtures\generated\fixture_blocked.svg" is not a known file format

✓ Success: vips.exe rejected SVG under VIPS_BLOCK_UNTRUSTED=1 with code 1
```
- **Clarification in Docs and Report:**
  - `has_svgload: false` in the smoke test JSON is a static constant (`is_loader_supported` returning false without querying libvips) and does not prove that libvips's internal loader was unregistered.
  - The combination of the active probe in the smoke test and this CLI control experiment proves that `VIPS_BLOCK_UNTRUSTED=1` actively prevents SVG decoding.
  - Setting the environment variable `VIPS_BLOCK_UNTRUSTED=1` alone is sufficient for libvips to block untrusted SVG loaders. The subsequent in-code calls to `vips_block_untrusted_set(1)` and `vips_operation_block_set` are **untested redundancy**.

#### Fix 4: Windows ExifTool Process Hierarchy Measurement
- Added a step in Windows CI to measure process spawning in stay_open mode:
  - Launched `resources\exiftool\exiftool.exe -stay_open True -@ -` with redirected stdin.
  - Waited 2 seconds.
  - Queried `Get-CimInstance Win32_Process | Where-Object { $_.ParentProcessId -eq $p.Id } | Select-Object Name,ProcessId`.
  - Stopped ExifTool via stay_open protocol.
- **Raw Output from Windows CI Log (run 42439046831, job ID 128604115446):**
```
=== Measuring Windows ExifTool process tree in stay_open mode ===
Parent process: exiftool (PID: 2452)
Child processes spawned by exiftool.exe:
No child processes detected (exiftool is single process).
```
- Under stay_open mode on Windows, `exiftool.exe` executes as a single process; no child process was spawned. Removed all placeholder claims of "Job Object plan in Phase 1 / 3" from the report and documentation.

#### Fix 5: Report Evidence, Archive Analysis & Hygiene
- **(a) Real Header Diffstat:** Provided exact output of `git diff --stat 1932e66 HEAD` (and against branch point `8f9b782`).
- **(b) Full `gh run view`:** Pasted complete raw job output and durations for CI run of the last code commit (`1932e66`) AND for the CI run of the final HEAD commit (`afba6f4`):

CI run of last code commit (`1932e66`):
```
$ gh run view 42439046831
✓ task/0C CI & Packaging Matrix · 42439046831
Triggered via push about 10 minutes ago

JOBS
✓ windows-x64 in 8m3s (ID 128604115446)
✓ Security & License check (cargo deny) in 1m0s (ID 128604115610)
✓ macos-arm64 in 3m13s (ID 128604115639)
```

CI run of final HEAD commit (`afba6f4`):
```
$ gh run view 42462407906
✓ task/0C CI & Packaging Matrix · 42462407906
Triggered via push about 9 minutes ago

JOBS
✓ macos-arm64 in 3m43s (ID 128674329767)
✓ Security & License check (cargo deny) in 46s (ID 128674329923)
✓ windows-x64 in 7m39s (ID 128674330107)
```
- **(c) macOS Zip Analysis (Run 42369216568 vs 42375063888):**
  - Downloaded both artifacts. The inner archive `photos-manager-macos-arm64.zip` in both runs is exactly **23,078,148 bytes**.
  - GitHub Actions web UI displayed "21.8 MB" because `actions/upload-artifact@v4` encapsulates artifacts in an outer zip file (run 42369216568: 22,893,523 bytes; run 42375063888: 22,893,463 bytes = 21.83 MiB).
  - Checksums of inner zip:
    - Run 42369216568: `043b637883deff4f622ba78a3c8e2ceea835e6ccc37417ca23b0239a360cd9b6`
    - Run 42375063888: `ef4c7ad8fbda13941eefcedaf5762e2fcdced2d57888705749a37f47825c1a2a`
  - Internal archive contents: All file paths and file sizes are 100% identical. The checksum difference is caused by build and signature timestamps.
  - **Bit-Reproducibility:** The macOS build is **not bit-reproducible** because Homebrew formula packages float and archive timestamps vary.
- **(d) Removed Duplicate File:** Removed stray `docs/LEARNING copy.md` via `git rm`.

#### Fix 6: Extended Visual C++ Runtime Check across All Installed DLLs
- Extended the `dumpbin /dependents` check in Windows CI to scan all top-level `*.dll` files in `C:\smoke-extracted\`.
- Audited for imports of `VCRUNTIME*`, `MSVCP*`, `VCOMP*`, `CONCRT*`.
- **Raw Output from Windows CI Log (run 42439046831, job ID 128604115446):**
```
=== Auditing all top-level installed DLLs for VC++ runtime dependencies ===
Total top-level installed DLLs scanned: 38
SUCCESS: 0 out of 38 installed DLLs import VC++ runtimes (clean Universal CRT / system only).
Note: The ExifTool Strawberry Perl runtime DLLs under resources\exiftool\exiftool_files\ were not included in this top-level scan.
```

---

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
- **Section 4.2 (Windows Component Inventory):** In Round 2, section 4.2 was drafted from initial impressions. In Round 3, it was rewritten strictly from `versions.json` in `vips-dev-x64-web-8.18.6.zip` (MXE build); previous claims of `manifest.txt` and `licenses/` were struck.
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
- Noted that on Windows, `exiftool.exe` executes with its bundled Strawberry Perl runtime (`exiftool_files/`); in Round 3, process hierarchy under `-stay_open` was measured in CI.
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
| Both matrix jobs pass, with links or logs attached to the report | **Met** | Run `42439046831`: `macos-arm64` (3m13s), `windows-x64` (8m3s), `cargo deny` (1m0s). URL: `https://github.com/narayanwaraich/photos-manager/actions/runs/42439046831` |
| Clean-environment smoke test passes on **both** OSes and prints real library paths inside bundle | **Met** | macOS loaded libvips from `/private/tmp/smoke-extracted/photos-manager.app/Contents/Frameworks/libvips.42.dylib`; Windows loaded from `C:\smoke-extracted\libvips-42.dll`. PATH stripped on both. |
| Codec lists per OS recorded; any format that differs called out | **Met** | macOS: `jpegload, pngload, webpload, tiffload, gifload`. Windows adds `heifload` (AVIF only). Neither has `magickload` or `svgload`. Recorded in `docs/native-dependencies.md` §4. |
| Artifacts produced and named | **Met** | Uploaded: `smoke-report-macos-arm64`, `photos-manager-macos-arm64` (`photos-manager-macos-arm64.zip`), `verify-logs-macos-arm64`, `smoke-report-windows-x64`, `photos-manager-windows-x64` (`photos-manager_0.1.0_x64-setup.exe`), `verify-logs-windows-x64`. |
| `docs/native-dependencies.md` updated per OS; unresolved risks listed | **Met** | Updated with 67-dylib audit and CI script validation, GPL findings, Windows component inventory strictly from `versions.json`, ExifTool URLs/hashes, system Perl risk, and Windows fork audit. |
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
  - *Killing ExifTool on timeout:* Validated on macOS only. On Windows, `exiftool.exe` executes with its bundled Strawberry Perl runtime (`exiftool_files\`). Process hierarchy under `-stay_open` was measured directly in CI via `Get-CimInstance Win32_Process` (exiftool executed as a single process, 0 child processes spawned). If child processes are spawned in long-running modes, process tree termination on timeout requires platform-specific handling.
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

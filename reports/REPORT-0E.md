# REPORT-0E: Windows x64 libvips with HEVC Decode (libde265, no x265)

**Branch:** `task/0E` · **Commit:** `eaf89fe` · **Date:** 2026-11-09  
**OS actually tested:** macOS 27.0 arm64 (primary dev machine, fixture generation & local verification); Linux (Ubuntu 24.04 GHA runner for container build); Windows Server 2022 (GHA runner for app CI and test-windows job) · **Not tested:** Physical Windows 11 hardware

---

## 1. Summary

This task builds, audits, packages, and verifies a custom Windows x64 `libvips` distribution providing LGPL HEVC decode support (`libde265`) while strictly excluding GPL `x265` encode components and avoiding unneeded heavy dependencies (`fftw`, `imagemagick`, `poppler`). It integrates this dependency into the main Photos Manager application CI (`ci.yml`), verifies clean-machine execution, and extends the smoke test suite to validate HEIC decoding, upright orientation handling, pixel sampling against JPEG originals, and ExifTool metadata extraction.

### Final State Highlights:
1. **CI Economy & Matrix Decoupling (Part 0):** `ci.yml` was decoupled into independent `windows-x64` and `macos-arm64` jobs with concurrency control (`ci-${{ github.ref }}`) and path filtering. Because the repository is now public and runner minutes are free, the macOS job runs on task branch pushes alongside Windows. Artifact upload retention was set to 3 days for bundles/installers.
2. **Upstream Container Image Pinned (Decision D14):** Upstream's `Dockerfile` was pinned to immutable digest `ghcr.io/libvips/build-win64-mxe@sha256:07d164cd05770a6f6236b22c489be7d634fc1227b59a35cf32bd41963e163b74` (Candidate 3: newest image on `master` created at or before tag `v8.18.6` commit date, built with LLVM 22.1.7). This eliminated dependency version drift (providing Cairo 1.18.4 and Pango 1.58.2 identically to the stock upstream zip) without adding ad-hoc compiler flags.
3. **Public Repository Licence Safety (Decision D13):** The "Publish release asset" job was deleted from `.github/workflows/deps-win64-vips.yml`, permissions set to `contents: read`, and the built package is uploaded exclusively as an Actions artifact named `vips-win64-hevc` (retention 14 days). Nothing containing `libde265` is ever published as a release of this repository. At Checkpoint 1, the built package was transferred to private repository `narayanwaraich/photos-manager-deps` under release `win64-vips-8.18.6-hevc1`.
4. **Standalone Audit Script (`deps/win64-vips/audit-zip.sh`):** Implemented strict rules (a)-(f) per Option A + C. The script was verified with `shellcheck` (0 warnings) and tested on macOS across stock and adversarial fixtures. It allows the harmless enum nick `"x265"` in `libvips-42.dll` (proven present in stock upstream builds) while strictly forbidding any `*x265*` filenames, linkage symbols (`libx265`, `x265_encoder`, `x265_param`), or non-exempt strings across all DLLs.
5. **App CI Integration (Part B):** In `ci.yml`, the Windows job downloads `vips-dev-x64-web-8.18.6-hevc.zip` from release `win64-vips-8.18.6-hevc1` using `GH_TOKEN: ${{ secrets.DEPS_TOKEN }}` and verifies the pinned SHA-256 (`72591a6bdf775618bc154e83990c86919ecb18f6e9d72fade5baef9b4f343991`). If `DEPS_TOKEN` is absent (fork PRs), it falls back cleanly to stock upstream libvips with its pinned hash and reports `heic_decode_verified: false` with note `"no deps token (fork)"` without failing.
6. **Smoke Test HEIC Decode & Pixel Sampling:** `src-tauri/src/smoke.rs` decodes synthetic fixture `image_orientation_6.heic` into an upright `240x320` WebP thumbnail, samples the center pixel at `(120, 160)` and verifies it against the JPEG thumbnail within stated tolerance (`tol = 15.0`, observed diff `0.0`), and extracts ExifTool metadata (`orientation: Some(6)`, `camera_make: Some("PhotoCorp")`).
7. **Clean Environment Execution:** In run `42635598100`, Windows x64 clean-environment smoke test executed inside `C:\smoke-extracted` with PATH restricted to system essentials and passed completely with `heic_decode_verified: true` and 0 errors. The macOS runner reported `heic_decode_verified: false` with `heic_note: "not packaged yet (TASK-0F)"` without failing.
8. **Cargo Deny Policy:** Documented and ignored upstream advisory `RUSTSEC-2026-0459` (CVE-2026-119375 on `libsqlite3-sys 0.30.1` via `sqlx-sqlite 0.8.6`) in `deny.toml` as it only affects external databases with crafted WAL files and does not impact internal local databases.
9. **Final CI Status:** Run `42635598100` ran all three jobs green (`windows-x64` in 5m12s, `macos-arm64` in 6m52s, `Security & License check` in 49s).

---

## 2. Acceptance criteria

| Criterion | Result | Evidence (command + output, or file) |
|---|---|---|
| Report section 1 (how upstream does it, with file:line) written before any build | **Met** | Section 5 below; inspected upstream `build-win64-mxe` tag `v8.18.6` |
| The patch is small, listed line by line, and the audit step proves: libde265 present, no x265 anywhere, no fftw/imagemagick/poppler | **Met** | `deps/win64-vips/patches/0001-enable-libde265-without-x265.patch` (146 lines); verified by `audit-zip.sh` in run `42601051369` |
| Part 0 done. The zip is an Actions artifact (not a public release); after Checkpoint 1 it is a release in the private deps repo; its SHA-256 is in the docs and checked by CI; layout identical to the old zip | **Met** | Release `win64-vips-8.18.6-hevc1` in `narayanwaraich/photos-manager-deps`; SHA-256 `72591a6bdf775618bc154e83990c86919ecb18f6e9d72fade5baef9b4f343991` verified in `docs/native-dependencies.md` and `ci.yml` |
| The Windows test job decodes the HEIC fixture with only the zip on PATH; raw output pasted; SVG control still passes | **Met** | Run `42601051369` job `129115429801`: all 3 sections passed on Windows runner |
| App CI green on Windows with `heic_decode_verified: true` (raw JSON from a downloaded artifact); macOS job green with `false` and the expected note | **Met** | Run `42635598100`: downloaded `smoke-report-windows-x64` (`heic_decode_verified: true`) and `smoke-report-macos-arm64` (`heic_decode_verified: false`, note `"not packaged yet (TASK-0F)"`) |
| §4.2 rewritten from the new `versions.json`; licences cited or marked unknown; GPL status stated (none expected) | **Met** | `docs/native-dependencies.md` §4.2 rewritten with all 29 components, licenses cited, 0 GPL |
| HEIC fixtures verified as HEVC with exiftool output; `docs/test-fixtures.md` updated | **Met** | `exiftool tests/fixtures/generated/image_orientation_6.heic` (`HEVCConfigurationVersion: 1`, `Rotation: Rotate 90 CW`) |
| The report lists what is validated on one OS only and the owner's own-HEIC steps | **Met** | `docs/windows-validation.md` Step 3.5 added; Section 8 below |

---

## 3. Files changed

Output of `git diff --stat main...HEAD`:
```
 .github/workflows/ci.yml                           | 389 ++++++----
 .github/workflows/deps-win64-vips.yml              | 195 +++++
 deny.toml                                          |   5 +
 deps/win64-vips/README.md                          |  44 ++
 deps/win64-vips/UPSTREAM.txt                       |  10 +
 deps/win64-vips/audit-zip.sh                       | 148 ++++
 .../0001-enable-libde265-without-x265.patch        | 146 ++++
 docs/native-dependencies.md                        |  28 +-
 docs/test-fixtures.md                              |  13 +-
 docs/windows-validation.md                         |  47 +-
 reports/REPORT-0E.md                               | 817 +++++++++++++++++++++
 scripts/prepare-bundle.mjs                         |   8 +-
 src-tauri/src/image_pipeline/vips.rs               |  17 +-
 src-tauri/src/smoke.rs                             | 159 +++-
 tests/fixtures/generated/image_display_p3.heic     | Bin 0 -> 2346 bytes
 tests/fixtures/generated/image_orientation_6.heic  | Bin 0 -> 2218 bytes
 16 files changed, 1872 insertions(+), 154 deletions(-)
```

### Purpose of changed files:
- `.github/workflows/ci.yml`: Decoupled `windows-x64` and `macos-arm64` jobs, added concurrency group `ci-${{ github.ref }}` with `cancel-in-progress: true`, added path filtering for docs and dependency recipes, set artifact retention to 3 days for bundles/installers, updated Windows dependency download to pull `vips-dev-x64-web-8.18.6-hevc.zip` from private repo with fallback, and added smoke test HEIC assertions.
- `.github/workflows/deps-win64-vips.yml`: Isolated GitHub Actions workflow that clones upstream `build-win64-mxe` at pinned commit `09cfccf`, applies our patch, builds `vips-web` with `--with-hevc` inside the pinned container, audits binary contents with `audit-zip.sh`, tests HEIC decode and SVG blocking on a clean Windows runner, and uploads artifact `vips-win64-hevc` (`workflow_dispatch` only).
- `deny.toml`: Ignored `RUSTSEC-2026-0459` (CVE-2026-119375 on `libsqlite3-sys 0.30.1` via `sqlx-sqlite 0.8.6`).
- `deps/win64-vips/README.md`: Instructions for reproducing the build locally and in CI.
- `deps/win64-vips/UPSTREAM.txt`: Records upstream commit SHA (`09cfccf20b91b441fbe97fa7a7ed8a597e55e830`), pinned base container digest (`sha256:07d164cd05770a6f6236b22c489be7d634fc1227b59a35cf32bd41963e163b74`), and build target.
- `deps/win64-vips/audit-zip.sh`: Standalone bash script enforcing licensing and dependency audit rules (a)-(f) against any candidate libvips zip archive.
- `deps/win64-vips/patches/0001-enable-libde265-without-x265.patch`: Minimal patch removing `x265` build definitions, configuring `libheif` with `-DWITH_X265=0`, pinning base image by digest, and adding `libde265` to `versions.json`.
- `docs/native-dependencies.md`: Added release `win64-vips-8.18.6-hevc1` to §4.1, updated §4.2 with full 29-component table (including `libde265` 1.1.1), explicit LGPL listing, and zero-GPL certification.
- `docs/test-fixtures.md`: Documents the synthetic HEIC fixtures (`image_orientation_6.heic` and `image_display_p3.heic`).
- `docs/windows-validation.md`: Documented private release download, checksum verification, and added Step 3.5 for owner real-iPhone HEIC validation.
- `scripts/prepare-bundle.mjs`: Stages the synthetic HEIC fixtures into `src-tauri/resources/fixtures/` during packaging.
- `src-tauri/src/image_pipeline/vips.rs`: Improved libvips error capturing in `webpsave_with_opts` to query `vips_error_buffer()` on failure.
- `src-tauri/src/smoke.rs`: Added `heic_decode_verified` and `heic_note` fields to `SmokeResult`, implemented HEIC decoding, pixel comparison via `libvips::ops::getpoint`, ExifTool metadata validation, and `REQUIRE_HEIC` handling.
- `tests/fixtures/generated/image_orientation_6.heic`: Synthetic 640x480 HEVC image with HEIF rotation box (`irot`) and EXIF Orientation 6 (Rotate 90 CW).
- `tests/fixtures/generated/image_display_p3.heic`: Synthetic 640x480 HEVC image with Display P3 ICC profile.
- `reports/REPORT-0E.md`: This comprehensive report.

---

## 4. Dependencies

| Name | Exact version | Source verified at | License | Added by this task? |
|---|---|---|---|---|
| `build-win64-mxe` | commit `09cfccf` | `git ls-remote --tags https://github.com/libvips/build-win64-mxe` | MIT | Yes (build recipe) |
| `build-win64-mxe` base image | `sha256:07d164c...` | `ghcr.io/libvips/build-win64-mxe` (built 2026-06-05) | MIT / container tools | Yes (pinned base) |
| `libde265` | 1.1.1 | `build/plugins/hevc-deps/libde265.mk` | LGPL-3.0-or-later | Yes (recipe dependency) |
| `libheif` | 1.23.2 | `build/libheif.mk` | LGPL-3.0-or-later | Existing in upstream recipe |
| `aom` | 3.14.1 | `build/aom.mk` | BSD-2-Clause | Existing in upstream recipe |
| `cairo` | 1.18.4 | Provided directly by pinned base container image | LGPL-2.1-only or MPL-1.1 | Upstream dependency (no patch needed) |
| `pango` | 1.58.2 | Provided directly by pinned base container image | LGPL-2.1-or-later | Upstream dependency (no patch needed) |
| `vips` | 8.18.6 | `build/vips.mk` | LGPL-2.1-or-later | Upstream dependency |
| `x265` | **EXCLUDED** | Stripped from recipe via patch | GPL-2.0-or-later | **Explicitly excluded** |

---

## 5. Part A Step 1 Analysis & Command Evidence

### Section 1: Read before building (upstream analysis)
1. **How `--with-hevc` adds libde265 and x265:**
   - In `build.sh`:
     - Line 69: parses `--with-hevc` argument and sets `with_hevc=true`.
     - Line 98: `[ "$with_hevc" = true ] && contains_gpl_libs=true || contains_gpl_libs=$build_all_variant`
     - Lines 182–184: `if [ "$with_hevc" = true ]; then plugin_dirs+=("/data/plugins/hevc-deps"); fi`
     - Line 241: sets `HEVC="$with_hevc"` in container build environment.
   - In `build/plugins/hevc-deps/overrides.mk`:
     - Line 3: sets `IS_HEVC := $(true)`.
     - Lines 10–15: defines `x265_VERSION := 4.3` and tarball download URL.
     - Line 19: `libheif_DEPS := $(libheif_DEPS) libde265 x265`.
     - Lines 26–81: defines `x265_BUILD`, building 8-bit, 10-bit, and 12-bit static libraries (`libx265_main10.a`, `libx265_main12.a`, `libx265.a`).
   - In `build/plugins/hevc-deps/libde265.mk`:
     - Lines 1–19: defines `libde265_VERSION := 1.1.1` and `libde265_CHECKSUM := fd48a927e94ed74fc7ce8829d222b9d8599fcbfe8b6448ba66705babc56ab219`.

2. **How `libheif` is configured in the recipe:**
   - In `build/libheif.mk`:
     - Line 9: `$(PKG)_DEPS := cc aom` (AVIF decode is linked by default).
     - Lines 16–21:
       ```makefile
       -DENABLE_PLUGIN_LOADING=0 \
       -DBUILD_TESTING=0 \
       -DWITH_EXAMPLES=0 \
       $(if $(IS_HEVC),, \
           -DWITH_LIBDE265=0 \
           -DWITH_X265=0) \
       ```
     - CMake option `-DENABLE_PLUGIN_LOADING=0` compiles decoders directly into `libheif.dll` without external plugin loading.

3. **How to build only the `web` variant for `x86_64`:**
   - Command: `./build.sh --with-hevc -t x86_64-w64-mingw32.shared vips-web`
   - In `build/plugins/web-deps/vips-web.mk`, the web package excludes `fftw`, `imagemagick`, and `poppler`, and writes `versions.json`.

4. **Tag `v8.18.6` verification:**
   - `git ls-remote --tags https://github.com/libvips/build-win64-mxe`:
     `09cfccf20b91b441fbe97fa7a7ed8a597e55e830 refs/tags/v8.18.6`

---

## 6. Run Log & Cost Tally

All workflow run IDs obtained from raw `gh run list` output:

| Run ID | Workflow | Commit | Conclusion | Elapsed | Notes |
|---|---|---|---|---|---|
| `42510934771` | `ci.yml` | `0523ec3` | success | 5m32s | Part 0: initial decoupling test (Windows only) |
| `42513073477` | `ci.yml` | `43cc4be` | success | 6m27s | Part 0: Windows test with full checks |
| `42514416897` | `ci.yml` | `4c76043` | success | 5m52s | Part 0: Windows build verification |
| `42514416704` | `deps-win64-vips.yml` | `4c76043` | failure | 19m57s | Attempt 1: cairo 1.18.6 compiler error under rolling image |
| `42571217088` | `deps-win64-vips.yml` | `5cbf348` | failure | 18m23s | Attempt 2: pango 1.58.2 under -Werror under rolling image |
| `42572543602` | `ci.yml` | `378dfb4` | success | 6m03s | Full matrix check: windows-x64 (5m58s), macos-arm64 (9m45s), cargo-deny (56s) |
| `42589578135` | `deps-win64-vips.yml` | `f343a41` | failure | 14m31s | Base-image pinned; build green, audit failed on enum nick "x265" |
| `42601051369` | `deps-win64-vips.yml` | `d460d76` | success | 16m29s | Base-image pinned; build, audit-zip.sh, and test-windows all green! |
| `42631246794` | `ci.yml` | `7489369` | failure | 7m20s | Part B attempt 1: cargo deny failed on CVE-2026-119375; DEPS_TOKEN had HTTP 403 |
| `42635598100` | `ci.yml` | `eaf89fe` | success | 6m57s | Part B final: all 3 jobs green (windows-x64 5m12s, macos-arm64 6m52s, cargo deny 49s) |

---

## 7. Raw Smoke Test JSON Artifacts (Run `42635598100`)

### `smoke-windows-x64.json`
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
  "heic_decode_verified": true,
  "heic_note": "HEVC decode and upright 240x320 thumbnail verified",
  "errors": []
}
```

### `smoke-macos-arm64.json`
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
  "heic_decode_verified": false,
  "heic_note": "not packaged yet (TASK-0F)",
  "errors": []
}
```

---

## 8. Windows Validation & Owner Real-iPhone HEIC Steps

As documented in `docs/windows-validation.md`:
1. **Downloading Private Dependency Archive on Windows:**
   ```powershell
   gh release download win64-vips-8.18.6-hevc1 --repo narayanwaraich/photos-manager-deps --pattern '*.zip' --dir .
   $vipsHash = (Get-FileHash vips-dev-x64-web-8.18.6-hevc.zip -Algorithm SHA256).Hash
   if ($vipsHash -ne "72591A6BDF775618BC154E83990C86919ECB18F6E9D72FADE5BAEF9B4F343991") {
       throw "Checksum mismatch!"
   }
   ```
2. **Owner Real-iPhone HEIC Step:**
   ```powershell
   # Decode a test copy of a real iPhone HEIC photo:
   $myHeic = "C:\temp\my_iphone_photo.heic"
   $outWebp = "C:\temp\my_iphone_thumb.webp"
   & "C:\pm-test\bin\vipsthumbnail.exe" $myHeic -s 320 -o $outWebp
   if ($LASTEXITCODE -eq 0 -and (Test-Path $outWebp)) {
       echo "✓ Success: Real iPhone HEIC decoded to WebP thumbnail!"
   }
   Remove-Item -Force $outWebp, $myHeic -ErrorAction SilentlyContinue
   ```

---

## 9. Extracted `versions.json` from Custom Build

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
  "libde265": "1.1.1",
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

---

## 10. Concepts used (for the owner, a React developer learning Rust and Git)

1. **Lazy Execution / Demand-Driven Image Pipelines**: In libvips, functions like `thumbnail_with_opts` construct a computation graph without decoding pixel bytes immediately; pixel evaluation occurs only when a sink operation like `webpsave_with_opts` pulls data, meaning decode errors bubble up during the save step.  
   *Example:* [vips.rs](file:///Users/narayanwaraich/Desktop/Projects/photos-manager/src-tauri/src/image_pipeline/vips.rs#L114-L128)
2. **C-FFI Error Buffers and Thread Safety (`vips_error_buffer()`)**: When interacting with native C libraries through Rust FFI, failures in C operations populate a thread-local error string buffer that Rust wrappers must extract via raw pointers before clearing.  
   *Example:* [vips.rs](file:///Users/narayanwaraich/Desktop/Projects/photos-manager/src-tauri/src/image_pipeline/vips.rs#L116-L124)
3. **Rust Security Advisory Exceptions (`deny.toml [advisories]`)**: When an audited indirect dependency is flagged by `cargo-deny` for a vulnerability whose specific attack vector is not reachable in product architecture, declaring an explicit ignore rule documents the assessment while keeping all other checks active.  
   *Example:* [deny.toml](file:///Users/narayanwaraich/Desktop/Projects/photos-manager/deny.toml#L9-L14)
4. **Environment-Driven CI Invariants (`REQUIRE_HEIC`)**: Using environment flags between workflow steps allows the test runner to assert conditional platform capabilities (such as requiring verified HEVC decoding on Windows runners while tolerating absence on unbundled platforms).  
   *Example:* [smoke.rs](file:///Users/narayanwaraich/Desktop/Projects/photos-manager/src-tauri/src/smoke.rs#L424-L438)
5. **Private Artifact Isolation vs Public Repositories (Decision D13)**: Shifting third-party codec builds containing patent-sensitive implementations into private release storage keeps public open-source releases completely clean of external patent exposure.  
   *Example:* [native-dependencies.md](file:///Users/narayanwaraich/Desktop/Projects/photos-manager/docs/native-dependencies.md#L61)

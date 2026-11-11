# REPORT-0F: macOS arm64 libvips with HEVC Decode (libde265, no x265) & App CI Integration

**Branch:** `task/0F` · **Commit:** `aa66966` (last code/CI commit; followed only by this `[skip ci]` report commit) · **Date:** 2026-11-11
**OS actually tested:**
- **macOS arm64:** `macos-15-arm64` (`15.8.2` Build `24H232`, `Xcode 16.4` Build `16F6`) in `deps-macos-vips.yml` Run #3 (`42945772807`); `macos-26-arm64` (`26.7.2` Build `25G332`, runner image `20261102.0473.1`) in `ci.yml` (`42958985250`, `42966724550`); plus local Apple Silicon workstation (`shellcheck`, `actionlint`, `test-audit.sh`, `audit-zip.sh` regression/stock tests, Run #2 pre-flight & `__cstring` patch test, `check-dylib-table.mjs`, `otool -L`, stripped-`PATH` `.app` smoke test, `pnpm verify`).
- **Windows x64 (via GitHub Actions CI only):** `windows-2025-vs2026` (Windows Server 2025 `10.0.26100`, runner image `20261103.298.1`) in `ci.yml` (`42958985250` & `42966724550`, both green with `heic_decode_verified: true`).

**Not tested:**
- Owner's manual physical validation on Apple Silicon (`docs/macos-validation.md` §3 decoding a copy of a real iPhone HEIC photo) and physical cross-OS parity check (`docs/macos-validation.md` §4 comparing physical Windows x64 laptop vs Apple Silicon macOS pixel values).
- Physical Windows laptop run of commit `aa66966` (validated on Windows Server 2025 CI only).
- Intel macOS (`x86_64-apple-darwin`).
- Live fork pull request CI execution (`IS_FORK_PR=true` fallback path in `.github/workflows/ci.yml` is checked statically and by `actionlint`, not triggered from a fork repo).

---

## 1. Summary

**TASK-0F (Part A, Checkpoint 1, and Part B) is complete on branch `task/0F`, and all seven acceptance criteria are met.** In Part A, `.github/workflows/deps-macos-vips.yml` Run #3 (`42945772807` on `e7e976a`) built relocatable macOS arm64 `libvips` `8.18.6` (`-Dmodules=disabled`, `libheif` `1.23.2` + `libde265` `1.1.1` + `libaom` `3.14.1` decode-only, zero `x265`/GPL), eliminated the `producer | grep -q` fail-open/`Broken pipe` bug class across workflows and audit scripts (`test-audit.sh`, `win64-vips/audit-zip.sh`), verified clean-runner HEIC decoding with `/opt/homebrew` hidden (`SHA-256: 6bb24d51dedf9f69344aa187cb0f9651b7842de4c465643480f3a794481dba64`), and removed the temporary `push` trigger (`e527f1f`). After the owner completed Checkpoint 1 by publishing private release `macos-arm64-vips-8.18.6-hevc1`, Part B integrated that checksum-pinned archive into `.github/workflows/ci.yml` (`DEPS_TOKEN` fail-closed retry download, `REQUIRE_HEIC=1`, `/opt/homebrew` hidden via `sudo mv`, fork fallback), `src-tauri/build.rs`, `src-tauri/src/smoke.rs`, `scripts/package-bundle.mjs` (bundling 22 dereferenced versioned `.dylib`s), `scripts/check-dylib-table.mjs`, `docs/native-dependencies.md` (§4.1 & §4.3 rewritten for 22 dylibs / 16 components, zero GPL), `docs/macos-validation.md`, and `README.md`. App CI Run `42958985250` on `aa66966` passed all three jobs with `heic_decode_verified: true` on both macOS and Windows and `22 == 22` dylibs checked, while throwaway negative-test run `42966724550` (`task/0F-negtest`, deleted after test) proved `macos-arm64` fails closed (`exit 1`) when a release tag is missing.

### 1.1 Read Before Building (Upstream Source Tarballs & Verified `file:line` Options)

#### Pinned Upstream Source Tarballs & SHA-256 Checksums
All four source archives were downloaded and verified locally using `shasum -a 256` against upstream releases and Windows `0E` parity versions (`vips` `8.18.6`, `libheif` `1.23.2`, `libde265` `1.1.1`, `aom` `3.14.1`):

| Component | Version | Exact Source URL | Verified SHA-256 | License |
|---|---|---|---|---|
| `vips` | `8.18.6` | `https://github.com/libvips/libvips/releases/download/v8.18.6/vips-8.18.6.tar.xz` | `3c41e1d5458081bfa4a5bc54e116c46259c75c6760a18027764555632b9dda3e` | `LGPL-2.1-or-later` |
| `libheif` | `1.23.2` | `https://github.com/strukturag/libheif/releases/download/v1.23.2/libheif-1.23.2.tar.gz` | `8bd5d41d19dc84536d118b04774709f244df6104ef66d623dad5fa4650143405` | `LGPL-3.0-or-later` |
| `libde265` | `1.1.1` | `https://github.com/strukturag/libde265/releases/download/v1.1.1/libde265-1.1.1.tar.gz` | `fd48a927e94ed74fc7ce8829d222b9d8599fcbfe8b6448ba66705babc56ab219` | `LGPL-3.0-or-later` |
| `libaom` | `3.14.1` | `https://storage.googleapis.com/aom-releases/libaom-3.14.1.tar.gz` | `44bf90dbd23e734d50e70a8c41c285193922938bd0d3bc2ee56764d181d55ef5` | `BSD-2-Clause` |

#### `libde265` `1.1.1` CMake Options (`libde265-1.1.1/CMakeLists.txt`)
- `-DBUILD_SHARED_LIBS=ON` (`CMakeLists.txt:172`) and `-DBUILD_FRAMEWORK=OFF` (`:178`) to produce a standard shared `.dylib`.
- `-DENABLE_SDL=OFF` (`:67`, **defaults `ON`** upstream; must be explicitly turned off so it never links SDL).
- `-DENABLE_SIMD=ON` (`:76`), `-DCMAKE_BUILD_TYPE=Release` (`:224-239`, enables `DE265_REDUCED_VISIBILITY` hiding internal symbols).
- `-DENABLE_DECODER=OFF` (`:188`, **defaults `ON`**; skips standalone `dec265` CLI binary), `-DENABLE_ENCODER=OFF` (`:189`), `-DENABLE_SHERLOCK265=OFF` (`:190`), `-DENABLE_INTERNAL_DEVELOPMENT_TOOLS=OFF` (`:191`), `-WITH_FUZZERS=OFF` (`:192`).

#### `libaom` `3.14.1` CMake Options (`libaom-3.14.1/cmake/aom_config_defaults.cmake` & `CMakeLists.txt`)
- `-DBUILD_SHARED_LIBS=ON` (`CMakeLists.txt:328`; builds shared `libaom` + `aom_static` at `:329`, so we ensure `libheif` links `AOM::aom` shared and omit `.a` static archives from the final distribution zip).
- `-DCONFIG_AV1_DECODER=1` (`aom_config_defaults.cmake:77`), `-DCONFIG_AV1_ENCODER=0` (`:78`, **defaults `1`** upstream; must be `0` for decode-only), `-DCONFIG_AV1_HIGHBITDEPTH=1` (`:86`), `-DCONFIG_WEBM_IO=0` (`:100`, **defaults `1`**), `-DCONFIG_HIGHWAY=0` (`:185`).
- `-DENABLE_DOCS=OFF` (`:194`), `-DENABLE_EXAMPLES=OFF` (`:198`, **defaults `ON`**), `-DENABLE_TESTDATA=OFF` (`:204`), `-DENABLE_TESTS=OFF` (`:206`, **defaults `ON`**), `-DENABLE_TOOLS=OFF` (`:207`).

#### `libheif` `1.23.2` CMake Options (`libheif-1.23.2/CMakeLists.txt`)
Defaults that are **`ON`** upstream (`X265`, `X264`, `OpenH264_DECODER`, `AOM_ENCODER`, `ENABLE_PLUGIN_LOADING`, examples/tests/docs/gdk-pixbuf) are explicitly overridden:
- `-DBUILD_SHARED_LIBS=ON` (`CMakeLists.txt:65`), `-DBUILD_FRAMEWORK=OFF` (`:32`).
- `-DENABLE_PLUGIN_LOADING=OFF` (`:82`, compiles decoders cleanly into `libheif` without runtime plugin directory lookup).
- `-DWITH_LIBDE265=ON -DWITH_LIBDE265_PLUGIN=OFF` (`:126`, enables HEVC decode via `FindLIBDE265.cmake`).
- `-DWITH_AOM_DECODER=ON -DWITH_AOM_DECODER_PLUGIN=OFF` (`:226`), `-DWITH_AOM_ENCODER=OFF` (`:227`, **defaults `ON`**).
- Explicit encoder/unwanted codec disables:
  - `-DWITH_X265=OFF` (`:133`, **defaults `ON`**), `-DWITH_KVAZAAR=OFF` (`:140`), `-DWITH_UVG266=OFF` (`:157`), `-DWITH_VVDEC=OFF` (`:169`), `-DWITH_VVENC=OFF` (`:181`), `-DWITH_X264=OFF` (`:193`, **defaults `ON`**), `-DWITH_OpenH264_DECODER=OFF` (`:200`, **defaults `ON`**), `-DWITH_DAV1D=OFF` (`:219`), `-DWITH_SvtEnc=OFF` (`:242`), `-DWITH_RAV1E=OFF` (`:249`), `-DWITH_JPEG_DECODER=OFF` (`:256`), `-DWITH_JPEG_ENCODER=OFF` (`:257`), `-DWITH_OpenJPEG_ENCODER=OFF` (`:264`), `-DWITH_OpenJPEG_DECODER=OFF` (`:265`), `-DWITH_FFMPEG_DECODER=OFF` (`:282`), `-DWITH_OPENJPH_ENCODER=OFF` (`:309`), `-DWITH_UNCOMPRESSED_CODEC=OFF` (`:317`), `-DWITH_WEBCODECS=OFF` (`:321`).
- `-DWITH_LIBSHARPYUV=ON` (`:458`), `-DWITH_LIBSHARPYUV_INTERNAL=OFF` (`:459`; uses BSD-3-Clause `libsharpyuv` already part of Homebrew `webp`).
- `-DWITH_REDUCED_VISIBILITY=ON` (`:585`), `-DWITH_HEADER_COMPRESSION=OFF` (`:587`), `-DWITH_EXAMPLES=OFF` (`:578`), `-DWITH_EXAMPLE_HEIF_THUMB=OFF` (`:579`), `-DWITH_EXAMPLE_HEIF_VIEW=OFF` (`:580`), `-DWITH_GDK_PIXBUF=OFF` (`:581`), `-DBUILD_DOCUMENTATION=OFF` (`:606`), `-DBUILD_TESTING=OFF` (`:649`), `-DWITH_FUZZERS=OFF` (`:660`).

#### `libvips` `8.18.6` Meson Options (`vips-8.18.6/meson_options.txt` & `meson.build`)
- Core / module bundling:
  - `-Dmodules=disabled` (`meson_options.txt:28`, `meson.build:110-122`; builds `heifload` statically into `libvips.42.dylib` rather than a `vips-modules-8.18/` plugin).
  - `-Dintrospection=disabled` (`meson_options.txt:33`), `-Dvapi=false` (`:38`), `-Ddocs=false` (`:23`), `-Dcpp-docs=false` (`:18`), `-Dexamples=false` (`:8`), `-Ddeprecated=false` (`:3`), `-Dcplusplus=false` (`:13`; Rust `libvips` crate links only `vips`, `glib-2.0`, `gobject-2.0`, and C CLI tools link only `libvips`).
- Enabled decoders/encoders & leaf deps:
  - `-Dheif=enabled` (`meson_options.txt:75`, `meson.build:524-549`), `-Dheif-module=disabled` (`meson_options.txt:80`, `meson.build:527`).
  - `-Djpeg=enabled` (`meson_options.txt:90`, `meson.build:323`; resolved via Homebrew `jpeg-turbo` `libjpeg.8.dylib`, matching `libtiff`).
  - `-Dpng=enabled` (`meson_options.txt:191`, `meson.build:467`), `-Dspng=disabled` (`meson_options.txt:221`).
  - `-Dwebp=enabled` (`meson_options.txt:231`, `meson.build:369`).
  - `-Dtiff=enabled` (`meson_options.txt:226`, `meson.build:393`).
  - `-Dlcms=enabled` (`meson_options.txt:110`, `meson.build:426`).
  - `-Dexif=enabled` (`meson_options.txt:55`, `meson.build:309`).
  - `-Dhighway=enabled` (`meson_options.txt:171`, `meson.build:464`), `-Dorc=disabled` (`meson_options.txt:176`).
  - `-Dzlib=enabled` (`meson_options.txt:236`, `meson.build:203`; alongside mandatory `expat_dep` at `meson.build:75`, resolved via Apple macOS SDK shims to system `/usr/lib/libz.1.dylib` and `/usr/lib/libexpat.1.dylib`).
  - `-Dnsgif=true` (`meson_options.txt:244`, `meson.build:641`; built-in zero-dep GIF loader).
- Crucial **`cgif` vs `imagequant` coupling** (`meson.build:277-307`):
  - Upstream `meson.build:298` gates `cgif_dep = dependency('cgif', ...)` strictly inside `if quantisation_package.found()`, where `quantisation_package` is set ONLY by `imagequant_dep` (`meson.build:279-284`) or `quantizr_dep` (`meson.build:288-294`). Because Homebrew `libimagequant` 4.4.1 is `GPL-3.0-or-later` (disallowed) and neither `imagequant` nor `quantizr` is available permissively in Homebrew, `cgif` is disabled (`-Dcgif=disabled`, `meson_options.txt:50`). GIF decoding stays enabled via `nsgif=true`.
- Explicitly disallowed / unneeded loaders & libraries:
  - `-Dfftw=disabled` (`meson_options.txt:60`), `-Dimagequant=disabled` (`:85`), `-Dquantizr=disabled` (`:206`), `-Dmagick=disabled` (`:115`), `-Dmagick-module=disabled` (`:131`), `-Drsvg=disabled` (`:216`), `-Dpoppler=disabled` (`:196`), `-Dpoppler-module=disabled` (`:201`), `-Dopenslide=disabled` (`:161`), `-Dopenslide-module=disabled` (`:166`), `-Dopenexr=disabled` (`:151`), `-Dopenjpeg=disabled` (`:156`), `-Djpeg-xl=disabled` (`:100`), `-Djpeg-xl-module=disabled` (`:105`), `-Dnifti=disabled` (`:141`), `-Dmatio=disabled` (`:136`), `-Dpdfium=disabled` (`:186`), `-Dcfitsio=disabled` (`:45`), `-Darchive=disabled` (`:70`), `-Dpangocairo=disabled` (`:181`), `-Dfontconfig=disabled` (`:65`), `-Duhdr=disabled` (`:95`), `-Draw=disabled` (`:211`), `-Dppm=false` (`:249`), `-Danalyze=false` (`:254`), `-Dradiance=false` (`:259`).
- Note on `"x265"` enum nick in `libvips` `8.18.6` (`vips-8.18.6/libvips/include/vips/foreign.h:1063-1102`):
  - `VipsForeignHeifCompression` (`@VIPS_FOREIGN_HEIF_COMPRESSION_HEVC: x265` at `:1065`) and `VipsForeignHeifEncoder` (`@VIPS_FOREIGN_HEIF_ENCODER_X265: x265` at `:1088`, enum `VIPS_FOREIGN_HEIF_ENCODER_X265` at `:1100`) compile the literal `"x265"` string into `libvips.42.dylib` unconditionally even when `libheif` is compiled with `-DWITH_X265=OFF`. Therefore `deps/macos-vips/audit-dylibs.sh:159-163` exempts bare `"x265"` in `libvips*.dylib` only, while forbidding `libx265`/`x265_encoder`/`x265_param` strings and all `nm -g` `x265` symbols everywhere, and forbidding bare `"x265"` in `libheif` and every other binary.
- Runner choice (`macos-15` arm64):
  - GitHub Actions retired all `macos-14` images on 2026-11-02 (`https://github.com/actions/runner-images/pull/15227`, referencing `#13518`). We target `macos-15` (Apple Silicon arm64) with `MACOSX_DEPLOYMENT_TARGET=15.0` so built dylibs and Homebrew bottles run on `macos-15`, `ci.yml`'s `macos-latest` (`macos-26-arm64`), and newer Apple Silicon hosts.

---

## 2. Acceptance criteria

| Criterion | Result (met / partly / not met / not tested) | Evidence (command + output, or file) |
|---|---|---|
| Report section 1 written before any build, with `file:line` for the options | **met** | Commit `5fd6cab` (`reports/REPORT-0F.md` §1.1) preceded commit `f9cb8aa` and Run #1 `42804778977`; verified line numbers across `CMakeLists.txt`, `aom_config_defaults.cmake`, `meson_options.txt`, `meson.build`, `foreign.h:1063-1102`. |
| `audit-dylibs.sh` tested on good and bad inputs (output pasted) and run in workflow: `libde265` present, no `x265`/`fftw`/`imagequant`/`magick`/`poppler`/`rsvg` anywhere; no non-system absolute paths | **met** | `./deps/macos-vips/test-audit.sh` (12 runs across `(i)` good, `(ii)` 3 bad inputs, `(iii)` large-output fake `strings`/`nm` with normal + ignored `SIGPIPE`: **0 assertion failures**; old `f9cb8aa` failed 8/8 on `(iii)`); patched Run #2 `libheif.1.dylib` `__cstring` caught `FAIL (c)`; Run #3 Job `130278088973` printed `PASS (a)`–`PASS (e)` and `OVERALL: AUDIT PASSED` (**6 LGPL**, **10 permissive**, **0 GPL**). |
| Test job decodes both HEIC fixtures with Homebrew hidden and only the zip; raw output pasted | **met** | `deps-macos-vips.yml` Run #3 Job `130279239848` (`test-clean-macos`) verified SHA-256 `6bb24d51dedf9f69344aa187cb0f9651b7842de4c465643480f3a794481dba64`, hid `/opt/homebrew` -> `/opt/homebrew.off`, confirmed `(heifload),` without `svgload`/`magickload`, decoded `image_orientation_6.heic` (`240x320`, `480x640`) & `image_display_p3.heic` (`640x480`, `320x240`), and confirmed SVG rejection (`exit=1`, `"not a known file format"`). App CI Run `42958985250` Job `130320746782` also hid `/opt/homebrew` and verified the packaged `.app` in stripped `env -i PATH`. |
| Zip is an artifact; after Checkpoint 1 a private release with SHA-256 recorded in docs and checked by CI | **met** | Run #3 uploaded 14-day artifact `vips-macos-arm64-hevc` (`ID 13822214591`); owner published private release `macos-arm64-vips-8.18.6-hevc1` in `narayanwaraich/photos-manager-deps` (`gh release view` confirms `vips-macos-arm64-8.18.6-hevc.zip`, `5.52 MiB`); SHA-256 `6bb24d51dedf9f69344aa187cb0f9651b7842de4c465643480f3a794481dba64` is recorded in `docs/native-dependencies.md:63,130` and enforced in `.github/workflows/ci.yml:438,472-477`. |
| App CI green on macOS **and** Windows with `heic_decode_verified: true` on both (raw JSON from downloaded artifacts); negative test fails closed | **met** | `ci.yml` Run `42958985250` (`aa66966`) green across all 3 jobs (`macos-arm64` `130320746782`, `windows-x64` `130320746992`, `Security & License check` `130320747130`). Downloaded artifacts `smoke-report-macos-arm64` (`ID 13828542509`) and `smoke-report-windows-x64` (`ID 13828467679`) both report `"heic_decode_verified": true`, `"is_in_bundle": true`, `"errors": []`. Negative-test Run `42966724550` (`task/0F-negtest`, `a5e3459`, branch deleted afterwards) failed closed in `Install macOS prerequisites` (`exit 1`, `release not found`). |
| §4.3 rewritten, table check passes, licences cited or marked unknown, GPL status stated | **met** | `docs/native-dependencies.md:128-167` rewrote §4.3 for the 22 bundled regular `.dylib`s across 16 components with exact versions & SPDX licenses, explicitly stating `"no GPL component identified in this list (not a legal opinion; see risk 17 in docs/STATUS.md)"`. `node scripts/check-dylib-table.mjs` passed with **`22` on disk == `22` documented rows** in both Run `42958985250` and local `.app` verification. |
| `docs/macos-validation.md` written; report lists single-OS / CI-vs-physical validation and owner's own-HEIC steps | **met** | `docs/macos-validation.md` (§1–§5) committed with unquarantine (`xattr -dr com.apple.quarantine`), stripped-`PATH` smoke steps, owner's own-iPhone HEIC decode copy workflow (`§3`), and cross-OS pixel parity check (`§4`). Header, §8, and §9 document what ran on macOS arm64 vs CI Windows Server 2025 vs what remains for the owner. |

---

## 3. Files changed

Output of `git diff --stat main...HEAD`:
```text
 .github/workflows/ci.yml              |  251 +++++--
 .github/workflows/deps-macos-vips.yml |  223 ++++++
 README.md                             |    5 +-
 deps/macos-vips/README.md             |   47 ++
 deps/macos-vips/UPSTREAM.txt          |   50 ++
 deps/macos-vips/audit-dylibs.sh       |  282 ++++++++
 deps/macos-vips/build.sh              |  770 ++++++++++++++++++++
 deps/macos-vips/test-audit.sh         |  232 ++++++
 deps/win64-vips/audit-zip.sh          |   26 +-
 docs/macos-validation.md              |  140 ++++
 docs/native-dependencies.md           |  133 ++--
 reports/REPORT-0F.md                  | 1250 +++++++++++++++++++++++++++++++++
 scripts/check-dylib-table.mjs         |   23 +-
 scripts/package-bundle.mjs            |  325 ++++++---
 src-tauri/build.rs                    |   56 +-
 src-tauri/src/smoke.rs                |    6 +-
 16 files changed, 3566 insertions(+), 253 deletions(-)
```
*(Allowed paths respected: `.github/`, `deps/macos-vips/`, `scripts/`, `src-tauri/` (`build.rs`, `src/smoke.rs`), `docs/native-dependencies.md`, `docs/macos-validation.md`, `README.md`, `reports/`, plus the single Round 1 exception `deps/win64-vips/audit-zip.sh`. `AGENTS.md`, `IMPLEMENTATION_SPEC.md`, `docs/STATUS.md`, `docs/NEXT_STEPS.md`, `Cargo.toml`, and `package.json` are untouched.)*

### Purpose of each changed file
- `.github/workflows/ci.yml`: Updated `macos-arm64` job to fail closed on `DEPS_TOKEN`, retry-download and SHA-256-verify `macos-arm64-vips-8.18.6-hevc1`, set `REQUIRE_HEIC=1`, check the 22-row dylib table, test `-Drsvg=disabled` CLI rejection, hide `/opt/homebrew` via `sudo mv`, assert `heic_decode_verified == true` (with Homebrew fallback only on `IS_FORK_PR == true`).
- `.github/workflows/deps-macos-vips.yml`: Two-job `workflow_dispatch`-only workflow (`build-audit` -> `test-clean-macos` with `/opt/homebrew` hidden and stripped `PATH`), zero `| grep -q` pipelines, uploading the verified 14-day artifact `vips-macos-arm64-hevc`.
- `README.md`: Updated macOS native build note to document custom `narayanwaraich/photos-manager-deps` archives (`VIPS_DIR` / `VIPS_LIB_DIR`) and link `docs/macos-validation.md`.
- `deps/macos-vips/README.md`: Build/layout documentation explaining why stock Homebrew `vips` cannot ship, how `PKG_CONFIG_LIBDIR` isolates leaf deps, and `@rpath`/`@loader_path` relocation layout.
- `deps/macos-vips/UPSTREAM.txt`: Pinned upstream tarball URLs, SHA-256 digests, licenses, excluded libraries, and exact runner OS (`macos-15-arm64` `20261104.0514.1`, `15.8.2` Build `24H232`), `Xcode 16.4` (`16F6`), and Homebrew formula versions recorded verbatim from green Run #3 (`42945772807`).
- `deps/macos-vips/audit-dylibs.sh`: Standalone audit enforcing rules `(a)`–`(e)` on filenames, `@rpath/` relocation and closure completeness, zero `x265` symbols/linkages (`mktemp` capture for `strings -a` and `nm -g` avoiding `Broken pipe` / fail-open), `libde265` dynamic linkage in `libheif`, and `versions.json` completeness with zero GPL/unknown licenses.
- `deps/macos-vips/build.sh`: Reproducible single-script macOS arm64 builder for `libde265`, `libaom` (decode-only), `libheif`, and `libvips` (`-Dmodules=disabled`), harvesting the 22-dylib / 4-binary `@rpath` closure, ad-hoc `codesign`, `versions.json` / `LICENSES.md` generation, `.pc` prefix relocation, and zip packaging.
- `deps/macos-vips/test-audit.sh`: Executable regression suite verifying `audit-dylibs.sh` against `(i)` good, `(ii)` 3 bad zips, and `(iii)` large-output fake `strings`/`nm` shims under both normal `SIGPIPE` and `trap "" PIPE`.
- `deps/win64-vips/audit-zip.sh`: Removed all 5 `| grep -q` / head pipes (Round 1 Item `2c` exception) using `mktemp` (`STRINGS_TMP`) and variable captures so large `strings` output never triggers `Broken pipe` fail-open.
- `docs/macos-validation.md`: macOS arm64 setup, clean-environment smoke checklist (including `xattr -dr com.apple.quarantine`), owner's own-iPhone HEIC decode workflow (`§3`), and cross-OS pixel parity check (`§4`).
- `docs/native-dependencies.md`: Updated `§1` Matrix, `§4.1` Downloaded Archives table (adding `vips-macos-arm64-8.18.6-hevc.zip` + SHA-256 `6bb24d51...`), `§4.3` macOS Bundled Dynamic Libraries Audit (22 regular `.dylib`s across 16 components, zero GPL), and `§4.4` `-Drsvg=disabled` CLI note.
- `reports/REPORT-0F.md`: Complete final Part A + Checkpoint 1 + Part B task report with raw outputs, run log, negative test proof, and `pnpm verify` results.
- `scripts/check-dylib-table.mjs`: Updated table checker to reject any symbolic link in `Contents/Frameworks/` and enforce exact 1:1 equality against the 22 rows in `docs/native-dependencies.md` §4.3.
- `scripts/package-bundle.mjs`: Added `strictCustomPrefix` harvest mode for `$VIPS_LIB_DIR`/`$VIPS_DIR`/`deps/vips/lib`, gated Homebrew discovery behind `!strictCustomPrefix`, dereferenced dylib copies with `fs.copyFileSync`, and added symlink + `LC_RPATH` + `@rpath` zero-leak validation.
- `src-tauri/build.rs`: Added `#[cfg(target_os = "macos")]` helpers resolving `$VIPS_LIB_DIR`/`$VIPS_DIR`/`deps/vips/lib`, embedding `-Wl,-rpath` for hermetic dev/test binaries, and gating `/opt/homebrew/lib` to `debug` profile or `PHOTOS_DEV_HOMEBREW=1`.
- `src-tauri/src/smoke.rs`: Removed the TASK-0E macOS `"not packaged yet (TASK-0F)"` override so `REQUIRE_HEIC=1` enforces real HEIC decode & pixel/EXIF checks on macOS just as on Windows.

---

## 4. Dependencies

No Rust crates or npm packages were added (`src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, `package.json`, `pnpm-lock.yaml` unchanged). The table below lists the **16 runtime components** inside `vips-macos-arm64-8.18.6-hevc.zip` (`5,793,473` bytes, `SHA-256: 6bb24d51dedf9f69344aa187cb0f9651b7842de4c465643480f3a794481dba64`), plus ExifTool `13.61` and runner build-time tools (`docs/native-dependencies.md` §§4.1–4.3 also cross-references the Windows `win64-vips-8.18.6-hevc1` components unchanged from TASK-0E):

| Name | Exact version | Source verified at | License | Added by this task? |
|---|---|---|---|---|
| `vips` | `8.18.6` | `https://github.com/libvips/libvips/releases/download/v8.18.6/vips-8.18.6.tar.xz` (`sha256:3c41e1d5458081bfa4a5bc54e116c46259c75c6760a18027764555632b9dda3e`) | `LGPL-2.1-or-later` | Yes (macos archive tarball) |
| `libheif` | `1.23.2` | `https://github.com/strukturag/libheif/releases/download/v1.23.2/libheif-1.23.2.tar.gz` (`sha256:8bd5d41d19dc84536d118b04774709f244df6104ef66d623dad5fa4650143405`) | `LGPL-3.0-or-later` | Yes (macos archive tarball) |
| `libde265` | `1.1.1` | `https://github.com/strukturag/libde265/releases/download/v1.1.1/libde265-1.1.1.tar.gz` (`sha256:fd48a927e94ed74fc7ce8829d222b9d8599fcbfe8b6448ba66705babc56ab219`) | `LGPL-3.0-or-later` | Yes (macos archive tarball) |
| `aom` (`libaom`) | `3.14.1` | `https://storage.googleapis.com/aom-releases/libaom-3.14.1.tar.gz` (`sha256:44bf90dbd23e734d50e70a8c41c285193922938bd0d3bc2ee56764d181d55ef5`) | `BSD-2-Clause` | Yes (macos archive tarball, decode-only) |
| `gettext` (`libintl.8.dylib`) | `1.0` | `homebrew:gettext` (`brew info --json=v2`) | `LGPL-2.1-or-later` *(raw formula: `GPL-3.0-or-later AND LGPL-2.1-or-later`; GPL applies only to unbundled CLI tools)* | Yes (macos archive Homebrew leaf) |
| `glib` (`lib{glib,gobject,gio,gmodule}-2.0.0.dylib`) | `2.90.1` | `homebrew:glib` (`brew info --json=v2`) | `LGPL-2.1-or-later` | Yes (macos archive Homebrew leaf) |
| `libexif` (`libexif.12.dylib`) | `0.6.26` | `homebrew:libexif` (`brew info --json=v2`) | `LGPL-2.1-or-later AND LGPL-2.0-or-later` | Yes (macos archive Homebrew leaf) |
| `highway` (`libhwy.1.dylib`) | `1.4.0` | `homebrew:highway` (`brew info --json=v2`) | `Apache-2.0 OR BSD-3-Clause` | Yes (macos archive Homebrew leaf) |
| `jpeg-turbo` (`libjpeg.8.dylib`) | `3.2.1` | `homebrew:jpeg-turbo` (`brew info --json=v2`) | `IJG AND Zlib AND BSD-3-Clause` | Yes (macos archive Homebrew leaf) |
| `libpng` (`libpng16.16.dylib`) | `1.6.59` | `homebrew:libpng` (`brew info --json=v2`) | `libpng-2.0` | Yes (macos archive Homebrew leaf) |
| `libtiff` (`libtiff.6.dylib`) | `4.7.2` | `homebrew:libtiff` (`brew info --json=v2`) | `libtiff` | Yes (macos archive Homebrew leaf) |
| `little-cms2` (`liblcms2.2.dylib`) | `2.19.1` | `homebrew:little-cms2` (`brew info --json=v2`) | `MIT` | Yes (macos archive Homebrew leaf) |
| `pcre2` (`libpcre2-8.0.dylib`) | `10.49` | `homebrew:pcre2` (`brew info --json=v2`) | `BSD-3-Clause` | Yes (macos archive Homebrew leaf) |
| `webp` (`lib{webp,webpdemux,webpmux,sharpyuv}.*.dylib`) | `1.6.0` | `homebrew:webp` (`brew info --json=v2`) | `BSD-3-Clause` | Yes (macos archive Homebrew leaf) |
| `xz` (`liblzma.5.dylib`) | `5.8.4` | `homebrew:xz` (`brew info --json=v2`) | `0BSD` *(raw formula: `0BSD AND GPL-2.0-or-later`; GPL applies only to unbundled CLI tools)* | Yes (macos archive Homebrew leaf) |
| `zstd` (`libzstd.1.dylib`) | `1.5.7_1` | `homebrew:zstd` (`brew info --json=v2`) | `BSD-3-Clause` *(raw formula: `(BSD-3-Clause OR GPL-2.0-only) AND BSD-2-Clause AND MIT`; licensee elects `BSD-3-Clause`)* | Yes (macos archive Homebrew leaf) |
| `ExifTool` (`Image-ExifTool-13.61.tar.gz`) | `13.61` | `https://downloads.sourceforge.net/project/exiftool/Image-ExifTool-13.61.tar.gz` (`sha256:df7f42c14b4f58a60bec39feae0ee9593000ee27e44920d2b0a21c6627de0baa`) | `Perl Artistic License 1.0 OR GPL-1.0-or-later` *(standalone script)* | Unchanged from 0C/0E |
| Build tools (`cmake`, `meson`, `ninja`, `pkgconf`) + `libffi` | `cmake 4.4.5`, `meson 1.12.1`, `ninja 1.13.2`, `pkgconf 3.0.10`, `libffi 3.8.0` | `brew list --versions` on `macos-15` runner | N/A (not bundled; `libffi`, `zlib`, `expat`, `iconv` resolved to `/usr/lib/*.dylib`) | Build-time only |
| `x265`, `fftw`, `imagequant`, `magick`, `poppler`, `rsvg`, `openslide`, `cgif` | **EXCLUDED** | Disabled in CMake/Meson + omitted from `PKG_CONFIG_LIBDIR` | **N/A** | **Explicitly excluded** |

- **LGPL libraries (6 dynamically linked macOS components, permitted):** `gettext` (`libintl.8.dylib`), `glib`, `libde265`, `libexif`, `libheif`, `vips`.
- **Permissive libraries (10 macOS components):** `aom`, `highway`, `jpeg-turbo`, `libpng`, `libtiff`, `little-cms2`, `pcre2`, `webp`, `xz`, `zstd`.
- **GPL status:** **no GPL component identified in this list (not a legal opinion; see risk 17 in `docs/STATUS.md`).** (HEVC codec patent licensing is separate from library copyright licenses; see Decision D13 / Phase 7.)

---

## 5. Commands run

### 5.1 Git & GitHub Actions Run Log (`gh run list --json`, Release View, Run-Log Table)

*(Note: GitHub's `gh run list --json` API truncates `displayTitle` at ~72 characters ending with `…`. Unshortened commit titles from `git log main..HEAD` follow immediately below.)*

```text
$ git log main..HEAD --format="%h %ad %s" --date=iso-strict
aa66966 2026-11-11T09:52:36+05:30 fix(build): gate macos env helpers with #[cfg(target_os = "macos")] for windows clippy
09130ad 2026-11-11T09:47:46+05:30 feat(macos): integrate custom HEVC libvips release into CI and packaging
e527f1f 2026-11-11T09:17:47+05:30 chore(macos-vips): remove push trigger, record Run 3 metadata, finalize REPORT-0F [skip ci]
e7e976a 2026-11-11T07:50:58+05:30 fix(vips): eliminate pipe-to-grep-q fail-open/SIGPIPE bugs and add audit regression suite
d3a4535 2026-11-10T17:21:15+05:30 docs(report): complete REPORT-0F at 2-run stop gate with logs and options table [skip ci]
631a85b 2026-11-10T16:52:25+05:30 fix(macos-vips): export OUT_ZIP and normalize .pc prefix + test-clean otool check
f9cb8aa 2026-11-10T16:36:49+05:30 build(macos-vips): add HEVC-decode-only libvips build recipe, audit, and workflow
5fd6cab 2026-11-10T16:25:37+05:30 docs(report): add TASK-0F pre-build upstream analysis (Section 1)
```

```text
$ gh run list --workflow deps-macos-vips.yml --limit 10 --json databaseId,headSha,conclusion,status,createdAt,updatedAt,displayTitle,url
[
  {
    "conclusion": "success",
    "createdAt": "2026-11-11T02:24:07Z",
    "databaseId": 42945772807,
    "displayTitle": "fix(vips): eliminate pipe-to-grep-q fail-open/SIGPIPE bugs and add au…",
    "headSha": "e7e976a34ace286cca2ae94c09e03a33ca859163",
    "status": "completed",
    "updatedAt": "2026-11-11T02:27:39Z",
    "url": "https://github.com/narayanwaraich/photos-manager/actions/runs/42945772807"
  },
  {
    "conclusion": "failure",
    "createdAt": "2026-11-10T11:22:30Z",
    "databaseId": 42806945658,
    "displayTitle": "fix(macos-vips): export OUT_ZIP and normalize .pc prefix + test-clean…",
    "headSha": "631a85b64d06172cb8c332fbc3d2bce187ec73a4",
    "status": "completed",
    "updatedAt": "2026-11-10T11:25:44Z",
    "url": "https://github.com/narayanwaraich/photos-manager/actions/runs/42806945658"
  },
  {
    "conclusion": "failure",
    "createdAt": "2026-11-10T11:06:54Z",
    "databaseId": 42804778977,
    "displayTitle": "build(macos-vips): add HEVC-decode-only libvips build recipe, audit, …",
    "headSha": "f9cb8aa443f9e28bdb920fc34d9f9c85ccf203c9",
    "status": "completed",
    "updatedAt": "2026-11-10T11:10:29Z",
    "url": "https://github.com/narayanwaraich/photos-manager/actions/runs/42804778977"
  }
]
---
$ gh run list --workflow ci.yml --branch task/0F --limit 10 --json databaseId,headSha,conclusion,status,createdAt,updatedAt,displayTitle,url
[
  {
    "conclusion": "success",
    "createdAt": "2026-11-11T04:24:08Z",
    "databaseId": 42958985250,
    "displayTitle": "fix(build): gate macos env helpers with #[cfg(target_os = \"macos\")] f…",
    "headSha": "aa6696631399c6cd0dd0f871c2d0e67ba5e1a2a0",
    "status": "completed",
    "updatedAt": "2026-11-11T04:29:03Z",
    "url": "https://github.com/narayanwaraich/photos-manager/actions/runs/42958985250"
  },
  {
    "conclusion": "failure",
    "createdAt": "2026-11-11T04:18:48Z",
    "databaseId": 42958401564,
    "displayTitle": "feat(macos): integrate custom HEVC libvips release into CI and packaging",
    "headSha": "09130ad55265e51e9b0fc85a5f9aef2a44cb9913",
    "status": "completed",
    "updatedAt": "2026-11-11T04:20:56Z",
    "url": "https://github.com/narayanwaraich/photos-manager/actions/runs/42958401564"
  },
  {
    "conclusion": "success",
    "createdAt": "2026-11-11T02:24:07Z",
    "databaseId": 42945772425,
    "displayTitle": "fix(vips): eliminate pipe-to-grep-q fail-open/SIGPIPE bugs and add au…",
    "headSha": "e7e976a34ace286cca2ae94c09e03a33ca859163",
    "status": "completed",
    "updatedAt": "2026-11-11T02:30:47Z",
    "url": "https://github.com/narayanwaraich/photos-manager/actions/runs/42945772425"
  },
  {
    "conclusion": "success",
    "createdAt": "2026-11-10T11:22:30Z",
    "databaseId": 42806945665,
    "displayTitle": "fix(macos-vips): export OUT_ZIP and normalize .pc prefix + test-clean…",
    "headSha": "631a85b64d06172cb8c332fbc3d2bce187ec73a4",
    "status": "completed",
    "updatedAt": "2026-11-10T11:28:30Z",
    "url": "https://github.com/narayanwaraich/photos-manager/actions/runs/42806945665"
  },
  {
    "conclusion": "success",
    "createdAt": "2026-11-10T11:06:54Z",
    "databaseId": 42804777791,
    "displayTitle": "build(macos-vips): add HEVC-decode-only libvips build recipe, audit, …",
    "headSha": "f9cb8aa443f9e28bdb920fc34d9f9c85ccf203c9",
    "status": "completed",
    "updatedAt": "2026-11-10T11:13:47Z",
    "url": "https://github.com/narayanwaraich/photos-manager/actions/runs/42804777791"
  }
]
---
$ gh run list --workflow ci.yml --branch task/0F-negtest --limit 5 --json databaseId,headSha,conclusion,status,createdAt,updatedAt,displayTitle,url
[
  {
    "conclusion": "failure",
    "createdAt": "2026-11-11T05:35:11Z",
    "databaseId": 42966724550,
    "displayTitle": "test(ci): prove macos-arm64 fails closed on missing release tag",
    "headSha": "a5e3459efe8e14eaf10c0bbb0aae81700e2bfe35",
    "status": "completed",
    "updatedAt": "2026-11-11T05:41:01Z",
    "url": "https://github.com/narayanwaraich/photos-manager/actions/runs/42966724550"
  }
]
```
*(Throwaway branch `task/0F-negtest` was deleted both locally and from `origin` immediately after downloading Run `42966724550`'s verification logs.)*

| Workflow | Run / Branch | Run ID | Commit | Status / Conclusion | Outcome Summary |
|---|---|---|---|---|---|
| `deps-macos-vips.yml` | #1 (`task/0F`) | `42804778977` | `f9cb8aa` | `completed` / `failure` | `build-audit` (`129791019071`) failed in `Build relocatable libvips macOS arm64 archive` with `KeyError: 'OUT_ZIP'` in `deps/macos-vips/build.sh`; fixed in `631a85b`. |
| `deps-macos-vips.yml` | #2 (`task/0F`) | `42806945658` | `631a85b` | `completed` / `failure` | `build-audit` (`129798381168`) passed; `test-clean-macos` (`129799793819`) exited `1` at workflow line `139` (`echo: write error: Broken pipe`, `echo` returned `1` with `SIGPIPE` ignored on runner); stopped at 2-run gate (`d3a4535`). |
| `deps-macos-vips.yml` | #3 (`task/0F`) | `42945772807` | `e7e976a` | `completed` / **`success`** | **`build-audit` (`130278088973`, `3m5s`) + `test-clean-macos` (`130279239848`, `20s`) BOTH GREEN.** Identical SHA-256 `6bb24d51dedf9f69344aa187cb0f9651b7842de4c465643480f3a794481dba64`; artifact `vips-macos-arm64-hevc` (`ID 13822214591`) uploaded. |
| `ci.yml` | #1 (`task/0F`) | `42804777791` | `f9cb8aa` | `completed` / `success` | Historical Part A baseline (macOS still used Homebrew `vips`; proves zero regression only). |
| `ci.yml` | #2 (`task/0F`) | `42806945665` | `631a85b` | `completed` / `success` | Historical Part A baseline (macOS still used Homebrew `vips`; proves zero regression only). |
| `ci.yml` | #3 (`task/0F`) | `42945772425` | `e7e976a` | `completed` / `success` | Historical Part A baseline (macOS still used Homebrew `vips`; proves zero regression only). |
| `ci.yml` | #4 (`task/0F`) | `42958401564` | `09130ad` | `completed` / `failure` | Part B first CI run: `windows-x64` failed `cargo clippy -- -D warnings` on unused `non_empty_env`/`is_env_truthy` in `src-tauri/build.rs`; fixed by `#[cfg(target_os = "macos")]` in `aa66966`. |
| `ci.yml` | **#5 (`task/0F`)** | **`42958985250`** | **`aa66966`** | `completed` / **`success`** | **ALL 3 JOBS GREEN:** `macos-arm64` (`130320746782`, `4m10s`), `windows-x64` (`130320746992`, `4m51s`), `Security & License check` (`130320747130`, `46s`). Custom HEVC archive + `REQUIRE_HEIC=1` verified on both OSes (`heic_decode_verified: true`). |
| `ci.yml` | **Neg test (`task/0F-negtest`)** | **`42966724550`** | **`a5e3459`** | `completed` / `failure` *(expected)* | **Proved fail-closed:** `macos-arm64` (`130345532153`, `1m21s`) aborted at `Install macOS prerequisites` (`exit 1`) after 3 retry attempts on nonexistent release tag `macos-arm64-vips-nonexistent-negtest`. |

```text
$ gh release view macos-arm64-vips-8.18.6-hevc1 --repo narayanwaraich/photos-manager-deps
macos-arm64-vips-8.18.6-hevc1
narayanwaraich released this about 1 hour ago

  Built by deps-macos-vips.yml run 42945772807                                


Assets
NAME                              DIGEST                                SIZE    
vips-macos-arm64-8.18.6-hevc.zip  sha256:6bb24d51dedf9f69344aa187cb...  5.52 MiB

View on GitHub: https://github.com/narayanwaraich/photos-manager-deps/releases/tag/macos-arm64-vips-8.18.6-hevc1

$ gh run view 42945772807
✓ task/0F Build & Test macOS arm64 libvips (HEVC Decode) · 42945772807
Trigger... via push

JOBS
✓ Build & audit macOS arm64 libvips (HEVC decode) in 3m5s (ID 130278088973)
✓ Clean-runner test with Homebrew hidden (macos-15) in 20s (ID 130279239848)
[ANNOTATIONS/section separator lines trimmed]
ARTIFACTS
vips-macos-arm64-hevc
vips-macos-arm64-hevc-staging

For more information about a job, try: gh run view --job=<job-id>
View this run on GitHub: https://github.com/narayanwaraich/photos-manager/actions/runs/42945772807

$ gh run view 42958985250
✓ task/0F CI & Packaging Matrix · 42958985250
Triggered via push about 1 hour ago

JOBS
✓ macos-arm64 in 4m10s (ID 130320746782)
✓ windows-x64 in 4m51s (ID 130320746992)
✓ Security & License check (cargo deny) in 46s (ID 130320747130)
[ANNOTATIONS Node.js 20 deprecation warnings trimmed]
ARTIFACTS
verify-logs-windows-x64
photos-manager-macos-arm64
smoke-report-macos-arm64
photos-manager-windows-x64
smoke-report-windows-x64
verify-logs-macos-arm64

$ gh run view 42966724550
X task/0F-negtest CI & Packaging Matrix · 42966724550
Triggered via push about 11 minutes ago

JOBS
✓ Security & License check (cargo deny) in 57s (ID 130345531854)
X macos-arm64 in 1m21s (ID 130345532153)
  ✓ Set up job
  ... [setup steps OK]
  X Install macOS prerequisites
  - Install frontend dependencies
  - Generate synthetic test fixtures
  - Run verification suite (pnpm verify)
  - Stage bundled resources
  - Build Tauri application (unsigned)
  - Post-build bundle packaging & library relocation
  - Clean-environment smoke test (macOS arm64)
  [remaining skipped steps & ANNOTATIONS trimmed]
✓ windows-x64 in 5m46s (ID 130345532234)
```

---

### 5.2 Part A Green Run #3 Evidence (`deps-macos-vips.yml` `42945772807` on `e7e976a`)

*(Tab-delimited job/step/timestamp prefixes and ANSI color escapes trimmed from GHA log lines below for readability; all line contents are verbatim from `gh run view --job=... --log`.)*

#### Job 1 (`130278088973`): Environment, Homebrew Versions, Build & Audit SHA-256, Full Audit Output, `versions.json`, `LICENSES.md`
```text
RUNNER_OS=macOS RUNNER_ARCH=ARM64
ImageOS=macos15 ImageVersion=20261104.0514.1
ProductName:		macOS
ProductVersion:		15.8.2
BuildVersion:		24H232
Darwin iad20-fj925-bd8162f8-8ab6-4d34-937a-203de9db3ef8-921248976DC5.local 24.6.0 Darwin Kernel Version 24.6.0: Sun Oct 11 23:44:11 PDT 2026; root:xnu-11417.140.69.713.47~2/RELEASE_ARM64_VMAPPLE arm64
Xcode 16.4
Build version 16F6

=== Installed Homebrew leaf versions ===
cmake 4.4.5
gettext 1.0
glib 2.90.1
highway 1.4.0
jpeg-turbo 3.2.1
libexif 0.6.26
libffi 3.8.0
libpng 1.6.59
libtiff 4.7.2
little-cms2 2.19.1
meson 1.12.1
ninja 1.13.2
pcre2 10.49
pkgconf 3.0.10
webp 1.6.0
xz 5.8.4
zstd 1.5.7_1

Closure collected: 22 dylibs, 4 executables
Generated versions.json and LICENSES.md:
  LGPL (6): gettext, glib, libde265, libexif, libheif, vips
  Permissive (10): aom, highway, jpeg-turbo, libpng, libtiff, little-cms2, pcre2, webp, xz, zstd
=== 6. Creating relocatable zip archive ===
✓ Built: /Users/runner/work/photos-manager/photos-manager/vips-macos-arm64-8.18.6-hevc.zip
-rw-r--r--  1 runner  staff   5.5M Nov 11 02:27 /Users/runner/work/photos-manager/photos-manager/vips-macos-arm64-8.18.6-hevc.zip
6bb24d51dedf9f69344aa187cb0f9651b7842de4c465643480f3a794481dba64  /Users/runner/work/photos-manager/photos-manager/vips-macos-arm64-8.18.6-hevc.zip

=== Auditing macOS libvips archive: vips-macos-arm64-8.18.6-hevc.zip ===
PASS (a): No forbidden filenames (x265|x264|fftw|imagequant|magick|poppler|rsvg|openslide) in archive
PASS (b): All 26 Mach-O files use @rpath/ cleanly with zero Homebrew/local path leaks or forbidden linkages
PASS (c): Zero x265 symbols or linkage strings across all Mach-O files (enum nick verified absent from all non-libvips files)
PASS (d): libde265.0.dylib present and dynamically linked by libheif.1.dylib
  Verified 16 components covering 22 regular dylibs.
  LGPL components: gettext (LGPL-2.1-or-later), glib (LGPL-2.1-or-later), libde265 (LGPL-3.0-or-later), libexif (LGPL-2.1-or-later AND LGPL-2.0-or-later), libheif (LGPL-3.0-or-later), vips (LGPL-2.1-or-later)
  Permissive components: aom (BSD-2-Clause), highway (Apache-2.0 OR BSD-3-Clause), jpeg-turbo (IJG AND Zlib AND BSD-3-Clause), libpng (libpng-2.0), libtiff (libtiff), little-cms2 (MIT), pcre2 (BSD-3-Clause), webp (BSD-3-Clause), xz (0BSD), zstd (BSD-3-Clause)
  GPL status: NONE (0 GPL libraries bundled)
PASS (e): versions.json valid, libde265 present, all dylibs accounted for, zero unknown/GPL licenses
OVERALL: AUDIT PASSED
{
  "target": "macos-arm64",
  "archive": "vips-macos-arm64-8.18.6-hevc.zip",
  "builtAt": "2026-11-11T02:27:06Z",
  "runnerOsVersion": "15.8.2",
  "deploymentTarget": "15.0",
  "xcode": "Xcode 16.4 Build version 16F6",
  "systemLibrariesUsed": [
    "/System/Library/Frameworks/*",
    "/usr/lib/libSystem.B.dylib",
    "/usr/lib/libc++.1.dylib",
    "/usr/lib/libexpat.1.dylib",
    "/usr/lib/libffi.dylib",
    "/usr/lib/libiconv.2.dylib",
    "/usr/lib/libobjc.A.dylib",
    "/usr/lib/libresolv.9.dylib",
    "/usr/lib/libz.1.dylib"
  ],
  "components": [
    {
      "name": "aom",
      "version": "3.14.1",
      "license": "BSD-2-Clause",
      "rawFormulaLicense": "BSD-2-Clause",
      "source": "tarball:https://storage.googleapis.com/aom-releases/libaom-3.14.1.tar.gz",
      "sha256": "44bf90dbd23e734d50e70a8c41c285193922938bd0d3bc2ee56764d181d55ef5",
      "dylibs": [
        "libaom.3.dylib"
      ]
    },
    {
      "name": "gettext",
      "version": "1.0",
      "license": "LGPL-2.1-or-later",
      "rawFormulaLicense": "GPL-3.0-or-later AND LGPL-2.1-or-later",
      "source": "homebrew:gettext",
      "dylibs": [
        "libintl.8.dylib"
      ],
      "licenseNote": "Runtime libintl shared library is LGPL-2.1-or-later; GPL-3.0-or-later applies only to gettext CLI build tools (not bundled)."
    },
    {
      "name": "glib",
      "version": "2.90.1",
      "license": "LGPL-2.1-or-later",
      "rawFormulaLicense": "LGPL-2.1-or-later",
      "source": "homebrew:glib",
      "dylibs": [
        "libgio-2.0.0.dylib",
        "libglib-2.0.0.dylib",
        "libgmodule-2.0.0.dylib",
        "libgobject-2.0.0.dylib"
      ]
    },
    {
      "name": "highway",
      "version": "1.4.0",
      "license": "Apache-2.0 OR BSD-3-Clause",
      "rawFormulaLicense": "Apache-2.0 OR BSD-3-Clause",
      "source": "homebrew:highway",
      "dylibs": [
        "libhwy.1.dylib"
      ]
    },
    {
      "name": "jpeg-turbo",
      "version": "3.2.1",
      "license": "IJG AND Zlib AND BSD-3-Clause",
      "rawFormulaLicense": "IJG AND Zlib AND BSD-3-Clause",
      "source": "homebrew:jpeg-turbo",
      "dylibs": [
        "libjpeg.8.dylib"
      ]
    },
    {
      "name": "libde265",
      "version": "1.1.1",
      "license": "LGPL-3.0-or-later",
      "rawFormulaLicense": "LGPL-3.0-or-later",
      "source": "tarball:https://github.com/strukturag/libde265/releases/download/v1.1.1/libde265-1.1.1.tar.gz",
      "sha256": "fd48a927e94ed74fc7ce8829d222b9d8599fcbfe8b6448ba66705babc56ab219",
      "dylibs": [
        "libde265.0.dylib"
      ]
    },
    {
      "name": "libexif",
      "version": "0.6.26",
      "license": "LGPL-2.1-or-later AND LGPL-2.0-or-later",
      "rawFormulaLicense": "LGPL-2.1-or-later AND LGPL-2.0-or-later",
      "source": "homebrew:libexif",
      "dylibs": [
        "libexif.12.dylib"
      ]
    },
    {
      "name": "libheif",
      "version": "1.23.2",
      "license": "LGPL-3.0-or-later",
      "rawFormulaLicense": "LGPL-3.0-or-later",
      "source": "tarball:https://github.com/strukturag/libheif/releases/download/v1.23.2/libheif-1.23.2.tar.gz",
      "sha256": "8bd5d41d19dc84536d118b04774709f244df6104ef66d623dad5fa4650143405",
      "dylibs": [
        "libheif.1.dylib"
      ]
    },
    {
      "name": "libpng",
      "version": "1.6.59",
      "license": "libpng-2.0",
      "rawFormulaLicense": "libpng-2.0",
      "source": "homebrew:libpng",
      "dylibs": [
        "libpng16.16.dylib"
      ]
    },
    {
      "name": "libtiff",
      "version": "4.7.2",
      "license": "libtiff",
      "rawFormulaLicense": "libtiff",
      "source": "homebrew:libtiff",
      "dylibs": [
        "libtiff.6.dylib"
      ]
    },
    {
      "name": "little-cms2",
      "version": "2.19.1",
      "license": "MIT",
      "rawFormulaLicense": "MIT",
      "source": "homebrew:little-cms2",
      "dylibs": [
        "liblcms2.2.dylib"
      ]
    },
    {
      "name": "pcre2",
      "version": "10.49",
      "license": "BSD-3-Clause",
      "rawFormulaLicense": "BSD-3-Clause",
      "source": "homebrew:pcre2",
      "dylibs": [
        "libpcre2-8.0.dylib"
      ]
    },
    {
      "name": "vips",
      "version": "8.18.6",
      "license": "LGPL-2.1-or-later",
      "rawFormulaLicense": "LGPL-2.1-or-later",
      "source": "tarball:https://github.com/libvips/libvips/releases/download/v8.18.6/vips-8.18.6.tar.xz",
      "sha256": "3c41e1d5458081bfa4a5bc54e116c46259c75c6760a18027764555632b9dda3e",
      "dylibs": [
        "libvips.42.dylib"
      ]
    },
    {
      "name": "webp",
      "version": "1.6.0",
      "license": "BSD-3-Clause",
      "rawFormulaLicense": "BSD-3-Clause",
      "source": "homebrew:webp",
      "dylibs": [
        "libsharpyuv.0.dylib",
        "libwebp.7.dylib",
        "libwebpdemux.2.dylib",
        "libwebpmux.3.dylib"
      ]
    },
    {
      "name": "xz",
      "version": "5.8.4",
      "license": "0BSD",
      "rawFormulaLicense": "0BSD AND GPL-2.0-or-later",
      "source": "homebrew:xz",
      "dylibs": [
        "liblzma.5.dylib"
      ],
      "licenseNote": "Runtime liblzma shared library is 0BSD public domain; GPL-2.0-or-later applies only to xz CLI utilities/scripts (not bundled)."
    },
    {
      "name": "zstd",
      "version": "1.5.7_1",
      "license": "BSD-3-Clause",
      "rawFormulaLicense": "(BSD-3-Clause OR GPL-2.0-only) AND BSD-2-Clause AND MIT",
      "source": "homebrew:zstd",
      "dylibs": [
        "libzstd.1.dylib"
      ],
      "licenseNote": "Dual-licensed (BSD-3-Clause OR GPL-2.0-only); licensee elects BSD-3-Clause alongside BSD-2-Clause AND MIT."
    }
  ]
}
# Bundled Component Licenses (macOS arm64 libvips HEVC-decode build)

- **Built at:** 2026-11-11T02:27:06Z
- **macOS version:** 15.8.2 (deployment target 15.0)
- **Toolchain:** Xcode 16.4 Build version 16F6
- **LGPL libraries (dynamically linked, permitted):** gettext, glib, libde265, libexif, libheif, vips
- **Permissive libraries:** aom, highway, jpeg-turbo, libpng, libtiff, little-cms2, pcre2, webp, xz, zstd
- **GPL libraries bundled:** None (x265, fftw, and imagequant 4.x are excluded from build and linkage).
- **Patent note:** HEVC codec patent licensing is separate from open-source library copyright licenses (D13 / Phase 7).

=== Archive SHA-256 and listing ===
-rw-r--r--  1 runner  staff  5793473 Nov 11 02:27 /Users/runner/work/photos-manager/photos-manager/vips-macos-arm64-8.18.6-hevc.zip
6bb24d51dedf9f69344aa187cb0f9651b7842de4c465643480f3a794481dba64  /Users/runner/work/photos-manager/photos-manager/vips-macos-arm64-8.18.6-hevc.zip
[upload-artifact action setup & Markdown license table lines trimmed — table matches §4 above]
Artifact vips-macos-arm64-hevc-staging.zip successfully finalized. Artifact ID 13822208328
```

#### Job 2 (`130279239848`): Matching SHA-256, `lib/` Inventory, Clean-Runner All Four Test Sections, 14-Day Artifact
```text
-rw-r--r--  1 runner  wheel  5793473 Nov 11 02:27 /tmp/vips-staging/vips-macos-arm64-8.18.6-hevc.zip
6bb24d51dedf9f69344aa187cb0f9651b7842de4c465643480f3a794481dba64  /tmp/vips-staging/vips-macos-arm64-8.18.6-hevc.zip
Extracted prefix: /tmp/vips-clean-test/vips-macos-arm64-8.18.6-hevc
=== Verifying otool -L across unpacked lib/* and bin/* ===
PASS: Zero non-system absolute paths in unpacked binaries/dylibs
=== 1. Check vips -l lists heifload with stripped PATH and hidden Homebrew ===
        VipsForeignLoadNsgif (gifload_base), load GIF with libnsgif, priority=50
          VipsForeignLoadNsgifFile (gifload), load GIF with libnsgif (.gif), priority=50, is_a, get_flags, get_flags_filename, header, load
[... other builtin gif/png/jpeg/webp/tiff loaders trimmed]
        VipsForeignLoadHeif (heifload_base), load a HEIF image, priority=0
          VipsForeignLoadHeifFile (heifload), load a HEIF image (.heic, .heif, .avif), priority=0, is_a, get_flags, header, load
          VipsForeignLoadHeifBuffer (heifload_buffer), load a HEIF image, priority=0, is_a_buffer, get_flags, header, load
          VipsForeignLoadHeifSource (heifload_source), load a HEIF image, nocache, priority=0, is_a_source, get_flags, header, load
✓ Success: vips -l lists heifload (and omits svgload/magickload)
=== 2. Decode orientation-6 HEIC fixture to PNG and verify upright dimensions ===
Thumbnail header: /tmp/heic_thumb.png: 240x320 uchar, 3 bands, srgb, pngload
Copy header: /tmp/heic_copy.png: 480x640 uchar, 3 bands, srgb, pngload
✓ Success: Orientation-6 HEIC decoded upright (240x320 thumb, 480x640 copy)
=== 3. Decode Display P3 HEIC fixture to PNG and verify dimensions ===
P3 copy header: /tmp/p3_copy.png: 640x480 uchar, 3 bands, srgb, pngload
P3 thumb header: /tmp/p3_thumb.png: 320x240 uchar, 3 bands, srgb, pngload
✓ Success: Display P3 HEIC decoded (640x480 copy, 320x240 thumb)
=== 4. SVG control check (-Drsvg=disabled: both unblocked and VIPS_BLOCK_UNTRUSTED=1 must fail) ===
Unblocked exit=1 output: VipsForeignLoad: "tests/fixtures/generated/fixture_blocked.svg" is not a known file format
Blocked exit=1 output: VipsForeignLoad: "tests/fixtures/generated/fixture_blocked.svg" is not a known file format
✓ Success: SVG load rejected cleanly both unblocked and under VIPS_BLOCK_UNTRUSTED=1 (no svgload built)
=== Final verified artifact details ===
-rw-r--r--  1 runner  wheel  5793473 Nov 11 02:27 /tmp/vips-staging/vips-macos-arm64-8.18.6-hevc.zip
6bb24d51dedf9f69344aa187cb0f9651b7842de4c465643480f3a794481dba64  /tmp/vips-staging/vips-macos-arm64-8.18.6-hevc.zip
Artifact vips-macos-arm64-hevc has been successfully uploaded! Final size is 5756758 bytes. Artifact ID is 13822214591
```

---

### 5.3 Part A `audit-dylibs.sh` Regression Suite (`deps/macos-vips/test-audit.sh` & Old `f9cb8aa`)

*(Summary of `./deps/macos-vips/test-audit.sh` 12-case run on new script vs `git show f9cb8aa:deps/macos-vips/audit-dylibs.sh`: full per-case output preserved in Part A; key lines verbatim below.)*

```text
$ ./deps/macos-vips/test-audit.sh
=== Running audit-dylibs.sh regression suite against: /Users/narayanwaraich/Desktop/Projects/photos-manager/deps/macos-vips/audit-dylibs.sh ===
--- [normal] (i) KNOWN GOOD (good.zip) --- -> OVERALL: AUDIT PASSED (EXIT_CODE=0)
--- [sigpipe-ignored] (i) KNOWN GOOD (good.zip) --- -> OVERALL: AUDIT PASSED (EXIT_CODE=0)
--- [normal] (ii-1) KNOWN BAD (bad-x265.zip) --- -> FAIL (c): Forbidden x265 strings or symbols detected -> OVERALL: AUDIT FAILED (EXIT_CODE=1)
--- [sigpipe-ignored] (ii-1) KNOWN BAD (bad-x265.zip) --- -> FAIL (c): Forbidden x265 strings or symbols detected -> OVERALL: AUDIT FAILED (EXIT_CODE=1)
--- [normal] (ii-2) KNOWN BAD (bad-fftw.zip) --- -> FAIL (a) + FAIL (e) -> OVERALL: AUDIT FAILED (EXIT_CODE=1)
--- [sigpipe-ignored] (ii-2) KNOWN BAD (bad-fftw.zip) --- -> FAIL (a) + FAIL (e) -> OVERALL: AUDIT FAILED (EXIT_CODE=1)
--- [normal] (ii-3) KNOWN BAD (bad-homebrew-path.zip) --- -> FAIL (b) -> OVERALL: AUDIT FAILED (EXIT_CODE=1)
--- [sigpipe-ignored] (ii-3) KNOWN BAD (bad-homebrew-path.zip) --- -> FAIL (b) -> OVERALL: AUDIT FAILED (EXIT_CODE=1)
--- [normal] (iii-a) FAKE strings (>5MB after x265_encoder_open on line 1) ---
  libheif.1.dylib contains forbidden x265 symbol/linkage string (libx265/x265_encoder/x265_param)
  Non-exempt binary libheif.1.dylib contains string 'x265'
  libde265.0.dylib contains forbidden x265 symbol/linkage string (libx265/x265_encoder/x265_param)
  Non-exempt binary libde265.0.dylib contains string 'x265'
  libvips.42.dylib contains forbidden x265 symbol/linkage string (libx265/x265_encoder/x265_param)
  vips contains forbidden x265 symbol/linkage string (libx265/x265_encoder/x265_param)
  Non-exempt binary vips contains string 'x265'
FAIL (c): Forbidden x265 strings or symbols detected
OVERALL: AUDIT FAILED
EXIT_CODE=1 (expected 1)
--- [sigpipe-ignored] (iii-a) FAKE strings (>5MB after x265_encoder_open on line 1) --- -> FAIL (c) -> OVERALL: AUDIT FAILED (EXIT_CODE=1)
--- [normal] (iii-b) FAKE nm (200k lines after _x265_foo on line 1) --- -> FAIL (c) -> OVERALL: AUDIT FAILED (EXIT_CODE=1)
--- [sigpipe-ignored] (iii-b) FAKE nm (200k lines after _x265_foo on line 1) --- -> FAIL (c) -> OVERALL: AUDIT FAILED (EXIT_CODE=1)
=== Summary: 0 assertion failure(s) ===
NEW_RC=0

$ git show f9cb8aa:deps/macos-vips/audit-dylibs.sh > /tmp/old-audit.sh && chmod +x /tmp/old-audit.sh
$ ./deps/macos-vips/test-audit.sh /tmp/old-audit.sh
[cases (i) and (ii-1..3) matched; all 4 large-output (iii-a)/(iii-b) cases failed open with EXIT_CODE=0:]
TEST ASSERTION FAILED [normal] (iii-a) FAKE strings (>5MB after x265_encoder_open on line 1): got exit 0, expected 1
TEST ASSERTION FAILED [normal] (iii-a) FAKE strings (>5MB after x265_encoder_open on line 1): missing expected output substring 'FAIL (c)'
awk: write error on /dev/stdout
TEST ASSERTION FAILED [sigpipe-ignored] (iii-a) FAKE strings (>5MB after x265_encoder_open on line 1): got exit 0, expected 1
TEST ASSERTION FAILED [sigpipe-ignored] (iii-a) FAKE strings (>5MB after x265_encoder_open on line 1): missing expected output substring 'FAIL (c)'
TEST ASSERTION FAILED [normal] (iii-b) FAKE nm (200k lines after _x265_foo on line 1): got exit 0, expected 1
TEST ASSERTION FAILED [normal] (iii-b) FAKE nm (200k lines after _x265_foo on line 1): missing expected output substring 'FAIL (c)'
TEST ASSERTION FAILED [sigpipe-ignored] (iii-b) FAKE nm (200k lines after _x265_foo on line 1): got exit 0, expected 1
TEST ASSERTION FAILED [sigpipe-ignored] (iii-b) FAKE nm (200k lines after _x265_foo on line 1): missing expected output substring 'FAIL (c)'
=== Summary: 8 assertion failure(s) ===
OLD_RC=1
```

---

### 5.4 Windows `audit-zip.sh` Fix (Item `2c`) & Run #2 Patched `libheif.1.dylib` `__cstring` Audit (Item `2d`)

```text
=== NEW win64 audit-zip.sh on fake-strings (>5MB) [normal & sigpipe-ignored] ===
FAIL (b): Found 'x265' string in non-exempt DLL
FAIL (c): Found forbidden x265 linkage string (libx265, x265_encoder, x265_param)
OVERALL: AUDIT FAILED (EXIT=1 in both normal and sigpipe-ignored modes)
# Contrast: OLD (631a85b) win64 audit-zip.sh printed PASS (b) and PASS (c) on fake-strings (>5MB) in both modes!

=== NEW win64 audit-zip.sh on stock vips-dev-x64-web-8.18.6.zip [normal & sigpipe-ignored] ===
10086f2ccc8e2a861831facabab75fa1b80b4bd03f0a8447623ec581002aa4b2  /tmp/vips-dev-x64-web-8.18.6.zip
PASS (a) / PASS (b) / PASS (c) / FAIL (d) / FAIL (e) / PASS (f) -> OVERALL: AUDIT FAILED (EXIT=1)

=== Item 2d: Run #2 (42806945658) lib/libheif.1.dylib copy patched in __cstring (offset 1268760) ===
$ ./deps/macos-vips/audit-dylibs.sh /tmp/run2-patched ; echo "PATCH_NEW_RC=$?"
  libheif.1.dylib contains forbidden x265 symbol/linkage string (libx265/x265_encoder/x265_param)
  Non-exempt binary libheif.1.dylib contains string 'x265'
FAIL (c): Forbidden x265 strings or symbols detected -> OVERALL: AUDIT FAILED -> PATCH_NEW_RC=1 / PATCH_SIGPIPE_RC=1
*(Honesty note: `/tmp/old-audit.sh` also caught this single 56,051-byte `libheif.1.dylib` because `strings` output was smaller than macOS's 64 KB pipe buffer.)*
```

---

### 5.5 Run #2 Pre-Flight on Unpacked Archive & `libvips.42.dylib` Enum Check (Item `3`)

```text
$ strings -a "$VIPS_PREFIX/lib/libvips.42.dylib" | grep -i x265 ; nm -gU "$VIPS_PREFIX/lib/libvips.42.dylib" | grep -i x265 || echo "(none)"
VIPS_FOREIGN_HEIF_ENCODER_X265
x265
NM:
(none)

$ DYLD_PRINT_LIBRARIES=1 "$VIPS_PREFIX/bin/vips" -l > /tmp/dyld.txt 2>&1
dyld lines: 711, parsed paths: 600, prefix_unique: 23, /usr/lib/: 195, /System/: 382, outside: 0
```

---

### 5.6 Part B Green App CI (`ci.yml` `42958985250` on `aa66966`), Downloaded Smoke Artifacts, & Negative Test (`42966724550`)

*(GHA `job\tstep\ttimestamp` prefixes, ANSI escape codes, setup/cache/Rust compile lines, and redundant stdout JSON echo trimmed from `gh run view --job=... --log`; line contents verbatim.)*

#### `macos-arm64` (`ID 130320746782`) + Downloaded `smoke-report-macos-arm64` (`ID 13828542509`)
```text
macOS 26.7.2 (Build 25G332), Image: macos-26-arm64, Version: 20261102.0473.1
Downloading custom HEVC libvips release 'macos-arm64-vips-8.18.6-hevc1' from 'narayanwaraich/photos-manager-deps'...
✓ Verified custom HEVC libvips hash: 6bb24d51dedf9f69344aa187cb0f9651b7842de4c465643480f3a794481dba64
exiftool.tar.gz: OK

Running post-build packaging and dependency relocation...
macOS dylib harvest mode: strict custom prefix (search dirs: /Users/runner/work/photos-manager/photos-manager/deps/vips/lib)
Found application bundle: /Users/runner/work/photos-manager/photos-manager/src-tauri/target/release/bundle/macos/photos-manager.app
Main binary linked libraries:
	... [Apple /System/Library/Frameworks/* and /usr/lib/{libSystem.B,libobjc.A,libiconv.2}.dylib trimmed]
	@rpath/libvips.42.dylib (compatibility version 63.0.0, current version 63.6.0)
	@rpath/libglib-2.0.0.dylib (compatibility version 9001.0.0, current version 9001.1.0)
	@rpath/libgobject-2.0.0.dylib (compatibility version 9001.0.0, current version 9001.1.0)
Bundled 22 dynamic libraries into Contents/Frameworks/
✓ Relocatable @rpath patching and ad-hoc codesign complete for macOS bundle.
Verifying clean otool -L dependency graph and LC_RPATH headers...
✓ Zero Homebrew, build-prefix, or unresolved @rpath linkages across all 23 binaries/libraries.
✓ Created portable macOS zip package: /Users/runner/work/photos-manager/photos-manager/src-tauri/target/release/bundle/macos/photos-manager-macos-arm64.zip

Testing zip package: src-tauri/target/release/bundle/macos/photos-manager-macos-arm64.zip
Testing extracted bundle at: /tmp/smoke-extracted/photos-manager.app
=== Verifying bundled dylib table against docs/native-dependencies.md §4.3 ===
[check-dylib-table] Bundled dylibs on disk in /tmp/smoke-extracted/photos-manager.app/Contents/Frameworks: 22
[check-dylib-table] Documented dylib rows in docs/native-dependencies.md §4.3: 22
SUCCESS: All 22 bundled dylibs exactly match the 22 entries in docs/native-dependencies.md §4.3.
=== CLI Control Test 1 (custom -Drsvg=disabled): vips -l lists heifload without svgload/magickload ===
✓ Success: custom CLI lists (heifload) and omits svgload/magickload
=== CLI Control Test 2 (custom -Drsvg=disabled): unblocked and VIPS_BLOCK_UNTRUSTED=1 both reject SVG ===
Unblocked exit=1: VipsForeignLoad: "tests/fixtures/generated/fixture_blocked.svg" is not a known file format
Blocked exit=1: VipsForeignLoad: "tests/fixtures/generated/fixture_blocked.svg" is not a known file format
✓ Success: SVG rejected cleanly unblocked and under VIPS_BLOCK_UNTRUSTED=1
=== Checking otool -L and LC_RPATH on main binary and all bundled dylibs ===
✓ Main binary and all bundled dylibs clean: zero Homebrew, /usr/local, or build-prefix references
[step moved /opt/homebrew -> /opt/homebrew.off via trap, ran env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin REQUIRE_HEIC=1]
✓ Verified macOS smoke output: heic_decode_verified == true (HEVC decode and upright 240x320 thumbnail verified)
Artifact smoke-report-macos-arm64 has been successfully uploaded! Final size is 784 bytes. Artifact ID is 13828542509
Artifact verify-logs-macos-arm64 has been successfully uploaded! Final size is 1380 bytes. Artifact ID is 13828004682
Artifact photos-manager-macos-arm64 has been successfully uploaded! Final size is 14684492 bytes. Artifact ID is 13828542516
```

Raw JSON read from downloaded CI artifact `smoke-report-macos-arm64/smoke-macos-arm64.json` (`ID 13828542509`):
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
    "version": "8.18.6",
    "loaded_library_path": "/private/tmp/smoke-extracted/photos-manager.app/Contents/Frameworks/libvips.42.dylib",
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
  "heic_decode_verified": true,
  "heic_note": "HEVC decode and upright 240x320 thumbnail verified",
  "errors": []
}
```

#### `windows-x64` (`ID 130320746992`) + Downloaded `smoke-report-windows-x64` (`ID 13828467679`)
```text
Microsoft Windows Server 2025 10.0.26100 Datacenter, Image: windows-2025-vs2026, Version: 20261103.298.1
Downloading custom HEVC libvips release 'win64-vips-8.18.6-hevc1' from 'narayanwaraich/photos-manager-deps'...
✓ Verified custom HEVC libvips hash: 72591A6BDF775618BC154E83990C86919ECB18F6E9D72FADE5BAEF9B4F343991
✓ Copied 39 Windows libvips DLLs to release target directory.
✓ Copied staged resources to release target directory.

=== Windows Host System Prerequisites ===
    LongPathsEnabled    REG_DWORD    0x1
C:\Windows\System32\vcruntime140.dll exists: True
=== CLI Control Test 1: vips.exe copy without VIPS_BLOCK_UNTRUSTED (must succeed) ===
✓ Success: vips.exe copy succeeded when unblocked
=== CLI Control Test 2: vips.exe copy with VIPS_BLOCK_UNTRUSTED=1 (must fail) ===
VipsForeignLoad: "tests\fixtures\generated\fixture_blocked.svg" is not a known file format
✓ Success: vips.exe rejected SVG under VIPS_BLOCK_UNTRUSTED=1 with code 1
=== Measuring Windows ExifTool process tree in stay_open mode ===
Parent process: exiftool (PID: 2920)
Child processes spawned by exiftool.exe:
No child processes detected (exiftool is single process).
=== Auditing all top-level installed DLLs for VC++ runtime dependencies ===
Total top-level installed DLLs scanned: 39
SUCCESS: 0 out of 39 installed DLLs import VC++ runtimes (clean Universal CRT / system only).
Smoke test heic_decode_verified: True
Smoke test heic_note: HEVC decode and upright 240x320 thumbnail verified
Artifact smoke-report-windows-x64 has been successfully uploaded! Final size is 783 bytes. Artifact ID is 13828467679
Artifact verify-logs-windows-x64 has been successfully uploaded! Final size is 1346 bytes. Artifact ID is 13828826040
Artifact photos-manager-windows-x64 has been successfully uploaded! Final size is 18185634 bytes. Artifact ID is 13828487815
```

Raw JSON read from downloaded CI artifact `smoke-report-windows-x64/smoke-windows-x64.json` (`ID 13828467679`):
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

#### Fail-Closed Negative Test (`ci.yml` `42966724550`, `task/0F-negtest`, `a5e3459`)
```text
Downloading custom HEVC libvips release 'macos-arm64-vips-nonexistent-negtest' from 'narayanwaraich/photos-manager-deps'...
release not found
Attempt 1 of 3 failed: gh release download failed with exit code 1
release not found
Attempt 2 of 3 failed: gh release download failed with exit code 1
release not found
Attempt 3 of 3 failed: gh release download failed with exit code 1
##[error]Failed to download custom HEVC libvips release 'macos-arm64-vips-nonexistent-negtest' from 'narayanwaraich/photos-manager-deps' after 3 attempts (gh release download failed with exit code 1). Check whether DEPS_TOKEN is expired/invalid, lacks Contents read permission on 'narayanwaraich/photos-manager-deps', or release tag 'macos-arm64-vips-nonexistent-negtest' is missing.
##[error]Process completed with exit code 1.
```

---

### 5.7 Final Local Checks on `aa66966` (Lint, `| grep` Audit, `check-dylib-table`, `otool -L`, Clean Smoke Test)

```text
$ shellcheck -x deps/macos-vips/*.sh deps/win64-vips/audit-zip.sh && echo "shellcheck: ok" && actionlint -verbose .github/workflows/deps-macos-vips.yml .github/workflows/ci.yml
shellcheck: ok
verbose: Linting 2 files
verbose: Linting .github/workflows/ci.yml
verbose: Using project at /Users/narayanwaraich/Desktop/Projects/photos-manager
verbose: Linting .github/workflows/deps-macos-vips.yml
verbose: Using project at /Users/narayanwaraich/Desktop/Projects/photos-manager
verbose: Found 0 parse errors in 0 ms for .github/workflows/deps-macos-vips.yml
verbose: Found 0 parse errors in 0 ms for .github/workflows/ci.yml
verbose: Rule "pyflakes" was disabled: exec: "pyflakes": executable file not found in $PATH
verbose: Rule "pyflakes" was disabled: exec: "pyflakes": executable file not found in $PATH
verbose: Found total 0 errors in 75 ms for .github/workflows/deps-macos-vips.yml
verbose: Found total 0 errors in 79 ms for .github/workflows/ci.yml
verbose: Found 0 errors in 2 files

$ grep -nE '\|[[:space:]]*grep' .github/workflows/ci.yml .github/workflows/deps-macos-vips.yml deps/macos-vips/*.sh deps/win64-vips/audit-zip.sh
.github/workflows/ci.yml:560:          # 3. Verify libvips CLI untrusted loader control test (zero | grep -q pipelines)
.github/workflows/ci.yml:619:            LEAKS="$(otool -L "$target" | tail -n +2 | grep -E "/opt/homebrew|/usr/local|/Cellar|/Users/|/tmp/|/var/" || true)"
.github/workflows/ci.yml:624:            RP_LEAKS="$(otool -l "$target" | grep -A2 "cmd LC_RPATH" | grep -E "/opt/homebrew|/usr/local|/Cellar|/Users/|/tmp/|/var/" || true)"
.github/workflows/deps-macos-vips.yml:105:            LEAKS=$(otool -L "$f" | tail -n +2 | grep -E "/opt/homebrew|/usr/local|/Cellar|/var/|/tmp/" || true)
.github/workflows/deps-macos-vips.yml:110:            RP_LEAKS=$(otool -l "$f" | grep -A2 "cmd LC_RPATH" | grep -E "/opt/homebrew|/usr/local|/Cellar|/var/|/tmp/" || true)
```
Explanation of remaining hits: **Zero `producer | grep -q` remain** anywhere in `.github/workflows/ci.yml`, `.github/workflows/deps-macos-vips.yml`, `deps/macos-vips/*.sh`, or `deps/win64-vips/audit-zip.sh`. Hit 1 (`ci.yml:560`) is a comment; the 4 code hits (`ci.yml:619,624`, `deps-macos-vips.yml:105,110`) omit `-q`, use `|| true`, and read every line emitted by `otool`, so the upstream writer is never closed early.

```text
$ node scripts/check-dylib-table.mjs src-tauri/target/release/bundle/macos/photos-manager.app && otool -L src-tauri/target/release/bundle/macos/photos-manager.app/Contents/MacOS/photos-manager
[check-dylib-table] Bundled dylibs on disk in src-tauri/target/release/bundle/macos/photos-manager.app/Contents/Frameworks: 22
[check-dylib-table] Documented dylib rows in docs/native-dependencies.md §4.3: 22
SUCCESS: All 22 bundled dylibs exactly match the 22 entries in docs/native-dependencies.md §4.3.
src-tauri/target/release/bundle/macos/photos-manager.app/Contents/MacOS/photos-manager:
	/System/Library/Frameworks/WebKit.framework/Versions/A/WebKit (compatibility version 1.0.0, current version 625.1.29)
	/System/Library/Frameworks/ApplicationServices.framework/Versions/A/ApplicationServices (compatibility version 1.0.0, current version 66.0.0)
	/System/Library/Frameworks/CoreGraphics.framework/Versions/A/CoreGraphics (compatibility version 64.0.0, current version 2050.0.0)
	/System/Library/Frameworks/Carbon.framework/Versions/A/Carbon (compatibility version 2.0.0, current version 170.0.0)
	/System/Library/Frameworks/CoreVideo.framework/Versions/A/CoreVideo (compatibility version 1.2.0, current version 758.27.0)
	/System/Library/Frameworks/CoreFoundation.framework/Versions/A/CoreFoundation (compatibility version 150.0.0, current version 5027.0.69)
	/usr/lib/libSystem.B.dylib (compatibility version 1.0.0, current version 1359.0.0)
	/System/Library/Frameworks/AppKit.framework/Versions/C/AppKit (compatibility version 45.0.0, current version 2775.10.103)
	/System/Library/Frameworks/QuartzCore.framework/Versions/A/QuartzCore (compatibility version 1.2.0, current version 1223.2.2)
	/System/Library/Frameworks/Foundation.framework/Versions/C/Foundation (compatibility version 300.0.0, current version 5027.0.69)
	/usr/lib/libobjc.A.dylib (compatibility version 1.0.0, current version 228.0.0)
	@rpath/libvips.42.dylib (compatibility version 63.0.0, current version 63.6.0)
	@rpath/libglib-2.0.0.dylib (compatibility version 9001.0.0, current version 9001.1.0)
	@rpath/libgobject-2.0.0.dylib (compatibility version 9001.0.0, current version 9001.1.0)
	/usr/lib/libiconv.2.dylib (compatibility version 7.0.0, current version 7.0.0)
	/System/Library/Frameworks/ColorSync.framework/Versions/A/ColorSync (compatibility version 1.0.0, current version 3929.0.0)
	/System/Library/Frameworks/CoreServices.framework/Versions/A/CoreServices (compatibility version 1.0.0, current version 1226.0.0)

$ env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin REQUIRE_HEIC=1 src-tauri/target/release/bundle/macos/photos-manager.app/Contents/MacOS/photos-manager --smoke-test
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
    "version": "8.18.6",
    "loaded_library_path": "/Users/narayanwaraich/Desktop/Projects/photos-manager/src-tauri/target/release/bundle/macos/photos-manager.app/Contents/Frameworks/libvips.42.dylib",
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
    "version": "13.55",
    "resolved_path": "/Users/narayanwaraich/Desktop/Projects/photos-manager/src-tauri/target/release/bundle/macos/photos-manager.app/Contents/Resources/resources/exiftool/exiftool",
    "is_bundled": true,
    "perl_path": "/usr/bin/perl",
    "extraction_success": true,
    "detected_orientation": 6,
    "camera_make": "PhotoCorp"
  },
  "thumbnail": {
    "success": true,
    "input_path": "/Users/narayanwaraich/Desktop/Projects/photos-manager/src-tauri/target/release/bundle/macos/photos-manager.app/Contents/Resources/resources/fixtures/image_orientation_6.jpg",
    "output_path": "/var/folders/vw/sxxzw9pd58qf70hwkhrlgsdr0000gn/T/photos_manager_smoke_thumb.webp",
    "width": 240,
    "height": 320,
    "byte_size": 1364
  },
  "heic_decode_verified": true,
  "heic_note": "HEVC decode and upright 240x320 thumbnail verified",
  "errors": []
}
```
*(Note: Local `.app` bundle used pre-staged local ExifTool `13.55`, whereas CI `42958985250` downloads and verifies official ExifTool `13.61` on both macOS and Windows.)*

### 5.8 Final Verification Suite (`pnpm verify`)

```text
$ pnpm verify
[WARN] Issue while reading "/Users/narayanwaraich/.npmrc". Operation not permitted (os error 1)
$ node scripts/verify.mjs
Photos Manager — pnpm verify verification run
Date: 2026-11-11T06:08:23.695Z
Log output file: /Users/narayanwaraich/Desktop/Projects/photos-manager/reports/logs/verify-2026-11-11.txt

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

    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.59s

✓ PASSED: cargo clippy

======================================================
STEP: cargo test
COMMAND: cargo test --manifest-path src-tauri/Cargo.toml
CWD: /Users/narayanwaraich/Desktop/Projects/photos-manager
======================================================


running 18 tests
test filesystem::tests::test_windows_cases_note ... ok
test filesystem::tests::test_macos_nfc_vs_nfd_normalization_probe ... ok
test filesystem::tests::test_roundtrip_non_utf8_path_unix ... ok
test filesystem::tests::test_roundtrip_non_ascii_path ... ok
test metadata::tests::test_exiftool_missing_file ... ok
test database::tests::test_init_pool_and_app_settings_migration ... ok
test metadata::tests::test_exiftool_rejects_non_absolute_path ... ok
test metadata::tests::test_exiftool_malformed_output ... ok
test metadata::tests::test_exiftool_non_ascii_filename ... ok
test metadata::tests::test_exiftool_timeout ... ok
test metadata::tests::test_exiftool_long_path ... ok
test metadata::tests::test_exiftool_success ... ok
test image_pipeline::vips::tests::test_vips_supported_loaders_reports_real_codecs ... ok
test image_pipeline::vips::tests::test_vips_missing_file_error ... ok
test image_pipeline::vips::tests::test_vips_blocks_svgload ... ok
test image_pipeline::vips::tests::test_vips_thumbnail_png ... ok
test image_pipeline::vips::tests::test_vips_thumbnail_rotated_jpeg ... ok
test metadata::tests::test_child_killed_after_timeout ... ok

test result: ok. 18 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.06s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.29s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/photos_manager_lib-1cc4cdd59e5db195)
     Running unittests src/main.rs (src-tauri/target/debug/deps/photos_manager-9307dd686c9317e1)
   Doc-tests photos_manager_lib

✓ PASSED: cargo test

======================================================
STEP: frontend typecheck
COMMAND: pnpm exec tsc --noEmit
CWD: /Users/narayanwaraich/Desktop/Projects/photos-manager
======================================================

[WARN] Issue while reading "/Users/narayanwaraich/.npmrc". Operation not permitted (os error 1)

✓ PASSED: frontend typecheck

======================================================
STEP: frontend lint
COMMAND: pnpm exec eslint src/
CWD: /Users/narayanwaraich/Desktop/Projects/photos-manager
======================================================

[WARN] Issue while reading "/Users/narayanwaraich/.npmrc". Operation not permitted (os error 1)

✓ PASSED: frontend lint

======================================================
STEP: production build
COMMAND: pnpm exec vite build
CWD: /Users/narayanwaraich/Desktop/Projects/photos-manager
======================================================

vite v8.3.5 building client environment for production...
transforming...
✓ 15 modules transformed.
rendering chunks...
computing gzip size...
dist/index.html                  0.32 kB │ gzip:  0.23 kB
dist/assets/index-DuaNDsJK.js  221.20 kB │ gzip: 69.39 kB

✓ built in 55ms
[WARN] Issue while reading "/Users/narayanwaraich/.npmrc". Operation not permitted (os error 1)

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

---

## 6. Decisions made

1. **Runner `macos-15` arm64 (`MACOSX_DEPLOYMENT_TARGET=15.0`):** `macos-14` images retired on 2026-11-02 (`https://github.com/actions/runner-images/pull/15227`, referencing `#13518`). `macos-15` (`macos-15-arm64` `20261104.0514.1`, `15.8.2` Build `24H232`, `Xcode 16.4` `16F6`) builds binaries compatible with both `macos-15` and `ci.yml`'s `macos-latest` (`macos-26-arm64`).
2. **Disable `cgif`, enable `nsgif` (`-Dcgif=disabled -Dnsgif=true`):** In `vips-8.18.6/meson.build:298`, `cgif_dep` is gated inside `if quantisation_package.found()` (`imagequant` or `quantizr`). Homebrew `libimagequant` 4.4.1 is `GPL-3.0-or-later` (forbidden) and `quantizr` is absent from Homebrew. GIF decoding remains built-in via `-Dnsgif=true`.
3. **Isolated `PKG_CONFIG_LIBDIR` (skipping `/opt/homebrew/lib/pkgconfig` and Homebrew `libffi`):** Constructing `PKG_CONFIG_LIBDIR` strictly from `STAGE_PREFIX/lib/pkgconfig`, allowlisted leaf kegs (`deps/macos-vips/build.sh:89-145`), and Apple SDK shims guarantees preinstalled runner formulas (`fftw`, `x265`, `librsvg`, `poppler`, `imagemagick`) cannot be discovered, while `expat`, `zlib`, `iconv`, and `ffi` resolve to `/usr/lib/*.dylib`.
4. **Scoped `x265` enum nick exemption in `deps/macos-vips/audit-dylibs.sh:158-164`:** `libvips.42.dylib` always contains `"x265"` / `"VIPS_FOREIGN_HEIF_ENCODER_X265"` from `foreign.h:1065,1088,1100` even with `-DWITH_X265=OFF`. The audit exempts bare `"x265"` **only** in `libvips*.dylib`, while failing on `libx265`, `x265_encoder`, `x265_param`, any `nm -g` `x265` symbol everywhere, and any `"x265"` string in `libheif` or any other Mach-O binary.
5. **Eliminate `producer | grep -q` across all scripts and workflows:** Under `set -euo pipefail`, `grep -q` exits immediately on first match and closes stdin, causing large writers (`strings`, `nm`, `echo`) to fail with `Broken pipe` (`exit 141` under default `SIGPIPE`, or write-error `exit 1`/`2` when `SIGPIPE` is ignored on CI runners) and causing `if producer | grep -q` checks to fail open (`false`). Fixed via `mktemp` captures (`audit-dylibs.sh:148-164`, `audit-zip.sh`), variable captures, and `[[ ... ]]` checks.
6. **Specific `*"(heifload),"*` loader check (`.github/workflows/deps-macos-vips.yml:131`, `.github/workflows/ci.yml:586`):** Requiring `(heifload),` avoids matching `heifload_base` first and guarantees `VipsForeignLoadHeifFile (heifload)` is actually registered.
7. **Explicit runtime-only license split rules (`deps/macos-vips/build.sh`):** Validates that only LGPL/permissive runtime dylibs (`libintl.8.dylib` `LGPL-2.1-or-later`, `liblzma.5.dylib` `0BSD`, `libzstd.1.dylib` elected `BSD-3-Clause`) are bundled while CLI utilities carry the GPL clauses and are omitted, recording both `license` and `rawFormulaLicense` in `versions.json`.
8. **Compile-time unversioned symlinks in `lib/` (`build.sh`):** Emits unversioned `.dylib` symlinks (`libvips.dylib` -> `libvips.42.dylib`) alongside versioned runtime dylibs so `cargo build` links cleanly against `$VIPS_LIB_DIR`, while keeping Mach-O IDs as `@rpath/<versioned>.dylib`.
9. **Strict custom-prefix resolution & explicit Homebrew dev gating (`src-tauri/build.rs:1-64`, `scripts/package-bundle.mjs:71-106,164-205`):** Non-fork CI and release builds resolve `$VIPS_LIB_DIR`/`$VIPS_DIR`/`deps/vips/lib` strictly, embed `-Wl,-rpath` in `build.rs` so `cargo test` runs hermetically against `@rpath/` dylibs (stripped by `package-bundle.mjs` for `.app` release), forbid `/opt/homebrew`/`/usr/local`/`Cellar` in `strictCustomPrefix`, and gate `/opt/homebrew/lib` to `debug` profile or explicit `PHOTOS_DEV_HOMEBREW=1` (fork-PR fallback only). Wrapped `non_empty_env`/`is_env_truthy` in `#[cfg(target_os = "macos")]` (`aa66966`) after `09130ad` hit Windows `cargo clippy -- -D warnings` `dead_code`.
10. **Dereferencing compile symlinks to regular `.dylib` files when bundling (`scripts/package-bundle.mjs:243-246,326-328`, `scripts/check-dylib-table.mjs:41-50`):** `fs.copyFileSync` dereferences symlinks when copying into `Contents/Frameworks/` so only the **22** regular versioned `.dylib` files enter the `.app` bundle, and both `package-bundle.mjs` and `check-dylib-table.mjs` reject any symbolic link.
11. **Dual untrusted-loader control & fail-closed macOS CI (`.github/workflows/ci.yml:418-497,552-684`, `src-tauri/src/smoke.rs:423-440`):** Non-fork macOS CI requires `DEPS_TOKEN`, retries download 3 times with exit-code check, verifies SHA-256 `6bb24d51...`, sets `REQUIRE_HEIC=1`, verifies `22 == 22` dylibs, confirms both unblocked and `VIPS_BLOCK_UNTRUSTED=1` reject `fixture_blocked.svg` (`-Drsvg=disabled`), hides `/opt/homebrew` via `sudo mv`, and asserts `heic_decode_verified == true`. Fork PRs explicitly use Homebrew + `PHOTOS_DEV_HOMEBREW=1` with `REQUIRE_HEIC` unset (`heic_note: "no deps token (fork)"`).

---

## 7. Deviations from the task, spec or amendments

1. **Orchestrator Round 1 scope widening (`deps/win64-vips/audit-zip.sh`, committed `deps/macos-vips/test-audit.sh`):** Fixed the full `producer | grep -q` bug class across both OS audit scripts, populated `deps/macos-vips/UPSTREAM.txt` from green Run #3, and paused at Checkpoint 1 before starting Part B.
2. **Temporary scoped `push` trigger during Runs #1–#3 (removed in `e527f1f`):** GitHub rejects `workflow_dispatch` for workflows not yet on `main`; removed immediately after Run #3 passed, leaving `workflow_dispatch:` only as specified.
3. **`cgif` disabled on macOS (`-Dcgif=disabled -Dnsgif=true`):** Required because `libvips` `8.18.6` `meson.build:298` gates `cgif` on `imagequant` (`GPL-3.0-or-later` in Homebrew) or `quantizr` (unavailable). GIF reading works via built-in `nsgif=true`.
4. **Minor header text note in `docs/native-dependencies.md:3`:** Header text currently reads `macOS 15/27 (arm64)` alongside existing `macOS 27.0.0` note in §5.2, whereas the actual GHA macOS runners tested in TASK-0F were `macos-15-arm64` (`15.8.2`) and `macos-26-arm64` (`26.7.2`) plus local Apple Silicon macOS. Recorded accurately here without altering files after green Run `42958985250`.

---

## 8. Known gaps and risks

1. **Owner's manual physical-device validations remain (`docs/macos-validation.md` §§3–4):**
   - Real iPhone HEIC decode check (`§3`): owner needs to copy one of their own iPhone `.HEIC` files to `/tmp` and test with the custom `vips` build under stripped `PATH`.
   - Physical Windows laptop vs Apple Silicon cross-OS pixel check (`§4`) & physical Windows laptop run of `aa66966`: Windows x64 parity on `aa66966` was verified on GitHub Actions `windows-2025-vs2026` (`42958985250`, `heic_decode_verified: true`) rather than re-running on the physical Windows laptop.
2. **Intel macOS (`x86_64-apple-darwin`):** Out of scope for TASK-0F and untested.
3. **Fork-PR CI fallback path (`IS_FORK_PR=true`):** Verified statically and with `actionlint`, not triggered live from an external fork pull request.
4. **Separate non-copyright/deferred items:** HEVC patent licensing (Decision D13 / Phase 7 legal review before public release) and ExifTool's macOS `/usr/bin/perl` dependency (`docs/native-dependencies.md` §5.2) remain open for Phase 7 / post-Phase-0 per task scope.
5. **CI artifact retention:** `smoke-report-*` / `verify-logs-*` expire after 14 days and app installer artifacts after 3 days, while the underlying dependency releases (`macos-arm64-vips-8.18.6-hevc1` and `win64-vips-8.18.6-hevc1`) are stored permanently in private repository `narayanwaraich/photos-manager-deps`.

---

## 9. Questions for the human / orchestrator

1. **Owner manual check (`docs/macos-validation.md` §§3–4):** Please test decoding a temporary copy of one of your own iPhone HEIC photos using `deps/vips/bin/vipsthumbnail` (or the packaged `.app` artifact from Run `42958985250`) and run the cross-OS `vips getpoint` pixel comparison between your Mac and Windows laptop before or alongside merging `task/0F`.
2. **Orchestrator review:** All acceptance criteria, CI matrices, negative fail-closed proof (`42966724550`), and local `pnpm verify` checks are complete. Please review `task/0F` (`aa66966` + this report commit) for merge to `main`.

---

## 10. Proposed next task

Orchestrator review and merge of `task/0F` into `main` (alongside the owner's manual `docs/macos-validation.md` §3 real iPhone HEIC check), then proceed to the next task in `docs/NEXT_STEPS.md` (Phase 1 vertical slice kickoff once Phase 0 exit gates are confirmed). *(Not started here.)*

---

## Concepts used (for the owner, a React developer learning Rust and Git)

1. **Conditional Compilation (`#[cfg(target_os = "macos")]`):** Rust compiles functions marked with `#[cfg(target_os = "macos")]` only on macOS targets so Windows builds neither include nor warn (`dead_code` under `-D warnings`) on macOS-only helpers.  
   *Example:* `src-tauri/build.rs:1-14` & `:22-64`
2. **Dereferencing Compile Symlinks to Regular Bundle Libraries (`fs.copyFileSync` + `lstatSync`):** Unversioned `.dylib` symlinks exist for Cargo's linker at build time, while `fs.copyFileSync` writes the target bytes as a regular versioned `.dylib` file inside `.app/Contents/Frameworks/` so the bundle stays self-contained and matches the docs table 1:1.  
   *Example:* `scripts/package-bundle.mjs:243-246` & `scripts/check-dylib-table.mjs:41-50`
3. **Fail-Closed CI Secret, Retry & Checksum Gates:** Non-fork CI jobs verify `DEPS_TOKEN` is non-empty, capture download exit codes across retries, and compare exact SHA-256 digests before continuing, so missing credentials or missing releases abort the build instead of falling back silently.  
   *Example:* `.github/workflows/ci.yml:418-497`
4. **`SIGPIPE`-Safe Temp-File Audit Capture (`set -euo pipefail`):** Redirecting `strings -a` and `nm -g` to `mktemp` files before `grep -q` avoids early-exit broken-pipe errors (`141` / `1`) that otherwise make `if producer | grep -q` silently evaluate to `false` (fail-open).  
   *Example:* `deps/macos-vips/audit-dylibs.sh:146-165`
5. **Mach-O `@rpath` / `@executable_path` Relocation & Ad-Hoc Codesigning:** Rewriting dynamic library install IDs to `@rpath/<file>`, setting `LC_RPATH` to `@executable_path/../Frameworks` (executable) and `@loader_path` (libraries), and re-signing with `codesign --force --deep -s -` makes the `.app` run hermetically at any disk location with `/opt/homebrew` hidden.  
   *Example:* `scripts/package-bundle.mjs:266-310` & `src-tauri/build.rs:39`

# REPORT-0F: macOS arm64 libvips with HEVC Decode (libde265, no x265)

**Branch:** `task/0F` · **Commit:** `631a85b` (prior to this report commit) · **Date:** 2026-11-10
**OS actually tested:** macOS 15 arm64 (GitHub Actions `macos-15-arm64` `15.8.2` Build `24H232`, `Xcode 16.4` Build `16F6`) and macOS arm64 (local workstation — `shellcheck`, `actionlint`, synthetic `audit-dylibs.sh` good/bad tests, `pnpm verify`) · **Not tested:** Complete clean-runner HEIC/Display P3 decode & SVG control in `.github/workflows/deps-macos-vips.yml` (Run #2 `test-clean-macos` exited 1 at line 139 on `SIGPIPE` before fixture decode steps ran), Checkpoint 1 private release, Part B app CI integration (`ci.yml`, `src-tauri/build.rs`, `scripts/package-bundle.mjs`, `src-tauri/src/smoke.rs`), Windows x64 parity on `task/0F`, Intel macOS

---

## 1. Summary

**TASK-0F** completed Part A Steps 1–4 (`reports/REPORT-0F.md` §1.1 pre-build research at `5fd6cab`, the relocatable `deps/macos-vips/` recipe and audit suite plus `.github/workflows/deps-macos-vips.yml` at `f9cb8aa`, and fixes at `631a85b`) and executed **two** GitHub Actions workflow runs (`42804778977` and `42806945658`).
In Run #2 (`42806945658`), Job 1 (`build-audit`, ID `129798381168`) **passed**: it compiled `libde265` `1.1.1`, `libaom` `3.14.1` (decode-only), `libheif` `1.23.2` (no `x265`), and `libvips` `8.18.6` (`-Dmodules=disabled`, built-in `heifload`), relocated all 26 Mach-O files to `@rpath/`, passed every `audit-dylibs.sh` rule `(a)-(e)` (**6 LGPL** dynamically linked + **10 permissive**, **0 GPL libraries bundled**), and uploaded `vips-macos-arm64-hevc-staging` (`ID 13757674701`, `5,793,473` bytes, SHA-256 `04491934384424e05335029b7b2fc6853f5be2ede59c885da97dcce39b29e5f6`).
Job 2 (`test-clean-macos`, ID `129799793819`) verified `PASS: Zero non-system absolute paths`, hid `/opt/homebrew` (`sudo mv /opt/homebrew /opt/homebrew.off`), executed `env -i PATH="$CLEAN_PATH" "$VIPS_BIN/vips" -l` and printed `VipsForeignLoadHeifFile (heifload)`, then exited 1 at `.github/workflows/deps-macos-vips.yml:139` (`if ! echo "$LOADERS_OUT" | grep -q "heifload"`) because `grep -q` closed the pipe on first match and sent `SIGPIPE` (`Broken pipe`) to `echo` under `set -euo pipefail`.
**Mandatory Stop Condition Triggered (`TASK-0F` lines 32 & 71):** Because the workflow is not green after **two** runs, execution stopped immediately—no third run was triggered, Checkpoint 1 was not reached, and Part B was not started. Acceptance criteria 1–2 are **met**, criteria 3–4 are **partly met**, and criteria 5–7 are **not tested / not met** (options table in §9).

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
- `-DENABLE_DECODER=OFF` (`:188`, **defaults `ON`**; skips standalone `dec265` CLI binary), `-DENABLE_ENCODER=OFF` (`:189`), `-DENABLE_SHERLOCK265=OFF` (`:190`), `-DENABLE_INTERNAL_DEVELOPMENT_TOOLS=OFF` (`:191`), `-DWITH_FUZZERS=OFF` (`:192`).

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
  - `VipsForeignHeifCompression` (`@VIPS_FOREIGN_HEIF_COMPRESSION_HEVC: x265` at `:1065`) and `VipsForeignHeifEncoder` (`@VIPS_FOREIGN_HEIF_ENCODER_X265: x265` at `:1088`, enum `VIPS_FOREIGN_HEIF_ENCODER_X265` at `:1100`) compile the literal `"x265"` string into `libvips.42.dylib` unconditionally even when `libheif` is compiled with `-DWITH_X265=OFF`. Therefore `deps/macos-vips/audit-dylibs.sh:151-157` exempts bare `"x265"` in `libvips*.dylib` only, while forbidding `libx265`/`x265_encoder`/`x265_param` strings and all `nm -g` `x265` symbols everywhere, and forbidding bare `"x265"` in `libheif` and every other binary.
- Runner choice (`macos-15` arm64):
  - GitHub Actions retired `macos-14` on 2026-11-02 (`actions/runner-images`). We target `macos-15` (Apple Silicon arm64) with `MACOSX_DEPLOYMENT_TARGET=15.0` so built dylibs and Homebrew bottles run on `macos-15`, `ci.yml`'s `macos-latest` (`macos-26-arm64`), and newer Apple Silicon hosts.

---

## 2. Acceptance criteria

| Criterion | Result (met / partly / not met / not tested) | Evidence (command + output, or file) |
|---|---|---|
| Report section 1 written before any build, with `file:line` for the options | **met** | Commit `5fd6cab` (`reports/REPORT-0F.md` §1.1) preceded commit `f9cb8aa` and Run #1 `42804778977`; verified line numbers across `CMakeLists.txt`, `aom_config_defaults.cmake`, `meson_options.txt`, `meson.build`, `foreign.h:1063-1102`. |
| `audit-dylibs.sh` tested on good and bad inputs (output pasted) and run in workflow: `libde265` present, no `x265`/`fftw`/`imagequant`/`magick`/`poppler`/`rsvg` anywhere; no non-system absolute paths | **met** | Local tests on `deps/macos-vips/audit-dylibs.sh` (synthetic `good.zip` exit `0`, `bad-x265.zip` exit `1`, `bad-fftw.zip` exit `1`, `bad-homebrew-path.zip` exit `1`); Run #2 Job `129798381168` (`Audit built zip archive`) printed `PASS (a)`–`PASS (e)` and `OVERALL: AUDIT PASSED` (**6 LGPL** [`gettext`, `glib`, `libde265`, `libexif`, `libheif`, `vips`] dynamically linked, **10 permissive**, **GPL status: NONE (0 GPL libraries bundled)**). |
| Test job decodes both HEIC fixtures with Homebrew hidden and only the zip; raw output pasted | **partly** (`not met` — stopped at 2-run limit) | Run #2 Job `129799793819` (`test-clean-macos`) printed `PASS: Zero non-system absolute paths in unpacked binaries/dylibs`, hid `/opt/homebrew` -> `/opt/homebrew.off`, ran `vips -l` showing `VipsForeignLoadHeifFile (heifload)`, then exited 1 at `.github/workflows/deps-macos-vips.yml:139` on `SIGPIPE` (`Broken pipe`) under `set -euo pipefail` before steps 2–4 (fixture decodes & SVG control) executed. |
| Zip is an artifact; after Checkpoint 1 a private release with SHA-256 recorded in docs and checked by CI | **partly** (`not tested` for release & 14-day artifact) | Run #2 Job `129798381168` uploaded staging artifact `vips-macos-arm64-hevc-staging` (`ID 13757674701`, 1-day retention, `SHA-256: 04491934384424e05335029b7b2fc6853f5be2ede59c885da97dcce39b29e5f6`); 14-day `vips-macos-arm64-hevc` artifact, Checkpoint 1 private release `macos-arm64-vips-8.18.6-hevc1`, and CI SHA-256 check are not yet executed. |
| App CI green on macOS **and** Windows with `heic_decode_verified: true` on both; negative test fails closed | **not tested** | Part B not started due to 2-run stop condition. |
| §4.3 rewritten, table check passes, licences cited or marked unknown, GPL status stated | **not met** | `docs/native-dependencies.md` untouched on `task/0F`; archive `versions.json` & `LICENSES.md` record all 16 components (22 dylibs) with `GPL status: NONE` inside the built zip, awaiting Checkpoint 1 + Part B rewrite of §4.3. |
| `docs/macos-validation.md` written; report lists single-OS validation and owner's own-HEIC steps | **not met** | `docs/macos-validation.md` not created yet (Part B.5); single-OS validation status recorded in header & §8 below. |

---

## 3. Files changed

Output of `git diff --stat main...HEAD` at base commit `631a85b` (prior to committing this updated `reports/REPORT-0F.md`):
```
 .github/workflows/deps-macos-vips.yml | 231 ++++++++++
 deps/macos-vips/README.md             |  47 +++
 deps/macos-vips/UPSTREAM.txt          |  43 ++
 deps/macos-vips/audit-dylibs.sh       | 272 ++++++++++++
 deps/macos-vips/build.sh              | 769 ++++++++++++++++++++++++++++++++++
 reports/REPORT-0F.md                  | 139 ++++++
 6 files changed, 1501 insertions(+)
```
*(No files in `src-tauri/`, `scripts/`, `docs/`, `AGENTS.md`, or `ci.yml` were modified.)*

### Purpose of each changed file
- `.github/workflows/deps-macos-vips.yml`: Two-job macOS arm64 (`macos-15`) workflow (`build-audit` -> `test-clean-macos` with `/opt/homebrew` moved to `/opt/homebrew.off` and stripped `PATH`); currently retains the temporary scoped `push` trigger (lines 7–12) and hits a `SIGPIPE` bug under `set -euo pipefail` at line 139.
- `deps/macos-vips/README.md`: Build/layout documentation explaining why stock Homebrew `vips` cannot ship, how `PKG_CONFIG_LIBDIR` isolates leaf deps, and `@rpath`/`@loader_path` relocation layout.
- `deps/macos-vips/UPSTREAM.txt`: Pinned upstream tarball URLs, SHA-256 digests, licenses, excluded libraries, allowlisted Homebrew formula names, and runner target `macos-15`.
- `deps/macos-vips/audit-dylibs.sh`: Standalone audit enforcing rules `(a)`–`(e)` on filenames, `@rpath/` relocation and closure completeness, zero `x265` symbols/linkages (exempting the verified `libvips` enum nick string), `libde265` dynamic linkage in `libheif`, and `versions.json` completeness with zero GPL/unknown licenses.
- `deps/macos-vips/build.sh`: Reproducible single-script macOS arm64 builder for `libde265`, `libaom` (decode-only), `libheif`, and `libvips` (`-Dmodules=disabled`), harvesting the `@rpath` dylib closure (22 dylibs, 4 CLI binaries), ad-hoc `codesign`, `versions.json` / `LICENSES.md` generation, `.pc` prefix relocation, and zip packaging.
- `reports/REPORT-0F.md`: Pre-build upstream `file:line` analysis (committed at `5fd6cab`) updated here with real CI logs, test outputs, failure root-cause analysis, and remaining-options table at the 2-run stop gate.

---

## 4. Dependencies

No crates or npm packages were added (`src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, `package.json`, `pnpm-lock.yaml` unchanged). The table below lists the **16 runtime components** inside `vips-macos-arm64-8.18.6-hevc.zip` (4 built from verified source tarballs + 12 allowlisted Homebrew leaf libraries, verified from Run #2 `versions.json` in job `129798381168`) plus runner build tools/SDK shims:

| Name | Exact version | Source verified at | License | Added by this task? |
|---|---|---|---|---|
| `vips` | `8.18.6` | `https://github.com/libvips/libvips/releases/download/v8.18.6/vips-8.18.6.tar.xz` (`sha256:3c41e1d5...dda3e`) | `LGPL-2.1-or-later` | Yes (recipe tarball; dynamically linked) |
| `libheif` | `1.23.2` | `https://github.com/strukturag/libheif/releases/download/v1.23.2/libheif-1.23.2.tar.gz` (`sha256:8bd5d41d...405`) | `LGPL-3.0-or-later` | Yes (recipe tarball; dynamically linked) |
| `libde265` | `1.1.1` | `https://github.com/strukturag/libde265/releases/download/v1.1.1/libde265-1.1.1.tar.gz` (`sha256:fd48a927...219`) | `LGPL-3.0-or-later` | Yes (recipe tarball; dynamically linked) |
| `aom` (`libaom`) | `3.14.1` | `https://storage.googleapis.com/aom-releases/libaom-3.14.1.tar.gz` (`sha256:44bf90db...ef5`) | `BSD-2-Clause` | Yes (recipe tarball; decode-only) |
| `gettext` (`libintl.8.dylib`) | `1.0` | `homebrew:gettext` (`brew info --json=v2`) | `LGPL-2.1-or-later` *(raw formula: `GPL-3.0-or-later AND LGPL-2.1-or-later`; GPL applies only to unbundled CLI tools)* | Yes (recipe Homebrew leaf) |
| `glib` (`lib{glib,gobject,gio,gmodule}-2.0.0.dylib`) | `2.90.1` | `homebrew:glib` (`brew info --json=v2`) | `LGPL-2.1-or-later` | Yes (recipe Homebrew leaf) |
| `libexif` (`libexif.12.dylib`) | `0.6.26` | `homebrew:libexif` (`brew info --json=v2`) | `LGPL-2.1-or-later AND LGPL-2.0-or-later` | Yes (recipe Homebrew leaf) |
| `highway` (`libhwy.1.dylib`) | `1.4.0` | `homebrew:highway` (`brew info --json=v2`) | `Apache-2.0 OR BSD-3-Clause` | Yes (recipe Homebrew leaf) |
| `jpeg-turbo` (`libjpeg.8.dylib`) | `3.2.1` | `homebrew:jpeg-turbo` (`brew info --json=v2`) | `IJG AND Zlib AND BSD-3-Clause` | Yes (recipe Homebrew leaf) |
| `libpng` (`libpng16.16.dylib`) | `1.6.59` | `homebrew:libpng` (`brew info --json=v2`) | `libpng-2.0` | Yes (recipe Homebrew leaf) |
| `libtiff` (`libtiff.6.dylib`) | `4.7.2` | `homebrew:libtiff` (`brew info --json=v2`) | `libtiff` | Yes (recipe Homebrew leaf) |
| `little-cms2` (`liblcms2.2.dylib`) | `2.19.1` | `homebrew:little-cms2` (`brew info --json=v2`) | `MIT` | Yes (recipe Homebrew leaf) |
| `pcre2` (`libpcre2-8.0.dylib`) | `10.49` | `homebrew:pcre2` (`brew info --json=v2`) | `BSD-3-Clause` | Yes (recipe Homebrew leaf) |
| `webp` (`lib{webp,webpdemux,webpmux,sharpyuv}.*.dylib`) | `1.6.0` | `homebrew:webp` (`brew info --json=v2`) | `BSD-3-Clause` | Yes (recipe Homebrew leaf) |
| `xz` (`liblzma.5.dylib`) | `5.8.4` | `homebrew:xz` (`brew info --json=v2`) | `0BSD` *(raw formula: `0BSD AND GPL-2.0-or-later`; GPL applies only to unbundled CLI tools)* | Yes (recipe Homebrew leaf) |
| `zstd` (`libzstd.1.dylib`) | `1.5.7_1` | `homebrew:zstd` (`brew info --json=v2`) | `BSD-3-Clause` *(raw formula: `(BSD-3-Clause OR GPL-2.0-only) AND BSD-2-Clause AND MIT`; licensee elects `BSD-3-Clause`)* | Yes (recipe Homebrew leaf) |
| Build tools (`cmake`, `meson`, `ninja`, `pkgconf`) + `libffi` | `cmake 4.4.5`, `meson 1.12.1`, `ninja 1.13.2`, `pkgconf 3.0.10`, `libffi 3.8.0` | `brew list --versions` on `macos-15` runner | N/A (not bundled; `libffi`, `zlib`, `expat`, `iconv` resolved to `/usr/lib/*.dylib`) | Build-time only on GHA runner |
| `x265`, `fftw`, `imagequant`, `magick`, `poppler`, `rsvg`, `openslide`, `cgif` | **EXCLUDED** | Disabled in CMake/Meson + omitted from `PKG_CONFIG_LIBDIR` | **N/A** | **Explicitly excluded** |

- **LGPL libraries (6, dynamically linked, permitted):** `gettext` (`libintl.8.dylib`), `glib`, `libde265`, `libexif`, `libheif`, `vips`.
- **Permissive libraries (10):** `aom`, `highway`, `jpeg-turbo`, `libpng`, `libtiff`, `little-cms2`, `pcre2`, `webp`, `xz`, `zstd`.
- **GPL status:** **NONE (0 GPL libraries bundled).**

---

## 5. Commands run

### 5.1 GitHub Actions Run Log (`deps-macos-vips.yml` — 2 runs max)
```text
$ gh run list --workflow deps-macos-vips.yml --limit 5 --json databaseId,headSha,conclusion,status,createdAt,updatedAt,displayTitle,url
[
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
```

#### Run #1 (`42804778977` on `f9cb8aa`) — Failed on missing `export OUT_ZIP` in `build.sh`
```text
$ gh run view 42804778977
X task/0F Build & Test macOS arm64 libvips (HEVC Decode) · 42804778977
Triggered via push about 30 minutes ago

JOBS
X Build & audit macOS arm64 libvips (HEVC decode) in 3m30s (ID 129791019071)
  ✓ Set up job
  ✓ Checkout code
  ✓ Record runner OS & Xcode environment
  ✓ Install build tools and allowlisted leaf dependencies
  X Build relocatable libvips macOS arm64 archive
  - Audit built zip archive
  - Upload staging archive for clean-runner verification
  ✓ Post Checkout code
  ✓ Complete job
- Clean-runner test with Homebrew hidden (macos-15) in 0s (ID 129792707855)

...
2026-11-10T11:10:24.7717390Z Closure collected: 22 dylibs, 4 executables
2026-11-10T11:10:24.7949140Z Traceback (most recent call last):
2026-11-10T11:10:24.7950140Z   File "<stdin>", line 333, in <module>
2026-11-10T11:10:24.7951290Z   File "<frozen os>", line 729, in __getitem__
2026-11-10T11:10:24.7952030Z KeyError: 'OUT_ZIP'
2026-11-10T11:10:25.2121300Z ##[error]Process completed with exit code 1.
```
*(Fixed in commit `631a85b` by exporting `OUT_ZIP` at `deps/macos-vips/build.sh:355`, adding `STAGE_PREFIX` path normalization for `.pc` files at `:595-612`, and tightening `otool -L | tail -n +2` + `LC_RPATH` checks at `.github/workflows/deps-macos-vips.yml:113-122`.)*

#### Run #2 (`42806945658` on `631a85b`) — Job 1 (`129798381168`) Passed Build & Audit; Job 2 (`129799793819`) Failed on `SIGPIPE`
```text
$ gh run view 42806945658
X task/0F Build & Test macOS arm64 libvips (HEVC Decode) · 42806945658
Triggered via push about 14 minutes ago

JOBS
✓ Build & audit macOS arm64 libvips (HEVC decode) in 2m58s (ID 129798381168)
X Clean-runner test with Homebrew hidden (macos-15) in 9s (ID 129799793819)
  ✓ Set up job
  ✓ Checkout code
  ✓ Download staging libvips archive
  X Verify archive on clean runner with Homebrew hidden and stripped PATH
  - Upload verified libvips artifact (14-day retention)
  ✓ Post Checkout code
  ✓ Complete job

ARTIFACTS
vips-macos-arm64-hevc-staging
```

Relevant raw output from Run #2 Job 1 (`129798381168`, `build-audit` on `macos-15-arm64` `15.8.2` Build `24H232`, `Xcode 16.4` Build `16F6`):
```text
=== macOS Build Environment ===
OS Version:      15.8.2 (Deployment target: 15.0)
Toolchain:       Xcode 16.4 Build version 16F6
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

=== Archive SHA-256 and listing ===
-rw-r--r--  1 runner  staff  5793473 Nov 10 11:25 /Users/runner/work/photos-manager/photos-manager/vips-macos-arm64-8.18.6-hevc.zip
04491934384424e05335029b7b2fc6853f5be2ede59c885da97dcce39b29e5f6  /Users/runner/work/photos-manager/photos-manager/vips-macos-arm64-8.18.6-hevc.zip
...
Artifact vips-macos-arm64-hevc-staging.zip successfully finalized. Artifact ID 13757674701
```

Relevant raw output from Run #2 Job 2 (`129799793819`, `test-clean-macos` on `macos-15-arm64` `15.8.2` Build `24H232`):
```text
04491934384424e05335029b7b2fc6853f5be2ede59c885da97dcce39b29e5f6  /tmp/vips-staging/vips-macos-arm64-8.18.6-hevc.zip
Extracted prefix: /tmp/vips-clean-test/vips-macos-arm64-8.18.6-hevc
=== Verifying otool -L across unpacked lib/* and bin/* ===
PASS: Zero non-system absolute paths in unpacked binaries/dylibs
=== 1. Check vips -l lists heifload with stripped PATH and hidden Homebrew ===
        VipsForeignLoadNsgif (gifload_base), load GIF with libnsgif, priority=50
...
        VipsForeignLoadPng (pngload_base), load png base class, priority=200
...
        VipsForeignLoadJpeg (jpegload_base), load jpeg, priority=50
...
        VipsForeignLoadWebp (webpload_base), load webp, priority=200
...
        VipsForeignLoadTiff (tiffload_base), load tiff, priority=50
...
        VipsForeignLoadHeif (heifload_base), load a HEIF image, priority=0
/Users/runner/work/_temp/e7999848-3d38-4cac-af5f-a583bc86032a.sh: line 51: echo: write error: Broken pipe
          VipsForeignLoadHeifFile (heifload), load a HEIF image (.heic, .heif, .avif), priority=0, is_a, get_flags, header, load
          VipsForeignLoadHeifBuffer (heifload_buffer), load a HEIF image, priority=0, is_a_buffer, get_flags, header, load
          VipsForeignLoadHeifSource (heifload_source), load a HEIF image, nocache, priority=0, is_a_source, get_flags, header, load
ERROR: vips -l does NOT list heifload!
##[error]Process completed with exit code 1.
```

### 5.2 Local Script Linting & `audit-dylibs.sh` Tests (Known Good + 3 Known Bad Fixtures)
```text
$ shellcheck -x deps/macos-vips/build.sh deps/macos-vips/audit-dylibs.sh && actionlint -verbose .github/workflows/deps-macos-vips.yml
verbose: Linting .github/workflows/deps-macos-vips.yml
verbose: Using project at /Users/narayanwaraich/Desktop/Projects/photos-manager
verbose: Found 0 parse errors in 0 ms for .github/workflows/deps-macos-vips.yml
verbose: Rule "pyflakes" was disabled: exec: "pyflakes": executable file not found in $PATH
verbose: Found total 0 errors in 68 ms for .github/workflows/deps-macos-vips.yml
```

```text
--- Running audit-dylibs.sh on KNOWN GOOD ---
=== Auditing macOS libvips archive: good.zip ===
PASS (a): No forbidden filenames (x265|x264|fftw|imagequant|magick|poppler|rsvg|openslide) in archive
PASS (b): All 4 Mach-O files use @rpath/ cleanly with zero Homebrew/local path leaks or forbidden linkages
PASS (c): Zero x265 symbols or linkage strings across all Mach-O files (enum nick verified absent from all non-libvips files)
PASS (d): libde265.0.dylib present and dynamically linked by libheif.1.dylib
  Verified 3 components covering 3 regular dylibs.
  LGPL components: libde265 (LGPL-3.0-or-later), libheif (LGPL-3.0-or-later), vips (LGPL-2.1-or-later)
  Permissive components: 
  GPL status: NONE (0 GPL libraries bundled)
PASS (e): versions.json valid, libde265 present, all dylibs accounted for, zero unknown/GPL licenses
OVERALL: AUDIT PASSED
EXIT_CODE=0

--- Running audit-dylibs.sh on KNOWN BAD 1 (x265 present) ---
=== Auditing macOS libvips archive: bad-x265.zip ===
PASS (a): No forbidden filenames (x265|x264|fftw|imagequant|magick|poppler|rsvg|openslide) in archive
PASS (b): All 4 Mach-O files use @rpath/ cleanly with zero Homebrew/local path leaks or forbidden linkages
  libheif.1.dylib exports/imports x265 symbol in Mach-O symbol table
FAIL (c): Forbidden x265 strings or symbols detected
PASS (d): libde265.0.dylib present and dynamically linked by libheif.1.dylib
  Verified 3 components covering 3 regular dylibs.
  LGPL components: libde265 (LGPL-3.0-or-later), libheif (LGPL-3.0-or-later), vips (LGPL-2.1-or-later)
  Permissive components: 
  GPL status: NONE (0 GPL libraries bundled)
PASS (e): versions.json valid, libde265 present, all dylibs accounted for, zero unknown/GPL licenses
OVERALL: AUDIT FAILED
EXIT_CODE=1

--- Running audit-dylibs.sh on KNOWN BAD 2 (fftw present) ---
=== Auditing macOS libvips archive: bad-fftw.zip ===
FAIL (a): Found forbidden filename matching (x265|x264|fftw|imagequant|magick|poppler|rsvg|openslide):
/var/folders/vw/sxxzw9pd58qf70hwkhrlgsdr0000gn/T/tmp.lnfuXjoggt/vips-macos-arm64-8.18.6-hevc/lib/libfftw3.3.dylib
PASS (b): All 5 Mach-O files use @rpath/ cleanly with zero Homebrew/local path leaks or forbidden linkages
PASS (c): Zero x265 symbols or linkage strings across all Mach-O files (enum nick verified absent from all non-libvips files)
PASS (d): libde265.0.dylib present and dynamically linked by libheif.1.dylib
  Disallowed component in versions.json: fftw
  Component 'fftw' has disallowed GPL license: 'GPL-2.0-or-later'
FAIL (e): versions.json audit failed
OVERALL: AUDIT FAILED
EXIT_CODE=1

--- Running audit-dylibs.sh on KNOWN BAD 3 (/opt/homebrew path left in otool -L) ---
=== Auditing macOS libvips archive: bad-homebrew-path.zip ===
PASS (a): No forbidden filenames (x265|x264|fftw|imagequant|magick|poppler|rsvg|openslide) in archive
  libheif.1.dylib has non-system, non-@rpath dependency path: /opt/homebrew/lib/libde265.0.dylib
FAIL (b): Relocatable otool -L or forbidden dependency check failed
PASS (c): Zero x265 symbols or linkage strings across all Mach-O files (enum nick verified absent from all non-libvips files)
PASS (d): libde265.0.dylib present and dynamically linked by libheif.1.dylib
  Verified 3 components covering 3 regular dylibs.
  LGPL components: libde265 (LGPL-3.0-or-later), libheif (LGPL-3.0-or-later), vips (LGPL-2.1-or-later)
  Permissive components: 
  GPL status: NONE (0 GPL libraries bundled)
PASS (e): versions.json valid, libde265 present, all dylibs accounted for, zero unknown/GPL licenses
OVERALL: AUDIT FAILED
EXIT_CODE=1
```

### 5.3 Enum Nick Exemption Proof (`x265` in `libvips.42.dylib` only)
```text
$ strings -a /opt/homebrew/lib/libvips.42.dylib | grep -i x265 ; nm -gU /opt/homebrew/lib/libvips.42.dylib | grep -i x265 || echo "(0 exported/imported x265 symbols)"
VIPS_FOREIGN_HEIF_ENCODER_X265
x265
(0 exported/imported x265 symbols)
```

### 5.4 Local Verification Suite (`pnpm verify`)
```text
$ node scripts/verify.mjs
Photos Manager — pnpm verify verification run
Date: 2026-11-10T11:39:29.829Z
Log output file: /Users/narayanwaraich/Desktop/Projects/photos-manager/reports/logs/verify-2026-11-10.txt

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
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.73s

✓ PASSED: cargo clippy

======================================================
STEP: cargo test
COMMAND: cargo test --manifest-path src-tauri/Cargo.toml
CWD: /Users/narayanwaraich/Desktop/Projects/photos-manager
======================================================


running 18 tests
test filesystem::tests::test_windows_cases_note ... ok
test filesystem::tests::test_roundtrip_non_ascii_path ... ok
test filesystem::tests::test_macos_nfc_vs_nfd_normalization_probe ... ok
test filesystem::tests::test_roundtrip_non_utf8_path_unix ... ok
test metadata::tests::test_exiftool_missing_file ... ok
test database::tests::test_init_pool_and_app_settings_migration ... ok
test metadata::tests::test_exiftool_rejects_non_absolute_path ... ok
test image_pipeline::vips::tests::test_vips_supported_loaders_reports_real_codecs ... ok
test image_pipeline::vips::tests::test_vips_missing_file_error ... ok
test metadata::tests::test_exiftool_timeout ... ok
test image_pipeline::vips::tests::test_vips_blocks_svgload ... ok
test metadata::tests::test_exiftool_success ... ok
test metadata::tests::test_exiftool_malformed_output ... ok
test metadata::tests::test_exiftool_non_ascii_filename ... ok
test metadata::tests::test_exiftool_long_path ... ok
test image_pipeline::vips::tests::test_vips_thumbnail_png ... ok
test image_pipeline::vips::tests::test_vips_thumbnail_rotated_jpeg ... ok
test metadata::tests::test_child_killed_after_timeout ... ok

test result: ok. 18 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.06s
...
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

✓ built in 56ms
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
*(Verification commands not run due to 2-run stop / pre-Checkpoint-1 state: `gh workflow run deps-macos-vips.yml`, `gh release view macos-arm64-vips-8.18.6-hevc1`, `gh run list --workflow ci.yml --branch task/0F` — CI also ignores `reports/**` pushes.)*

---

## 6. Decisions made

1. **Runner `macos-15` arm64 (`MACOSX_DEPLOYMENT_TARGET=15.0`):** `macos-14` retired on 2026-11-02 (`actions/runner-images`). `macos-15` is the active GitHub Actions Apple Silicon baseline and produces binaries compatible with both `macos-15` and `ci.yml`'s `macos-latest` (`macos-26-arm64`).
2. **Disable `cgif`, enable `nsgif` (`-Dcgif=disabled -Dnsgif=true`):** In `vips-8.18.6/meson.build:298`, `cgif_dep` is gated inside `if quantisation_package.found()` (`imagequant` or `quantizr`). Homebrew `libimagequant` 4.4.1 is `GPL-3.0-or-later` (forbidden) and `quantizr` is unavailable in Homebrew, whereas Windows MXE uses permissively licensed `libimagequant` `2.4.1` (`BSD-2-Clause`). GIF reading stays enabled via `-Dnsgif=true`; GIF writing via `cgif` is disabled until/unless a permissive quantizer is built from source.
3. **Isolated `PKG_CONFIG_LIBDIR` (skipping `/opt/homebrew/lib/pkgconfig` and Homebrew `libffi`):** Clearing `PKG_CONFIG_PATH` and constructing `PKG_CONFIG_LIBDIR` strictly from `STAGE_PREFIX/lib/pkgconfig`, allowlisted leaf kegs (`deps/macos-vips/build.sh:89-145`), and Apple's macOS SDK shims guarantees runner-preinstalled formulas (`fftw`, `x265`, `librsvg`, `poppler`, `imagemagick`) can never be accidentally discovered by CMake or Meson, while `expat`, `zlib`, `iconv`, and `ffi` resolve to `/usr/lib/*.dylib`.
4. **Scoped `x265` enum nick exemption in `audit-dylibs.sh:151-157`:** `libvips.42.dylib` always contains `"x265"` / `"VIPS_FOREIGN_HEIF_ENCODER_X265"` from `foreign.h:1065,1088,1100` even with `-DWITH_X265=OFF`. The audit exempts bare `"x265"` **only** in `libvips*.dylib`, while failing on `libx265`, `x265_encoder`, `x265_param`, any `nm -g` `x265` symbol anywhere, and any `"x265"` string in `libheif` or any other Mach-O file.
5. **Explicit runtime-only license split rules (`build.sh:629-648`):** `brew info --json=v2` reports formula-wide SPDX strings (`GPL-3.0-or-later AND LGPL-2.1-or-later` for `gettext`; `0BSD AND GPL-2.0-or-later` for `xz`; `(BSD-3-Clause OR GPL-2.0-only) AND BSD-2-Clause AND MIT` for `zstd`). Because only the LGPL/permissive runtime dylibs (`libintl.8.dylib`, `liblzma.5.dylib`, `libzstd.1.dylib`) are bundled and CLI utilities are omitted, `build.sh` verifies `expected_raw` + `expected_dylibs` and records both `license` and `rawFormulaLicense` in `versions.json`.
6. **Compile-time unversioned symlinks in `lib/` (`build.sh:576-585`):** Created unversioned `.dylib` symlinks (`libvips.dylib` -> `libvips.42.dylib`, etc.) alongside real versioned dylibs so `cargo build` (`-lvips -lglib-2.0 -lgobject-2.0`) links cleanly in Part B while `install_name_tool` IDs remain `@rpath/<versioned>.dylib`.

---

## 7. Deviations from the task, spec or amendments

1. **Workflow not green within 2 runs (Stop Condition Triggered):** Stopped after Run #2 (`42806945658`) without editing `.github/workflows/deps-macos-vips.yml`, starting Run #3, reaching Checkpoint 1, or starting Part B.
2. **Temporary scoped `push` trigger still present in `.github/workflows/deps-macos-vips.yml:5-12`:** Task Part A.5 specifies `workflow_dispatch` only. GitHub rejects `workflow_dispatch` for workflows not yet on the default branch (`main`), so a scoped `push` trigger on `task/0F` (`paths: [.github/workflows/deps-macos-vips.yml, deps/macos-vips/**]`) was included temporarily, intended to be removed in a `[skip ci]` commit after clean-runner verification passes. Because Run #2 hit the 2-run stop gate, that temporary trigger remains in place.
3. **`cgif` disabled on macOS vs Task Part A.1 wording (`"keep ... cgif ... as in the Windows zip"`):** Disabled (`-Dcgif=disabled`, `-Dnsgif=true`) because `libvips` `8.18.6` `meson.build:298` requires `imagequant` or `quantizr` before detecting `cgif`, and Homebrew `libimagequant` is `GPL-3.0-or-later` (Decision #2 above).

---

## 8. Known gaps and risks

1. **Exact failure blocking clean-runner verification (`.github/workflows/deps-macos-vips.yml:139`):** Under `set -euo pipefail` (line 89), `if ! echo "$LOADERS_OUT" | grep -q "heifload"` fails with `echo: write error: Broken pipe` (exit `141`) because `vips -l` emits ~30 KB of output and `grep -q` closes stdin as soon as it sees `VipsForeignLoadHeif` before `echo` finishes writing. Identical latent `echo "$VAR" | grep -q` checks exist on lines `143, 154, 162, 173, 181, 199, 213`. Because Run #2 exited at line 139, steps 2–4 (`image_orientation_6.heic` 240x320/480x640 decode, `image_display_p3.heic` 640x480/320x240 decode, unblocked + `VIPS_BLOCK_UNTRUSTED=1` SVG rejection, and 14-day `vips-macos-arm64-hevc` artifact upload) have not yet executed in CI.
2. **Staging artifact expiry:** `vips-macos-arm64-hevc-staging` (`ID 13757674701`) has `retention-days: 1` (built `2026-11-10T11:25Z`, expires `2026-11-11`).
3. **Part B app integration not started:**
   - `.github/workflows/ci.yml:418-530` still installs Homebrew `vips` and sets `PHOTOS_DEV_HOMEBREW: "1"`.
   - `src-tauri/build.rs:9-19` checks `VIPS_LIB_DIR` on macOS, not `VIPS_DIR` (unlike Windows at `:27-31`).
   - `scripts/package-bundle.mjs:91,102,135` hardcodes `/opt/homebrew/lib` when resolving `@rpath/` dylibs instead of checking `$VIPS_LIB_DIR` / `$VIPS_DIR/lib` first, and must avoid copying unversioned compile symlinks into `Contents/Frameworks/` alongside the versioned `.dylib` files (which would cause `scripts/check-dylib-table.mjs` mismatch or duplicate copies).
   - `docs/native-dependencies.md` §4.1/§4.3 (67-row Homebrew table) and `docs/macos-validation.md` (with owner's real-iPhone HEIC copy steps) remain untouched.
4. **Single-OS testing & hardware gaps:** Tested only on Apple Silicon (`macos-15-arm64` in CI and local M-series macOS). Intel macOS (`x86_64`) and Windows `x64` were not tested on `task/0F`, and own-iPhone HEIC hardware verification awaits Checkpoint 1 / Part B.
5. **Runtime/build risks:** Homebrew bottle formula versions (`glib 2.90.1`, etc.) float over time on future runs of `build.sh` (though once built and SHA-256 pinned in `narayanwaraich/photos-manager-deps`, app CI will no longer float). HEVC patent pool licensing (Decision D13 / Phase 7) remains separate from copyright licenses and must be reviewed before any public/commercial distribution.

---

## 9. Questions for the human / orchestrator

### Remaining Options Table (Required by `TASK-0F` lines 32 & 71)

| Option | Description | Licence Impact | Effort | Risk |
|---|---|---|---|---|
| **Option 1 (Recommended): Patch `SIGPIPE` bash checks & approve Run #3** | Replace `echo "$VAR" \| grep -q` on `.github/workflows/deps-macos-vips.yml:139,143,154,162,173,181,199,213` with bash substring/regex checks (`[[ "$VAR" == *...* ]]` / `[[ ... =~ ... ]]`) while keeping `set -euo pipefail`, authorize Run #3, then proceed to Checkpoint 1 -> Part B (remove temporary `push` trigger via `[skip ci]` after Run #3 passes). | Same (**6 LGPL + 10 permissive, 0 GPL**). | Low (~5 min YAML edit + ~3 min CI run). | **Low.** Job 1 (`build-audit`) already passed completely, and `vips -l` with `/opt/homebrew` hidden already printed `VipsForeignLoadHeifFile (heifload)`. |
| **Option 2: Verify Run #2 staging artifact manually + defer workflow fix** | Download `vips-macos-arm64-hevc-staging` (`ID 13757674701`, SHA-256 `04491934...`) before its 24h expiry, verify both HEIC fixtures locally on macOS with Homebrew hidden, publish it as `macos-arm64-vips-8.18.6-hevc1` for Checkpoint 1, and push the YAML `SIGPIPE` fix unrun before merge (matching 0E's `872ae48` precedent). | Same (**0 GPL**). | Low-Medium (~15 min manual download & test + CI integration; tight 24h TTL window). | **Medium.** Skips independent clean-runner GHA confirmation of HEIC decode in `deps-macos-vips.yml` prior to Part B `ci.yml` integration and loses artifact if 24h TTL lapses first. |
| **Option 3: Drop `-o pipefail` from `test-clean-macos`** | Change `set -euo pipefail` (`.github/workflows/deps-macos-vips.yml:89`) to `set -eu` and re-run Run #3. | Same (**0 GPL**). | Trivial (~2 min edit + ~3 min CI run). | **Medium.** Weakens shell script safety by masking non-zero exits in earlier pipeline stages; Option 1 fixes the root bug while keeping `pipefail` strictness. |
| **Option 4 (Disallowed by D10, D18 & Task Stop Rules): Use macOS ImageIO or Homebrew `libheif`** | Decode HEIC via macOS ImageIO (`objc2`) or ship Homebrew's `libheif` / `vips-heif.dylib`. | System-framework coupling (ImageIO) or **GPL** (`x265` hard-linked in Homebrew `libheif`). | High / Forbidden. | **High / Policy violation.** Explicitly disallowed by `TASK-0F` lines 15, 71 and D10/D18. |

### Numbered Questions Needing Orchestrator Decision
1. **Run #3 authorization:** Do you approve **Option 1** (fixing `deps-macos-vips.yml` lines `139,143,154,162,173,181,199,213` to eliminate `SIGPIPE` and running Run #3 beyond the 2-run cap), or do you prefer **Option 2** using staging artifact `ID 13757674701` (`SHA-256: 04491934384424e05335029b7b2fc6853f5be2ede59c885da97dcce39b29e5f6`)?
2. **`cgif` exclusion confirmation:** Confirm keeping `-Dcgif=disabled -Dnsgif=true` on macOS (GIF decode via `nsgif` works; GIF encode via `cgif` cannot be enabled without a permissive quantizer like `quantizr` or `libimagequant 2.4.1`, neither of which is in Homebrew).
3. **Temporary `push` trigger cleanup timing:** Confirm keeping `.github/workflows/deps-macos-vips.yml:5-12` until Run #3 passes, then removing it in a `[skip ci]` commit (`workflow_dispatch` only) prior to merging `task/0F`.

---

## 10. Proposed next task

Upon orchestrator approval of **Option 1**, resume **TASK-0F** on branch `task/0F`: replace the `echo "$VAR" | grep -q` pipelines in `.github/workflows/deps-macos-vips.yml` with bash `[[ ... == *...* ]]` checks under `set -euo pipefail`, execute Run #3 to confirm clean-runner decoding of both HEIC fixtures (`image_orientation_6.heic` and `image_display_p3.heic`) and SVG blocking with Homebrew hidden, strip the temporary `push` trigger once green, and stop at **Checkpoint 1** for the owner to publish `macos-arm64-vips-8.18.6-hevc1` before implementing Part B (`ci.yml`, `build.rs`, `package-bundle.mjs`, `docs/native-dependencies.md` §4.3, `docs/macos-validation.md`).

---

## Concepts used (for the owner, a React developer learning Rust and Git)

1. **Mach-O `@rpath` and `@loader_path` Relocation**: Instead of hardcoding `/opt/homebrew/lib/...`, macOS dynamic libraries declare an install ID starting with `@rpath/` and an `LC_RPATH` search entry (`@loader_path` for libraries, `@executable_path/../lib` for executables) so the bundle runs identically from any folder even when Homebrew is absent.  
   *Example:* `deps/macos-vips/build.sh:536-564`
2. **POSIX `SIGPIPE` Under `set -euo pipefail`**: When `echo` writes a large buffer into `grep -q`, `grep` exits `0` immediately on its first match and closes the pipe, sending `SIGPIPE` (exit `141`) to `echo`; with `pipefail` active, bash treats that broken pipe as a pipeline failure.  
   *Example:* `.github/workflows/deps-macos-vips.yml:89,139`
3. **Isolated `PKG_CONFIG_LIBDIR` for Clean Builds**: Clearing `PKG_CONFIG_PATH` and building `PKG_CONFIG_LIBDIR` strictly from the staging prefix, allowlisted Homebrew kegs, and Apple's SDK shims prevents CMake and Meson from accidentally discovering disallowed runner packages like `fftw` or `x265`.  
   *Example:* `deps/macos-vips/build.sh:105-145`
4. **Build-Time Link Symlinks vs. Runtime `SONAME`s**: Linkers and `build.rs` pass `-lvips` expecting an unversioned `libvips.dylib` symlink, whereas macOS `dyld` records and resolves only the versioned `@rpath/libvips.42.dylib` file at runtime.  
   *Example:* `deps/macos-vips/build.sh:576-585`
5. **Scoped Workflow `paths:` Filters for CI Isolation**: Narrowing a temporary `push` trigger with `paths:` ensures committing or pushing reports (`reports/REPORT-0F.md`) does not trigger or re-run the expensive dependency-build workflow.  
   *Example:* `.github/workflows/deps-macos-vips.yml:7-12`

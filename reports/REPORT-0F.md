# REPORT-0F: macOS arm64 libvips with HEVC Decode (libde265, no x265)

**Branch:** `task/0F` · **Commit:** pending (pre-build Section 1 commit) · **Date:** 2026-11-10
**OS actually tested:** macOS arm64 (primary dev machine — source tarball verification and pre-build audit only) · **Not tested:** GitHub Actions macOS build, clean-runner HEIC decode, Windows CI integration (pending subsequent steps)

---

## 1. Summary & Pre-Build Analysis (Read Before Building)

This report tracks **TASK-0F** to build a standalone, relocatable macOS arm64 `libvips` `8.18.6` distribution with `libheif` `1.23.2` + `libde265` `1.1.1` (HEVC decode) + `libaom` `3.14.1` (AV1 decode) and **no x265, no fftw, no imagequant, no ImageMagick, no poppler, and no librsvg**, verify it on a clean runner with Homebrew physically hidden, and integrate it into the app build and CI (`ci.yml`).

*(Sections 2–10 below will be completed with raw command/CI evidence after build, Checkpoint 1, and Part B integration.)*

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
Defaults that are dangerously **`ON`** upstream (`X265`, `X264`, `OpenH264_DECODER`, `AOM_ENCODER`, `ENABLE_PLUGIN_LOADING`, examples/tests/docs/gdk-pixbuf) are explicitly overridden:
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
  - Upstream `meson.build:298` gates `cgif_dep = dependency('cgif', ...)` strictly inside `if quantisation_package.found()`, where `quantisation_package` is set ONLY by `imagequant_dep` (`meson.build:279-284`) or `quantizr_dep` (`meson.build:288-294`). Because Homebrew `libimagequant` 4.4.1 is `GPL-3.0-or-later` (disallowed) and neither `imagequant` nor `quantizr` is used (`-Dimagequant=disabled -Dquantizr=disabled`), `cgif` is never linked even though `cgif` itself is MIT (`-Dcgif=disabled`, `meson_options.txt:50`). GIF *load* stays enabled via `nsgif=true`.
- Explicitly disallowed / unneeded loaders & libraries:
  - `-Dfftw=disabled` (`meson_options.txt:60`), `-Dimagequant=disabled` (`:85`), `-Dquantizr=disabled` (`:206`), `-Dmagick=disabled` (`:115`), `-Dmagick-module=disabled` (`:131`), `-Drsvg=disabled` (`:216`), `-Dpoppler=disabled` (`:196`), `-Dpoppler-module=disabled` (`:201`), `-Dopenslide=disabled` (`:161`), `-Dopenslide-module=disabled` (`:166`), `-Dopenexr=disabled` (`:151`), `-Dopenjpeg=disabled` (`:156`), `-Djpeg-xl=disabled` (`:100`), `-Djpeg-xl-module=disabled` (`:105`), `-Dnifti=disabled` (`:141`), `-Dmatio=disabled` (`:136`), `-Dpdfium=disabled` (`:186`), `-Dcfitsio=disabled` (`:45`), `-Darchive=disabled` (`:70`), `-Dpangocairo=disabled` (`:181`), `-Dfontconfig=disabled` (`:65`), `-Duhdr=disabled` (`:95`), `-Draw=disabled` (`:211`), `-Dppm=false` (`:249`), `-Danalyze=false` (`:254`), `-Dradiance=false` (`:259`).
- Note on `"x265"` enum nick in `libvips 8.18.6` (`vips-8.18.6/libvips/include/vips/foreign.h:1063-1102`):
  - `VipsForeignHeifCompression` (`nick: "x265"`, line `1065`? wait — `foreign.h:1065` & `1088` verified: `VIPS_FOREIGN_HEIF_COMPRESSION_HEVC` and `VIPS_FOREIGN_HEIF_ENCODER_X265`) compiles the literal `"x265"` string into `enumtypes.c` in `libvips.42.dylib` unconditionally even with `-DWITH_X265=OFF`. Therefore `audit-dylibs.sh` exempts bare `"x265"` in `libvips.42.dylib` (while verifying via `nm -gU` and `otool -L` that ZERO `x265` symbols or linkage targets exist anywhere, and forbidding bare `"x265"` in `libheif` and every other dylib).
- Runner choice (`macos-15` arm64):
  - GitHub Actions retired `macos-14` on 2026-11-02 (`actions/runner-images`). We target `macos-15` (Apple Silicon arm64) with `MACOSX_DEPLOYMENT_TARGET=15.0` so built dylibs and Homebrew bottles run both on `macos-15` and on `ci.yml`'s `macos-latest` (`macos-26-arm64`) as well as newer macOS hosts.
  - Note on `workflow_dispatch` + task-branch execution: because GitHub rejects `gh workflow run` for a workflow not yet on `main`, `.github/workflows/deps-macos-vips.yml` will carry a temporary scoped `push` trigger during `task/0F` verification and then be reduced to `workflow_dispatch` only via a final `[skip ci]` commit, matching the accepted `0E` pattern.

---

## 2. Acceptance criteria

| Criterion | Result | Evidence |
|---|---|---|
| Report section 1 written before any build, with file:line for the options | **Met (Part A Step 1)** | Section 1.1 above, committed before any workflow build |
| `audit-dylibs.sh` tested on good and bad inputs and run in workflow | Pending | To be run in Part A |
| Test job decodes both HEIC fixtures with Homebrew hidden and only the zip | Pending | To be run in Part A |
| Zip is an artifact; after Checkpoint 1 a private release with SHA-256 recorded and checked by CI | Pending | To be run in Part A / B |
| App CI green on macOS and Windows with `heic_decode_verified: true`; negative test fails closed | Pending | To be run in Part B |
| §4.3 rewritten, table check passes, licences cited or marked unknown, GPL status stated | Pending | To be run in Part B |
| `docs/macos-validation.md` written; single-OS validation and own-HEIC steps documented | Pending | To be run in Part B |

---

## 3. Files changed

Pending completion of Part A and Part B.

---

## 4. Dependencies

Pending completion of `build.sh` and `versions.json` generation.

---

## 5. Commands run

Pending completion of local tests, `pnpm verify`, and CI runs.

---

## 6. Decisions made

Pending completion of Part A and Part B.

---

## 7. Deviations from the task, spec or amendments

None recorded yet (`macos-14` retired 2026-11-02 -> `macos-15` arm64 runner selected within Task §Part A.5 allowance).

---

## 8. Known gaps and risks

Pending completion of Part A and Part B.

---

## 9. Questions for the human / orchestrator

None prior to Checkpoint 1.

---

## 10. Proposed next task

Pending completion of TASK-0F.

---

## Concepts used (for the owner, a React developer learning Rust and Git)

Pending completion of TASK-0F.

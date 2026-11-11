# REPORT-0F: macOS arm64 libvips with HEVC Decode (libde265, no x265) — Part A & Checkpoint 1

**Branch:** `task/0F` · **Commit:** `e7e976a` (last code commit before final `[skip ci]` trigger/metadata/report commit) · **Date:** 2026-11-11
**OS actually tested:** macOS 15 arm64 (GitHub Actions `macos-15-arm64` `15.8.2` Build `24H232`, `Xcode 16.4` Build `16F6`) and macOS arm64 (local workstation — `shellcheck`, `actionlint`, `test-audit.sh` regression suite, Windows `audit-zip.sh` regression/stock tests, Run #2 pre-flight & `__cstring` patch test, `pnpm verify`) · **Not tested:** Checkpoint 1 private release (`macos-arm64-vips-8.18.6-hevc1`) & Part B app CI integration (`ci.yml`, `src-tauri/build.rs`, `scripts/package-bundle.mjs`, `docs/native-dependencies.md` §4.3, `docs/macos-validation.md`), Intel macOS (`x86_64`), own-iPhone HEIC sample validation; Windows x64 parity in `ci.yml` (`42804777791`, `42806945665`, and `42945772425`) ran green across all three jobs (`macos-arm64`, `windows-x64`, `Security & License check (cargo deny)`), which proves only that nothing regressed because `ci.yml` macOS builds still use Homebrew `vips` until Part B.

---

## 1. Summary

**TASK-0F Part A is complete and Run #3 (`42945772807` on commit `e7e976a`) of `.github/workflows/deps-macos-vips.yml` is fully green.** Job 1 (`build-audit`, ID `130278088973`) built `libde265` `1.1.1`, `libaom` `3.14.1` (decode-only), `libheif` `1.23.2` (no `x265`), and `libvips` `8.18.6` (`-Dmodules=disabled`, built-in `heifload`) from pinned source tarballs with isolated allowlisted Homebrew leaves, relocated all 26 Mach-O files to `@rpath/`, passed every `audit-dylibs.sh` rule `(a)`–`(e)` (**6 LGPL** dynamically linked + **10 permissive**, **0 GPL libraries bundled**), and recorded archive SHA-256 `6bb24d51dedf9f69344aa187cb0f9651b7842de4c465643480f3a794481dba64` (`5,793,473` bytes). Job 2 (`test-clean-macos`, ID `130279239848`) printed the identical SHA-256 `6bb24d51dedf9f69344aa187cb0f9651b7842de4c465643480f3a794481dba64`, hid `/opt/homebrew`, verified `(heifload),` without `svgload`/`magickload` under stripped `env -i PATH`, decoded both HEIC fixtures (`image_orientation_6.heic` -> `240x320` & `480x640`; `image_display_p3.heic` -> `640x480` & `320x240`), confirmed SVG rejection both unblocked and under `VIPS_BLOCK_UNTRUSTED=1`, and uploaded the 14-day artifact `vips-macos-arm64-hevc` (`ID 13822214591`).
Round 1 fixes also eliminated the entire `producer | grep -q` fail-open/`Broken pipe` bug class across `.github/workflows/deps-macos-vips.yml`, `deps/macos-vips/audit-dylibs.sh`, `deps/macos-vips/build.sh`, and `deps/win64-vips/audit-zip.sh` (the sole allowed Windows exception), backed by committed regression suite `deps/macos-vips/test-audit.sh`. The final `[skip ci]` commit removes the temporary `push` trigger from `.github/workflows/deps-macos-vips.yml` (`workflow_dispatch:` only), populates `deps/macos-vips/UPSTREAM.txt` with exact runner/Xcode/Homebrew versions from Run #3's logs, and stops cleanly at **Checkpoint 1** (`Run ID: 42945772807`, `SHA-256: 6bb24d51dedf9f69344aa187cb0f9651b7842de4c465643480f3a794481dba64`); Part B has not been started.

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
| `audit-dylibs.sh` tested on good and bad inputs (output pasted) and run in workflow: `libde265` present, no `x265`/`fftw`/`imagequant`/`magick`/`poppler`/`rsvg` anywhere; no non-system absolute paths | **met** | `./deps/macos-vips/test-audit.sh` (12 runs across `(i)` good, `(ii)` 3 bad inputs, `(iii)` large-output fake `strings`/`nm` with normal + ignored `SIGPIPE`: **0 assertion failures**; old `f9cb8aa` failed 8/8 on `(iii)`); patched Run #2 `libheif.1.dylib` `__cstring` caught `FAIL (c)`; Run #3 Job `130278088973` printed `PASS (a)`–`PASS (e)` and `OVERALL: AUDIT PASSED` (**6 LGPL**, **10 permissive**, **GPL status: NONE**). |
| Test job decodes both HEIC fixtures with Homebrew hidden and only the zip; raw output pasted | **met** | Run #3 Job `130279239848` (`test-clean-macos`) verified SHA-256 `6bb24d51dedf9f69344aa187cb0f9651b7842de4c465643480f3a794481dba64`, printed `PASS: Zero non-system absolute paths`, moved `/opt/homebrew` -> `/opt/homebrew.off`, confirmed `(heifload),` without `svgload`/`magickload`, decoded `image_orientation_6.heic` (`240x320` thumb, `480x640` copy) & `image_display_p3.heic` (`640x480` copy, `320x240` thumb), and confirmed unblocked + `VIPS_BLOCK_UNTRUSTED=1` SVG rejection (`exit=1`, `"not a known file format"`). |
| Zip is an artifact; after Checkpoint 1 a private release with SHA-256 recorded in docs and checked by CI | **partly** (`not tested` for release & Part B CI check — stopped at Checkpoint 1) | Run #3 Job `130279239848` uploaded 14-day artifact `vips-macos-arm64-hevc` (`ID 13822214591`, URL `https://github.com/narayanwaraich/photos-manager/actions/runs/42945772807/artifacts/13822214591`) for `vips-macos-arm64-8.18.6-hevc.zip` (`5,793,473` bytes, `SHA-256: 6bb24d51dedf9f69344aa187cb0f9651b7842de4c465643480f3a794481dba64`). Checkpoint 1 private release `macos-arm64-vips-8.18.6-hevc1` and Part B CI SHA-256 check await owner handoff. |
| App CI green on macOS **and** Windows with `heic_decode_verified: true` on both; negative test fails closed | **not tested** | Part B not started yet (stopping at Checkpoint 1 per task order). (`ci.yml` runs `42804777791`, `42806945665`, `42945772425` are green across all 3 jobs, proving zero regression only because macOS CI still uses Homebrew `vips` until Part B.) |
| §4.3 rewritten, table check passes, licences cited or marked unknown, GPL status stated | **partly** (archive metadata complete; `docs/native-dependencies.md` awaits Part B) | Archive `versions.json` & `LICENSES.md` record all 16 components (22 regular dylibs) with `GPL status: NONE`; `docs/native-dependencies.md` §4.3 rewrite & table check run in Part B after Checkpoint 1. |
| `docs/macos-validation.md` written; report lists single-OS validation and owner's own-HEIC steps | **not met** (awaiting Part B) | `docs/macos-validation.md` is Part B.5 after Checkpoint 1; single-OS testing & owner's own-iPhone HEIC requirement documented in header & §8 below. |

---

## 3. Files changed

Output of `git diff --stat 31e54eb` (`31e54eb` is the merge-base where `task/0F` branched from `main`, prior to `72f9904` review commit on `main`):
```text
 .github/workflows/deps-macos-vips.yml |  223 +++++
 deps/macos-vips/README.md             |   47 ++
 deps/macos-vips/UPSTREAM.txt          |   50 ++
 deps/macos-vips/audit-dylibs.sh       |  282 +++++++
 deps/macos-vips/build.sh              |  770 ++++++++++++++++++
 deps/macos-vips/test-audit.sh         |  232 ++++++
 deps/win64-vips/audit-zip.sh          |   26 +-
 reports/REPORT-0F.md                  | 1430 +++++++++++++++++++++++++++++++++
 8 files changed, 3051 insertions(+), 9 deletions(-)
```
*(Allowed paths respected: `.github/`, `deps/macos-vips/`, `reports/`, plus the single Round 1 exception `deps/win64-vips/audit-zip.sh`. No files in `src-tauri/`, `scripts/`, `docs/`, `AGENTS.md`, or `.github/workflows/ci.yml` were modified.)*

### Purpose of each changed file
- `.github/workflows/deps-macos-vips.yml`: Two-job macOS arm64 (`macos-15`) workflow (`build-audit` -> `test-clean-macos` with `/opt/homebrew` moved to `/opt/homebrew.off` and stripped `PATH`), `workflow_dispatch:` only (temporary `push` trigger removed in final `[skip ci]` commit), zero `| grep -q` pipelines, uploading the verified 14-day artifact `vips-macos-arm64-hevc`.
- `deps/macos-vips/README.md`: Build/layout documentation explaining why stock Homebrew `vips` cannot ship, how `PKG_CONFIG_LIBDIR` isolates leaf deps, and `@rpath`/`@loader_path` relocation layout.
- `deps/macos-vips/UPSTREAM.txt`: Pinned upstream tarball URLs, SHA-256 digests, licenses, excluded libraries, and exact runner OS (`macos-15-arm64` `20261104.0514.1`, `15.8.2` Build `24H232`), `Xcode 16.4` (`16F6`), and Homebrew formula versions recorded verbatim from green Run #3 (`42945772807`).
- `deps/macos-vips/audit-dylibs.sh`: Standalone audit enforcing rules `(a)`–`(e)` on filenames, `@rpath/` relocation and closure completeness, zero `x265` symbols/linkages (reading `strings -a` and `nm -g` via `mktemp` files rather than pipes to avoid `Broken pipe` / fail-open), `libde265` dynamic linkage in `libheif`, and `versions.json` completeness with zero GPL/unknown licenses.
- `deps/macos-vips/build.sh`: Reproducible single-script macOS arm64 builder for `libde265`, `libaom` (decode-only), `libheif`, and `libvips` (`-Dmodules=disabled`), harvesting the `@rpath` dylib closure (22 dylibs, 4 CLI binaries), ad-hoc `codesign`, `versions.json` / `LICENSES.md` generation, `.pc` prefix relocation, and zip packaging (pipe-free `otool` verification).
- `deps/macos-vips/test-audit.sh`: Executable regression suite verifying `audit-dylibs.sh` against `(i)` known-good synthetic Mach-O zip, `(ii)` three known-bad zips (`bad-x265`, `bad-fftw`, `bad-homebrew-path`), and `(iii)` large-output fake `strings` (`x265_encoder_open` + >5 MB filler) and fake `nm` (`_x265_foo` + 200k lines), each tested under both default `SIGPIPE` and `trap "" PIPE`.
- `deps/win64-vips/audit-zip.sh`: Removed all 5 `| grep -q` / head pipes (Round 1 Item `2c` exception) using `mktemp` (`STRINGS_TMP`) and variable captures so large `strings` output never triggers `Broken pipe` fail-open.
- `reports/REPORT-0F.md`: Final Part A / Checkpoint 1 implementation report with verbatim CI logs, regression evidence, pre-flight results, and `pnpm verify` output.

---

## 4. Dependencies

No crates or npm packages were added (`src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, `package.json`, `pnpm-lock.yaml` unchanged). The table below lists the **16 runtime components** inside `vips-macos-arm64-8.18.6-hevc.zip` (`5,793,473` bytes, `SHA-256: 6bb24d51dedf9f69344aa187cb0f9651b7842de4c465643480f3a794481dba64` — 4 built from verified source tarballs + 12 allowlisted Homebrew leaf libraries from Run #3 `42945772807` `versions.json`) plus runner build tools/SDK shims:

| Name | Exact version | Source verified at | License | Added by this task? |
|---|---|---|---|---|
| `vips` | `8.18.6` | `https://github.com/libvips/libvips/releases/download/v8.18.6/vips-8.18.6.tar.xz` (`sha256:3c41e1d5458081bfa4a5bc54e116c46259c75c6760a18027764555632b9dda3e`) | `LGPL-2.1-or-later` | Yes (recipe tarball; dynamically linked) |
| `libheif` | `1.23.2` | `https://github.com/strukturag/libheif/releases/download/v1.23.2/libheif-1.23.2.tar.gz` (`sha256:8bd5d41d19dc84536d118b04774709f244df6104ef66d623dad5fa4650143405`) | `LGPL-3.0-or-later` | Yes (recipe tarball; dynamically linked) |
| `libde265` | `1.1.1` | `https://github.com/strukturag/libde265/releases/download/v1.1.1/libde265-1.1.1.tar.gz` (`sha256:fd48a927e94ed74fc7ce8829d222b9d8599fcbfe8b6448ba66705babc56ab219`) | `LGPL-3.0-or-later` | Yes (recipe tarball; dynamically linked) |
| `aom` (`libaom`) | `3.14.1` | `https://storage.googleapis.com/aom-releases/libaom-3.14.1.tar.gz` (`sha256:44bf90dbd23e734d50e70a8c41c285193922938bd0d3bc2ee56764d181d55ef5`) | `BSD-2-Clause` | Yes (recipe tarball; decode-only) |
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

### 5.1 GitHub Actions Run Log (`gh run list --json` & Run-Log Table)

*(Note per Review Item 5a: GitHub's `gh run list --json` API truncates `displayTitle` at ~72 characters ending with `…`. Unshortened commit titles from `git log 31e54eb..HEAD` follow immediately below the JSON outputs.)*

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
```

Full unshortened commit titles on `task/0F`:
```text
$ git log main..HEAD --format="%h %ad %s" --date=iso-strict
e7e976a 2026-11-11T07:50:58+05:30 fix(vips): eliminate pipe-to-grep-q fail-open/SIGPIPE bugs and add audit regression suite
d3a4535 2026-11-10T17:21:15+05:30 docs(report): complete REPORT-0F at 2-run stop gate with logs and options table [skip ci]
631a85b 2026-11-10T16:52:25+05:30 fix(macos-vips): export OUT_ZIP and normalize .pc prefix + test-clean otool check
f9cb8aa 2026-11-10T16:36:49+05:30 build(macos-vips): add HEVC-decode-only libvips build recipe, audit, and workflow
5fd6cab 2026-11-10T16:25:37+05:30 docs(report): add TASK-0F pre-build upstream analysis (Section 1)
```

| Workflow | Run | Run ID | Commit | Status / Conclusion | Duration / Outcome Summary |
|---|---|---|---|---|---|
| `deps-macos-vips.yml` | #1 | `42804778977` | `f9cb8aa` | `completed` / `failure` | `build-audit` (`129791019071`) failed in `Build relocatable libvips macOS arm64 archive` with `KeyError: 'OUT_ZIP'` at `deps/macos-vips/build.sh`; fixed in `631a85b`. |
| `deps-macos-vips.yml` | #2 | `42806945658` | `631a85b` | `completed` / `failure` | `build-audit` (`129798381168`) **passed** (SHA-256 `04491934384424e05335029b7b2fc6853f5be2ede59c885da97dcce39b29e5f6`); `test-clean-macos` (`129799793819`) passed path & `/opt/homebrew` hide checks, printed `heifload`, then exited `1` at workflow line `139` (`echo: write error: Broken pipe`, `echo` returned `1` with `SIGPIPE` ignored on runner); stopped at 2-run gate and reported. |
| `deps-macos-vips.yml` | #3 | `42945772807` | `e7e976a` | `completed` / **`success`** | **`build-audit` (`130278088973`, `3m5s`) + `test-clean-macos` (`130279239848`, `20s`) BOTH GREEN.** Identical SHA-256 `6bb24d51dedf9f69344aa187cb0f9651b7842de4c465643480f3a794481dba64`; 14-day artifact `vips-macos-arm64-hevc` (`ID 13822214591`) uploaded. |
| `ci.yml` | #1 | `42804777791` | `f9cb8aa` | `completed` / `success` | All 3 jobs green (`macos-arm64`, `windows-x64`, `Security & License check`); proves zero regression only (macOS CI still uses Homebrew `vips` until Part B). |
| `ci.yml` | #2 | `42806945665` | `631a85b` | `completed` / `success` | All 3 jobs green (`macos-arm64` `129798381363`, `windows-x64` `129798381741`, `cargo deny` `129798381810`); proves zero regression only. |
| `ci.yml` | #3 | `42945772425` | `e7e976a` | `completed` / `success` | All 3 jobs green (`windows-x64` `130278088546`, `cargo deny` `130278088770`, `macos-arm64` `130278088872`); proves zero regression only. |

```text
$ gh run view 42945772807
✓ task/0F Build & Test macOS arm64 libvips (HEVC Decode) · 42945772807
Triggered via push about 4 minutes ago

JOBS
✓ Build & audit macOS arm64 libvips (HEVC decode) in 3m5s (ID 130278088973)
✓ Clean-runner test with Homebrew hidden (macos-15) in 20s (ID 130279239848)
[ANNOTATIONS/section separator lines trimmed]
ARTIFACTS
vips-macos-arm64-hevc
vips-macos-arm64-hevc-staging

For more information about a job, try: gh run view --job=<job-id>
View this run on GitHub: https://github.com/narayanwaraich/photos-manager/actions/runs/42945772807
```

```text
$ gh run view 42945772425
✓ task/0F CI & Packaging Matrix · 42945772425
Triggered via push about 1 hour ago

JOBS
✓ windows-x64 in 6m5s (ID 130278088546)
✓ Security & License check (cargo deny) in 48s (ID 130278088770)
✓ macos-arm64 in 6m36s (ID 130278088872)
```

---

### 5.2 Green Run #3 Evidence (`42945772807` on `e7e976a`)

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

| Component | Version | Bundled `.dylib`(s) | License | Source | Notes |
|---|---|---|---|---|---|
| `aom` | `3.14.1` | `libaom.3.dylib` | `BSD-2-Clause` | `tarball:https://storage.googleapis.com/aom-releases/libaom-3.14.1.tar.gz` |  |
| `gettext` | `1.0` | `libintl.8.dylib` | `LGPL-2.1-or-later` | `homebrew:gettext` | Runtime libintl shared library is LGPL-2.1-or-later; GPL-3.0-or-later applies only to gettext CLI build tools (not bundled). |
| `glib` | `2.90.1` | `libgio-2.0.0.dylib`, `libglib-2.0.0.dylib`, `libgmodule-2.0.0.dylib`, `libgobject-2.0.0.dylib` | `LGPL-2.1-or-later` | `homebrew:glib` |  |
| `highway` | `1.4.0` | `libhwy.1.dylib` | `Apache-2.0 OR BSD-3-Clause` | `homebrew:highway` |  |
| `jpeg-turbo` | `3.2.1` | `libjpeg.8.dylib` | `IJG AND Zlib AND BSD-3-Clause` | `homebrew:jpeg-turbo` |  |
| `libde265` | `1.1.1` | `libde265.0.dylib` | `LGPL-3.0-or-later` | `tarball:https://github.com/strukturag/libde265/releases/download/v1.1.1/libde265-1.1.1.tar.gz` |  |
| `libexif` | `0.6.26` | `libexif.12.dylib` | `LGPL-2.1-or-later AND LGPL-2.0-or-later` | `homebrew:libexif` |  |
| `libheif` | `1.23.2` | `libheif.1.dylib` | `LGPL-3.0-or-later` | `tarball:https://github.com/strukturag/libheif/releases/download/v1.23.2/libheif-1.23.2.tar.gz` |  |
| `libpng` | `1.6.59` | `libpng16.16.dylib` | `libpng-2.0` | `homebrew:libpng` |  |
| `libtiff` | `4.7.2` | `libtiff.6.dylib` | `libtiff` | `homebrew:libtiff` |  |
| `little-cms2` | `2.19.1` | `liblcms2.2.dylib` | `MIT` | `homebrew:little-cms2` |  |
| `pcre2` | `10.49` | `libpcre2-8.0.dylib` | `BSD-3-Clause` | `homebrew:pcre2` |  |
| `vips` | `8.18.6` | `libvips.42.dylib` | `LGPL-2.1-or-later` | `tarball:https://github.com/libvips/libvips/releases/download/v8.18.6/vips-8.18.6.tar.xz` |  |
| `webp` | `1.6.0` | `libsharpyuv.0.dylib`, `libwebp.7.dylib`, `libwebpdemux.2.dylib`, `libwebpmux.3.dylib` | `BSD-3-Clause` | `homebrew:webp` |  |
| `xz` | `5.8.4` | `liblzma.5.dylib` | `0BSD` | `homebrew:xz` | Runtime liblzma shared library is 0BSD public domain; GPL-2.0-or-later applies only to xz CLI utilities/scripts (not bundled). |
| `zstd` | `1.5.7_1` | `libzstd.1.dylib` | `BSD-3-Clause` | `homebrew:zstd` | Dual-licensed (BSD-3-Clause OR GPL-2.0-only); licensee elects BSD-3-Clause alongside BSD-2-Clause AND MIT. |
=== Archive SHA-256 and listing ===
-rw-r--r--  1 runner  staff  5793473 Nov 11 02:27 /Users/runner/work/photos-manager/photos-manager/vips-macos-arm64-8.18.6-hevc.zip
6bb24d51dedf9f69344aa187cb0f9651b7842de4c465643480f3a794481dba64  /Users/runner/work/photos-manager/photos-manager/vips-macos-arm64-8.18.6-hevc.zip
[upload-artifact action setup lines trimmed]
Artifact vips-macos-arm64-hevc-staging.zip successfully finalized. Artifact ID 13822208328
```

#### Job 2 (`130279239848`): Matching SHA-256, `lib/` Inventory, Clean-Runner All Four Test Sections, 14-Day Artifact
```text
-rw-r--r--  1 runner  wheel  5793473 Nov 11 02:27 /tmp/vips-staging/vips-macos-arm64-8.18.6-hevc.zip
6bb24d51dedf9f69344aa187cb0f9651b7842de4c465643480f3a794481dba64  /tmp/vips-staging/vips-macos-arm64-8.18.6-hevc.zip
Extracted prefix: /tmp/vips-clean-test/vips-macos-arm64-8.18.6-hevc
total 29288
drwxr-xr-x  33 runner  wheel     1056 Nov 11 02:27 .
drwxr-xr-x   8 runner  wheel      256 Nov 11 02:27 ..
-rwxr-xr-x   1 runner  wheel  2073408 Nov 11 02:27 libaom.3.dylib
lrwxr-xr-x   1 runner  wheel       14 Nov 11 02:27 libaom.dylib -> libaom.3.dylib
-rwxr-xr-x   1 runner  wheel   464432 Nov 11 02:27 libde265.0.dylib
lrwxr-xr-x   1 runner  wheel       16 Nov 11 02:27 libde265.dylib -> libde265.0.dylib
-rwxr-xr-x   1 runner  wheel   263280 Nov 11 02:27 libexif.12.dylib
-rwxr-xr-x   1 runner  wheel  2058208 Nov 11 02:27 libgio-2.0.0.dylib
lrwxr-xr-x   1 runner  wheel       18 Nov 11 02:27 libgio-2.0.dylib -> libgio-2.0.0.dylib
-rwxr-xr-x   1 runner  wheel  1384880 Nov 11 02:27 libglib-2.0.0.dylib
lrwxr-xr-x   1 runner  wheel       19 Nov 11 02:27 libglib-2.0.dylib -> libglib-2.0.0.dylib
-rwxr-xr-x   1 runner  wheel    71600 Nov 11 02:27 libgmodule-2.0.0.dylib
lrwxr-xr-x   1 runner  wheel       22 Nov 11 02:27 libgmodule-2.0.dylib -> libgmodule-2.0.0.dylib
-rwxr-xr-x   1 runner  wheel   429296 Nov 11 02:27 libgobject-2.0.0.dylib
lrwxr-xr-x   1 runner  wheel       22 Nov 11 02:27 libgobject-2.0.dylib -> libgobject-2.0.0.dylib
-rwxr-xr-x   1 runner  wheel  1923072 Nov 11 02:27 libheif.1.dylib
lrwxr-xr-x   1 runner  wheel       15 Nov 11 02:27 libheif.dylib -> libheif.1.dylib
-rwxr-xr-x   1 runner  wheel    98320 Nov 11 02:27 libhwy.1.dylib
-rwxr-xr-x   1 runner  wheel   230048 Nov 11 02:27 libintl.8.dylib
-rwxr-xr-x   1 runner  wheel   474896 Nov 11 02:27 libjpeg.8.dylib
-rwxr-xr-x   1 runner  wheel   374752 Nov 11 02:27 liblcms2.2.dylib
-rwxr-xr-x   1 runner  wheel   185520 Nov 11 02:27 liblzma.5.dylib
-rwxr-xr-x   1 runner  wheel   592032 Nov 11 02:27 libpcre2-8.0.dylib
-rwxr-xr-x   1 runner  wheel   209392 Nov 11 02:27 libpng16.16.dylib
-rwxr-xr-x   1 runner  wheel    69856 Nov 11 02:27 libsharpyuv.0.dylib
-rwxr-xr-x   1 runner  wheel   545920 Nov 11 02:27 libtiff.6.dylib
-rwxr-xr-x   1 runner  wheel  2358576 Nov 11 02:27 libvips.42.dylib
lrwxr-xr-x   1 runner  wheel       16 Nov 11 02:27 libvips.dylib -> libvips.42.dylib
-rwxr-xr-x   1 runner  wheel   354208 Nov 11 02:27 libwebp.7.dylib
-rwxr-xr-x   1 runner  wheel    53360 Nov 11 02:27 libwebpdemux.2.dylib
-rwxr-xr-x   1 runner  wheel    73488 Nov 11 02:27 libwebpmux.3.dylib
-rwxr-xr-x   1 runner  wheel   653328 Nov 11 02:27 libzstd.1.dylib
drwxr-xr-x   6 runner  wheel      192 Nov 11 02:27 pkgconfig
=== Verifying otool -L across unpacked lib/* and bin/* ===
PASS: Zero non-system absolute paths in unpacked binaries/dylibs
=== 1. Check vips -l lists heifload with stripped PATH and hidden Homebrew ===
        VipsForeignLoadNsgif (gifload_base), load GIF with libnsgif, priority=50
          VipsForeignLoadNsgifFile (gifload), load GIF with libnsgif (.gif), priority=50, is_a, get_flags, get_flags_filename, header, load
          VipsForeignLoadNsgifBuffer (gifload_buffer), load GIF with libnsgif, priority=50, is_a_buffer, get_flags, get_flags_filename, header, load
          VipsForeignLoadNsgifSource (gifload_source), load gif from source, nocache, priority=50, is_a_source, get_flags, get_flags_filename, header, load
        VipsForeignLoadPng (pngload_base), load png base class, priority=200
          VipsForeignLoadPngFile (pngload), load png from file (.png), priority=200, is_a, get_flags, get_flags_filename, header, load
          VipsForeignLoadPngBuffer (pngload_buffer), load png from buffer, priority=200, is_a_buffer, get_flags, get_flags_filename, header, load
          VipsForeignLoadPngSource (pngload_source), load png from source, nocache, priority=200, is_a_source, get_flags, get_flags_filename, header, load
        VipsForeignLoadJpeg (jpegload_base), load jpeg, priority=50
          VipsForeignLoadJpegFile (jpegload), load jpeg from file (.jpg, .jpeg, .jpe, .jfif), priority=50, is_a, get_flags, get_flags_filename, header, load
          VipsForeignLoadJpegBuffer (jpegload_buffer), load jpeg from buffer, priority=50, is_a_buffer, get_flags, get_flags_filename, header, load
          VipsForeignLoadJpegSource (jpegload_source), load image from jpeg source, nocache, priority=50, is_a_source, get_flags, get_flags_filename, header, load
        VipsForeignLoadWebp (webpload_base), load webp, priority=200
          VipsForeignLoadWebpFile (webpload), load webp from file (.webp), priority=200, is_a, get_flags, get_flags_filename, header, load
          VipsForeignLoadWebpBuffer (webpload_buffer), load webp from buffer, priority=200, is_a_buffer, get_flags, get_flags_filename, header, load
          VipsForeignLoadWebpSource (webpload_source), load webp from source, nocache, priority=200, is_a_source, get_flags, get_flags_filename, header, load
        VipsForeignLoadTiff (tiffload_base), load tiff, priority=50
          VipsForeignLoadTiffFile (tiffload), load tiff from file (.tif, .tiff), priority=50, is_a, get_flags, get_flags_filename, header, load
          VipsForeignLoadTiffBuffer (tiffload_buffer), load tiff from buffer, priority=50, is_a_buffer, get_flags, get_flags_filename, header, load
          VipsForeignLoadTiffSource (tiffload_source), load tiff from source, nocache, priority=50, is_a_source, get_flags, get_flags_filename, header, load
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
Unblocked SVG exit=1 output: VipsForeignLoad: "tests/fixtures/generated/fixture_blocked.svg" is not a known file format
Blocked SVG exit=1 output: VipsForeignLoad: "tests/fixtures/generated/fixture_blocked.svg" is not a known file format
✓ Success: SVG load rejected cleanly both unblocked and under VIPS_BLOCK_UNTRUSTED=1 (no svgload built)
=== Final verified artifact details ===
-rw-r--r--  1 runner  wheel  5793473 Nov 11 02:27 /tmp/vips-staging/vips-macos-arm64-8.18.6-hevc.zip
6bb24d51dedf9f69344aa187cb0f9651b7842de4c465643480f3a794481dba64  /tmp/vips-staging/vips-macos-arm64-8.18.6-hevc.zip
[upload-artifact action setup & zipping lines trimmed]
Uploaded bytes 5756758
Finished uploading artifact content to blob storage!
SHA256 digest of uploaded artifact zip is 7f2beb21c043bc2e47d1ec8f12d089f6508f21f459bb432532679266855d08cd
Finalizing artifact upload
Artifact vips-macos-arm64-hevc.zip successfully finalized. Artifact ID 13822214591
Artifact vips-macos-arm64-hevc has been successfully uploaded! Final size is 5756758 bytes. Artifact ID is 13822214591
Artifact download URL: https://github.com/narayanwaraich/photos-manager/actions/runs/42945772807/artifacts/13822214591
```

---

### 5.3 Static Checks & `audit-dylibs.sh` Regression Suite (Items 1, 2a, 2b)

#### Static Linting & Remaining `| grep` Audit
```text
$ shellcheck -x deps/macos-vips/*.sh deps/win64-vips/audit-zip.sh && echo "shellcheck: ok" && actionlint -verbose .github/workflows/deps-macos-vips.yml
shellcheck: ok
verbose: Linting .github/workflows/deps-macos-vips.yml
verbose: Using project at /Users/narayanwaraich/Desktop/Projects/photos-manager
verbose: Found 0 parse errors in 0 ms for .github/workflows/deps-macos-vips.yml
verbose: Rule "pyflakes" was disabled: exec: "pyflakes": executable file not found in $PATH
verbose: Found total 0 errors in 60 ms for .github/workflows/deps-macos-vips.yml

$ grep -nE '\|[[:space:]]*grep' .github/workflows/deps-macos-vips.yml deps/macos-vips/*.sh deps/win64-vips/audit-zip.sh
.github/workflows/deps-macos-vips.yml:105:            LEAKS=$(otool -L "$f" | tail -n +2 | grep -E "/opt/homebrew|/usr/local|/Cellar|/var/|/tmp/" || true)
.github/workflows/deps-macos-vips.yml:110:            RP_LEAKS=$(otool -l "$f" | grep -A2 "cmd LC_RPATH" | grep -E "/opt/homebrew|/usr/local|/Cellar|/var/|/tmp/" || true)
```
Explanation of remaining hits: **Zero `producer | grep -q` remain** anywhere in `.github/workflows/deps-macos-vips.yml`, `deps/macos-vips/*.sh`, or `deps/win64-vips/audit-zip.sh`. Both remaining hits (`.github/workflows/deps-macos-vips.yml:105` and `:110`) omit `-q`, use `|| true`, and read every line emitted by `otool`, so the upstream writer is never closed early.

#### Regression Suite Output: New `deps/macos-vips/audit-dylibs.sh`
```text
$ ./deps/macos-vips/test-audit.sh
=== Running audit-dylibs.sh regression suite against: /Users/narayanwaraich/Desktop/Projects/photos-manager/deps/macos-vips/audit-dylibs.sh ===
--- [normal] (i) KNOWN GOOD (good.zip) ---
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
EXIT_CODE=0 (expected 0)
--- [sigpipe-ignored] (i) KNOWN GOOD (good.zip) ---
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
EXIT_CODE=0 (expected 0)
--- [normal] (ii-1) KNOWN BAD (bad-x265.zip) ---
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
EXIT_CODE=1 (expected 1)
--- [sigpipe-ignored] (ii-1) KNOWN BAD (bad-x265.zip) ---
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
EXIT_CODE=1 (expected 1)
--- [normal] (ii-2) KNOWN BAD (bad-fftw.zip) ---
=== Auditing macOS libvips archive: bad-fftw.zip ===
FAIL (a): Found forbidden filename matching (x265|x264|fftw|imagequant|magick|poppler|rsvg|openslide):
/var/folders/vw/sxxzw9pd58qf70hwkhrlgsdr0000gn/T/tmp.86mNHrSN3c/vips-macos-arm64-8.18.6-hevc/lib/libfftw3.3.dylib
PASS (b): All 5 Mach-O files use @rpath/ cleanly with zero Homebrew/local path leaks or forbidden linkages
PASS (c): Zero x265 symbols or linkage strings across all Mach-O files (enum nick verified absent from all non-libvips files)
PASS (d): libde265.0.dylib present and dynamically linked by libheif.1.dylib
  Disallowed component in versions.json: fftw
  Component 'fftw' has disallowed GPL license: 'GPL-2.0-or-later'
FAIL (e): versions.json audit failed
OVERALL: AUDIT FAILED
EXIT_CODE=1 (expected 1)
--- [sigpipe-ignored] (ii-2) KNOWN BAD (bad-fftw.zip) ---
=== Auditing macOS libvips archive: bad-fftw.zip ===
FAIL (a): Found forbidden filename matching (x265|x264|fftw|imagequant|magick|poppler|rsvg|openslide):
/var/folders/vw/sxxzw9pd58qf70hwkhrlgsdr0000gn/T/tmp.G8d4TcXB9S/vips-macos-arm64-8.18.6-hevc/lib/libfftw3.3.dylib
PASS (b): All 5 Mach-O files use @rpath/ cleanly with zero Homebrew/local path leaks or forbidden linkages
PASS (c): Zero x265 symbols or linkage strings across all Mach-O files (enum nick verified absent from all non-libvips files)
PASS (d): libde265.0.dylib present and dynamically linked by libheif.1.dylib
  Disallowed component in versions.json: fftw
  Component 'fftw' has disallowed GPL license: 'GPL-2.0-or-later'
FAIL (e): versions.json audit failed
OVERALL: AUDIT FAILED
EXIT_CODE=1 (expected 1)
--- [normal] (ii-3) KNOWN BAD (bad-homebrew-path.zip) ---
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
EXIT_CODE=1 (expected 1)
--- [sigpipe-ignored] (ii-3) KNOWN BAD (bad-homebrew-path.zip) ---
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
EXIT_CODE=1 (expected 1)
--- [normal] (iii-a) FAKE strings (>5MB after x265_encoder_open on line 1) ---
=== Auditing macOS libvips archive: good.zip ===
PASS (a): No forbidden filenames (x265|x264|fftw|imagequant|magick|poppler|rsvg|openslide) in archive
PASS (b): All 4 Mach-O files use @rpath/ cleanly with zero Homebrew/local path leaks or forbidden linkages
  libheif.1.dylib contains forbidden x265 symbol/linkage string (libx265/x265_encoder/x265_param)
  Non-exempt binary libheif.1.dylib contains string 'x265'
  libde265.0.dylib contains forbidden x265 symbol/linkage string (libx265/x265_encoder/x265_param)
  Non-exempt binary libde265.0.dylib contains string 'x265'
  libvips.42.dylib contains forbidden x265 symbol/linkage string (libx265/x265_encoder/x265_param)
  vips contains forbidden x265 symbol/linkage string (libx265/x265_encoder/x265_param)
  Non-exempt binary vips contains string 'x265'
FAIL (c): Forbidden x265 strings or symbols detected
PASS (d): libde265.0.dylib present and dynamically linked by libheif.1.dylib
  Verified 3 components covering 3 regular dylibs.
  LGPL components: libde265 (LGPL-3.0-or-later), libheif (LGPL-3.0-or-later), vips (LGPL-2.1-or-later)
  Permissive components: 
  GPL status: NONE (0 GPL libraries bundled)
PASS (e): versions.json valid, libde265 present, all dylibs accounted for, zero unknown/GPL licenses
OVERALL: AUDIT FAILED
EXIT_CODE=1 (expected 1)
--- [sigpipe-ignored] (iii-a) FAKE strings (>5MB after x265_encoder_open on line 1) ---
=== Auditing macOS libvips archive: good.zip ===
PASS (a): No forbidden filenames (x265|x264|fftw|imagequant|magick|poppler|rsvg|openslide) in archive
PASS (b): All 4 Mach-O files use @rpath/ cleanly with zero Homebrew/local path leaks or forbidden linkages
  libheif.1.dylib contains forbidden x265 symbol/linkage string (libx265/x265_encoder/x265_param)
  Non-exempt binary libheif.1.dylib contains string 'x265'
  libde265.0.dylib contains forbidden x265 symbol/linkage string (libx265/x265_encoder/x265_param)
  Non-exempt binary libde265.0.dylib contains string 'x265'
  libvips.42.dylib contains forbidden x265 symbol/linkage string (libx265/x265_encoder/x265_param)
  vips contains forbidden x265 symbol/linkage string (libx265/x265_encoder/x265_param)
  Non-exempt binary vips contains string 'x265'
FAIL (c): Forbidden x265 strings or symbols detected
PASS (d): libde265.0.dylib present and dynamically linked by libheif.1.dylib
  Verified 3 components covering 3 regular dylibs.
  LGPL components: libde265 (LGPL-3.0-or-later), libheif (LGPL-3.0-or-later), vips (LGPL-2.1-or-later)
  Permissive components: 
  GPL status: NONE (0 GPL libraries bundled)
PASS (e): versions.json valid, libde265 present, all dylibs accounted for, zero unknown/GPL licenses
OVERALL: AUDIT FAILED
EXIT_CODE=1 (expected 1)
--- [normal] (iii-b) FAKE nm (200k lines after _x265_foo on line 1) ---
=== Auditing macOS libvips archive: good.zip ===
PASS (a): No forbidden filenames (x265|x264|fftw|imagequant|magick|poppler|rsvg|openslide) in archive
PASS (b): All 4 Mach-O files use @rpath/ cleanly with zero Homebrew/local path leaks or forbidden linkages
  libheif.1.dylib exports/imports x265 symbol in Mach-O symbol table
  libde265.0.dylib exports/imports x265 symbol in Mach-O symbol table
  libvips.42.dylib exports/imports x265 symbol in Mach-O symbol table
  vips exports/imports x265 symbol in Mach-O symbol table
FAIL (c): Forbidden x265 strings or symbols detected
PASS (d): libde265.0.dylib present and dynamically linked by libheif.1.dylib
  Verified 3 components covering 3 regular dylibs.
  LGPL components: libde265 (LGPL-3.0-or-later), libheif (LGPL-3.0-or-later), vips (LGPL-2.1-or-later)
  Permissive components: 
  GPL status: NONE (0 GPL libraries bundled)
PASS (e): versions.json valid, libde265 present, all dylibs accounted for, zero unknown/GPL licenses
OVERALL: AUDIT FAILED
EXIT_CODE=1 (expected 1)
--- [sigpipe-ignored] (iii-b) FAKE nm (200k lines after _x265_foo on line 1) ---
=== Auditing macOS libvips archive: good.zip ===
PASS (a): No forbidden filenames (x265|x264|fftw|imagequant|magick|poppler|rsvg|openslide) in archive
PASS (b): All 4 Mach-O files use @rpath/ cleanly with zero Homebrew/local path leaks or forbidden linkages
  libheif.1.dylib exports/imports x265 symbol in Mach-O symbol table
  libde265.0.dylib exports/imports x265 symbol in Mach-O symbol table
  libvips.42.dylib exports/imports x265 symbol in Mach-O symbol table
  vips exports/imports x265 symbol in Mach-O symbol table
FAIL (c): Forbidden x265 strings or symbols detected
PASS (d): libde265.0.dylib present and dynamically linked by libheif.1.dylib
  Verified 3 components covering 3 regular dylibs.
  LGPL components: libde265 (LGPL-3.0-or-later), libheif (LGPL-3.0-or-later), vips (LGPL-2.1-or-later)
  Permissive components: 
  GPL status: NONE (0 GPL libraries bundled)
PASS (e): versions.json valid, libde265 present, all dylibs accounted for, zero unknown/GPL licenses
OVERALL: AUDIT FAILED
EXIT_CODE=1 (expected 1)
=== Summary: 0 assertion failure(s) ===
NEW_RC=0
```

#### Proof That Old Script Silently Fails Open: `f9cb8aa:deps/macos-vips/audit-dylibs.sh`
```text
$ git show f9cb8aa:deps/macos-vips/audit-dylibs.sh > /tmp/old-audit.sh && chmod +x /tmp/old-audit.sh
$ ./deps/macos-vips/test-audit.sh /tmp/old-audit.sh
=== Running audit-dylibs.sh regression suite against: /tmp/old-audit.sh ===
--- [normal] (i) KNOWN GOOD (good.zip) ---
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
EXIT_CODE=0 (expected 0)
--- [sigpipe-ignored] (i) KNOWN GOOD (good.zip) ---
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
EXIT_CODE=0 (expected 0)
--- [normal] (ii-1) KNOWN BAD (bad-x265.zip) ---
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
EXIT_CODE=1 (expected 1)
--- [sigpipe-ignored] (ii-1) KNOWN BAD (bad-x265.zip) ---
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
EXIT_CODE=1 (expected 1)
--- [normal] (ii-2) KNOWN BAD (bad-fftw.zip) ---
=== Auditing macOS libvips archive: bad-fftw.zip ===
FAIL (a): Found forbidden filename matching (x265|x264|fftw|imagequant|magick|poppler|rsvg|openslide):
/var/folders/vw/sxxzw9pd58qf70hwkhrlgsdr0000gn/T/tmp.kMkCwEVkMh/vips-macos-arm64-8.18.6-hevc/lib/libfftw3.3.dylib
PASS (b): All 5 Mach-O files use @rpath/ cleanly with zero Homebrew/local path leaks or forbidden linkages
PASS (c): Zero x265 symbols or linkage strings across all Mach-O files (enum nick verified absent from all non-libvips files)
PASS (d): libde265.0.dylib present and dynamically linked by libheif.1.dylib
  Disallowed component in versions.json: fftw
  Component 'fftw' has disallowed GPL license: 'GPL-2.0-or-later'
FAIL (e): versions.json audit failed
OVERALL: AUDIT FAILED
EXIT_CODE=1 (expected 1)
--- [sigpipe-ignored] (ii-2) KNOWN BAD (bad-fftw.zip) ---
=== Auditing macOS libvips archive: bad-fftw.zip ===
FAIL (a): Found forbidden filename matching (x265|x264|fftw|imagequant|magick|poppler|rsvg|openslide):
/var/folders/vw/sxxzw9pd58qf70hwkhrlgsdr0000gn/T/tmp.lojnn6oDEr/vips-macos-arm64-8.18.6-hevc/lib/libfftw3.3.dylib
PASS (b): All 5 Mach-O files use @rpath/ cleanly with zero Homebrew/local path leaks or forbidden linkages
PASS (c): Zero x265 symbols or linkage strings across all Mach-O files (enum nick verified absent from all non-libvips files)
PASS (d): libde265.0.dylib present and dynamically linked by libheif.1.dylib
  Disallowed component in versions.json: fftw
  Component 'fftw' has disallowed GPL license: 'GPL-2.0-or-later'
FAIL (e): versions.json audit failed
OVERALL: AUDIT FAILED
EXIT_CODE=1 (expected 1)
--- [normal] (ii-3) KNOWN BAD (bad-homebrew-path.zip) ---
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
EXIT_CODE=1 (expected 1)
--- [sigpipe-ignored] (ii-3) KNOWN BAD (bad-homebrew-path.zip) ---
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
EXIT_CODE=1 (expected 1)
--- [normal] (iii-a) FAKE strings (>5MB after x265_encoder_open on line 1) ---
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
EXIT_CODE=0 (expected 1)
TEST ASSERTION FAILED [normal] (iii-a) FAKE strings (>5MB after x265_encoder_open on line 1): got exit 0, expected 1
TEST ASSERTION FAILED [normal] (iii-a) FAKE strings (>5MB after x265_encoder_open on line 1): missing expected output substring 'FAIL (c)'
--- [sigpipe-ignored] (iii-a) FAKE strings (>5MB after x265_encoder_open on line 1) ---
=== Auditing macOS libvips archive: good.zip ===
PASS (a): No forbidden filenames (x265|x264|fftw|imagequant|magick|poppler|rsvg|openslide) in archive
PASS (b): All 4 Mach-O files use @rpath/ cleanly with zero Homebrew/local path leaks or forbidden linkages
awk: write error on /dev/stdout
 source line number 4
awk: write error on /dev/stdout
 source line number 4
awk: write error on /dev/stdout
 source line number 4
awk: write error on /dev/stdout
 source line number 4
awk: write error on /dev/stdout
 source line number 4
awk: write error on /dev/stdout
 source line number 4
awk: write error on /dev/stdout
 source line number 4
PASS (c): Zero x265 symbols or linkage strings across all Mach-O files (enum nick verified absent from all non-libvips files)
PASS (d): libde265.0.dylib present and dynamically linked by libheif.1.dylib
  Verified 3 components covering 3 regular dylibs.
  LGPL components: libde265 (LGPL-3.0-or-later), libheif (LGPL-3.0-or-later), vips (LGPL-2.1-or-later)
  Permissive components: 
  GPL status: NONE (0 GPL libraries bundled)
PASS (e): versions.json valid, libde265 present, all dylibs accounted for, zero unknown/GPL licenses
OVERALL: AUDIT PASSED
EXIT_CODE=0 (expected 1)
TEST ASSERTION FAILED [sigpipe-ignored] (iii-a) FAKE strings (>5MB after x265_encoder_open on line 1): got exit 0, expected 1
TEST ASSERTION FAILED [sigpipe-ignored] (iii-a) FAKE strings (>5MB after x265_encoder_open on line 1): missing expected output substring 'FAIL (c)'
--- [normal] (iii-b) FAKE nm (200k lines after _x265_foo on line 1) ---
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
EXIT_CODE=0 (expected 1)
TEST ASSERTION FAILED [normal] (iii-b) FAKE nm (200k lines after _x265_foo on line 1): got exit 0, expected 1
TEST ASSERTION FAILED [normal] (iii-b) FAKE nm (200k lines after _x265_foo on line 1): missing expected output substring 'FAIL (c)'
--- [sigpipe-ignored] (iii-b) FAKE nm (200k lines after _x265_foo on line 1) ---
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
EXIT_CODE=0 (expected 1)
TEST ASSERTION FAILED [sigpipe-ignored] (iii-b) FAKE nm (200k lines after _x265_foo on line 1): got exit 0, expected 1
TEST ASSERTION FAILED [sigpipe-ignored] (iii-b) FAKE nm (200k lines after _x265_foo on line 1): missing expected output substring 'FAIL (c)'
=== Summary: 8 assertion failure(s) ===
OLD_RC=1
```

---

### 5.4 Windows `audit-zip.sh` Fix (Item 2c) & Run #2 Patched `libheif.1.dylib` `__cstring` Audit (Item 2d)

#### Item 2c: `deps/win64-vips/audit-zip.sh` Fake-`strings` (>5 MB) & Stock `8.18.6` Archive Checks
```text
=== NEW win64 audit-zip.sh on fake-strings (>5MB) [normal] ===
=== Auditing libvips archive: mock-win.zip ===
PASS (a): No file containing 'x265' in name anywhere in archive
  Non-exempt DLL libde265.dll contains string 'x265'
  Non-exempt DLL libheif.dll contains string 'x265'
FAIL (b): Found 'x265' string in non-exempt DLL
  DLL libde265.dll contains forbidden x265 symbol/linkage string
  DLL libvips-42.dll contains forbidden x265 symbol/linkage string
  DLL libheif.dll contains forbidden x265 symbol/linkage string
FAIL (c): Found forbidden x265 linkage string (libx265, x265_encoder, x265_param)
  libheif DLL (/var/folders/vw/sxxzw9pd58qf70hwkhrlgsdr0000gn/T/tmp.OzZu1LQigk/vips-dev-8.18/bin/libheif.dll) does not contain string 'libde265'
FAIL (d): libde265 DLL missing or libheif DLL does not reference libde265
PASS (e): versions.json names libde265
PASS (f): No fftw, imagemagick, or poppler components found in files or versions.json
OVERALL: AUDIT FAILED
EXIT=1
=== NEW win64 audit-zip.sh on fake-strings (>5MB) [sigpipe-ignored] ===
=== Auditing libvips archive: mock-win.zip ===
PASS (a): No file containing 'x265' in name anywhere in archive
  Non-exempt DLL libde265.dll contains string 'x265'
  Non-exempt DLL libheif.dll contains string 'x265'
FAIL (b): Found 'x265' string in non-exempt DLL
  DLL libde265.dll contains forbidden x265 symbol/linkage string
  DLL libvips-42.dll contains forbidden x265 symbol/linkage string
  DLL libheif.dll contains forbidden x265 symbol/linkage string
FAIL (c): Found forbidden x265 linkage string (libx265, x265_encoder, x265_param)
  libheif DLL (/var/folders/vw/sxxzw9pd58qf70hwkhrlgsdr0000gn/T/tmp.ONufgga6VF/vips-dev-8.18/bin/libheif.dll) does not contain string 'libde265'
FAIL (d): libde265 DLL missing or libheif DLL does not reference libde265
PASS (e): versions.json names libde265
PASS (f): No fftw, imagemagick, or poppler components found in files or versions.json
OVERALL: AUDIT FAILED
EXIT=1
=== OLD (631a85b) win64 audit-zip.sh on fake-strings (>5MB) [normal] ===
=== Auditing libvips archive: mock-win.zip ===
PASS (a): No file containing 'x265' in name anywhere in archive
PASS (b): No 'x265' string found in any non-exempt DLL
PASS (c): No forbidden x265 linkage strings (libx265, x265_encoder, x265_param) found in any DLL
  libheif DLL (/var/folders/vw/sxxzw9pd58qf70hwkhrlgsdr0000gn/T/tmp.Awb5Y2MgC7/vips-dev-8.18/bin/libheif.dll) does not contain string 'libde265'
FAIL (d): libde265 DLL missing or libheif DLL does not reference libde265
PASS (e): versions.json names libde265
PASS (f): No fftw, imagemagick, or poppler components found in files or versions.json
OVERALL: AUDIT FAILED
EXIT=1
=== OLD (631a85b) win64 audit-zip.sh on fake-strings (>5MB) [sigpipe-ignored] ===
=== Auditing libvips archive: mock-win.zip ===
PASS (a): No file containing 'x265' in name anywhere in archive
awk: write error on /dev/stdout
 source line number 1
awk: write error on /dev/stdout
 source line number 1
PASS (b): No 'x265' string found in any non-exempt DLL
awk: write error on /dev/stdout
 source line number 1
awk: write error on /dev/stdout
 source line number 1
awk: write error on /dev/stdout
 source line number 1
PASS (c): No forbidden x265 linkage strings (libx265, x265_encoder, x265_param) found in any DLL
  libheif DLL (/var/folders/vw/sxxzw9pd58qf70hwkhrlgsdr0000gn/T/tmp.r6l2C6XxN5/vips-dev-8.18/bin/libheif.dll) does not contain string 'libde265'
FAIL (d): libde265 DLL missing or libheif DLL does not reference libde265
PASS (e): versions.json names libde265
PASS (f): No fftw, imagemagick, or poppler components found in files or versions.json
OVERALL: AUDIT FAILED
EXIT=1
=== NEW win64 audit-zip.sh on stock vips-dev-x64-web-8.18.6.zip [normal] ===
10086f2ccc8e2a861831facabab75fa1b80b4bd03f0a8447623ec581002aa4b2  /tmp/vips-dev-x64-web-8.18.6.zip
=== Auditing libvips archive: vips-dev-x64-web-8.18.6.zip ===
PASS (a): No file containing 'x265' in name anywhere in archive
PASS (b): No 'x265' string found in any non-exempt DLL
PASS (c): No forbidden x265 linkage strings (libx265, x265_encoder, x265_param) found in any DLL
  libde265 DLL not found in bin/
  libheif DLL (/var/folders/vw/sxxzw9pd58qf70hwkhrlgsdr0000gn/T/tmp.GNDkBZV6AE/vips-dev-8.18/bin/libheif.dll) does not contain string 'libde265'
FAIL (d): libde265 DLL missing or libheif DLL does not reference libde265
FAIL (e): versions.json lacks 'libde265'
PASS (f): No fftw, imagemagick, or poppler components found in files or versions.json
OVERALL: AUDIT FAILED
EXIT=1
=== NEW win64 audit-zip.sh on stock vips-dev-x64-web-8.18.6.zip [sigpipe-ignored] ===
=== Auditing libvips archive: vips-dev-x64-web-8.18.6.zip ===
PASS (a): No file containing 'x265' in name anywhere in archive
PASS (b): No 'x265' string found in any non-exempt DLL
PASS (c): No forbidden x265 linkage strings (libx265, x265_encoder, x265_param) found in any DLL
  libde265 DLL not found in bin/
  libheif DLL (/var/folders/vw/sxxzw9pd58qf70hwkhrlgsdr0000gn/T/tmp.dcLyFf7FsK/vips-dev-8.18/bin/libheif.dll) does not contain string 'libde265'
FAIL (d): libde265 DLL missing or libheif DLL does not reference libde265
FAIL (e): versions.json lacks 'libde265'
PASS (f): No fftw, imagemagick, or poppler components found in files or versions.json
OVERALL: AUDIT FAILED
EXIT=1
```

#### Item 2d: Real-Size Binary Test on Run #2 (`42806945658`) `lib/libheif.1.dylib` Copy (`__cstring` Patched)
```text
$ otool -l /tmp/run2-unpacked/vips-macos-arm64-8.18.6-hevc/lib/libheif.1.dylib | grep -A 8 'sectname __cstring'
  sectname __cstring
   segname __TEXT
      addr 0x0000000000135c18
      size 0x0000000000007d40
    offset 1268760
     align 2^3 (8)
    reloff 0
    nreloc 0
     flags 0x00000002
# Copied tree to /tmp/run2-patched and overwrote bytes at offset 1268760 (b'vector\x00Read beyond...') with b'x265_encoder_open\x00'

$ ./deps/macos-vips/audit-dylibs.sh /tmp/run2-patched ; echo "PATCH_NEW_RC=$?"
$ bash -c 'trap "" PIPE; ./deps/macos-vips/audit-dylibs.sh /tmp/run2-patched' ; echo "PATCH_SIGPIPE_RC=$?"
=== Auditing macOS libvips archive: run2-patched ===
PASS (a): No forbidden filenames (x265|x264|fftw|imagequant|magick|poppler|rsvg|openslide) in archive
PASS (b): All 26 Mach-O files use @rpath/ cleanly with zero Homebrew/local path leaks or forbidden linkages
  libheif.1.dylib contains forbidden x265 symbol/linkage string (libx265/x265_encoder/x265_param)
  Non-exempt binary libheif.1.dylib contains string 'x265'
FAIL (c): Forbidden x265 strings or symbols detected
PASS (d): libde265.0.dylib present and dynamically linked by libheif.1.dylib
  Verified 16 components covering 22 regular dylibs.
  LGPL components: gettext (LGPL-2.1-or-later), glib (LGPL-2.1-or-later), libde265 (LGPL-3.0-or-later), libexif (LGPL-2.1-or-later AND LGPL-2.0-or-later), libheif (LGPL-3.0-or-later), vips (LGPL-2.1-or-later)
  Permissive components: aom (BSD-2-Clause), highway (Apache-2.0 OR BSD-3-Clause), jpeg-turbo (IJG AND Zlib AND BSD-3-Clause), libpng (libpng-2.0), libtiff (libtiff), little-cms2 (MIT), pcre2 (BSD-3-Clause), webp (BSD-3-Clause), xz (0BSD), zstd (BSD-3-Clause)
  GPL status: NONE (0 GPL libraries bundled)
PASS (e): versions.json valid, libde265 present, all dylibs accounted for, zero unknown/GPL licenses
OVERALL: AUDIT FAILED
PATCH_NEW_RC=1
=== Auditing macOS libvips archive: run2-patched ===
[trimmed identical middle output; same FAIL (c) lines under SIGPIPE ignored as normal mode above]
OVERALL: AUDIT FAILED
PATCH_SIGPIPE_RC=1
```
*(Honesty check on the old script `/tmp/old-audit.sh /tmp/run2-patched`: it also exited `1` with `FAIL (c)`, because `strings -a libheif.1.dylib` emits `56,051` bytes total—smaller than macOS's 64 KB pipe buffer—so `strings` finished writing before `grep -q` closed the pipe. The >5 MB `test-audit.sh` cases `(iii-a)` and `(iii-b)` prove the real pipe-buffer failure mode where `f9cb8aa` failed open 8/8 times.)*

---

### 5.5 Test-Job Pre-Flight on Run #2 (`42806945658`) Unpacked Artifact & `libvips.42.dylib` Enum Check (Item 3)

*(Run before Run #3 without moving `/opt/homebrew` on the local Mac.)*

```text
$ VIPS_PREFIX=/tmp/run2-unpacked/vips-macos-arm64-8.18.6-hevc
$ strings -a "$VIPS_PREFIX/lib/libvips.42.dylib" | grep -i x265 ; nm -gU "$VIPS_PREFIX/lib/libvips.42.dylib" | grep -i x265 || echo "(none)"
VIPS_FOREIGN_HEIF_ENCODER_X265
x265
NM:
(none)

$ DYLD_PRINT_LIBRARIES=1 "$VIPS_PREFIX/bin/vips" -l > /tmp/dyld.txt 2>&1
# Summary of 711 dyld lines (600 parsed library paths):
# - 23 unique paths inside /private/tmp/run2-unpacked/vips-macos-arm64-8.18.6-hevc/
# - 195 paths inside /usr/lib/
# - 382 paths inside /System/
# - 0 paths outside <prefix>, /usr/lib, and /System (no /opt/homebrew or /usr/local)
dyld lines: 711, parsed paths: 600, prefix_unique: 23, /usr/lib/: 195, /System/: 382, outside: 0
  /private/tmp/run2-unpacked/vips-macos-arm64-8.18.6-hevc/bin/vips
  /private/tmp/run2-unpacked/vips-macos-arm64-8.18.6-hevc/lib/libaom.3.dylib
  /private/tmp/run2-unpacked/vips-macos-arm64-8.18.6-hevc/lib/libde265.0.dylib
  /private/tmp/run2-unpacked/vips-macos-arm64-8.18.6-hevc/lib/libexif.12.dylib
  /private/tmp/run2-unpacked/vips-macos-arm64-8.18.6-hevc/lib/libgio-2.0.0.dylib
  /private/tmp/run2-unpacked/vips-macos-arm64-8.18.6-hevc/lib/libglib-2.0.0.dylib
  /private/tmp/run2-unpacked/vips-macos-arm64-8.18.6-hevc/lib/libgmodule-2.0.0.dylib
  /private/tmp/run2-unpacked/vips-macos-arm64-8.18.6-hevc/lib/libgobject-2.0.0.dylib
  /private/tmp/run2-unpacked/vips-macos-arm64-8.18.6-hevc/lib/libheif.1.dylib
  /private/tmp/run2-unpacked/vips-macos-arm64-8.18.6-hevc/lib/libhwy.1.dylib
  /private/tmp/run2-unpacked/vips-macos-arm64-8.18.6-hevc/lib/libintl.8.dylib
  /private/tmp/run2-unpacked/vips-macos-arm64-8.18.6-hevc/lib/libjpeg.8.dylib
  /private/tmp/run2-unpacked/vips-macos-arm64-8.18.6-hevc/lib/liblcms2.2.dylib
  /private/tmp/run2-unpacked/vips-macos-arm64-8.18.6-hevc/lib/liblzma.5.dylib
  /private/tmp/run2-unpacked/vips-macos-arm64-8.18.6-hevc/lib/libpcre2-8.0.dylib
  /private/tmp/run2-unpacked/vips-macos-arm64-8.18.6-hevc/lib/libpng16.16.dylib
  /private/tmp/run2-unpacked/vips-macos-arm64-8.18.6-hevc/lib/libsharpyuv.0.dylib
  /private/tmp/run2-unpacked/vips-macos-arm64-8.18.6-hevc/lib/libtiff.6.dylib
  /private/tmp/run2-unpacked/vips-macos-arm64-8.18.6-hevc/lib/libvips.42.dylib
  /private/tmp/run2-unpacked/vips-macos-arm64-8.18.6-hevc/lib/libwebp.7.dylib
  /private/tmp/run2-unpacked/vips-macos-arm64-8.18.6-hevc/lib/libwebpdemux.2.dylib
  /private/tmp/run2-unpacked/vips-macos-arm64-8.18.6-hevc/lib/libwebpmux.3.dylib
  /private/tmp/run2-unpacked/vips-macos-arm64-8.18.6-hevc/lib/libzstd.1.dylib
```

Manual pre-flight steps 1–4 header and SVG outputs under `env -i PATH="/usr/bin:/bin:/usr/sbin:/sbin:$VIPS_PREFIX/bin"`:
```text
/tmp/preflight-out/heic_thumb.png: 240x320 uchar, 3 bands, srgb, pngload
/tmp/preflight-out/heic_copy.png: 480x640 uchar, 3 bands, srgb, pngload
/tmp/preflight-out/p3_copy.png: 640x480 uchar, 3 bands, srgb, pngload
/tmp/preflight-out/p3_thumb.png: 320x240 uchar, 3 bands, srgb, pngload
VipsForeignLoad: "tests/fixtures/generated/fixture_blocked.svg" is not a known file format
SVG_UNBLOCKED_EXIT=1
VipsForeignLoad: "tests/fixtures/generated/fixture_blocked.svg" is not a known file format
SVG_BLOCKED_EXIT=1
```

---

### 5.6 Local Verification Suite (`pnpm verify`)

```text
$ pnpm verify
[WARN] Issue while reading "/Users/narayanwaraich/.npmrc". Operation not permitted (os error 1)
$ node scripts/verify.mjs
Photos Manager — pnpm verify verification run
Date: 2026-11-11T03:46:15.463Z
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

    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.86s

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
test metadata::tests::test_exiftool_malformed_output ... ok
test metadata::tests::test_exiftool_success ... ok
test metadata::tests::test_exiftool_non_ascii_filename ... ok
test metadata::tests::test_exiftool_long_path ... ok
test metadata::tests::test_exiftool_timeout ... ok
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

    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.30s
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

✓ built in 58ms
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
*(Task verification commands not executed because Part A stops at Checkpoint 1 and Run #3 ran cleanly via scoped `push` trigger prior to its removal: `gh workflow run deps-macos-vips.yml` and `gh release view macos-arm64-vips-8.18.6-hevc1 --repo narayanwaraich/photos-manager-deps`.)*

---

## 6. Decisions made

1. **Runner `macos-15` arm64 (`MACOSX_DEPLOYMENT_TARGET=15.0`):** `macos-14` images retired on 2026-11-02 (`https://github.com/actions/runner-images/pull/15227`, referencing `#13518`). `macos-15` (`macos-15-arm64` `20261104.0514.1`, `15.8.2` Build `24H232`, `Xcode 16.4` `16F6`) builds binaries compatible with both `macos-15` and `ci.yml`'s `macos-latest` (`macos-26-arm64`).
2. **Disable `cgif`, enable `nsgif` (`-Dcgif=disabled -Dnsgif=true`):** In `vips-8.18.6/meson.build:298`, `cgif_dep` is gated inside `if quantisation_package.found()` (`imagequant` or `quantizr`). Homebrew `libimagequant` 4.4.1 is `GPL-3.0-or-later` (forbidden) and `quantizr` is absent from Homebrew, whereas Windows MXE uses `libimagequant` `2.4.1` (`BSD-2-Clause`). GIF reading remains built-in via `-Dnsgif=true`; GIF writing via `cgif` is disabled.
3. **Isolated `PKG_CONFIG_LIBDIR` (skipping `/opt/homebrew/lib/pkgconfig` and Homebrew `libffi`):** Clearing `PKG_CONFIG_PATH` and constructing `PKG_CONFIG_LIBDIR` strictly from `STAGE_PREFIX/lib/pkgconfig`, allowlisted leaf kegs (`deps/macos-vips/build.sh:89-145`), and Apple's SDK shims guarantees preinstalled runner formulas (`fftw`, `x265`, `librsvg`, `poppler`, `imagemagick`) cannot be discovered by CMake or Meson, while `expat`, `zlib`, `iconv`, and `ffi` resolve to `/usr/lib/*.dylib`.
4. **Scoped `x265` enum nick exemption in `audit-dylibs.sh:159-163`:** `libvips.42.dylib` always contains `"x265"` / `"VIPS_FOREIGN_HEIF_ENCODER_X265"` from `foreign.h:1065,1088,1100` even with `-DWITH_X265=OFF`. The audit exempts bare `"x265"` **only** in `libvips*.dylib`, while failing on `libx265`, `x265_encoder`, `x265_param`, any `nm -g` `x265` symbol everywhere, and any `"x265"` string in `libheif` or any other Mach-O binary.
5. **Eliminate `producer | grep -q` across all scripts and workflows:** Under `set -euo pipefail`, `grep -q` exits immediately on first match and closes stdin, causing large writers (`strings`, `nm`, `echo`) to fail with `Broken pipe` (`exit 141` when `SIGPIPE` is default, or write-error `exit 1`/`2` when `SIGPIPE` is ignored on GHA runners) and causing `if producer | grep -q` checks to fail open (`false`). All checks now redirect `strings`/`nm` to `mktemp` files cleaned by `cleanup()` traps (`audit-dylibs.sh:32-41,148-164`; `win64-vips/audit-zip.sh:32-33,63-86,106-111`) or capture small variables for `[[ ... == *...* ]]` / `<<<` here-strings.
6. **Specific `*"(heifload),"*` workflow check (`.github/workflows/deps-macos-vips.yml:131`):** Requiring `(heifload),` avoids matching `heifload_base` first and guarantees `VipsForeignLoadHeifFile (heifload)` is actually registered.
7. **Explicit runtime-only license split rules (`build.sh:630-649`):** `brew info --json=v2` reports formula-wide SPDX strings (`GPL-3.0-or-later AND LGPL-2.1-or-later` for `gettext`; `0BSD AND GPL-2.0-or-later` for `xz`; `(BSD-3-Clause OR GPL-2.0-only) AND BSD-2-Clause AND MIT` for `zstd`). Because only the LGPL/permissive runtime dylibs (`libintl.8.dylib`, `liblzma.5.dylib`, `libzstd.1.dylib`) are bundled and CLI utilities are omitted, `build.sh` validates `expected_raw` + `expected_dylibs` and documents both `license` and `rawFormulaLicense` in `versions.json`.
8. **Compile-time unversioned symlinks in `lib/` (`build.sh:577-586`):** Created unversioned `.dylib` symlinks (`libvips.dylib` -> `libvips.42.dylib`, etc.) alongside versioned runtime dylibs so `cargo build` links cleanly against `$VIPS_LIB_DIR` in Part B while `install_name_tool` IDs remain `@rpath/<versioned>.dylib`.

---

## 7. Deviations from the task, spec or amendments

1. **Orchestrator Round 1 scope widening (`.github/`, `deps/macos-vips/`, `reports/`, plus `deps/win64-vips/audit-zip.sh`):** Approved fixing the full `producer | grep -q` bug class (including `deps/win64-vips/audit-zip.sh` as a single permitted exception outside `deps/macos-vips/`, committed test `deps/macos-vips/test-audit.sh`), running Run #3 (`42945772807`), populating `deps/macos-vips/UPSTREAM.txt` with Run #3 values, and stopping at **Checkpoint 1** before Part B.
2. **Temporary scoped `push` trigger used for Runs #1–#3, now removed:** GitHub rejects `workflow_dispatch` for workflows not yet on `main`, so `.github/workflows/deps-macos-vips.yml` carried a path-scoped `push` trigger on `task/0F` through Run #3 (`42945772807`). That trigger has been removed in the final `[skip ci]` commit, leaving `workflow_dispatch:` only as specified in Task Part A.5.
3. **`cgif` disabled on macOS vs Task Part A.1 wording (`"keep ... cgif ... as in the Windows zip"`):** Disabled (`-Dcgif=disabled -Dnsgif=true`) because `libvips` `8.18.6` `meson.build:298` requires `imagequant` or `quantizr` before probing `cgif`, and Homebrew `libimagequant` is `GPL-3.0-or-later` (Decision #2 above).

---

## 8. Known gaps and risks

1. **Historical Run #2 failure (now fixed & verified in Run #3):** Run #2 `test-clean-macos` failed at line `139` (`if ! echo "$LOADERS_OUT" | grep -q "heifload"`) with `echo: write error: Broken pipe`, and `echo` returned `1` because GHA runners ignore `SIGPIPE` (`SIG_IGN`) whereas a default-signal shell exits `141`. Workflow lines `154, 162, 173, 181, 199, 213` at `631a85b` shared the `"same pattern, not exposed"` because their variables held single-line strings; lines `139` and `143` (large ~23 KB `LOADERS_OUT`) and `audit-dylibs.sh` / `win64-vips/audit-zip.sh` `strings`/`nm` pipes were the exposed fail-open paths, all now eliminated and verified.
2. **Checkpoint 1 & Part B app integration not yet executed:**
   - Verified 14-day artifact `vips-macos-arm64-hevc` (`ID 13822214591`, `SHA-256: 6bb24d51dedf9f69344aa187cb0f9651b7842de4c465643480f3a794481dba64`) has not yet been published as release `macos-arm64-vips-8.18.6-hevc1` in `narayanwaraich/photos-manager-deps` (owner action at Checkpoint 1).
   - `.github/workflows/ci.yml` still installs Homebrew `vips` on `macos-arm64`; thus `ci.yml` green runs `42804777791`, `42806945665`, and `42945772425` across `macos-arm64`, `windows-x64`, and `Security & License check` prove only that nothing regressed.
   - `src-tauri/build.rs`, `scripts/package-bundle.mjs` (which must also skip unversioned compile symlinks in `lib/` so only the 22 runtime `.dylib`s enter `Contents/Frameworks/`), `docs/native-dependencies.md` §4.1/§4.3 (67-row Homebrew table -> 22 bundled `.dylib`s), `REQUIRE_HEIC=1` / fork-PR fallback, fail-closed negative test (`task/0F-negtest`), and `docs/macos-validation.md` remain for Part B after Checkpoint 1.
3. **Single-OS testing & hardware gaps:** Tested only on Apple Silicon (`macos-15-arm64` in CI and local M-series macOS). Intel macOS (`x86_64`) and real iPhone HEIC samples (owner step in `docs/macos-validation.md`) were not tested; Windows `audit-zip.sh` was verified on macOS via stock `vips-dev-x64-web-8.18.6.zip` and fake-`strings` regressions only (`ci.yml` Windows build unchanged).
4. **Runtime/build risks:** Homebrew bottle formula versions (`glib 2.90.1`, etc.) float on future manual runs of `build.sh` (though once pinned by SHA-256 in `photos-manager-deps`, app CI is hermetic). HEVC patent licensing (Decision D13 / Phase 7) remains separate from copyright licenses and requires review prior to public distribution.

---

## 9. Questions for the human / orchestrator

### Checkpoint 1 Handoff (Owner Action Required Before Part B)
1. **Download and inspect the verified 14-day artifact from Run #3 (`42945772807`):**
   ```bash
   gh run download 42945772807 -n vips-macos-arm64-hevc -D /tmp/checkpoint1-vips
   shasum -a 256 /tmp/checkpoint1-vips/vips-macos-arm64-8.18.6-hevc.zip
   # Expected:
   # 6bb24d51dedf9f69344aa187cb0f9651b7842de4c465643480f3a794481dba64  .../vips-macos-arm64-8.18.6-hevc.zip
   ```
2. **Publish private release `macos-arm64-vips-8.18.6-hevc1`** in `narayanwaraich/photos-manager-deps` attaching `vips-macos-arm64-8.18.6-hevc.zip` (`5,793,473` bytes, `SHA-256: 6bb24d51dedf9f69344aa187cb0f9651b7842de4c465643480f3a794481dba64`), confirm the release exists and matches, and authorize starting Part B (`ci.yml`, `src-tauri/build.rs`, `scripts/package-bundle.mjs`, `docs/native-dependencies.md`, `docs/macos-validation.md`).

---

## 10. Proposed next task

Resume **TASK-0F (Part B)** on branch `task/0F` once Checkpoint 1 confirms release `macos-arm64-vips-8.18.6-hevc1` (`SHA-256: 6bb24d51dedf9f69344aa187cb0f9651b7842de4c465643480f3a794481dba64`) in `narayanwaraich/photos-manager-deps`: update `.github/workflows/ci.yml` to fail closed on `DEPS_TOKEN` + verified macOS zip download + `REQUIRE_HEIC=1` (with fork fallback and `task/0F-negtest` negative test proof), wire `$VIPS_DIR`/`$VIPS_LIB_DIR` into `src-tauri/build.rs` and `scripts/package-bundle.mjs`, rewrite `docs/native-dependencies.md` §4.1/§4.3 (verifying `check-dylib-table`), and add `docs/macos-validation.md`.

---

## Concepts used (for the owner, a React developer learning Rust and Git)

1. **`SIGPIPE` Ignored Disposition & Fail-Open Pipelines**: When `grep -q` exits early on a match and closes its stdin pipe while the writer still has kilobytes/megabytes buffered, `set -o pipefail` treats the resulting broken-pipe error (`141` under default signals, or `1`/`2` when `SIGPIPE` is ignored on CI runners) as pipeline failure—turning `if producer | grep -q` silently `false` (fail-open).  
   *Example:* `deps/macos-vips/audit-dylibs.sh:148-164`
2. **Regression Testing With `PATH` Shims & `trap "" PIPE`**: Placing mock `strings` and `nm` binaries ahead of system tools on `$PATH` and re-running under `bash -c 'trap "" PIPE; ...'` deterministically reproduces both desktop (`SIG_DFL`) and CI-runner (`SIG_IGN`) pipe failures in seconds.  
   *Example:* `deps/macos-vips/test-audit.sh:154-202`
3. **Mach-O `@rpath` and `@loader_path` Relocation**: Rewriting dylib IDs to `@rpath/<file>` and setting `LC_RPATH` to `@loader_path` (`@executable_path/../lib` for binaries) makes the entire native dependency tree portable to any unpack path with zero Homebrew dependency.  
   *Example:* `deps/macos-vips/build.sh:537-564`
4. **Build-Time Link Symlinks vs. Runtime Versioned Dylibs**: Compilers and Cargo (`build.rs`) pass `-lvips` expecting an unversioned `libvips.dylib` symlink, whereas macOS `dyld` loads only the versioned `@rpath/libvips.42.dylib` ID at runtime.  
   *Example:* `deps/macos-vips/build.sh:577-586`
5. **`[skip ci]` Metadata Commits & Trigger Cleanup in Git**: Putting `[skip ci]` in the final commit subject prevents removing a temporary `push` trigger or updating `UPSTREAM.txt` and `REPORT-0F.md` from kicking off an unnecessary fourth dependency build on `task/0F`.  
   *Example:* `.github/workflows/deps-macos-vips.yml:3-4`

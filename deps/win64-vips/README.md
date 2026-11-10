# Windows x64 libvips (HEVC-Decode-Only) Build Recipe

This directory contains the recipe and patches to build a reproducible, minimal, LGPL-licensed Windows x64 `libvips` package with `libheif` + `libde265` for decoding iPhone HEIC/HEVC photos, with **zero GPL contamination** (x265 encoder is completely excluded).

## Background & Rationale

- **Upstream flag:** Upstream `build-win64-mxe` provides `./build.sh --with-hevc`, which compiles `libde265` (LGPL-3.0 decoder) together with `x265` (GPL-2.0 encoder).
- **Compliance requirement:** Photos Manager requires a permissive/LGPL runtime without GPL copyleft licensing obligations. Encoding HEVC is out of scope.
- **Recipe:** We patch the upstream MXE recipe at pinned commit `09cfccf20b91b441fbe97fa7a7ed8a597e55e830` (tag `v8.18.6`):
  1. `build.sh`: Removes `--with-hevc` from `contains_gpl_libs` since our build contains zero GPL libraries.
  2. `build/libheif.mk`: Adds `-DWITH_X265=0` explicitly while enabling `WITH_LIBDE265` when `IS_HEVC` is set.
  3. `build/plugins/hevc-deps/overrides.mk`: Removes `x265` from `libheif_DEPS` and deletes all `x265` download/build definitions.
  4. `build/plugins/hevc-deps/patches/`: Removes the unused `x265` patch.
  5. `build/plugins/web-deps/vips-web.mk`: Adds `"libde265": "$(libde265_VERSION)"` to `versions.json`.

## How to Build

### Using GitHub Actions (Recommended)
Trigger workflow `.github/workflows/deps-win64-vips.yml` via `workflow_dispatch`:
```bash
gh workflow run deps-win64-vips.yml
```
The workflow:
1. Clones upstream at pinned commit `09cfccf20b91b441fbe97fa7a7ed8a597e55e830`.
2. Verifies the commit SHA cryptographically.
3. Applies `patches/0001-enable-libde265-without-x265.patch`.
4. Runs `./build.sh --with-hevc -t x86_64-w64-mingw32.shared vips-web`.
5. Runs strict audit checks:
   - Fails if any x265 DLL or symbol appears in `bin/`.
   - Fails if `libde265` DLL is missing.
   - Fails if `versions.json` lacks `libde265`.
   - Fails if `fftw`, `imagemagick`, or `poppler` are present.
6. Runs a Windows verification job with reduced PATH (`C:\Windows\System32;C:\Windows` + zip's `bin`), decoding a real HEVC HEIC fixture and testing the SVG untrusted loader block control.
7. Publishes release `deps-win64-vips-8.18.6-hevc1` with the verified archive attached.

### Local Container Build
```bash
git clone https://github.com/libvips/build-win64-mxe.git
cd build-win64-mxe
git checkout 09cfccf20b91b441fbe97fa7a7ed8a597e55e830
patch -p1 < /path/to/photos-manager/deps/win64-vips/patches/0001-enable-libde265-without-x265.patch
./build.sh --with-hevc -t x86_64-w64-mingw32.shared vips-web
```
The resulting archive will be located at `packaging/vips-dev-x64-web-8.18.6-hevc.zip`.

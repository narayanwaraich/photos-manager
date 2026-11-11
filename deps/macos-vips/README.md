# Custom macOS arm64 libvips (HEVC Decode Only, No x265, No GPL)

This directory contains the reproducible build and audit recipe for Photos Manager's macOS arm64 `libvips` distribution (`vips-macos-arm64-8.18.6-hevc.zip`).

## Why a custom build?

Stock Homebrew `vips` cannot be shipped in the packaged `.app` cleanly (TASK-0D / TASK-0F):
1. Homebrew links `fftw` (`GPL-2.0-or-later`) and `libimagequant` `4.4.x` (`GPL-3.0-or-later`).
2. Homebrew compiles `heifload` as a separate runtime module (`vips-modules-8.18/vips-heif.dylib`) whose `libheif` hard-links GPL `x265`.
3. Homebrew floating bottles drag 67 dylibs including X11, Cairo, Pango, Fontconfig, librsvg, Poppler, OpenEXR, and LibRaw.

Our recipe builds pinned upstream tarballs matching Windows (`0E`):
- `libvips` `8.18.6` (`-Dmodules=disabled`, built-in `heifload`, no `fftw`, no `imagequant`, no `magick`, no `rsvg`, no `poppler`, no `openslide`, no `openexr`, no `jpeg-xl`, no `raw`)
- `libheif` `1.23.2` (`-DENABLE_PLUGIN_LOADING=OFF`, `-DWITH_LIBDE265=ON`, `-DWITH_AOM_DECODER=ON`, `-DWITH_X265=OFF`, encoders disabled)
- `libde265` `1.1.1` (HEVC decoder, LGPL-3.0-or-later, SDL/tools/encoders disabled)
- `libaom` `3.14.1` (AV1 decoder only, BSD-2-Clause, encoder/tools disabled)

Homebrew is used **strictly** for build tools (`cmake`, `meson`, `ninja`, `pkgconf`) and allowlisted non-GPL leaf libraries (`glib`, `gettext`, `pcre2`, `libffi`, `jpeg-turbo`, `libpng`, `webp`, `libtiff`, `little-cms2`, `libexif`, `highway`, `zstd`, `xz`). `expat`, `zlib`, `iconv`, and `ffi` resolve to macOS system libraries (`/usr/lib/`) unless explicitly pulled by GLib.

> Note on `cgif`: In `libvips 8.18.6` (`meson.build:297-307`), `cgif` is only probed when `imagequant` or `quantizr` is present. Because GPL `imagequant` 4.x is excluded, GIF saving via `cgif` is disabled while GIF decoding remains built in (`-Dnsgif=true`).

## Layout & Relocation

`vips-macos-arm64-8.18.6-hevc.zip` unpacks cleanly to a single directory `vips-macos-arm64-8.18.6-hevc/`:
- `bin/` — `vips`, `vipsthumbnail`, `vipsheader`, `vipsedit` (`LC_RPATH = @executable_path/../lib`)
- `lib/` — Relocated closure `.dylib` files (`LC_ID_DYLIB = @rpath/<file>`, `LC_RPATH = @loader_path`, zero `/opt/homebrew` or `/usr/local` references, ad-hoc signed with `codesign --force -s -`), plus compile-time unversioned `.dylib` link symlinks (`libvips.dylib`, `libglib-2.0.dylib`, `libgobject-2.0.dylib`) and rewritten `lib/pkgconfig/*.pc`
- `include/` — Headers from staged builds
- `share/` — Built `libvips` data/man/locale assets
- `versions.json` — Machine-readable inventory of built tarballs and Homebrew leaf formulas (with `brew info --json=v2` licenses)
- `LICENSES.md` — Human-readable component and license summary (noting LGPL dynamic libraries and zero GPL libraries)

## Local Usage

```bash
# 1. Install build tools and allowlisted leaf dependencies via Homebrew
brew install cmake meson ninja pkgconf glib gettext pcre2 jpeg-turbo libpng webp libtiff little-cms2 libexif highway zstd xz

# 2. Build and package relocatable archive in current directory
./deps/macos-vips/build.sh

# 3. Audit the generated zip (filenames, otool -L, strings/nm symbols, libde265 linkage, licenses)
./deps/macos-vips/audit-dylibs.sh ./vips-macos-arm64-8.18.6-hevc.zip
```

## CI Usage

Run the `Build & Test macOS arm64 libvips (HEVC Decode)` workflow (`.github/workflows/deps-macos-vips.yml`). It builds on `macos-15` (Apple Silicon), audits via `audit-dylibs.sh`, tests the archive on a second clean runner with `/opt/homebrew` renamed to `/opt/homebrew.off` and stripped `PATH`, and uploads the verified `vips-macos-arm64-hevc` artifact.

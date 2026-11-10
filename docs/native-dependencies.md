# Native Dependencies & Feasibility Findings

**Tasks:** TASK-0A, TASK-0B, TASK-0C · **Date:** 2026-11-06 · **Verified on:** macOS 27.0 (arm64) and Windows Server 2025 (x64 via CI)

This document records the exact versions, sources, licenses, install requirements, codecs, bundling strategies, and risks for all native dependencies evaluated in Phase 0. Every dependency and model shape has been verified against official registries or documentation (rule: *verify, do not recall*).

> [!NOTE]
> **Dev-Only Homebrew Paths Gated:** Link search paths (`/opt/homebrew/lib` in `src-tauri/build.rs`) and candidate executable paths in `which_exiftool()` are strictly developer conveniences for local development. Release builds gate these paths behind `cfg(debug_assertions)` and explicit dev environment flags (`PHOTOS_DEV_HOMEBREW=1`, `PHOTOS_DEV_ALLOW_SYSTEM_EXIFTOOL=1`), requiring all release packages to resolve libvips and ExifTool hermetically from the application bundle.

---

## 1. Verified Dependency Matrix

| Component | Verified Version | License | Source / Registry URL | Date Verified | Notes / Install Requirements |
|---|---|---|---|---|---|
| **Rust Toolchain** | `1.99.1` (pinned) | MIT OR Apache-2.0 | `https://www.rust-lang.org/` | 2026-11-06 | Host `aarch64-apple-darwin` and `x86_64-pc-windows-msvc`. Pinned via `rust-toolchain.toml` with `rustfmt` and `clippy`. |
| **cargo-deny Action** | `3c6349835b2b...` (v2.1.1) | MIT OR Apache-2.0 | `https://github.com/EmbarkStudios/cargo-deny-action` | 2026-11-06 | cargo-deny `0.20.2`. Action pinned by full commit SHA in `.github/workflows/ci.yml`. |
| **Tauri Core** | `tauri 2.12.5` | MIT OR Apache-2.0 | `https://crates.io/crates/tauri/2.12.5` | 2026-11-02 | Tauri 2 stable release. Least-privilege capabilities configured in `src-tauri/capabilities/default.json`. |
| **Tauri CLI / API** | `@tauri-apps/cli 2.12.4`, `@tauri-apps/api 2.12.4` | MIT OR Apache-2.0 | `https://www.npmjs.com/package/@tauri-apps/cli` | 2026-11-02 | Node 26.9.0, pnpm 12.8.1. |
| **SQLx (SQLite)** | `sqlx 0.8.6` | MIT OR Apache-2.0 | `https://crates.io/crates/sqlx/0.8.6` | 2026-11-02 | Features: `sqlite`, `runtime-tokio`, `migrate`, `macros`. Bundled `libsqlite3-sys 0.30.1`. Configured with WAL, foreign keys ON, busy timeout 5000ms. |
| **libvips Rust Binding** | `libvips 1.7.2` (patched) | MIT | `https://github.com/kuma-00/libvips-rust-bindings-for-windows.git` | 2026-11-06 | Fork commit `80097ad9387180f678b21482b8b0cfbfb436a308`. Fixes Windows LLP64 64-bit integer sizing across crates.io patch. |
| **libvips (Native C Library)** | `8.18.6` (Windows), `8.18.7` (macOS) | LGPL-2.1-or-later | `https://github.com/libvips/libvips` | 2026-11-06 | Dynamically linked. Minimal codec configuration verified; untrusted loaders blocked at startup. |
| **ExifTool** | `13.61` | Perl Artistic License 1.0 OR GPL-1.0-or-later | `https://exiftool.org/` | 2026-11-06 | Official distribution on both OSes. Windows standalone zip (Strawberry Perl runtime) + macOS Perl script. |
| **ONNX Runtime Rust Binding** | `ort = "=2.0.0-rc.15"` | MIT OR Apache-2.0 | `https://crates.io/crates/ort/2.0.0-rc.15` | 2026-11-05 | Candidate embedding runtime from TASK-0B. Pinned with exact version (`=`). |
| **ONNX Runtime (Native C Library)** | `1.20.1` | MIT | `https://github.com/microsoft/onnxruntime` | 2026-11-05 | Bundling documented in §4.5. Not integrated in product code until Phase 4B. |
| **EmbeddingGemma 2** (Model) | Spike in TASK-0B | Apache-2.0 | `https://ai.google.dev/gemma/docs/embeddinggemma` | 2026-11-02 | Multimodal (768-d / 256-d Matryoshka). Weights downloaded on consent; see TASK-0B. |

---

## 2. Gemini Cloud AI API Verification

- **Official Documentation Source:** `https://ai.google.dev/api/generate-content` / `https://ai.google.dev/gemini-api/docs/models` (Verified 2026-11-02)
- **Model IDs:**
  - Recommended active production model IDs: `gemini-3.8-flash`, `gemini-3.5-flash-lite`.
  - Legacy models: `gemini-2.5-flash`.
  - *Invariant adherence:* Model IDs are stored as user-configurable settings in SQLite (`app_settings`), never hardcoded in Rust logic.
- **Privacy & Safety Invariants (Spec §9.3):**
  - EXIF GPS tags and file paths are stripped before sending.
  - Payloads require explicit per-provider user consent.
  - API keys reside solely in the OS credential store (Keyring / Keychain / Credential Manager), never in SQLite, files, logs, or WebView.

---

## 3. Decision D4: IPC Types Generation Check (`tauri-specta`)

- **Candidate:** `tauri-specta` (`https://crates.io/crates/tauri-specta`)
- **Recommendation for Phase 1:** Keep hand-written DTOs + Zod validation as the zero-dependency baseline for Phase 0. The project owner will formally decide whether to adopt `tauri-specta` 2.0-rc or hand-written DTOs at the start of Phase 1.

---

## 4. Packaging, Provenance & Clean-Machine Distribution Strategy

**Fundamental Rule (Spec §3.4):** System-installed copies (via Homebrew, Chocolatey, Scoop, or winget) are strictly developer conveniences and **are not sufficient** for end-user distribution. The packaged application must be entirely self-contained.

### 4.1 Downloaded Archives Provenance & Checksums

All external archives downloaded during CI and packaging are pinned with exact upstream URLs and verified by cryptographic SHA-256 checksums. CI fails immediately if any archive checksum does not match.

| Package / Archive | OS | Version | Exact Download URL | Verified SHA-256 |
|---|---|---|---|---|
| `vips-dev-x64-web-8.18.6-hevc.zip` | Windows x64 | 8.18.6-hevc1 | `https://github.com/narayanwaraich/photos-manager-deps/releases/download/win64-vips-8.18.6-hevc1/vips-dev-x64-web-8.18.6-hevc.zip` (private repo, token-authenticated) | `72591a6bdf775618bc154e83990c86919ecb18f6e9d72fade5baef9b4f343991` |
| `vips-dev-x64-web-8.18.6.zip` (stock fallback) | Windows x64 | 8.18.6 | `https://github.com/libvips/build-win64-mxe/releases/download/v8.18.6/vips-dev-x64-web-8.18.6.zip` | `10086f2ccc8e2a861831facabab75fa1b80b4bd03f0a8447623ec581002aa4b2` |
| `exiftool-13.61_64.zip` | Windows x64 | 13.61 | `https://downloads.sourceforge.net/project/exiftool/exiftool-13.61_64.zip` | `ab75b37515385a753aa60a58f68ffffe1ff81e43b319df9540978fb19e0580e5` |
| `Image-ExifTool-13.61.tar.gz` | macOS arm64 | 13.61 | `https://downloads.sourceforge.net/project/exiftool/Image-ExifTool-13.61.tar.gz` | `df7f42c14b4f58a60bec39feae0ee9593000ee27e44920d2b0a21c6627de0baa` |

*Note on ExifTool URLs:* Official download links on `https://exiftool.org/` redirect to official SourceForge project mirrors. Both Windows standalone and macOS Perl archives are pinned to identical version 13.61.

---

### 4.2 Windows Custom `vips-dev-x64-web-8.18.6-hevc` Component & License Audit

The custom Windows precompiled package (`vips-dev-x64-web-8.18.6-hevc.zip`, release `win64-vips-8.18.6-hevc1` in private repository `narayanwaraich/photos-manager-deps`, SHA-256 `72591a6bdf775618bc154e83990c86919ecb18f6e9d72fade5baef9b4f343991`) provides a clean-machine runtime with LGPL HEVC decoding and no GPL component identified in this list (not a legal opinion; see risk 17 in docs/STATUS.md):
- **Build System:** Pinned MXE cross-compilation container (`ghcr.io/libvips/build-win64-mxe@sha256:07d164cd05770a6f6236b22c489be7d634fc1227b59a35cf32bd41963e163b74`, Decision D14).
- **Archive Structure:** Contains a single root directory `vips-dev-8.18/` with `bin/` (39 DLLs and 4 EXEs: `vips.exe`, `vipsedit.exe`, `vipsheader.exe`, `vipsthumbnail.exe`), `include/`, `lib/`, `share/`, `etc/`, `ChangeLog`, `LICENSE` (LGPL-2.1 text), `README.md`, and `versions.json`.
- **Archive License Policy:** The archive ships **no per-component license files** and no `licenses/` directory; only the root LGPL-2.1 `LICENSE` is bundled. Per-component licenses cited below are verified directly from upstream package repositories.
- **Component Inventory (`versions.json`):** Exactly 29 upstream components are documented in `versions.json`. All 39 DLLs in `bin/` are accounted for by these 29 components plus the MXE LLVM toolchain runtime libraries:

| Component | Version (`versions.json`) | Bundled DLL(s) in `bin/` | Upstream License & Citation URL | Distribution Status |
|---|---|---|---|---|
| `aom` | `3.14.1` | `libaom.dll` | [BSD-2-Clause](https://aomedia.googlesource.com/aom/+/refs/heads/main/LICENSE) | Permissive |
| `archive` | `3.8.9` | `libarchive-13.dll` | [BSD-2-Clause](https://github.com/libarchive/libarchive/blob/v3.8.9/COPYING) | Permissive |
| `cairo` | `1.18.4` | `libcairo-2.dll` | [LGPL-2.1-only OR MPL-1.1](https://gitlab.freedesktop.org/cairo/cairo/-/blob/1.18.4/COPYING) | LGPL (Permitted via dynamic linking) |
| `cgif` | `0.5.3` | `libcgif-0.dll` | [MIT](https://github.com/dloebl/cgif/blob/v0.5.3/LICENSE) | Permissive |
| `exif` | `0.6.26` | `libexif-12.dll` | [LGPL-2.1-or-later](https://github.com/libexif/libexif/blob/libexif-0_6_26-release/COPYING) | LGPL (Permitted via dynamic linking) |
| `expat` | `2.8.3` | `libexpat-1.dll` | [MIT](https://github.com/libexpat/libexpat/blob/R_2_8_3/expat/COPYING) | Permissive |
| `ffi` | `3.8.0` | `libffi-8.dll` | [MIT](https://github.com/libffi/libffi/blob/v3.8.0/LICENSE) | Permissive |
| `fontconfig` | `2.18.3` | `libfontconfig-1.dll` | [Fontconfig (MIT-like)](https://gitlab.freedesktop.org/fontconfig/fontconfig/-/blob/2.18.3/COPYING) | Permissive |
| `freetype` | `2.14.3` | `libfreetype-6.dll` | [FTL OR GPL-2.0-only](https://gitlab.freedesktop.org/freetype/freetype/-/blob/VER-2-14-3/docs/FTL.TXT) | Permissive (under FTL) |
| `fribidi` | `1.0.16` | `libfribidi-0.dll` | [LGPL-2.1-or-later](https://github.com/fribidi/fribidi/blob/v1.0.16/COPYING) | LGPL (Permitted via dynamic linking) |
| `glib` | `2.89.4` | `libgio-2.0-0.dll`, `libglib-2.0-0.dll`, `libgmodule-2.0-0.dll`, `libgobject-2.0-0.dll` | [LGPL-2.1-or-later](https://gitlab.gnome.org/GNOME/glib/-/blob/2.89.4/LICENSES/LGPL-2.1-or-later.txt) | LGPL (Permitted via dynamic linking) |
| `harfbuzz` | `14.3.1` | `libharfbuzz-0.dll` | [MIT](https://github.com/harfbuzz/harfbuzz/blob/14.3.1/COPYING) | Permissive |
| `heif` | `1.23.2` | `libheif.dll` | [LGPL-3.0-or-later](https://github.com/strukturag/libheif/blob/v1.23.2/COPYING) | LGPL (Permitted via dynamic linking) |
| `highway` | `1.4.0` | `libhwy.dll` | [Apache-2.0 OR BSD-3-Clause](https://github.com/google/highway/blob/1.4.0/LICENSE) | Permissive |
| `imagequant` | `2.4.1` | `libimagequant.dll` | [BSD-2-Clause](https://github.com/ImageOptim/libimagequant/blob/2.4.1/COPYRIGHT) | Permissive (Original C v2.4.1 fork) |
| `lcms` | `2.19.1` | `liblcms2-2.dll` | [MIT](https://github.com/mm2/Little-CMS/blob/lcms2.19.1/LICENSE) | Permissive |
| `libde265` | `1.1.1` | `libde265.dll` | [LGPL-3.0-or-later](https://github.com/strukturag/libde265/blob/v1.1.1/COPYING) | LGPL (Permitted via dynamic linking) |
| `mozjpeg` | `0826579` | `libjpeg-62.dll` | [BSD-3-Clause](https://github.com/mozilla/mozjpeg/blob/0826579/LICENSE.md) | Permissive |
| `pango` | `1.58.2` | `libpango-1.0-0.dll`, `libpangocairo-1.0-0.dll`, `libpangoft2-1.0-0.dll` | [LGPL-2.1-or-later](https://gitlab.gnome.org/GNOME/pango/-/blob/1.58.2/COPYING) | LGPL (Permitted via dynamic linking) |
| `pixman` | `0.46.4` | `libpixman-1-0.dll` | [MIT](https://gitlab.freedesktop.org/pixman/pixman/-/blob/pixman-0.46.4/COPYING) | Permissive |
| `png` | `1.6.58` | `libpng16-16.dll` | [libpng-2.0](https://github.com/pnggroup/libpng/blob/v1.6.58/LICENSE) | Permissive |
| `proxy-libintl` | `0.5` | *(integrated statically into glib)* | [LGPL-2.1-or-later](https://github.com/frida/proxy-libintl/blob/0.5/COPYING) | LGPL |
| `rsvg` | `2.62.91` | `librsvg-2-2.dll` | [LGPL-2.1-or-later](https://gitlab.gnome.org/GNOME/librsvg/-/blob/2.62.91/COPYING.LIB) | LGPL (Blocked at startup) |
| `tiff` | `4.7.2` | `libtiff-6.dll` | [libtiff (BSD-like)](https://gitlab.com/libtiff/libtiff/-/blob/v4.7.2/LICENSE.md) | Permissive |
| `uhdr` | `2.0.2` | `libuhdr.dll` | [Apache-2.0](https://github.com/google/libultrahdr/blob/v2.0.2/LICENSE) | Permissive |
| `vips` | `8.18.6` | `libvips-42.dll`, `libvips-cpp-42.dll` | [LGPL-2.1-or-later](https://github.com/libvips/libvips/blob/v8.18.6/LICENSE) | LGPL (Permitted via dynamic linking) |
| `webp` | `1.6.0` | `libsharpyuv-0.dll`, `libwebp-7.dll`, `libwebpdemux-2.dll`, `libwebpmux-3.dll` | [BSD-3-Clause](https://chromium.googlesource.com/webm/libwebp/+/refs/tags/v1.6.0/COPYING) | Permissive |
| `xml2` | `2.15.3` | `libxml2-16.dll` | [MIT](https://gitlab.gnome.org/GNOME/libxml2/-/blob/v2.15.3/Copyright) | Permissive |
| `zlib-ng` | `2.3.3` | `libz1.dll` | [Zlib](https://github.com/zlib-ng/zlib-ng/blob/2.3.3/LICENSE.md) | Permissive |
| *(MXE Toolchain)* | N/A | `libc++.dll`, `libunwind.dll` | [Apache-2.0 with LLVM Exception](https://llvm.org/LICENSE.txt) | Permissive |

- **License Breakdown:**
  - **LGPL libraries (permitted via dynamic linking):** `cairo` (1.18.4, LGPL-2.1-only or MPL-1.1), `exif` (0.6.26, LGPL-2.1-or-later), `fribidi` (1.0.16, LGPL-2.1-or-later), `glib` (2.89.4, LGPL-2.1-or-later), `heif` (1.23.2, LGPL-3.0-or-later), `libde265` (1.1.1, LGPL-3.0-or-later), `pango` (1.58.2, LGPL-2.1-or-later), `proxy-libintl` (0.5, LGPL-2.1-or-later), `rsvg` (2.62.91, LGPL-2.1-or-later), `vips` (8.18.6, LGPL-2.1-or-later).
  - **Permissive open-source libraries:** `aom` (BSD-2-Clause), `archive` (BSD-2-Clause), `cgif` (MIT), `expat` (MIT), `ffi` (MIT), `fontconfig` (MIT-like), `freetype` (FTL), `harfbuzz` (MIT), `highway` (Apache-2.0 or BSD-3-Clause), `imagequant` (BSD-2-Clause), `lcms` (MIT), `mozjpeg` (BSD-3-Clause), `pixman` (MIT), `png` (libpng-2.0), `tiff` (libtiff BSD-like), `uhdr` (Apache-2.0), `webp` (BSD-3-Clause), `xml2` (MIT), `zlib-ng` (Zlib), LLVM toolchain `libc++`/`libunwind` (Apache-2.0 with LLVM Exception).
  - **No GPL components identified:** The GPL-licensed `x265` encoder was stripped out via patch `0001-enable-libde265-without-x265.patch`. Disallowed heavy/GPL components (`fftw`, `imagemagick`, `poppler`) are omitted entirely from the web distribution; no GPL component identified in this list (not a legal opinion; see risk 17 in docs/STATUS.md).

- **Key Facts Observed in Archive:**
  - `bin/` contains `libde265.dll` (linked dynamically into `libheif.dll`).
  - `libheif.dll` references `libde265` (`strings libheif.dll` confirms `libde265`).
  - `bin/` contains **no** `*x265*.dll` files and no linkage symbols (`libx265`, `x265_encoder_*`, `x265_param_*`).
  - `heifload` in this package decodes BOTH AVIF/AV1 (via `libaom`) AND HEIC/HEVC (via `libde265`). Full HEIC/HEVC decoding is verified on Windows.
  - `librsvg-2-2.dll` **is** present in the archive, which makes runtime blocking strictly mandatory (`VIPS_BLOCK_UNTRUSTED=1` blocks it).
  - `libimagequant` is version 2.4.1 (the BSD-2-Clause C library, not the GPL-3.0 Rust rewrite).
  - ImageMagick (`magickload`) is omitted entirely from the web build.

---

### 4.3 macOS Bundled Dynamic Libraries Audit & GPL Findings

All 67 dynamic library files bundled into `Photos Manager.app/Contents/Frameworks/` from the host environment were audited against Homebrew formula metadata (`brew info --json=v2`). The table row count matches the exact file count in `Contents/Frameworks/` (verified automatically by `scripts/check-dylib-table.mjs` in CI):

| Library (`.dylib`) | Homebrew Formula | Formula Version | License | Copyleft Risk / Distribution Status |
|---|---|---|---|---|
| `libIex-3_5.34.dylib` | `openexr` | `3.5.1` | `BSD-3-Clause` | Permissive |
| `libIlmThread-3_5.34.dylib` | `openexr` | `3.5.1` | `BSD-3-Clause` | Permissive |
| `libImath-3_2.30.dylib` | `imath` | `3.2.3` | `BSD-3-Clause` | Permissive |
| `libOpenEXR-3_5.34.dylib` | `openexr` | `3.5.1` | `BSD-3-Clause` | Permissive |
| `libOpenEXRCore-3_5.34.dylib` | `openexr` | `3.5.1` | `BSD-3-Clause` | Permissive |
| `libX11.6.dylib` | `libx11` | `1.8.13` | `MIT` | Permissive |
| `libXau.6.dylib` | `libxau` | `1.0.12` | `MIT` | Permissive |
| `libXdmcp.6.dylib` | `libxdmcp` | `1.1.5` | `MIT` | Permissive |
| `libXext.6.dylib` | `libxext` | `1.3.7` | `MIT` | Permissive |
| `libXrender.1.dylib` | `libxrender` | `0.9.12` | `MIT` | Permissive |
| `libaec.0.dylib` | `libaec` | `1.1.7` | `BSD-2-Clause` | Permissive |
| `libarchive.13.dylib` | `libarchive` | `3.8.9` | `BSD-2-Clause` | Permissive |
| `libb2.1.dylib` | `libb2` | `0.98.1` | `CC0-1.0` | Public Domain / Permissive |
| `libbrotlicommon.1.dylib` | `brotli` | `1.2.0` | `MIT` | Permissive |
| `libbrotlidec.1.dylib` | `brotli` | `1.2.0` | `MIT` | Permissive |
| `libcairo-gobject.2.dylib` | `cairo` | `1.18.6` | `LGPL-2.1-only OR MPL-1.1` | LGPL (Permitted via dynamic linking) |
| `libcairo.2.dylib` | `cairo` | `1.18.6` | `LGPL-2.1-only OR MPL-1.1` | LGPL (Permitted via dynamic linking) |
| `libcfitsio.10.dylib` | `cfitsio` | `4.7.0` | `CFITSIO` | Permissive |
| `libcgif.0.dylib` | `cgif` | `0.5.4` | `MIT` | Permissive |
| `libdatrie.1.dylib` | `libdatrie` | `0.2.14` | `LGPL-2.1-or-later` | LGPL (Permitted via dynamic linking) |
| `libdeflate.0.dylib` | `libdeflate` | `1.26` | `MIT` | Permissive |
| `libexif.12.dylib` | `libexif` | `0.6.26` | `LGPL-2.1-or-later AND LGPL-2.0-or-later` | LGPL (Both components LGPL; permitted via dynamic linking) |
| `libfftw3.3.dylib` | `fftw` | `3.3.11` | `GPL-2.0-or-later AND BSD-2-Clause` | **COPYLEFT RISK (GPL)** (Homebrew builds GPL FFTW) |
| `libfontconfig.1.dylib` | `fontconfig` | `2.18.3` | `HPND-sell-variant AND Unicode-3.0 AND MIT` | Permissive |
| `libfreetype.6.dylib` | `freetype` | `2.14.3` | `FTL` | Permissive |
| `libfribidi.0.dylib` | `fribidi` | `1.0.17` | `GPL-2.0-or-later AND LGPL-2.1-or-later` | LGPL (`libfribidi` shared library is LGPL-2.1-or-later; GPL applies only to `fribidi` CLI utility) |
| `libgdk_pixbuf-2.0.0.dylib` | `gdk-pixbuf` | `2.44.8` | `LGPL-2.1-or-later` | LGPL (Permitted via dynamic linking) |
| `libgio-2.0.0.dylib` | `glib` | `2.90.0` | `LGPL-2.1-or-later` | LGPL (Permitted via dynamic linking) |
| `libglib-2.0.0.dylib` | `glib` | `2.90.0` | `LGPL-2.1-or-later` | LGPL (Permitted via dynamic linking) |
| `libgmodule-2.0.0.dylib` | `glib` | `2.90.0` | `LGPL-2.1-or-later` | LGPL (Permitted via dynamic linking) |
| `libgobject-2.0.0.dylib` | `glib` | `2.90.0` | `LGPL-2.1-or-later` | LGPL (Permitted via dynamic linking) |
| `libgraphite2.3.dylib` | `graphite2` | `1.3.15` | `MIT OR MPL-2.0 OR LGPL-2.1-or-later OR GPL-2.0-or-later` | Permissive / LGPL (Quadruple-licensed; licensee selects MIT or LGPL) |
| `libharfbuzz.0.dylib` | `harfbuzz` | `14.5.0` | `MIT` | Permissive |
| `libhdf5.320.dylib` | `hdf5` | `2.2.0` | `BSD-3-Clause` | Permissive |
| `libhwy.1.dylib` | `highway` | `1.4.0` | `Apache-2.0 OR BSD-3-Clause` | Permissive |
| `libimagequant.0.4.dylib` | `libimagequant` | `4.4.1` | `GPL-3.0-or-later AND HPND` | **COPYLEFT RISK (GPL)** (Homebrew formula builds Rust v4 under GPLv3) |
| `libintl.8.dylib` | `gettext` | `1.0` | `GPL-3.0-or-later AND LGPL-2.1-or-later` | LGPL (Runtime `libintl` is LGPL-2.1-or-later; GPL applies to developer build tools like `msgfmt`) |
| `libjpeg.62.dylib` | `mozjpeg` | `4.1.5` | `IJG AND Zlib AND BSD-3-Clause` | Permissive |
| `libjpeg.8.dylib` | `jpeg-turbo` | `3.2.0` | `IJG AND Zlib AND BSD-3-Clause` | Permissive |
| `liblcms2.2.dylib` | `little-cms2` | `2.19.1` | `MIT` | Permissive |
| `liblz4.1.dylib` | `lz4` | `1.10.0` | `BSD-2-Clause` | Permissive |
| `liblzma.5.dylib` | `xz` | `5.8.4` | `0BSD AND GPL-2.0-or-later` | Permissive (Runtime `liblzma` is 0BSD public domain; GPL applies to `xz` CLI tools/scripts) |
| `libmatio.14.dylib` | `libmatio` | `1.5.30` | `BSD-2-Clause` | Permissive |
| `libomp.dylib` | `libomp` | `23.1.2` | `Apache-2.0 with LLVM Exception` | Permissive |
| `libopenjp2.7.dylib` | `openjpeg` | `2.5.4` | `BSD-2-Clause` | Permissive |
| `libopenjph.0.32.dylib` | `openjph` | `0.32.0` | `BSD-2-Clause` | Permissive |
| `libpango-1.0.0.dylib` | `pango` | `1.58.2` | `LGPL-2.0-or-later` | LGPL (Permitted via dynamic linking) |
| `libpangocairo-1.0.0.dylib` | `pango` | `1.58.2` | `LGPL-2.0-or-later` | LGPL (Permitted via dynamic linking) |
| `libpangoft2-1.0.0.dylib` | `pango` | `1.58.2` | `LGPL-2.0-or-later` | LGPL (Permitted via dynamic linking) |
| `libpcre2-8.0.dylib` | `pcre2` | `10.49` | `BSD-3-Clause` | Permissive |
| `libpixman-1.0.dylib` | `pixman` | `0.46.4` | `MIT` | Permissive |
| `libpng16.16.dylib` | `libpng` | `1.6.58` | `libpng-2.0` | Permissive |
| `libraw_r.25.dylib` | `libraw` | `0.22.2` | `LGPL-2.1-only OR CDDL-1.0` | LGPL (Permitted via dynamic linking) |
| `librsvg-2.2.dylib` | `librsvg` | `2.63.2` | `LGPL-2.1-or-later` | LGPL (Permitted via dynamic linking; blocked at startup) |
| `libsharpyuv.0.dylib` | `webp` | `1.6.0` | `BSD-3-Clause` | Permissive |
| `libsz.2.dylib` | `libaec` | `1.1.7` | `BSD-2-Clause` | Permissive |
| `libthai.0.dylib` | `libthai` | `0.1.30` | `LGPL-2.1-or-later` | LGPL (Permitted via dynamic linking) |
| `libtiff.6.dylib` | `libtiff` | `4.7.2` | `libtiff` | Permissive |
| `libuhdr.2.dylib` | `libultrahdr` | `2.0.2` | `Apache-2.0` | Permissive |
| `libvips.42.dylib` | `vips` | `8.18.7` | `LGPL-2.1-or-later` | LGPL (Permitted via dynamic linking) |
| `libwebp.7.dylib` | `webp` | `1.6.0` | `BSD-3-Clause` | Permissive |
| `libwebpdemux.2.dylib` | `webp` | `1.6.0` | `BSD-3-Clause` | Permissive |
| `libwebpmux.3.dylib` | `webp` | `1.6.0` | `BSD-3-Clause` | Permissive |
| `libxcb-render.0.dylib` | `libxcb` | `1.17.0` | `MIT` | Permissive |
| `libxcb-shm.0.dylib` | `libxcb` | `1.17.0` | `MIT` | Permissive |
| `libxcb.1.dylib` | `libxcb` | `1.17.0` | `MIT` | Permissive |
| `libzstd.1.dylib` | `zstd` | `1.5.7_1` | `(BSD-3-Clause OR GPL-2.0-only) AND BSD-2-Clause AND MIT` | Permissive (Dual-licensed; licensee selects BSD-3-Clause) |

> [!WARNING]
> **GPL Copyleft Finding in Stock Homebrew Distribution:**
> The package audit identified two libraries under strict GPL copyleft licenses pulled in by Homebrew's default `vips` formula:
> 1. `libfftw3.3.dylib` (`GPL-2.0-or-later`): Fast Fourier Transform library used for image sharpening and frequency analysis.
> 2. `libimagequant.0.4.dylib` (`GPL-3.0-or-later`): Palette quantization library for GIF and PNG8.
>
> Distributing an application containing GPL-licensed libraries requires releasing the entire application source code under GPL terms. While acceptable during local developer testing and CI smoke testing, **these libraries cannot be distributed in production**.
>
> **Actionable Resolution:** Before Phase 2, the project must produce its own minimal libvips build for macOS using Meson with `-Dfftw=disabled -Dimagequant=disabled -Dintrospection=disabled -Dmagick=disabled -Drsvg=disabled`, ensuring an entirely LGPL/permissive runtime identical in architecture to the Windows MXE build.

> [!NOTE]
> **macOS `heifload` Status & Homebrew Modular Loaders:**
> Homebrew's libvips formula splits `libheif`, `imagemagick`, `libjxl`, `openslide`, and `poppler` into separate dynamically-loaded modules located in `/opt/homebrew/lib/vips-modules-8.18/` (e.g. `vips-heif.dylib`). Because `vips-heif.dylib` is an external runtime module and not directly linked into `libvips.42.dylib`, `package-bundle.mjs` does not copy it into `Contents/Frameworks/`. In clean-machine smoke testing with Homebrew hidden (and on end-user Macs without Homebrew), libvips loads only its built-in core loaders (`jpegload`, `pngload`, `webpload`, `tiffload`, `gifload`). Therefore, `heifload` is absent in the packaged macOS application, and HEIC/HEVC decoding is **not verified on either OS**.

---

### 4.4 Untrusted Loaders Blocking (`svgload` and `magickload`)

Per TASK-0C Known Issue 2 and Review Finding 3, `svgload` (librsvg) and `magickload` (ImageMagick) present substantial attack surfaces that must never execute in production.

- **Startup Blocking Mechanism:** In `src-tauri/src/image_pipeline/vips.rs`, libvips initialization configures blocking for untrusted operations:
  1. Sets environment variable `VIPS_BLOCK_UNTRUSTED=1`.
  2. Calls C API `vips_block_untrusted_set(1)` on startup.
  3. Explicitly calls `vips_operation_block_set("svgload", 1)` and `vips_operation_block_set("magickload", 1)`.
- **Validation Probe & Smoke JSON Clarification:**
  - `has_svgload: false` in the smoke test JSON is a static constant (`is_loader_supported` hardcodes `false` for svgload and magickload before querying libvips). It proves that application code refuses to dispatch to svgload, but **does not** prove that libvips's own internal loader was unregistered.
  - The active probe inside the smoke test attempts to decode `tests/fixtures/generated/fixture_blocked.svg` via `libvips`. It requires that libvips returns an error ("is not a known file format"), proving that this SVG cannot be decoded at runtime.
- **Stock CLI Control Verification (macOS & Windows CI):**
  To distinguish "blocked by security policy" from "loader absent":
  1. **Unblocked Control:** Running stock CLI `vips copy tests/fixtures/generated/fixture_blocked.svg <out>.png` without `VIPS_BLOCK_UNTRUSTED` succeeds (exit code 0), proving that librsvg is present and functional in the distribution.
  2. **Blocked Control:** Running `VIPS_BLOCK_UNTRUSTED=1 vips copy tests/fixtures/generated/fixture_blocked.svg <out>.png` fails with `"VipsForeignLoad: ... is not a known file format"` (exit code 1).
  This control experiment proves that setting the environment variable `VIPS_BLOCK_UNTRUSTED=1` alone is sufficient for libvips to block untrusted SVG loaders. The subsequent in-code calls to `vips_block_untrusted_set(1)` and `vips_operation_block_set` are **untested redundancy**.

---

### 4.5 ONNX Runtime Packaging (EmbeddingGemma 2 local runtime)

Per the TASK-0B spike recommendation (`docs/embedding-spike.md`):
- **Crate Version Pin:** `ort = "=2.0.0-rc.15"` pinned in `Cargo.toml`.
- **Paired Native Library:** ONNX Runtime `v1.20.1` (MIT License).
- **Bundling Location on macOS arm64:** `libonnxruntime.1.20.1.dylib` placed in `Contents/Frameworks/` and linked via `@rpath`.
- **Bundling Location on Windows x64:** `onnxruntime.dll` placed adjacent to `photos-manager.exe`.
- **Status:** Bundling steps documented. Implementation deferred to Phase 4B per spec §18 delivery plan.

---

## 5. ExifTool Packaging & macOS System Perl Risk

### 5.1 Architecture & Invocation
- Invoked strictly as an isolated child process using `tokio::process::Command` without shell wrappers.
- Unicode and long paths handled by piping arguments over stdin (`-@ -`) with `-charset utf8 -charset filename=utf8 -api WindowsLongPath=1`.
- Bound with a 5-second execution timeout and 512 KB stdout limit.

### 5.2 macOS System Perl Analysis & Architectural Risk
- **System Perl Status:** On macOS 27.0.0 (Apple Silicon, Darwin 27.0.0):
  - Binary path: `/usr/bin/perl`
  - Permissions: `-rwxr-xr-x 1 root wheel 144560 Jan 1 2020 /usr/bin/perl`
  - Version: `perl 5, version 34, subversion 1 (v5.34.1)`
- **Bundled ExifTool Execution:** The pure-Perl script `exiftool` starts with `#!/usr/bin/perl` and executes using macOS system Perl.
- **Identified Architectural Risk:**
  Apple deprecated built-in scripting language runtimes (Perl, Python 2.7, Ruby) in macOS 10.15 Catalina and removed Python 2 in macOS 12.3 Monterey. If Apple removes `/usr/bin/perl` in future macOS releases (e.g. macOS 28+), ExifTool will fail to execute unless:
  1. A minimal standalone Perl interpreter is bundled alongside ExifTool (analogous to the Strawberry Perl runtime in Windows `exiftool_files/`), or
  2. Metadata extraction is transitioned to a pure-Rust parser (e.g. `kamadak-exif` or in-tree parser) for core EXIF/IPTC/XMP extraction.

---

## 6. libvips Windows Binding Fork Audit

| Field | Value |
|---|---|
| **Fork Repository** | `https://github.com/kuma-00/libvips-rust-bindings-for-windows.git` |
| **Pinned Commit** | `80097ad9387180f678b21482b8b0cfbfb436a308` |
| **Crate Version** | `libvips 1.7.2` (in `src-tauri/Cargo.lock`) |
| **License** | MIT License (matches upstream `libvips` crate) |
| **Date of Last Commit** | July 20, 2025 |
| **Diffstat vs Upstream Base (`1ca5b35`)** | `build.rs        \| 13 +++++++++++++`<br>`src/bindings.rs \| 19 +++++++++++++------`<br>`src/image.rs    \| 10 +++++-----`<br>`src/manual.rs   \|  4 ++--`<br>`src/utils.rs    \|  3 ++-`<br>`5 files changed, 35 insertions(+), 14 deletions(-)` |
| **Changes Applied** | Replaced `c_ulong`/`c_long` with `c_ulonglong`/`c_longlong` across FFI signatures to fix 64-bit integer sizing under Windows LLP64; added Windows link search paths in `build.rs`; commented out missing symbol `vips_target_finish`. |
| **Scope of Application** | Applied workspace-wide via `[patch.crates-io]` in `src-tauri/Cargo.toml`. Cargo patches apply unconditionally across every OS. On LP64 systems (macOS/Linux) `c_ulonglong` and `c_ulong` are identical 64-bit types (`u64`), compiling cleanly on macOS as well. |
| **Future Alternatives** | 1. Upstream PR to `olxgroup-oss/libvips-rust-bindings`.<br>2. Custom in-tree `bindgen` generating clean FFI bindings for only the ~5 libvips methods required by `ImageProcessor`. |

---

## 7. Phase 7 Signing & Notarization Architecture (Specification)

While unsigned builds are produced during Phase 0–6 for developer testing, distribution packages in Phase 7 require:

### 7.1 macOS Requirements:
1. **Apple Developer ID Application Certificate:** Enrolled in Apple Developer Program.
2. **Hardened Runtime:** Compiled with `--options runtime` enabled.
3. **Entitlements (`entitlements.plist`):** Least privilege; no JIT or unsigned memory execution required.
4. **Notarization Ticket:** Submitted to Apple notary service via `xcrun notarytool submit --keychain-profile ... --wait`.
5. **Stapling:** `xcrun stapler staple "Photos Manager.app"`.

### 7.2 Windows Requirements:
1. **Authenticode Code Signing Certificate:** Standard or EV certificate issued by an accredited CA.
2. **Timestamping:** Sign using `signtool.exe sign /fd SHA256 /tr http://timestamp.digicert.com /td SHA256`.
3. **SmartScreen Reputation:** Building reputation or EV hardware token to avoid Windows Defender SmartScreen blocks.
4. **WebView2 Runtime:** Windows 10/11 includes the evergreen WebView2 runtime by default; installer includes fallback check.

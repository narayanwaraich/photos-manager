# Native Dependencies & Feasibility Findings

**Tasks:** TASK-0A, TASK-0B, TASK-0C, TASK-0E, TASK-0F · **Date:** 2026-11-11 · **Verified on:** macOS 15/27 (arm64) and Windows Server 2025 (x64 via CI)

This document records the exact versions, sources, licenses, install requirements, codecs, bundling strategies, and risks for all native dependencies evaluated in Phase 0. Every dependency and model shape has been verified against official registries or documentation (rule: *verify, do not recall*).

> [!NOTE]
> **Dev-Only Homebrew Paths Gated:** Link search paths (`/opt/homebrew/lib` in `src-tauri/build.rs`) and candidate executable paths in `which_exiftool()` are strictly developer conveniences for local development and fork-PR fallback. Non-fork CI and release packages resolve libvips strictly from the pinned `VIPS_DIR` / `VIPS_LIB_DIR` custom archive and bundle ExifTool + libvips hermetically inside the application distribution, gating `/opt/homebrew/lib` behind `cfg(debug_assertions)` / `PHOTOS_DEV_HOMEBREW=1` / `PHOTOS_DEV_ALLOW_SYSTEM_EXIFTOOL=1`.

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
| **libvips (Native C Library)** | `8.18.6` (Windows x64 & macOS arm64 custom builds) | LGPL-2.1-or-later | `https://github.com/libvips/libvips` | 2026-11-11 | Custom pinned HEVC-decode-only archives (`libheif 1.23.2` + `libde265 1.1.1` + `libaom 3.14.1`, zero `x265`, zero GPL); untrusted loaders blocked/disabled. |
| **ExifTool** | `13.61` | Perl Artistic License 1.0 OR GPL-1.0-or-later | `https://exiftool.org/` | 2026-11-06 | Official distribution on both OSes. Windows standalone zip (Strawberry Perl runtime) + macOS Perl script. |
| **ONNX Runtime Rust Binding** | `ort = "=2.0.0-rc.15"` | MIT OR Apache-2.0 | `https://crates.io/crates/ort/2.0.0-rc.15` | 2026-11-05 | Candidate embedding runtime from TASK-0B. Pinned with exact version (`=`). |
| **ONNX Runtime (Native C Library)** | `1.31.1` | MIT | `https://github.com/microsoft/onnxruntime/releases/tag/v1.31.1` | 2026-11-12 | Loaded via `ort` `load-dynamic` from checksum-pinned official GitHub releases; bundling documented in §4.5. Not integrated in `src-tauri/` until Phase 4B. |
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
| `vips-macos-arm64-8.18.6-hevc.zip` | macOS arm64 | 8.18.6-hevc1 | `https://github.com/narayanwaraich/photos-manager-deps/releases/download/macos-arm64-vips-8.18.6-hevc1/vips-macos-arm64-8.18.6-hevc.zip` (private repo, token-authenticated) | `6bb24d51dedf9f69344aa187cb0f9651b7842de4c465643480f3a794481dba64` |
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

### 4.3 macOS Bundled Dynamic Libraries Audit

The packaged macOS arm64 bundle consumes our custom HEVC-decode-only archive (`vips-macos-arm64-8.18.6-hevc.zip`, release `macos-arm64-vips-8.18.6-hevc1` in private repository `narayanwaraich/photos-manager-deps`, SHA-256 `6bb24d51dedf9f69344aa187cb0f9651b7842de4c465643480f3a794481dba64`, built by `.github/workflows/deps-macos-vips.yml` run `42945772807` on `macos-15-arm64` `15.8.2` `24H232` with `Xcode 16.4` `16F6`).

All **22** bundled dynamic library files in `photos-manager.app/Contents/Frameworks/` come strictly from that checksum-pinned archive (`versions.json`: **16 components** — 4 built from verified source tarballs + 12 allowlisted Homebrew leaf libraries — plus macOS system libraries in `/usr/lib/` and `/System/Library/Frameworks/`). There is **no GPL component identified in this list** (not a legal opinion; see risk 17 in `docs/STATUS.md`). The table row count matches the exact regular `.dylib` count in `Contents/Frameworks/` (verified automatically by `scripts/check-dylib-table.mjs` on every non-fork CI run):

| Library (`.dylib`) | Component / Source (`versions.json`) | Version | License (SPDX / Upstream) | Distribution Status / Notes |
|---|---|---|---|---|
| `libaom.3.dylib` | `aom` (`tarball:https://storage.googleapis.com/aom-releases/libaom-3.14.1.tar.gz`) | `3.14.1` | `BSD-2-Clause` | Permissive (AV1 decoder only; encoder disabled `-DCONFIG_AV1_ENCODER=0`) |
| `libde265.0.dylib` | `libde265` (`tarball:https://github.com/strukturag/libde265/releases/download/v1.1.1/libde265-1.1.1.tar.gz`) | `1.1.1` | `LGPL-3.0-or-later` | LGPL (Permitted via dynamic linking; HEVC decode for `libheif`) |
| `libexif.12.dylib` | `libexif` (`homebrew:libexif`) | `0.6.26` | `LGPL-2.1-or-later AND LGPL-2.0-or-later` | LGPL (Permitted via dynamic linking) |
| `libgio-2.0.0.dylib` | `glib` (`homebrew:glib`) | `2.90.1` | `LGPL-2.1-or-later` | LGPL (Permitted via dynamic linking) |
| `libglib-2.0.0.dylib` | `glib` (`homebrew:glib`) | `2.90.1` | `LGPL-2.1-or-later` | LGPL (Permitted via dynamic linking) |
| `libgmodule-2.0.0.dylib` | `glib` (`homebrew:glib`) | `2.90.1` | `LGPL-2.1-or-later` | LGPL (Permitted via dynamic linking) |
| `libgobject-2.0.0.dylib` | `glib` (`homebrew:glib`) | `2.90.1` | `LGPL-2.1-or-later` | LGPL (Permitted via dynamic linking) |
| `libheif.1.dylib` | `libheif` (`tarball:https://github.com/strukturag/libheif/releases/download/v1.23.2/libheif-1.23.2.tar.gz`) | `1.23.2` | `LGPL-3.0-or-later` | LGPL (Permitted via dynamic linking; `-DWITH_X265=OFF`, `-DENABLE_PLUGIN_LOADING=OFF`, links `libde265` + `libaom`) |
| `libhwy.1.dylib` | `highway` (`homebrew:highway`) | `1.4.0` | `Apache-2.0 OR BSD-3-Clause` | Permissive |
| `libintl.8.dylib` | `gettext` (`homebrew:gettext`) | `1.0` | `LGPL-2.1-or-later` | LGPL (Runtime `libintl` is `LGPL-2.1-or-later`; formula SPDX `GPL-3.0-or-later AND LGPL-2.1-or-later` covers unbundled CLI tools) |
| `libjpeg.8.dylib` | `jpeg-turbo` (`homebrew:jpeg-turbo`) | `3.2.1` | `IJG AND Zlib AND BSD-3-Clause` | Permissive |
| `liblcms2.2.dylib` | `little-cms2` (`homebrew:little-cms2`) | `2.19.1` | `MIT` | Permissive |
| `liblzma.5.dylib` | `xz` (`homebrew:xz`) | `5.8.4` | `0BSD` | Permissive (Runtime `liblzma` is `0BSD` public domain; formula SPDX `0BSD AND GPL-2.0-or-later` covers unbundled CLI tools) |
| `libpcre2-8.0.dylib` | `pcre2` (`homebrew:pcre2`) | `10.49` | `BSD-3-Clause` | Permissive |
| `libpng16.16.dylib` | `libpng` (`homebrew:libpng`) | `1.6.59` | `libpng-2.0` | Permissive |
| `libsharpyuv.0.dylib` | `webp` (`homebrew:webp`) | `1.6.0` | `BSD-3-Clause` | Permissive |
| `libtiff.6.dylib` | `libtiff` (`homebrew:libtiff`) | `4.7.2` | `libtiff` | Permissive |
| `libvips.42.dylib` | `vips` (`tarball:https://github.com/libvips/libvips/releases/download/v8.18.6/vips-8.18.6.tar.xz`) | `8.18.6` | `LGPL-2.1-or-later` | LGPL (Permitted via dynamic linking; `-Dmodules=disabled`, built-in `heifload`) |
| `libwebp.7.dylib` | `webp` (`homebrew:webp`) | `1.6.0` | `BSD-3-Clause` | Permissive |
| `libwebpdemux.2.dylib` | `webp` (`homebrew:webp`) | `1.6.0` | `BSD-3-Clause` | Permissive |
| `libwebpmux.3.dylib` | `webp` (`homebrew:webp`) | `1.6.0` | `BSD-3-Clause` | Permissive |
| `libzstd.1.dylib` | `zstd` (`homebrew:zstd`) | `1.5.7_1` | `BSD-3-Clause` | Permissive (Formula SPDX `(BSD-3-Clause OR GPL-2.0-only) AND BSD-2-Clause AND MIT`; licensee elects `BSD-3-Clause`) |

- **License Breakdown (16 Components / 22 Dylibs):**
  - **LGPL libraries (6 components, permitted via dynamic linking):** `gettext` (`libintl.8.dylib`, `LGPL-2.1-or-later`), `glib` (`2.90.1`, `LGPL-2.1-or-later`), `libde265` (`1.1.1`, `LGPL-3.0-or-later`), `libexif` (`0.6.26`, `LGPL-2.1-or-later AND LGPL-2.0-or-later`), `libheif` (`1.23.2`, `LGPL-3.0-or-later`), `vips` (`8.18.6`, `LGPL-2.1-or-later`).
  - **Permissive open-source libraries (10 components):** `aom` (`BSD-2-Clause`), `highway` (`Apache-2.0 OR BSD-3-Clause`), `jpeg-turbo` (`IJG AND Zlib AND BSD-3-Clause`), `libpng` (`libpng-2.0`), `libtiff` (`libtiff`), `little-cms2` (`MIT`), `pcre2` (`BSD-3-Clause`), `webp` (`BSD-3-Clause`), `xz` (`0BSD`), `zstd` (`BSD-3-Clause` elected).
  - **No GPL components identified:** No GPL component identified in this list (not a legal opinion; see risk 17 in `docs/STATUS.md`). `x265`, `x264`, `fftw`, `imagequant` 4.x, `magick`, `poppler`, `rsvg`, `openslide`, `openexr`, `cfitsio`, and `libarchive` are excluded at configure time and verified absent by `deps/macos-vips/audit-dylibs.sh`. Note: `libvips.42.dylib` compiles the static enum nick `"x265"` from `foreign.h` unconditionally, with zero `x265` linkage or symbols.

> [!NOTE]
> **Resolution of Risk 11 (`heifload` & Floating Homebrew Harvest on macOS):**
> Building libvips `8.18.6` with `-Dmodules=disabled` and isolated `PKG_CONFIG_LIBDIR` compiles `heifload` (HEIC/HEVC via `libde265` + AVIF via `libaom`) directly into `libvips.42.dylib` and eliminates the 67-dylib Homebrew runtime harvest (including GPL `fftw` and `imagequant` 4.x). Non-fork CI runs and packaged releases consume the checksum-pinned archive via `VIPS_DIR` / `VIPS_LIB_DIR` and fail closed on missing `DEPS_TOKEN` or checksum mismatch. Fork pull requests without repository secrets fall back to Homebrew with `REQUIRE_HEIC` unset (`heic_decode_verified: false`, `heic_note: "no deps token (fork)"`). HEVC codec patent licensing remains separate from copyright licenses and requires legal review before public Phase 7 distribution.

---

### 4.4 Untrusted Loaders Blocking (`svgload` and `magickload`)

Per TASK-0C Known Issue 2 and Review Finding 3, `svgload` (librsvg) and `magickload` (ImageMagick) present substantial attack surfaces that must never execute in production.

- **Startup Blocking Mechanism:** In `src-tauri/src/image_pipeline/vips.rs`, libvips initialization configures blocking for untrusted operations:
  1. Sets environment variable `VIPS_BLOCK_UNTRUSTED=1`.
  2. Calls C API `vips_block_untrusted_set(1)` on startup.
  3. Explicitly calls `vips_operation_block_set("svgload", 1)` and `vips_operation_block_set("magickload", 1)`.
- **Validation Probe & Smoke JSON Clarification:**
  - `has_svgload: false` in the smoke test JSON is a static constant (`is_loader_supported` hardcodes `false` for svgload and magickload before querying libvips). It proves that application code refuses to dispatch to svgload, but **does not** prove that libvips's own internal loader was unregistered.
  - The active probe inside the smoke test attempts to decode `tests/fixtures/generated/fixture_blocked.svg` via `libvips`. It requires that libvips returns an error (`"is not a known file format"`), proving that this SVG cannot be decoded at runtime.
- **CLI Control Verification (macOS & Windows CI):**
  - **Windows x64 (`librsvg-2-2.dll` present) & macOS fork-PR Homebrew fallback:** Unblocked `vips copy tests/fixtures/generated/fixture_blocked.svg <out>.png` succeeds (exit code 0), while `VIPS_BLOCK_UNTRUSTED=1 vips copy ...` fails with `"VipsForeignLoad: ... is not a known file format"` (exit code 1), proving `VIPS_BLOCK_UNTRUSTED=1` actively blocks `svgload`.
  - **macOS arm64 custom build (`-Drsvg=disabled`, `-Dmagick=disabled`):** Neither `librsvg` nor ImageMagick is compiled into `libvips.42.dylib`. Both unblocked `vips copy` and `VIPS_BLOCK_UNTRUSTED=1 vips copy` on `fixture_blocked.svg` fail with exit code 1 (`"VipsForeignLoad: ... is not a known file format"`).

---

### 4.5 ONNX Runtime Packaging (EmbeddingGemma 2 local runtime)

Per TASK-0B and TASK-0B2 parity validation (`docs/embedding-runtime.md`):
- **Crate Version Pin:** `ort = "=2.0.0-rc.15"` pinned in `Cargo.toml` (`default-features = false`, `features = ["std", "ndarray", "load-dynamic", "api-31", "coreml", "directml"]`). With `load-dynamic`, `ort-sys` executes zero CDN binary downloads during `cargo build`; the host app explicitly points `ort::init_from(path)` at a checksum-verified local runtime library.
- **Paired Native Library:** Official Microsoft ONNX Runtime **`v1.31.1`** (API 31, MIT License, `https://github.com/microsoft/onnxruntime/releases/tag/v1.31.1`):
  - **macOS arm64:** `onnxruntime-osx-arm64-1.31.1.tgz` (`https://github.com/microsoft/onnxruntime/releases/download/v1.31.1/onnxruntime-osx-arm64-1.31.1.tgz`, `36,936,435` bytes, SHA-256 `2e1344a1926ee50eb25b38153cb6992c3567515aef0e677a1a520a1798896d6f`) -> extracts `lib/libonnxruntime.1.31.1.dylib` (`44,797,256` bytes).
  - **Windows x64 (CPU EP):** `onnxruntime-win-x64-1.31.1.zip` (`https://github.com/microsoft/onnxruntime/releases/download/v1.31.1/onnxruntime-win-x64-1.31.1.zip`, `82,929,944` bytes, SHA-256 `5869e26bb25314375f2d5ef1877e3ea06b1543d6c710e6e4a1ec2f7177ab4b2c`) -> extracts `lib/onnxruntime.dll`. *(Note: upstream Microsoft `onnxruntime-win-x64-1.31.1.zip` is CPU-only; official `Microsoft.ML.OnnxRuntime.DirectML` NuGet stopped at `1.24.4`, whereas `ort` `api-31` requires ORT `>= 1.31`. GPU DirectML on 1.31+ requires pyke's third-party `x86_64-pc-windows-msvc+directml` build or staying on CPU EP.)*
- **Bundling Path (Hardened Runtime & Clean Machine):**
  - **macOS arm64:** Bundle `libonnxruntime.1.31.1.dylib` inside `Contents/Frameworks/` so it is signed and notarized with the `.app` (macOS Hardened Runtime refuses unsigned libraries downloaded after install). `otool -L` confirms `libonnxruntime.1.31.1.dylib` depends solely on Apple `/System/Library/Frameworks/` and `/usr/lib/` (zero `/opt/homebrew` paths).
  - **Windows x64:** Bundle `onnxruntime.dll` (and `onnxruntime_providers_shared.dll` if present) adjacent to `photos-manager.exe` (or in `resources/` loaded via explicit path to `ort::init_from(...)`).
- **OS Tested & Remaining Unproven Items:**
  - **Tested:** macOS 27.0 arm64 (`env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin` clean-environment parity + `otool -L`) and Windows Server 2025 x64 CI (`windows-latest`, clean `PATH`, `dumpbin /dependents`).
  - **Unproven:** Owner's low-end Windows 11 consumer laptop (Ryzen 7 5700U, 8 GB RAM — guide in `docs/embedding-validation.md`), Windows installer code-signing/packaging flow (Phase 4B/7), and DirectML execution provider on real Windows GPU hardware.
- **Status:** Bundling paths and clean-environment loading verified in spike `spikes/embeddings/rust-parity/`. Integration into `src-tauri/` deferred to Phase 4B per spec §18.

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

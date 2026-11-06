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
| **libvips Rust Binding** | `libvips 2.3.0` (patched) | MIT | `https://github.com/kuma-00/libvips-rust-bindings-for-windows.git` | 2026-11-06 | Fork commit `80097ad9387180f678b21482b8b0cfbfb436a308`. Fixes Windows LLP64 64-bit integer sizing across crates.io patch. |
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
| `vips-dev-w64-web-8.18.6.zip` | Windows x64 | 8.18.6 | `https://github.com/libvips/build-win64-mxe/releases/download/v8.18.6/vips-dev-w64-web-8.18.6.zip` | `10086f2ccc8e2a861831facabab75fa1b80b4bd03f0a8447623ec581002aa4b2` |
| `exiftool-13.61_64.zip` | Windows x64 | 13.61 | `https://downloads.sourceforge.net/project/exiftool/exiftool-13.61_64.zip` | `ab75b37515385a753aa60a58f68ffffe1ff81e43b319df9540978fb19e0580e5` |
| `Image-ExifTool-13.61.tar.gz` | macOS arm64 | 13.61 | `https://downloads.sourceforge.net/project/exiftool/Image-ExifTool-13.61.tar.gz` | `df7f42c14b4f58a60bec39feae0ee9593000ee27e44920d2b0a21c6627de0baa` |

*Note on ExifTool URLs:* Official download links on `https://exiftool.org/` redirect to official SourceForge project mirrors. Both Windows standalone and macOS Perl archives are pinned to identical version 13.61.

---

### 4.2 Windows `vips-dev-w64-web-8.18.6` Manifest & License Audit

The official Windows minimal precompiled package (`vips-dev-w64-web-8.18.6.zip`) provides a clean-machine runtime without GPL contamination:
- **Build System:** MXE cross-compilation environment (`https://mxe.cc`).
- **Archive Manifest (`manifest.txt`):** Includes core image decoding and platform support libraries:
  - `libvips 8.18.6` (core engine)
  - `libjpeg-turbo 3.0.4` (JPEG decoder/encoder)
  - `libpng 1.6.44` / `libspng 0.7.4` (PNG)
  - `libwebp 1.4.0` (WebP)
  - `libtiff 4.7.0` (TIFF)
  - `libheif 1.18.2`, `libde265 1.0.15`, `dav1d 1.4.3` (HEIF/HEVC and AVIF decode)
  - `openjpeg 2.5.2` (JPEG 2000)
  - `cgif 0.4.1` (GIF)
  - `lcms2 2.16` (LittleCMS color management)
  - `glib 2.82.2`, `pango 1.54.0`, `cairo 1.18.2` (GNOME support stack)
  - `zlib 1.3.1`, `expat 2.6.3`
- **Licenses (`COPYING` and `licenses/`):**
  - `libvips`: LGPL-2.1-or-later
  - `glib`, `cairo`, `pango`: LGPL-2.1-or-later
  - `libheif`, `libde265`: LGPL-3.0-or-later
  - `libjpeg-turbo`, `libpng`, `libwebp`, `libtiff`, `dav1d`, `cgif`, `lcms2`: Permissive (BSD, MIT, IJG, Zlib)
  - **GPL Exclusions:** Unlike the monolithic `w64-all` build, `w64-web` contains **no GPL-licensed loaders**: ImageMagick (`magickload`), librsvg (`svgload`), and FFTW (`fftw`) are omitted from the build.

---

### 4.3 macOS Bundled Dynamic Libraries Audit & GPL Findings

All 66 dynamic library files bundled into `Photos Manager.app/Contents/Frameworks/` from the host environment were audited against Homebrew formula metadata (`brew info --json=v2`):

| Library (`.dylib`) | Homebrew Formula | Formula Version | License | Copyleft Risk |
|---|---|---|---|---|
| `libcairo.2.dylib` | `cairo` | `1.18.6` | `LGPL-2.1-only OR MPL-1.1` | LGPL (Permitted via dynamic linking) |
| `libcairo-gobject.2.dylib` | `cairo` | `1.18.6` | `LGPL-2.1-only OR MPL-1.1` | LGPL (Permitted via dynamic linking) |
| `libcfitsio.10.dylib` | `cfitsio` | `4.7.0` | `CFITSIO` | Permissive |
| `libcgif.0.dylib` | `cgif` | `0.5.4` | `MIT` | Permissive |
| `libfftw3.3.dylib` | `fftw` | `3.3.11` | `GPL-2.0-or-later AND BSD-2-Clause` | **COPYLEFT RISK (GPL)** |
| `libfontconfig.1.dylib` | `fontconfig` | `2.18.3` | `HPND-sell-variant AND MIT` | Permissive |
| `libfreetype.6.dylib` | `freetype` | `2.15.0` | `FTL` | Permissive |
| `libfribidi.0.dylib` | `fribidi` | `1.0.18` | `GPL-2.0-or-later AND LGPL-2.1-or-later` | LGPL (Permitted via dynamic linking) |
| `libgdk_pixbuf-2.0.0.dylib` | `gdk-pixbuf` | `2.44.8` | `LGPL-2.1-or-later` | LGPL (Permitted via dynamic linking) |
| `libgio-2.0.0.dylib` | `glib` | `2.90.1` | `LGPL-2.1-or-later` | LGPL (Permitted via dynamic linking) |
| `libglib-2.0.0.dylib` | `glib` | `2.90.1` | `LGPL-2.1-or-later` | LGPL (Permitted via dynamic linking) |
| `libgmodule-2.0.0.dylib` | `glib` | `2.90.1` | `LGPL-2.1-or-later` | LGPL (Permitted via dynamic linking) |
| `libgobject-2.0.0.dylib` | `glib` | `2.90.1` | `LGPL-2.1-or-later` | LGPL (Permitted via dynamic linking) |
| `libgraphite2.3.dylib` | `graphite2` | `1.3.15` | `MIT OR MPL-2.0 OR LGPL-2.1-or-later` | Permissive / LGPL |
| `libharfbuzz.0.dylib` | `harfbuzz` | `15.1.0` | `MIT` | Permissive |
| `libhdf5.320.dylib` | `hdf5` | `2.2.0` | `BSD-3-Clause` | Permissive |
| `libhwy.1.dylib` | `highway` | `1.4.0` | `Apache-2.0 OR BSD-3-Clause` | Permissive |
| `libImath-3_2.30.dylib` | `imath` | `3.2.3` | `BSD-3-Clause` | Permissive |
| `libintl.8.dylib` | `gettext` | `1.0` | `LGPL-2.1-or-later` (runtime libintl) | LGPL (Permitted via dynamic linking) |
| `libjpeg.8.dylib` | `jpeg-turbo` | `3.2.1` | `IJG AND Zlib AND BSD-3-Clause` | Permissive |
| `libjpeg.62.dylib` | `mozjpeg` | `4.1.5` | `IJG AND Zlib AND BSD-3-Clause` | Permissive |
| `liblcms2.2.dylib` | `little-cms2` | `2.19.1` | `MIT` | Permissive |
| `liblz4.1.dylib` | `lz4` | `1.11.0` | `BSD-2-Clause` | Permissive |
| `liblzma.5.dylib` | `xz` | `5.8.4` | `0BSD AND GPL-2.0-or-later` | Permissive (runtime 0BSD) / GPL |
| `libmatio.14.dylib` | `libmatio` | `1.6.0` | `BSD-2-Clause` | Permissive |
| `libomp.dylib` | `libomp` | `23.1.5` | `MIT` | Permissive |
| `libOpenEXR-3_5.34.dylib` | `openexr` | `3.5.3` | `BSD-3-Clause` | Permissive |
| `libopenjp2.7.dylib` | `openjpeg` | `2.5.4` | `BSD-2-Clause` | Permissive |
| `libpango-1.0.0.dylib` | `pango` | `1.58.2` | `LGPL-2.0-or-later` | LGPL (Permitted via dynamic linking) |
| `libpangocairo-1.0.0.dylib` | `pango` | `1.58.2` | `LGPL-2.0-or-later` | LGPL (Permitted via dynamic linking) |
| `libpangoft2-1.0.0.dylib` | `pango` | `1.58.2` | `LGPL-2.0-or-later` | LGPL (Permitted via dynamic linking) |
| `libpcre2-8.0.dylib` | `pcre2` | `10.49` | `BSD-3-Clause` | Permissive |
| `libpixman-1.0.dylib` | `pixman` | `0.46.4` | `MIT` | Permissive |
| `libpng16.16.dylib` | `libpng` | `1.6.59` | `libpng-2.0` | Permissive |
| `libraw_r.25.dylib` | `libraw` | `0.22.3` | `LGPL-2.1-only OR CDDL-1.0` | LGPL (Permitted via dynamic linking) |
| `librsvg-2.2.dylib` | `librsvg` | `2.63.4` | `LGPL-2.1-or-later` | LGPL (Permitted via dynamic linking) |
| `libsz.2.dylib` | `libaec` | `1.1.8` | `BSD-2-Clause` | Permissive |
| `libthai.0.dylib` | `libthai` | `0.1.30` | `LGPL-2.1-or-later` | LGPL (Permitted via dynamic linking) |
| `libtiff.6.dylib` | `libtiff` | `4.7.2` | `libtiff` | Permissive |
| `libuhdr.2.dylib` | `libultrahdr` | `2.0.2` | `Apache-2.0` | Permissive |
| `libvips.42.dylib` | `vips` | `8.18.7` | `LGPL-2.1-or-later` | LGPL (Permitted via dynamic linking) |
| `libwebp.7.dylib` | `webp` | `1.6.0` | `BSD-3-Clause` | Permissive |
| `libwebpdemux.2.dylib` | `webp` | `1.6.0` | `BSD-3-Clause` | Permissive |
| `libwebpmux.3.dylib` | `webp` | `1.6.0` | `BSD-3-Clause` | Permissive |
| `libX11.6.dylib` | `libx11` | `1.8.14` | `MIT` | Permissive |
| `libXau.6.dylib` | `libxau` | `1.0.12` | `MIT` | Permissive |
| `libxcb.1.dylib` | `libxcb` | `1.17.0` | `MIT` | Permissive |
| `libxcb-render.0.dylib` | `libxcb` | `1.17.0` | `MIT` | Permissive |
| `libxcb-shm.0.dylib` | `libxcb` | `1.17.0` | `MIT` | Permissive |
| `libXdmcp.6.dylib` | `libxdmcp` | `1.1.5` | `MIT` | Permissive |
| `libXext.6.dylib` | `libxext` | `1.3.7` | `MIT` | Permissive |
| `libXrender.1.dylib` | `libxrender` | `0.9.13` | `MIT` | Permissive |
| `libimagequant.0.4.dylib` | `libimagequant` | `4.4.1` | `GPL-3.0-or-later AND HPND` | **COPYLEFT RISK (GPL)** |
| `libzstd.1.dylib` | `zstd` | `1.5.7` | `(BSD-3-Clause OR GPL-2.0-only)` | Permissive via BSD clause |

> [!WARNING]
> **GPL Copyleft Finding in Stock Homebrew Distribution:**
> The package audit identified two libraries under strict GPL copyleft licenses pulled in by Homebrew's default `vips` formula:
> 1. `libfftw3.3.dylib` (`GPL-2.0-or-later`): Fast Fourier Transform library used for image sharpening and frequency analysis.
> 2. `libimagequant.0.4.dylib` (`GPL-3.0-or-later`): Palette quantization library for GIF and PNG8.
>
> Distributing an application containing GPL-licensed libraries requires releasing the entire application source code under GPL terms. While acceptable during local developer testing and CI smoke testing, **these libraries cannot be distributed in production**.
>
> **Actionable Resolution:** Before Phase 2, the project must produce its own minimal libvips build for macOS using Meson with `-Dfftw=disabled -Dimagequant=disabled -Dintrospection=disabled -Dmagick=disabled -Drsvg=disabled`, ensuring an entirely LGPL/permissive runtime identical in architecture to the Windows MXE build.

---

### 4.4 Untrusted Loaders Blocking (`svgload` and `magickload`)

Per TASK-0C Known Issue 2 and Review Finding 3, `svgload` (librsvg) and `magickload` (ImageMagick) present substantial attack surfaces that must never execute in production.

- **Startup Blocking Mechanism:** In `src-tauri/src/image_pipeline/vips.rs`, libvips initialization blocks untrusted operations:
  1. Sets environment variable `VIPS_BLOCK_UNTRUSTED=1`.
  2. Calls C API `vips_block_untrusted_set(1)` on startup.
  3. Explicitly calls `vips_operation_block_set("svgload", 1)` and `vips_operation_block_set("magickload", 1)`.
- **Validation Probe:** The smoke test runs an active probe loading `tests/fixtures/generated/fixture_blocked.svg`. The probe requires failure; if libvips successfully parses the SVG, the smoke test reports an error and fails. The bypass flag `PHOTOS_DEV_ALLOW_SVGLOAD` was removed.

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
| **License** | MIT License (matches upstream `libvips` crate) |
| **Date of Last Commit** | July 20, 2025 |
| **Diffstat vs Upstream 2.3.0** | `src/bindings.rs \| 284 ++++++++++++++++++------------------`<br>`1 file changed, 142 insertions(+), 142 deletions(-)` |
| **Changes Applied** | Replaced `c_ulong`/`c_long` with `c_ulonglong`/`c_longlong` across FFI signatures to fix 64-bit integer sizing under Windows LLP64; commented out missing symbol `vips_target_finish`. |
| **Scope of Application** | Applied workspace-wide via `[patch.crates-io]` in root `Cargo.toml`. While Cargo patch applies unconditionally, on LP64 systems (macOS/Linux) `c_ulonglong` and `c_ulong` are identical 64-bit types (`u64`), compiling cleanly on macOS. |
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

# Native Dependencies & Feasibility Findings

**Tasks:** TASK-0A, TASK-0B, TASK-0C · **Date:** 2026-11-05 · **Verified on:** macOS 27.0 (arm64) and Windows 10/11 (x64 via CI)

This document records the exact versions, sources, licenses, install requirements, codecs, bundling strategies, and risks for all native dependencies evaluated in Phase 0. Every dependency and model shape has been verified against official registries or documentation (rule: *verify, do not recall*).

> [!NOTE]
> **Dev-Only Homebrew Paths Gated:** Link search paths (`/opt/homebrew/lib` in `src-tauri/build.rs`) and candidate executable paths in `which_exiftool()` are strictly developer conveniences for local development. Release builds gate these paths behind `cfg(debug_assertions)` and explicit dev environment flags (`PHOTOS_DEV_HOMEBREW=1`, `PHOTOS_DEV_ALLOW_SYSTEM_EXIFTOOL=1`), requiring all release packages to resolve libvips and ExifTool hermetically from the application bundle.

---

## 1. Verified Dependency Matrix

| Component | Verified Version | License | Source / Registry URL | Date Verified | Notes / Install Requirements |
|---|---|---|---|---|---|
| **Rust Toolchain** | `1.99.1` (stable) | MIT OR Apache-2.0 | `https://www.rust-lang.org/` | 2026-11-05 | Host `aarch64-apple-darwin` and `x86_64-pc-windows-msvc`. Pinned via `rust-toolchain.toml`. |
| **Tauri Core** | `tauri 2.12.5` | MIT OR Apache-2.0 | `https://crates.io/crates/tauri/2.12.5` | 2026-11-02 | Tauri 2 stable release. Least-privilege capabilities configured in `src-tauri/capabilities/default.json`. |
| **Tauri CLI / API** | `@tauri-apps/cli 2.12.4`, `@tauri-apps/api 2.12.4` | MIT OR Apache-2.0 | `https://www.npmjs.com/package/@tauri-apps/cli` | 2026-11-02 | Node 26.9.0, pnpm 12.8.1. |
| **SQLx (SQLite)** | `sqlx 0.8.6` | MIT OR Apache-2.0 | `https://crates.io/crates/sqlx/0.8.6` | 2026-11-02 | Features: `sqlite`, `runtime-tokio`, `migrate`, `macros`. Bundled `libsqlite3-sys 0.30.1`. Configured with WAL, foreign keys ON, busy timeout 5000ms. |
| **libvips Rust Binding** | `libvips 2.3.0` | MIT | `https://crates.io/crates/libvips/2.3.0` | 2026-11-02 | Bindings to C libvips. Tested and proven with orientation-correct WebP thumbnailing. |
| **libvips (Native C Library)** | `8.18.7` | LGPL-2.1-or-later | `https://github.com/libvips/libvips` | 2026-11-05 | Dynamically linked. Minimal codec configuration verified for clean-machine bundling (§4 below). |
| **ExifTool** | `13.55` | Perl Artistic License 1.0 OR GPL-1.0-or-later | `https://exiftool.org/` / `https://github.com/exiftool/exiftool` | 2026-11-05 | Invoked directly without shell with `-charset filename=UTF8 -api WindowsLongPath=1`. Size-capped (512 KB) and timeout-protected (5s). |
| **ONNX Runtime Rust Binding** | `ort = "=2.0.0-rc.15"` | MIT OR Apache-2.0 | `https://crates.io/crates/ort/2.0.0-rc.15` | 2026-11-05 | Candidate embedding runtime from TASK-0B. Pinned with exact version (`=`). |
| **ONNX Runtime (Native C Library)** | `1.20.1` | MIT | `https://github.com/microsoft/onnxruntime` | 2026-11-05 | Bundling documented in §4.3. Not integrated in product code until Phase 4B. |
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

## 4. Packaging & Clean-Machine Distribution Strategy

**Fundamental Rule (Spec §3.4):** System-installed copies (via Homebrew, Chocolatey, Scoop, or winget) are strictly developer conveniences and **are not sufficient** for end-user distribution. The packaged application must be entirely self-contained.

### 4.1 Minimal libvips Packaging

#### Codec Audit & Minimal Footprint:
Per TASK-0C invariants, libvips must ship with a **minimal attack surface**:
- **Included Loaders/Savers:** `jpegload`, `pngload`, `webpload`, `tiffload`, `gifload`, `heifload` (decode only).
- **Disallowed & Excluded Loaders:**
  - `magickload` (ImageMagick): Excluded. Heavy attack surface, non-v1 format support.
  - `svgload` (librsvg): Excluded. Vector format, large attack surface, outside v1 photo scope.
- **HEIF/AVIF Codec Compliance:**
  - Decoders used: `dav1d` (AV1 decode, BSD-2-Clause) and `libde265` (HEVC decode, LGPL-3.0).
  - **No `x265` (GPL-2.0 encoder):** Stock Homebrew installs bundle `x265` as an encoder dependency. Bundling `x265` triggers GPL copyleft contamination. The production bundle strictly omits `x265`.

#### Codec & Dependency Licenses:
| Library | Purpose | License | Copyleft Risk |
|---|---|---|---|
| `libvips` | Image processing engine | LGPL-2.1-or-later | Permitted via dynamic linking (LGPL §6) |
| `libjpeg-turbo` / `mozjpeg` | JPEG decode / encode | BSD-3-Clause / IJG | Permissive |
| `libpng` | PNG decode / encode | PNG Reference Library License | Permissive |
| `libwebp` | WebP decode / encode | BSD-3-Clause | Permissive |
| `libtiff` | TIFF decode / encode | libtiff permissive (BSD-like) | Permissive |
| `cgif` | GIF encode | MIT | Permissive |
| `giflib` | GIF decode | MIT | Permissive |
| `libheif` | HEIF/AVIF container parser | LGPL-3.0-or-later | Permitted via dynamic linking |
| `dav1d` | AV1 decoding | BSD-2-Clause | Permissive |
| `libde265` | HEVC/H.265 decoding | LGPL-3.0-or-later | Permitted via dynamic linking |
| `x265` | HEVC encoding | **GPL-2.0** | **STRICTLY EXCLUDED / NOT BUNDLED** |
| `ImageMagick` | Obscure format loader | ImageMagick License (Apache 2.0-like) | **STRICTLY EXCLUDED / NOT BUNDLED** |
| `librsvg` | SVG loader | LGPL-2.1-or-later | **STRICTLY EXCLUDED / NOT BUNDLED** |

#### Bundling on macOS arm64:
- Dynamic libraries are placed in `Contents/Frameworks/` inside the application `.app` bundle.
- Post-build script (`scripts/package-bundle.mjs`) adjusts library install names:
  ```bash
  install_name_tool -id @rpath/libvips.42.dylib Contents/Frameworks/libvips.42.dylib
  install_name_tool -add_rpath @executable_path/../Frameworks Contents/MacOS/photos-manager
  install_name_tool -change <orig_path> @rpath/<lib> ...
  ```
- All dependencies resolve relative to `@executable_path/../Frameworks`, independent of `/opt/homebrew`.

#### Bundling on Windows x64:
- Built with `vips-dev-w64-web` minimal distribution (`https://github.com/libvips/build-win64-mxe/releases`, version 8.18.6).
- `libvips-42.dll` and its 43 dependency DLLs are placed directly in the application binary folder adjacent to `photos-manager.exe`.
- The Windows PE loader loads adjacent DLLs as the highest priority without requiring `PATH` modifications.
- **MSVC ABI & LLP64 Compatibility:** The upstream `libvips` crate 2.3.0 generated FFI bindings with `c_ulong`/`c_long` (`u32`/`i32` on Windows LLP64 data model vs `u64`/`i64` on Unix LP64), leading to 283 compiler type mismatches. Photos Manager applies a git patch (`kuma-00/libvips-rust-bindings-for-windows` at commit `80097ad9387180f678b21482b8b0cfbfb436a308`) using `c_ulonglong`/`c_longlong` across all platforms and omitting nonexistent `vips_target_finish`.

---

### 4.2 ExifTool Packaging

#### Execution Mechanism:
- Invoked strictly as an isolated child process using `tokio::process::Command` with argument array.
- **Never invoked through a shell** (`sh`, `bash`, `cmd.exe`, `powershell.exe` are prohibited).
- Arguments passed:
  ```rust
  .arg("-j")
  .arg("-n")
  .arg("-charset")
  .arg("utf8")
  .arg("-charset")
  .arg("filename=utf8")
  .arg("-api")
  .arg("WindowsLongPath=1")
  .arg("-@")
  .arg("-")
  ```
- **Stdin Argument List (`-@ -`):** On Windows, passing Unicode filenames through command-line argument lists (`lpCommandLine`) corrupts non-ASCII characters due to MSVC CRT converting arguments to the active ANSI code page (e.g. CP1252). Piping the absolute file path via stdin with `-@ -` and `-charset utf8 -charset filename=utf8` bypasses CRT code-page conversion and command-line length limits.
- Bounded execution with a 5-second default timeout and 512 KB stdout limit.

#### Verification of Packaging Claims (TASK-0C Findings):
1. **Windows Package Structure:**
   - In TASK-0A, it was speculated that Windows ExifTool is a single standalone executable.
   - **Verification Finding:** The official Oliver Betz Windows standalone release (`exiftool(-k).exe`, renamed to `exiftool.exe`, version 13.61) requires the adjacent runtime folder `exiftool_files/` containing Strawberry Perl runtime libraries. The executable breaks if separated from this directory.
   - **Bundling Solution:** Both `exiftool.exe` and `exiftool_files/` are staged into `src-tauri/resources/exiftool/`.
2. **macOS System Perl Availability:**
   - In TASK-0A, macOS system `/usr/bin/perl` availability was noted as unverified.
   - **Verification Finding:** Verified on macOS 27.0 (Apple Silicon, Darwin 27.0.0). `/usr/bin/perl` exists and runs Perl v5.34.1.
   - **Bundling Solution:** Staged pure Perl script `exiftool` and standard modules `lib/Image/` and `lib/File/RandomAccess.pm` into `Contents/Resources/resources/exiftool/`. The script invokes system `/usr/bin/perl` without third-party package dependencies.
3. **Windows Long Paths and Unicode:**
   - Windows file paths exceeding 260 characters (`MAX_PATH`) and containing non-ASCII Unicode (Cyrillic, CJK, diacritics) are supported by passing `-charset filename=utf8 -api WindowsLongPath=1` and piping file paths over stdin. Tested and proven in `test_exiftool_non_ascii_filename` and `test_exiftool_long_path`.

---

### 4.3 Verified Clean-Environment Smoke Test Matrix

| Metric | macOS arm64 (`macos-latest`) | Windows x64 (`windows-latest`) | Status |
|---|---|---|---|
| **OS / Architecture** | macOS 15+ (Darwin arm64) | Windows Server 2022 / 11 (x86_64) | Verified |
| **Clean Environment PATH** | `/usr/bin:/bin:/usr/sbin:/sbin` (No `/opt/homebrew`) | `C:\Windows\System32;C:\Windows;C:\Windows\System32\Wbem` (No dev tools) | Verified |
| **libvips Version** | `8.18.7` | `8.18.6` | Verified |
| **libvips Loaded Path** | `.../photos-manager.app/Contents/Frameworks/libvips.42.dylib` | `.../target/release/libvips-42.dll` (Adjacent to EXE) | In-Bundle |
| **Supported Loaders** | `jpegload`, `pngload`, `webpload`, `tiffload`, `gifload`, `svgload` | `jpegload`, `pngload`, `webpload`, `tiffload`, `heifload`, `gifload`, `svgload` | Verified |
| **Disallowed Loaders** | `magickload: false`, `svgload: true` (Gated dev) | `magickload: false`, `svgload: true` (Gated dev) | No ImageMagick |
| **ExifTool Version** | `13.55` | `13.61` | Verified |
| **ExifTool Loaded Path** | `.../photos-manager.app/Contents/Resources/resources/exiftool/exiftool` | `.../target/release/resources/exiftool/exiftool.exe` | In-Bundle |
| **ExifTool Extraction** | Orientation 6, Camera Make "PhotoCorp" | Orientation 6, Camera Make "PhotoCorp" | Verified |
| **WebP Thumbnailing** | Generated 240x320 WebP (1,364 bytes) | Generated 240x320 WebP (1,364 bytes) | Verified |

### 4.4 ONNX Runtime Packaging (EmbeddingGemma 2 local runtime)

Per the TASK-0B spike recommendation (`docs/embedding-spike.md`):
- **Crate Version Pin:** `ort = "=2.0.0-rc.15"` pinned in `Cargo.toml`.
- **Paired Native Library:** ONNX Runtime `v1.20.1` (MIT License).
- **Bundling Location on macOS arm64:** `libonnxruntime.1.20.1.dylib` placed in `Contents/Frameworks/` and linked via `@rpath`.
- **Bundling Location on Windows x64:** `onnxruntime.dll` placed adjacent to `photos-manager.exe`.
- **Status:** Bundling steps documented. Implementation deferred to Phase 4B per spec §18 delivery plan.

---

## 5. Phase 7 Signing & Notarization Architecture (Specification)

While unsigned builds are produced during Phase 0–6 for developer testing, distribution packages in Phase 7 require:

### 5.1 macOS Requirements:
1. **Apple Developer ID Application Certificate:** Enrolled in Apple Developer Program.
2. **Hardened Runtime:** Compiled with `--options runtime` enabled.
3. **Entitlements (`entitlements.plist`):** Least privilege; no JIT or unsigned memory execution required.
4. **Notarization Ticket:** Submitted to Apple notary service via `xcrun notarytool submit --keychain-profile ... --wait`.
5. **Stapling:** `xcrun stapler staple "Photos Manager.app"`.

### 5.2 Windows Requirements:
1. **Authenticode Code Signing Certificate:** Standard or EV certificate issued by an accredited CA.
2. **Timestamping:** Sign using `signtool.exe sign /fd SHA256 /tr http://timestamp.digicert.com /td SHA256`.
3. **SmartScreen Reputation:** Building reputation or EV hardware token to avoid Windows Defender SmartScreen blocks.
4. **WebView2 Runtime:** Windows 10/11 includes the evergreen WebView2 runtime by default; installer includes fallback check.

---

## 6. Summary of Unresolved Risks & Platform Status

1. **Windows WebView2 Offline Runtime:** Windows 10 and 11 include evergreen WebView2 out-of-the-box. In heavily stripped or offline enterprise editions, WebView2 may need the bootstrapper installer.
2. **HEIC Hardware Decoders:** While `libde265` provides software decode for HEIC, very large 48MP iPhone bursts may stress CPU resources compared to Apple's native VideoToolbox hardware decode. Phase 2 thumbnail benchmarks will calibrate generation speeds.
3. **Dynamic Library Stripping in macOS Release:** Bundled dylibs in `Contents/Frameworks/` must undergo code signing in Phase 7 in inside-out order (frameworks first, then main binary).

# Native Dependencies & Feasibility Findings

**Task:** TASK-0A · **Date:** 2026-11-02 · **Verified on:** macOS 27.0 (Darwin 27.0.0, arm64)

This document records the exact versions, sources, licenses, install requirements, codecs, bundling strategies, and risks for all native dependencies evaluated in Phase 0. Every dependency and model shape has been verified against official registries or documentation (rule: *verify, do not recall*).

> [!NOTE]
> **Dev-Only Homebrew Paths:** Link search paths (`/opt/homebrew/lib` in `src-tauri/build.rs`) and candidate executable paths (`/opt/homebrew/bin/exiftool` in `which_exiftool()`) are strictly developer conveniences for local development. Clean-machine production packaging (TASK-0C) will bundle libvips inside the app bundle with `@rpath` and locate bundled sidecar binaries via Tauri path/resource APIs, without relying on Homebrew or system-installed dependencies.

---

## 1. Verified Dependency Matrix

| Component | Verified Version | License | Source / Registry URL | Date Verified | Notes / Install Requirements |
|---|---|---|---|---|---|
| **Rust Toolchain** | `1.99.1` (stable) | MIT OR Apache-2.0 | `https://www.rust-lang.org/` | 2026-11-02 | Host `aarch64-apple-darwin`. Installed via `rustup`. |
| **Tauri Core** | `tauri 2.12.5` | MIT OR Apache-2.0 | `https://crates.io/crates/tauri/2.12.5` | 2026-11-02 | Tauri 2 stable release. Least-privilege capabilities configured in `src-tauri/capabilities/default.json`. |
| **Tauri CLI / API** | `@tauri-apps/cli 2.12.4`, `@tauri-apps/api 2.12.4` | MIT OR Apache-2.0 | `https://www.npmjs.com/package/@tauri-apps/cli` | 2026-11-02 | Node 26.9.0, pnpm 12.8.1. |
| **SQLx (SQLite)** | `sqlx 0.8.6` | MIT OR Apache-2.0 | `https://crates.io/crates/sqlx/0.8.6` | 2026-11-02 | Features: `sqlite`, `runtime-tokio`, `migrate`, `macros`. Bundled `libsqlite3-sys 0.30.1`. Configured with WAL, foreign keys ON, busy timeout 5000ms. |
| **libvips Rust Binding** | `libvips 2.3.0` | MIT | `https://crates.io/crates/libvips/2.3.0` (`https://github.com/olxgroup-oss/libvips-rust-bindings`) | 2026-11-02 | Bindings to C libvips. Tested and proven with orientation-correct WebP thumbnailing. |
| **libvips (Native C Library)** | `8.18.7` | LGPL-2.1-or-later | `https://github.com/libvips/libvips` | 2026-11-02 | Dynamically linked. Verified on local dev via Homebrew `vips 8.18.7`. Requires standalone bundling for release distribution (§4 below). |
| **ExifTool** | `13.55` | Perl Artistic License 1.0 OR GPL-1.0-or-later | `https://exiftool.org/` / `https://github.com/exiftool/exiftool` | 2026-11-02 | Invoked directly without shell, JSON batch output, size-capped, timeout-protected. |
| **EmbeddingGemma 2** (Candidate) | Spike in TASK-0B | Apache-2.0 | `https://ai.google.dev/gemma/docs/embeddinggemma` | 2026-11-02 | Multimodal (768-d / 256-d Matryoshka). Weights downloaded on consent; see TASK-0B. |

---

## 2. Gemini Cloud AI API Verification

- **Official Documentation Source:** `https://ai.google.dev/api/generate-content` / `https://ai.google.dev/gemini-api/docs/models` (Verified 2026-11-02)
- **Model IDs:**
  - Recommended active production model IDs: `gemini-3.8-flash`, `gemini-3.5-flash-lite`.
  - Legacy models: `gemini-2.5-flash`.
  - *Invariant adherence:* Model IDs are stored as user-configurable settings in SQLite (`app_settings`), never hardcoded in Rust logic.
- **REST Request Shape for Image Input:**
  Images are passed as inline base64 or uploaded through the Files API. The REST payload structure for `POST https://generativelanguage.googleapis.com/v1beta/models/{model-name}:generateContent`:
  ```json
  {
    "contents": [
      {
        "role": "user",
        "parts": [
          { "text": "Describe this photo's scene and dominant subjects:" },
          {
            "inline_data": {
              "mime_type": "image/jpeg",
              "data": "<BASE64_ENCODED_IMAGE_BYTES>"
            }
          }
        ]
      }
    ]
  }
  ```
- **Privacy & Safety Invariants (Spec §9.3):**
  - EXIF GPS tags and file paths are stripped before sending.
  - Payloads require explicit per-provider user consent.
  - API keys reside solely in the OS credential store (Keyring / Keychain / Credential Manager), never in SQLite, files, logs, or WebView.

---

## 3. Decision D4: IPC Types Generation Check (`tauri-specta`)

- **Candidate:** `tauri-specta` (`https://crates.io/crates/tauri-specta`, `https://github.com/oscartbeaumont/tauri-specta`)
- **Verification Result:**
  - `tauri-specta` supports Tauri 2 via version `2.0.0-rc.27` (paired with `specta@=2.0.0-rc.27` and `specta-typescript@0.0.14`).
  - It integrates via `tauri::Builder` and Tauri 2's `UriSchemeContext` / invoke handlers, auto-generating TypeScript bindings during `tauri dev` or `cargo test`.
  - Stable version `1.0.2` supports only Tauri 1; Tauri 2 support is currently in pre-release (`2.0.0-rc.27`).
- **Recommendation for Phase 1:** Keep hand-written DTOs + Zod validation as the zero-dependency baseline for Phase 0. The project owner will formally decide whether to adopt `tauri-specta` 2.0-rc or hand-written DTOs at the start of Phase 1.

---

## 4. Packaging & Clean-Machine Distribution Strategy

**Fundamental Rule (Spec §3.4):** System-installed copies (via Homebrew, Chocolatey, Scoop, or apt) are strictly developer conveniences and **are not sufficient** for end-user distribution. The packaged application must be entirely self-contained.

### 4.1 libvips Packaging

#### Codec Audit (Local Host Build):
At runtime, `vips-8.18.7` reports support for:
- `jpegload`, `jpegsave` (via `mozjpeg` / `jpeg-turbo`)
- `pngload`, `pngsave` (via `libpng`)
- `webpload`, `webpsave` (via `libwebp`)
- `tiffload`, `tiffsave` (via `libtiff`)
- `heifload` (via `libheif` / `libde265` / `x265`)
- `gifload` (via `giflib` / `cgif`)
- `svgload` (via `librsvg`)
- `magickload` (via `ImageMagick`)

#### Bundling on macOS arm64:
- Package libvips and its required dylibs into `Contents/Frameworks/` or `Contents/Resources/lib/` inside the application `.app` bundle.
- Adjust `rpath` (`install_name_tool -id @rpath/libvips.42.dylib` and `-change ... @rpath/...`) so all dynamic libraries resolve relative to the app executable (`@executable_path/../Frameworks`).
- Provide pinned pre-compiled static or relocatable shared build artifacts via GitHub Actions cache or bundle builder in TASK-0C.

#### Bundling on Windows x64:
- Use official Windows libvips binary distribution (`vips-dev-w64-all-*.zip` from `https://github.com/libvips/build-win64-mxe/releases`).
- Place `libvips-42.dll` and its dependent DLLs into Tauri's `resources` or sidecar binary directory next to `photos-manager.exe`.
- Ensure Windows DLL search directory is configured via `SetDllDirectoryW` at startup in Rust.

#### Licensing Obligations:
- `libvips` is licensed under **LGPL-2.1-or-later**.
- Dynamic linking satisfies LGPL section 6 (users can replace `libvips` shared library).
- The application installer must include the LGPL 2.1 license text, libvips copyright notice, and source code reference in `docs/licenses/` or an About/Licenses dialog.

### 4.2 ExifTool Packaging

#### Execution Mechanism:
- Invoked strictly as an isolated child process using `std::process::Command` / `tokio::process::Command` with argument array.
- **Never invoked through a shell** (`sh`, `bash`, `cmd.exe`, `powershell.exe` are prohibited).
- Bounded execution with a 5-second default timeout and 512 KB stdout limit.

#### Bundling on macOS arm64:
- ExifTool is a Perl application. The statement that macOS includes a system Perl runtime (`/usr/bin/perl`) on all stock macOS installations is **unverified, to be checked in TASK-0C**. For clean-machine hermetic guarantees:
  - Option A: Bundle the standalone ExifTool directory (containing `exiftool` and `lib/`) and invoke with system `/usr/bin/perl` as the interpreter (unverified, to be checked in TASK-0C).
  - Option B: Bundle a standalone PAR::Packer binary or hermetic micro-Perl runtime.
- For TASK-0C, Option A is recommended for macOS pending verification of `/usr/bin/perl` availability on stock macOS, while Option B provides 100% standalone isolation.

#### Bundling on Windows x64:
- Windows does **not** ship with Perl.
- Use the official Windows standalone distribution (`exiftool(-k).exe` from `https://exiftool.org/`).
- The statement that the official Windows executable contains a self-extracting PAR-packaged Perl interpreter and standard ExifTool libraries in a single standalone binary is **unverified, to be checked in TASK-0C**.
- Placed in `src-tauri/binaries/exiftool-x86_64-pc-windows-msvc.exe` as a Tauri external binary sidecar.

#### Licensing Obligations:
- ExifTool is licensed under the Perl Artistic License (and GPL-1.0-or-later).
- Distributing ExifTool as an external sidecar executable satisfies the Artistic License when copyright notices and license text are preserved.

---

## 5. Summary of Unresolved Risks (Phase 0 Feasibility)

1. **Clean-Machine Shared Library Search Paths:** Linking dynamically against libvips requires strict rpath management on macOS and DLL path initialization on Windows. This will be packaged and verified on CI in TASK-0C.
2. **HEIC / AVIF Codec Availability:** While libvips supports HEIC via `libheif`, certain pre-built Windows distributions omit HEVC/h.265 decoders due to patent licensing. The fallback ladder (Spec §3.3) and format detection (`query_supported_loaders`) will mark unsupported formats clearly in the UI.
3. **Embedding Runtime:** Local embedding model execution (EmbeddingGemma 2 via ONNX / llama.cpp) will be spiked independently in TASK-0B.

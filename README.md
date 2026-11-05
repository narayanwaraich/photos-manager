# Photos Manager

A local-first, privacy-respecting cross-platform desktop photo manager built with Tauri 2, React, TypeScript, Vite, Rust, SQLite, libvips, and ExifTool.

---

## Prerequisites

### macOS (Apple Silicon arm64 / Intel x64)
- **Node.js**: v22+ (tested with Node v26.9.0)
- **pnpm**: v12+ (`brew install pnpm` or `npm install -g pnpm`)
- **Rust Toolchain**: 1.80+ stable (tested with Rust 1.99.1) installed via `rustup`:
  ```bash
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  ```
- **Native Development Libraries (Local Dev Only)**:
  ```bash
  brew install vips exiftool pkg-config
  ```
  *(Note: Homebrew packages and paths such as `/opt/homebrew/lib` and `/opt/homebrew/bin/exiftool` are developer-only conveniences for local development. Clean-machine production packaging in TASK-0C bundles libvips and ExifTool inside the application distribution without relying on Homebrew or system-installed dependencies.)*
- **Python 3**: with Pillow (for test fixture generation):
  ```bash
  brew install pillow
  ```

### Windows 10/11 x64 *(Untested in TASK-0A; to be validated on Windows PC / TASK-0C)*
- **Node.js**: v22+
- **pnpm**: v12+ (`npm install -g pnpm` or via winget)
- **Rust Toolchain**: `rustup-init.exe` targeting `x86_64-pc-windows-msvc`
- **Build Tools**: Microsoft C++ Build Tools (Visual Studio 2022)
- **libvips**: Windows dev binary release unpacked and added to `PATH`
- **ExifTool**: Standalone Windows executable `exiftool.exe` added to `PATH`

---

## Getting Started

### 1. Install Frontend Dependencies
```bash
pnpm install
```

### 2. Generate Synthetic Test Fixtures
```bash
python3 scripts/generate-fixtures.py
```

### 3. Run Development Server
```bash
pnpm tauri dev
```
*(Windows PowerShell: `pnpm tauri dev` - untested)*

### 4. Run Full Verification Suite
The project enforces a single verification command (`pnpm verify`) which checks formatting, clippy linter warnings, Rust unit/integration tests, TypeScript type checks, frontend linter, Vite production build, and `AGENTS.md` length invariant:
```bash
pnpm verify
```
Logs are saved automatically to `reports/logs/verify-<date>.txt`.

### 5. Build Production Application
```bash
# Build frontend and compile release binary (without packaging)
pnpm tauri build --no-bundle

# Build packaged installer (.dmg on macOS, .msi/.exe on Windows)
pnpm tauri build
```
*(Windows PowerShell: `pnpm tauri build` - untested)*

---

## Verification & Test Commands

```bash
# Full test suite
cargo test --manifest-path src-tauri/Cargo.toml

# Security check: verify no shell invocations
rg -n "Command::new\(\"(sh|bash|cmd|powershell)" src-tauri/src   # must return no matches

# Architectural check: verify no generic arbitrary SQL execution
rg -n "execute_sql" src-tauri/src                                  # must return no matches

# Rule file length invariant
wc -c AGENTS.md                                                    # must be < 12000
```

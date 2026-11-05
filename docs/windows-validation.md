# Windows Validation Guide & Manual Checklist

**Target Platform:** Windows 10/11 x64 (MSVC ABI `x86_64-pc-windows-msvc`)  
**Applicability:** Phase 0 feasibility gate and platform-sensitive release milestones (Phases 1, 2, 5, 7 per spec §17.2).

> [!IMPORTANT]
> **Cross-Platform Validation Invariant (Spec §17.2):**
> CI is the first line of defence on both operating systems, but it cannot test real hardware, real USB drives, filesystem unplugging, OS Recycle Bin transitions, or native WebView2 runtime quirks. A phase with platform-sensitive features does not pass its gate on macOS evidence alone.

---

## 1. Prerequisites Setup (Developer Machine)

Run the following in PowerShell (Administrator where noted):

1. **Install Node.js & pnpm:**
   ```powershell
   winget install OpenJS.NodeJS.LTS
   npm install -g pnpm
   ```
2. **Install Rust Toolchain (MSVC):**
   ```powershell
   winget install Rustlang.Rustup
   rustup default stable-x86_64-pc-windows-msvc
   rustup component add clippy rustfmt
   ```
3. **Install Visual Studio 2022 C++ Build Tools:**
   Ensure "Desktop development with C++" workload is selected.
4. **Install Development libvips & ExifTool (Dev Only):**
   ```powershell
   # Development libvips web distribution
   # Unpack to C:\libvips and add C:\libvips\bin to system PATH
   # Development ExifTool standalone distribution
   winget install OliverBetz.ExifTool
   ```

---

## 2. Developer Workflow Validation

On the owner's Windows PC:

```powershell
# 1. Pull latest code on the active branch
git pull

# 2. Install dependencies
pnpm install --frozen-lockfile

# 3. Generate synthetic test fixtures
python scripts/generate-fixtures.py

# 4. Run the full cross-platform verification suite
pnpm verify
```
*Expected Result:* All steps (`cargo fmt`, `clippy`, `cargo test`, `tsc`, `eslint`, `vite build`, `AGENTS.md` length check) must pass with 0 warnings and 0 errors.

```powershell
# 5. Launch Tauri development mode
pnpm tauri dev
```
*Expected Result:* The desktop window opens displaying the Photos Manager interface with WebView2; thumbnails load over the custom URI scheme without CSP violations.

---

## 3. Clean-Machine Smoke Test (From CI Artifact)

To ensure native dependencies are truly self-contained in the built package, validate the packaged release build on a clean machine or fresh user profile without developer tools.

### Step 3.1: Download CI Artifact
1. Navigate to the GitHub repository **Actions** tab.
2. Select the latest run on branch `task/0C` (or release tag).
3. Download the `photos-manager-windows-x64` artifact.
4. Extract the zip to a clean local directory: e.g. `C:\TestApp\`.

### Step 3.2: Run Hermetic Smoke Test under Stripped PATH
Open standard PowerShell (as regular user) and run:

```powershell
# Strip PATH of any dev tools (simulate a virgin machine without Git, Rust, Perl, Python, vips)
$env:PATH = "C:\Windows\System32;C:\Windows;C:\Windows\System32\Wbem"

# Execute packaged smoke test binary
Set-Location "C:\TestApp"
.\photos-manager.exe --smoke-test --out smoke-windows-local.json
```

*Verification Points:*
- [ ] Process exits with code `0`.
- [ ] Console outputs clean JSON report.
- [ ] `libvips.loaded_library_path` indicates `libvips-42.dll` was loaded from `C:\TestApp\`, **not** from a global dev path.
- [ ] `exiftool.is_bundled` is `true`, resolving to `C:\TestApp\resources\exiftool\exiftool.exe`.
- [ ] `libvips.disallowed_loaders_absent` is `true` (`magickload: false`, `svgload: false`).
- [ ] Thumbnail generation creates `photos_manager_smoke_thumb.webp` (240x320) with non-zero size.

---

## 4. Platform-Sensitive Edge Cases Checklist

Run these manual tests to verify Windows filesystem specifics:

### 4.1 Non-ASCII Folder & Filename Test
1. Create a folder with Unicode and accented characters:
   ```powershell
   New-Item -ItemType Directory -Path "C:\Photos\写真_2026_café_тест"
   Copy-Item "tests\fixtures\generated\image_orientation_6.jpg" "C:\Photos\写真_2026_café_тест\foto_ümlaut_📸.jpg"
   ```
2. Verify ExifTool reads metadata cleanly without mojibake or errors.

### 4.2 Long Path Test (> 260 Characters)
1. Windows NTFS traditionally limits path strings to 260 characters (`MAX_PATH`).
2. Create a deeply nested path exceeding 280 characters:
   ```powershell
   $deepPath = "C:\Photos\" + ("nested_folder_name_segment_12345\" * 10)
   New-Item -ItemType Directory -Path $deepPath -Force
   Copy-Item "tests\fixtures\generated\image_orientation_6.jpg" (Join-Path $deepPath "test_image.jpg")
   ```
3. Verify the app and ExifTool (`-api WindowsLongPath=1`) extract metadata and create thumbnails without `ERROR_PATH_NOT_FOUND` (3) or `ERROR_BUFFER_OVERFLOW` (111).

### 4.3 External USB Drive & Disconnect Handling *(Needed from Phase 1)*
1. Connect an external USB flash drive or portable SSD (e.g. `E:\`).
2. Add a folder on `E:\` as a library source.
3. Observe library scanning behavior.
4. Safely unmount/eject the external drive:
   - Ensure the app marks assets as `offline` or `unavailable` without crashing.
   - Verify SQLite database does not record false deletions.
   - Reconnect the drive and ensure availability is restored.

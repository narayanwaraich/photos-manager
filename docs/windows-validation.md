# Windows Validation Guide & Manual Checklist

**Target Platform:** Windows 10/11 x64 (MSVC ABI `x86_64-pc-windows-msvc`)  
**Reference Low-End Machine:** Acer Swift SF314-43, Ryzen 7 5700U, 8 GB RAM, Radeon graphics, Windows 11  
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
   npm install -g pnpm@12.8.1
   ```
2. **Install Rust Toolchain (MSVC):**
   ```powershell
   winget install Rustlang.Rustup
   rustup component add clippy rustfmt
   ```
   *(Note: Do not run `rustup default stable`; `rust-toolchain.toml` in the repository root automatically selects the pinned `1.99.1` toolchain).*

3. **Install Visual Studio 2022 C++ Build Tools:**
   Ensure the "Desktop development with C++" workload is selected.

4. **Install Custom Development libvips & ExifTool (Dev Only):**
   Download the custom HEVC libvips build from the private dependency repository `narayanwaraich/photos-manager-deps` (Release `win64-vips-8.18.6-hevc1`):
   ```powershell
   # Via GitHub CLI (authenticated with access to private deps repository):
   gh release download win64-vips-8.18.6-hevc1 --repo narayanwaraich/photos-manager-deps --pattern '*.zip' --dir .
   
   # Or download vips-dev-x64-web-8.18.6-hevc.zip directly from the GitHub release page.

   # Verify the pinned SHA-256 checksum:
   $vipsHash = (Get-FileHash vips-dev-x64-web-8.18.6-hevc.zip -Algorithm SHA256).Hash
   if ($vipsHash -ne "72591A6BDF775618BC154E83990C86919ECB18F6E9D72FADE5BAEF9B4F343991") {
       throw "Checksum mismatch: expected 72591A6BDF775618BC154E83990C86919ECB18F6E9D72FADE5BAEF9B4F343991, got $vipsHash"
   }
   echo "✓ Custom libvips checksum verified."

   # Unpack to C:\vips:
   New-Item -ItemType Directory -Path "C:\vips" -Force
   tar.exe -xf vips-dev-x64-web-8.18.6-hevc.zip -C "C:\vips" --strip-components=1
   ```

   Download `exiftool-13.61_64.zip` from `https://exiftool.org/` and unpack to `C:\tools\exiftool` (ensure `exiftool.exe` and `exiftool_files\` are directly in `C:\tools\exiftool`).

   Configure developer environment variables:
   ```powershell
   [System.Environment]::SetEnvironmentVariable("VIPS_DIR", "C:\vips", "User")
   [System.Environment]::SetEnvironmentVariable("VIPS_LIB_DIR", "C:\vips\lib", "User")
   [System.Environment]::SetEnvironmentVariable("EXIFTOOL_DIR", "C:\tools\exiftool", "User")
   [System.Environment]::SetEnvironmentVariable("PKG_CONFIG_PATH", "C:\vips\lib\pkgconfig", "User")
   [System.Environment]::SetEnvironmentVariable("LIB", "C:\vips\lib;$env:LIB", "User")
   $env:Path += ";C:\vips\bin;C:\tools\exiftool"
   [System.Environment]::SetEnvironmentVariable("Path", $env:Path, "User")
   ```
   *(Note: `src-tauri/build.rs` automatically adds `cargo:rustc-link-search=native` for `VIPS_LIB_DIR`, `VIPS_DIR\lib`, and `C:\vips\lib` if present).*

5. **Memory Constraint Limit for 8 GB Laptops:**
   On machines with 8 GB RAM, running `cargo clippy` and `cargo test` with unbounded parallelism can cause memory exhaustion. Limit build concurrency in PowerShell:
   ```powershell
   $env:CARGO_BUILD_JOBS = "2"
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

### Step 3.1: Download & Unpack CI Artifact
1. Navigate to the GitHub repository **Actions** tab.
2. Select the latest successful run on the branch you are testing.
3. Download the `photos-manager-windows-x64` artifact.
4. Extract the zip to a clean local directory: e.g. `C:\DownloadArtifact\`.
5. Unblock downloaded files in PowerShell:
   ```powershell
   Get-ChildItem -Path "C:\DownloadArtifact" -Recurse | Unblock-File
   ```

### Step 3.2: Check Host System Prerequisites
Before testing, record whether Visual C++ runtime and LongPaths are present:
```powershell
# 1. Check if LongPathsEnabled is set in Windows Registry
reg query "HKLM\SYSTEM\CurrentControlSet\Control\FileSystem" /v LongPathsEnabled

# 2. Check if VC++ runtime DLL exists in System32
Test-Path "C:\Windows\System32\vcruntime140.dll"
```
*(Note: A pass on a machine that already has Visual Studio or the VC++ redistributable installed does not prove the app runs without them. For production release testing in Phase 7, test on a virgin VM or ensure the installer bundles the redistributable).*

### Step 3.3: Install Package Silently
The NSIS installer produced by Tauri must be installed into a fresh target folder:
```powershell
$installer = (Get-ChildItem -Path "C:\DownloadArtifact\bundle\nsis\*.exe").FullName | Select-Object -First 1

# Note: SmartScreen may warn about the unsigned installer. Select "More info" -> "Run anyway" if running interactively.
# NSIS silent installation requires /D= to be the last argument and unquoted:
Start-Process -FilePath $installer -ArgumentList "/S /D=C:\pm-test" -Wait

# Verify installation directory
Get-ChildItem -Path "C:\pm-test"
```

### Step 3.4: Run Hermetic Smoke Test under Stripped PATH
The release executable is a Windows GUI-subsystem binary (`subsystem = "windows"`), so PowerShell will not wait for it or display console stdout unless invoked with `Start-Process -Wait -PassThru`:

```powershell
# 1. Strip PATH of developer tools (simulate a clean machine)
$origPath = $env:PATH
$env:PATH = "C:\Windows\System32;C:\Windows;C:\Windows\System32\Wbem"

# 2. Execute packaged smoke test binary with output file
$proc = Start-Process -FilePath "C:\pm-test\photos-manager.exe" -ArgumentList @("--smoke-test", "--out", "C:\pm-test\smoke-laptop.json") -Wait -NoNewWindow -PassThru
$env:PATH = $origPath

# 3. Print exit code and JSON result
echo "Smoke test ExitCode: $($proc.ExitCode)"
Get-Content "C:\pm-test\smoke-laptop.json"
```

*Verification Points:*
- [ ] Process exits with code `0`.
- [ ] `libvips.loaded_library_path` indicates `libvips-42.dll` was loaded from `C:\pm-test\libvips-42.dll`, **not** from a global path.
- [ ] `exiftool.is_bundled` is `true`, resolving to `C:\pm-test\resources\exiftool\exiftool.exe`.
- [ ] `libvips.disallowed_loaders_absent` is `true` (`magickload: false`, `svgload: false`, `blocked_untrusted_svg_load_verified: true`).
- [ ] Thumbnail generation creates `photos_manager_smoke_thumb.webp` (240x320) with non-zero size.
- [ ] `heic_decode_verified` is `true` with `heic_note: "HEVC decode and upright 240x320 thumbnail verified"`.

### Step 3.5: Owner Step — Real iPhone HEIC File Validation
Test decoding a real iPhone HEIC file from personal photos:
> [!IMPORTANT]
> **Privacy Invariant:** Decode one of your own iPhone HEIC files (work on a copy, not the original). **Do not commit personal photos to git.**

```powershell
# 1. Place a test iPhone HEIC file at a temporary path (work on a copy, not the original):
$myHeic = "C:\temp\my_iphone_photo_copy.heic"

# 2. Test CLI decode using the bundled vipsthumbnail from the installed application:
$outWebp = "C:\temp\my_iphone_thumb.webp"
& "C:\pm-test\bin\vipsthumbnail.exe" $myHeic -s 320 -o $outWebp
if ($LASTEXITCODE -eq 0 -and (Test-Path $outWebp)) {
    echo "✓ Success: Real iPhone HEIC decoded to WebP thumbnail!"
} else {
    echo "❌ Failed to decode iPhone HEIC file."
}

# 3. Clean up only the generated output thumbnail (preserve input photo copy):
Remove-Item -Force $outWebp -ErrorAction SilentlyContinue
```

### Step 3.6: Clean Up / Uninstall
```powershell
Start-Process -FilePath "C:\pm-test\uninstall.exe" -ArgumentList "/S" -Wait
Remove-Item -Recurse -Force "C:\pm-test" -ErrorAction SilentlyContinue
```

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

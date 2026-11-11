# macOS Validation Guide & Manual Checklist

**Target Platform:** macOS 15+ Apple Silicon arm64 (`aarch64-apple-darwin`)  
**Untested Target:** Intel macOS (`x86_64-apple-darwin`)  
**Applicability:** Phase 0 HEIC packaging gate and platform-sensitive release milestones (Phases 1, 2, 5, 7 per spec §17.2).

> [!IMPORTANT]
> **Cross-Platform Validation Invariant (Spec §17.2):**
> CI verifies the packaged `.app` with Homebrew hidden on `macos-latest`, while physical Apple Silicon validation covers real iPhone HEIC files, Gatekeeper quarantine removal, and Unicode NFD/NFC path edge cases.

---

## 1. Prerequisites Setup (Developer Machine)

1. **Install Xcode Command Line Tools, Node.js 22+, pnpm 12.8.1, and Rust `1.99.1`** (`rust-toolchain.toml` selects the pinned Rust toolchain automatically).
2. **Download Custom HEVC-Decode libvips (`macos-arm64-vips-8.18.6-hevc1`):**
   ```bash
   gh release download macos-arm64-vips-8.18.6-hevc1 --repo narayanwaraich/photos-manager-deps --pattern '*.zip' --dir /tmp/vips-dl
   shasum -a 256 /tmp/vips-dl/vips-macos-arm64-8.18.6-hevc.zip
   # Expected:
   # 6bb24d51dedf9f69344aa187cb0f9651b7842de4c465643480f3a794481dba64  .../vips-macos-arm64-8.18.6-hevc.zip

   mkdir -p deps/_extract && rm -rf deps/_extract/* deps/vips
   unzip -q /tmp/vips-dl/vips-macos-arm64-8.18.6-hevc.zip -d deps/_extract
   mv deps/_extract/vips-macos-arm64-8.18.6-hevc deps/vips
   rmdir deps/_extract
   ```
   Export for shell builds / packaging tests against the custom prefix:
   ```bash
   export VIPS_DIR="$PWD/deps/vips"
   export VIPS_LIB_DIR="$VIPS_DIR/lib"
   export PKG_CONFIG_PATH="$VIPS_LIB_DIR/pkgconfig"
   ```
   *(For quick local debug builds, `cargo test` in debug profile or `PHOTOS_DEV_HOMEBREW=1` can still use Homebrew; release builds require `VIPS_DIR` / `VIPS_LIB_DIR` or explicit `PHOTOS_DEV_HOMEBREW=1`.)*
3. **Run Verification Suite:**
   ```bash
   pnpm install --frozen-lockfile
   python3 scripts/generate-fixtures.py
   pnpm verify
   ```

---

## 2. Clean-Environment Smoke Test (From CI Artifact)

1. Navigate to the GitHub Actions run on the branch you are testing and download `photos-manager-macos-arm64` (unzip if downloaded via browser so `photos-manager-macos-arm64.zip` is in hand).
2. Unpack to a fresh directory outside the repo and clear the Gatekeeper quarantine extended attribute on the unsigned bundle:
   ```bash
   rm -rf /tmp/pm-test && mkdir -p /tmp/pm-test
   unzip -q photos-manager-macos-arm64.zip -d /tmp/pm-test
   xattr -dr com.apple.quarantine "/tmp/pm-test/photos-manager.app"
   ```
3. Verify zero Homebrew references in `otool -L` and `LC_RPATH`, and confirm exact 22 bundled `.dylib`s:
   ```bash
   APP="/tmp/pm-test/photos-manager.app"
   EXE="$APP/Contents/MacOS/photos-manager"
   otool -L "$EXE"
   otool -l "$EXE" | grep -A2 "cmd LC_RPATH"
   ls -1 "$APP/Contents/Frameworks"/*.dylib | wc -l   # Expected: 22
   ```
4. Execute the packaged binary in a stripped environment with reduced `PATH`:
   ```bash
   env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin REQUIRE_HEIC=1 \
     "$EXE" --smoke-test --out /tmp/pm-test/smoke-mac.json
   cat /tmp/pm-test/smoke-mac.json
   ```

*Verification Points:*
- [ ] Exits with code `0`.
- [ ] `status: "ok"`, `errors: []`.
- [ ] `libvips.version` is `"8.18.6"` and `loaded_library_path` points to `/tmp/pm-test/photos-manager.app/Contents/Frameworks/libvips.42.dylib` (not `/opt/homebrew`).
- [ ] `libvips.supported_loaders` includes `"heifload"`, `disallowed_loaders_absent: true`.
- [ ] `exiftool.is_bundled: true` and `thumbnail` outputs `240x320` WebP.
- [ ] `heic_decode_verified: true` with `heic_note: "HEVC decode and upright 240x320 thumbnail verified"`.

---

## 3. Owner Step — Real iPhone HEIC File Validation

> [!IMPORTANT]
> **Privacy Invariant:** Decode a **copy** of one of your own iPhone HEIC files in `/tmp` — never modify an original library file and **never commit personal photos to git**.

Decode using the custom `vips` CLI archive (same `libvips.42.dylib` + `libheif.1.dylib` + `libde265.0.dylib` runtime bundled in `photos-manager.app`) under reduced `PATH`:

```bash
# 1. Copy one of your iPhone HEIC photos into /tmp (unquarantine custom vips dir if downloaded via browser):
cp "/path/to/your/IMG_XXXX.HEIC" /tmp/my_iphone_photo_copy.heic
VIPS_PREFIX="$PWD/deps/vips"   # or unpacked path to vips-macos-arm64-8.18.6-hevc
xattr -dr com.apple.quarantine "$VIPS_PREFIX"

# 2. Decode upright 320px WebP thumbnail under stripped PATH (using zero Homebrew libraries):
env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin "$VIPS_PREFIX/bin/vipsthumbnail" \
  /tmp/my_iphone_photo_copy.heic -s 320 -o /tmp/my_iphone_thumb.webp

# 3. Inspect dimensions & inspect visually in Preview:
env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin "$VIPS_PREFIX/bin/vipsheader" /tmp/my_iphone_thumb.webp
open /tmp/my_iphone_thumb.webp

# 4. Clean up temporary output copy when done:
rm -f /tmp/my_iphone_thumb.webp /tmp/my_iphone_photo_copy.heic
```

*(Note: Once the Phase 1/2 library UI lands, open `/tmp/my_iphone_photo_copy.heic` directly inside `photos-manager.app` as well.)*

---

## 4. Cross-OS Parity Check (macOS arm64 vs Windows x64)

Compare the **same synthetic HEIC fixture** (`tests/fixtures/generated/image_orientation_6.heic`) across macOS (`deps/vips` / `smoke-mac.json`) and Windows (`C:\vips` / `smoke-laptop.json` from `docs/windows-validation.md`):

### macOS arm64 commands:
```bash
VIPS_BIN="$PWD/deps/vips/bin"
env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin "$VIPS_BIN/vipsthumbnail" \
  tests/fixtures/generated/image_orientation_6.heic -s 320 -o /tmp/mac_heic_240x320.png
env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin "$VIPS_BIN/vipsheader" /tmp/mac_heic_240x320.png
env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin "$VIPS_BIN/vips" getpoint /tmp/mac_heic_240x320.png 120 160
rm -f /tmp/mac_heic_240x320.png
```

### Windows x64 PowerShell commands:
```powershell
C:\vips\bin\vipsthumbnail.exe tests\fixtures\generated\image_orientation_6.heic -s 320 -o C:\temp\win_heic_240x320.png
C:\vips\bin\vipsheader.exe C:\temp\win_heic_240x320.png
C:\vips\bin\vips.exe getpoint C:\temp\win_heic_240x320.png 120 160
Remove-Item -Force C:\temp\win_heic_240x320.png
```

*Expected parity:*
- `vipsheader` reports identical upright dimensions **`240x320`** (`uchar, 3 bands, srgb`) on both macOS and Windows.
- `vips getpoint ... 120 160` reports matching center pixel RGB (`70 130 180` within encoder/rounding tolerance) on both OSes.
- Both `smoke-mac.json` and `smoke-laptop.json` report `"libvips.version": "8.18.6"` and `"heic_decode_verified": true`.

---

## 5. Clean Up

```bash
rm -rf /tmp/pm-test
```

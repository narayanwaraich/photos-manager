# Test Fixtures

Generated synthetic test fixtures for Photos Manager Phase 0 (TASK-0A and TASK-0E).
All fixtures are synthesized using `scripts/generate-fixtures.py` via Pillow and macOS `sips`; no real personal photographs are included.

## Fixture Inventory

| File | Format | Dimensions | EXIF Orientation | Metadata | Purpose |
|---|---|---|---|---|---|
| `image_orientation_6.jpg` | JPEG | 640×480 | 6 (Rotate 90° CW) | Make: `PhotoCorp`, Model: `CameraPro 1`, Date: `2026:06:15 14:30:00` | Testing orientation-correct thumbnailing (renders upright as 480×640 before scaling) and ExifTool orientation tag parsing. |
| `image_orientation_1.jpg` | JPEG | 640×480 | 1 (Normal / Identity) | Make: `PhotoCorp`, Model: `CameraPro 2`, Date: `2026:07:20 09:15:30` | Baseline standard orientation JPEG thumbnailing and ExifTool metadata parsing. |
| `image_plain.png` | PNG | 640×480 | None | None | PNG support validation without EXIF metadata. |
| `corrupt_truncated.jpg` | JPEG | Incomplete (300 B) | Truncated | Corrupt header | Decoder resilience testing: fails gracefully without process panics or unbounded hangs. |
| `not_an_image.txt` | Text | N/A | None | Plain text | Verifying non-image handling, parsing rejection, and error reporting. |
| `fixture_blocked.svg` | SVG | 100×100 | None | Vector XML | Testing active runtime untrusted loader blocking (`svgload`). |
| `image_orientation_6.heic` | HEIC (HEVC) | 640×480 | 6 (Rotate 90° CW) | Make: `PhotoCorp`, Model: `CameraPro 1`, HEVC Config v1 | iPhone-style HEIC fixture generated with `sips -s format heic`. Used to verify HEVC decoding, upright 240×320 thumbnailing, and ExifTool metadata extraction. |
| `image_display_p3.heic` | HEIC (HEVC) | 640×480 | 1 (Normal) | Make: `PhotoCorp`, Model: `CameraPro 2`, Profile: `Display P3` | Wide-gamut Display P3 colour profile HEIC fixture generated with `sips -s format heic -m "/System/Library/ColorSync/Profiles/Display P3.icc"`. |

## Generation Script

To regenerate the fixture set:
```bash
python3 scripts/generate-fixtures.py

# On macOS, generate synthetic HEIC fixtures from JPEG sources:
sips -s format heic tests/fixtures/generated/image_orientation_6.jpg --out tests/fixtures/generated/image_orientation_6.heic
sips -s format heic -m "/System/Library/ColorSync/Profiles/Display P3.icc" tests/fixtures/generated/image_orientation_1.jpg --out tests/fixtures/generated/image_display_p3.heic
```
Outputs are written to `tests/fixtures/generated/`. Windows and macOS CI use the committed bytes directly.

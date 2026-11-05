# Test Fixtures

Generated synthetic test fixtures for Photos Manager Phase 0 (TASK-0A).
All fixtures are synthesized using `scripts/generate-fixtures.py` via Pillow; no real personal photographs are included.

## Fixture Inventory

| File | Format | Dimensions | EXIF Orientation | Metadata | Purpose |
|---|---|---|---|---|---|
| `image_orientation_6.jpg` | JPEG | 640×480 | 6 (Rotate 90° CW) | Make: `PhotoCorp`, Model: `CameraPro 1`, Date: `2026:06:15 14:30:00` | Testing orientation-correct thumbnailing (renders upright as 480×640 before scaling) and ExifTool orientation tag parsing. |
| `image_orientation_1.jpg` | JPEG | 640×480 | 1 (Normal / Identity) | Make: `PhotoCorp`, Model: `CameraPro 2`, Date: `2026:07:20 09:15:30` | Baseline standard orientation JPEG thumbnailing and ExifTool metadata parsing. |
| `image_plain.png` | PNG | 640×480 | None | None | PNG support validation without EXIF metadata. |
| `corrupt_truncated.jpg` | JPEG | Incomplete (300 B) | Truncated | Corrupt header | Decoder resilience testing: fails gracefully without process panics or unbounded hangs. |
| `not_an_image.txt` | Text | N/A | None | Plain text | Verifying non-image handling, parsing rejection, and error reporting. |

## Generation Script

To regenerate the fixture set:
```bash
python3 scripts/generate-fixtures.py
```
Outputs are written to `tests/fixtures/generated/`.

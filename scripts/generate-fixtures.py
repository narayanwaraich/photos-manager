#!/usr/bin/env python3
"""
Generate synthetic test fixtures for Photos Manager TASK-0A.
Generates non-real photos with known dimensions, EXIF orientations,
and metadata for testing image pipeline and ExifTool extractors.
"""

from pathlib import Path
from PIL import Image, ImageDraw

FIXTURES_DIR = Path(__file__).resolve().parent.parent / "tests" / "fixtures" / "generated"

def generate_fixtures():
    FIXTURES_DIR.mkdir(parents=True, exist_ok=True)

    # 1. JPEG with EXIF Orientation 6 (rotated 90 degrees CW; 640x480 source -> upright 480x640)
    img_rot6 = Image.new("RGB", (640, 480), color=(70, 130, 180)) # Steel Blue
    draw = ImageDraw.Draw(img_rot6)
    draw.rectangle([50, 50, 200, 200], fill=(255, 215, 0)) # Gold box on top-left
    draw.text((60, 60), "Orientation 6 Test", fill=(0, 0, 0))
    
    exif_rot6 = img_rot6.getexif()
    exif_rot6[0x0112] = 6  # Orientation: 6 (Rotate 90 CW)
    exif_rot6[0x010f] = "PhotoCorp"
    exif_rot6[0x0110] = "CameraPro 1"
    exif_rot6[0x9003] = "2026:06:15 14:30:00"
    rot6_path = FIXTURES_DIR / "image_orientation_6.jpg"
    img_rot6.save(rot6_path, "JPEG", exif=exif_rot6, quality=90)
    print(f"Generated: {rot6_path}")

    # 2. JPEG with EXIF Orientation 1 (Normal orientation; 640x480)
    img_rot1 = Image.new("RGB", (640, 480), color=(46, 139, 87)) # Sea Green
    draw1 = ImageDraw.Draw(img_rot1)
    draw1.rectangle([50, 50, 200, 200], fill=(255, 99, 71)) # Tomato
    draw1.text((60, 60), "Orientation 1 Test", fill=(255, 255, 255))
    
    exif_rot1 = img_rot1.getexif()
    exif_rot1[0x0112] = 1  # Orientation: 1 (Normal)
    exif_rot1[0x010f] = "PhotoCorp"
    exif_rot1[0x0110] = "CameraPro 2"
    exif_rot1[0x9003] = "2026:07:20 09:15:30"
    rot1_path = FIXTURES_DIR / "image_orientation_1.jpg"
    img_rot1.save(rot1_path, "JPEG", exif=exif_rot1, quality=90)
    print(f"Generated: {rot1_path}")

    # 3. Standard PNG (640x480, no EXIF)
    img_png = Image.new("RGB", (640, 480), color=(106, 90, 205)) # Slate Blue
    draw_png = ImageDraw.Draw(img_png)
    draw_png.ellipse([100, 100, 300, 300], fill=(255, 165, 0)) # Orange circle
    png_path = FIXTURES_DIR / "image_plain.png"
    img_png.save(png_path, "PNG")
    print(f"Generated: {png_path}")

    # 4. Truncated / corrupt JPEG file
    corrupt_path = FIXTURES_DIR / "corrupt_truncated.jpg"
    with open(rot6_path, "rb") as src, open(corrupt_path, "wb") as dst:
        dst.write(src.read(300))  # truncated header only
    print(f"Generated: {corrupt_path}")

    # 5. Non-image plain text file
    txt_path = FIXTURES_DIR / "not_an_image.txt"
    txt_path.write_text("This is plain text content and not a valid image format.\n")
    print(f"Generated: {txt_path}")

if __name__ == "__main__":
    generate_fixtures()

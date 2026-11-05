#!/usr/bin/env python3
"""
generate_fixtures.py

Generates synthetic fixtures for the EmbeddingGemma 2 spike:
- 20 base images with distinct, describable visual content (shapes, colors, rendered text, gradients, patterns).
- Near-duplicate variants of each base:
    * resized (512 -> 384)
    * jpeg_q90
    * jpeg_q60
    * crop_10 (10% center crop, resized back to 512)
    * crop_25 (25% crop, resized back to 512)
    * rotate_90
    * bright_contrast (+20% brightness, +15% contrast)
    * blur (Gaussian blur radius 1.5)
- Distractor pairs (visually similar color/layout but semantically different objects).
- 10 text search queries with documented target fixtures.
"""

import json
import math
import os
from pathlib import Path
from PIL import Image, ImageDraw, ImageFilter, ImageEnhance, ImageFont

FIXTURES_DIR = Path(__file__).resolve().parent / "fixtures"
IMAGES_DIR = FIXTURES_DIR / "images"

def create_gradient(w, h, color1, color2, direction="horizontal"):
    base = Image.new("RGB", (w, h), color1)
    top = Image.new("RGB", (w, h), color2)
    mask = Image.new("L", (w, h))
    mask_data = []
    for y in range(h):
        for x in range(w):
            if direction == "horizontal":
                val = int(255 * (x / max(1, w - 1)))
            elif direction == "vertical":
                val = int(255 * (y / max(1, h - 1)))
            elif direction == "diagonal":
                val = int(255 * ((x + y) / max(1, w + h - 2)))
            else:
                val = 0
            mask_data.append(val)
    mask.putdata(mask_data)
    base.paste(top, (0, 0), mask)
    return base

def draw_star(draw, center, outer_r, inner_r, points, fill, outline=None):
    cx, cy = center
    coords = []
    step = math.pi / points
    for i in range(2 * points):
        r = outer_r if i % 2 == 0 else inner_r
        angle = i * step - math.pi / 2
        coords.append((cx + r * math.cos(angle), cy + r * math.sin(angle)))
    draw.polygon(coords, fill=fill, outline=outline)

def build_base_images():
    images = {}
    size = (512, 512)

    # 1. Red circle on blue background
    im = Image.new("RGB", size, (20, 50, 180))
    draw = ImageDraw.Draw(im)
    draw.ellipse([106, 106, 406, 406], fill=(220, 20, 20), outline=(255, 100, 100), width=4)
    images["01_red_circle_on_blue"] = im

    # 2. Grid of green triangles on yellow background
    im = Image.new("RGB", size, (240, 230, 80))
    draw = ImageDraw.Draw(im)
    for row in range(4):
        for col in range(4):
            x = 64 + col * 100
            y = 64 + row * 100
            pts = [(x, y - 35), (x - 35, y + 35), (x + 35, y + 35)]
            draw.polygon(pts, fill=(20, 140, 30), outline=(10, 80, 20))
    images["02_green_triangles_grid"] = im

    # 3. Bright yellow star on dark purple background
    im = Image.new("RGB", size, (35, 10, 55))
    draw = ImageDraw.Draw(im)
    draw_star(draw, (256, 256), outer_r=180, inner_r=80, points=5, fill=(255, 220, 10), outline=(255, 240, 120))
    images["03_yellow_star_on_purple"] = im

    # 4. Cyan to magenta smooth gradient
    im = create_gradient(512, 512, (0, 220, 240), (230, 20, 160), direction="diagonal")
    images["04_cyan_magenta_gradient"] = im

    # 5. Concentric black and white rings (target bullseye)
    im = Image.new("RGB", size, (255, 255, 255))
    draw = ImageDraw.Draw(im)
    for r in range(240, 0, -30):
        color = (20, 20, 20) if (r // 30) % 2 == 1 else (245, 245, 245)
        draw.ellipse([256 - r, 256 - r, 256 + r, 256 + r], fill=color)
    images["05_concentric_target_rings"] = im

    # 6. Banner with text "SUNSET BEACH"
    im = create_gradient(512, 512, (255, 140, 0), (180, 40, 80), direction="vertical")
    draw = ImageDraw.Draw(im)
    draw.rectangle([40, 200, 472, 312], fill=(20, 20, 30))
    # Fallback default font drawing
    draw.text((80, 235), "SUNSET BEACH", fill=(255, 220, 80), font=ImageFont.load_default())
    images["06_sunset_beach_banner"] = im

    # 7. Colorful horizontal rainbow stripes
    im = Image.new("RGB", size, (255, 255, 255))
    draw = ImageDraw.Draw(im)
    colors = [
        (255, 0, 0), (255, 127, 0), (255, 255, 0), (0, 255, 0),
        (0, 0, 255), (75, 0, 130), (148, 0, 211)
    ]
    h = 512 / len(colors)
    for i, c in enumerate(colors):
        draw.rectangle([0, int(i * h), 512, int((i + 1) * h)], fill=c)
    images["07_rainbow_stripes"] = im

    # 8. Blue diagonal striped pattern on white
    im = Image.new("RGB", size, (250, 250, 250))
    draw = ImageDraw.Draw(im)
    for offset in range(-512, 1024, 40):
        draw.line([offset, 0, offset + 512, 512], fill=(30, 100, 200), width=16)
    images["08_blue_diagonal_stripes"] = im

    # 9. Silhouette of a dog on green field
    im = Image.new("RGB", size, (80, 180, 60))
    draw = ImageDraw.Draw(im)
    # Draw simple dog silhouette: body, head, legs, tail
    draw.ellipse([140, 240, 360, 340], fill=(40, 25, 15))  # body
    draw.ellipse([300, 170, 400, 260], fill=(40, 25, 15))  # head
    draw.polygon([(360, 170), (380, 130), (340, 160)], fill=(40, 25, 15)) # ear
    draw.rectangle([160, 320, 190, 420], fill=(40, 25, 15)) # leg 1
    draw.rectangle([210, 320, 240, 420], fill=(40, 25, 15)) # leg 2
    draw.rectangle([280, 320, 310, 420], fill=(40, 25, 15)) # leg 3
    draw.rectangle([320, 320, 350, 420], fill=(40, 25, 15)) # leg 4
    draw.line([150, 270, 90, 220], fill=(40, 25, 15), width=14) # tail
    images["09_dog_silhouette_grass"] = im

    # 10. Coffee mug with steam on wooden table
    im = Image.new("RGB", size, (140, 90, 50)) # wood table
    draw = ImageDraw.Draw(im)
    # Mug
    draw.rectangle([180, 220, 332, 380], fill=(230, 230, 230), outline=(180, 180, 180), width=4)
    # Handle
    draw.arc([300, 250, 380, 350], start=-90, end=90, fill=(230, 230, 230), width=16)
    # Coffee inside
    draw.ellipse([185, 210, 327, 235], fill=(60, 35, 15))
    # Steam
    draw.arc([220, 120, 260, 200], start=180, end=360, fill=(210, 210, 210), width=4)
    draw.arc([260, 100, 300, 190], start=0, end=180, fill=(210, 210, 210), width=4)
    images["10_coffee_mug_steam"] = im

    # 11. Sharp mountain peaks under starry night sky
    im = create_gradient(512, 512, (10, 15, 45), (35, 45, 90), direction="vertical")
    draw = ImageDraw.Draw(im)
    # Stars
    for pos in [(50, 60), (120, 40), (220, 90), (310, 50), (410, 70), (460, 120), (180, 140)]:
        draw.ellipse([pos[0]-2, pos[1]-2, pos[0]+2, pos[1]+2], fill=(255, 255, 230))
    # Mountain peaks
    draw.polygon([(0, 512), (140, 260), (260, 420), (380, 200), (512, 512)], fill=(50, 55, 75))
    # Snow caps
    draw.polygon([(140, 260), (115, 305), (165, 305)], fill=(240, 245, 255))
    draw.polygon([(380, 200), (345, 255), (415, 255)], fill=(240, 245, 255))
    images["11_mountain_peaks_stars"] = im

    # 12. Red apple with green leaf on white backdrop
    im = Image.new("RGB", size, (250, 250, 250))
    draw = ImageDraw.Draw(im)
    # Apple body
    draw.ellipse([160, 180, 352, 390], fill=(215, 30, 30))
    # Indentation
    draw.ellipse([230, 175, 282, 195], fill=(250, 250, 250))
    # Stem
    draw.line([256, 185, 265, 130], fill=(90, 50, 20), width=6)
    # Leaf
    draw.ellipse([265, 125, 325, 160], fill=(50, 170, 40))
    images["12_red_apple_leaf"] = im

    # 13. Sailboat on deep blue ocean
    im = create_gradient(512, 512, (180, 220, 245), (20, 60, 140), direction="vertical")
    draw = ImageDraw.Draw(im)
    # Sea
    draw.rectangle([0, 340, 512, 512], fill=(20, 60, 140))
    # Hull
    draw.polygon([(170, 350), (340, 350), (310, 390), (200, 390)], fill=(230, 230, 230))
    # Mast
    draw.line([250, 350, 250, 160], fill=(60, 60, 60), width=5)
    # Sail
    draw.polygon([(250, 170), (250, 335), (325, 335)], fill=(245, 245, 240))
    images["13_sailboat_ocean"] = im

    # 14. Skyscraper with glowing windows at night
    im = Image.new("RGB", size, (15, 18, 30))
    draw = ImageDraw.Draw(im)
    draw.rectangle([160, 120, 352, 512], fill=(40, 45, 60), outline=(60, 65, 80))
    # Windows
    for r in range(140, 480, 30):
        for c in range(180, 330, 28):
            color = (255, 220, 90) if ((r * 7 + c * 3) % 5 > 1) else (30, 35, 45)
            draw.rectangle([c, r, c + 16, r + 18], fill=color)
    images["14_skyscraper_night_windows"] = im

    # 15. Bright yellow sunflower blossom
    im = Image.new("RGB", size, (100, 180, 120))
    draw = ImageDraw.Draw(im)
    # Petals
    cx, cy = 256, 256
    for i in range(16):
        ang = i * (2 * math.pi / 16)
        px = cx + 130 * math.cos(ang)
        py = cy + 130 * math.sin(ang)
        draw.ellipse([px - 30, py - 30, px + 30, py + 30], fill=(255, 205, 10))
    # Center disc
    draw.ellipse([cx - 80, cy - 80, cx + 80, cy + 80], fill=(70, 40, 15))
    images["15_sunflower_blossom"] = im

    # 16. Vintage film camera illustration
    im = Image.new("RGB", size, (230, 230, 225))
    draw = ImageDraw.Draw(im)
    # Camera body
    draw.rectangle([120, 200, 392, 380], fill=(50, 50, 55))
    draw.rectangle([120, 200, 392, 250], fill=(180, 180, 185))
    # Shutter button
    draw.rectangle([150, 180, 180, 200], fill=(160, 160, 165))
    # Lens
    draw.ellipse([200, 240, 312, 352], fill=(20, 20, 25), outline=(170, 170, 175), width=8)
    draw.ellipse([225, 265, 287, 327], fill=(50, 70, 100))
    images["16_vintage_camera"] = im

    # 17. Slice of pepperoni pizza
    im = Image.new("RGB", size, (240, 235, 225))
    draw = ImageDraw.Draw(im)
    # Crust & cheese triangle
    draw.polygon([(256, 440), (120, 140), (392, 140)], fill=(245, 195, 45), outline=(195, 120, 30), width=6)
    # Crust arc top
    draw.arc([110, 110, 402, 170], start=180, end=360, fill=(180, 105, 20), width=18)
    # Pepperonis
    for px, py in [(220, 220), (290, 240), (250, 310), (200, 290), (310, 190)]:
        draw.ellipse([px - 22, py - 22, px + 22, py + 22], fill=(185, 35, 25))
    images["17_pepperoni_pizza_slice"] = im

    # 18. Green potted cactus in terracotta pot
    im = Image.new("RGB", size, (245, 240, 230))
    draw = ImageDraw.Draw(im)
    # Terracotta pot
    draw.polygon([(190, 320), (322, 320), (295, 430), (217, 430)], fill=(205, 95, 55))
    draw.rectangle([180, 305, 332, 325], fill=(215, 105, 65))
    # Cactus body
    draw.ellipse([226, 170, 286, 320], fill=(45, 145, 60))
    # Left arm
    draw.arc([180, 200, 235, 270], start=90, end=270, fill=(45, 145, 60), width=20)
    draw.line([180, 200, 180, 230], fill=(45, 145, 60), width=20)
    # Right arm
    draw.arc([275, 220, 330, 290], start=270, end=90, fill=(45, 145, 60), width=20)
    draw.line([330, 220, 330, 250], fill=(45, 145, 60), width=20)
    images["18_cactus_terracotta_pot"] = im

    # 19. Purple lavender field with horizon
    im = create_gradient(512, 512, (240, 220, 180), (140, 180, 220), direction="vertical")
    draw = ImageDraw.Draw(im)
    # Field
    for r in range(260, 512, 10):
        shade = int(120 + (r - 260) * 0.4)
        draw.rectangle([0, r, 512, r + 10], fill=(shade, 40, shade + 30))
    images["19_lavender_field_horizon"] = im

    # 20. Snow covered evergreen pine tree
    im = create_gradient(512, 512, (180, 200, 220), (230, 235, 245), direction="vertical")
    draw = ImageDraw.Draw(im)
    # Trunk
    draw.rectangle([240, 420, 272, 480], fill=(85, 50, 25))
    # Tiers of foliage
    tiers = [
        ([(256, 120), (180, 230), (332, 230)], [(256, 120), (220, 190), (292, 190)]),
        ([(256, 200), (150, 320), (362, 320)], [(256, 200), (200, 275), (312, 275)]),
        ([(256, 280), (120, 430), (392, 430)], [(256, 280), (180, 370), (332, 370)])
    ]
    for green_pts, snow_pts in tiers:
        draw.polygon(green_pts, fill=(25, 95, 45))
        draw.polygon(snow_pts, fill=(245, 250, 255))
    images["20_snowy_pine_tree"] = im

    return images

def build_distractor_pairs():
    """Pairs that share visual structure/color palette but differ semantically."""
    pairs = {}
    size = (512, 512)

    # Distractor 1: Red square on blue background (distractor to 01_red_circle_on_blue)
    im1 = Image.new("RGB", size, (20, 50, 180))
    d1 = ImageDraw.Draw(im1)
    d1.rectangle([110, 110, 402, 402], fill=(220, 20, 20), outline=(255, 100, 100), width=4)
    pairs["distractor_01_red_square_on_blue"] = im1

    # Distractor 2: Blue horizontal stripes on white (distractor to 08_blue_diagonal_stripes)
    im2 = Image.new("RGB", size, (250, 250, 250))
    d2 = ImageDraw.Draw(im2)
    for y in range(0, 512, 40):
        d2.rectangle([0, y, 512, y + 16], fill=(30, 100, 200))
    pairs["distractor_02_blue_horizontal_stripes"] = im2

    # Distractor 3: Desert pyramid under starry night (distractor to 11_mountain_peaks_stars)
    im3 = create_gradient(512, 512, (10, 15, 45), (35, 45, 90), direction="vertical")
    d3 = ImageDraw.Draw(im3)
    for pos in [(50, 60), (120, 40), (220, 90), (310, 50), (410, 70), (460, 120)]:
        d3.ellipse([pos[0]-2, pos[1]-2, pos[0]+2, pos[1]+2], fill=(255, 255, 230))
    d3.polygon([(60, 512), (256, 260), (452, 512)], fill=(180, 140, 70)) # pyramid
    d3.polygon([(256, 260), (452, 512), (380, 512)], fill=(130, 100, 50)) # shaded side
    pairs["distractor_03_pyramid_stars"] = im3

    # Distractor 4: Red tomato with green calyx (distractor to 12_red_apple_leaf)
    im4 = Image.new("RGB", size, (250, 250, 250))
    d4 = ImageDraw.Draw(im4)
    d4.ellipse([150, 190, 362, 380], fill=(225, 35, 20))
    # Star-shaped sepals / green calyx
    draw_star(d4, (256, 190), outer_r=45, inner_r=15, points=5, fill=(40, 150, 30))
    pairs["distractor_04_red_tomato"] = im4

    # Distractor 5: Yellow circle on dark purple (distractor to 03_yellow_star_on_purple)
    im5 = Image.new("RGB", size, (35, 10, 55))
    d5 = ImageDraw.Draw(im5)
    d5.ellipse([126, 126, 386, 386], fill=(255, 220, 10), outline=(255, 240, 120), width=4)
    pairs["distractor_05_yellow_circle_on_purple"] = im5

    return pairs

def generate_near_duplicates(base_images):
    variants = {}
    for name, img in base_images.items():
        w, h = img.size
        # 1. Resized (512 -> 384)
        variants[f"{name}__resized384"] = img.resize((384, 384), Image.Resampling.BILINEAR)

        # 2. JPEG q90
        # Will save and reload to apply JPEG compression artifacts
        variants[f"{name}__jpeg_q90"] = ("jpeg", 90, img)

        # 3. JPEG q60
        variants[f"{name}__jpeg_q60"] = ("jpeg", 60, img)

        # 4. Center crop 10%
        crop_box_10 = (int(w * 0.05), int(h * 0.05), int(w * 0.95), int(h * 0.95))
        c10 = img.crop(crop_box_10).resize((w, h), Image.Resampling.BICUBIC)
        variants[f"{name}__crop10"] = c10

        # 5. Crop 25%
        crop_box_25 = (int(w * 0.125), int(h * 0.125), int(w * 0.875), int(h * 0.875))
        c25 = img.crop(crop_box_25).resize((w, h), Image.Resampling.BICUBIC)
        variants[f"{name}__crop25"] = c25

        # 6. Rotate 90 deg
        variants[f"{name}__rot90"] = img.rotate(90)

        # 7. Brightness +20%, Contrast +15%
        b = ImageEnhance.Brightness(img).enhance(1.20)
        bc = ImageEnhance.Contrast(b).enhance(1.15)
        variants[f"{name}__bright_contrast"] = bc

        # 8. Light blur (Gaussian radius 1.5)
        variants[f"{name}__blur"] = img.filter(ImageFilter.GaussianBlur(radius=1.5))

    return variants

QUERIES = [
    {
        "query": "task: search result | query: red circle on blue background",
        "target_base": "01_red_circle_on_blue"
    },
    {
        "query": "task: search result | query: grid of green triangles",
        "target_base": "02_green_triangles_grid"
    },
    {
        "query": "task: search result | query: bright yellow star on dark purple background",
        "target_base": "03_yellow_star_on_purple"
    },
    {
        "query": "task: search result | query: cyan and magenta color gradient",
        "target_base": "04_cyan_magenta_gradient"
    },
    {
        "query": "task: search result | query: black and white target bullseye concentric rings",
        "target_base": "05_concentric_target_rings"
    },
    {
        "query": "task: search result | query: banner with text SUNSET BEACH in warm colors",
        "target_base": "06_sunset_beach_banner"
    },
    {
        "query": "task: search result | query: horizontal rainbow stripes",
        "target_base": "07_rainbow_stripes"
    },
    {
        "query": "task: search result | query: blue diagonal stripes pattern",
        "target_base": "08_blue_diagonal_stripes"
    },
    {
        "query": "task: search result | query: silhouette of a dog running on grass",
        "target_base": "09_dog_silhouette_grass"
    },
    {
        "query": "task: search result | query: hot coffee mug with rising steam on wooden table",
        "target_base": "10_coffee_mug_steam"
    },
    {
        "query": "task: search result | query: snow capped mountain peaks at night under stars",
        "target_base": "11_mountain_peaks_stars"
    },
    {
        "query": "task: search result | query: red ripe apple with green leaf on white backdrop",
        "target_base": "12_red_apple_leaf"
    },
    {
        "query": "task: search result | query: small sailboat on blue ocean waves",
        "target_base": "13_sailboat_ocean"
    },
    {
        "query": "task: search result | query: tall city skyscraper at night with lit windows",
        "target_base": "14_skyscraper_night_windows"
    },
    {
        "query": "task: search result | query: close-up of a blooming yellow sunflower",
        "target_base": "15_sunflower_blossom"
    },
    {
        "query": "task: search result | query: vintage classic film camera",
        "target_base": "16_vintage_camera"
    },
    {
        "query": "task: search result | query: slice of pepperoni pizza with melted cheese",
        "target_base": "17_pepperoni_pizza_slice"
    },
    {
        "query": "task: search result | query: green potted cactus in terracotta pot",
        "target_base": "18_cactus_terracotta_pot"
    },
    {
        "query": "task: search result | query: purple lavender field with distant horizon",
        "target_base": "19_lavender_field_horizon"
    },
    {
        "query": "task: search result | query: snow covered evergreen pine tree in winter",
        "target_base": "20_snowy_pine_tree"
    }
]

def main():
    IMAGES_DIR.mkdir(parents=True, exist_ok=True)
    manifest = {
        "bases": [],
        "near_duplicates": [],
        "distractors": [],
        "queries": QUERIES
    }

    base_images = build_base_images()
    print(f"Generated {len(base_images)} base images.")
    for name, img in base_images.items():
        out_path = IMAGES_DIR / f"{name}.png"
        img.save(out_path)
        manifest["bases"].append(name)

    distractor_images = build_distractor_pairs()
    print(f"Generated {len(distractor_images)} distractor images.")
    for name, img in distractor_images.items():
        out_path = IMAGES_DIR / f"{name}.png"
        img.save(out_path)
        manifest["distractors"].append(name)

    variants = generate_near_duplicates(base_images)
    print(f"Generated {len(variants)} near-duplicate variants.")
    for name, val in variants.items():
        if isinstance(val, tuple) and val[0] == "jpeg":
            _, q, base_img = val
            out_path = IMAGES_DIR / f"{name}.jpg"
            base_img.save(out_path, "JPEG", quality=q)
        else:
            out_path = IMAGES_DIR / f"{name}.png"
            val.save(out_path)
        manifest["near_duplicates"].append(name)

    manifest_path = FIXTURES_DIR / "manifest.json"
    with open(manifest_path, "w") as f:
        json.dump(manifest, f, indent=2)

    total_images = len(manifest["bases"]) + len(manifest["distractors"]) + len(manifest["near_duplicates"])
    print(f"Done! Total fixture images: {total_images}. Manifest written to {manifest_path}")

if __name__ == "__main__":
    main()

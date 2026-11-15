#!/usr/bin/env python3
"""
generate_parity_data.py

Generates committed, synthetic-only parity reference artifacts under
`spikes/embeddings/parity/` for TASK-0B2:
  1. Synthetic aspect-ratio edge-case images under `parity/images/`
  2. Token IDs (`tokens_ref.json`) for 20 fixture queries + edge cases
     (query and document forms, with max_length=8192 truncation)
  3. Preprocessed `pixel_values` and `pixel_position_ids` tensors
     (zlib-compressed little-endian f32/i64) for 22 diverse images at
     budgets 140 and 280 (`tensors_index.json` + `tensors/`)
  4. Final 768-d vectors as raw little-endian `.f32` files + `vectors_index.json`
     from both Python `onnxruntime` Q8 ("same-file reference") and PyTorch fp32
     ("fp32 reference") at budgets 140 and 280.

Usage:
  python spikes/embeddings/generate_parity_data.py
"""

import argparse
import hashlib
import json
import math
import zlib
from pathlib import Path

import numpy as np
import onnxruntime as ort
import torch
from PIL import Image, ImageDraw
from transformers import AutoModel, AutoProcessor, AutoTokenizer

MAX_SEQ_LENGTH = 8192
PADDING_SIDE = "right"


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def generate_aspect_images(out_dir: Path) -> list[Path]:
    """Generate 10 deterministic synthetic PNG images covering diverse aspect ratios."""
    out_dir.mkdir(parents=True, exist_ok=True)
    specs = [
        ("aspect_01_landscape_16x9.png", 640, 360, (40, 120, 210), (240, 180, 50)),
        ("aspect_02_landscape_3x2.png", 600, 400, (30, 160, 90), (220, 60, 60)),
        ("aspect_03_portrait_9x16.png", 360, 640, (130, 50, 170), (250, 220, 80)),
        ("aspect_04_portrait_2x3.png", 400, 600, (200, 90, 40), (40, 200, 220)),
        ("aspect_05_tiny_16x16.png", 16, 16, (255, 0, 128), (0, 255, 128)),
        ("aspect_06_tiny_32x24.png", 32, 24, (20, 40, 80), (240, 200, 100)),
        ("aspect_07_exact_144x96.png", 144, 96, (90, 190, 190), (190, 40, 90)),
        ("aspect_08_ultrawide_2000x20.png", 2000, 20, (15, 80, 150), (255, 210, 40)),
        ("aspect_09_ultratall_20x2000.png", 20, 2000, (150, 40, 80), (60, 230, 180)),
        ("aspect_10_panoramic_1600x100.png", 1600, 100, (70, 110, 50), (240, 120, 20)),
    ]
    paths = []
    for name, w, h, bg, fg in specs:
        img = Image.new("RGB", (w, h), bg)
        draw = ImageDraw.Draw(img)
        # Draw deterministic diagonal bands and centered rectangle for visual structure
        draw.rectangle([w // 4, h // 4, (3 * w) // 4, (3 * h) // 4], fill=fg)
        draw.line([(0, 0), (w - 1, h - 1)], fill=(255, 255, 255), width=max(1, min(w, h) // 16))
        p = out_dir / name
        img.save(p, format="PNG")
        paths.append(p)
    return paths


def build_text_cases(manifest: dict) -> list[dict]:
    """Build text parity set: 20 fixture queries + edge cases in query & document forms."""
    cases = []

    for idx, q in enumerate(manifest["queries"]):
        raw_query = q["query"]
        # raw_query already starts with "task: search result | query: "
        suffix = raw_query.split("query: ", 1)[1]
        cases.append({
            "id": f"fixture_query_{idx:02d}",
            "kind": "query",
            "text": raw_query,
            "include_in_vectors": True,
        })
        cases.append({
            "id": f"fixture_doc_{idx:02d}",
            "kind": "document",
            "text": f"title: none | text: {suffix}",
            "include_in_vectors": False,
        })

    edge_raws = [
        ("empty", ""),
        ("whitespace_only", "   "),
        ("leading_trailing_ws", "   red circle on blue background   "),
        ("accented_latin", "café résumé naïve façade Zürich jalapeño"),
        ("devanagari_hindi", "हिमालय की बर्फीली चोटियाँ और सूर्योदय"),
        ("cjk", "東京の夜景と桜の花びら"),
        ("emoji", "🌅🏔️📸🐕☕"),
        ("mixed_unicode", "Photo 2026: 東京 café नमस्ते 📸"),
        ("prefix_only_query", "task: search result | query: "),
        ("prefix_only_doc", "title: none | text: "),
        ("titled_doc", "title: Sunset Photo | text: orange sky over ocean waves"),
    ]
    for name, raw in edge_raws:
        if name in ("prefix_only_query", "prefix_only_doc", "titled_doc"):
            cases.append({
                "id": f"edge_{name}",
                "kind": "query" if "query" in name else "document",
                "text": raw,
                "include_in_vectors": True,
            })
        else:
            cases.append({
                "id": f"edge_{name}_query",
                "kind": "query",
                "text": f"task: search result | query: {raw}",
                "include_in_vectors": True,
            })
            cases.append({
                "id": f"edge_{name}_doc",
                "kind": "document",
                "text": f"title: none | text: {raw}",
                "include_in_vectors": True,
            })

    # Very long string past 8,192-token context limit (tested for tokenization parity; excluded from slow ONNX vector run or run separately)
    very_long = ("sunset mountain lake reflection " * 2500).strip()
    cases.append({
        "id": "edge_very_long_query",
        "kind": "query",
        "text": f"task: search result | query: {very_long}",
        "include_in_vectors": False,
    })
    cases.append({
        "id": "edge_very_long_doc",
        "kind": "document",
        "text": f"title: none | text: {very_long}",
        "include_in_vectors": False,
    })
    return cases


def l2_normalize_rows(arr: np.ndarray) -> np.ndarray:
    norms = np.linalg.norm(arr, axis=-1, keepdims=True)
    norms[norms == 0] = 1.0
    return (arr / norms).astype(np.float32)


def main():
    parser = argparse.ArgumentParser(description="Generate parity reference data for TASK-0B2.")
    parser.add_argument(
        "--onnx-dir",
        type=Path,
        default=Path(__file__).resolve().parent / "models" / "onnx",
        help="Path to downloaded onnx-community/embeddinggemma-2-ONNX directory.",
    )
    parser.add_argument(
        "--fixtures-dir",
        type=Path,
        default=Path(__file__).resolve().parent / "fixtures",
        help="Path to synthetic fixtures directory.",
    )
    parser.add_argument(
        "--out-dir",
        type=Path,
        default=Path(__file__).resolve().parent / "parity",
        help="Output parity directory.",
    )
    args = parser.parse_args()

    out_dir: Path = args.out_dir
    tensors_dir = out_dir / "tensors"
    vectors_dir = out_dir / "vectors"
    aspect_dir = out_dir / "images"
    tensors_dir.mkdir(parents=True, exist_ok=True)
    vectors_dir.mkdir(parents=True, exist_ok=True)

    with open(args.fixtures_dir / "manifest.json", "r", encoding="utf-8") as f:
        manifest = json.load(f)

    print("1. Generating synthetic aspect-ratio images...")
    aspect_paths = generate_aspect_images(aspect_dir)

    print("2. Loading HuggingFace tokenizer and processor...")
    tokenizer = AutoTokenizer.from_pretrained(str(args.onnx_dir), local_files_only=True)
    processor = AutoProcessor.from_pretrained(str(args.onnx_dir), local_files_only=True)

    print("3. Generating token parity reference (tokens_ref.json)...")
    text_cases = build_text_cases(manifest)
    token_records = []
    for case in text_cases:
        tok_out = tokenizer(
            case["text"],
            truncation=True,
            max_length=MAX_SEQ_LENGTH,
            return_tensors="np",
        )
        ids = tok_out["input_ids"][0].astype(int).tolist()
        mask = tok_out["attention_mask"][0].astype(int).tolist()
        token_records.append({
            "id": case["id"],
            "kind": case["kind"],
            "text": case["text"],
            "input_ids": ids,
            "attention_mask": mask,
            "include_in_vectors": case["include_in_vectors"],
        })

    tokens_ref = {
        "max_length": MAX_SEQ_LENGTH,
        "padding_side": PADDING_SIDE,
        "cases": token_records,
    }
    with open(out_dir / "tokens_ref.json", "w", encoding="utf-8") as f:
        json.dump(tokens_ref, f, ensure_ascii=False, separators=(",", ":"))
    print(f"   Wrote {len(token_records)} token test cases.")

    # Select 20 diverse images for tensor parity (10 aspect + 10 from fixtures)
    fixture_imgs = sorted(
        list((args.fixtures_dir / "images").glob("*.png"))
        + list((args.fixtures_dir / "images").glob("*.jpg"))
    )
    selected_tensor_paths = aspect_paths + [
        args.fixtures_dir / "images" / "01_red_circle_on_blue.png",
        args.fixtures_dir / "images" / "02_green_triangles_grid.png",
        args.fixtures_dir / "images" / "03_yellow_star_on_purple.png",
        args.fixtures_dir / "images" / "04_cyan_magenta_gradient.png",
        args.fixtures_dir / "images" / "06_sunset_beach_banner.png",
        args.fixtures_dir / "images" / "09_dog_silhouette_grass.png",
        args.fixtures_dir / "images" / "11_mountain_peaks_stars.png",
        args.fixtures_dir / "images" / "14_skyscraper_night_windows.png",
        args.fixtures_dir / "images" / "16_vintage_camera.png",
        args.fixtures_dir / "images" / "01_red_circle_on_blue__rot90.png",
    ]
    assert len(selected_tensor_paths) == 20

    repo_root = Path(__file__).resolve().parent.parent.parent
    print("4. Generating preprocessed tensor reference (20 images x budgets 140, 280)...")
    tensor_entries = []
    for img_path in selected_tensor_paths:
        rel_path = img_path.relative_to(repo_root).as_posix()
        stem = img_path.stem
        im = Image.open(img_path).convert("RGB")
        orig_w, orig_h = im.size
        for budget in (140, 280):
            proc_out = processor(images=im, return_tensors="np", max_soft_tokens=budget)
            pv = np.ascontiguousarray(proc_out["pixel_values"][0], dtype="<f4")
            pid = np.ascontiguousarray(proc_out["image_position_ids"][0], dtype="<i8")
            inp_ids = proc_out["input_ids"][0].astype(int).tolist()
            num_soft = sum(1 for x in inp_ids if x == 258880)
            valid_patches = int(np.sum(pid[:, 0] != -1))

            pv_bytes = pv.tobytes()
            pid_bytes = pid.tobytes()
            pv_u8 = np.rint(pv * 255.0).astype(np.uint8)
            assert np.array_equal(pv, pv_u8.astype(np.float32) * np.float32(1.0 / 255.0))
            pv_file = f"{stem}_b{budget}_pv.u8.zlib"
            pid_file = f"{stem}_b{budget}_pid.i64.zlib"
            (tensors_dir / pv_file).write_bytes(zlib.compress(pv_u8.tobytes(), level=9))
            (tensors_dir / pid_file).write_bytes(zlib.compress(pid_bytes, level=9))

            tensor_entries.append({
                "key": f"{stem}_b{budget}",
                "image_name": img_path.name,
                "image_rel_path": rel_path,
                "orig_width": orig_w,
                "orig_height": orig_h,
                "budget": budget,
                "max_patches": int(pv.shape[0]),
                "valid_patches": valid_patches,
                "num_soft_tokens": num_soft,
                "pixel_values_shape": list(pv.shape),
                "pixel_values_encoding": "u8_times_inv255_f32_le",
                "pixel_values_sha256": sha256_bytes(pv_bytes),
                "pixel_values_file": f"tensors/{pv_file}",
                "pixel_position_ids_shape": list(pid.shape),
                "pixel_position_ids_sha256": sha256_bytes(pid_bytes),
                "pixel_position_ids_file": f"tensors/{pid_file}",
                "input_ids": inp_ids,
            })

    with open(out_dir / "tensors_index.json", "w", encoding="utf-8") as f:
        json.dump({"entries": tensor_entries}, f, ensure_ascii=False, separators=(",", ":"))
    print(f"   Wrote {len(tensor_entries)} tensor entries.")

    # All images for vector parity: 185 fixtures + 10 aspect = 195 images
    all_image_paths = fixture_imgs + aspect_paths
    vector_text_cases = [r for r in token_records if r["include_in_vectors"]]
    print(f"5. Computing Python ONNX Q8 vectors ({len(vector_text_cases)} texts, {len(all_image_paths)} images)...")

    opts = ort.SessionOptions()
    opts.intra_op_num_threads = 4
    opts.inter_op_num_threads = 1
    sess_text = ort.InferenceSession(
        str(args.onnx_dir / "onnx" / "model_quantized.onnx"),
        sess_options=opts,
        providers=["CPUExecutionProvider"],
    )
    sess_vision = ort.InferenceSession(
        str(args.onnx_dir / "onnx" / "vision_encoder_quantized.onnx"),
        sess_options=opts,
        providers=["CPUExecutionProvider"],
    )
    empty_feats = np.zeros((0, 512), dtype=np.float32)

    q8_text_vecs = []
    for rec in vector_text_cases:
        ids = np.array([rec["input_ids"]], dtype=np.int64)
        mask = np.array([rec["attention_mask"]], dtype=np.int64)
        out = sess_text.run(
            ["sentence_embedding"],
            {
                "input_ids": ids,
                "attention_mask": mask,
                "image_features": empty_feats,
                "video_features": empty_feats,
                "audio_features": empty_feats,
            },
        )[0][0]
        q8_text_vecs.append(out)
    q8_text_arr = l2_normalize_rows(np.stack(q8_text_vecs))
    (vectors_dir / "q8_text.f32").write_bytes(np.ascontiguousarray(q8_text_arr, dtype="<f4").tobytes())

    q8_img_vecs = {140: [], 280: []}
    for img_path in all_image_paths:
        im = Image.open(img_path).convert("RGB")
        for budget in (140, 280):
            inp = processor(images=im, return_tensors="np", max_soft_tokens=budget)
            img_feats = sess_vision.run(
                ["image_features"],
                {
                    "pixel_values": inp["pixel_values"].astype(np.float32),
                    "pixel_position_ids": inp["image_position_ids"].astype(np.int64),
                },
            )[0]
            out = sess_text.run(
                ["sentence_embedding"],
                {
                    "input_ids": inp["input_ids"].astype(np.int64),
                    "attention_mask": inp["attention_mask"].astype(np.int64),
                    "image_features": img_feats,
                    "video_features": empty_feats,
                    "audio_features": empty_feats,
                },
            )[0][0]
            q8_img_vecs[budget].append(out)

    for budget in (140, 280):
        arr = l2_normalize_rows(np.stack(q8_img_vecs[budget]))
        (vectors_dir / f"q8_b{budget}.f32").write_bytes(np.ascontiguousarray(arr, dtype="<f4").tobytes())

    del sess_text, sess_vision

    print("6. Computing PyTorch fp32 reference vectors...")
    am = AutoModel.from_pretrained(
        "google/embeddinggemma-2",
        local_files_only=True,
        dtype=torch.float32,
        audio_config=None,
    )
    am.eval()

    fp32_text_vecs = []
    with torch.no_grad():
        for rec in vector_text_cases:
            inp = {
                "input_ids": torch.tensor([rec["input_ids"]], dtype=torch.long),
                "attention_mask": torch.tensor([rec["attention_mask"]], dtype=torch.long),
            }
            out = am(**inp)
            mask = inp["attention_mask"].unsqueeze(-1).float()
            pooled = (out.last_hidden_state * mask).sum(dim=1) / mask.sum(dim=1)
            normed = torch.nn.functional.normalize(pooled, p=2, dim=-1)[0].numpy()
            fp32_text_vecs.append(normed)
    fp32_text_arr = l2_normalize_rows(np.stack(fp32_text_vecs))
    (vectors_dir / "fp32_text.f32").write_bytes(np.ascontiguousarray(fp32_text_arr, dtype="<f4").tobytes())

    fp32_img_vecs = {140: [], 280: []}
    with torch.no_grad():
        for idx, img_path in enumerate(all_image_paths):
            if idx % 25 == 0:
                print(f"   fp32 image {idx}/{len(all_image_paths)}...")
            im = Image.open(img_path).convert("RGB")
            for budget in (140, 280):
                inp = processor(images=im, return_tensors="pt", max_soft_tokens=budget)
                out = am(**inp)
                mask = inp["attention_mask"].unsqueeze(-1).float()
                pooled = (out.last_hidden_state * mask).sum(dim=1) / mask.sum(dim=1)
                normed = torch.nn.functional.normalize(pooled, p=2, dim=-1)[0].numpy()
                fp32_img_vecs[budget].append(normed)

    for budget in (140, 280):
        arr = l2_normalize_rows(np.stack(fp32_img_vecs[budget]))
        (vectors_dir / f"fp32_b{budget}.f32").write_bytes(np.ascontiguousarray(arr, dtype="<f4").tobytes())

    vectors_index = {
        "dim": 768,
        "dtype": "f32_le",
        "text_ids": [r["id"] for r in vector_text_cases],
        "texts": [r["text"] for r in vector_text_cases],
        "image_names": [p.name for p in all_image_paths],
        "image_rel_paths": [p.relative_to(repo_root).as_posix() for p in all_image_paths],
        "files": {
            "q8_text": "vectors/q8_text.f32",
            "q8_b140": "vectors/q8_b140.f32",
            "q8_b280": "vectors/q8_b280.f32",
            "fp32_text": "vectors/fp32_text.f32",
            "fp32_b140": "vectors/fp32_b140.f32",
            "fp32_b280": "vectors/fp32_b280.f32",
        },
    }
    with open(out_dir / "vectors_index.json", "w", encoding="utf-8") as f:
        json.dump(vectors_index, f, ensure_ascii=False, separators=(",", ":"))

    total_bytes = sum(p.stat().st_size for p in out_dir.rglob("*") if p.is_file())
    print(f"Done! Total size of {out_dir}: {total_bytes / (1024 * 1024):.2f} MiB ({total_bytes} B)")


if __name__ == "__main__":
    main()

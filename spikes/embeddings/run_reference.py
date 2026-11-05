#!/usr/bin/env python3
"""
run_reference.py

Generates reference 768-d embeddings using the official SentenceTransformer
implementation of google/embeddinggemma-2 in fp32 on CPU (R4).
Text inputs receive the documented task prefixes; image inputs receive no prefix.
"""

import argparse
import json
import os
import sys
import time
from pathlib import Path

import numpy as np
import torch
from PIL import Image
from sentence_transformers import SentenceTransformer

def parse_args():
    parser = argparse.ArgumentParser(
        description="Generate reference 768-d vectors for the EmbeddingGemma 2 spike."
    )
    parser.add_argument(
        "--fixtures-dir",
        type=Path,
        default=Path(__file__).resolve().parent / "fixtures",
        help="Path to directory containing manifest.json and images/.",
    )
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=Path(__file__).resolve().parent / "output",
        help="Path where reference embeddings will be saved.",
    )
    parser.add_argument(
        "--model-id",
        type=str,
        default="google/embeddinggemma-2",
        help="HuggingFace model ID or local path.",
    )
    parser.add_argument(
        "--device",
        type=str,
        default="cpu",
        help="Computation device ('cpu' or 'mps'). Default: cpu.",
    )
    parser.add_argument(
        "--precision",
        type=str,
        choices=["fp32", "bf16"],
        default="fp32",
        help="Precision for reference vectors (fp32 recommended on CPU; never fp16).",
    )
    parser.add_argument(
        "--batch-size",
        type=int,
        default=1,
        help="Batch size for embedding generation.",
    )
    return parser.parse_args()

def main():
    args = parse_args()
    args.output_dir.mkdir(parents=True, exist_ok=True)

    manifest_path = args.fixtures_dir / "manifest.json"
    if not manifest_path.exists():
        print(f"Error: manifest.json not found at {manifest_path}. Run generate_fixtures.py first.")
        sys.exit(1)

    with open(manifest_path, "r") as f:
        manifest = json.load(f)

    dtype = torch.bfloat16 if args.precision == "bf16" else torch.float32
    print(f"Loading {args.model_id} on {args.device} in {args.precision}...")
    t0 = time.perf_counter()
    model = SentenceTransformer(
        args.model_id,
        device=args.device,
        model_kwargs={"torch_dtype": dtype},
        config_kwargs={"audio_config": None},
    )
    t_load = time.perf_counter() - t0
    print(f"Model loaded in {t_load:.2f}s.")

    # 1. Encode text queries
    queries_data = manifest.get("queries", [])
    query_texts = [q["query"] for q in queries_data]
    print(f"Encoding {len(query_texts)} text queries...")
    t_text_start = time.perf_counter()
    # Normalize is True per spec §9.1
    query_embs = model.encode(
        query_texts,
        batch_size=args.batch_size,
        normalize_embeddings=True,
        show_progress_bar=False,
    )
    t_text_total = time.perf_counter() - t_text_start
    print(f"Encoded {len(query_texts)} queries in {t_text_total:.2f}s ({t_text_total/max(1, len(query_texts))*1000:.1f}ms/query).")

    # 2. Encode fixture images
    images_dir = args.fixtures_dir / "images"
    image_files = sorted(list(images_dir.glob("*.png")) + list(images_dir.glob("*.jpg")))
    print(f"Encoding {len(image_files)} fixture images...")
    image_embs_dict = {}
    image_times = []
    for img_path in image_files:
        t_img_start = time.perf_counter()
        img = Image.open(img_path).convert("RGB")
        emb = model.encode(img, normalize_embeddings=True, show_progress_bar=False)
        t_img_elapsed = time.perf_counter() - t_img_start
        image_times.append(t_img_elapsed)
        image_embs_dict[img_path.name] = np.asarray(emb, dtype=np.float32)

    avg_img_ms = (sum(image_times) / max(1, len(image_times))) * 1000
    print(f"Encoded {len(image_files)} images in {sum(image_times):.2f}s (avg {avg_img_ms:.1f}ms/image).")

    # Save to npz
    out_file = args.output_dir / "reference_vectors.npz"
    save_dict = {
        "query_keys": np.array([q["target_base"] for q in queries_data]),
        "query_texts": np.array(query_texts),
        "query_embeddings": np.asarray(query_embs, dtype=np.float32),
        "image_keys": np.array(list(image_embs_dict.keys())),
        "image_embeddings": np.stack(list(image_embs_dict.values())),
    }
    np.savez_compressed(out_file, **save_dict)

    # Save metadata summary
    meta = {
        "model_id": args.model_id,
        "device": args.device,
        "precision": args.precision,
        "load_time_s": t_load,
        "num_queries": len(query_texts),
        "query_avg_ms": (t_text_total / max(1, len(query_texts))) * 1000,
        "num_images": len(image_files),
        "image_avg_ms": avg_img_ms,
        "dim": int(query_embs.shape[1]),
    }
    with open(args.output_dir / "reference_metadata.json", "w") as f:
        json.dump(meta, f, indent=2)

    print(f"Reference vectors successfully saved to {out_file}")

if __name__ == "__main__":
    main()

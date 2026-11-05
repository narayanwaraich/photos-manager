#!/usr/bin/env python3
"""
eval_onnx.py

Comprehensive evaluation and measurement script for EmbeddingGemma 2 ONNX models:
- Tests Q8 (quantized), Q4, and FP16 models.
- Tests vision token budget 280 vs 140.
- Tests preprocessing parity ablation (deliberate wrong normalization).
- Measures:
    * Model load times (text backbone vs vision encoder)
    * Resident memory (RSS) before and after on-demand vision encoder load
    * Per-text and per-image inference latencies and throughput
    * Cosine similarity vs reference for identical inputs (min, mean, max)
    * MRL retrieval accuracy (Top-1, Top-5) at 768, 512, 256, 128 d
    * Near-duplicate precision/recall across cosine thresholds
    * Distractor pair scores
"""

import argparse
import gc
import json
import os
import resource
import sys
import time
from pathlib import Path

import numpy as np
import onnxruntime as ort
import torch
from PIL import Image
from transformers import AutoProcessor, AutoTokenizer

from compare import (
    compute_cosine_stats,
    evaluate_distractors,
    evaluate_near_duplicates,
    evaluate_retrieval,
    l2_normalize,
    truncate_and_normalize,
)

def get_rss_mb():
    # macOS ru_maxrss is in bytes
    rusage = resource.getrusage(resource.RUSAGE_SELF)
    return rusage.ru_maxrss / (1024.0 * 1024.0)

class OnnxEmbedder:
    def __init__(self, model_path, vision_path, token_budget=280):
        self.model_path = model_path
        self.vision_path = vision_path
        self.token_budget = token_budget
        
        # Configure session options: CPU EP, thread counts
        opts = ort.SessionOptions()
        opts.intra_op_num_threads = 4
        opts.inter_op_num_threads = 1
        opts.execution_mode = ort.ExecutionMode.ORT_SEQUENTIAL
        self.opts = opts
        
        # Load text model first
        t0 = time.perf_counter()
        self.sess_model = ort.InferenceSession(
            str(model_path), sess_options=opts, providers=["CPUExecutionProvider"]
        )
        self.text_load_time_s = time.perf_counter() - t0
        self.sess_vision = None
        self.vision_load_time_s = None

    def load_vision_encoder(self):
        if self.sess_vision is None:
            t0 = time.perf_counter()
            self.sess_vision = ort.InferenceSession(
                str(self.vision_path), sess_options=self.opts, providers=["CPUExecutionProvider"]
            )
            self.vision_load_time_s = time.perf_counter() - t0
        return self.vision_load_time_s

    def unload_vision_encoder(self):
        self.sess_vision = None
        gc.collect()

    def embed_text(self, text, tokenizer):
        tok = tokenizer(text, return_tensors="np")
        input_ids = tok["input_ids"].astype(np.int64)
        attn_mask = tok["attention_mask"].astype(np.int64)
        empty_feats = np.zeros((0, 512), dtype=np.float32)

        feeds = {
            "input_ids": input_ids,
            "attention_mask": attn_mask,
            "image_features": empty_feats,
            "video_features": empty_feats,
            "audio_features": empty_feats,
        }
        out = self.sess_model.run(["sentence_embedding"], feeds)
        emb = out[0][0]
        norm = np.linalg.norm(emb)
        if norm > 0:
            emb = emb / norm
        return emb

    def embed_image(self, img_pil, processor, wrong_norm=False):
        self.load_vision_encoder()
        
        # Run processor
        inputs = processor(
            images=img_pil,
            return_tensors="np",
            max_soft_tokens=self.token_budget,
        )
        pixel_values = inputs["pixel_values"].astype(np.float32)
        position_ids = inputs["image_position_ids"].astype(np.int64)

        if wrong_norm:
            # Deliberate wrong preprocessing: ImageNet mean/std on [0, 1] pixels
            mean = np.array([0.485, 0.456, 0.406], dtype=np.float32)
            std = np.array([0.229, 0.224, 0.225], dtype=np.float32)
            # pixel_values shape is (1, max_patches, 768) where 768 is 16*16*3
            # We reshape to (1, max_patches, 16, 16, 3) and apply wrong norm
            orig_shape = pixel_values.shape
            pv = pixel_values.reshape(1, orig_shape[1], 16, 16, 3)
            pv = (pv - mean) / std
            pixel_values = pv.reshape(orig_shape)

        v_feeds = {
            "pixel_values": pixel_values,
            "pixel_position_ids": position_ids,
        }
        v_out = self.sess_vision.run(["image_features"], v_feeds)
        img_feats = v_out[0]

        empty_feats = np.zeros((0, 512), dtype=np.float32)
        m_feeds = {
            "input_ids": inputs["input_ids"].astype(np.int64),
            "attention_mask": inputs["attention_mask"].astype(np.int64),
            "image_features": img_feats,
            "video_features": empty_feats,
            "audio_features": empty_feats,
        }
        m_out = self.sess_model.run(["sentence_embedding"], m_feeds)
        emb = m_out[0][0]
        norm = np.linalg.norm(emb)
        if norm > 0:
            emb = emb / norm
        return emb

def run_evaluation_variant(
    name,
    model_path,
    vision_path,
    token_budget,
    wrong_norm,
    manifest,
    images_dir,
    tokenizer,
    processor,
    ref_data,
    output_dir,
):
    print(f"\n==================================================")
    print(f"RUNNING EVALUATION: {name}")
    print(f"==================================================")

    gc.collect()
    rss_before = get_rss_mb()
    embedder = OnnxEmbedder(model_path, vision_path, token_budget=token_budget)
    rss_text = get_rss_mb()
    print(f"Text model loaded in {embedder.text_load_time_s*1000:.1f}ms. RSS: {rss_text:.1f} MB (Delta: {rss_text - rss_before:.1f} MB)")

    # 1. Benchmark Text Queries
    queries_data = manifest.get("queries", [])
    query_texts = [q["query"] for q in queries_data]
    query_keys = [q["target_base"] for q in queries_data]

    print(f"Encoding {len(query_texts)} text queries...")
    text_times = []
    query_embs = []
    for q in query_texts:
        t0 = time.perf_counter()
        emb = embedder.embed_text(q, tokenizer)
        text_times.append(time.perf_counter() - t0)
        query_embs.append(emb)

    query_embs = np.array(query_embs, dtype=np.float32)
    avg_text_ms = (sum(text_times) / max(1, len(text_times))) * 1000.0
    print(f"Text encoding avg latency: {avg_text_ms:.1f}ms/query.")

    # 2. Benchmark Vision Encoder On-Demand Loading
    t_vload = embedder.load_vision_encoder()
    rss_vision = get_rss_mb()
    vision_delta_mb = rss_vision - rss_text
    print(f"Vision encoder loaded in {t_vload*1000:.1f}ms. RSS: {rss_vision:.1f} MB (Vision Delta: {vision_delta_mb:.1f} MB)")

    # 3. Benchmark Fixture Images
    image_files = sorted(list(images_dir.glob("*.png")) + list(images_dir.glob("*.jpg")))
    print(f"Encoding {len(image_files)} fixture images (budget={token_budget}, wrong_norm={wrong_norm})...")

    img_times = []
    image_embs_dict = {}
    for img_p in image_files:
        img = Image.open(img_p).convert("RGB")
        t0 = time.perf_counter()
        emb = embedder.embed_image(img, processor, wrong_norm=wrong_norm)
        img_times.append(time.perf_counter() - t0)
        image_embs_dict[img_p.name] = emb

    image_keys = list(image_embs_dict.keys())
    image_embs = np.stack(list(image_embs_dict.values())).astype(np.float32)

    total_img_time = sum(img_times)
    avg_img_ms = (total_img_time / max(1, len(img_times))) * 1000.0
    throughput = len(img_times) / max(1e-4, total_img_time)
    print(f"Image encoding: {len(img_times)} images in {total_img_time:.2f}s ({avg_img_ms:.1f}ms/img, {throughput:.2f} img/s).")

    # Save candidate npz
    out_npz = output_dir / f"onnx_{name}.npz"
    np.savez_compressed(
        out_npz,
        query_keys=np.array(query_keys),
        query_embeddings=query_embs,
        image_keys=np.array(image_keys),
        image_embeddings=image_embs,
    )

    # 4. Compare vs Reference
    ref_q_embs = l2_normalize(ref_data["query_embeddings"])
    ref_img_embs = l2_normalize(ref_data["image_embeddings"])
    cand_q_embs = l2_normalize(query_embs)
    cand_img_embs = l2_normalize(image_embs)

    text_cos = compute_cosine_stats(ref_q_embs, cand_q_embs)
    img_cos = compute_cosine_stats(ref_img_embs, cand_img_embs)
    print(f"Cosine vs Reference:")
    print(f"  Text:  Mean={text_cos['mean']:.5f}, Min={text_cos['min']:.5f}, Max={text_cos['max']:.5f}")
    print(f"  Image: Mean={img_cos['mean']:.5f}, Min={img_cos['min']:.5f}, Max={img_cos['max']:.5f}")

    # 5. Retrieval across dims
    retrieval = {}
    for dim in [768, 512, 256, 128]:
        t_q = truncate_and_normalize(cand_q_embs, dim)
        t_img = truncate_and_normalize(cand_img_embs, dim)
        res = evaluate_retrieval(t_q, query_keys, t_img, image_keys, manifest)
        retrieval[f"dim_{dim}"] = res
        print(f"  dim={dim:3d} | Top-1: {res['top1_acc']*100:5.1f}% | Top-5: {res['top5_acc']*100:5.1f}%")

    # 6. Near-duplicate detection
    near_dups = evaluate_near_duplicates(
        truncate_and_normalize(cand_img_embs, 256),
        image_keys,
        manifest,
        [0.80, 0.85, 0.90, 0.92, 0.95],
    )

    # 7. Distractors
    distractors = evaluate_distractors(
        truncate_and_normalize(cand_img_embs, 256),
        image_keys,
        manifest,
    )

    return {
        "name": name,
        "token_budget": token_budget,
        "wrong_norm": wrong_norm,
        "text_load_time_ms": embedder.text_load_time_s * 1000.0,
        "vision_load_time_ms": embedder.vision_load_time_s * 1000.0,
        "rss_before_mb": rss_before,
        "rss_text_mb": rss_text,
        "rss_vision_mb": rss_vision,
        "rss_vision_delta_mb": vision_delta_mb,
        "query_avg_ms": avg_text_ms,
        "image_avg_ms": avg_img_ms,
        "image_throughput_fps": throughput,
        "text_cosine_vs_reference": text_cos,
        "image_cosine_vs_reference": img_cos,
        "retrieval_across_dims": retrieval,
        "near_duplicate_detection_256d": near_dups,
        "distractor_scores_256d": distractors,
    }

def main():
    root = Path(__file__).resolve().parent
    fixtures_dir = root / "fixtures"
    manifest_path = fixtures_dir / "manifest.json"
    images_dir = fixtures_dir / "images"
    models_dir = root / "models" / "onnx" / "onnx"
    output_dir = root / "output"
    output_dir.mkdir(parents=True, exist_ok=True)

    with open(manifest_path, "r") as f:
        manifest = json.load(f)

    ref_data = np.load(output_dir / "reference_vectors.npz")

    print("Loading HuggingFace tokenizer and processor...")
    tokenizer = AutoTokenizer.from_pretrained("google/embeddinggemma-2", local_files_only=True)
    processor = AutoProcessor.from_pretrained("google/embeddinggemma-2", local_files_only=True)

    variants = [
        {
            "name": "q8_b280",
            "model_path": models_dir / "model_quantized.onnx",
            "vision_path": models_dir / "vision_encoder_quantized.onnx",
            "token_budget": 280,
            "wrong_norm": False,
        },
        {
            "name": "q4_b280",
            "model_path": models_dir / "model_q4.onnx",
            "vision_path": models_dir / "vision_encoder_q4.onnx",
            "token_budget": 280,
            "wrong_norm": False,
        },
        {
            "name": "fp16_b280",
            "model_path": models_dir / "model_fp16.onnx",
            "vision_path": models_dir / "vision_encoder_fp16.onnx",
            "token_budget": 280,
            "wrong_norm": False,
        },
        {
            "name": "q8_b140",
            "model_path": models_dir / "model_quantized.onnx",
            "vision_path": models_dir / "vision_encoder_quantized.onnx",
            "token_budget": 140,
            "wrong_norm": False,
        },
        {
            "name": "q8_wrong_norm",
            "model_path": models_dir / "model_quantized.onnx",
            "vision_path": models_dir / "vision_encoder_quantized.onnx",
            "token_budget": 280,
            "wrong_norm": True,
        },
    ]

    all_results = {}
    for var in variants:
        res = run_evaluation_variant(
            name=var["name"],
            model_path=var["model_path"],
            vision_path=var["vision_path"],
            token_budget=var["token_budget"],
            wrong_norm=var["wrong_norm"],
            manifest=manifest,
            images_dir=images_dir,
            tokenizer=tokenizer,
            processor=processor,
            ref_data=ref_data,
            output_dir=output_dir,
        )
        all_results[var["name"]] = res

    # Save summary report
    summary_path = output_dir / "benchmark_summary.json"
    with open(summary_path, "w") as f:
        json.dump(all_results, f, indent=2)

    print(f"\nAll benchmark variants complete! Summary saved to {summary_path}")

if __name__ == "__main__":
    main()

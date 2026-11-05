#!/usr/bin/env python3
"""
compare.py

Computes quality metrics comparing candidate embeddings to reference embeddings:
- Cosine similarity vs reference for identical inputs (text and image: min, mean, max).
- Retrieval accuracy (Top-1 and Top-5) for text-to-image queries across MRL dimensions (768, 512, 256, 128).
- Near-duplicate precision, recall, and F1 across multiple cosine thresholds (0.80, 0.85, 0.90, 0.92, 0.95).
- Distractor pair separation scores.
"""

import argparse
import json
import sys
from pathlib import Path
import numpy as np

def parse_args():
    parser = argparse.ArgumentParser(
        description="Compare candidate embeddings against reference embeddings."
    )
    parser.add_argument(
        "--reference",
        type=Path,
        required=True,
        help="Path to reference .npz file (from run_reference.py).",
    )
    parser.add_argument(
        "--candidate",
        type=Path,
        required=True,
        help="Path to candidate .npz file.",
    )
    parser.add_argument(
        "--manifest",
        type=Path,
        default=Path(__file__).resolve().parent / "fixtures" / "manifest.json",
        help="Path to fixtures manifest.json.",
    )
    parser.add_argument(
        "--dims",
        type=int,
        nargs="+",
        default=[768, 512, 256, 128],
        help="MRL dimensions to evaluate (default: 768 512 256 128).",
    )
    parser.add_argument(
        "--thresholds",
        type=float,
        nargs="+",
        default=[0.80, 0.85, 0.90, 0.92, 0.95],
        help="Cosine thresholds for near-duplicate detection.",
    )
    parser.add_argument(
        "--output-json",
        type=Path,
        default=None,
        help="Optional path to output JSON metrics.",
    )
    return parser.parse_args()

def l2_normalize(vecs):
    norms = np.linalg.norm(vecs, axis=-1, keepdims=True)
    norms[norms == 0] = 1.0
    return vecs / norms

def truncate_and_normalize(vecs, dim):
    truncated = vecs[..., :dim]
    return l2_normalize(truncated)

def compute_cosine_stats(v1, v2):
    # assumes v1 and v2 are L2-normalized
    cosines = np.sum(v1 * v2, axis=-1)
    return {
        "min": float(np.min(cosines)),
        "mean": float(np.mean(cosines)),
        "max": float(np.max(cosines)),
        "std": float(np.std(cosines)),
    }

def evaluate_retrieval(query_embs, query_keys, image_embs, image_keys, manifest):
    """
    Evaluates text-to-image retrieval top-1 and top-5 accuracy.
    Matches query target_base against image base names.
    """
    # Create map from image index to base name
    # Image key can be e.g. "01_red_circle_on_blue.png" -> base is "01_red_circle_on_blue"
    img_bases = [Path(k).stem for k in image_keys]
    
    # Filter only base images for standard retrieval ranking
    base_indices = [i for i, b in enumerate(img_bases) if b in manifest.get("bases", [])]
    if not base_indices:
        base_indices = list(range(len(image_keys)))
    
    sub_image_embs = image_embs[base_indices]
    sub_bases = [img_bases[i] for i in base_indices]
    
    # Cosine similarities: (num_queries, num_bases)
    sims = query_embs @ sub_image_embs.T
    
    top1_correct = 0
    top5_correct = 0
    num_queries = len(query_keys)
    
    for q_idx in range(num_queries):
        target = query_keys[q_idx]
        ranked_indices = np.argsort(-sims[q_idx])
        ranked_bases = [sub_bases[i] for i in ranked_indices]
        if ranked_bases[0] == target:
            top1_correct += 1
        if target in ranked_bases[:5]:
            top5_correct += 1
            
    return {
        "num_queries": num_queries,
        "top1_acc": top1_correct / max(1, num_queries),
        "top5_acc": top5_correct / max(1, num_queries),
    }

def evaluate_near_duplicates(image_embs, image_keys, manifest, thresholds):
    """
    Evaluates near-duplicate precision, recall, and F1 at various cosine thresholds.
    Group-level ground truth: every pair of images in the same base group
    (base image + all variants in that group) is a positive.
    20 groups x (9 * 8 / 2 = 36 pairs) = 720 ground truth positive pairs.
    """
    stem_to_idx = {Path(k).stem: i for i, k in enumerate(image_keys)}
    bases = manifest.get("bases", [])
    near_dups = manifest.get("near_duplicates", [])
    
    # Group images by base: each group contains the base + its 8 variants (9 images total)
    groups = {b: [b] for b in bases}
    for nd in near_dups:
        base_name = nd.split("__")[0]
        if base_name in groups:
            groups[base_name].append(nd)

    tp_pairs = set()
    for base_name, members in groups.items():
        member_indices = [stem_to_idx[m] for m in members if m in stem_to_idx]
        for i in range(len(member_indices)):
            for j in range(i + 1, len(member_indices)):
                idx1 = min(member_indices[i], member_indices[j])
                idx2 = max(member_indices[i], member_indices[j])
                tp_pairs.add((idx1, idx2))
            
    # Pairwise cosine similarity matrix
    sim_matrix = image_embs @ image_embs.T
    n = len(image_keys)
    
    results = {}
    for th in thresholds:
        pred_pairs = set()
        for i in range(n):
            for j in range(i + 1, n):
                if sim_matrix[i, j] >= th:
                    pred_pairs.add((i, j))
                    
        hits = len(pred_pairs.intersection(tp_pairs))
        precision = hits / max(1, len(pred_pairs))
        recall = hits / max(1, len(tp_pairs))
        f1 = 2 * precision * recall / max(1e-8, precision + recall)
        results[f"th_{th:.2f}"] = {
            "threshold": th,
            "predicted_pairs": len(pred_pairs),
            "true_positive_hits": hits,
            "total_ground_truth_pairs": len(tp_pairs),
            "precision": float(precision),
            "recall": float(recall),
            "f1": float(f1),
        }
    return results

def evaluate_distractors(image_embs, image_keys, manifest):
    """
    Computes cosine similarity between each distractor image and its intended counterpart.
    Distractor pairs are visually similar shapes/colors but semantically different objects.
    """
    key_to_idx = {k: i for i, k in enumerate(image_keys)}
    distractor_map = {
        "distractor_01_red_square_on_blue": "01_red_circle_on_blue",
        "distractor_02_blue_horizontal_stripes": "08_blue_diagonal_stripes",
        "distractor_03_pyramid_stars": "11_mountain_peaks_stars",
        "distractor_04_red_tomato": "12_red_apple_leaf",
        "distractor_05_yellow_circle_on_purple": "03_yellow_star_on_purple",
    }
    scores = {}
    for d_name, b_name in distractor_map.items():
        d_key = next((k for k in image_keys if Path(k).stem == d_name), None)
        b_key = next((k for k in image_keys if Path(k).stem == b_name), None)
        if d_key and b_key:
            sim = float(image_embs[key_to_idx[d_key]] @ image_embs[key_to_idx[b_key]])
            scores[f"{b_name}__vs__{d_name}"] = sim
    return scores

def main():
    args = parse_args()

    ref_data = np.load(args.reference)
    cand_data = np.load(args.candidate)

    with open(args.manifest, "r") as f:
        manifest = json.load(f)

    # Validate keys match
    ref_q_keys = list(ref_data["query_keys"])
    cand_q_keys = list(cand_data["query_keys"])
    ref_img_keys = list(ref_data["image_keys"])
    cand_img_keys = list(cand_data["image_keys"])

    ref_q_embs = l2_normalize(ref_data["query_embeddings"])
    cand_q_embs = l2_normalize(cand_data["query_embeddings"])
    ref_img_embs = l2_normalize(ref_data["image_embeddings"])
    cand_img_embs = l2_normalize(cand_data["image_embeddings"])

    print("=" * 60)
    print("EMBEDDING COMPARISON REPORT")
    print("=" * 60)

    # 1. Parity cosine stats (Candidate vs Reference)
    text_cos = compute_cosine_stats(ref_q_embs, cand_q_embs)
    img_cos = compute_cosine_stats(ref_img_embs, cand_img_embs)
    print(f"Text Cosine vs Reference:  Mean={text_cos['mean']:.5f}, Min={text_cos['min']:.5f}, Max={text_cos['max']:.5f}")
    print(f"Image Cosine vs Reference: Mean={img_cos['mean']:.5f}, Min={img_cos['min']:.5f}, Max={img_cos['max']:.5f}")

    # 2. Retrieval accuracy across MRL dimensions
    retrieval_results = {}
    print("\nText-to-Image Retrieval Across MRL Dimensions (Candidate):")
    for dim in args.dims:
        t_q = truncate_and_normalize(cand_q_embs, dim)
        t_img = truncate_and_normalize(cand_img_embs, dim)
        res = evaluate_retrieval(t_q, cand_q_keys, t_img, cand_img_keys, manifest)
        retrieval_results[f"dim_{dim}"] = res
        print(f"  dim={dim:3d} | Top-1: {res['top1_acc']*100:5.1f}% | Top-5: {res['top5_acc']*100:5.1f}%")

    # 3. Near-duplicate precision/recall at default dimension
    default_dim = args.dims[0]
    t_img = truncate_and_normalize(cand_img_embs, default_dim)
    near_dup_results = evaluate_near_duplicates(t_img, cand_img_keys, manifest, args.thresholds)
    print(f"\nNear-Duplicate Detection (dim={default_dim}):")
    for th_key, res in near_dup_results.items():
        print(f"  Threshold {res['threshold']:.2f} | Precision: {res['precision']*100:5.1f}% | Recall: {res['recall']*100:5.1f}% | F1: {res['f1']:.3f}")

    # 4. Distractor scores
    distractor_scores = evaluate_distractors(t_img, cand_img_keys, manifest)
    print("\nDistractor Pair Cosine Scores (dim=default):")
    for pair, score in distractor_scores.items():
        print(f"  {pair}: {score:.4f}")

    all_metrics = {
        "text_cosine_vs_reference": text_cos,
        "image_cosine_vs_reference": img_cos,
        "retrieval_across_dims": retrieval_results,
        "near_duplicate_detection": near_dup_results,
        "distractor_scores": distractor_scores,
    }

    if args.output_json:
        args.output_json.parent.mkdir(parents=True, exist_ok=True)
        with open(args.output_json, "w") as f:
            json.dump(all_metrics, f, indent=2)
        print(f"\nDetailed metrics written to {args.output_json}")

if __name__ == "__main__":
    main()

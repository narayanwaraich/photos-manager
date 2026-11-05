# REPORT-0B: EmbeddingGemma 2 Runtime Spike

**Branch:** `task/0B` · **Commit:** `4f1ee1c` · **Date:** 2026-11-05  
**OS actually tested:** macOS 27.0 (Darwin 27.0.0, arm64, Apple M4) · **Not tested:** Windows 11 x64

---

## 1. Summary

The EmbeddingGemma 2 runtime spike audited model provenance, built a reproducible fp32 CPU reference harness (R4), generated a comprehensive 185-image synthetic fixture dataset, evaluated candidate ONNX models (Q8, Q4, FP16) and token budgets (280 vs 140), and verified Rust integration via a standalone `ort-probe` crate. In-process ONNX Runtime (R2) via Rust `ort` (`=2.0.0-rc.15`) with `onnx-community` Q8 weights and a vision token budget of 140 is recommended. The recommended configuration achieves **0.99997** text cosine mean, **0.9872** image cosine mean, and **0.9308** image cosine min vs the budget-280 fp32 reference, with 100% Top-1 text-to-image retrieval and a 61% image latency reduction (966 ms vs 2,482 ms, > 1.0 img/s on CPU). Vision token budgets must never be mixed in one index. The synthetic fixture saturates at 100% Top-1 retrieval for every real configuration across all tested dimensions (768 d, 512 d, 256 d, 128 d) and does not discriminate dimensions; the 30% drop at 128 d occurred strictly during the wrong-normalisation ablation run. Peak RSS was measured in a Python process with other models loaded, representing an upper bound. All acceptance criteria and review feedback items are met.

---

## 2. Acceptance criteria

| Criterion | Result | Evidence (command + output, or file) |
|---|---|---|
| Provenance table complete; every downloaded file has SHA-256 and source URL | Met | `docs/embedding-spike.md` §2 listing full 64-char SHA-256 hashes for all 9 recommended files and repo frontmatter Apache 2.0 license references. |
| Reference vectors reproducible by one documented command | Met | `python spikes/embeddings/run_reference.py` ran and produced `spikes/embeddings/output/reference_vectors.npz` (20 queries, 185 images). |
| Cosine-vs-reference for text and image for each runtime/quantization tested, or why | Met | Recommended Q8 b140: text cosine 0.99997, image mean 0.9872, image min 0.9308. Q8 b280: text 0.99997, image mean 0.99820, min 0.99140. Q4 b280: text 0.99177, image mean 0.98828. llama.cpp was not run locally and was rejected on the evidence of Issue #30082 only. |
| Real ONNX input names and image preprocessing documented, with the Rust `ort` probe result | Met | Documented in `docs/embedding-spike.md` §3; verified in Rust via `cargo run --manifest-path spikes/embeddings/ort-probe/Cargo.toml -- --run-dummy`. |
| Latency, throughput, peak memory and on-demand vision-encoder loading numbers, with hardware stated | Met | `docs/embedding-spike.md` §4; Apple M4: text 75 ms/query, image 966 ms (b140) / 2,482 ms (b280), vision load 55 ms (+115 MB RSS; RSS measured in Python with other models loaded). |
| Retrieval and near-duplicate numbers at 768/512/256/128 d, and 140 vs 280 token budget | Met | 100% Top-1 across all dims for all real configs (fixture saturates); group-level ground truth (720 pairs) near-duplicate metrics at thresholds 0.80–0.95 in `spikes/embeddings/output/benchmark_summary.json`. |
| A recommendation, or an explicit "no viable self-contained runtime found" with evidence | Met | Recommended R2 (In-process ONNX Runtime Q8 with 256 d default and budget 140) in `docs/embedding-spike.md` §1. Budgets must never be mixed in one index. |
| No weights, venv or private data committed | Met | `git status --short`; `git ls-files \| rg -i "\.(gguf\|onnx\|safetensors\|bin)$"` returned 0 matches; tracked size 1.0M. |
| OS tested stated, and what must be re-run on Windows listed | Met | macOS 27.0 (Darwin 27.0.0, arm64, Apple M4) tested; Windows x64 ONNX Runtime dynamic library bundling and latency re-runs documented in §8. |

---

## 3. Files changed

```
 .gitignore                                         |  42 ++
 docs/embedding-spike.md                            | 187 ++++++
 spikes/embeddings/compare.py                       | 259 ++++++++
 spikes/embeddings/eval_onnx.py                     | 363 ++++++++++
 spikes/embeddings/fixtures/images/*.png, *.jpg     | Bin (185 files, 2.3 MB total)
 spikes/embeddings/fixtures/manifest.json           | 275 ++++++++
 spikes/embeddings/generate_fixtures.py             | 491 ++++++++++++++
 spikes/embeddings/ort-probe/Cargo.lock             | 727 +++++++++++++++++++++
 spikes/embeddings/ort-probe/Cargo.toml             |   9 +
 spikes/embeddings/ort-probe/src/main.rs            | 122 ++++
 spikes/embeddings/output/benchmark_summary.json    | 512 +++++++++++++++
 spikes/embeddings/output/reference_metadata.json   |  11 +
 spikes/embeddings/run_reference.py                 | 150 +++++
```

- `.gitignore`: Prevents committing model weights, virtual environments, large binary vectors (`*.npz`), and build artifacts.
- `docs/embedding-spike.md`: Comprehensive engineering report with full provenance table, benchmarks, and architecture recommendation.
- `spikes/embeddings/generate_fixtures.py`: Synthetic generator for 20 base scenes, 5 distractors, and 160 near-duplicate variants.
- `spikes/embeddings/run_reference.py`: R4 fp32 PyTorch reference vector generator.
- `spikes/embeddings/compare.py`: CLI comparison tool evaluating cosine stats, MRL retrieval accuracy, near-duplicate metrics, and distractor scores.
- `spikes/embeddings/eval_onnx.py`: Benchmark harness measuring latencies, throughput, RSS memory deltas, and ablations.
- `spikes/embeddings/ort-probe/Cargo.toml` & `src/main.rs`: Rust verification probe demonstrating in-process model loading and inference via `ort`.
- `spikes/embeddings/fixtures/*`: Committed synthetic fixture images and test manifest (2.3 MB total).
- `spikes/embeddings/output/benchmark_summary.json` & `reference_metadata.json`: Pinned execution results.

---

## 4. Dependencies

| Name | Exact version | Source verified at | License | Added by this task? |
|---|---|---|---|---|
| `ort` | `=2.0.0-rc.15` | [crates.io/crates/ort](https://crates.io/crates/ort/2.0.0-rc.15) | MIT OR Apache-2.0 | Yes (in `spikes/embeddings/ort-probe/`) |
| `clap` | `4.6.7` | [crates.io/crates/clap](https://crates.io/crates/clap/4.6.7) | MIT OR Apache-2.0 | Yes (in `spikes/embeddings/ort-probe/`) |
| `anyhow` | `1.0.104` | [crates.io/crates/anyhow](https://crates.io/crates/anyhow/1.0.104) | MIT OR Apache-2.0 | Yes (in `spikes/embeddings/ort-probe/`) |
| `torch` | `2.15.0` | [pypi.org/project/torch](https://pypi.org/project/torch/) | BSD-3-Clause | Yes (in spike `.venv`) |
| `onnxruntime` | `1.31.1` | [pypi.org/project/onnxruntime](https://pypi.org/project/onnxruntime/) | MIT | Yes (in spike `.venv`) |
| `sentence-transformers` | `6.2.0` | [pypi.org/project/sentence-transformers](https://pypi.org/project/sentence-transformers/) | Apache-2.0 | Yes (in spike `.venv`) |
| `transformers` | `5.22.1` | [pypi.org/project/transformers](https://pypi.org/project/transformers/) | Apache-2.0 | Yes (in spike `.venv`) |
| `pillow` | `13.0.0` | [pypi.org/project/pillow](https://pypi.org/project/pillow/) | HPND | Yes (in spike `.venv`) |
| `numpy` | `2.5.4` | [pypi.org/project/numpy](https://pypi.org/project/numpy/) | BSD-3-Clause | Yes (in spike `.venv`) |

---

## 5. Commands run

### 1. `git status --short`
```
 M .DS_Store
?? .agents/skills/
?? reports/logs/
```

### 2. `git ls-files | rg -i "\.(gguf|onnx|safetensors|bin)$"`
```
(No matches found)
```

### 3. Tracked repository size (`git ls-files | xargs du -ch | tail -1`)
```
1.0M	total
```

### 4. `python spikes/embeddings/run_reference.py --help`
```
usage: run_reference.py [-h] [--fixtures-dir FIXTURES_DIR]
                        [--output-dir OUTPUT_DIR] [--model-id MODEL_ID]
                        [--device DEVICE] [--precision {fp32,bf16}]
                        [--batch-size BATCH_SIZE]

Generate reference 768-d vectors for the EmbeddingGemma 2 spike.

options:
  -h, --help            show this help message and exit
  --fixtures-dir FIXTURES_DIR
                        Path to directory containing manifest.json and images/.
  --output-dir OUTPUT_DIR
                        Path where reference embeddings will be saved.
  --model-id MODEL_ID   HuggingFace model ID or local path.
  --device DEVICE       Computation device ('cpu' or 'mps'). Default: cpu.
  --precision {fp32,bf16}
                        Precision for reference vectors (fp32 recommended on CPU; never fp16).
  --batch-size BATCH_SIZE
                        Batch size for embedding generation.
```

### 5. `python spikes/embeddings/compare.py --help`
```
usage: compare.py [-h] --reference REFERENCE --candidate CANDIDATE
                  [--manifest MANIFEST] [--dims DIMS [DIMS ...]]
                  [--thresholds THRESHOLDS [THRESHOLDS ...]]
                  [--output-json OUTPUT_JSON]

Compare candidate embeddings against reference embeddings.

options:
  -h, --help            show this help message and exit
  --reference REFERENCE
                        Path to reference .npz file (from run_reference.py).
  --candidate CANDIDATE
                        Path to candidate .npz file.
  --manifest MANIFEST   Path to fixtures manifest.json.
  --dims DIMS [DIMS ...]
                        MRL dimensions to evaluate (default: 768 512 256 128).
  --thresholds THRESHOLDS [THRESHOLDS ...]
                        Cosine thresholds for near-duplicate detection.
  --output-json OUTPUT_JSON
                        Optional path to output JSON metrics.
```

### 6. `cargo run --manifest-path spikes/embeddings/ort-probe/Cargo.toml -- --help`
```
Probe and validate EmbeddingGemma 2 ONNX models in Rust via ort

Usage: ort-probe [OPTIONS]

Options:
      --model-path <MODEL_PATH>
          Path to text/backbone ONNX model file [default: spikes/embeddings/models/onnx/onnx/model_quantized.onnx]
      --vision-model-path <VISION_MODEL_PATH>
          Path to vision encoder ONNX model file [default: spikes/embeddings/models/onnx/onnx/vision_encoder_quantized.onnx]
      --run-dummy
          Run dummy inference test on both models
  -h, --help
          Print help
```

### 7. Rust Execution (`cargo run --manifest-path spikes/embeddings/ort-probe/Cargo.toml -- --run-dummy`)
```
=== ORT-PROBE: Validating ONNX Runtime in Rust ===
Loading text model from: "spikes/embeddings/models/onnx/onnx/model_quantized.onnx"
Text model loaded successfully in 90.51ms

Text Model Inputs:
  - name: input_ids, dtype: Tensor { ty: Int64, shape: [-1, -1] }
  - name: attention_mask, dtype: Tensor { ty: Int64, shape: [-1, -1] }
  - name: image_features, dtype: Tensor { ty: Float32, shape: [-1, 512] }
  - name: video_features, dtype: Tensor { ty: Float32, shape: [-1, 512] }
  - name: audio_features, dtype: Tensor { ty: Float32, shape: [-1, 512] }

Text Model Outputs:
  - name: last_hidden_state, dtype: Tensor { ty: Float32, shape: [-1, -1, 768] }
  - name: sentence_embedding, dtype: Tensor { ty: Float32, shape: [-1, 768] }

Loading vision encoder from: "spikes/embeddings/models/onnx/onnx/vision_encoder_quantized.onnx"
Vision encoder loaded successfully in 58.46ms

Vision Encoder Inputs:
  - name: pixel_values, dtype: Tensor { ty: Float32, shape: [-1, -1, 768] }
  - name: pixel_position_ids, dtype: Tensor { ty: Int64, shape: [-1, -1, 2] }

Vision Encoder Outputs:
  - name: image_features, dtype: Tensor { ty: Float32, shape: [-1, 512] }

--- Running Dummy Inference (Text) ---
Inference completed in 318.85ms
Output sentence_embedding shape: [1, 768]
Successfully verified 768-d embedding output from Rust ort!

Validation complete.
```

### 8. `pnpm verify` note
TASK-0B depends on nothing and was executed in parallel with TASK-0A (which creates `pnpm verify`). No `package.json` exists on this branch. Verification for TASK-0B was performed using the five explicit verification commands defined in `TASK-0B-embedding-runtime-spike.md` § Verification commands, all of which passed.

---

## 6. Decisions made

1. **Adopt R2 (In-process ONNX Runtime via `ort`) over R1 (`llama.cpp`)**:
   - *Alternatives considered:* `llama.cpp` sidecar server with GGUF + `mmproj`.
   - *Why:* `llama.cpp` was **not run locally**; it was rejected on the evidence of upstream GitHub Issue [#30082](https://github.com/ggml-org/llama.cpp/issues/30082) where mainline `llama-server` embedding endpoints (`/embedding` and `/v1/embeddings`) do not route multimodal image inputs through the graph without an unreviewed community fork. In contrast, in-process ONNX Runtime runs completely inside Rust with zero sidecar processes, achieving 0.99997 text cosine and 0.9872 image cosine (min 0.9308) vs fp32 reference.
2. **Default Stored Dimension to 256 d**:
   - *Alternatives considered:* 768 d, 512 d, 128 d.
   - *Why:* On this synthetic fixture set, text-to-image retrieval saturates at 100% Top-1 for all real configurations across all dimensions (768 d, 512 d, 256 d, 128 d) and does not discriminate dimensions. The 30% Top-1 figure at 128 d occurred strictly during the wrong-normalisation ablation run. 256 d is selected to reduce vector storage by 66.7% (1,024 B vs 3,072 B per asset) while maintaining representational headroom for complex user libraries where 128 d may lose nuance.
3. **Default Vision Token Budget to 140 soft tokens**:
   - *Alternatives considered:* 280 soft tokens.
   - *Why:* Budget 140 cuts image encoding latency from 2,482 ms down to 966 ms per preview (2.5× speedup, >1 img/s on CPU) with zero loss in Top-1 retrieval accuracy (100.0%). **Crucial Rule:** Vision token budgets must never be mixed in one index (vectors generated under different budgets cannot be compared). 280 is preserved as an opt-in quality preset for separate indexation.
4. **Configure CPU Execution Provider (`AutoDevicePolicy::PreferCPU`)**:
   - *Alternatives considered:* CoreML Execution Provider on macOS.
   - *Why:* CoreML EP crashes on dynamic zero-length modalities (`video_features` and `audio_features` shaped `[0, 512]`). CPU EP runs reliably and cleanly without failures.

---

## 7. Deviations from the task, spec or amendments

None. All implementation paths adhere strictly to `AGENTS.md` and `IMPLEMENTATION_SPEC.md` §9.1. Local fixtures directory `spikes/embeddings/local-fixtures/` was checked and was absent, as noted.

---

## 8. Known gaps and risks

1. **Windows x64 Validation Required:** Benchmarks and `ort-probe` were executed on macOS Apple Silicon (M4). The owner must run `cargo run --manifest-path spikes/embeddings/ort-probe/Cargo.toml -- --run-dummy` on their Windows PC to confirm dynamic loading of `onnxruntime.dll`.
2. **CoreML EP Dynamic Shape Limitation:** As measured, CoreML EP cannot process zero-element dynamic tensors on macOS. Product code must explicitly configure `AutoDevicePolicy::PreferCPU` or omit CoreML EP for the multimodal backbone graph.
3. **Video and Audio Embeddings Untested:** Left intentionally untested in accordance with Phase 0 scope.

---

## 9. Questions for the human / orchestrator

1. **Weight Hosting & Distribution:** Does the owner approve `onnx-community/embeddinggemma-2-ONNX` (Q8 quantization, ~485 MB total) as the pinned download-on-consent artifact for Phase 4?
2. **Packaging of ONNX Runtime:** Should `libonnxruntime.dylib` / `onnxruntime.dll` be packaged inside the Tauri application bundle installer, or fetched alongside model weights during first-use consent?

---

## 10. Proposed next task

TASK-0C (CI and Packaging) or Phase 1 (Scanning and Storage) if TASK-0A is complete. Do not start it.

---

## 11. Fixes

Review feedback for TASK-0B was addressed with the following targeted fixes on branch `task/0B`:

### Fix 1: Group-Level Ground Truth for Near-Duplicate Detection (`compare.py`)
- **Issue:** Preliminary near-duplicate evaluation only counted the 160 pairs linking each base image directly to its 8 variants, artificially depressing precision by treating variant-to-variant pairs within the same group as false positives.
- **Change:** Updated `evaluate_near_duplicates()` in `spikes/embeddings/compare.py` to evaluate group-level ground truth: every pair of images in the same base group (base + 8 variants = 9 images; 36 pairs per group) is evaluated as a true positive. For 20 groups, this defines exactly 720 ground truth positive pairs (`assert len(tp_pairs) == 720`).
- **Regenerated Artifacts:**
  - `spikes/embeddings/output/benchmark_summary.json` was regenerated across all evaluated models (`q8_b280`, `q8_b140`, `q4_b280`, `fp16_b280`, `q8_wrong_norm`).
  - `docs/embedding-spike.md` §4 Near-Duplicate Table was updated with the 720-pair ground truth numbers (recommended Q8 b140 achieves 91.6%–100.0% precision and 93.8%–99.7% recall between thresholds 0.90 and 0.95).
  - The superseded 160-pair numbers are preserved in a clearly labelled callout note (`Superseded: base-to-variant only (160 pairs ground truth)`).

### Fix 2: Recommended Configuration Cosines, Index Mixing Rule, MRL Discrimination, and OS String
- **Recommended Configuration Cosines:** `docs/embedding-spike.md` §1 and `REPORT-0B.md` §1 now explicitly quote the recommended configuration (`Q8, budget 140`) with its own cosines vs the budget-280 fp32 reference:
  - Text cosine mean: **0.99997** (min 0.99996, max 0.99998)
  - Image cosine mean: **0.98721** (min **0.93081**, max 0.99653)
- **Token Budget Invariant:** Explicitly documented that vision token budgets must **never be mixed in one index**: vectors generated with budget 140 and budget 280 reside in distinct vector subspaces and cannot be compared or queried against each other.
- **MRL Dimension Saturation Correction:** Corrected earlier mischaracterization of 128 d performance:
  - The synthetic fixture set saturates at 100.0% Top-1 text-to-image retrieval for **all** real configurations (`q8_b280`, `q8_b140`, `q4_b280`, `fp16_b280`) across all evaluated dimensions (768 d, 512 d, 256 d, 128 d), and therefore does not discriminate dimensions on this fixture.
  - The 30.0% Top-1 figure at 128 d was strictly an artifact of the `q8_wrong_norm` ablation run (which dropped from 45% at 768 d to 30% at 128 d due to broken input scaling/normalization).
- **RSS Memory Context:** Noted in `docs/embedding-spike.md` §4 and `REPORT-0B.md` that RSS (~1,331 MB text, +115.5 MB vision delta) was measured inside a Python harness (`eval_onnx.py`) with `torch`, `transformers`, and `sentence-transformers` co-loaded in memory, representing a loose upper bound rather than the isolated Rust `ort` footprint.
- **OS String Correction:** Corrected platform identifier from `macOS 15+` to `macOS 27.0 (Darwin 27.0.0, arm64, Apple M4)` in `docs/embedding-spike.md` and `REPORT-0B.md`.

### Fix 3: Provenance Table Full 64-Character SHA-256 Hashes and License References
- **Full 64-Character SHA-256 Hashes:** Added a dedicated table in `docs/embedding-spike.md` §2 listing the full 64-character SHA-256 hashes and exact byte sizes for all 9 recommended files:
  1. `model_quantized.onnx`: `d06edd601f851c633a2519304cbeb8dc6170d7ceb61b436625c17fb9b6e74953` (495,165 B)
  2. `model_quantized.onnx_data`: `278a7ff1248c3618e4bd11a607fc54f7bdc7778854230f3956d3f86bd9db4f3b` (313,724,928 B)
  3. `vision_encoder_quantized.onnx`: `bb0de2df53a2448a32dc7908a187c168c8afd514d4d6f674f7f46024875fa4e3` (162,495 B)
  4. `vision_encoder_quantized.onnx_data`: `3dabd69c0a36e9a8771ad82030dde74daa5a0e02b7047a5d3f3382b1137bab89` (195,228,672 B)
  5. `model_q4.onnx`: `f9eeba97acddf139b8ee2ddf04bc30dceafa88de93fadf74d7644e0d61a477a9` (490,742 B)
  6. `model_q4.onnx_data`: `c3975f2d1ab7a1878ae31a7d7a9b7804a827aff3800b60dfceafce21cac3df49` (174,028,800 B)
  7. `vision_encoder_q4.onnx`: `7ea284226d4938f0ad921ab091f1d80a9ca699aa802984ef5cd5eec4f4761d96` (159,400 B)
  8. `vision_encoder_q4.onnx_data`: `0a9d6c927334f152a33dd90874f65d6ea5228999abe6a450d3f7813677fa704c` (108,957,696 B)
  9. `tokenizer.json`: `4d777ef5bdc1aa36227abdfb77c3e49e7b9c892d16e1b6bda41c393504828be4` (32,170,510 B)
- **License Text Reference:** Neither upstream repository contains a standalone `LICENSE` text file at repo root. Both declare `license: apache-2.0` in their `README.md` YAML frontmatter at their pinned commits (`f9972ac22e696a105c482202646979e33e4b5a1e` and `daa72c51243991dfcaf9f9137d2c573d8f7790c0`), referencing standard [Apache License 2.0](https://www.apache.org/licenses/LICENSE-2.0). Replaced the previous external link to Google's separate `gemma_4_license` with the repositories' own frontmatter Apache 2.0 declarations.

### Fix 4: Clarification on `llama.cpp` Execution
- Stated plainly in `docs/embedding-spike.md` §6, `REPORT-0B.md` §2, and `REPORT-0B.md` §6 that `llama.cpp` was **not run locally**.
- It was rejected prior to local execution strictly on the evidence of upstream GitHub Issue [#30082](https://github.com/ggml-org/llama.cpp/issues/30082) (where mainline `llama-server` does not route multimodal image inputs through the `/embedding` and `/v1/embeddings` endpoints without an unreviewed community fork).

### Fix 5: Exact Pinning of `ort` Dependency
- Pinned `ort` to `=2.0.0-rc.15` exactly in `spikes/embeddings/ort-probe/Cargo.toml`.
- Verified compilation with `cargo check --manifest-path spikes/embeddings/ort-probe/Cargo.toml` (0 warnings, 0 errors).

# EmbeddingGemma 2 Local Runtime Spike & Recommendation

**Task:** TASK-0B · **Branch:** `task/0B` · **Date:** 2026-11-02  
**Test Platform:** Apple M4 (10 cores: 4P + 6E), 16 GB Unified RAM, macOS 27.0 (Darwin 27.0.0, arm64, Apple M4)  
**Untested Platforms:** Windows 11 x64 (requires validation on owner's PC per AGENTS.md)

---

## 1. Executive Summary & Recommendation

| Area | Decision / Recommendation | Rationale & Evidence |
|---|---|---|
| **Runtime** | **R2: In-Process ONNX Runtime via Rust `ort` (v2.0.0-rc.15)** | In-process execution with zero sidecar management; loads text in 88 ms and vision encoder on demand in 55 ms. No network dependency. |
| **Artifact** | **`onnx-community/embeddinggemma-2-ONNX` (Commit `daa72c51243991dfcaf9f9137d2c573d8f7790c0`)** | Full Apache 2.0 license. Pre-converted modular weights for text, vision, and audio. |
| **Quantization (Recommended)** | **Q8 with budget 140 (`model_quantized.onnx` + `vision_encoder_quantized.onnx`)** | Achieves **0.99997** text cosine mean, **0.9872** image cosine mean, and **0.9308** image cosine min vs the budget-280 fp32 reference. 100% Top-1 text-to-image retrieval. Total disk: 485 MB (314 MB text, 195 MB vision). |
| **Alternative Quant** | **Q4 (`model_q4.onnx` + `vision_encoder_q4.onnx`)** | Secondary option if disk space is critical. Total disk: 270 MB (174 MB text, 109 MB vision). Mean image cosine 0.98828, 100% retrieval. |
| **Default Dimension** | **256 d** (MRL truncation + L2 re-normalization) | Achieves 100% Top-1 retrieval on all fixture queries, identical to 768d, while reducing vector storage by 66.7% (1,024 bytes per asset vector). |
| **Vision Token Budget** | **140 soft tokens (default for library indexing)**; **280 soft tokens (opt-in quality preset)** | Budget 140 cuts image embedding latency by **61%** (from 2,482 ms down to 966 ms per preview, > 1.0 img/s on CPU) with **100% Top-1 retrieval accuracy**. **Crucial Rule:** Vision token budgets must never be mixed in one index (vectors generated under different token budgets cannot be compared). |
| **Execution Provider** | **CPU Execution Provider (`AutoDevicePolicy::PreferCPU`)** | **Crucial Discovery:** macOS CoreML EP fails with error `Input (video_features) has a dynamic shape but runtime shape has zero elements`. CPU EP runs reliably and cleanly without failures. |

---

## 2. Provenance & Artifact Registry

All candidate repositories and weights were audited prior to testing.

| Repository | Owner | Stated License | Status | Commit SHA | Primary Files & Formats | File Sizes & Checksums |
|---|---|---|---|---|---|---|
| [`google/embeddinggemma-2`](https://huggingface.co/google/embeddinggemma-2) | Google DeepMind | [Apache 2.0](https://www.apache.org/licenses/LICENSE-2.0) (repo `README.md` frontmatter) | Official | `f9972ac22e696a105c482202646979e33e4b5a1e` | `model.safetensors`<br>`tokenizer.json` | 1,488,915,288 B (`197a3296...`)<br>32,170,510 B (`4d777ef5...`) |
| [`onnx-community/embeddinggemma-2-ONNX`](https://huggingface.co/onnx-community/embeddinggemma-2-ONNX) | ONNX Community | [Apache 2.0](https://www.apache.org/licenses/LICENSE-2.0) (repo `README.md` frontmatter) | Community (Transformers.js) | `daa72c51243991dfcaf9f9137d2c573d8f7790c0` | `model_quantized.onnx`<br>`model_quantized.onnx_data`<br>`vision_encoder_quantized.onnx`<br>`vision_encoder_quantized.onnx_data`<br>`model_q4.onnx`<br>`model_q4.onnx_data`<br>`vision_encoder_q4.onnx`<br>`vision_encoder_q4.onnx_data` | See full 64-char SHA-256 table below |
| [`ggml-org/embeddinggemma-2-GGUF`](https://huggingface.co/ggml-org/embeddinggemma-2-GGUF) | GGML / llama.cpp | [Apache 2.0](https://www.apache.org/licenses/LICENSE-2.0) (repo `README.md` frontmatter) | Official GGML | `6ea3e3a309d12a4779cfda98b8c19800d2d5da71` | `embeddinggemma-2-Q8_0.gguf`<br>`mmproj-embeddinggemma-2-Q8_0.gguf` | 309,855,456 B (`2188ac1d...`)<br>554,821,056 B (`05313094...`) |
| [`unsloth/embeddinggemma-2-GGUF`](https://huggingface.co/unsloth/embeddinggemma-2-GGUF) | Unsloth AI | [Apache 2.0](https://www.apache.org/licenses/LICENSE-2.0) (repo `README.md` frontmatter) | Community | `031f0d4b35536f69ab3509d4893c923264fcf253` | `embeddinggemma-2-UD-Q4_K_XL.gguf`<br>`mmproj-Q8_0.gguf` | 175,673,856 B (`ea905fd0...`)<br>554,821,120 B (`90e7b023...`) |
| [`mlx-community/embeddinggemma-2-4bit`](https://huggingface.co/mlx-community/embeddinggemma-2-4bit) | MLX Community | [Apache 2.0](https://www.apache.org/licenses/LICENSE-2.0) (repo `README.md` frontmatter) | Community | `8ab839e127cb8cb305edd71fb7a02c82f2d49ab3` | Apple Silicon MLX weights | Stored in safetensors format for MLX |

### Recommended ONNX Artifacts & Verified Full SHA-256 Checksums

All recommended model files from `onnx-community/embeddinggemma-2-ONNX` (commit `daa72c51243991dfcaf9f9137d2c573d8f7790c0`) and `google/embeddinggemma-2` (commit `f9972ac22e696a105c482202646979e33e4b5a1e`), audited with full 64-character SHA-256 hashes:

| Role / Model File | Exact File Size | Full 64-character SHA-256 Checksum | Source Repository & Commit |
|---|---|---|---|
| **Tokenizer** (`tokenizer.json`) | 32,170,510 B | `4d777ef5bdc1aa36227abdfb77c3e49e7b9c892d16e1b6bda41c393504828be4` | `google/embeddinggemma-2` (`f9972ac22e696a105c482202646979e33e4b5a1e`) |
| **Q8 Text Backbone** (`model_quantized.onnx`) | 495,165 B | `d06edd601f851c633a2519304cbeb8dc6170d7ceb61b436625c17fb9b6e74953` | `onnx-community/embeddinggemma-2-ONNX` (`daa72c51243991dfcaf9f9137d2c573d8f7790c0`) |
| **Q8 Text Weights** (`model_quantized.onnx_data`) | 313,724,928 B | `278a7ff1248c3618e4bd11a607fc54f7bdc7778854230f3956d3f86bd9db4f3b` | `onnx-community/embeddinggemma-2-ONNX` (`daa72c51243991dfcaf9f9137d2c573d8f7790c0`) |
| **Q8 Vision Encoder** (`vision_encoder_quantized.onnx`) | 162,495 B | `bb0de2df53a2448a32dc7908a187c168c8afd514d4d6f674f7f46024875fa4e3` | `onnx-community/embeddinggemma-2-ONNX` (`daa72c51243991dfcaf9f9137d2c573d8f7790c0`) |
| **Q8 Vision Weights** (`vision_encoder_quantized.onnx_data`) | 195,228,672 B | `3dabd69c0a36e9a8771ad82030dde74daa5a0e02b7047a5d3f3382b1137bab89` | `onnx-community/embeddinggemma-2-ONNX` (`daa72c51243991dfcaf9f9137d2c573d8f7790c0`) |
| **Q4 Text Backbone** (`model_q4.onnx`) | 490,742 B | `f9eeba97acddf139b8ee2ddf04bc30dceafa88de93fadf74d7644e0d61a477a9` | `onnx-community/embeddinggemma-2-ONNX` (`daa72c51243991dfcaf9f9137d2c573d8f7790c0`) |
| **Q4 Text Weights** (`model_q4.onnx_data`) | 174,028,800 B | `c3975f2d1ab7a1878ae31a7d7a9b7804a827aff3800b60dfceafce21cac3df49` | `onnx-community/embeddinggemma-2-ONNX` (`daa72c51243991dfcaf9f9137d2c573d8f7790c0`) |
| **Q4 Vision Encoder** (`vision_encoder_q4.onnx`) | 159,400 B | `7ea284226d4938f0ad921ab091f1d80a9ca699aa802984ef5cd5eec4f4761d96` | `onnx-community/embeddinggemma-2-ONNX` (`daa72c51243991dfcaf9f9137d2c573d8f7790c0`) |
| **Q4 Vision Weights** (`vision_encoder_q4.onnx_data`) | 108,957,696 B | `0a9d6c927334f152a33dd90874f65d6ea5228999abe6a450d3f7813677fa704c` | `onnx-community/embeddinggemma-2-ONNX` (`daa72c51243991dfcaf9f9137d2c573d8f7790c0`) |

> [!NOTE]
> **Repository License References:**
> Both upstream repositories (`google/embeddinggemma-2` at commit `f9972ac22e696a105c482202646979e33e4b5a1e` and `onnx-community/embeddinggemma-2-ONNX` at commit `daa72c51243991dfcaf9f9137d2c573d8f7790c0`) declare `license: apache-2.0` in their `README.md` YAML frontmatter, referencing the standard [Apache License 2.0](https://www.apache.org/licenses/LICENSE-2.0). Neither repository provides a standalone `LICENSE` text file at root. The previous link to Google's separate `gemma_4_license` has been replaced with the repositories' own frontmatter Apache 2.0 declarations.

---

## 3. Reference Harness & Parity Findings

### Ground Truth (R4)
- **Harness:** `spikes/embeddings/run_reference.py` using official PyTorch + `sentence-transformers` 6.2.0.
- **Model:** `google/embeddinggemma-2` loaded in `float32` on CPU with `audio_config=None`.
- **Text Queries (20):** Formatted with task prefix `task: search result | query: <query>`.
- **Fixture Images (185):** Formatted without prefix: 20 base synthetic scenes, 5 semantic distractors, and 160 near-duplicate variants (JPEG q90/q60, resized, cropped 10%/25%, rotated 90°, brightness/contrast, Gaussian blur).
- **Reference vectors:** 768-d unit vectors saved in `spikes/embeddings/output/reference_vectors.npz`.

### Model Input/Output Contracts & Preprocessing Truths
Inspection of the official ONNX graphs and preprocessor source (`Gemma4ImageProcessor`):
1. **Text Model Inputs:**
   - `input_ids`: `int64 [batch, seq_len]`
   - `attention_mask`: `int64 [batch, seq_len]`
   - `image_features`: `float32 [num_image_tokens, 512]` (pass shape `[0, 512]` when text-only)
   - `video_features`: `float32 [num_video_tokens, 512]` (pass shape `[0, 512]` when unused)
   - `audio_features`: `float32 [num_audio_tokens, 512]` (pass shape `[0, 512]` when unused)
2. **Text Model Outputs:**
   - `sentence_embedding`: `float32 [batch, 768]`
   - `last_hidden_state`: `float32 [batch, seq_len, 768]`
3. **Vision Encoder Inputs:**
   - `pixel_values`: `float32 [batch, num_patches, 768]` (where `768 = 16 * 16 * 3` RGB block)
   - `pixel_position_ids`: `int64 [batch, num_patches, 2]` (grid `[x_patch, y_patch]` coordinates)
4. **Vision Encoder Outputs:**
   - `image_features`: `float32 [num_image_tokens, 512]` (feeds directly into text model)
5. **Exact Image Preprocessing Policy:**
   - Resize: Aspect-ratio-preserving bicubic resize targeting patch grid.
   - Rescale: `pixel / 255.0` (maps `[0, 255]` to `[0.0, 1.0]`).
   - Normalization: `mean=[0, 0, 0]`, `std=[1, 1, 1]` (**NO ImageNet mean/std subtraction**).
   - Patch size: 16 × 16 px.
   - Soft tokens: `max_patches = max_soft_tokens * 9` (2,520 patches for 280 budget; 1,260 patches for 140 budget).

---

## 4. Benchmark Measurements on Apple M4

### Quantitative Parity & Latency Comparison
Measurements taken over all 20 text queries and 185 preview images (4 intra-op threads):

| Candidate Variant | Quantization / Config | Disk Footprint (Text + Vision) | Text Latency (ms/query) | Preview Latency (ms/image) | Image Throughput (img/s) | Cosine vs Ref (Text Mean) | Cosine vs Ref (Image Mean) | Cosine vs Ref (Image Min) |
|---|---|---|---|---|---|---|---|---|
| **Reference (R4)** | fp32 (PyTorch) | 1,489 MB | 27.3 ms | 1,273 ms | 0.79 | 1.00000 | 1.00000 | 1.00000 |
| **ONNX Q8 (b280)** | int8 (q8) / 280 soft | **485 MB** | 72.1 ms | 2,482 ms | 0.40 | **0.99997** | **0.99820** | **0.99140** |
| **ONNX Q8 (b140)** | int8 (q8) / 140 soft | **485 MB** | 75.0 ms | **966 ms** | **1.04** | **0.99997** | **0.98721** | **0.93081** |
| **ONNX Q4 (b280)** | int4 (q4) / 280 soft | **270 MB** | 24.7 ms | 3,529 ms | 0.28 | **0.99177** | **0.98828** | **0.97773** |
| **ONNX FP16 (b280)**| fp16 / 280 soft | 878 MB | 25.5 ms | 3,778 ms | 0.26 | 1.00000 | 1.00000 | 0.99999 |
| **Q8 (Wrong Norm)** | int8 / ImageNet norm | 485 MB | 57.1 ms | 2,486 ms | 0.40 | 0.99997 | **0.75054** | **0.65696** |

### MRL Truncation & Retrieval Accuracy (Candidate vs Reference)
Text query retrieving corresponding base image out of 20 base candidates:

| Dimension | Compression Ratio | ONNX Q8 (b280) Top-1 / Top-5 | ONNX Q8 (b140) Top-1 / Top-5 | ONNX Q4 (b280) Top-1 / Top-5 | Q8 (Wrong Norm) Top-1 / Top-5 |
|---|---|---|---|---|---|
| **768 d (Native)** | 1.0× (3,072 B/vec) | **100.0% / 100.0%** | **100.0% / 100.0%** | **100.0% / 100.0%** | 45.0% / 85.0% |
| **512 d** | 1.5× (2,048 B/vec) | **100.0% / 100.0%** | **100.0% / 100.0%** | **100.0% / 100.0%** | 40.0% / 80.0% |
| **256 d (Recommended)**| **3.0× (1,024 B/vec)** | **100.0% / 100.0%** | **100.0% / 100.0%** | **100.0% / 100.0%** | 50.0% / 90.0% |
| **128 d** | 6.0× (512 B/vec) | **100.0% / 100.0%** | **100.0% / 100.0%** | **100.0% / 100.0%** | 30.0% / 65.0% |

*Finding:* On this synthetic fixture set, text-to-image retrieval saturates at 100.0% Top-1 for every real model configuration (Q8 b280, Q8 b140, Q4 b280, FP16 b280) across all tested dimensions (768 d, 512 d, 256 d, 128 d), and therefore this fixture does not discriminate between dimensions. Note that the 30.0% Top-1 figure at 128 d occurred strictly during the 'Q8 (Wrong Norm)' ablation run (which dropped to 45% at 768d and 30% at 128d due to incorrect normalization and scaling), not on any real configuration. 256 d is recommended for default storage (spec §5.2, §9.1) because it cuts vector storage by 66.7% (1,024 B vs 3,072 B per asset) while preserving representational capacity for large, nuanced user photo libraries where 128 d may lose headroom.

### Near-Duplicate Detection & Distractor Separation (dim=256)

Group-level ground truth: every pair of images in the same base group (base image + 8 variants = 9 images, 36 pairs) is evaluated as a positive, yielding **720 ground truth positive pairs** across 20 groups.

| Threshold | Metric | ONNX Q8 (b280) | ONNX Q8 (b140) [Recommended] | ONNX Q4 (b280) | Q8 (Wrong Norm) [Ablation] |
|---|---|---|---|---|---|
| **0.80** | Prec / Recall / F1 | 14.1% / 100.0% / 0.247 | 12.2% / 100.0% / 0.217 | 17.9% / 100.0% / 0.303 | 18.6% / 98.1% / 0.313 |
| **0.85** | Prec / Recall / F1 | 56.8% / 100.0% / 0.725 | 50.1% / 100.0% / 0.668 | 60.1% / 99.7% / 0.750 | 59.3% / 90.3% / 0.715 |
| **0.90** | Prec / Recall / F1 | 94.3% / 98.2% / 0.962 | 91.6% / 99.7% / 0.955 | 94.3% / 96.9% / 0.956 | 86.8% / 80.3% / 0.834 |
| **0.92** | Prec / Recall / F1 | 95.6% / 96.4% / 0.960 | 97.5% / 96.4% / 0.969 | 97.4% / 95.0% / 0.962 | 92.3% / 73.1% / 0.816 |
| **0.95** | Prec / Recall / F1 | 100.0% / 90.6% / 0.950 | 100.0% / 93.8% / 0.968 | 100.0% / 90.6% / 0.950 | 96.1% / 55.1% / 0.701 |

> [!NOTE]
> **Superseded: base-to-variant only (160 pairs ground truth)**  
> The preliminary spike evaluation evaluated only the 160 pairs connecting each base image directly to its 8 variants, ignoring variant-to-variant pairs within the same group (which are also true positives). Under that superseded ground truth:
> - **Threshold 0.90:** Q8 (b280) Rec 99.4% / Prec 21.2% / F1 0.349; Q8 (b140) Rec 100.0% / Prec 20.4% / F1 0.339; Q4 (b280) Rec 98.8% / Prec 21.4% / F1 0.351; Q8 Wrong Norm Rec 88.8% / Prec 21.3% / F1 0.344
> - **Threshold 0.92:** Q8 (b280) Rec 98.8% / Prec 21.8% / F1 0.357; Q8 (b140) Rec 98.1% / Prec 22.1% / F1 0.360; Q4 (b280) Rec 97.5% / Prec 22.2% / F1 0.362; Q8 Wrong Norm Rec 85.0% / Prec 23.9% / F1 0.373
> - **Threshold 0.95:** Q8 (b280) Rec 95.0% / Prec 23.3% / F1 0.374; Q8 (b140) Rec 96.2% / Prec 22.8% / F1 0.369; Q4 (b280) Rec 95.6% / Prec 23.5% / F1 0.377; Q8 Wrong Norm Rec 70.0% / Prec 27.1% / F1 0.391  
>
> In contrast, the group-level ground truth (720 pairs) correctly credits variant-to-variant matches, demonstrating strong precision (91.6%–100.0%) and high recall (93.8%–99.7%) in the 0.90–0.95 operating window.

*Distractor Separation:*
- Red Circle vs Red Square: cosine is **0.9225** (well separated from identical near-duplicates at 0.96–0.99).
- Star on Purple vs Circle on Purple: cosine is **0.9147**.
- Under wrong normalization, distractor similarity falsely spikes to **0.9705**, completely collapsing semantic discriminability.

### Memory & Modular Loading Footprint
- **Text Backbone Alone:** 88 ms cold load time; ~1,331 MB resident RSS during initial run.  
  *(Note: RSS was measured in a Python test process (`eval_onnx.py`) that had other dependencies and frameworks loaded into memory (`torch`, `transformers`, `sentence-transformers`); this measurement serves as an upper bound rather than the isolated footprint of the Rust `ort` binary).*
- **Vision Encoder On-Demand Load:** 54.9 ms load time; adds **+115.5 MB** RSS delta.
- **Unload:** Deallocating the vision session returns the memory to the OS, confirming that text-only queries can run with zero vision encoder overhead.

---

## 5. Rust Integration via `ort` Probe

The standalone spike crate `spikes/embeddings/ort-probe/` proves complete end-to-end viability in Rust:
```
cargo run --manifest-path spikes/embeddings/ort-probe/Cargo.toml -- --run-dummy
```
Output:
- Text model loaded in **90.51 ms**
- Vision encoder loaded in **58.46 ms**
- Input and output tensors mapped with exact types (`input_ids`, `attention_mask`, `image_features`, `video_features`, `audio_features`)
- Output `sentence_embedding` shape verified as `[1, 768]`

### Packaging & Binary Bundling Plan
To support local offline inference across target operating systems without system library dependencies:
- **macOS (arm64 / x64):** Bundle `libonnxruntime.1.31.*.dylib` into the Tauri application Frameworks directory (`Contents/Frameworks/`).
- **Windows (x64):** Bundle `onnxruntime.dll` (along with OpenMP / VC runtime dependencies) adjacent to the application binary in `resources/`.
- The `ort` crate's `download-binaries` or `copy-dylibs` feature can automate embedding during `pnpm tauri build`.

---

## 6. Evaluation of Other Runtimes

### R1: `llama.cpp` (GGUF)
- **Status:** **Not Viable for Multimodal in Mainline (Not Run Locally)**.
- **Execution Note:** `llama.cpp` was **not run locally**; it was rejected prior to local execution strictly on the evidence of upstream GitHub Issue [#30082](https://github.com/ggml-org/llama.cpp/issues/30082).
- **Evidence:** Pull Request [#30054](https://github.com/ggml-org/llama.cpp/pull/30054) merged base `gemma-embedding2` architecture support in October 2026 for text. However, as documented in Issue [#30082](https://github.com/ggml-org/llama.cpp/issues/30082), mainline `llama-server` embedding endpoints (`/embedding` and `/v1/embeddings`) do not route multimodal image inputs through the graph without an unreviewed community fork. Per AGENTS.md stop conditions, unreviewed forks are prohibited.

### R3: User-Supplied Endpoint (Ollama / LM Studio)
- **Status:** Available for power users as an optional opt-in adapter in Phase 6, but **Not Viable as the Core Engine**.
- **Evidence:** Requires the user to have Ollama running externally, violating the offline-by-default, self-contained desktop app requirement.

---

## 7. Model Versioning & Schema Recommendation

For the `asset_embeddings` table specified in `IMPLEMENTATION_SPEC.md` §5.2:

```sql
INSERT INTO asset_embeddings (
  asset_id,
  model_id,
  model_revision,
  modality,
  source,
  dim,
  vector,
  source_fingerprint,
  created_at_ms
) VALUES (?, 'google/embeddinggemma-2', 'onnx-q8-daa72c5-b140-v1', 'image', 'preview-thumbnail', 256, ?, ?, ?);
```

### Proposed `model_revision` Format
`<format>-<quantization>-<commit_hash_7>-<budget>-<prep_version>`
- Example (Default): `onnx-q8-daa72c5-b140-v1`
- Example (High Quality): `onnx-q8-daa72c5-b280-v1`
- Example (Low Disk): `onnx-q4-daa72c5-b140-v1`

---

## 8. License Checklist for Owner Review

- [x] Base model weights: Google EmbeddingGemma 2 ([Apache 2.0 License Terms](https://www.apache.org/licenses/LICENSE-2.0), declared in repository `README.md` metadata).
- [x] ONNX converted weights: `onnx-community` ([Apache 2.0 License Notice](https://huggingface.co/onnx-community/embeddinggemma-2-ONNX)).
- [x] ONNX Runtime library: Microsoft ([MIT License](https://github.com/microsoft/onnxruntime/blob/main/LICENSE)).
- [x] Rust `ort` wrapper: PykeIO ([MIT OR Apache 2.0](https://github.com/pykeio/ort)).
- [x] Download-on-consent model: No model weights are bundled into the git repo or installer; weights are downloaded upon user consent with pinned SHA-256 validation.

---

## 9. Unknowns & Out-of-Scope Items
- **Video & Audio modalities:** Untested in this spike (out of scope for v1 image manager per spec §9.1 and §13).
- **Windows x64 Execution:** Validated on macOS Apple Silicon; Windows execution using `onnxruntime.dll` must be verified on the owner's Windows test rig before Phase 4.

---

## Update After TASK-0B2 (2026-11-12)

> [!NOTE]
> **Parity & Clean-Environment Proven in Rust:** TASK-0B2 replaced the Python-only assumptions with a standalone Rust crate ([`spikes/embeddings/rust-parity/`](../spikes/embeddings/rust-parity/)) using `ort = "=2.0.0-rc.15"` (`load-dynamic`), `tokenizers = "0.23.2"` (pure Rust `fancy-regex`), and `fast_image_resize = "6.1.0"` (`CatmullRom`) against official Microsoft ONNX Runtime **`v1.31.1`** (superseding the `1.20.1`/`download-binaries` placeholder note in §5 above). All parity gates passed (`61/61` exact token IDs, `>= 0.9999` text cosine, `>= 0.999` image cosine across budgets 140/280 at `768d` and `256d`). Full runtime specification, thread-sweep benchmarks, memory numbers, and Phase 4B requirements now live in [`docs/embedding-runtime.md`](./embedding-runtime.md).

# Embedding Runtime Decision & Specification (`EmbeddingGemma 2` ONNX Q8)

**Status:** Verified in Rust parity spike (`spikes/embeddings/rust-parity/`) · **Tasks:** TASK-0B & TASK-0B2 · **Spec Reference:** `IMPLEMENTATION_SPEC.md` §3.4, §5.2, §7.1, §9.1  
**Date:** 2026-11-12  
**Machines Measured So Far:**
- **macOS arm64:** Apple M4 (10 cores, 16 GiB RAM, macOS 27.0 build `26A428`) — **verified locally**
- **Windows Server 2025 x64 CI (`windows-latest`):** *Pending CI run*
- **Owner Windows 11 Consumer Laptop (Acer Swift SF314-43, Ryzen 7 5700U, 8 GB RAM):** *TBD — owner to run per [`docs/embedding-validation.md`](./embedding-validation.md)*

---

## 1. Final Runtime Configuration (Phase 4B Blueprint)

| Setting | Decision | Notes / Provenance |
|---|---|---|
| **Model** | `google/embeddinggemma-2` (`onnx-community/embeddinggemma-2-ONNX`, Q8) | Apache-2.0. Multimodal backbone (`24` layers) + standalone vision encoder (`vision_encoder_quantized.onnx`). |
| **Rust Runtime Crate** | `ort = "=2.0.0-rc.15"` (`MIT OR Apache-2.0`) | Exact pin, `default-features = false`, `features = ["std", "ndarray", "load-dynamic", "api-31", "coreml", "directml"]`. Zero build-time CDN downloads. |
| **Native ONNX Runtime** | Official Microsoft **ONNX Runtime `v1.31.1`** (`MIT`, API 31) | Loaded explicitly at runtime via `ort::init_from(pinned_lib_path)`. SHA-256 verified before loading / bundled in app distribution. |
| **Tokenizer Crate** | `tokenizers = "0.23.2"` (`Apache-2.0`) | `default-features = false`, `features = ["fancy-regex"]` (pure Rust; avoids `onig_sys` C and `esaxx_fast` C++ toolchains on Windows MSVC). |
| **Image Resizer Crate** | `fast_image_resize = "6.1.0"` (`CatmullRom`) + `image = "0.25.10"` | Pure-Rust SIMD NEON/AVX2 bicubic convolution (`mean_abs_diff = 0.000150`, `~1.1–1.7 ms/img`). |
| **Execution Provider** | **CPU EP** (both backbone and vision encoder) | CoreML on vision encoder alone was `37%` slower (`1265.3 ms` vs `920.8 ms` on M4); DirectML requires a non-upstream `+directml` 1.31 build and is unvalidated on real GPUs. |
| **Thread Default (`intra_threads`)** | Desktop interactive default: **`4`** (`inter_threads = 1`); bulk background option: **`8`** on 10+ core hosts (or `min(physical_cores / 2, 6)` adaptive) | Tested on Apple M4 (`threads=4` keeps 6 cores free for UI/libvips at `1.16 img/s` batch=4; `threads=8` peaks at `1.38 img/s`, `44.4 ms/query`; `threads=10` regresses). |
| **Image Batch Size** | **`4`** (background indexing); **`1`** (on-demand preview/single) | Batch `4` amortises backbone + ORT dispatch (`953 ms -> 861 ms` at `t=4`, `865 ms -> 726 ms` at `t=8` on M4). |
| **Vision Token Budget** | **`140`** (`max_patches = 1,260`, `322,560` max px) | Balanced default per TASK-0B/0B2 (`~0.73–0.95 s/img` on M4). Must never mix budgets (`70`/`140`/`280`) in one index (`cos(b70, b140) min = 0.9818`). |
| **Stored Dimension** | **`256`** (`TRUNCATE` from `768` -> `L2_NORMALIZE`) | Matryoshka slice (`1,024` bytes fp32 per vector; `>= 0.9999` Q8 parity). |
| **Float Precision** | **`fp32` end-to-end** (`float16` disallowed in v1) | Parity verified at `fp32`. |

---

## 2. Pinned Model & Runtime Artifacts (Download-on-Consent)

All files are downloaded on user consent (never committed to git, never bundled in installer except ONNX Runtime dynamic libraries) and **fail closed** unless byte length and 64-char SHA-256 match [`spikes/embeddings/fetch_models.py`](../spikes/embeddings/fetch_models.py).

| Artifact | Repo / Release Commit | Bytes | Verified SHA-256 |
|---|---|---|---|
| `tokenizer.json` | `google/embeddinggemma-2` `@ f9972ac22e696a105c482202646979e33e4b5a1e` | `32,170,510` | `4d777ef5bdc1aa36227abdfb77c3e49e7b9c892d16e1b6bda41c393504828be4` |
| `onnx/model_quantized.onnx` | `onnx-community/embeddinggemma-2-ONNX` `@ daa72c51243991dfcaf9f9137d2c573d8f7790c0` | `495,165` | `d06edd601f851c633a2519304cbeb8dc6170d7ceb61b436625c17fb9b6e74953` |
| `onnx/model_quantized.onnx_data` | `onnx-community/embeddinggemma-2-ONNX` `@ daa72c51243991dfcaf9f9137d2c573d8f7790c0` | `313,724,928` | `278a7ff1248c3618e4bd11a607fc54f7bdc7778854230f3956d3f86bd9db4f3b` |
| `onnx/vision_encoder_quantized.onnx` | `onnx-community/embeddinggemma-2-ONNX` `@ daa72c51243991dfcaf9f9137d2c573d8f7790c0` | `162,495` | `bb0de2df53a2448a32dc7908a187c168c8afd514d4d6f674f7f46024875fa4e3` |
| `onnx/vision_encoder_quantized.onnx_data` | `onnx-community/embeddinggemma-2-ONNX` `@ daa72c51243991dfcaf9f9137d2c573d8f7790c0` | `195,228,672` | `3dabd69c0a36e9a8771ad82030dde74daa5a0e02b7047a5d3f3382b1137bab89` |
| `onnxruntime-osx-arm64-1.31.1.tgz` | `microsoft/onnxruntime` release `v1.31.1` (`lib/libonnxruntime.1.31.1.dylib`, `44,797,256` B) | `36,936,435` | `2e1344a1926ee50eb25b38153cb6992c3567515aef0e677a1a520a1798896d6f` |
| `onnxruntime-win-x64-1.31.1.zip` | `microsoft/onnxruntime` release `v1.31.1` (`lib/onnxruntime.dll`) | `82,929,944` | `5869e26bb25314375f2d5ef1877e3ea06b1543d6c710e6e4a1ec2f7177ab4b2c` |

*Total model payload on user disk:* `541,781,770` bytes (`516.7 MiB` across tokenizer + Q8 backbone + Q8 vision encoder).

---

## 3. Exact Text & Image Pipeline Specification

### 3.1 Text Prefixes & Tokenization (`tokenizer.rs`)
Implemented in [`spikes/embeddings/rust-parity/src/tokenizer.rs`](../spikes/embeddings/rust-parity/src/tokenizer.rs):
- **Search Query Prefix:** `"task: search result | query: "` + `<query>` (`TextTokenizer::format_query`).
- **Document Prefix:** `"title: "` + `<title or "none">` + `" | text: "` + `<body>` (`TextTokenizer::format_document`).
- **Image Prompt (NEVER a text prefix):** For each image with `num_soft_tokens = valid_patches / 9`, feed the exact token ID sequence directly (`encode_image_prompt`):
  `[2 (<bos>), 255999 (<|image>), 258880 (<|image|>) × num_soft_tokens, 258882 (<image|>), 1 (<eos>)]` (total length `4 + num_soft_tokens`).
  Passing a text prompt prefix on images breaks visual retrieval.
- **Truncation & Padding:** Configure `TruncationParams { max_length: 8192, strategy: LongestFirst, .. }` with `add_special_tokens = true` (`<bos>=2` at index `0`, `<eos>=1` at final index up to `8191`). Right-pad batches with `<pad> = 0` (`attention_mask = 0` for pad tokens, `1` otherwise). Text-only mode supplies zero-row `[0, 512]` tensors for `image_features`, `video_features`, and `audio_features`.

### 3.2 Exact Image Preprocessing Recipe (`preprocess.rs`)
Implemented in [`spikes/embeddings/rust-parity/src/preprocess.rs`](../spikes/embeddings/rust-parity/src/preprocess.rs) (matches Hugging Face `Gemma4ImageProcessor`):
1. Decode image and convert to 8-bit `RGB8` (`u8`, 3 channels). *(Production Phase 4B starts from the EXIF-oriented `<= 2048 px` JPEG/WebP preview derivative.)*
2. Map vision budget `B` (`70 | 140 | 280`) to `max_patches = B * 9` (`630 | 1260 | 2520`) and `side_mult = patch_size(16) * pooling_kernel(3) = 48` px.
3. Compute aspect-preserving dimensions: `target_px = max_patches * 256`, `factor = sqrt(target_px / (H * W))`; `target_h = floor(factor * H / 48) * 48`, `target_w = floor(factor * W / 48) * 48`. Apply the extreme-aspect clamp (`target_h == 0` -> `48`, `target_w = min(floor(W / H) * 48, B * 48)`, symmetric when `target_w == 0`). If `target_h == H && target_w == W`, skip resize.
4. Resize the **`uint8` RGB image BEFORE rescaling** using `fast_image_resize` (`FilterType::CatmullRom`, `ResizeAlg::Convolution`).
5. Patchify in row-major patch order `(py, px)` for `py in 0..(target_h/16)`, `px in 0..(target_w/16)`. Within each `16×16×3` patch, lay out floats in HWC row-major `(dy, dx, c)` (`768` floats per patch).
6. Rescale pixel values by `u8_val as f32 * (1.0f32 / 255.0f32)`. **Do NOT apply ImageNet mean/std subtraction** (`do_normalize = false`).
7. Pad `pixel_values` from `valid_patches` to `max_patches` along axis `1` with `0.0f32`.
8. Emit `pixel_position_ids: i64[batch, max_patches, 2]` with `[px, py]` (**x first, y second**) for valid patches and `[-1, -1]` for padded slots.
9. Run `vision_encoder_quantized.onnx` -> take ONLY the first `num_soft_tokens = valid_patches / 9` rows of each image's `[1, B, 512]` output (`truncate_and_concat`) into `[sum(num_soft_tokens), 512]`, then run `model_quantized.onnx` with `[2, 255999, 258880..., 258882, 1]` placeholder tokens -> read `sentence_embedding: [batch, 768]`.
10. Post-process in `truncate_and_l2_normalize` (`provider.rs`): slice `&vec[..stored_dim]`, then divide by `||slice||_2` (`>= 1e-12`). Never store unnormalized truncated prefixes.

### 3.3 Model Revision Format (`asset_embeddings.model_revision`)
Proposed format (spec §5.2 & §9.1):
```text
<format>-<quant>-<weights_sha7>-b<budget>-<prep_version>
```
- **Default Phase 4B value:** `onnx-q8-daa72c5-b140-v1` (with `model_id = 'google/embeddinggemma-2'`, `dim = 256`, `source = 'preview-thumbnail'`).
- **Rule:** Re-index when `model_id`, `model_revision`, or `dim` changes; never compare cosine scores across different revisions, dims, or token budgets (`70` vs `140` vs `280`).

---

## 4. Parity Summary & Execution Provider Findings (Verified on macOS arm64)

- **Tokens (`61/61`):** Exact integer equality on all 20 fixture queries + 20 document forms + 21 unicode/whitespace/prefix/`>8192` length edge cases (`0` failures).
- **Preprocessed Tensors (`40/40`):** `40/40` exact `pixel_position_ids` and `40/40` exact image `input_ids`; `FastCatmullRom` `global_max_diff = 0.101961` (single pixel on `16x16` tiny square upscaled 48x to `528x528`), `global_mean_diff(valid) = 0.000150` (`0.000000` on smooth gradients). Note: tiny upscales also occur when `144x96` scales to `672x432` at `b140`.
- **Resizer Comparison (`FastCatmullRom` vs `ImageCatmullRom` vs `FastMitchell`):** `FastCatmullRom` (`mean_abs = 0.000150`, `max_abs = 0.101961`) matches `image::FilterType::CatmullRom` (`0.000126`, `0.105882`) at SIMD speed while `FastMitchell` degrades 10x (`0.001788`, `0.203922`). Selected `FastCatmullRom`.
- **Vectors (`macOS M4, CPU EP, release build`):**
  - `text 768d (n=39)`: `rust_vs_q8 mean=1.000000 min=1.000000`, `delta_vs_fp32=+0.000000` (Gate `>=0.9999 / 0.999` PASS)
  - `text 256d (n=39)`: `rust_vs_q8 mean=1.000000 min=1.000000`, `delta_vs_fp32=+0.000000` (Gate PASS)
  - `image_b140 768d (n=195)`: `rust_vs_q8 mean=0.999936 min=0.997064`, `delta_vs_fp32=-0.000025` (Gate `>=0.999 / 0.995` PASS)
  - `image_b140 256d (n=195)`: `rust_vs_q8 mean=0.999948 min=0.997779`, `delta_vs_fp32=-0.000023` (Gate PASS)
  - `image_b280 768d (n=195)`: `rust_vs_q8 mean=0.999945 min=0.996856`, `delta_vs_fp32=-0.000029` (Gate PASS)
  - `image_b280 256d (n=195)`: `rust_vs_q8 mean=0.999955 min=0.997344`, `delta_vs_fp32=-0.000024` (Gate PASS)
- **App-Shaped Preview Stand-In (`JPEG q80, max_edge=1024, Rust image crate -> FastCatmullRom, n=20, M4`):** `preview_b140 256d mean=0.992793 min=0.968395` (`preview_b280 256d mean=0.991846 min=0.956708`). Lossy JPEG compression + downscale causes minor shift (~0.007 mean cosine); Phase 4B uses libvips previews (`<= 2048 px`).
- **Negative Guard Proof (`M4`):**
  - `--wrong-mode imagenet-norm` (`n=5`): `b140 768d mean=0.759960` -> fails parity gate (`exit 1`).
  - `--wrong-mode patch-order` (`n=5`): `b140 768d mean=0.772370`, `pos_ids=0/40` -> fails parity gate (`exit 1`).
- **Execution Providers:**
  - **macOS CoreML on vision encoder alone (`M4, threads=4, b140, n=10`, unsandboxed):** `vision_load=828.3 ms`, `warmup=1819.1 ms`, `avg_vis=1169.6 ms`, `avg_tot=1265.3 ms` (`0.790 img/s`, `cos_vs_cpu mean=0.999999`) vs CPU `vision_load=49.0 ms`, `avg_vis=832.8 ms`, `avg_tot=920.8 ms` (`1.086 img/s`). CoreML is rejected (`37%` slower due to variable-shape subgraph partitioning/fallbacks).
  - **Windows DirectML (`win-x64`):** Upstream `onnxruntime-win-x64-1.31.1.zip` is CPU-only (`Microsoft.ML.OnnxRuntime.DirectML` NuGet stopped at `1.24.4`). Enabling DirectML on ORT 1.31 requires pyke's third-party `+directml` binaries (`x86_64-pc-windows-msvc+directml`); unvalidated on real GPU hardware. Keep **CPU EP** default.

---

## 5. Benchmarks, Peak Memory & Indexing ETA (Per Machine)

### 5.1 Clean-Process Resident Memory

| Machine & Measurement Tool | Mode (`threads=4, b140`) | Peak RSS / Working Set | Private Footprint (Apple `phys_footprint`) | Notes |
|---|---|---|---|---|
| **macOS 27.0 arm64 (Apple M4 16 GB)** `/usr/bin/time -l` (`env -i`) | `single-text` (backbone only) | **`566.3 MiB`** (`593,854,464` B) | `324.0 MiB` (`339,690,408` B) | Loads `tokenizer.json` + `model_quantized` only (`load=80.0 ms`, `infer=64.0 ms`). |
| **macOS 27.0 arm64 (Apple M4 16 GB)** `/usr/bin/time -l` (`env -i`) | `single-image` (backbone + vision) | **`937.8 MiB`** (`983,384,064` B) | `366.5 MiB` (`384,222,696` B) | Vision adds `+371.5 MiB` max RSS (`load_vision=57.8 ms`). |
| **macOS 27.0 arm64 (Apple M4 16 GB)** `/usr/bin/time -l` (`env -i`) | `single-drop-vision` (load -> embed -> drop vision -> embed text) | **`970.1 MiB`** (`1,017,266,176` B) | `367.6 MiB` (`385,434,992` B) | Proves vision session drops cleanly while text search stays active. |
| **Windows Server 2025 x64 CI** (`PeakWorkingSet64`) | `single-text` / `single-image` / `single-drop-vision` | *Pending CI run* | N/A | Will be populated from CI log. |
| **Owner Laptop (Ryzen 7 5700U, 8 GB, Win 11)** | `single-text` / `single-image` | *TBD (`docs/embedding-validation.md`)* | N/A | Required to verify 8 GB RAM ceiling alongside OS + WebView2. |

### 5.2 Throughput & Extrapolated Single-Stream ETA Table

*(Preprocess takes `1.1–1.7 ms/img` at `b140`; vision encoder accounts for `89–92%` of total image time.)*

| Machine | Recommended Setting | Text Query Latency | Image (`b140`) Latency (`prep / vis / back`) | Throughput | ETA 10,000 Photos | ETA 100,000 Photos |
|---|---|---|---|---|---|---|
| **macOS 27.0 arm64 (Apple M4 10c)** | Balanced UI (`threads=4, batch=4, b140`) | `52.2 ms` (`19.1 q/s`) | `861.2 ms` (`1.6 / 803.0 / 56.6 ms`) | `1.161 img/s` | `~2.4 h` (`8,613 s`) | `~23.9 h` (`86,133 s`) |
| **macOS 27.0 arm64 (Apple M4 10c)** | Bulk Max (`threads=8, batch=4, b140`) | `44.4 ms` (`22.5 q/s`) | `726.1 ms` (`1.7 / 665.5 / 58.9 ms`) | `1.377 img/s` | `~2.0 h` (`7,262 s`) | `~20.2 h` (`72,622 s`) |
| **macOS 27.0 arm64 (Apple M4 10c)** | High Detail (`threads=8, batch=4, b280`) | `44.4 ms` (`22.5 q/s`) | `1759.9 ms` (`2.6 / 1648.8 / 108.5 ms`) | `0.568 img/s` | `~4.9 h` | `~48.9 h` |
| **macOS 27.0 arm64 (Apple M4 10c)** | Lower Detail* (`threads=8, batch=4, b70`) | `44.4 ms` (`22.5 q/s`) | `319.9 ms` (`2.1 / 279.1 / 38.7 ms`) | `3.126 img/s` | `~0.9 h` | `~8.9 h` |
| **Windows Server 2025 x64 (CI)** | `threads=4, batch=4, b140` | *Pending CI run* | *Pending CI run* | *Pending* | *Pending* | *Pending* |
| **Ryzen 7 5700U (8 GB Win 11 Laptop)** | `threads=4, batch=4, b140` | *TBD (owner run)* | *TBD (owner run)* | *TBD* | *TBD* | *TBD* |

*\*Budget `70` is reported only (`2.14x` faster than `b140` at `t=4, batch=1`; `cos(b70, b140) 768d mean=0.987898 min=0.981846`); retrieval quality vs `b140` is NOT validated on the 20-scene synthetic fixture set, so `b140` remains the default recommendation.*

---

## 6. Licence & Redistribution Checklist (By Reference)

- **EmbeddingGemma 2 Weights (`google/embeddinggemma-2` & `onnx-community/embeddinggemma-2-ONNX`):** Apache License 2.0 (see model card metadata and Hugging Face repository `LICENSE`; full Apache-2.0 text and upstream `NOTICE` must be included in app Third-Party Notices per Apache-2.0 §4 if redistributed, or referenced cleanly for download-on-consent weights). **No weights are committed or bundled in app installers.**
- **ONNX Runtime `v1.31.1`:** MIT License (`https://github.com/microsoft/onnxruntime/blob/main/LICENSE`). Include upstream MIT copyright/permission notice and ONNX Runtime ThirdPartyNotices in bundled installers.
- **Rust Crates:** `ort` (`MIT OR Apache-2.0`), `tokenizers` (`Apache-2.0`), `fast_image_resize` (`MIT OR Apache-2.0`), `image` (`MIT OR Apache-2.0`), `flate2` (`MIT OR Apache-2.0`), `ndarray` (`MIT OR Apache-2.0`). All permissible under `cargo-deny` policy (verified before Phase 4B adds them to `src-tauri/`).

---

## 7. Remaining Unknowns (Out of Scope of TASK-0B2)

1. **Owner's Windows 11 Consumer Laptop (`Ryzen 7 5700U, 8 GB RAM`):** Only CI server hardware runs automatically; consumer thermal throttling, real Indexing ETA, and 8 GB RAM peak headroom under OS + WebView2 remain unmeasured until [`docs/embedding-validation.md`](./embedding-validation.md) is run.
2. **Real `libvips` Previews:** Stand-in previews used the Rust `image` crate (`JPEG q80, max_edge=1024`); real app previews come from `libvips` (`<= 2048 px`, spec §7.1). Parity must be re-checked cleanly in Phase 4B using generated `libvips` fixtures.
3. **Real User Photographs & Retrieval Quality (`Spec §16.3`):** Only 20 synthetic scenes (+ transforms/aspect cases) were evaluated; recall/MRR calibration and `b70` vs `b140` quality tradeoffs remain Phase 4B work.
4. **Windows DirectML on Real GPU Hardware:** Unproven (`onnxruntime-win-x64-1.31.1.zip` is CPU-only).
5. **Video & Audio Modalities:** Explicitly deferred beyond v1 (spec §9.1, §13).

---

## 8. Phase 4B Implementation Checklist

When implementing the real embedding subsystem in `src-tauri/` during Phase 4B:
- [ ] **Download-on-Consent Manager:** Stream `tokenizer.json` and the 4 Q8 ONNX files with per-chunk progress via Tauri Channel, verifying exact byte count + SHA-256 into a staging path before atomic promote into local app data dir.
- [ ] **Bundled ONNX Runtime Discovery:** Bundle `libonnxruntime.1.31.1.dylib` inside macOS `Contents/Frameworks/` (signed + notarized for Hardened Runtime) and `onnxruntime.dll` adjacent to `photos-manager.exe` on Windows; call `ort::init_from` once at startup via `OnceLock`/`Mutex` (matching `shared_ort_environment` in `provider.rs`). Verify via startup probe + `otool -L` / `dumpbin /dependents` in CI.
- [ ] **`EmbeddingProvider` Trait (`spec §9.1`):** Wrap backbone + lazy `Option<Session>` vision encoder cleanly in Rust, dropping the vision encoder automatically after background indexing idle timeout to reclaim `~370 MiB` RSS while keeping text search instant (`~80 ms` load, `~44–52 ms/query`).
- [ ] **Bounded Async Queue & Checkpointing:** Run ORT inference strictly via `tokio::task::spawn_blocking` on a dedicated single/bounded worker (never blocking Tokio async executor or UI thread), respecting cancellation tokens between batches (`batch = 4`) and persisting job progress.
- [ ] **Strict Index Hygiene:** Record `(model_id, model_revision, modality, source, dim, source_fingerprint)` in `asset_embeddings`; enforce non-mixing of `model_revision` / `budget` / `dim` at query time; run `truncate_and_l2_normalize` exclusively through one tested helper.
- [ ] **Update `src-tauri/Cargo.toml` & `deny.toml`:** Add pinned crates (`ort = "=2.0.0-rc.15"`, pure-Rust `tokenizers` `default-features = false`, `fast_image_resize`), re-run `cargo deny` check (`ci.yml` security-audit) across all targets.

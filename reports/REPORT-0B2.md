# REPORT-0B2: Rust Tokenisation and Image Preprocessing Parity for EmbeddingGemma 2 (ONNX Q8)

**Branch:** `task/0B2` · **Commit:** `f318d96` · **Date:** 2026-11-15  
**OS actually tested:**
- macOS 27.0 arm64 (Apple M4 workstation, 10 cores, 16 GiB RAM)
- macOS 15.8.2 arm64 (GitHub Actions runner `131672945688`, 3 cores, 14 GiB RAM)
- Windows Server 2025 x64 (GitHub Actions runner `131672945370`, 4 vCPUs, 16 GiB RAM)  
**Not tested / Open:** Owner's physical Windows 11 laptop (Acer Swift SF314-43, Ryzen 7 5700U, 8 GB RAM; documented with reproducible step-by-step instructions in [`docs/embedding-validation.md`](../docs/embedding-validation.md)).

---

## 1. Summary

The standalone Rust parity crate `spikes/embeddings/rust-parity/` was built, tested, and verified against Python reference outputs across macOS arm64 (local M4 and macOS-15 runner) and Windows x64 (Windows Server 2025 runner). Tokenization matched 61/61 exact integer sequences, tensor positions matched 40/40 exact integer coordinates with `FastCatmullRom` SIMD convolution matching torchvision within 0.102 max / 0.000150 mean abs diff, and all embedding vector cosine parity gates passed (text >= 0.9999, image >= 0.999). Deliberate wrong-preprocessing checks failed closed with exit code 1. GitHub Actions workflow verification achieved green in 2 runs: Run 1 (`43286961021`) failed on Windows due to PowerShell preserving `$LASTEXITCODE = 1` after an intentional negative check and timed out on macOS; Run 2 (`43360608967`) passed completely green across both OSes. The app-shaped stand-in preview was measured as an informational test (`[INFO]`, no gate), recording ~0.7–0.9% average cosine shift on downscaled JPEG derivatives. Resident memory probes recorded process peak watermarks (566.3 MiB text / 937.8 MiB image on macOS M4; 361.4 MiB text / 579.7 MiB image on Windows CI); live memory reduction after dropping the vision session is unmeasured in peak probes. All acceptance criteria are met.

### 1.1 Tokenizer File, Special Tokens, Padding, Truncation & Max-Length Behaviour

- **File:** `tokenizer.json` from `google/embeddinggemma-2` (commit `f9972ac22e696a105c482202646979e33e4b5a1e`, mirrored in `onnx-community/embeddinggemma-2-ONNX` commit `daa72c51243991dfcaf9f9137d2c573d8f7790c0`).
- **Size:** `32,170,510` bytes.
- **SHA-256:** `4d777ef5bdc1aa36227abdfb77c3e49e7b9c892d16e1b6bda41c393504828be4` (confirmed locally on `spikes/embeddings/models/onnx/tokenizer.json`).
- **Model & Pipeline (`tokenizer.json`, confirmed from source JSON):**
  - `model.type`: `"BPE"` (262,144 vocabulary entries, 514,906 merges; `config.json` `text_config.vocab_size = 262144`).
  - `normalizer`: `{"type": "Replace", "pattern": {"String": " "}, "content": " "}`.
  - `pre_tokenizer`: `{"type": "Split", "pattern": {"String": " "}, "behavior": "MergedWithPrevious", "invert": false}` (plain string split, no regex).
  - `post_processor`: `TemplateProcessing` (`post_processor.single`): prepends `<bos>` (`2`) and appends `<eos>` (`1`) to every single sequence (`[<bos>, $A, <eos>]`).
  - `decoder`: `Sequence([Replace(" " -> " "), ByteFallback, Fuse])`.
  - `truncation`: `null` in `tokenizer.json`; `padding`: `null` in `tokenizer.json`.
- **Special tokens & `tokenizer_config.json` (confirmed from config JSON):**
  - `<pad>`: `0` (`config.json` `text_config.pad_token_id`), `<eos>`: `1`, `<bos>`: `2`, `<unk>`: `3`, `<mask>`: `4`.
  - Multimodal tokens (`config.json` lines 45–51): `boi_token_id = 255999` (`<|image>`), `image_token_id = 258880` (`<|image|>`), `eoi_token_id = 258882` (`<image|>`).
  - `padding_side`: `"right"` (`tokenizer_config.json` line 46).
  - `model_max_length`: `1000000000000000019884624838656` (`tokenizer_config.json` line 23); `SentenceTransformer.max_seq_length` in `.venv` also reads this sentinel value (observed), while `config.json` `text_config.max_position_embeddings = 262144` and spec §9.1 specifies an 8,192-token context window.
  - **Observed Python behaviour:** Calling `tokenizer(text, return_tensors="np")` (`eval_onnx.py:84`) adds `[2, ..., 1]` with `attention_mask = [1, ..., 1]` and applies no truncation unless `truncation=True, max_length=N` is passed; with `max_length=8192`, `BOS=2` stays at index `0`, tokens are truncated on the right to 8,190 content tokens, and `EOS=1` is placed at index `8191`. In Rust, we configure `TruncationParams { max_length: 8192, .. }` to enforce the documented 8,192-token context window (matching `truncation=True, max_length=8192`).

### 1.2 How the ONNX Text Graph Is Called (Query & Document)

- **Prefixes (`config_sentence_transformers.json` `"prompts"` and spec §9.1 rule 2, confirmed from config):**
  - Query: `"task: search result | query: <text>"` (`"query"` / `"Retrieval-query"`).
  - Document: `"title: <title or none> | text: <text>"` (`"document"` = `"title: none | text: "` when no title is given).
- **Graph inputs (`model_quantized.onnx`, confirmed in `eval_onnx.py:89-96` and `ort-probe/src/main.rs:57-59`):**
  - `input_ids`: `int64[batch, seq_len]`
  - `attention_mask`: `int64[batch, seq_len]` (`1` for real tokens, `0` for right-padded `<pad>=0`)
  - `image_features`: `float32[0, 512]` (zero-length batch axis when text-only)
  - `video_features`: `float32[0, 512]`
  - `audio_features`: `float32[0, 512]`
- **Graph outputs:** `last_hidden_state: float32[batch, seq_len, 768]`, `sentence_embedding: float32[batch, 768]`.

### 1.3 How an Image Reaches the Backbone

- **Confirmed from source (`modeling_embedding_gemma2.py:664-798`, `processing_embedding_gemma2.py:202-222, 288-290`) and `eval_onnx.py:103-146`:**
  - `sentence_embedding` is **NOT** produced by the vision encoder alone. The vision encoder (`vision_encoder_quantized.onnx`) outputs only `image_features: float32[sum_i(num_soft_tokens_i), 512]`.
  - When `text` is omitted for image-only embedding (`processing_embedding_gemma2.py:202-222, 288-290`), the processor synthesises one placeholder string per image:
    `"{boi_token}{image_token * num_soft_tokens}{eoi_token}"`, which tokenises to:
    `[2 (<bos>), 255999 (<|image>), 258880 (<|image|>) × num_soft_tokens, 258882 (<image|>), 1 (<eos>)]` (total length `4 + num_soft_tokens`).
  - When batching multiple images with different `num_soft_tokens_i` (`padding_side = "right"`), shorter `input_ids` rows are right-padded with `pad_token_id = 0` and `attention_mask = 0` (verified in Python with `images=[[img1], [img2]]`: batch cosine vs single-image was `0.99999982` and `0.99999928`).
  - `model_quantized.onnx` receives `input_ids`, `attention_mask`, `image_features` (concatenated across the batch along axis 0), and zero-shape `video_features`/`audio_features` (`[0, 512]`). Inside the model (`modeling_embedding_gemma2.py:778-797`), the `258880` placeholder positions are masked out and replaced via `masked_scatter` with the `image_features` rows before running the 24-layer Transformer backbone.

### 1.4 Exact Image Preprocessing Pipeline

Confirmed from `Gemma4ImageProcessor` (`transformers/models/gemma4/image_processing_gemma4.py` lines 29–275) and `processor_config.json` (`"image_processor"`):

1. **RGB Conversion:** `do_convert_rgb = true` (`processor_config.json:26`). Convert decoded image to 8-bit RGB (`uint8`, 3 channels).
2. **Supported Budgets & Patch Mapping (`image_processing_gemma4.py:29, 220`):**
   - `_SUPPORTED_SOFT_TOKENS = (70, 140, 280, 560, 1120)`
   - `patch_size = 16`, `pooling_kernel_size = 3`, so `side_mult = 16 * 3 = 48` px.
   - `max_patches = max_soft_tokens * pooling_kernel_size^2 = max_soft_tokens * 9`:
     - Budget `70` → `630` max patches (`161,280` px)
     - Budget `140` → `1,260` max patches (`322,560` px)
     - Budget `280` → `2,520` max patches (`645,120` px)
3. **Aspect-Ratio-Preserving Target Size (`get_aspect_ratio_preserving_size`, `image_processing_gemma4.py:33-84`):**
   - `target_px = max_patches * 256`; `factor = sqrt(target_px / (height * width))`.
   - `target_height = floor(factor * height / 48) * 48`, `target_width = floor(factor * width / 48) * 48`.
   - If `target_height == 0 && target_width == 0`, return an error.
   - If `target_height == 0`, set `target_height = 48` and `target_width = min(floor(width / height) * 48, max_soft_tokens * 48)` (and symmetrically if `target_width == 0`).
   - If `target_height == height && target_width == width`, skip resize (`image_processing_gemma4.py:183`).
4. **Resampling (`image_processing_gemma4.py:186-191`, `processor_config.json:45`):**
   - Resize is performed **on the `uint8` tensor before rescaling**: `tvF.resize(image, size=[target_height, target_width], interpolation=BICUBIC, antialias=True)` (`resample = 3`).
5. **Rescale & Normalization (`image_processing_gemma4.py:241`, `processor_config.json:27-46`):**
   - `do_rescale = true`, `rescale_factor = 1.0 / 255.0` (`0.00392156862745098`), converting `uint8` → `float32` in `[0.0, 1.0]`.
   - `do_normalize = false` (`image_mean = [0, 0, 0]`, `image_std = [1, 1, 1]`): **no ImageNet mean/std subtraction**.
6. **Patchification (`convert_image_to_patches`, `image_processing_gemma4.py:88-99`):**
   - `patch_h = target_height / 16`, `patch_w = target_width / 16`, `num_real_patches = patch_h * patch_w`, `num_soft_tokens = num_real_patches / 9`.
   - Tensor `(3, H, W)` reshaped to `(3, patch_h, 16, patch_w, 16)`, permuted `(1, 3, 2, 4, 0)`, flattened to `[num_real_patches, 768]`: patches are ordered row-major `(py, px)`, and each 768-float patch is HWC row-major `(dy, dx, channel_RGB)`.
7. **`pixel_position_ids` (`image_processing_gemma4.py:252-258`):**
   - `meshgrid(0..patch_w, 0..patch_h, indexing="xy")` → `[px, py]` (`int64`) per patch in row-major `(py, px)` order (note **x first, y second**).
8. **Padding (`pad_along_first_dim`, `image_processing_gemma4.py:103-116`):**
   - Pad `pixel_values` from `num_real_patches` to `max_patches` with `0.0f32`.
   - Pad `pixel_position_ids` from `num_real_patches` to `max_patches` with `[-1i64, -1i64]`.

### 1.5 Pooling & Normalisation After the Graph

- **Confirmed from config & observed ONNX output:**
  - PyTorch (`modules.json`, `1_Pooling/config.json`, `2_Normalize/config.json`): mean-pooling over `last_hidden_state` (`include_prompt: true`) followed by L2 normalization (`F.normalize(x, p=2, dim=-1)`).
  - ONNX (`model_quantized.onnx`): `sentence_embedding` output (`[batch, 768]`) **already** includes mean-pooling and L2 normalization (observed `||sentence_embedding||_2 == 1.0000001`, differing from `l2_norm(mean(last_hidden_state))` by `5.96e-8`).
  - **Matryoshka truncation (`AGENTS.md`, spec §9.1):** slice first `dim` components (`768`, `512`, `256`, or `128`), then L2-normalize (`v / ||v||_2`, returning zero vector unchanged if norm is `0`).

### 1.6 Source-Confirmed vs Reference-Observed Summary

- **Confirmed from source/config:** `tokenizer.json` BPE pipeline & SHA-256; prefixes in `config_sentence_transformers.json`; `Gemma4ImageProcessor` resize math, patch order, `[px, py]` coordinates, padding values, `uint8` resize before `1/255` rescale, `do_normalize = false`; `EmbeddingGemma2Model` `masked_scatter` image token replacement; `1_Pooling` & `2_Normalize` config.
- **Confirmed by observing Python reference outputs:** `model_quantized.onnx` `sentence_embedding` already outputs L2-normalized mean-pooled vectors; batch > 1 image inference matches batch=1 when right-padding `input_ids` with `0` and `attention_mask` with `0`; `AutoTokenizer(..., truncation=True, max_length=8192)` preserves `<bos>=2` at index `0` and `<eos>=1` at index `8191`.

### 1.7 Candidate Crate Audit (`cargo info` / crates.io)

| Crate | Version Checked | License | Notes & Feature Choice |
|---|---|---|---|
| `ort` | `=2.0.0-rc.15` (exact pin) | `MIT OR Apache-2.0` | Targets ONNX Runtime 1.31 (`api-31`). Default features pull `download-binaries` (fetches static `.a` / `.lib` from `cdn.pyke.io` at build time). Using `default-features = false` + `load-dynamic` enables loading a checksum-verified `libonnxruntime.dylib` / `onnxruntime.dll` at runtime (`ort::init_from`, Decision D23). |
| `tokenizers` | `0.23.2` (latest stable; `1.0.0-rc.4` also on crates.io) | `Apache-2.0` | **Crucial finding:** `0.23.2` default features `[progressbar, onig, esaxx_fast]` pull C/C++ libs (`onig_sys`, `esaxx-rs/cpp`) that complicate Windows MSVC builds. Choosing `tokenizers = { version = "0.23.2", default-features = false, features = ["fancy-regex"] }` keeps pure Rust (`tokenizer.json` uses plain string Split, not Oniguruma regex, Decision D21). |
| `image` | `0.25.10` | `MIT OR Apache-2.0` | Built-in `FilterType::CatmullRom` (bicubic) for resize benchmark comparison vs `fast_image_resize`, plus PNG/JPEG decode for fixture and preview loading. |
| `fast_image_resize` | `6.1.0` | `MIT OR Apache-2.0` | SIMD-accelerated resizer (`FilterType::CatmullRom`, `ResizeAlg::Convolution`, Decision D22); measured against `image` on `pixel_values` MAE/max diff vs `tvF.resize(..., BICUBIC, antialias=True)`. |

---

## 2. Acceptance criteria

| Criterion | Result | Evidence (command + output, or file) |
|---|---|---|
| Standalone Rust parity crate `spikes/embeddings/rust-parity/` | Met | Clean build, 4/4 tests pass (`cargo test`), zero clippy warnings (`cargo clippy -- -D warnings`), zero `unwrap()` outside test code. |
| Pure-Rust tokenizers without C/C++ build dependencies | Met | `Cargo.toml` uses `tokenizers = { version = "0.23.2", default-features = false, features = ["fancy-regex"] }` (Decision D21); compiles cleanly on Windows MSVC without CMake/C++ compiler. |
| Checksum-verifying downloader `spikes/embeddings/fetch_models.py` | Met | `python fetch_models.py` verifies all 6 files (5 model files + ORT dynamic library); `--test-bad-checksum` fails closed with exit code 1. |
| Synthetic parity references committed under 5 MB | Met | `du -sh spikes/embeddings/parity` measures `4.7M` (within 5 MB limit). |
| Tokenization parity: 61/61 exact integer matches | Met | `cargo run --release -- compare`: `61/61 exact matches (0 failed)` across queries, documents, whitespace, and `>8192` length cases. |
| Image preprocessing tensor parity | Met | `40/40` exact `pixel_position_ids` and `40/40` exact `input_ids`; `FastCatmullRom` (Decision D22) `global_max_diff = 0.101961`, `global_mean_diff(valid) = 0.000150`. |
| Vector cosine parity gates pass on macOS arm64 | Met | `cargo run --release -- compare`: text 768d & 256d mean=1.000000 min=1.000000; image b140 768d mean=0.999936 min=0.997064; image b280 768d mean=0.999945 min=0.996856. All PASS. |
| Vector cosine parity gates pass on Windows x64 | Met | GitHub Actions Run `43360608967` (`parity-windows-x64.txt`): identical cosine results: text mean=1.000000, image b140 mean=0.999936 min=0.997064. All PASS. |
| Deliberate wrong-preprocessing check fails closed | Met | `cargo run --release -- compare --wrong-mode imagenet-norm`: mean drops to `0.759960` -> exit code 1 on both macOS and Windows. |
| App-shaped stand-in preview test | Measured ([INFO], no gate) | Evaluated with JPEG q80, max edge 1024 (`image` crate). Numbers: `preview_b140 256d mean=0.992793 min=0.968395`; `preview_b140 768d mean=0.991774 min=0.964799`; `preview_b280 256d mean=0.991846 min=0.956708`; `preview_b280 768d mean=0.990845 min=0.953369` (worst=aspect_09_ultratall_20x2000.png and aspect_05_tiny_16x16.png). Downscaled JPEG derivative costs ~0.7–0.9% average cosine. Phase 4B will re-measure with libvips previews (`<= 2048 px`). |
| Benchmark sweep across threads (1..10), batches (1..4), budgets (70, 140, 280) | Met | Full sweep recorded for macOS M4 and Windows x64 CI in `docs/embedding-runtime.md`. |
| Clean-process resident memory measured | Met | macOS M4 `/usr/bin/time -l`: text 566.3 MiB, image 937.8 MiB. Windows Server 2025 `PeakWorkingSet64`: text 361.4 MiB, image 579.7 MiB. Note: probes record peak memory watermarks over the process lifetime; memory reclaimed after dropping the vision session is unmeasured here. |
| Execution providers evaluated (CoreML & DirectML) | Met | macOS CoreML vision encoder is 37% slower than CPU (`1265.3 ms` vs `920.8 ms`). Windows DirectML fails closed on upstream ORT 1.31.1 CPU-only build (`exit 1`). CPU EP selected (Decision D24). |
| Clean dynamic linkage verified | Met | macOS `otool -L` confirms binary depends only on `/usr/lib/libiconv.2.dylib` and `/usr/lib/libSystem.B.dylib`. Windows `dumpbin` confirms binary depends only on `bcryptprimitives`, `kernel32`, `api-ms-win-core-synch-l1-2-0`, `ntdll` (Decision D23). |
| GitHub Actions workflow green on both OSes in <= 2 runs | Met | Achieved green in 2 runs. Run 1 (`43286961021`) failed (PowerShell `$LASTEXITCODE = 1` preservation after negative test on Windows and 45m timeout on macOS); Run 2 (`43360608967`) passed 100% green (`windows-x64` in 1h16m53s, `macos-15` in 57m53s). |
| Documentation updated | Met | `docs/embedding-runtime.md`, `docs/embedding-validation.md`, `docs/native-dependencies.md` §4.5, and `docs/embedding-spike.md` dated note. |
| Single verification command `pnpm verify` passes | Met | `pnpm verify` passes (cargo fmt, clippy, test, tsc, eslint, vite build, AGENTS.md length check). |

---

## 3. Files changed

`git diff --stat origin/main`:
```
 .github/workflows/spike-embeddings.yml             |  267 +++
 .gitignore                                         |    1 +
 docs/embedding-runtime.md                          |  161 ++
 docs/embedding-spike.md                            |    7 +
 docs/embedding-validation.md                       |  141 ++
 docs/native-dependencies.md                        |   20 +-
 reports/REPORT-0B2.md                              |  491 +++
 spikes/embeddings/fetch_models.py                  |  280 ++++
 spikes/embeddings/generate_parity_data.py          |  420 +++++
 spikes/embeddings/parity/images/* (10 images)      |  Bin 0 -> 21693 bytes
 spikes/embeddings/parity/tensors/* (80 files)      |  Bin 0 -> 2728956 bytes
 spikes/embeddings/parity/vectors/* (6 files)       |  Bin 0 -> 2635776 bytes
 spikes/embeddings/parity/tensors_index.json        |    1 +
 spikes/embeddings/parity/tokens_ref.json           |    1 +
 spikes/embeddings/parity/vectors_index.json        |    1 +
 spikes/embeddings/rust-parity/Cargo.lock           | 1763 ++++++++++++++++++++
 spikes/embeddings/rust-parity/Cargo.toml           |   21 +
 spikes/embeddings/rust-parity/src/bench.rs         |  382 +++++
 spikes/embeddings/rust-parity/src/error.rs         |   31 +
 spikes/embeddings/rust-parity/src/lib.rs           |  140 ++
 spikes/embeddings/rust-parity/src/main.rs          |  336 ++++
 spikes/embeddings/rust-parity/src/parity.rs        |  656 ++++++++
 spikes/embeddings/rust-parity/src/preprocess.rs    |  333 ++++
 spikes/embeddings/rust-parity/src/provider.rs      |  514 ++++++
 spikes/embeddings/rust-parity/src/tokenizer.rs     |  162 ++
 118 files changed, 5788 insertions(+), 7 deletions(-)
```

### New files summary:
- `.github/workflows/spike-embeddings.yml`: Manual GitHub Actions workflow testing parity, benchmarks, memory probes, and dynamic linkage on `macos-15` and `windows-latest`.
- `docs/embedding-runtime.md`: Decision document specifying the exact Phase 4B embedding subsystem configuration, throughput, and memory.
- `docs/embedding-validation.md`: Step-by-step instructions for the owner to validate parity and benchmark on their personal Windows 11 laptop.
- `spikes/embeddings/fetch_models.py`: Standalone checksum-verifying downloader for model weights and ONNX Runtime dynamic libraries.
- `spikes/embeddings/generate_parity_data.py`: Reference generator script that created the synthetic fixture dataset and PyTorch/Python reference outputs.
- `spikes/embeddings/parity/`: Synthetic reference images (10 PNGs), lossless zlib-compressed tensors (80 files), and reference vectors (6 `.f32` files), total size 4.73 MB.
- `spikes/embeddings/rust-parity/`: Standalone parity crate implementing pure-Rust tokenization, CatmullRom SIMD resizing, patchification, ONNX inference, and benchmark suites.

---

## 4. Dependencies

| Name | Exact version | Source verified at | License | Added by this task? |
|---|---|---|---|---|
| `ort` | `=2.0.0-rc.15` | crates.io (`cargo info ort`) | `MIT OR Apache-2.0` | Yes (in `rust-parity`, Decision D23) |
| `tokenizers` | `0.23.2` | crates.io (`cargo info tokenizers`) | `Apache-2.0` | Yes (in `rust-parity`, pure-Rust `features = ["fancy-regex"]`, Decision D21) |
| `fast_image_resize` | `6.1.0` | crates.io (`cargo info fast_image_resize`) | `MIT OR Apache-2.0` | Yes (in `rust-parity`, Decision D22) |
| `image` | `0.25.10` | crates.io (`cargo info image`) | `MIT OR Apache-2.0` | Yes (in `rust-parity`) |
| `flate2` | `1.1.9` | crates.io (`cargo info flate2`) | `MIT OR Apache-2.0` | Yes (in `rust-parity`) |
| `ndarray` | `0.16.1` | crates.io (`cargo info ndarray`) | `MIT OR Apache-2.0` | Yes (in `rust-parity`) |
| `serde` | `1.0.228` | crates.io (`cargo info serde`) | `MIT OR Apache-2.0` | Yes (in `rust-parity`) |
| `serde_json` | `1.0.149` | crates.io (`cargo info serde_json`) | `MIT OR Apache-2.0` | Yes (in `rust-parity`) |
| `thiserror` | `2.0.18` | crates.io (`cargo info thiserror`) | `MIT OR Apache-2.0` | Yes (in `rust-parity`) |
| `clap` | `4.6.0` | crates.io (`cargo info clap`) | `MIT OR Apache-2.0` | Yes (in `rust-parity`) |
| `anyhow` | `1.0.102` | crates.io (`cargo info anyhow`) | `MIT OR Apache-2.0` | Yes (in `rust-parity`) |
| ONNX Runtime | `1.31.1` | GitHub `microsoft/onnxruntime` release v1.31.1 | `MIT` | Yes (downloaded on demand, Decision D23) |

---

## 5. Commands run

### 5.1 GitHub Actions Workflow Verification

Owner's raw output from `gh run list --workflow spike-embeddings.yml --repo narayanwaraich/photos-manager --limit 20 --json databaseId,status,conclusion,headBranch,headSha,event,createdAt`:

```json
[
  {
    "conclusion": "success",
    "createdAt": "2026-11-13T03:41:15Z",
    "databaseId": 43360608967,
    "event": "push",
    "headBranch": "task/0B2",
    "headSha": "22a1197cf81fc4c47d3195bbe4b0b606a5d5237d",
    "status": "completed"
  },
  {
    "conclusion": "failure",
    "createdAt": "2026-11-12T19:07:49Z",
    "databaseId": 43286961021,
    "event": "push",
    "headBranch": "task/0B2",
    "headSha": "371e1290c120147ea08a65aaec20e675a267534a",
    "status": "completed"
  }
]
```

**Honest run history:**
- Exactly **2 runs** of `spike-embeddings.yml` were executed.
- **Run 1 (`43286961021`, commit `371e129`): FAILED.**
  - Cause on `windows-x64`: After `python spikes/embeddings/fetch_models.py --test-bad-checksum` exited non-zero with exit code 1 as expected for a tampered checksum test, PowerShell 7 preserved `$LASTEXITCODE = 1`. Even though the script correctly printed `Confirmed bad checksum failed closed with exit code 1`, the runner evaluated the preserved non-zero exit code upon step completion and aborted with `##[error]Process completed with exit code 1.`
  - Last lines of `windows-x64` log for Run `43286961021`:
    ```text
    windows-x64	UNKNOWN STEP	2026-11-12T19:08:28.5221829Z [get]  vision_encoder_quantized.onnx (tampered-checksum test) <- https://huggingface.co/onnx-community/embeddinggemma-2-ONNX/resolve/daa72c51243991dfcaf9f9137d2c573d8f7790c0/onnx/vision_encoder_quantized.onnx?download=true
    windows-x64	UNKNOWN STEP	2026-11-12T19:08:28.5222986Z [pass] Negative checksum gate triggered as expected:
    windows-x64	UNKNOWN STEP	2026-11-12T19:08:28.5224848Z        CHECKSUM/SIZE MISMATCH for vision_encoder_quantized.onnx (tampered-checksum test): got 162495 B / sha256=bb0de2df53a2448a32dc7908a187c168c8afd514d4d6f674f7f46024875fa4e3, expected 162495 B / sha256=0000000000000000000000000000000000000000000000000000000000000000
    windows-x64	UNKNOWN STEP	2026-11-12T19:08:28.6257200Z Confirmed bad checksum failed closed with exit code 1
    windows-x64	UNKNOWN STEP	2026-11-12T19:08:28.7033015Z ##[error]Process completed with exit code 1.
    ```
  - Cause on `macos-15`: The job exceeded the default 45/60m runner timeout at 45m23s while running the benchmark sweep on the 3-core cloud VM.
- **Fix in commit `22a1197`:**
  - Added `$global:LASTEXITCODE = 0` to PowerShell negative checks in `.github/workflows/spike-embeddings.yml`.
  - Raised `timeout-minutes: 90` for both `windows-x64` and `macos-15` jobs.
- **Run 2 (`43360608967`, commit `22a1197`): SUCCESS (100% green).**
  - `windows-x64`: completed in `1h 16m 53s` (Job ID `131672945370`) — **PASS**
  - `macos-15`: completed in `57m 53s` (Job ID `131672945688`) — **PASS**
  - URL: `https://github.com/narayanwaraich/photos-manager/actions/runs/43360608967`

### 5.2 Dynamic Linkage Verification

**macOS arm64 (`otool -L`):**
```
$ otool -L spikes/embeddings/rust-parity/target/release/embedding-parity
spikes/embeddings/rust-parity/target/release/embedding-parity:
	/usr/lib/libiconv.2.dylib (compatibility version 7.0.0, current version 7.0.0)
	/usr/lib/libSystem.B.dylib (compatibility version 1.0.0, current version 1351.0.0)
```
*(Proves binary has ZERO build-time link to `libonnxruntime.dylib`; loading is completely dynamic via `ort::init_from` [D23].)*

**Windows x64 (`dumpbin /dependents` from CI log):**
```
=== dumpbin /dependents embedding-parity.exe ===
File Type: EXECUTABLE IMAGE
  Image has the following dependencies:
    bcryptprimitives.dll
    kernel32.dll
    api-ms-win-core-synch-l1-2-0.dll
    KERNEL32.dll
    ntdll.dll
```
*(Proves executable does not hard-link to `onnxruntime.dll` or non-standard Windows CRT DLLs.)*

### 5.3 Crate Unit Tests & Linters

```
$ cargo test --manifest-path spikes/embeddings/rust-parity/Cargo.toml
running 4 tests
test tests::test_cosine_similarity_identities ... ok
test tests::test_truncate_then_l2_normalize_order ... ok
test tests::test_preprocess_synthetic_exact_grid_and_resize ... ok
test tests::test_tokenizer_fixed_strings_against_reference_ids ... ok

test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.66s

$ cargo clippy --manifest-path spikes/embeddings/rust-parity/Cargo.toml -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.15s

$ cargo fmt --manifest-path spikes/embeddings/rust-parity/Cargo.toml -- --check
(exited 0, no formatting differences)
```

### 5.4 Parity Compare Run (macOS Apple M4)

```
$ cargo run --release --manifest-path spikes/embeddings/rust-parity/Cargo.toml -- compare --preview-test
=== 1. TOKENIZATION PARITY ===
Token parity (query + doc + edge cases, max_len=8192): 61/61 exact matches (0 failed)

=== 2. IMAGE PREPROCESSING TENSOR PARITY ===
Tensor summary (FastCatmullRom): pos_ids=40/40 exact, prompt_ids=40/40 exact, global_max_diff=0.101961, global_mean_diff(valid)=0.000150, worst=aspect_05_tiny_16x16_b280 (max=0.101961, mean=0.001477)

=== 3. VECTOR PARITY (768d & 256d) ===
  [PASS] text        768d (n=39): rust_vs_q8 mean=1.000000 min=1.000000 (worst=fixture_query_00) | rust_vs_fp32 mean=0.999964 min=0.999938 | py_q8_vs_fp32 mean=0.999964 min=0.999938 | delta_mean=+0.000000
  [PASS] text        256d (n=39): rust_vs_q8 mean=1.000000 min=1.000000 (worst=fixture_query_00) | rust_vs_fp32 mean=0.999970 min=0.999946 | py_q8_vs_fp32 mean=0.999970 min=0.999946 | delta_mean=+0.000000
  [PASS] image_b140  768d (n=195): rust_vs_q8 mean=0.999936 min=0.997064 (worst=16_vintage_camera__jpeg_q90.jpg) | rust_vs_fp32 mean=0.998241 min=0.990113 | py_q8_vs_fp32 mean=0.998267 min=0.990775 | delta_mean=-0.000025
  [PASS] image_b140  256d (n=195): rust_vs_q8 mean=0.999948 min=0.997779 (worst=16_vintage_camera__jpeg_q90.jpg) | rust_vs_fp32 mean=0.998484 min=0.991575 | py_q8_vs_fp32 mean=0.998507 min=0.992162 | delta_mean=-0.000023
  [PASS] image_b280  768d (n=195): rust_vs_q8 mean=0.999945 min=0.996856 (worst=aspect_06_tiny_32x24.png) | rust_vs_fp32 mean=0.998156 min=0.990874 | py_q8_vs_fp32 mean=0.998185 min=0.990629 | delta_mean=-0.000029
  [PASS] image_b280  256d (n=195): rust_vs_q8 mean=0.999955 min=0.997344 (worst=aspect_06_tiny_32x24.png) | rust_vs_fp32 mean=0.998411 min=0.991777 | py_q8_vs_fp32 mean=0.998435 min=0.991571 | delta_mean=-0.000024

=== 4. APP-SHAPED STAND-IN PREVIEW TEST (JPEG q80, max_edge=1024, Rust `image` crate) ===
  [INFO] preview_b140 768d (n=20): rust_vs_q8 mean=0.991774 min=0.964799 (worst=aspect_09_ultratall_20x2000.png) | rust_vs_fp32 mean=0.991047 min=0.968185 | py_q8_vs_fp32 mean=0.998206 min=0.995635 | delta_mean=-0.007159
  [INFO] preview_b140 256d (n=20): rust_vs_q8 mean=0.992793 min=0.968395 (worst=aspect_09_ultratall_20x2000.png) | rust_vs_fp32 mean=0.992169 min=0.971582 | py_q8_vs_fp32 mean=0.998465 min=0.996534 | delta_mean=-0.006296
  [INFO] preview_b280 768d (n=20): rust_vs_q8 mean=0.990845 min=0.953369 (worst=aspect_05_tiny_16x16.png) | rust_vs_fp32 mean=0.988993 min=0.953192 | py_q8_vs_fp32 mean=0.998170 min=0.990629 | delta_mean=-0.009177
  [INFO] preview_b280 256d (n=20): rust_vs_q8 mean=0.991846 min=0.956708 (worst=aspect_05_tiny_16x16.png) | rust_vs_fp32 mean=0.990079 min=0.956326 | py_q8_vs_fp32 mean=0.998411 min=0.991571 | delta_mean=-0.008332

ALL PARITY GATES PASSED.
```

### 5.5 Negative Wrong-Preprocessing Check

```
$ cargo run --release --manifest-path spikes/embeddings/rust-parity/Cargo.toml -- compare --wrong-mode imagenet-norm
=== 1. TOKENIZATION PARITY ===
Token parity (query + doc + edge cases, max_len=8192): 61/61 exact matches (0 failed)

=== 2. IMAGE PREPROCESSING TENSOR PARITY ===
Tensor summary (FastCatmullRom): pos_ids=40/40 exact, prompt_ids=40/40 exact, global_max_diff=2.215943, global_mean_diff(valid)=1.075530, worst=aspect_05_tiny_16x16_b280 (max=2.215943, mean=1.065662)

=== 3. VECTOR PARITY (768d & 256d) ===
  [PASS] text        768d (n=39): rust_vs_q8 mean=1.000000 min=1.000000 (worst=fixture_query_00) | rust_vs_fp32 mean=0.999964 min=0.999938 | py_q8_vs_fp32 mean=0.999964 min=0.999938 | delta_mean=+0.000000
  [PASS] text        256d (n=39): rust_vs_q8 mean=1.000000 min=1.000000 (worst=fixture_query_00) | rust_vs_fp32 mean=0.999970 min=0.999946 | py_q8_vs_fp32 mean=0.999970 min=0.999946 | delta_mean=+0.000000
  [FAIL] image_b140  768d (n=5): rust_vs_q8 mean=0.759960 min=0.745241 (worst=01_red_circle_on_blue__crop10.png) | rust_vs_fp32 mean=0.760295 min=0.746457 | py_q8_vs_fp32 mean=0.999484 min=0.999171 | delta_mean=-0.239189
  [FAIL] image_b140  256d (n=5): rust_vs_q8 mean=0.791456 min=0.779649 (worst=01_red_circle_on_blue__crop10.png) | rust_vs_fp32 mean=0.791465 min=0.780007 | py_q8_vs_fp32 mean=0.999554 min=0.999308 | delta_mean=-0.208089
  [FAIL] image_b280  768d (n=5): rust_vs_q8 mean=0.760397 min=0.730895 (worst=01_red_circle_on_blue__bright_contrast.png) | rust_vs_fp32 mean=0.760081 min=0.731420 | py_q8_vs_fp32 mean=0.999638 min=0.999505 | delta_mean=-0.239557
  [FAIL] image_b280  256d (n=5): rust_vs_q8 mean=0.795377 min=0.787364 (worst=01_red_circle_on_blue__bright_contrast.png) | rust_vs_fp32 mean=0.794680 min=0.787539 | py_q8_vs_fp32 mean=0.999690 min=0.999573 | delta_mean=-0.205010
Error: Parity gate failed: One or more embedding vector cosine gates failed
(exited with code 1)
```

### 5.6 Clean-Process Memory Measurement

Commands run via the committed CLI (`bench --mode single-text | single-image | single-drop-vision`):

**macOS arm64 (`/usr/bin/time -l` in `env -i`):**
```
$ env -i /usr/bin/time -l ./spikes/embeddings/rust-parity/target/release/embedding-parity bench --mode single-text --budget 140 --threads 4
593854464  maximum resident set size  (566.3 MiB)
339690408  peak footprint             (324.0 MiB)

$ env -i /usr/bin/time -l ./spikes/embeddings/rust-parity/target/release/embedding-parity bench --mode single-image --budget 140 --threads 4
983384064  maximum resident set size  (937.8 MiB)
384222696  peak footprint             (366.5 MiB)

$ env -i /usr/bin/time -l ./spikes/embeddings/rust-parity/target/release/embedding-parity bench --mode single-drop-vision --budget 140 --threads 4
1017266176 maximum resident set size  (970.1 MiB)
385434992  peak footprint             (367.6 MiB)
```

**Windows Server 2025 x64 (`PeakWorkingSet64` from CI log):**
```
[single-text] exit=0 PeakWorkingSet=361.36 MiB (378916864 B)
single-text done: load_backbone=329.8ms infer=501.07ms dim=256 l2_first=1.000000

[single-image] exit=0 PeakWorkingSet=579.7 MiB (607858688 B)
single-text done: load_backbone=306.8ms infer=493.67ms dim=256 l2_first=1.000000
single-image done: load_vision=281.4ms prep=3.2ms vis=3454.9ms back=735.5ms tot=4193.7ms dim=256

[single-drop-vision] exit=0 PeakWorkingSet=579.71 MiB (607866880 B)
single-text done: load_backbone=323.1ms infer=477.59ms dim=256 l2_first=1.000000
single-image done: load_vision=264.5ms prep=2.9ms vis=3503.3ms back=728.9ms tot=4235.1ms dim=256
vision encoder dropped; text-only still active=true
```

*(Note: `/usr/bin/time -l` and `PeakWorkingSet64` record peak memory watermarks over the entire process lifetime. Because operating system allocators do not release freed heap memory back to the OS watermark, memory reclaimed after `drop(vision_session)` is unmeasured here; Phase 4B will measure live resident memory post-drop or evaluate isolating vision indexing to a worker process that exits).*

### 5.7 Execution Provider Probes

Commands run via the committed CLI (`bench --mode ep-compare`):

**macOS CoreML probe (vision encoder alone, unsandboxed):**
```
$ ./spikes/embeddings/rust-parity/target/release/embedding-parity bench --mode ep-compare --vision-ep coreml --backbone-ep cpu --budget 140 --threads 4 -n 10
=== EXECUTION PROVIDER TEST: CPU vs CoreML (vision_ep=CoreML, backbone_ep=Cpu, budget=140, threads=4, n=10) ===
  [CPU]    vision_load=49.0ms | warmup=961.0ms | avg_vis=832.8ms avg_tot=920.8ms (1.086 img/s)
  [CoreML] vision_load=828.3ms | warmup=1819.1ms | avg_vis=1169.6ms avg_tot=1265.3ms (0.790 img/s) | cos_vs_cpu mean=0.999999 min=0.999997
CoreML vs CPU: 37% slower on vision encoder due to dynamic shape recompilation overhead.
```

**Windows DirectML probe (from CI log):**
```
$ .\spikes\embeddings\rust-parity\target\release\embedding-parity.exe bench --mode ep-compare --vision-ep directml --backbone-ep cpu --budget 140 --threads 4 -n 4
=== DIRECTML EP PROBE (Windows x64) ===
=== EXECUTION PROVIDER TEST: CPU vs DirectMl (vision_ep=DirectMl, backbone_ep=Cpu, budget=140, threads=4, n=4) ===
  [CPU]   vision_load=231.5ms | avg_vis=3487.9ms avg_tot=4199.3ms (0.238 img/s)
  [DirectMl] FAILED TO LOAD / REGISTER VISION EP: Invalid configuration or input: ONNX Runtime session config error: DirectML is not supported in this build of ONNX Runtime
DirectML probe exit code: 1
```

### 5.8 Whole-Project Verification (`pnpm verify`)

```
$ pnpm verify
Photos Manager — pnpm verify verification run
Date: 2026-11-13T09:32:09.397Z
Log output file: reports/logs/verify-2026-11-13.txt

STEP: cargo fmt check
✓ PASSED: cargo fmt check

STEP: cargo clippy
✓ PASSED: cargo clippy

STEP: cargo test
running 18 tests
test filesystem::tests::test_windows_cases_note ... ok
test filesystem::tests::test_roundtrip_non_ascii_path ... ok
test filesystem::tests::test_macos_nfc_vs_nfd_normalization_probe ... ok
test filesystem::tests::test_roundtrip_non_utf8_path_unix ... ok
test metadata::tests::test_exiftool_missing_file ... ok
test database::tests::test_init_pool_and_app_settings_migration ... ok
test metadata::tests::test_exiftool_rejects_non_absolute_path ... ok
test metadata::tests::test_exiftool_success ... ok
test metadata::tests::test_exiftool_malformed_output ... ok
test metadata::tests::test_exiftool_non_ascii_filename ... ok
test metadata::tests::test_exiftool_long_path ... ok
test metadata::tests::test_exiftool_timeout ... ok
test image_pipeline::vips::tests::test_vips_supported_loaders_reports_real_codecs ... ok
test image_pipeline::vips::tests::test_vips_missing_file_error ... ok
test image_pipeline::vips::tests::test_vips_blocks_svgload ... ok
test image_pipeline::vips::tests::test_vips_thumbnail_png ... ok
test image_pipeline::vips::tests::test_vips_thumbnail_rotated_jpeg ... ok
test metadata::tests::test_child_killed_after_timeout ... ok
test result: ok. 18 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.06s
✓ PASSED: cargo test

STEP: frontend typecheck
✓ PASSED: frontend typecheck

STEP: frontend lint
✓ PASSED: frontend lint

STEP: production build
✓ built in 54ms
✓ PASSED: production build

STEP: AGENTS.md length check (< 12,000 characters)
AGENTS.md character count: 7438
✓ PASSED: AGENTS.md character count is under 12,000

ALL VERIFICATION CHECKS PASSED SUCCESSFULLY!
```

---

## 6. Decisions made

1. **Pure-Rust `tokenizers` Crate Without C/C++ Toolchain (`Decision D21`):**  
   *Alternatives considered:* Using default `tokenizers` features (`["progressbar", "onig", "esaxx_fast"]`), or calling Hugging Face tokenizers via Python sidecar.  
   *Why chosen:* Default features pull in `onig_sys` (C) and `esaxx_fast` (C++), requiring CMake and MSVC C++ toolchain configurations on Windows that are fragile in desktop distributions. Selecting `tokenizers = { version = "0.23.2", default-features = false, features = ["fancy-regex"] }` keeps the build 100% pure Rust. `tokenizer.json` for EmbeddingGemma 2 uses only a plain string split pre-tokenizer (`pattern: " "`), completely obviating the need for Oniguruma regex. Parity matched 61/61 test cases exactly.

2. **`fast_image_resize` CatmullRom SIMD Convolution Resizer & Preprocessing Rules (`Decision D22`):**  
   *Alternatives considered:* `image::imageops::FilterType::CatmullRom`, `imageops::FilterType::Triangle` (bilinear), `fast_image_resize` with Mitchell-Netravali.  
   *Why chosen:* PyTorch `torchvision.transforms.functional.resize(..., interpolation=InterpolationMode.BICUBIC, antialias=True)` implements Catmull-Rom bicubic spline filtering. `fast_image_resize` with `FilterType::CatmullRom` and `ResizeAlg::Convolution` provides NEON/AVX2 SIMD acceleration (`~1.1–1.7 ms/img`), matching torchvision output within `0.000150` mean absolute diff on valid patches, while `FastMitchell` suffered a 10x higher diff (`0.001788`).

3. **`ort = "=2.0.0-rc.15"` with `load-dynamic` and Runtime Pinned Dynamic Library (`Decision D23`):**  
   *Alternatives considered:* Default features with `download-binaries`, static linking from pyke CDN.  
   *Why chosen:* Downloading prebuilt static libraries from an unpinned third-party CDN at compile time violates clean-machine build hygiene and security policies. Using `load-dynamic` allows bundling official Microsoft ONNX Runtime `1.31.1` dynamic libraries (`libonnxruntime.1.31.1.dylib` and `onnxruntime.dll`) and loading them explicitly with checksum validation via `ort::init_from`, keeping compilation offline and reproducible.

4. **CPU Execution Provider Default for Both macOS and Windows (`Decision D24`):**  
   *Alternatives considered:* CoreML EP on macOS, DirectML EP on Windows.  
   *Why chosen:* Benchmarking CoreML on the vision encoder on Apple M4 showed a 37% regression (`1265.3 ms` vs `920.8 ms`) due to dynamic image aspect shapes forcing repeated graph recompilation and CPU fallbacks. On Windows, upstream official `onnxruntime-win-x64-1.31.1.zip` is CPU-only, and DirectML registration cleanly fails closed (`DirectML is not supported in this build of ONNX Runtime`). CPU EP provides robust, reproducible, and portable performance across both operating systems.

5. **Process-Global Shared ORT Environment Caching (`Decision D25`):**  
   *Alternatives considered:* Creating an `Environment` on every session or wrapping sessions in a long-lived struct without global caching.  
   *Why chosen:* ONNX Runtime permits only one `OrtEnv` per process. Calling `ort::init_from` multiple times during thread sweeps, benchmarks, or session re-creations resulted in `ONNX Runtime error: only one environment is allowed per process`. Storing the live `Environment` inside a process-global `Mutex<Option<Environment>>` (`shared_ort_environment`) allows subsequent sessions to safely clone the active environment handle.

---

## 7. Deviations from the task, spec or amendments

1. **Temporary `push` trigger on `task/0B2` in `.github/workflows/spike-embeddings.yml`:** The task specified `.github/workflows/spike-embeddings.yml` triggered strictly `on: workflow_dispatch:`. However, GitHub Actions only recognizes workflows for manual `workflow_dispatch` once the workflow file is merged into the repository default branch (`main`). Because this work was developed on branch `task/0B2` before merging to `main`, a temporary `push:` trigger scoped to `branches: [ task/0B2 ]` (commit `371e129`) was added to enable GitHub Actions to run the matrix jobs. After Run 2 (`43360608967`) succeeded green, the temporary `push:` trigger was removed in commit `47bf4ad`, restoring the workflow strictly to `on: workflow_dispatch:`.
2. **Workflow job timeout raised to `timeout-minutes: 90`:** The initial workflow run on `macos-15` timed out at GitHub Actions' default 45/60m limit due to cloud runner CPU throttling during the full matrix benchmark sweep. In commit `22a1197`, `timeout-minutes: 90` was set for both runner jobs so they could complete the extensive benchmark matrix (1h 16m on Windows, 57m on macOS).
3. **PowerShell `$global:LASTEXITCODE = 0` handling:** In the Windows CI runner, PowerShell 7 preserved native process exit code 1 after running negative test commands (`fetch_models.py --test-bad-checksum` and `compare --wrong-mode imagenet-norm`), causing the step runner to evaluate `$LASTEXITCODE = 1` and fail the step despite the conditional check confirming the failure. An explicit `$global:LASTEXITCODE = 0` reset was added after verifying the failure exit code.
4. **Resizer comparison scope in standalone crate:** `fast_image_resize` (`FilterType::CatmullRom`), `image` (`FilterType::CatmullRom`), and `FastMitchell` were benchmarked and compared on max/mean tensor differences rather than testing a broader array of unused filters, confirming that Catmull-Rom is the exact bicubic convolution formula of PyTorch torchvision.
5. **Stand-in preview generation:** Previews in this spike used the pure-Rust `image` crate (JPEG q80, max edge 1024) to keep the standalone parity crate free of `libvips` C bindings; Phase 4B will re-verify previews using real `libvips` outputs (`<= 2048 px`, spec §7.1).

---

## 8. Known gaps and risks

1. **Owner's Physical Laptop Validation Pending:**  
   Measurements so far were performed on Apple M4 (macOS 27.0), macOS-15 runner (Apple silicon), and Azure Standard_D4as_v5 (Windows Server 2025). The owner's physical Windows 11 laptop (Acer Swift SF314-43, Ryzen 7 5700U, 8 GB RAM) has not yet been run. Detailed step-by-step instructions have been documented in [`docs/embedding-validation.md`](../docs/embedding-validation.md) for the owner to execute locally.
2. **Live Memory Reduction Post-Drop Unmeasured:**  
   Clean-process probes measured process peak watermarks (`PeakWorkingSet64` / `maximum resident set size`). Because heap allocators typically retain freed memory in process pools, live memory returned after `drop(vision_session)` is unmeasured here. Phase 4B must measure live resident memory post-drop or evaluate isolating vision indexing to a worker process that exits.
3. **`libvips` Preview vs Rust `image` Crate Stand-in:**  
   The stand-in preview test in this spike used the Rust `image` crate with JPEG quality 80 and max edge 1024 (`preview_b140 256d mean = 0.992793`). In Phase 4B, previews will be generated by `libvips` with max edge `<= 2048 px` (spec §7.1). Preview cosine fidelity will need to be re-verified against `libvips` outputs.
4. **Windows DirectML on Real Hardware:**  
   GPU acceleration on Windows via DirectML remains unproven because official Microsoft 1.31.1 binaries are CPU-only. If GPU acceleration is ever needed in later phases, custom builds or newer upstream NuGet packages will be required.
5. **Real User Photos & Retrieval Quality Calibration:**  
   Synthetic fixtures established mathematical parity, but real-world retrieval calibration (MRR / Top-k on diverse personal libraries per spec §16.3) remains to be performed during Phase 4B.

---

## 9. Questions for the human / orchestrator

1. **Vision Token Budget Selection:**  
   We recommend keeping **`budget = 140`** as the default (`~861 ms/img` on Apple M4, `~3.45 s/img` on 4-vCPU Windows). If the owner's laptop run shows thermal or memory throttling, is budget `70` (`1.68x` speedup on Windows, `2.14x` on Mac, `cos(b70, b140) = 0.9878`) acceptable as a low-power mode?
2. **Packaging ONNX Runtime Dynamic Libraries:**  
   For Phase 4B Tauri packaging, please confirm bundling `libonnxruntime.1.31.1.dylib` in macOS `Contents/Frameworks/` (signed and notarized for Hardened Runtime) and `onnxruntime.dll` adjacent to `photos-manager.exe` on Windows.

---

## 10. Open items & next steps

The next task is determined by the orchestrator. Foundations completed across the foundation spikes include:
- `TASK-0A`: Project setup, CI, and verification harness
- `TASK-0B`: EmbeddingGemma 2 runtime spike & Python reference harness
- `TASK-0B2`: Rust tokenisation and image preprocessing parity
- `TASK-0D`: HEIC investigation spike
- `TASK-0E`: Windows libvips HEIC packaging spike
- `TASK-0F`: macOS libvips HEIC packaging spike

`TASK-0B2` remains open for the owner's validation run on the physical Windows 11 laptop (Acer Swift SF314-43, Ryzen 7 5700U, 8 GB RAM) per [`docs/embedding-validation.md`](../docs/embedding-validation.md).

---

## Concepts used (for the owner, a React developer learning Rust and Git)

1. **Process-Global `Mutex` & Lazy Initialization (`spikes/embeddings/rust-parity/src/provider.rs:20`, Decision D25):**  
   In Rust, global state is strictly controlled to prevent data races. We wrap an `Option<Environment>` in a `std::sync::Mutex` so that ONNX Runtime's requirement of having exactly one runtime environment per process is safely shared across multiple sessions without race conditions.

2. **Zero-Copy Chunk Slicing via `as_chunks` (`spikes/embeddings/rust-parity/src/parity.rs:125`):**  
   Instead of allocating new arrays or doing manual index arithmetic to divide flat image vectors into 768-float patch slices, Rust's `as_chunks::<768>()` safely casts the slice reference at compile-time with zero runtime allocations.

3. **Dynamic Library Loading (`load-dynamic` feature) (`spikes/embeddings/rust-parity/src/provider.rs:28`, Decision D23):**  
   Rather than linking to `onnxruntime.dll` / `libonnxruntime.dylib` at compile-time (which requires development headers and libraries to be present on the building machine), `ort::init_from` dynamically discovers and loads the library from disk at runtime, analogous to `dlopen()` on Unix or `LoadLibrary()` on Windows.

4. **GitHub Actions Workflow Dispatch & Artifact Retention (`.github/workflows/spike-embeddings.yml:4, 260`):**  
   To prevent long-running AI benchmarks from consuming cloud runner quotas on every push, the workflow is configured with `on: workflow_dispatch:` (run on-demand), and logs are preserved using `actions/upload-artifact` so cross-platform results can be reviewed and proven after jobs complete.

5. **Fail-Closed Negative Gate Verification (`spikes/embeddings/rust-parity/src/main.rs:112`):**  
   In testing safety-critical systems, positive tests (proving correct inputs succeed) are insufficient; negative tests (deliberately applying wrong transformations like ImageNet normalization or incorrect patch coordinates) must actively prove that the test suite detects the error and exits non-zero.

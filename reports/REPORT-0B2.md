# REPORT-0B2: Rust Tokenisation and Image Preprocessing Parity for EmbeddingGemma 2 (ONNX Q8)

**Branch:** `task/0B2` · **Commit:** *(pending)* · **Date:** 2026-11-12
**OS actually tested:** *(pending)* · **Not tested:** *(pending)*

## 1. Summary & Part A: Pre-Code Analysis (Written Before Coding)

*(Summary to be completed after Parts B–F finish.)*

### 1.1 Tokenizer File, Special Tokens, Padding, Truncation & Max-Length Behaviour

- **File:** `tokenizer.json` from `google/embeddinggemma-2` (commit `f9972ac22e696a105c482202646979e33e4b5a1e`, mirrored in `onnx-community/embeddinggemma-2-ONNX` commit `daa72c51243991dfcaf9f9137d2c573d8f7790c0`).
- **Size:** `32,170,510` bytes.
- **SHA-256:** `4d777ef5bdc1aa36227abdfb77c3e49e7b9c892d16e1b6bda41c393504828be4` (confirmed locally on `spikes/embeddings/models/onnx/tokenizer.json`).
- **Model & Pipeline (`tokenizer.json`, confirmed from source JSON):**
  - `model.type`: `"BPE"` (262,144 vocabulary entries, 514,906 merges; `config.json` `text_config.vocab_size = 262144`).
  - `normalizer`: `{"type": "Replace", "pattern": {"String": " "}, "content": "▁"}`.
  - `pre_tokenizer`: `{"type": "Split", "pattern": {"String": " "}, "behavior": "MergedWithPrevious", "invert": false}` (plain string split, no regex).
  - `post_processor`: `TemplateProcessing` (`post_processor.single`): prepends `<bos>` (`2`) and appends `<eos>` (`1`) to every single sequence (`[<bos>, $A, <eos>]`).
  - `decoder`: `Sequence([Replace("▁" -> " "), ByteFallback, Fuse])`.
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
| `ort` | `=2.0.0-rc.15` (exact pin) | `MIT OR Apache-2.0` | Targets ONNX Runtime 1.31 (`api-31`). Default features pull `download-binaries` (fetches static `.a` / `.lib` from `cdn.pyke.io` at build time). Using `default-features = false` + `load-dynamic` enables loading a checksum-verified `libonnxruntime.dylib` / `onnxruntime.dll` at runtime (`ort::init_from`). |
| `tokenizers` | `0.23.2` (latest stable; `1.0.0-rc.4` also on crates.io) | `Apache-2.0` | **Crucial finding:** `0.23.2` default features `[progressbar, onig, esaxx_fast]` pull C/C++ libs (`onig_sys`, `esaxx-rs/cpp`) that complicate Windows MSVC builds. Choosing `tokenizers = { version = "0.23.2", default-features = false, features = ["fancy-regex"] }` keeps pure Rust (`tokenizer.json` uses plain string Split, not Oniguruma regex). |
| `image` | `0.25.10` | `MIT OR Apache-2.0` | Built-in `FilterType::CatmullRom` (bicubic) for resize benchmark comparison vs `fast_image_resize`, plus PNG/JPEG decode for fixture and preview loading. |
| `fast_image_resize` | `6.1.0` | `MIT OR Apache-2.0` | SIMD-accelerated resizer (`FilterType::CatmullRom`, `ResizeAlg::Convolution`); will be measured against `image` on `pixel_values` MAE/max diff vs `tvF.resize(..., BICUBIC, antialias=True)`. |

---

## 2. Acceptance criteria
*(To be filled after completion.)*

## 3. Files changed
*(To be filled after completion.)*

## 4. Dependencies
*(To be filled after completion.)*

## 5. Commands run
*(To be filled after completion.)*

## 6. Decisions made
*(To be filled after completion.)*

## 7. Deviations from the task, spec or amendments
*(To be filled after completion.)*

## 8. Known gaps and risks
*(To be filled after completion.)*

## 9. Questions for the human / orchestrator
*(To be filled after completion.)*

## 10. Proposed next task
*(To be filled after completion.)*

## Concepts used (for the owner, a React developer learning Rust and Git)
*(To be filled after completion.)*

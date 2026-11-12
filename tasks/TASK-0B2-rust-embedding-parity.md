# TASK-0B2: Rust tokenisation and image preprocessing parity for EmbeddingGemma 2 (ONNX Q8), on both OSes

**Phase:** 0 (does **not** gate Phase 1; must be accepted before Phase 4B starts) · **Branch:** `task/0B2` (branch from `main` after 0F is merged) · **Depends on:** TASK-0B accepted (runtime decision R2), TASK-0C/0E/0F accepted (CI on both OSes)
**Orchestrator note:** Decisions D3, D5, D6, D13, D17, D19 (see `docs/STATUS.md`). TASK-0B chose ONNX Runtime through the Rust `ort` crate, Q8 weights, stored dimension 256, vision token budget 140. Every cosine number in 0B came from **Python** `onnxruntime`; the Rust side only loaded the graphs and ran one dummy text input (`spikes/embeddings/ort-probe`). Nothing yet proves that Rust can **tokenise text and preprocess images exactly like the reference**. A silent mismatch there degrades retrieval without any error (the 0B wrong-normalisation experiment dropped image cosine to 0.75 and top-1 retrieval to 45 %). This task closes that gap, measures speed properly in Rust on both machines, and proves ONNX Runtime itself loads in a clean environment. It is a spike: nothing here goes into `src-tauri/`. Phase 4B writes the product module from what this task proves.
You do not touch secrets, tokens, repositories, releases or settings (AGENTS.md, D17). This task needs none.

## Read first
- `AGENTS.md` (embeddings rules: prefixes on text only, truncate then L2-normalise, no float16, never mix models, dims or budgets)
- `IMPLEMENTATION_SPEC.md` §3.4, §5.2 (`asset_embeddings`), §7.1, §9.1 (all), §15
- `reports/REPORT-0B.md` and `docs/embedding-spike.md` (provenance table with full SHA-256, the real ONNX input names, the preprocessing notes, benchmark numbers), `reports/REVIEW-0A-0B.md` rounds 2 and 3 (what the reviewer found: budgets differ, fixture saturation, memory caveat)
- `spikes/embeddings/` (`run_reference.py`, `eval_onnx.py`, `compare.py`, `ort-probe/`, `fixtures/manifest.json`, `output/benchmark_summary.json`)
- `docs/native-dependencies.md` §3.4-style release gate rules and the 0E/0F pattern for pinned downloads (`deps/`), only as a model for how we record a native dependency

## Facts established by TASK-0B (verify against the repo, do not assume)
1. Artifact: `onnx-community` EmbeddingGemma 2 ONNX export, Q8 text/backbone graph (`model_quantized.onnx`) and Q8 vision encoder (`vision_encoder_quantized.onnx`), about 485 MiB together; pinned commit and full SHA-256 values are in `docs/embedding-spike.md`.
2. Text model inputs seen by the Rust probe: `input_ids` and `attention_mask` (int64, `[-1,-1]`), plus `image_features`, `video_features`, `audio_features` (float32 `[-1,512]`); outputs `last_hidden_state` and `sentence_embedding` (`[-1,768]`). Vision encoder: `pixel_values` (float32 `[-1,-1,768]`), `pixel_position_ids` (int64 `[-1,-1,2]`), output `image_features` (`[-1,512]`).
3. CoreML EP crashed on the zero-length `video_features`/`audio_features` inputs of the backbone; CPU EP works. Not yet tried: CoreML on the vision encoder alone, DirectML on Windows, thread sweeps.
4. Measured in **Python** on an Apple M4 (CPU): text 72 ms per query, image 966 ms (budget 140) and 2,482 ms (budget 280), vision encoder load 55 ms (+115 MB RSS). Image vectors at budget 140 are **different embeddings** from budget 280 (mean cosine 0.9872, min 0.9308 between them): budgets must never be mixed in one index.
5. The Python reference uses the documented text prefixes (`task: search result | query: <text>`; documents `title: <title or none> | text: <text>`) and **no** prefix for images. Reference vectors are produced at fp32 (never float16) and are git-ignored (`*.npz`).

## Goal
A standalone Rust crate under `spikes/embeddings/` that, from the pinned Q8 ONNX files and the pinned tokenizer, produces text and image embeddings for the committed fixture set; a stage-by-stage comparison with the Python reference (token ids, preprocessed tensors, final vectors) with the numbers below met on **macOS arm64 and Windows x64**; honest Rust speed and memory numbers (thread sweep, vision/backbone time split, execution-provider tests); proof that ONNX Runtime loads from a pinned, checksum-recorded library with Homebrew and other system copies hidden; and one decision document that Phase 4B can build from.

## In scope

### Part A: read before coding (report section 1, written first)
1. From the pinned model repository and the reference implementation (quote file and line, or the exact config key), write down: the tokenizer file (name, size, SHA-256) and its padding, truncation, special-token and maximum-length behaviour; how the ONNX text graph is called for a text query and for a document; **how an image reaches the backbone** (does the image path need `input_ids` containing image placeholder tokens plus the `image_features` tensor, or is `sentence_embedding` taken from the vision path alone? find out from the reference and from the graph, not from guessing); the exact image preprocessing (resize policy and target size as a function of the token budget, resampling filter, rescale factor, mean and standard deviation, patch size, patch ordering, padding and `pixel_position_ids` layout, how the budget 70/140/280 maps to patch count); the pooling and normalisation applied after the graph.
2. State which of these you could confirm from source and which only from observing the Python reference outputs (say so; both are acceptable, a guess is not).
3. Check the candidate crates before use (`cargo info`, registry pages): `ort` stays pinned exactly at `=2.0.0-rc.15` unless you can show a reason; for tokenisation use the Hugging Face `tokenizers` crate (verify version, licence, and that its default features do not pull a C library that complicates a Windows build; choose the feature set deliberately); for resizing compare the Rust options against the filter the reference uses (for example the `image` crate against `fast_image_resize`) and choose by measurement, not by name. Record exact versions and licences.

### Part B: reference data for the comparison
1. Regenerate, with one documented command each, and store small, committed, synthetic-only artefacts under `spikes/embeddings/parity/` (total under 5 MB; the fixture images are synthetic, never real photos):
   - token ids for a text set: the 20 fixture queries, plus edge cases: empty string, a very long string past the model's maximum length, accented Latin, Devanagari (for example Hindi), CJK, an emoji, leading and trailing whitespace, and the prefix strings themselves. Both the query form and the document form;
   - for a subset of images (at least 20, spanning landscape, portrait, square, tiny, very large aspect ratios): the preprocessed `pixel_values` and `pixel_position_ids` tensors from the Python reference at budgets 140 and 280;
   - final 768-d vectors at budgets 140 and 280 from **Python `onnxruntime` with the same Q8 files** ("same-file reference") and from the fp32 PyTorch reference ("fp32 reference"), as little-endian f32 files plus a JSON index. Not `*.npz` (git-ignored, unreadable from Rust).
2. Keep model weights, the virtualenv and any downloaded file out of git (`git ls-files | rg -i '\.(onnx|safetensors|bin|gguf)$'` must print nothing; weights are fetched by a script that checks SHA-256).

### Part C: the Rust crate
1. `spikes/embeddings/rust-parity/` (its own `Cargo.toml`, `Cargo.lock` committed, not part of any workspace, nothing in `src-tauri/`). A small library plus a command-line binary with subcommands for: `tokens` (print ids), `preprocess` (write the tensor), `embed-text`, `embed-image`, `compare` (runs the whole fixture set against the committed reference data and prints the table below), `bench` (Part D).
2. Hold to the app's future shape: model sessions behind a small struct that can later implement the `EmbeddingProvider` trait of spec §9.1; text-only mode loads only the backbone; the vision encoder loads on demand and can be dropped; dimension truncation then L2-normalisation in one tested function; budgets are an explicit parameter and part of the vector's `model_revision` string (propose the string format in the decision document). Execution provider: CPU unless Part D shows a better one that is also stable.
3. Simple, explicit Rust the owner can read (AGENTS.md). No `unwrap()` outside tests. Unit tests (they run under `cargo test` in the crate): truncation and normalisation, cosine of identical vectors, tokenisation of a few fixed strings against hard-coded ids, preprocessing of a tiny synthetic image against a hard-coded tensor.
4. **Gates** (numbers chosen by the orchestrator because the weights are identical, so only preprocessing and numerics can differ; if a gate cannot be met, do not weaken it, stop and report with the diff analysis):
   - **Tokens:** exact integer equality with the reference for every text in the set (query and document forms). Any difference is a failure.
   - **Text vectors, 768 d and 256 d:** Rust vs same-file reference: mean cosine at least 0.9999 and minimum at least 0.999.
   - **Image vectors, budgets 140 and 280, 768 d and 256 d:** Rust vs same-file reference: mean cosine at least 0.999 and minimum at least 0.995, on all fixture images that have a reference vector.
   - Also print Rust vs fp32 reference (not a gate: Python's own Q8-vs-fp32 numbers from 0B are the ceiling; the Rust numbers must not be worse than those by more than 0.001 on the mean).
   - Per-image statistics of `pixel_values`: maximum and mean absolute difference vs the reference tensor, and the identity of the worst image. If the tensors differ, find out why (usually the resampling filter or rounding) and say which stage is responsible; do not tune until the cosine passes without understanding it.
   - A **deliberately wrong** preprocessing (for example wrong normalisation, wrong resampling filter, wrong patch order) run through the same `compare` command, with its numbers, to show the gate can fail (the 0F lesson: prove the guard says no).
5. **App-shaped input test.** The app will embed from the orientation-corrected **preview derivative** (a lossy file of up to about 2,048 px, spec §7.1), not from the original. For the image subset, create stand-in previews (for example JPEG quality 80, long edge 1024, using the Rust `image` crate or Pillow; say which) and report Rust vs reference cosines when the Rust side starts from the preview and the reference starts from the original. This is an estimate of the Phase 4B situation (the real previews come from libvips); the report states that gap.

### Part D: speed, memory, execution providers (Rust, release build, both machines)
1. **Thread sweep** (intra-op threads 1, 2, 4, 6, 8, 10 on the Mac; 1, 2, 4, 8 on the laptop), image batch sizes 1, 2, 4, per budget 70, 140 and 280. Report: model load time, per-image latency split into vision encoder and backbone, per-text latency, throughput, and peak resident memory **from a single clean process** (nothing else loaded; say how you measured it: `/usr/bin/time -l` on macOS, peak working set on Windows).
2. **Execution providers:** CoreML on the **vision encoder alone** (the 0B crash was in the backbone, on zero-length modality inputs) with CPU for the backbone, on the Mac; DirectML on Windows (check the `ort` feature, the runtime build it needs and its packaging cost). Report speed and cosine against the CPU result. A provider that is faster but changes vectors beyond the gates, or crashes, is not recommended.
3. **ETA table** for the decision document: seconds per image and per text on each machine at the recommended settings, and the extrapolated time for 10,000 and 100,000 photos on one stream. (0B: about 1 s per image on the Mac at budget 140, about 27 h for 100,000; the Windows laptop is not yet measured.)
4. Budget 70: speed and cosine against budget 140 only (the 20-scene fixture cannot judge retrieval quality; do not recommend 70 from this data, only report it).

### Part E: ONNX Runtime provenance and the clean-environment gate (spec §3.4)
1. Find out exactly which ONNX Runtime version `ort =2.0.0-rc.15` needs and how it obtains the library (build-time download of prebuilt binaries, or `load-dynamic` with a library you supply). Record: version, source URL, SHA-256, licence, file names per OS, size. Prefer the approach that lets us **pin and checksum the library ourselves** and bundle it inside the app (reviewer's note from 0B: a hardened-runtime macOS app cannot load an unsigned library fetched after install; it must be in the bundle). If `ort` can only fetch from its own CDN at build time, say so and propose the pin (a checksum check in CI, or our own copy in the private deps repo, owner's decision).
2. **macOS:** run the release binary with `env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin` and only the pinned library (Homebrew's `onnxruntime` not on any search path), `otool -L` of the binary and the library pasted.
3. **Windows:** a manually started workflow `.github/workflows/spike-embeddings.yml` (`workflow_dispatch` only, `timeout-minutes` set, `permissions: contents: read`, no secrets) that on `windows-latest` and `macos-15` builds the crate, downloads the two Q8 files and the tokenizer from the pinned commit with SHA-256 checks (fail closed; the weights are public, no token needed), runs `compare` and prints the table, and uploads **only** the binary and the printed tables as an artifact (`retention-days: 3`; never upload weights or vectors derived from private images: the repository is public, D13). Prove the failure path (wrong checksum makes the job red). Limit: if it is not green after **two** runs, stop and report with logs and options.
4. **Owner's Windows laptop (Acer Swift, Ryzen 7 5700U, 8 GB RAM):** write `docs/embedding-validation.md` (short, copy the style of `docs/windows-validation.md`): download the artifact, run one script that fetches the weights with checksum, run `compare` and `bench` on a copy of nothing but the committed synthetic fixtures, paste the output. The report lists exactly what only the laptop can show (low-end speed, memory with 8 GB).
5. Add a short ONNX Runtime section to `docs/native-dependencies.md` (version, source, SHA-256, licence, bundling path, OS tested, what is still unproven).

### Part F: the decision document
`docs/embedding-runtime.md` (this is the file spec §9.1 names). Contents: final runtime configuration (ORT version and library source, execution provider, thread default, budget 140, stored dimension 256, Q8 files with commit and SHA-256, tokenizer file with SHA-256), the exact text prefixes, the exact image preprocessing recipe in prose and in the code location, the proposed `model_revision` format (weights + quantisation + budget + preprocessing version), memory and ETA numbers per machine, the licence checklist (Apache 2.0 text and NOTICE content, quoted by reference only), what stays unknown (video and audio untested; real libvips previews; real photos; budget 70 quality; DirectML on real hardware if not measured), and a list of what Phase 4B must implement. Mark every number with the machine it came from.

## Out of scope (do not do)
- Anything in `src-tauri/` or `src/`, Tauri commands, SQLite tables, jobs, UI, a vector index.
- Retrieval-quality calibration, product thresholds, a harder labelled query set (Phase 4B, spec §16.3).
- Video or audio embeddings, float16 as a recommendation, llama.cpp or any other runtime.
- Committing weights, the virtualenv, `*.npz`, or anything derived from real photos.
- Secrets, tokens, repositories, releases, settings (D17); editing `docs/STATUS.md`, `docs/NEXT_STEPS.md`, the spec or `AGENTS.md`.

## Allowed paths
`spikes/embeddings/`, `docs/embedding-spike.md` (append a dated "after 0B2" note only), `docs/embedding-runtime.md`, `docs/embedding-validation.md`, `docs/native-dependencies.md` (ONNX Runtime section only), `.github/workflows/spike-embeddings.yml` (new file), `.gitignore` (additions only), `reports/`.

## Acceptance criteria
- [ ] Report section 1 written before any code, with file and line (or config key) for every preprocessing and tokenisation rule; unconfirmed items marked.
- [ ] Token ids equal for every text in the set; table pasted.
- [ ] The vector gates (text, image at 140 and 280, 768 d and 256 d) met on **macOS arm64 (this machine)** and **Windows x64 (workflow run)**; raw `compare` output from both pasted; the deliberately wrong preprocessing run shows the gate failing.
- [ ] `pixel_values` difference statistics pasted, with the stage responsible for any difference explained.
- [ ] App-shaped input test pasted, with the gap to the real libvips previews stated.
- [ ] Thread sweep, vision/backbone split, clean-process memory, CoreML-on-vision-encoder and DirectML results pasted with the machine named; ETA table written.
- [ ] ONNX Runtime provenance recorded; macOS `env -i` run and `otool -L` pasted; Windows workflow green with the checksum failure path shown.
- [ ] `docs/embedding-runtime.md` and `docs/embedding-validation.md` written; `native-dependencies.md` section added.
- [ ] `cargo test` in the crate green; `pnpm verify` still green; no weights, venv or private data committed (commands below).
- [ ] The report lists what is validated on one OS only and what the owner's laptop run must still show.

## Verification commands
```
cargo test --manifest-path spikes/embeddings/rust-parity/Cargo.toml
cargo run --release --manifest-path spikes/embeddings/rust-parity/Cargo.toml -- compare
git status --short
git ls-files | rg -i '\.(gguf|onnx|safetensors|bin|npz)$'      # expect no matches
git ls-files spikes/embeddings/parity | xargs du -ch | tail -n 1
gh workflow run spike-embeddings.yml
gh run list --workflow spike-embeddings.yml --limit 3
pnpm verify
```
Note for the file-count commands: `xargs` can split a long list in batches, and `tail -n 1` then shows only the last batch; use `du -sh spikes/embeddings/parity` for the total.

## Stop conditions
Stop and report (with the options table: effort, risk, licence) if:
- the image path cannot be reproduced without code that is not available to us, or only by copying the Python processor behaviour you cannot verify;
- the Rust tokens differ from the reference on any text and you cannot explain the cause;
- the image gate is missed after the preprocessing tensors are shown to match within the resampler's expected tolerance, or the tensors cannot be made to match;
- `ort` cannot be pinned (the library can only come from a source we cannot checksum), or the library will not load in the clean environment;
- the Windows workflow is not green after two runs;
- a gate would have to be weakened.

## Deliverable
`reports/REPORT-0B2.md` (use `reports/_REPORT_TEMPLATE.md`; raw outputs, not summaries; an honest run log from `gh run list --json`), `docs/embedding-runtime.md`, `docs/embedding-validation.md`. Then stop. Do not start Phase 4B.

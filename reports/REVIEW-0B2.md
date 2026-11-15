# REVIEW-0B2 — Rust tokenisation and image preprocessing parity (EmbeddingGemma 2, ONNX Q8)

Reviewer: orchestrator (Claude) · Date: 2026-11-14 · Branch `task/0B2`, HEAD `f318d96` (commits `c8903c3`, `371e129`, `22a1197`, `47bf4ad`, `f318d96`) · Report: `reports/REPORT-0B2.md` (stated commit `47bf4ad`)

## Verdict: FIX (small, report and docs only; no code change asked) — then ACCEPT

The technical work is good and the gates are met. What fails the review is the report's honesty on four points (below) and two claims in `docs/embedding-runtime.md` that the measurements do not support. None of these needs a rerun of the spike. 0B2 does **not** gate Phase 1; it must be accepted and merged before Phase 4B starts. The owner's laptop run is part of 0B2 and is still open (see "Open").

## What I checked, and how

- Read-only clone of the public repo on `task/0B2` at `f318d96`. Read in full: `.github/workflows/spike-embeddings.yml` (final form), `spikes/embeddings/rust-parity/Cargo.toml`, `src/preprocess.rs` (first part: budgets, aspect-ratio rule, constants), `fetch_models.py` pins, `docs/embedding-runtime.md`, `docs/embedding-validation.md`, the `docs/native-dependencies.md` and `docs/embedding-spike.md` diffs, `.gitignore` diff. Greps over the crate for `unsafe`, `unwrap()` and `expect(`.
- Not read line by line: `provider.rs`, `parity.rs`, `bench.rs`, `main.rs`, `tokenizer.rs`, `generate_parity_data.py`. I rely on the gate output for those (see "Limits of this review").
- I could not query the GitHub run API from my session (HTTP 403, repo not enabled for me). Run facts below come from the agent's report, the owner's screenshot of the Actions list and the owner's pasted `gh run list`; I could not open job logs.
- The agent's pasted compare output, memory probes and EP probes are read as **agent evidence**, not as owner-run evidence. The Windows numbers come from a CI log the agent quotes.

## Gates (task file) against evidence

| Gate | Result | Basis |
|---|---|---|
| Token ids: exact equality | Met: 61/61 | agent's `compare` output (Mac); Windows CI file `parity-windows-x64.txt` reported identical |
| Image tensors | Met: 40/40 position ids and prompt ids exact; pixel values max abs diff 0.102 (tiny 16x16 case), mean 0.00015 | agent output |
| Text cosine vs same-file Python Q8 | Met: mean 1.000000, min 1.000000 (768 d and 256 d) | agent output |
| Image cosine vs same-file Python Q8, budget 140 | Met: mean 0.999936, min 0.997064 | agent output (task gate: mean ≥ 0.999, min ≥ 0.995) |
| Image cosine, budget 280 | Met: mean 0.999945, min 0.996856 | agent output |
| Negative guard: wrong preprocessing must fail | Met: ImageNet normalisation gives image cosine mean 0.76, `[FAIL]`, exit 1 | agent output; text rows still PASS (as they should) |
| ONNX Runtime provenance, clean load | Met on paper: ORT 1.31.1 pinned by size and SHA-256 for both OSes; `otool -L` and `dumpbin /dependents` show no link to the ORT library; Mac run under `env -i` | docs and agent output; workflow read |
| Both OSes through a manual workflow | Met in final form: `workflow_dispatch` only, `contents: read`, uploads the exe and one text file, 3-day retention, no weights uploaded | workflow read |
| Scope | Clean: files changed are all inside the allowed paths (`spikes/embeddings/`, `docs/embedding-*`, `docs/native-dependencies.md`, the workflow, `.gitignore`, `reports/`); no `src-tauri/` change; no weights committed (`*.onnx` ignored); parity fixtures 4.7 MB | diffstat and `git ls-files` |
| Owner's laptop | **Open** (not run) | see below |

The key numbers are credible: the Mac text-only result of exactly 1.000000 is what a faithful tokenizer plus an identical graph must give, and the ~0.9999 image mean sits at the level of Q8 non-determinism between two runtimes (Python's own Q8-vs-fp32 is 0.9982). The negative test is the part I trust most: it shows the gate can fail.

## Findings

### Must fix (report and docs; the agent does these)

1. **"Deviations: None" is false.** The workflow could only be started from the default branch, so the agent added a temporary `push` trigger to start it from `task/0B2`, and removed it later. Whatever else departed from the task text belongs in §7 too. The task asked for deviations to be stated, not omitted.
2. **Run history is not honest.** The report says green on both OSes "in under 2 runs" and shows only run `43360608967`. The owner's `gh run list` shows earlier runs of the spike workflow, including failed runs `43286961021` and `43286960771`, with no explanation in the report. Also the run URL uses the wrong organisation (`Antigravity-Workspace/photos-manager`); the repo is `narayanwaraich/photos-manager`. The report must list every run of `spike-embeddings.yml` with its conclusion and the cause of each red one, using the owner's raw `gh run list --workflow spike-embeddings.yml --limit 20` output (not a table the agent made up) and the tail of each failed run's log. Windows job time was 1h16m, macOS 57m.
3. **Decision IDs collide.** The report numbers its decisions D18 to D22; D18 to D20 already exist in the spec and STATUS (macOS build, gate scripts, macOS differences). In my spec v2.1.1 they are **D21 to D25** (tokenizer features, preprocessing rules, ORT 1.31.1 via `load-dynamic`, CPU execution provider, one ORT environment). The report and any doc that cites a D-number must use these.
4. **Unproven memory claims.** `docs/embedding-runtime.md` (§5.1 notes and the Phase 4B checklist) says dropping the vision encoder reclaims about 370 MiB and "proves the vision session drops cleanly". The probes record the **peak** (a peak can never go down), the Windows peak is identical with and without the drop (579.70 vs 579.71 MiB), and the Mac peak RSS after the drop is higher (970 vs 938 MiB). So nothing here shows memory was returned. The text must say "unproven; Phase 4B measures current memory after the drop (or uses a worker process that exits)". The same claim sits in the spec's old rule 7; I corrected it in v2.1.1.

### Should fix (same round; short)

5. **Preview test is labelled as passing.** The report's acceptance row says the preview test "passes"; the output prints `[INFO]` and no gate applies. State the numbers (256 d, JPEG q80, longest edge 1024: mean 0.9928, min 0.9684; at 768 d mean 0.9918, min 0.9648; budget 280: min 0.953) as measurements, not as a pass. They matter: embedding the preview instead of the original costs about 0.7 to 0.9 % of cosine on average against the original-image vector, and the worst case is an extreme thin image (20x2000). Phase 4B re-measures with libvips previews (the agent already says so in "Known gaps" 2).
6. **Command names in the report do not match the committed CLI.** The report shows `mem-single-text` and `bench-coreml`; the committed crate has `bench --mode single-text | single-image | single-drop-vision | ep-compare | sweep | b70-vs-b140`, and the workflow uses those. Replace the report's commands with the ones that exist (or say these were earlier names).
7. **§10 mislabels earlier tasks** (calls 0D libvips thumbnails, 0E ExifTool, 0F file operations). Actual: 0D HEIC investigation, 0E Windows HEIC libvips build, 0F macOS HEIC libvips build. The proposed next task is not the agent's decision to make; the orchestrator writes it.
8. **`docs/embedding-validation.md` for the laptop**: add a `python --version` check, say the owner's repo must be on `task/0B2` (or `main` after merge) and plugged in with the Windows power mode on "Best performance", use `--num-images 4` as the CI did (12 images over 4 thread settings, 3 budgets and 3 batch sizes will take far longer than the CI run on a 5700U; expect 1 to 1.5 hours even with 4), and say what to do when the 3-day artifact has expired (ask the orchestrator to dispatch the workflow again). The agent's file must not claim Windows Server numbers apply to the laptop.

### Noted, no action

- **Memory is bigger than planned.** Measured clean-process peaks, Rust, 4 threads, budget 140: Mac text-only 566 MiB RSS (324 MiB private footprint), with vision 938 MiB (367 MiB footprint); Windows CI text-only 361 MiB, with vision 580 MiB. The spec's "about 200 MB" for text-only was wrong (corrected in v2.1.1). On the 8 GB laptop the model is a real cost next to Windows, WebView2 and the app: the laptop run decides how the setting is offered.
- **Windows is slow.** 3.45 s per image on a 4-vCPU server (0.29 img/s) means about 96 hours for 100,000 photos, and a text query takes about 410 ms there against 52 ms on the Mac. This is what the opt-in, resumable, pausable design in the spec is for; the laptop figure is what the UI's ETA will really show. Budget 70 is about 1.7 to 2.1 times faster (cosine to budget 140: 0.988) but retrieval quality at 70 has not been measured, so 140 stays the default.
- **CoreML slower, DirectML absent.** CoreML on the vision encoder: 1265 ms against 921 ms on CPU (agent output). DirectML: the official ORT 1.31.1 Windows zip is CPU-only, so registration fails closed; the Cargo feature `directml` is enabled but cannot work with that library. CPU EP is the decision (D24); a GPU path would need a different ORT build and its own task.
- **Dependencies:** `ort =2.0.0-rc.15` with `default-features = false` and `load-dynamic` (no CDN download at build time), `tokenizers 0.23.2` pure Rust, `fast_image_resize 6.1.0`, `image 0.25.10`; versions read from `Cargo.toml`, licences are as the report states (not independently re-checked on crates.io).
- **`unsafe` / `unwrap`:** none outside tests (`expect` only in `#[cfg(test)]`); no `unsafe` found in the crate.
- Synthetic fixtures only: parity is shown, retrieval quality on real photos is not (Phase 4B calibration, spec §16).

## Limits of this review

I did not read the four large source files line by line and I did not open any job log. Parity rests on the agent's pasted outputs, whose numbers are consistent with each other and with the reference (tokens exact, text 1.000000, a failing negative control, Mac and Windows rows identical). If the owner's laptop run reproduces the parity lines (it runs the committed binary against the same fixtures), that is independent confirmation on real hardware.

## Open (owner)

- **Failed runs:** paste the tail of the log of runs `43286961021` and `43286960771` (`gh run view <id> --repo narayanwaraich/photos-manager --log-failed | tail -80`) so item 2 can be closed with facts.
- **Laptop run of the 0B2 binary** (`docs/embedding-validation.md`): needs the Windows artifact of run `43360608967`; GitHub keeps it 3 days, so about 2026-11-16. If it has expired, the agent or I need to dispatch the workflow again.

## Answers to the agent's questions

1. **Vision budget:** keep **140** as the default. Budget 70 is not a "low-power mode" yet: it may only become a setting after a retrieval-quality test on a harder labelled set (spec §16.3), and vectors made at 70 would be a different embedding (never mixed with 140).
2. **Packaging ONNX Runtime:** yes. Bundle `libonnxruntime.1.31.1.dylib` inside the signed `.app` (`Contents/Frameworks/`) and `onnxruntime.dll` next to the Windows exe, loaded by explicit path with `ort::init_from`; the library is never downloaded with the weights (spec §9.1, D23). Phase 4B owns the packaging work.

## Fix prompt for the agent (after the owner has pasted the two failed-run logs and the raw run list)

> TASK-0B2, review round 1 (`reports/REVIEW-0B2.md`). No code changes. Edit only `reports/REPORT-0B2.md`, `docs/embedding-runtime.md` and `docs/embedding-validation.md`. (1) §7 Deviations: list every deviation, including the temporary `push` trigger used to start the workflow and its removal. (2) Replace the run log with the owner's raw `gh run list --workflow spike-embeddings.yml --limit 20` output and, for each red run, the cause with the last lines of its log; fix the repo URL to `narayanwaraich/photos-manager`; say honestly how many runs it took. (3) Renumber your decisions D18 to D22 as D21 to D25, in this order: tokenizer features (D21), preprocessing rules (D22), ORT 1.31.1 with load-dynamic (D23), CPU execution provider (D24), shared ORT environment (D25), everywhere they appear (your old D21 shared-environment and D22 CPU-EP numbers swap into D25 and D24). (4) Remove the claims that dropping the vision encoder reclaims about 370 MiB or "proves it drops cleanly" (report and `docs/embedding-runtime.md`, §5.1 and the Phase 4B checklist); write that the probes record peaks only and that current memory after the drop is unmeasured. (5) Preview test: report it as a measurement ([INFO], no gate) with the numbers. (6) Replace `mem-single-*` and `bench-coreml` with the commands that exist in the committed CLI. (7) Fix the task labels in §10 and remove the "proposed next task". (8) `docs/embedding-validation.md`: add `python --version`, branch requirement, power mode, `--num-images 4`, expected duration (1 to 1.5 hours), and the expired-artifact note. Run `pnpm verify`. Report the new commit id. Do not touch the workflow or the crate.

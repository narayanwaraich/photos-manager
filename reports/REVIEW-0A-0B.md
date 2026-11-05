# Review of TASK-0A and TASK-0B (2026-11-03)

Evidence reviewed: REPORT-0A.md, REPORT-0B.md, one `pnpm verify` log (the two logs supplied were byte-identical and both come from the 0A branch; 0B had no `pnpm verify`, which is expected because it ran in parallel on a branch without `package.json`).
Not yet seen: any source file, `docs/native-dependencies.md`, `docs/embedding-spike.md`, `benchmark_summary.json`. Versions in the reports (Tauri 2.12.5, vite 8.3.5, `ort` 2.0.0-rc.15 and others) were not independently verified by the reviewer.

## TASK-0A — Verdict: FIX (evidence only, no new scope)

Scope: ok. Safety invariants: nothing violated on the evidence given (capabilities `core:default` only, no shell, argument-array ExifTool, strict CSP). The 13 tests and the verify log are consistent with the report.

Gaps:
1. **"`pnpm tauri dev` starts the app" is not shown.** The evidence is a release build plus `vite build`. Needed: a screenshot of the running app with the thumbnail rendered through the `asset-thumb` protocol, or the dev-run output.
2. **Security-critical code not yet seen.** The protocol handler (`src-tauri/src/lib.rs`) must be checked for id-to-path handling (no path traversal, no raw paths from the URL). `src-tauri/build.rs` adds a Homebrew library search path; that is acceptable for the dev machine only and must not be what makes a release build work (spec §3.4).
3. **Mechanical checks.** Only two of the REVIEW_CHECKLIST §2 greps were run (with `grep`). Run the full set with `rg`.
4. **Repo hygiene.** `src-tauri/gen/schemas/*` (about 4,600 lines of generated JSON) is committed; these are normally ignored. `.DS_Store` shows as tracked and modified on the 0B branch.
5. **OS strings disagree.** 0A says macOS 26.0; 0B says "macOS 15+ (Darwin 27.0.0)". Run `sw_vers && uname -a`.
6. **Versions** need a registry spot-check by the owner: `cargo info tauri`, `cargo info sqlx`, `cargo info libvips`, `npm view vite version`.

Files requested: `src-tauri/src/lib.rs`, `src-tauri/build.rs`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, `src-tauri/capabilities/default.json`, `src-tauri/src/metadata/mod.rs`, `docs/native-dependencies.md`.

Good: orientation test (EXIF 6 gives upright 240x320), timeout and malformed-output tests, NFC/NFD and non-UTF-8 path tests, bounded ExifTool output, the D4 check done without adopting a dependency.

## TASK-0B — Verdict: ACCEPT the runtime decision provisionally (R2: `ort` + ONNX Q8); FIX the evidence; one follow-up task

Provisional because `docs/embedding-spike.md` and `benchmark_summary.json` have not been read. The recommendation is plausible and consistent with the model card: Q8 near-lossless, Q4 visibly worse, 256 d default, vision encoder loadable on demand (+115 MB RSS), CPU provider because CoreML failed on zero-length modality inputs.

Gaps:
1. **Rust parity is not proven.** `ort-probe` loads both graphs and runs a dummy text input. Every cosine number comes from Python `onnxruntime`. Tokenisation and image preprocessing (patching, position ids, token budget) in Rust are untested. This is the main integration risk of R2 and needs its own task before Phase 4B (proposed TASK-0B2, see below).
2. **Report gives one cosine per case; the task asked for min and mean.** FP16 at exactly 1.00000 for images is unusually perfect next to Q8 at 0.9982; show min/mean and confirm candidate and reference are different computations.
3. **llama.cpp (R1) appears to be dismissed from a GitHub issue (#30082), not from a run.** The reviewer could not confirm that issue from here. Say whether the server was actually run for text, and link the issue.
4. **Fixture data is easy.** 20 synthetic scenes; 100 % top-1 at 256, 512 and 768 d does not distinguish them, and 128 d falling to 30 % is a large cliff on so small a set. The 256 d default stands on the model card's own result (about 95 % of full quality), not on this data. Fine, but do not cite the fixture result as proof.
5. **Size discrepancy.** Report: 185 fixture files, 2.3 MB. `git ls-files | xargs du -ch`: 1.0M total. Run `git ls-files spikes/embeddings/fixtures | wc -l` and `du -sh spikes/embeddings/fixtures`.
6. Task item "deliberately wrong preprocessing to show the damage" is not mentioned.

Files requested: `docs/embedding-spike.md`, `spikes/embeddings/output/benchmark_summary.json`.

### Implication the report did not state
At 966 ms per image (budget 140, Apple M4, CPU) a 100,000-photo library is about 27 hours of embedding time on one stream. Phase 4B must therefore make embedding **opt-in per root, resumable, pausable, with an ETA**, and test budget 70 and Windows execution providers (DirectML) before committing to "embed everything". Proposed spec change (apply to §9.1 after ACCEPT).

### Proposed follow-up
TASK-0B2 (before Phase 4B, not blocking the Phase 0 gate): Rust text tokenisation and image preprocessing for the chosen ONNX files; produce vectors for the fixture set from Rust; cosine vs the Python reference at least 0.999 for Q8; Windows run of `ort-probe`.

## Answers to the agents' questions (owner decides D5/D6)
- D4 (tauri-specta 2.0-rc.27): decide at the start of Phase 1; reviewer leans to adopting it in a small isolated task with Zod as the fallback.
- Windows ExifTool: use the official Windows executable package, renamed `exiftool.exe` (not the `-k` variant that waits for a keypress), with its support folder; measure startup cost in 0C and consider `-stay_open` to batch.
- Weights: provisional yes to the pinned download-on-consent artifact from `onnx-community`, conditional on the provenance table (commit, SHA-256, licence file) and the owner's reading of the licence. The artifact size in the report (about 485 MB for Q8) differs from the model page's total (about 850 MB); confirm what is included (probably no audio encoder).
- ONNX Runtime library: bundle inside the signed app, not downloaded with the weights. macOS hardened runtime and notarisation restrict loading unsigned native libraries fetched later. Verify in 0C.


---

# Round 2 (2026-11-05): source files, docs and owner's command output

Evidence added: `lib.rs`, `build.rs`, `Cargo.toml`, `tauri.conf.json`, `capabilities/default.json`, `metadata/mod.rs`, `native-dependencies.md`, `embedding-spike.md`, `benchmark_summary.json`, owner's terminal output. Reviewer fetched two sources: Google's model list (confirms `gemini-3.8-flash` and `gemini-3.5-flash-lite` exist) and llama.cpp issue #30082 (confirms the server ignores or rejects image input for this model; closed "not planned").

## Owner-side findings
- `pnpm tauri dev` failed with `cargo: command not found`: Rust is not on the PATH of the owner's terminal (the agent's shell had it). Environment issue, not a code defect. Fix: `. "$HOME/.cargo/env"` (or install via rustup), then re-run. Until then the owner cannot run `pnpm verify` himself, which the loop requires.
- `rg` checks: no matches for SQL/shell/fs plugin, shell invocation, destructive file calls, secrets, weights, fp16, stale document names. The `unwrap/expect/panic` check did not run (zsh history expansion on `!`; use single quotes).
- `sw_vers`: macOS 27.0, Darwin 27.0.0, arm64. REPORT-0A and `native-dependencies.md` say "macOS 26.0"; REPORT-0B says "macOS 15+". Both wrong; correct to macOS 27.0.
- `npm view vite version` = 8.3.5, matches the report. Cargo versions still unverified (no cargo).
- Fixture count 186 files, 2.3 MB: the report was right. The earlier "1.0M total" came from the checklist command itself (`xargs` splits the list and `tail -1` shows only the last batch). Reviewer's error; command fixed in REVIEW_CHECKLIST.

## TASK-0A — still FIX; now with concrete defects (fix prompt for Antigravity below)
1. **ExifTool child process is not killed on timeout or oversize output** (`metadata/mod.rs`): `tokio::process::Child` does not kill on drop, so each timeout leaves a running ExifTool. The test passes (1 ns timeout) while leaking processes. Violates "bounded, time-limited sidecars" (spec §2.2, §7.3).
2. stderr is piped but never read (can block a chatty child); a relative path starting with `-` could be read as an ExifTool option; parse errors echo up to 512 KB of output into the error string (log bloat, metadata leak).
3. `Cargo.toml` release profile sets `panic = "abort"`. Spec §11 requires worker panics to be caught and recorded as job failures; abort makes that impossible. Remove it.
4. `lib.rs` protocol handler: acceptable as a spike (it ignores the URL, so no path traversal), but it sends `Access-Control-Allow-Origin: *` (unneeded), runs libvips synchronously on the handler thread, writes to a shared temp file name (race), and uses a compile-time fixture path. Must be labelled spike-only and replaced in Phase 2 by id lookup in the app cache dir through an async handler. `setup` uses `expect` for app-data and DB init, so a startup failure panics with no message.
5. `build.rs` Homebrew search path: dev-only, accepted for now, TASK-0C removes it. Document as dev-only.
6. `native-dependencies.md`: wrong macOS version; Gemini IDs `gemini-3.8-flash` and `gemini-3.5-flash-lite` are now verified, but the "legacy `gemini-1.5-flash` retained" line is unsupported (the current model page does not list it); Windows ExifTool "single standalone binary" and macOS "`/usr/bin/perl` on all stock macOS" are unverified and carried to TASK-0C.
7. Positive: capabilities are only `core:default`; CSP is tight; no shell; `-n` numeric output; bounded read; tests for the four ExifTool cases; path tests. `core:default` is the Tauri default set, to be narrowed later.
Note for Phase 1: nothing yet calls `invoke` from the frontend. Confirm IPC works under the CSP (`connect-src` for the IPC scheme) in the first Phase 1 command task.

Fix prompt (paste into the 0A Antigravity conversation, same branch):
```
Review feedback for TASK-0A. Fix only these items on branch task/0A, re-run pnpm verify, and add a "Fixes" section to reports/REPORT-0A.md. Do not add scope.
1. metadata/mod.rs: kill the ExifTool child on timeout and on output-size overflow (kill_on_drop(true) and an explicit kill+wait), send stderr to Stdio::null(), reject non-absolute input paths with a MetadataError, truncate the JSON echoed in ParseError to 200 bytes. Add a unix-only test using a tiny executable script (spawned directly, not via sh -c in product code) that sleeps; assert the child process no longer exists after the timeout.
2. Cargo.toml: remove `panic = "abort"` from [profile.release].
3. lib.rs: remove the Access-Control-Allow-Origin header; add a clear "SPIKE ONLY" comment on the protocol handler listing what Phase 2 must replace (id lookup, app cache dir via Tauri path API, async handler); replace the `expect` calls in setup with error propagation.
4. build.rs and metadata::which_exiftool: add comments marking the Homebrew paths dev-only, and state it in README.md and docs/native-dependencies.md.
5. docs/native-dependencies.md: change "macOS 26.0" to "macOS 27.0 (Darwin 27.0.0, arm64)"; delete the "legacy gemini-1.5-flash" claim; mark the Windows ExifTool single-binary statement and the macOS /usr/bin/perl statement as "unverified, to be checked in TASK-0C".
6. .gitignore: ensure `.DS_Store` and `/src-tauri/gen/schemas/` are ignored (the owner is untracking them on this branch; do not re-add).
Then run: rg -n 'unwrap\(\)|expect\(|panic!|todo!|unimplemented!' src-tauri/src -g '!*test*' and paste the output.
```
Owner still to supply after the fix round: a screenshot of the running app showing the thumbnail (after fixing PATH), and his own `pnpm verify` tail.

## TASK-0B — runtime decision ACCEPTED (R2, ONNX Runtime Q8, 256 d); docs need FIX; follow-up 0B2 required
Resolved from round 1: min/mean cosines are in `benchmark_summary.json` (fp16 is 0.9999992 mean, not exactly 1.0, plausible); the wrong-normalisation experiment exists and shows the damage (image cosine 0.75, retrieval 45 %); the Q8 download is 485 MiB (about 509 MB) for text plus vision, no audio encoder; llama.cpp image embedding is confirmed unusable by issue #30082 (closed "not planned", `/embedding` silently returns text-only vectors).

New findings:
1. **The recommended configuration is quoted with another configuration's numbers.** The summary says image cosine 0.99820; that is Q8 at budget 280. The recommended Q8 at budget 140 measures mean 0.9872, min 0.9308 against the budget-280 reference. This is expected (a different budget is a different embedding), so vectors from different budgets must never be mixed in one index. The proposed `model_revision` string already encodes the budget; keep it.
2. **"128 d fell to 30 %" is wrong.** The 30 % belongs to the wrong-normalisation run. Every real configuration scores 100 % top-1 and top-5 at every dimension, so these 20 queries cannot tell 128, 256 and 768 d apart. The 256 d default rests on the model card, not on this fixture.
3. **Near-duplicate precision of 21–23 % is almost certainly an evaluation artefact.** Ground truth counts only 160 base-to-variant pairs, but each base has 8 variants, so same-group pairs total 20 x C(9,2) = 720. At threshold 0.95 the model predicts 652 pairs, close to 720, so most "false positives" are correct variant-to-variant pairs. Recompute with group-level labels. Distractors still score 0.91–0.935, close to the true-duplicate range, which supports the spec: embeddings are advisory and pHash decides duplicates.
4. Speed: Q8 ONNX at budget 280 (2.48 s per image) is slower than the PyTorch fp32 reference (1.27 s), and 4 of 10 cores were used. At budget 140, about 1 s per image is roughly 27 hours for 100,000 photos. Memory figures come from a Python process that had other models loaded (only the first variant is a clean reading); treat 1.3 GB as an upper bound. Measure in Rust (0B2): thread sweep, time split between vision encoder and backbone, CoreML on the vision encoder alone (the failure was on zero-length modality inputs in the backbone), DirectML on Windows.
5. Provenance table shows 8-character hash prefixes. The pinned download (D6) needs full SHA-256 values, the tokenizer file, and the repository's LICENSE file at the pinned commit. The licence link in the doc points to a page named `gemma_4_license`; cite the repo's own LICENSE.
6. `ort` is `2.0.0-rc.15`, a release candidate: pin exactly.

Fix prompt (paste into the 0B Antigravity conversation, same branch):
```
Review feedback for TASK-0B. Fix only these items on branch task/0B, and add a "Fixes" section to reports/REPORT-0B.md. Do not add scope.
1. compare.py: compute near-duplicate precision/recall with group-level ground truth (every pair of images in the same base group is a positive: 20 groups x 36 pairs = 720) and regenerate benchmark_summary.json and the near-duplicate table in docs/embedding-spike.md. Keep the old numbers in a clearly labelled "superseded: base-to-variant only" note.
2. docs/embedding-spike.md and REPORT-0B.md: quote the recommended configuration (Q8, budget 140) with its own cosines (mean 0.9872, min 0.9308 vs the budget-280 fp32 reference) and say budgets must never be mixed in one index; correct the statement that 128 d fell to 30 % (that was the wrong-normalisation run) and state that the fixture saturates at 100 % for every real configuration and so does not discriminate dimensions; note that RSS was measured in a Python process with other models loaded; correct "macOS 15+" to "macOS 27.0 (Darwin 27.0.0, arm64, Apple M4)".
3. Provenance: list the full 64-character SHA-256 of every ONNX file recommended (Q8 and Q4, text and vision, plus tokenizer.json), and add the repository LICENSE file text reference at the pinned commit (do not paste the licence text). Replace the licence link that points to gemma_4_license with the repo's own LICENSE.
4. Say plainly whether llama.cpp was actually run; if not, say it was rejected on the evidence of issue #30082 only.
5. Pin ort exactly in ort-probe/Cargo.toml (`=2.0.0-rc.15`).
```
Proposed TASK-0B2 (write after 0B is accepted): in Rust, tokenise text and preprocess images for the chosen ONNX files; produce fixture vectors; cosine at least 0.999 for Q8 at budget 280 vs the Python reference; threads sweep, time split, CoreML-on-vision-encoder, Windows run.

---

# Round 3 (2026-11-05): after the fixes

Evidence: owner's own `pnpm verify` (15 tests, all pass, including `test_child_killed_after_timeout` and `test_exiftool_rejects_non_absolute_path`), screenshot of the app running under `pnpm tauri dev` showing the thumbnail loaded through `asset-thumb://localhost/test-thumb`, `cargo 1.99.1 (2026-08-27)` (matches the report), updated REPORT-0A and REPORT-0B, git logs of both branches.

## TASK-0A — ACCEPT, after one owner-side check
- The two missing acceptance proofs now exist: the app runs in dev mode and the custom-protocol thumbnail renders under the CSP.
- The ExifTool child-process leak now has a test that proves the process is gone after a timeout.
- Not yet seen by the reviewer: the changed source. The report asks to confirm via `rg` for `unwrap/expect/panic` (output was not pasted) and `Cargo.toml` (a PID check with `kill(pid, 0)` may have added a dependency not listed in the report). NEXT_STEPS step 2 gives the owner three commands with expected results; any surprise is pasted to Claude before merging.
- Cosmetic: the report header commit hash (`cbeddb3`) does not appear in the branch log, and the acceptance table still contains pre-fix text (4 metadata tests, "macOS 26.0"). The "Fixes" section supersedes them. Use `git rev-parse --short HEAD` for the header in future.
- `cargo info tauri` printed `3.0.0-alpha.5`: a Tauri 3 alpha exists. We stay on the stable 2.x line (`Cargo.toml` says `"2"`). Never upgrade to an alpha. Confirm the locked version with `grep -A1 '^name = "tauri"$' src-tauri/Cargo.lock`.

## TASK-0B — ACCEPT (runtime decision: R2, ONNX Runtime via `ort`, Q8, 256 d, vision budget 140)
- Reviewer's near-duplicate suspicion confirmed: with group-level ground truth (720 pairs) the recommended Q8 b140 gets 91.6–100 % precision and 93.8–99.7 % recall at thresholds 0.90–0.95. The old 21–23 % precision was a counting artefact. Distractors still sit close to true duplicates, so embeddings stay advisory and pHash decides duplicates.
- Corrections made as requested: recommended-config cosines, budgets never mixed in one index, "128 d = 30 %" attributed to the wrong-normalisation run, fixture saturation admitted, RSS caveat, OS string, full SHA-256 values (they match the earlier truncated prefixes), llama.cpp "not run, rejected on issue #30082" (reviewer confirmed the issue), `ort` pinned with `=`.
- Not re-read by the reviewer: the regenerated `embedding-spike.md` and `benchmark_summary.json`; the report's numbers are consistent with the earlier data.
- Still open (by design): Rust tokenisation and image preprocessing parity, speed tuning, Windows run: TASK-0B2, written after the Phase 0 gate work.
- Licence note: neither repo ships a LICENSE file (licence is declared in README frontmatter). The app must ship the Apache 2.0 text (the owner's attached copy) plus a NOTICE naming the model, the converted-weights repository and commit.

## Owner decisions still needed (defaults used until answered)
The seven questions are in `docs/NEXT_STEPS.md` §8.

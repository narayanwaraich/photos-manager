# Project Status

Last updated: 2026-11-06 · Updated by the orchestrator after each review.

## Phase gates

| Phase | State | Gate evidence |
|---|---|---|
| 0 Feasibility | **In progress** (0A, 0B accepted and merged; 0C in FIX round 1) | reports/REVIEW-0A-0B.md, reports/REVIEW-0C.md. Still needed: 0C accepted, owner's Windows full pass |
| 1 Catalogue and scanning | Blocked on Phase 0 | |
| 2 Grid and viewer | Blocked | |
| 3 Metadata and search | Blocked | |
| 4 Exact and visual duplicates | Blocked | |
| 4B Local embeddings and semantic search | Blocked on 0B2 | |
| 5 Safe file operations | Blocked | |
| 6 Cloud AI (multi-provider) | Blocked; may shrink to bring-your-own-key + local endpoint and move after Phase 7 (owner's call) | |
| 7 Hardening and release | Blocked | |

## Tasks

| ID | Title | State | Verdict | Report | Tag |
|---|---|---|---|---|---|
| 0A | Scaffold, DB, libvips/ExifTool, protocol/CSP spikes | Accepted, merged to main | ACCEPT | reports/REPORT-0A.md | accepted/0A |
| 0B | EmbeddingGemma 2 runtime spike | Accepted, merged to main: ONNX Runtime Q8, 256 d, budget 140 | ACCEPT | reports/REPORT-0B.md | accepted/0B |
| 0B2 | Rust tokenisation + image preprocessing parity for the chosen ONNX files; run on both machines (proposed; write after 0C accepted; before Phase 4B) | Proposed | | | |
| 0C | CI matrix and clean-machine packaging | FIX round 1 (9 items; smoke test must run from the package, svgload bypass removed, licences/provenance, pins, Windows test evidence) | FIX | reports/REPORT-0C.md | accepted/0C (after ACCEPT and merge) |

## Decisions

Decision IDs refer to spec §3.5.

| ID | Decision | State |
|---|---|---|
| D1 | Tauri 2 + Rust + React/TS | Accepted |
| D2 | libvips + ExifTool, with fallback ladder | Accepted; libvips binding provisional (Windows uses a git fork of the bindings, see pending decisions) |
| D3 | Local embeddings first, cloud second | Accepted in principle; runtime provisional (0B; Rust parity in 0B2) |
| D4 | IPC type generation (`tauri-specta` vs manual DTO + Zod) | Open, decide at start of Phase 1 |
| D5 | EmbeddingGemma 2 licence: Apache 2.0 (owner-confirmed) | Accepted; ship notices; owner decides on redistributing converted weights |
| D6 | Weights downloaded on consent, checksum-pinned, never bundled | Accepted |
| D7 | English-only UI in v1, strings centralised | Accepted |

### Pending owner decisions (not yet in the spec)

| Topic | State |
|---|---|
| App identifier (placeholder `com.photosmanager.desktop`; app-data folder is derived from it) | Reviewer recommends choosing before Phase 1; owner's default was "placeholder stays" |
| HEIF/HEIC decode on macOS (Windows build has it, macOS build does not) | Not needed for Phase 1; needed by Phase 2. Lean: own minimal libvips build for both OSes (one task before Phase 2). Apple ImageIO-only on macOS would be a spec change |
| Windows libvips binding: personal git fork of the Rust bindings (rung 1 of the §3.3 ladder) | Provisional; 0C fix item 8 documents it; revisit in the libvips build task |
| D4 | Open (see above) |

### Owner's answers (2026-11-06)
- Windows PC: Acer Swift SF314-43 laptop, Ryzen 7 5700U, 8 GB RAM, 496 MB graphics, Windows 11. It is the low-end reference machine.
- Cloud AI: not a priority; small local models and/or bring-your-own accounts (in practice: user's own API key or a local OpenAI-compatible endpoint).
- Defaults accepted: personal use first, library up to 100k photos on SSD, JPEG + iPhone HEIC (RAW later), sidecars and other managers ignored, name "Photos Manager".

### Pending spec edits (apply together as spec v2.1 when the first Phase 1 task is issued)
1. §9.1: embedding is opt-in per root, resumable, pausable, with an ETA; default budget 140 and 256 d; budgets never mixed in one index.
2. §15: name the reference machines (Mac M4 16 GB; Windows Ryzen 7 5700U 8 GB) and measure on both.
3. §9.2 / §19.2: cloud AI is bring-your-own-key plus local OpenAI-compatible endpoint first; Phase 6 may follow Phase 7.
4. §17.3: Rust toolchain pinned to an exact version (not `stable`).
5. §3.3 / §6.5: HEIF stack, after the owner's decision.

## Known risks

1. Packaging libvips, ExifTool (Perl) and the embedding runtime self-contained on both OSes is the largest technical risk. 0C shows both OSes pass a smoke test with PATH stripped, but from the build folder; package-level proof is pending (0C fix 1, 2).
2. Image embedding runtime: ONNX exports exist but preprocessing and input names are undocumented outside Transformers.js; image embedding through mainline llama.cpp is unproven (confirmed unusable, issue #30082).
3. The coding model's knowledge cutoff (March 2026) can produce stale crate and API usage.
4. Windows is validated through CI plus the owner's manual pass on the Windows laptop (`docs/ORCHESTRATION.md` §6); the owner develops on a Mac.
5. Gemini 3.8 Flash API model ID verified in 0A; promotional pricing ends 31 Dec 2026. Cloud AI is low priority now.
6. Embedding time: about 1 s per image on an M4 CPU (budget 140), about 27 h for 100k photos; the Windows laptop is probably several times slower (not measured). Embedding must be opt-in, resumable, with ETA.
7. Rust-side tokenisation and image preprocessing for the ONNX path are unproven (0B2).
8. Phase 2 prerequisite (found in 0A check): `VipsImageProcessor::new()` calls `VipsApp::new(...).expect(...)` (image_pipeline/vips.rs:12) and the thumbnail handler creates one per request; libvips must be initialised once (e.g. `OnceLock`) and return a `Result` instead of panicking (spec §3.3, §11). `lib.rs` keeps the standard Tauri `.expect` on startup; replace with an error dialog in Phase 1. 0C may have changed these lines; re-check.
9. Scope creep toward later-version features (spec §13); each needs an ADR first.
10. Bundled libvips builds currently include `svgload` (librsvg) on both OSes: attack surface not wanted in v1 (0C fix 3).
11. macOS libvips is a Homebrew harvest (65 libraries) with no licence inventory and no HEIF; macOS ExifTool may depend on system Perl (0C fixes 4, 5).
12. iPhone HEIC is a main format of the owner; decode support must be equal on both OSes by Phase 2.
13. The Windows libvips binding is a single-maintainer git fork (0C fix 8).
14. 8 GB Windows laptop: Rust builds may be slow or memory-tight; app memory target (≤ ~500 MB RSS excluding model) should be checked there.

## Log

- 2026-11-02 — Pack v1 created.
- 2026-11-02 — Pack v2: spec rewritten as one merged file (v2.0) with extension seams for RAW, maps, import, backup, video, faces; amendments, AI addendum and ADR files folded in and removed; Windows PC validation added; tasks 0A/0B/0C revised; ANTIGRAVITY_SETUP and three skills added.
- 2026-11-03 — Reports 0A and 0B reviewed (reports/REVIEW-0A-0B.md): 0A FIX (evidence), 0B provisional ACCEPT of ONNX Runtime Q8 + FIX (evidence); 0B2 proposed.
- 2026-11-05 — Round 2 review (reports/REVIEW-0A-0B.md): concrete fix prompts issued for 0A and 0B; git cleanup plan given (prep commit on main, merge main into each branch, squash-merge on ACCEPT); owner's terminal lacks cargo on PATH.
- 2026-11-05 — Round 3: 0A and 0B ACCEPTED. App runs under `pnpm tauri dev` (screenshot), 15 tests pass on the owner's machine. Next: merge both to main, run TASK-0C. Owner asked for `docs/NEXT_STEPS.md` (single step-by-step file, rewritten each conversation) and a Rust/Git learning track (`docs/LEARNING.md`).
- 2026-11-05 (evening) — 0A confirmation checks passed (Cargo.toml no longer has panic=abort; no new dependency; `unwrap/expect` hits are tests, two response-builder unwraps in lib.rs, startup expect, and vips.rs:12 logged as Phase 2 prerequisite). Owner had already unzipped the doc update on task/0A; fixed by carrying the changes to main. NEXT_STEPS rewritten.
- 2026-11-06 — 0A and 0B squash-merged to `main`, tagged, pushed (git log shows origin/main at accepted/0B). TASK-0C ran: 10 CI runs, run #9 green on macOS arm64, Windows x64 and cargo deny. Reviewed (reports/REVIEW-0C.md): FIX, nine items. Owner's answers recorded (Windows laptop specs, cloud AI de-prioritised, defaults for the rest). NEXT_STEPS and LEARNING (lesson 2) updated.

# Project Status

Last updated: 2026-11-02 · Updated by the orchestrator after each review.

## Phase gates

| Phase | State | Gate evidence |
|---|---|---|
| 0 Feasibility | **In progress** (0A, 0B accepted; 0C next) | See reports/REVIEW-0A-0B.md |
| 1 Catalogue and scanning | Blocked on Phase 0 | |
| 2 Grid and viewer | Blocked | |
| 3 Metadata and search | Blocked | |
| 4 Exact and visual duplicates | Blocked | |
| 4B Local embeddings and semantic search | Blocked on 0B | |
| 5 Safe file operations | Blocked | |
| 6 Cloud AI (multi-provider) | Blocked | |
| 7 Hardening and release | Blocked | |

## Tasks

| ID | Title | State | Verdict | Report | Tag |
|---|---|---|---|---|---|
| 0A | Scaffold, DB, libvips/ExifTool, protocol/CSP spikes | Accepted (owner runs 3 confirmation commands, then squash-merges) | ACCEPT | reports/REPORT-0A.md | accepted/0A (after merge) |
| 0B | EmbeddingGemma 2 runtime spike | Accepted: ONNX Runtime Q8, 256 d, budget 140 | ACCEPT | reports/REPORT-0B.md | accepted/0B (after merge) |
| 0B2 | Rust tokenisation + image preprocessing parity for the chosen ONNX files (proposed, before Phase 4B) | Proposed | | | |
| 0C | CI matrix and clean-machine packaging | Ready to run after 0A and 0B are merged to main | | | |

## Decisions

Decision IDs refer to spec §3.5.

| ID | Decision | State |
|---|---|---|
| D1 | Tauri 2 + Rust + React/TS | Accepted |
| D2 | libvips + ExifTool, with fallback ladder | Accepted; libvips binding provisional (0A) |
| D3 | Local embeddings first, cloud second | Accepted in principle; runtime provisional (0B) |
| D4 | IPC type generation (`tauri-specta` vs manual DTO + Zod) | Open, decide at start of Phase 1 |
| D5 | EmbeddingGemma 2 licence: Apache 2.0 (owner-confirmed) | Accepted; ship notices; owner decides on redistributing converted weights |
| D6 | Weights downloaded on consent, checksum-pinned, never bundled | Accepted |
| D7 | English-only UI in v1, strings centralised | Accepted |

## Known risks

1. Packaging libvips, ExifTool (Perl) and the embedding runtime self-contained on both OSes is the largest technical risk.
2. Image embedding runtime: ONNX exports exist but preprocessing and input names are undocumented outside Transformers.js; image embedding through mainline llama.cpp is unproven (TASK-0B).
3. The coding model's knowledge cutoff (March 2026) can produce stale crate and API usage.
4. Windows is validated through CI plus the owner's manual pass on the Windows PC (`docs/ORCHESTRATION.md` §6); the owner develops on a Mac.
5. Gemini 3.8 Flash API model ID unverified; promotional pricing ends 31 Dec 2026.
6. Embedding time: about 1 s per image on an M4 CPU (budget 140) means about 27 h for 100k photos; embedding must be opt-in, resumable, with ETA (spec §9.1 change proposed).
7. Rust-side tokenisation and image preprocessing for the ONNX path are unproven (0B2).
8. Scope creep toward later-version features (spec §13); each needs an ADR first.

## Log

- 2026-11-02 — Pack v1 created.
- 2026-11-02 — Pack v2: spec rewritten as one merged file (v2.0) with extension seams for RAW, maps, import, backup, video, faces; amendments, AI addendum and ADR files folded in and removed; Windows PC validation added; tasks 0A/0B/0C revised; ANTIGRAVITY_SETUP and three skills added.
- 2026-11-03 — Reports 0A and 0B reviewed (reports/REVIEW-0A-0B.md): 0A FIX (evidence), 0B provisional ACCEPT of ONNX Runtime Q8 + FIX (evidence); 0B2 proposed.
- 2026-11-05 — Round 2 review (reports/REVIEW-0A-0B.md): concrete fix prompts issued for 0A and 0B; git cleanup plan given (prep commit on main, merge main into each branch, squash-merge on ACCEPT); owner's terminal lacks cargo on PATH.
- 2026-11-05 — Round 3: 0A and 0B ACCEPTED. App runs under `pnpm tauri dev` (screenshot), 15 tests pass on the owner's machine. Next: merge both to main, run TASK-0C. Owner asked for `docs/NEXT_STEPS.md` (single step-by-step file, rewritten each conversation) and a Rust/Git learning track (`docs/LEARNING.md`).

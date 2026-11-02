# Project Status

Last updated: 2026-11-02 · Updated by the orchestrator after each review.

## Phase gates

| Phase | State | Gate evidence |
|---|---|---|
| 0 Feasibility | **Not started** | |
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
| 0A | Scaffold, DB, libvips/ExifTool, protocol/CSP spikes | Ready to run | | | |
| 0B | EmbeddingGemma 2 runtime spike (ONNX/llama.cpp vs Python reference) | Ready to run | | | |
| 0C | CI matrix and clean-machine packaging | Waiting for 0A | | | |

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
6. Scope creep toward later-version features (spec §13); each needs an ADR first.

## Log

- 2026-11-02 — Pack v1 created.
- 2026-11-02 — Pack v2: spec rewritten as one merged file (v2.0) with extension seams for RAW, maps, import, backup, video, faces; amendments, AI addendum and ADR files folded in and removed; Windows PC validation added; tasks 0A/0B/0C revised; ANTIGRAVITY_SETUP and three skills added.

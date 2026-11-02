# Photos Manager — Antigravity Workspace Rules

Local-first Windows/macOS desktop photo manager. You are the **implementer**. A separate orchestrator reviews your written reports and decides what happens next. Work only from the task file you are given.

## Document precedence
1. This file (safety invariants are never relaxed)
2. `IMPLEMENTATION_SPEC.md` (the single spec; cite sections as §N)
3. The current task file, within the limits of 1 and 2

If two documents conflict, stop and report the conflict. Never pick the riskier reading.

## How you work
- One task per conversation. Start from `tasks/TASK-<id>-*.md`. Read this file, the task file, and only the spec sections the task lists (read the whole spec only if the task says so).
- Work on git branch `task/<id>`. Commit in small steps. Never force-push or rewrite history. Never commit secrets, model weights, or any binary over 5 MB without approval.
- Stay inside the task's *Allowed paths*. Do not edit `AGENTS.md` or `IMPLEMENTATION_SPEC.md`; propose changes in your report. Record new decisions as `docs/adr/NNNN-title.md` only when the task says so.
- No placeholder features, fake data or stub commands that look implemented.
- Finish by writing `reports/REPORT-<id>.md` from `reports/_REPORT_TEMPLATE.md`, then **stop**. Do not begin the next task.
- Evidence rule: every claim in the report needs a command plus its real output, or a file path. "Not tested" is an acceptable answer. An unsupported "works" is not.
- If a requirement is ambiguous or conflicts with a rule here, stop and ask in the report instead of guessing.

## Do not trust memory for versions
Your training knowledge ends before today's date (the Gemini 3.8 Flash model card gives a March 2026 cutoff). Versions and APIs of Tauri, Rust crates, npm packages, libvips, ExifTool, llama.cpp, Gemini and other providers must be checked against official docs or registries (`cargo info`, `npm view`, official docs) before use. Never invent package names, API fields, CLI flags or model IDs. If you cannot verify something, say so.

## Architecture (non-negotiable)
- Stack: Tauri 2, React, TypeScript, Vite, Rust, SQLx + SQLite, libvips, ExifTool. Local embeddings (EmbeddingGemma 2, Apache 2.0) and cloud AI providers are optional modules. RAW, video, faces, maps, import and backup are later versions (spec §13): design seams exist, but never scaffold, stub or add empty tables for them.
- Rust owns filesystem access, SQLite, scanning, image processing, metadata, duplicate detection, jobs, embeddings, AI requests and file operations.
- React calls typed Tauri commands through `src/lib/tauri/`. Never expose arbitrary SQL, shell access or unrestricted filesystem access to the frontend.
- TanStack Query for catalogue state, Zustand for UI state only, TanStack Virtual for large grids and lists.
- Originals stay in place. SQLite stores catalogue data; the app cache stores regenerable thumbnails. Never modify originals while scanning, hashing, previewing, embedding or analysing.
- Core features work offline with no API key. AI is opt-in and never controls file changes.

## Safety invariants
- Every move/copy/rename/trash goes through the Rust `OperationManager` and a persisted journal.
- Show a validated dry-run plan and conflict list before any bulk change. Never silently overwrite.
- Cross-volume move = stage copy → verify size + BLAKE3 → finalize destination → only then remove source.
- Delete means OS Trash/Recycle Bin. Never silently fall back to permanent deletion.
- Do not follow symlinks by default. Validate path containment in Rust; never trust path strings from the frontend or from an AI.
- Never auto-delete duplicates. Duplicate groups are suggestions the user reviews.
- Cloud AI payloads need explicit per-provider consent; strip GPS and unnecessary metadata; send no paths or filenames. Never log credentials or image data.
- API keys live in the OS credential store, one entry per provider. Never in files, SQLite, logs or the frontend.

## AI rules
- Providers sit behind `AiProvider`; embedding runtimes behind `EmbeddingProvider`. Product code never depends on one vendor or runtime.
- Model IDs are settings, not constants. Validate every AI response in Rust types.
- Embeddings: apply the documented text prefix, never prefix images, truncate then L2-normalize, no float16 unless TASK-0B validated it against the fp32 reference, store `model_id`, `model_revision`, `dim`, `source`. Never compare vectors from different models or dims.
- Embeddings and AI output are advisory. They never decide exact duplicates and never trigger file operations.
- Model weights are downloaded on consent with a pinned checksum. Never bundled, never committed.

## Engineering rules
- One phase/vertical slice at a time. Do not scaffold future features.
- Before adding a native dependency, verify maintenance, license, OS/architecture support, codecs and clean-machine packaging. A dependency that works only because Homebrew/Chocolatey/system libraries are installed does **not** work.
- SQLx repositories and versioned migrations in Rust. No generic `execute_sql` command.
- Tauri Channels for ordered progress; events only for low-volume notices. Persist job state.
- Native and CPU-heavy work stays off the UI thread and the Tokio executor; use bounded queues, semaphores and cancellation checkpoints.
- Typed DTOs with camelCase serialization and stable error codes. Zod at the frontend boundary.
- DB and cache stay on local disk. No WAL database on a network share.
- No O(n²) image comparison at scale. BLAKE3 for exact; versioned pHash with Hamming-distance candidate search for visual. Similarity is not transitive; avoid naive connected components.
- Pin dependencies in lockfiles. Add libraries only with a clear need, and list each new one in the report.
- `pnpm verify` is the single verification command (created in TASK-0A). Run it before reporting.
- Keep this file under 12,000 characters; `pnpm verify` checks it.

## Environment limits
- Network: package registries, GitHub, official documentation, and any host the task file names. Nothing else.
- Use generated fixtures and temporary directories only. Never read, modify or upload a real user photo library unless the task names a specific local-fixtures folder and says it is read-only.
- No destructive shell commands outside the workspace or temp directories. Declare any global tool you install.
- Platform honesty: say which OS you actually tested (primary dev is macOS; the owner validates on a Windows PC and CI). Never claim Windows or macOS support from a build on only one. Write path, drive and Trash code for both OSes from the start.

## Report format
Use `reports/_REPORT_TEMPLATE.md`. Be concise and factual. List what is not done.

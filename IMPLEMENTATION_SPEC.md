# Photos Manager — Product and Technical Specification

**Version:** 2.1 (merged rewrite, updated after Phase 0 findings) · **Date:** 2026-11-12
**Targets:** Windows 10/11 x64 and macOS (Apple Silicon first). Primary development on macOS; a Windows laptop is available for validation.
**Model:** local-first, single-user desktop app; many library roots; optional AI.
**Stack:** Tauri 2 · Rust · React + TypeScript · SQLite · libvips · ExifTool · optional local embeddings (EmbeddingGemma 2) · optional cloud AI providers.

This file replaces the earlier draft spec, the amendments file, the AI addendum and the decision records. It is the single source of truth. Status tags: **[MVP]** build in v1, **[LATER]** designed for but not built in v1, **[SPIKE]** unverified until a Phase 0 task proves it.

## 0. About this document

- **Precedence:** `AGENTS.md` (safety rules) > this file > everything else. If two sources conflict, stop and report; never pick the riskier reading.
- **Tasks cite sections** by number (for example "§9.2"). Agents read this file's sections named in their task, not the whole file, except in TASK-0A.
- **Unverified facts** are marked **(unverified)**. Library versions, API fields, model IDs and CLI flags must be checked against official sources before use; the coding model's knowledge ends in March 2026.
- **Extensibility is designed, not scaffolded.** Later features (§13) must be possible without rewriting v1, but v1 contains no placeholder modules, empty tables or stub commands for them.
- **Agent boundaries (D17, also in `AGENTS.md`):** coding agents never create, change or delete GitHub secrets, tokens, repositories, releases or settings, and never read or reuse the owner's login token. They stop and tell the owner what to do.

---

## 1. Product definition

A cross-platform desktop photo manager that indexes photos wherever they already live, shows one unified catalogue, finds exact and visually similar duplicates, and gives safe tools to view, search, tag, group, move, copy, rename and reorganise files.

It is **not** an importer that forces originals into a proprietary library. Originals stay at their paths unless the user runs an explicit file operation. The database and cache live elsewhere.

The first real use is managing the owner's iPhone photos (HEIC, and sometimes Apple ProRAW) on a Mac and a Windows laptop.

### 1.1 Principles

1. **Local-first.** Browsing, indexing, metadata, duplicates, semantic search (with local embeddings) and file operations work offline.
2. **Originals are authoritative.** Indexing, hashing, thumbnails, embeddings and AI analysis never modify an original.
3. **AI is optional and advisory.** No core feature needs a key or the network. AI never decides exact duplicates and never changes files.
4. **Safe by default.** No automatic permanent deletion, no unreviewed bulk change, no silent overwrite.
5. **Incremental and resumable.** Scans and jobs continue after restarts without redoing unchanged work.
6. **Observable.** Long work shows progress, warnings and cancellation.
7. **Recoverable.** File operations are journaled; crashes expose a recovery path.
8. **Portable.** Same behaviour on both OSes; platform code sits behind adapters.
9. **Extensible.** Physical files, logical assets, volumes, derivatives and analyzers are separate concepts (§5, §13), so video, RAW, faces, maps, import and backup add to the model instead of bending it.

### 1.2 MVP scope [MVP]

Add/remove/pause/rescan library roots · recursive indexing (symlinks off) · virtualized thumbnail grid and folder tree · full-size viewer with metadata panel · search and filters (name, folder, date, size, dimensions, camera, tags, favorites) · exact duplicates (BLAKE3) · visually similar groups (pHash) · local semantic search and "find similar" (EmbeddingGemma 2, opt-in per root) · safe move/copy/rename/trash with dry-run, journal, undo where possible · iPhone HEIC photos · RAW photos shown from their embedded JPEG preview (Tier 1, §13.1, D15) · optional cloud captions/tags/query parsing through a provider abstraction (bring-your-own-key, §9.2).

### 1.3 Later versions [LATER] — designed for in §13

Full RAW decoding (Tier 2) and RAW+JPEG pairs · GPS map view · import from external drives and cards · photo backup · video management · face recognition. Suggested order: RAW pairs → import → backup → maps → video → faces.

### 1.4 Not in scope

Photo editing, cloud photo storage, multi-user libraries, an always-running daemon, AI-controlled file changes, metadata writes into originals (any future write feature is separate, opt-in and backed up).

### 1.5 Vocabulary

- **Volume:** a disk or share a library can live on (internal, removable, network).
- **Library root:** a user-chosen directory on a volume that is indexed.
- **File:** one physical file at one path. Operations, hashes and backups act on files.
- **Asset:** one logical photo or video the user sees. It has a primary file and may later have more (RAW + JPEG, sidecar, Live Photo video).
- **Derivative:** generated, disposable output for an asset (thumbnail, preview, RAW preview, poster frame, face crop).
- **Analyzer:** a versioned background computation on assets (pHash, embedding, caption, later face detection).
- **Exact duplicate:** byte-identical files. **Visual duplicate:** similar image content (resized, recompressed, rotated, lightly edited).
- **Job:** asynchronous background work. **Operation:** a user-requested file change (move, copy, rename, trash, later import/backup).

---

## 2. Architecture

A modular monolith: one Tauri desktop process, one React WebView, one Rust backend, one SQLite file, background workers. No microservices and no local HTTP server for the app itself.

```text
┌──────────────────────────────────────────────────────────────┐
│ React + TypeScript   (TanStack Query · Zustand · Virtual)    │
└───────────────┬──────────────────────────────────────────────┘
                │ typed Tauri commands · Channels for progress
┌───────────────▼──────────────────────────────────────────────┐
│ Rust backend                                                 │
│  commands ─ domain services                                  │
│   ├ library/scanner/watcher     ├ metadata (ExifTool)        │
│   ├ catalog (SQLx repositories) ├ image pipeline (libvips)   │
│   ├ duplicates (BLAKE3, pHash)  ├ embeddings (EmbeddingProv.)│
│   ├ jobs (queues, cancel)       ├ ai (AiProvider adapters)   │
│   └ operations (journal, undo, Trash)  security · platform   │
└───────┬──────────────┬───────────────┬───────────────────────┘
   SQLite catalogue  originals      app cache (derivatives)
                       (read-only except via operations)
                           optional outbound: cloud AI (opt-in)
```

### 2.1 Boundaries

- React never uses Node APIs, shell, arbitrary filesystem access or SQL. It calls a small typed command set through `src/lib/tauri/`.
- Rust owns paths, files, SQLite, scanning, decoding, hashing, embeddings, AI requests and every file mutation.
- SQLite is the source of truth for catalogue and user organisation data. The filesystem is the source of truth for original bytes and current paths.
- The derivative cache is disposable and rebuildable.
- No unrestricted `execute_sql`, shell or broad filesystem permission in the WebView.

### 2.2 Process model

Everything runs in the Tauri process except optional sidecars (ExifTool; later a local model server or ffmpeg) launched by Rust with fixed paths and argument arrays. Sidecars are bounded, time-limited and restarted on failure.

### 2.3 Data locations (resolve with Tauri path APIs; never hard-code)

| Data | Location | Notes |
|---|---|---|
| SQLite DB + WAL | app data dir | Local disk only, never a network share, never inside a library root |
| Derivative cache | app cache dir | Rebuildable |
| Model files | app data dir `models/` | Downloaded on consent, checksum-pinned, never committed |
| Logs | app log dir | Redact paths and secrets |
| Settings | SQLite `app_settings` | No secrets |
| API keys | OS credential store | One entry per provider |
| Originals | user library roots | Never copied or deleted silently |

The app data folder is named after the app identifier `com.narayanwaraich.photosmanager` (D8).

---

## 3. Technology decisions

### 3.1 Frontend

React + TypeScript (strict, no `any`) · Vite · Tailwind CSS · shadcn/ui + Radix · TanStack Router · TanStack Query (catalogue state) · Zustand (UI-only state) · TanStack Virtual (required for grids and lists) · TanStack Table (details view) · dnd-kit · react-resizable-panels · Zod (runtime validation at the IPC boundary) · lucide-react. Add no overlapping UI library. Do not mirror the catalogue in Zustand.

### 3.2 Rust backend

| Concern | Choice | Rule |
|---|---|---|
| Shell | `tauri` 2.x | Capabilities least-privilege; never an alpha |
| Async | `tokio`, `tokio-util` (cancellation) | CPU/native work off the executor |
| DB | `sqlx` (SQLite, migrations) | Verified in TASK-0A; `rusqlite` on a dedicated thread is the accepted alternative if SQLx proves awkward |
| Errors / IDs / time | `thiserror` + `anyhow`, `uuid`, `time` | Stable error codes at the IPC edge |
| Hash | `blake3` | Streaming |
| Traversal / watching | `walkdir` then `jwalk` only if profiled; `notify` + debounce | |
| HTTP | `reqwest` with rustls | Cloud AI and model download |
| Secrets | OS credential store (`keyring` or reviewed Tauri plugin) | Never plaintext |
| Trash | tested cross-platform adapter (`trash` crate candidate) | Verify on both OSes |
| ONNX | `ort` (ONNX Runtime), pinned exactly | Chosen by TASK-0B (R2); Rust parity proven by TASK-0B2 **[SPIKE]** |
| Logging | `tracing` | |

### 3.3 Image and metadata libraries

- **libvips is the only image engine on both OSes (D10).** It does thumbnails, resize, orientation and all decoding, behind an `ImageProcessor` trait. There is no second decode path (no macOS ImageIO, no Windows WIC) in v1: it would give different colour, orientation and HDR behaviour on one OS and add unsafe FFI. The Rust binding is the `libvips` crate (resolved as 1.7.2; on Windows a personal git fork of the bindings through `[patch.crates-io]` in `src-tauri/Cargo.toml`, rung 1 of the ladder below; revisit once the own-build zip is stable on both OSes). Rules: initialise libvips once at app start and return a `Result` (never `.expect()` inside a lazy init, never `std::env::set_var` after threads exist); limit concurrency; query loader capabilities (HEIC/AVIF/WebP/TIFF) at runtime; libvips's error buffer is process-wide, so read it through a locked copy or serialise error reads before any worker pool thumbnails in parallel.
- **HEIF stack (D11, D18):** the HEIC/HEIF/AVIF loader is built **into** our own pinned libvips build (libvips 8.18.6; libheif with libde265 for HEVC decoding and libaom for AV1 decoding; **no x265 and no encoders**; no runtime plugin or module loading). The same recipe and versions on both OSes, so both behave the same on the same files. HEVC patents are separate from library licences (fine for personal use; a legal check is required before any distribution, §19.1). Decoder libraries are dynamic libraries so they stay replaceable (LGPL).
- **Known differences between the two builds (D20):** the macOS build cannot write GIF (libvips probes `cgif` only when `imagequant` or `quantizr` is found, and Homebrew's `imagequant` 4.x is GPL; GIF reading works through `nsgif`) and targets macOS 15.0 or later. Revisit in Phase 7 if GIF export or older macOS is wanted.
- **ExifTool** is a bundled sidecar, invoked without a shell with argument arrays, batch JSON output, size and time caps; its child process is killed on timeout and oversize output. Call it with minor-warning tolerance (`-m`) so large array tags do not fail a read. Read-only in v1. It also extracts embedded RAW previews (§13.1).
- **LibRaw** (full RAW rendering, through libvips `dcrawload`) and **ffmpeg** (video) are [LATER], behind `RawDecoder` and `VideoProbe`/`VideoDecoder` traits. Mind LGPL/GPL obligations when choosing builds. The stock Windows libvips zip contains no LibRaw, so Tier 2 RAW needs our own pinned build (§3.4).
- The Rust `image` crate may serve tests and a fallback; it is not HEIC/RAW support.

**Fallback ladder** if libvips cannot be made self-contained on both OSes, tried in order and each logged as a decision: (1) another binding or our own thin C-API wrapper; (2) bundled libvips command-line tools as a sidecar; (3) pure-Rust JPEG/PNG/WebP/TIFF (`image`, `fast_image_resize`, `kamadak-exif`) with HEIC/RAW shown as unsupported; (4) a stack change (Electron + `sharp`), which needs explicit human approval. `rexiv2` is excluded (needs a system library). A platform decoder for HEIC is a documented fallback only, and only with the owner's agreement.

### 3.4 Native dependency release gate

Before any feature depends on a native dependency (libvips, ExifTool, ONNX Runtime, later LibRaw/ffmpeg), prove a **clean release build** loads it on both OSes with Homebrew/Chocolatey/system copies removed from the environment. Record exact version, source, license, architecture, codecs, bundling path and smoke result in `docs/native-dependencies.md`. System-installed copies never count.

**Dependency builds we produce ourselves (libvips with HEVC decode, later LibRaw or others):**

1. The recipe lives under `deps/<platform>-vips/` (README, `UPSTREAM.txt`, build script, audit script). Upstream source is **not** vendored: every tarball, commit and base image is pinned by SHA-256 or digest and recorded in `UPSTREAM.txt` (D14). Leaf libraries taken from a package manager are recorded with versions and licences.
2. The build runs in a **manually started** workflow (`workflow_dispatch` only), is audited (no forbidden library by file name or linkage, no non-system absolute path, no GPL or unknown licence), and is tested on a second clean runner with the package manager hidden, using only the zip.
3. The result is stored as a **checksum-pinned release asset in the private deps repo** (D13), never as a public release of this repository and never rebuilt by app CI. A rebuild floats (leaf versions, base images), so the stored asset and its SHA-256 are the real artifact.
4. App CI downloads it **fail-closed**: token present on non-fork runs, a download with real retries and exit-code checks, SHA-256 matched, `REQUIRE_HEIC=1` for the smoke test. A fork pull request (no secrets) falls back to the stock or Homebrew path and reports `heic_decode_verified: false`. An expired token turns CI red on purpose.
5. **Gate-script rule (D19):** audit and CI check scripts never end a pipe in an early-exiting reader (`grep -q`, `head`) under `set -o pipefail`: a closed pipe fails the pipeline, giving a false alarm or, when the check looks for something bad, a silent pass. Large output goes to a file or to a variable read without a pipe. Every gate is proven against a bad input of realistic size with the needle first (fake `strings`/`nm` on PATH), not only against tiny files, and has a committed regression test.

### 3.5 Decision log

| ID | Decision | State |
|---|---|---|
| D1 | Tauri 2 + Rust + React/TS. Electron + `sharp` is the fallback only if the §3.3 ladder is exhausted; needs human approval | Accepted |
| D2 | libvips + ExifTool, ladder in §3.3 | Accepted; binding provisional (Windows uses a git fork) |
| D3 | Local embeddings first (EmbeddingGemma 2), cloud providers second and opt-in | Accepted; runtime chosen by TASK-0B (ONNX Runtime via `ort`, Q8, 256 d, vision budget 140), Rust parity pending (TASK-0B2) |
| D4 | IPC types: hand-written DTOs + Zod, or generate from Rust (`tauri-specta` candidate). Agent verifies Tauri 2 support, then the owner decides | Open, decide at start of Phase 1 |
| D5 | EmbeddingGemma 2 is Apache 2.0 (owner and Google's announcement). Ship the license text and notices with the app and any redistributed weights; link Google's acceptable-use note in the licenses screen | Accepted |
| D6 | Model weights are downloaded on consent from a pinned source with a pinned SHA-256; never bundled or committed | Accepted |
| D7 | UI language is English in v1; keep strings in one place so localisation is possible | Accepted |
| D8 | App identifier `com.narayanwaraich.photosmanager`; the app-data folder name follows it; dev data is not migrated | Accepted |
| D9 | iPhone HEIC decode on both OSes is a Phase 0 task | Accepted |
| D10 | libvips stays the only image engine on both OSes; no ImageIO or WIC seam in v1 | Accepted |
| D11 | HEIC-capable libvips comes from our own pinned build recipe (libheif + libde265 + libaom decode, no x265) | Accepted |
| D12 | CI hygiene: docs-only commits `[skip ci]` and `paths-ignore`; `concurrency` cancels superseded runs; push trigger `task/**` and `main` (§17.1) | Accepted |
| D13 | Public repository (free Actions minutes): no secrets or personal data in files or history; no binary with HEVC or GPL code as a public release; the HEVC libvips zips live in a private deps repo behind a read-only token; app bundles and installers are Actions artifacts with short retention | Accepted |
| D14 | Own-build recipes pin their base images and tarballs by digest or SHA-256 and record them in `UPSTREAM.txt` (§3.4) | Accepted |
| D15 | RAW in two tiers: Tier 1 (embedded JPEG preview through ExifTool, no new native library) planned for Phase 1/2; Tier 2 (full decode through LibRaw) later with its own ADR (§13.1) | Proposed; owner confirms |
| D16 | Commits use GitHub's no-reply address; existing history is not rewritten | Accepted |
| D17 | Agent boundaries for secrets and accounts (§0, `AGENTS.md`) | Accepted |
| D18 | The macOS libvips is our own build on a manual macOS arm64 runner workflow, stored checksum-pinned in the private deps repo | Accepted |
| D19 | Gate-script rule: no early-exit pipe readers under `pipefail`; every gate proven against a realistic bad input (§3.4) | Accepted |
| D20 | macOS libvips differs from the Windows zip: no GIF writing, deployment target macOS 15.0 (§3.3) | Accepted |

---

## 4. Repository and module layout

```text
photos-manager/
├── AGENTS.md  IMPLEMENTATION_SPEC.md  README.md
├── .agents/rules/   (workspace skills: see docs/ANTIGRAVITY_SETUP.md for the path your version uses)
├── docs/        (ORCHESTRATION, REVIEW_CHECKLIST, STATUS, ANTIGRAVITY_SETUP, native-dependencies, ...)
├── deps/        (recipes for dependency builds we produce ourselves: win64-vips, macos-vips)
├── tasks/  reports/  spikes/
├── src/                      # React: app/ components/ features/ lib/tauri/ stores/ shared/
├── src-tauri/
│   ├── migrations/  capabilities/  binaries/  resources/
│   └── src/ commands/ domain/ database/ library/ scanner/ watcher/ metadata/
│            image_pipeline/ duplicates/ embeddings/ ai/ jobs/ operations/
│            filesystem/ security/ platform/
└── tests/ fixtures/ integration/ e2e/
```

Create a module only in the phase that needs it.

| Module | Owns | Does not own |
|---|---|---|
| `commands` | DTOs, validation, dispatch | SQL, business loops |
| `domain` | Types, invariants | Platform APIs |
| `database` | Pool, migrations, repositories | File changes |
| `library`/`scanner`/`watcher` | Volumes, roots, traversal, scan generations, change hints | UI state |
| `metadata` | ExifTool adapter, normalisation | Writing metadata |
| `image_pipeline` | `ImageProcessor`, derivatives, pHash pixels | DB orchestration |
| `duplicates` | BLAKE3, pHash, candidate index, grouping | Deleting files |
| `embeddings` | `EmbeddingProvider`, model lifecycle, vector search | Provider HTTP for cloud LLMs |
| `ai` | `AiProvider` adapters, prompts, schema validation, consent, budgets | File actions |
| `jobs` | Queues, priorities, cancellation, progress | Feature SQL |
| `operations` | Plans, journal, execution, undo, recovery | UI |
| `filesystem`/`security`/`platform` | Path model, containment, secrets, OS differences | Presentation |

---

## 5. Data model

### 5.1 Rules

- SQLite via SQLx in Rust. `PRAGMA foreign_keys = ON` per connection, WAL, busy timeout, bounded pool, short write transactions.
- **Migrations are additive per phase.** The schema below is the target. Each phase's migration creates only the tables that phase uses.
- No image bytes in SQLite. Timestamps are UTC Unix milliseconds in `*_ms` columns; keep the original EXIF date string and offset separately. Never assume UTC when the photo has no offset.
- **Paths:** store a native-path BLOB (`rel_path_native`) plus a display string, relative to the library root, so remounting a drive means updating one root row. Non-UTF-8 or unrepresentable paths are skipped and reported, never lossily converted. Do not lowercase paths blindly; derive `path_key` from the volume's case-sensitivity and Unicode-normalisation policy.
- Deleting a catalogue row never deletes a file.
- Hashes are `BLOB`; sizes are checked to fit signed INTEGER.

### 5.2 Target schema (logical; types and helper columns may change through reviewed migrations)

```sql
-- Storage
CREATE TABLE volumes (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL DEFAULT 'unknown' CHECK (kind IN ('internal','removable','network','unknown')),
  label TEXT, fs_uuid TEXT, fs_type TEXT,
  case_sensitive INTEGER NOT NULL DEFAULT 0 CHECK (case_sensitive IN (0,1)),
  last_seen_at_ms INTEGER
);
CREATE TABLE library_roots (
  id TEXT PRIMARY KEY,
  volume_id TEXT REFERENCES volumes(id),
  label TEXT NOT NULL,
  path_display TEXT NOT NULL, path_native BLOB NOT NULL,   -- current absolute mount path of the root
  status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active','paused','offline','removed')),
  follow_symlinks INTEGER NOT NULL DEFAULT 0 CHECK (follow_symlinks IN (0,1)),
  include_hidden INTEGER NOT NULL DEFAULT 0 CHECK (include_hidden IN (0,1)),
  created_at_ms INTEGER NOT NULL, updated_at_ms INTEGER NOT NULL,
  last_scan_at_ms INTEGER, last_scan_generation TEXT
);
CREATE TABLE folders (
  id TEXT PRIMARY KEY,
  root_id TEXT NOT NULL REFERENCES library_roots(id) ON DELETE CASCADE,
  parent_id TEXT REFERENCES folders(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  rel_path_display TEXT NOT NULL, rel_path_native BLOB NOT NULL, path_key BLOB NOT NULL,
  status TEXT NOT NULL DEFAULT 'present' CHECK (status IN ('present','missing','inaccessible')),
  modified_at_ms INTEGER, last_seen_generation TEXT,
  UNIQUE (root_id, path_key)
);

-- Physical files (what operations, hashes and backups act on)
CREATE TABLE files (
  id TEXT PRIMARY KEY,
  root_id TEXT NOT NULL REFERENCES library_roots(id) ON DELETE CASCADE,
  folder_id TEXT REFERENCES folders(id) ON DELETE SET NULL,
  name TEXT NOT NULL, extension TEXT,
  rel_path_display TEXT NOT NULL, rel_path_native BLOB NOT NULL, path_key BLOB NOT NULL,
  size_bytes INTEGER NOT NULL CHECK (size_bytes >= 0),
  modified_at_ms INTEGER NOT NULL, created_at_ms INTEGER,
  file_identity TEXT,                      -- best-effort volume+inode/file id
  status TEXT NOT NULL DEFAULT 'present'
    CHECK (status IN ('present','missing','inaccessible','unsupported','error')),
  scan_generation TEXT, last_verified_at_ms INTEGER,
  created_in_catalog_at_ms INTEGER NOT NULL, updated_at_ms INTEGER NOT NULL,
  UNIQUE (root_id, path_key)
);
CREATE INDEX idx_files_size ON files(size_bytes);
CREATE INDEX idx_files_identity ON files(file_identity);

-- Logical items (what the user browses)
CREATE TABLE assets (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL DEFAULT 'image' CHECK (kind IN ('image','video')),
  primary_file_id TEXT NOT NULL REFERENCES files(id),
  format TEXT,                              -- jpeg, png, heic, dng, ...
  width INTEGER, height INTEGER, orientation INTEGER,
  date_taken_ms INTEGER, date_taken_source TEXT, date_taken_raw TEXT, tz_offset_min INTEGER,
  camera_make TEXT, camera_model TEXT, lens_model TEXT,
  iso INTEGER, aperture REAL, shutter_speed TEXT, focal_length REAL,
  latitude REAL, longitude REAL,
  is_favorite INTEGER NOT NULL DEFAULT 0 CHECK (is_favorite IN (0,1)),
  status TEXT NOT NULL DEFAULT 'present'
    CHECK (status IN ('present','missing','unsupported','error')),
  created_in_catalog_at_ms INTEGER NOT NULL, updated_at_ms INTEGER NOT NULL
);
CREATE INDEX idx_assets_date ON assets(date_taken_ms);
CREATE TABLE asset_files (
  asset_id TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
  file_id TEXT NOT NULL REFERENCES files(id) ON DELETE CASCADE,
  role TEXT NOT NULL DEFAULT 'primary',     -- v1 uses only 'primary'; later: raw, sidecar, live_video, edited
  PRIMARY KEY (asset_id, file_id)
);
CREATE TABLE asset_metadata (
  asset_id TEXT PRIMARY KEY REFERENCES assets(id) ON DELETE CASCADE,
  exif_json TEXT, iptc_json TEXT, xmp_json TEXT,
  metadata_version INTEGER NOT NULL DEFAULT 1,
  extracted_at_ms INTEGER NOT NULL,
  extraction_status TEXT NOT NULL DEFAULT 'success'
    CHECK (extraction_status IN ('success','partial','failed','unsupported')),
  error_message TEXT
);

-- Disposable generated outputs
CREATE TABLE derivatives (
  id TEXT PRIMARY KEY,
  asset_id TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
  kind TEXT NOT NULL,                       -- thumb_grid | preview | raw_preview | (later) video_poster | face_crop
  cache_relative_path TEXT NOT NULL,
  width INTEGER NOT NULL, height INTEGER NOT NULL,
  source_fingerprint TEXT NOT NULL, processor_version INTEGER NOT NULL,
  generated_at_ms INTEGER NOT NULL,
  UNIQUE (asset_id, kind)
);

-- Hashes, signatures, similarity
CREATE TABLE file_hashes (
  file_id TEXT NOT NULL REFERENCES files(id) ON DELETE CASCADE,
  algorithm TEXT NOT NULL, algorithm_version INTEGER NOT NULL DEFAULT 1,
  hash_value BLOB NOT NULL, source_fingerprint TEXT NOT NULL, computed_at_ms INTEGER NOT NULL,
  PRIMARY KEY (file_id, algorithm, algorithm_version)
);
CREATE INDEX idx_file_hashes_value ON file_hashes(algorithm, algorithm_version, hash_value);
CREATE TABLE asset_signatures (            -- perceptual hashes
  asset_id TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
  algorithm TEXT NOT NULL, algorithm_version INTEGER NOT NULL,
  value BLOB NOT NULL, source_fingerprint TEXT NOT NULL, computed_at_ms INTEGER NOT NULL,
  PRIMARY KEY (asset_id, algorithm, algorithm_version)
);
CREATE TABLE similarity_edges (            -- pair-level evidence; similarity is NOT transitive
  id TEXT PRIMARY KEY,
  asset_a_id TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
  asset_b_id TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
  source TEXT NOT NULL,                     -- phash | embedding | ...
  algorithm_version INTEGER NOT NULL,
  distance INTEGER, score REAL,
  created_at_ms INTEGER NOT NULL,
  CHECK (asset_a_id < asset_b_id),
  UNIQUE (asset_a_id, asset_b_id, source, algorithm_version)
);
CREATE TABLE review_groups (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL CHECK (kind IN ('exact','visual')),
  basis TEXT NOT NULL,                      -- algorithm + version + threshold used to build it
  status TEXT NOT NULL DEFAULT 'needs_review' CHECK (status IN ('needs_review','reviewed','dismissed')),
  created_at_ms INTEGER NOT NULL, updated_at_ms INTEGER NOT NULL
);
CREATE TABLE review_group_members (
  group_id TEXT NOT NULL REFERENCES review_groups(id) ON DELETE CASCADE,
  file_id TEXT NOT NULL REFERENCES files(id) ON DELETE CASCADE,   -- what operations act on
  asset_id TEXT REFERENCES assets(id) ON DELETE CASCADE,          -- what the UI shows
  distance INTEGER,
  decision TEXT NOT NULL DEFAULT 'undecided' CHECK (decision IN ('undecided','keep','remove')),
  PRIMARY KEY (group_id, file_id)
);

-- Organisation
CREATE TABLE tags (id TEXT PRIMARY KEY, name TEXT NOT NULL COLLATE NOCASE UNIQUE,
  source TEXT NOT NULL DEFAULT 'user' CHECK (source IN ('user','ai','imported')), created_at_ms INTEGER NOT NULL);
CREATE TABLE asset_tags (asset_id TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
  tag_id TEXT NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
  source TEXT NOT NULL DEFAULT 'user', confidence REAL, created_at_ms INTEGER NOT NULL,
  PRIMARY KEY (asset_id, tag_id, source));
CREATE TABLE albums (id TEXT PRIMARY KEY, name TEXT NOT NULL, description TEXT,
  kind TEXT NOT NULL DEFAULT 'manual' CHECK (kind IN ('manual','smart')), smart_rule_json TEXT,
  created_at_ms INTEGER NOT NULL, updated_at_ms INTEGER NOT NULL);
CREATE TABLE album_assets (album_id TEXT NOT NULL REFERENCES albums(id) ON DELETE CASCADE,
  asset_id TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
  position INTEGER NOT NULL DEFAULT 0, added_at_ms INTEGER NOT NULL, PRIMARY KEY (album_id, asset_id));

-- Work and safety
CREATE TABLE jobs (
  id TEXT PRIMARY KEY, job_type TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('queued','running','cancelling','completed','cancelled','failed','paused')),
  priority INTEGER NOT NULL DEFAULT 50, payload_json TEXT NOT NULL,
  progress_done INTEGER NOT NULL DEFAULT 0, progress_total INTEGER, current_item TEXT,
  warning_count INTEGER NOT NULL DEFAULT 0, error_count INTEGER NOT NULL DEFAULT 0,
  created_at_ms INTEGER NOT NULL, started_at_ms INTEGER, updated_at_ms INTEGER NOT NULL, finished_at_ms INTEGER,
  error_code TEXT, error_message TEXT
);
CREATE TABLE file_operations (
  id TEXT PRIMARY KEY,
  operation_type TEXT NOT NULL,             -- move | copy | rename | trash | restore (later: import | backup)
  status TEXT NOT NULL CHECK (status IN ('planned','running','completed','partially_completed',
    'failed','cancelled','undoing','undone','needs_recovery')),
  plan_json TEXT NOT NULL, result_json TEXT, confirmation_nonce_hash BLOB, expires_at_ms INTEGER,
  error_code TEXT, error_message TEXT,
  created_at_ms INTEGER NOT NULL, started_at_ms INTEGER, finished_at_ms INTEGER,
  undo_available INTEGER NOT NULL DEFAULT 0 CHECK (undo_available IN (0,1))
);
CREATE TABLE file_operation_items (
  id TEXT PRIMARY KEY,
  operation_id TEXT NOT NULL REFERENCES file_operations(id) ON DELETE CASCADE,
  file_id TEXT REFERENCES files(id) ON DELETE SET NULL,
  source_path_display TEXT NOT NULL, source_path_native BLOB NOT NULL,
  destination_path_display TEXT, destination_path_native BLOB,
  status TEXT NOT NULL CHECK (status IN ('pending','staged','copied','verified','source_removed',
    'completed','failed','skipped','restored')),
  source_fingerprint TEXT, destination_fingerprint TEXT, trash_reference TEXT,
  error_code TEXT, error_message TEXT
);

-- AI
CREATE TABLE ai_analysis (
  id TEXT PRIMARY KEY, asset_id TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
  provider TEXT NOT NULL, model_id TEXT NOT NULL, prompt_version INTEGER NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('queued','running','completed','failed','skipped')),
  result_json TEXT, consent_scope TEXT NOT NULL, analyzed_at_ms INTEGER, error_code TEXT, error_message TEXT,
  UNIQUE (asset_id, provider, model_id, prompt_version)
);
CREATE TABLE asset_embeddings (
  asset_id TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
  model_id TEXT NOT NULL,                   -- e.g. 'embeddinggemma-2'
  model_revision TEXT NOT NULL,             -- weights + quantisation + vision token budget + preprocessing version, pinned by checksum
  modality TEXT NOT NULL CHECK (modality IN ('image','text')),
  source TEXT NOT NULL,                     -- 'preview-thumbnail' | 'caption' | 'filename'
  dim INTEGER NOT NULL,                     -- 128 | 256 | 512 | 768
  vector BLOB NOT NULL,                     -- little-endian f32, L2-normalised after truncation
  source_fingerprint TEXT NOT NULL, created_at_ms INTEGER NOT NULL,
  PRIMARY KEY (asset_id, model_id, model_revision, modality, source, dim)
);

CREATE TABLE app_settings (key TEXT PRIMARY KEY, value_json TEXT NOT NULL, updated_at_ms INTEGER NOT NULL);

CREATE VIRTUAL TABLE assets_fts USING fts5(
  asset_id UNINDEXED, filename, folder_path, caption, tags, camera_model,
  tokenize='unicode61 remove_diacritics 2');
```

### 5.3 Notes

- **Why files and assets are separate:** exact-duplicate review, move/copy/trash, backup and import act on files; browsing, search, embeddings and faces act on assets. v1 creates exactly one asset per image file with one `primary` asset_file row. Pairing and sidecars (§13) then need no schema rework.
- **Fingerprint** = size + modified time + file identity. A changed fingerprint invalidates hashes, derivatives, signatures and embeddings for that file. A full BLAKE3 hash is the only equality proof.
- `scan_generation` ensures nothing is marked missing after an interrupted scan (§6).
- FTS5 is text search only. Keep it in sync in repository code or explicit triggers.
- Vector search starts as an in-memory brute-force cosine scan over normalised vectors. At 100k photos, 512-d f32 is about 205 MB and 256-d about 102 MB. Adopt `sqlite-vec` or an ANN index only if a benchmark shows a need; loading a SQLite extension through SQLx needs extra proof.
- Backup of the catalogue itself uses SQLite's online backup API or a checkpointed copy, never a bare copy of a WAL database.

---

## 6. Library, scanning and indexing

### 6.1 Roots and volumes

- Adding a root records its volume (label, filesystem UUID/type where the OS exposes them, case-sensitivity probed by test file in the app data dir, not in the root). Roots are canonicalised; **overlapping roots are rejected** (one inside another).
- A root whose path is unreachable becomes `offline`, never `removed`. Its files become `inaccessible`, not `missing`, so a disconnected drive never produces a mass-deletion illusion. When the volume returns (matched by filesystem UUID, else by user confirmation), update `path_native` once; all file rows stay valid.
- Removing a root deletes catalogue rows only, after a count-and-confirm prompt. Never touches files.
- Network volumes: supported read-only for browsing and scanning, with a warning; no watcher guarantee; the DB never lives there.

### 6.2 Scan algorithm (idempotent, resumable)

1. Create a `scan_generation` (UUID) and a `jobs` row. Persist it before touching anything.
2. Walk the root with `walkdir`. Symlinks are not followed unless the root opts in; with follow on, detect cycles by file identity. Skip hidden entries unless enabled and always skip OS metadata (`.DS_Store`, `Thumbs.db`, `@eaDir`, `.Trashes`, `System Volume Information`, `$RECYCLE.BIN`).
3. For each entry, classify by extension then by magic bytes where cheap. Non-media files are ignored, not recorded. Unsupported media (known extension, no decoder) is recorded with `status='unsupported'` so the user can see it.
4. Compare fingerprint (size + mtime + file identity) to the existing row. Unchanged: stamp `scan_generation`, nothing else. New or changed: upsert the `files` row, create or refresh its `assets` row, invalidate dependent derivatives/hashes/signatures/embeddings for that file, enqueue metadata and thumbnail jobs.
5. Write in bounded batches (about 500 rows per transaction) so UI reads never wait long. Persist progress after every batch.
6. **Only after the walk completes successfully**, mark rows of this root whose `scan_generation` differs as `missing`. A cancelled, crashed or errored walk never marks anything missing.
7. Detect moves: a `missing` file whose identity or (size + BLAKE3) matches a new file in the same scan is treated as a move: keep the file id, tags, albums and favorites. Without a hash match it is a new file.
8. Unreadable entries produce a warning with a stable code and do not abort the scan.

### 6.3 Watching

`notify` with debounce yields change **hints** that trigger a targeted rescan of the affected folder. A periodic and on-focus reconciliation scan is the safety net; watchers on macOS and Windows both drop or coalesce events. The app's own operations suppress their own events by journal id.

### 6.4 Paths and names

- Store native path bytes; compare using `path_key` built from the volume policy (§5.1).
- Windows: handle `\\?\` long paths, reserved names (`CON`, `NUL`, ...), trailing dots/spaces, 260-character limits, and case-insensitive collisions during plans.
- macOS: NFC/NFD differences must not create phantom duplicates or false "missing" files.
- Display strings are for UI only; operations always use native bytes.

### 6.5 Supported formats

| Class | v1 | Notes |
|---|---|---|
| JPEG, PNG, WebP, TIFF, BMP | Yes | Via libvips |
| GIF | Read on both OSes; write only on Windows | D20 |
| HEIC/HEIF, AVIF | Yes, through our own pinned libvips build on both OSes (§3.3) | Verified in CI on synthetic fixtures with a clean environment on both OSes; real-iPhone-file checks are the owner's (Phase 0 exit). Patent and licence notes in `docs/native-dependencies.md` |
| RAW (DNG, CR2/CR3, NEF, ARW, RAF, ORF, RW2…) | Tier 1 (D15): indexed through ExifTool metadata and shown from the camera's embedded JPEG preview; no RAW decoder | §13.1. Preview size varies by camera; a phone DNG's preview may be small |
| Video (MP4/MOV/…) | Recognised, not indexed in v1 | §13.5 |

---

## 7. Image pipeline and metadata

### 7.1 Derivatives

- `thumb_grid`: 256 px long edge, WebP or JPEG quality ~80 (choose by benchmark). `preview`: up to ~2048 px for the viewer; the full original is only decoded when the user zooms to 100%. `raw_preview` (Tier 1): the embedded JPEG extracted by ExifTool, then thumbnailed and previewed like any other source.
- Always apply EXIF orientation when generating derivatives (for HEIC, libvips `heifload` applies the container rotation on load; the `vipsthumbnail` path gives upright output with no double rotation: verified on the synthetic fixture on Windows, on a real landscape iPhone photo on macOS; a real **portrait** HEIC and the orientation of RAW previews are still to be checked, STATUS risks 29 and 39). Convert to sRGB; never trust an embedded profile blindly. Strip metadata from derivatives.
- Writes are atomic: temp file in the cache dir, fsync, rename. A derivative row is inserted after the file exists. Cache keys derive from `source_fingerprint` + `processor_version`; a mismatch regenerates.
- Cache size cap with LRU eviction (default 5 GB, user setting). Eviction never touches originals.
- Decoder safety: pixel-count and memory caps (default 200 MP), per-file timeout, panic isolation per job, a corrupt file marks the asset `error` and moves on. Untrusted loaders (SVG, ImageMagick) are blocked at runtime (`VIPS_BLOCK_UNTRUSTED`).
- Priority: visible-first. The grid reports visibility; jobs for visible items jump the queue, and cancel when scrolled away.

### 7.2 Serving images to the WebView

Use a Tauri custom URI protocol (for example `asset-thumb://<asset_id>/<kind>`) that resolves ids to cache paths in Rust. **No image bytes over IPC commands. No raw filesystem paths exposed to the WebView. No `fs` plugin or wide `asset:` scope.** Set a strict CSP allowing only that scheme for `img-src`. Verified in TASK-0A on both OSes; WebView2 and WKWebView treat custom protocols slightly differently (URL shape), which the spike recorded. The 0A handler is spike-only: Phase 2 replaces it with id lookup in the app cache dir through an async handler.

### 7.3 Metadata

- ExifTool batch JSON, read-only, `-n` numeric output plus a separate human pass if needed, minor-warning tolerance (`-m`), with per-call size/time caps, and a restart policy. Never interpolate paths into a shell; pass an arguments file or arg array, with `-charset filename=<native>` handling per OS (verified on Windows non-ASCII paths in TASK-0A). A kill-on-timeout test exists on macOS; the Windows variant is added with the metadata work.
- Normalise into `assets` columns (dimensions, orientation, date taken with its source, camera, lens, exposure, GPS). Keep full tag dumps in `asset_metadata` with size caps (truncate and flag over 256 KB).
- Date taken priority: `DateTimeOriginal` → `CreateDate` → `ModifyDate` → filename pattern → file mtime. Record which one won in `date_taken_source`. Keep the offset if present (`OffsetTimeOriginal`).
- GPS is stored (lat/lon as REAL, altitude in `asset_metadata`) from v1 even though the map is later (§13.2). It is treated as sensitive: never sent to a cloud provider by default (§9.3).
- Metadata extraction failure yields `partial/failed` status and never blocks thumbnailing or browsing.

---

## 8. Duplicate detection

### 8.1 Exact duplicates [MVP]

1. Group candidates by `size_bytes` (a unique size cannot be a duplicate).
2. Stream BLAKE3 over each candidate file, bounded concurrency, I/O-friendly ordering, cancellable, resumable (hash rows are keyed by file fingerprint).
3. Group by hash. Equality = identical hash **and** equal size. For small groups with paranoid mode on, byte-compare.
4. Exact groups are `review_groups(kind='exact')`. Offer a *suggested keeper* by explainable rules (shortest path depth, oldest, in a preferred root, largest resolution) and always show the reason. Never auto-select removal without the user choosing a rule and seeing the dry-run.

Hardlinks and clones: files sharing a file identity are the same data; show them as "same file" rather than as removable duplicates (removing one saves nothing).

### 8.2 Visual duplicates [MVP]

- **Signature:** a versioned perceptual hash (start with a 64-bit DCT pHash; consider dHash/wHash as extra signals) computed from the orientation-corrected **preview/thumbnail**, never the original, so it is cheap and consistent. Store `algorithm_version`; a version bump invalidates and recomputes.
- **Candidates:** BK-tree or multi-index banding over the 64 bits (split into 4×16 bits; pair candidates share one exact band) so the search is sub-quadratic. **Never compare all pairs.**
- **Evidence:** a candidate pair gets a `similarity_edges` row with Hamming distance. A second cheap check (aspect ratio compatibility, optional low-resolution structural similarity) reduces false positives.
- **Grouping is not transitive.** A~B and B~C do not imply A~C. Build review groups as cliques/clusters around a seed with a stated maximum member distance (for example complete-linkage or "all members within T of the seed"), record the `basis`, and show per-member distances. No naive connected components over the edge graph.
- **Thresholds** come from a labelled fixture set (§16.3), not from guesses. Defaults: strict (distance ≤ 4), balanced (≤ 8), loose (≤ 12); the UI offers the three labelled presets.
- Rotation, crop, heavy edits and screenshots/memes are known weak cases for pHash; the semantic layer (§9.1) is the "find more like this" complement, not a replacement. TASK-0B measured that embedding distractors score close to true duplicates, which is why pHash and BLAKE3 decide duplicates and embeddings only advise.
- Burst shots, RAW+JPEG and edited versions are **similar, not duplicates**; the UI says "similar" and offers grouping/stacking, never "delete".

### 8.3 Review UI behaviour

Side-by-side and overlay compare, synchronised zoom, metadata diff (resolution, size, date, camera, path), keyboard flow (arrow = next, K = keep, X = mark remove, Enter = apply plan), per-group and bulk actions that **only build a plan** (§10). Decisions persist in `review_group_members.decision` so review can be interrupted.

---

## 9. AI

AI is advisory, opt-in, isolated behind traits, and never touches files (AGENTS.md invariants).

### 9.1 Local embeddings (EmbeddingGemma 2, Apache 2.0)

**What the model gives us (from Google's model card, developer guide and tech report):** a multimodal embedder (text, image; also video/audio, unused in v1) producing vectors in a **single shared 768-d space**, with Matryoshka truncation to 512/256/128. Because the space is shared, a text query can retrieve images directly, and image-to-image similarity works with the same vectors. Modular loading exists: text-only (~270M params), text+image (~440M), full (~740M); adding an encoder later does not invalidate existing vectors. 8,192-token context.

**v1 use:** (a) semantic search ("beach at sunset") by embedding the text query and ranking image vectors; (b) "find similar" from an image vector; (c) optional text embeddings of captions/filenames for hybrid ranking with FTS5. It is advisory ranking; it never decides duplicates.

**Cost and consequence (TASK-0B, Apple M4, CPU):** about 1 s per image at budget 140 means about 27 hours for 100,000 photos on one stream; the 8 GB Windows laptop is slower and not yet measured. Therefore **embedding is opt-in per library root, resumable, pausable, and shows an ETA** (measured throughput × remaining items). The app never starts embedding a library on its own, and semantic search over a root works only for the part already embedded (the UI says how much).

**Rules (checked against the model card and measured by TASK-0B; parity in Rust to be proven by TASK-0B2):**

1. **Dimension:** default stored dimension **256** (card/report: about 95% of full-dimension image retrieval quality at one third of the storage; the 0B fixture set saturates at 100% for every real configuration and cannot discriminate dimensions, so the default rests on the model card). Allow 512 or 768 as a setting; 128 only for coarse text shortlisting, never for image search. Truncate the first *d* components then **re-normalise (L2)**. Query and corpus must use the same *d*; never mix dims in one search. The stored `dim` column enforces this.
2. **Prefixes apply to text only.** Queries: `task: search result | query: <text>`. Documents: `title: <title or none> | text: <text>`. Images take **no** prefix.
3. **Precision:** run in fp32 or bf16. The model card warns against fp16 for activations. The recommended artifact is the Q8 ONNX export (cosine to the fp32 reference: text 0.99997, images 0.9982 at budget 280; Q4 is visibly worse). fp16 and int4 are not used.
4. **Vision token budget: default 140** (2.5× faster than 280, no loss on the 0B fixtures); 280 is an optional quality setting. **A different budget is a different embedding** (mean cosine 0.9872, minimum 0.9308 between 140 and 280): budgets are never mixed in one index, and the budget is part of `model_revision`. Image input is the orientation-corrected preview derivative used elsewhere. Preprocessing (resize, normalisation, patching, position ids, tokenisation) must match the reference implementation exactly; a mismatch silently degrades quality (wrong normalisation measured: image cosine 0.75, retrieval 45%), so parity against the Python reference is a gate (TASK-0B2).
5. **Cosine only:** with normalised vectors, cosine = dot product. Unit tests cover normalisation, truncation and ordering.
6. **Versioning:** each vector row carries `model_id`, `model_revision` (weights + quantisation + budget + preprocessing version) and `dim`. A revision change marks vectors stale and queues re-embedding in the background; old rows remain usable until replaced.
7. **Memory:** text-only mode is the default (about 200 MB class on-device per the report); load the vision encoder on demand when embedding images or running image-based queries (+~115 MB measured in Python), unload after an idle timeout. Clean-process memory in Rust is measured by TASK-0B2. Embedding jobs are lowest priority, battery/power aware where the OS allows, and pause on user activity.
8. **License:** Apache 2.0. Neither model repository ships a LICENSE file (the licence is declared in the README front matter), so the app ships the Apache 2.0 text plus a NOTICE naming the model, the converted-weights repository and its pinned commit; link Google's usage notice in the app's licences screen. Redistributing converted weights stays the owner's decision (D5/D6); default is download-on-consent.

**Runtime (decided by TASK-0B, accepted 2026-11-05):** R2, ONNX Runtime in-process through `ort` (pinned exactly; it is a release candidate), with the `onnx-community` Q8 export: about 485 MiB for the text graph plus the vision encoder, no audio encoder. CPU execution provider: CoreML failed on the zero-length video/audio inputs of the backbone; CoreML on the vision encoder alone and DirectML on Windows are measured by TASK-0B2. The ONNX Runtime library is bundled inside the signed app, never downloaded with the weights (hardened-runtime macOS apps cannot load unsigned libraries fetched later); it is a native dependency under §3.4. Alternatives, kept for the record:

| ID | Option | State |
|---|---|---|
| R1 | llama.cpp (GGUF) sidecar | Rejected on evidence: mainline `llama-server` returns text-only vectors for this model (upstream issue 30082, closed "not planned"); not run by us |
| R2 | ONNX Runtime via `ort` | **Chosen** (TASK-0B); Rust tokenisation and preprocessing parity **[SPIKE]** until TASK-0B2 |
| R3 | User-supplied OpenAI-compatible embedding endpoint (Ollama, LM Studio, MLX) | Optional later setting; not offline-by-default |
| R4 | Python `sentence-transformers` | The **reference** for parity tests and fixture vectors; never shipped |

The app depends only on the `EmbeddingProvider` trait:

```rust
trait EmbeddingProvider {
    fn info(&self) -> ModelInfo;                         // id, revision, supported dims, modalities
    fn embed_text(&self, kind: TextKind, items: &[String], dim: u16) -> Result<Vec<Vec<f32>>>;
    fn embed_images(&self, previews: &[ImageInput], dim: u16) -> Result<Vec<Vec<f32>>>;
}
```

If the Rust path fails TASK-0B2 on either OS, semantic search is deferred; the rest of the product is unaffected (that is the point of the seam). The decision and evidence go to `docs/embedding-runtime.md`.

### 9.2 Cloud and LLM providers (Phase 6, may follow Phase 7)

Cloud AI is not a priority. **First version: bring-your-own-key plus a local OpenAI-compatible endpoint** (Ollama and similar), not a set of vendor-specific adapters. An `AiProvider` trait keeps vendor adapters (Gemini, Claude, OpenAI) possible later. Capabilities per provider are declared (vision input, structured output, max image size) and never assumed. Model IDs are user-editable settings and are verified against the provider's current docs at implementation time; never hard-coded from memory. Phase 6 may be scheduled after Phase 7 at the owner's call.

**Uses:** captions and tags, natural-language query → structured filter JSON, and duplicate-review explanations. All outputs are validated against a Rust schema, stored in `ai_analysis` with provider/model/prompt version, shown as suggestions, and applied only by the user.

### 9.3 Consent and privacy

- First-use consent **per provider**, naming exactly what is sent (downscaled preview, never the original unless the user opts in).
- Strip metadata before upload (EXIF, GPS, camera serials). **GPS and file paths are never in prompts.** Filenames are not sent unless the user enables it.
- No upload during scanning or indexing. Cloud analysis runs only on an explicit user action or an explicitly enabled batch with a visible cost estimate and a hard budget cap.
- Keys live only in the OS credential store; never logged; never in settings, DB or the bundle.
- Provider errors, rate limits and timeouts are isolated per provider with retry/backoff; one failing provider never blocks the app.
- Treat every model output as untrusted input: schema-validate, length-cap, escape for display; never execute or interpret it as a command or path.
- A "local only" master switch disables all outbound AI calls.

### 9.4 AI never does

Delete, move, rename or overwrite files; decide exact duplicates; auto-apply tags without review in bulk; read outside the preview it was handed.

---

## 10. File operations (the safety core)

### 10.1 Operations

Move, copy, rename, create folder, move to Trash, restore from Trash where the OS allows, and bulk reorganise by rule (for example `YYYY/MM` from date taken). Later: import (§13.3) and backup (§13.4) reuse this engine.

### 10.2 Invariants

1. Every mutation goes through `OperationManager`. No other module calls `remove_*`, `rename` or `copy` on user files.
2. **Plan → preview → confirm → journal → execute.** A plan is computed without touching disk and stored (`planned`). It lists every source, destination, conflict and the byte total. Confirmation uses a one-time nonce bound to the plan hash and expiring in minutes; the UI cannot execute an unseen or modified plan.
3. **Revalidate at execution:** each source's fingerprint must equal the planned one; otherwise the item is skipped with `stale_source`.
4. **No silent overwrite.** Conflicts are resolved in the plan by explicit policy (skip, keep both with a deterministic suffix, or user-chosen replace, which still sends the replaced file to Trash).
5. **Same volume:** rename is atomic and used directly. **Cross volume:** copy to a temp name in the destination folder → fsync → verify (size and BLAKE3) → rename into place → only then remove the source (to Trash). Source removal never happens before verification; a crash leaves the source intact.
6. **Delete means OS Trash/Recycle Bin only.** No permanent deletion in v1 (an explicit "empty app-managed holding area" is not offered either). If Trash is unavailable for a volume (some network/external drives), the operation refuses and explains.
7. **Journal first:** each item's state (`pending → staged → copied → verified → source_removed → completed`) is persisted before and after the step. On startup, any `running` operation becomes `needs_recovery` and the UI offers resume, roll back or leave as is, using the recorded states.
8. **Undo** is available for moves/renames/copies (reverse plan) and for Trash restore when the OS reports the item; it revalidates like any operation and never promises more than it can do.
9. Destination containment: destinations must be inside a registered root, canonicalised, with `..` and symlink escape rejected. Source and destination may not be the same path. Moving a folder into itself is rejected.
10. Partial success is a first-class state with a per-item report; the UI never says "done" when items failed.
11. File operations update the catalogue transactionally after each verified item, preserve file ids (so tags, albums and favorites survive a move), and suppress their own watcher events.
12. Disk-space and permission pre-checks in the plan; read-only media and locked files are reported, not retried blindly.

### 10.3 Duplicate cleanup

A duplicate action only builds a plan of `trash` items for the files marked `remove`, with the same preview and journal. It refuses to build a plan that would trash every member of a group, and it never trashes the last copy of anything it hashed.

### 10.4 Windows specifics

Locked files (antivirus, indexers) retry with bounded backoff and then report; handle sharing violations; verify Recycle Bin behaviour for files above the bin size limit and on removable drives; long paths via `\\?\`. These are exactly the cases the Windows laptop is used to validate manually (§17.2).

---

## 11. Jobs and concurrency

- A `JobManager` with typed jobs (`scan`, `metadata`, `thumbnail`, `hash`, `phash`, `embed`, `ai_analyze`, `cache_evict`), priorities, per-type concurrency limits (I/O-bound vs CPU-bound), and `CancellationToken` for every job.
- Defaults: scanning 1 walker per root; hashing bounded by disk (1–2 concurrent on spinning disks, 4 on SSD); thumbnail and pHash by CPU cores minus one; embeddings 1 batch at a time; cloud AI per-provider limits.
- Job state is persisted (`jobs` table). On restart, interrupted jobs resume from their last persisted checkpoint or are re-queued; every job is idempotent. `embed` jobs are per root, opt-in, pausable, and report an ETA.
- Progress is pushed via Tauri **Channels** (throttled to a few updates per second); the UI never polls in a tight loop.
- A global "pause background work" switch and automatic throttling while the user is interacting with the grid.
- No unbounded queues; no unbounded in-memory collections of file records; stream and batch.
- Panics in a worker are caught, recorded as a job failure with an error code, and do not take down the app (so release builds must not use `panic = "abort"`).

---

## 12. IPC, commands and security

### 12.1 Command rules

- Commands are thin: validate (Zod on the frontend, typed DTO + checks in Rust), authorise, delegate to a service. Business logic lives in services.
- **Every list command is paginated with a hard maximum page size** (default 200, max 1000). Queries use keyset pagination for large sets. No command returns "all photos".
- Errors cross the boundary as `{ code, message, details? }` with stable codes; no raw `anyhow` strings or paths with user data in logs by default.
- IDs, not paths, identify items across the boundary. The only commands that accept a path are "add root" (via the native folder dialog result), "choose destination" (same), and import/backup source selection later.
- Long operations return a job or operation id immediately; progress arrives through Channels.
- Hand-written DTOs + Zod is the baseline. Generating types from Rust (D4) may replace it at the start of Phase 1.

### 12.2 Command groups (illustrative; exact set evolves per phase)

`library.*` (roots, scan, status) · `catalog.*` (list/query assets, folders, detail) · `search.*` · `duplicates.*` · `albums.*`/`tags.*` · `operations.*` (plan, preview, confirm, status, undo, recover) · `jobs.*` · `ai.*` (providers, consent, analyse) · `embeddings.*` · `settings.*`.

### 12.3 Tauri security

- Capabilities and permissions are least-privilege and listed in `src-tauri/capabilities/`. No `shell`, `fs` or `sql` plugin access from the WebView. The dialog plugin is allowed for folder selection only.
- Strict CSP; no remote scripts; no `eval`; the app loads only bundled assets. External links open in the default browser via a vetted allowlisted command.
- Sidecars (ExifTool and later) are launched from Rust at fixed resource paths with argument arrays and timeouts; never through a shell.
- Dependency hygiene: `cargo audit`/`cargo deny` and `pnpm audit` in CI; new dependencies are listed with exact version, source and licence in each task report. A documented advisory ignore needs a reason, a date and a re-check condition.
- Logging redacts paths by default (hash or relativise), never logs keys or prompts that contain user data, and has size-rotated files.

### 12.4 Data protection

The catalogue contains paths, GPS and AI text and is sensitive. It stays local; "export diagnostics" strips paths and metadata; there is no telemetry in v1.

---

## 13. Extension seams for later versions [LATER]

Each item states **what is built in v1 to make it possible** (cheap, already in this spec) and **what the later version adds**. Nothing in this section is implemented before the owner schedules it, except RAW Tier 1 (§13.1, D15); v1 must not contain stubs for the rest.

### 13.1 RAW: previews (Tier 1), full decode (Tier 2), RAW+JPEG pairs

- **v1 seams:** `files` vs `assets` vs `asset_files(role)`; RAW extensions recognised; `derivatives.kind` is open; ExifTool is already bundled.
- **Tier 1 (D15, planned for Phase 1 and 2; no new native library):** every RAW format is indexed by its ExifTool metadata and shown from the camera's **embedded JPEG preview** (`PreviewImage` / `JpgFromRaw`), extracted by ExifTool as a `raw_preview` derivative and thumbnailed by libvips, so the same code runs on both OSes. Evidence (2026-11-10): an iPhone 12 Pro ProRAW DNG carries a full-size 4032×3024 JPEG preview (3.8 MB) and ExifTool reads it (use `-m`, it warns about large arrays). Open points: the preview's orientation versus the DNG `Orientation` tag; preview size varies by camera (phone DNGs sometimes embed only a tiny one, and then the asset shows as low-resolution rather than failing).
- **Tier 2 (later, own ADR):** full RAW decode through libvips `dcrawload` (LibRaw), which needs our own pinned build like HEIC (the stock Windows zip has no LibRaw); check LibRaw's licence (LGPL/CDDL) and packaging first. Unproven on ProRAW.
- **Pairs:** pair `IMG_1234.CR3` + `IMG_1234.JPG` into one asset (`asset_files.role = raw | primary`), with a setting for which is "primary" and operations that move both together. XMP sidecars need rules (Phase 4/5). Duplicate logic must not treat a RAW and its JPEG as removable duplicates of each other.

### 13.2 Maps from GPS metadata

- **v1 seams:** `assets.latitude/longitude` extracted and indexed from Phase 3; GPS excluded from cloud prompts; the catalogue can filter by bounding box.
- **Later:** a map view with clustering (MapLibre GL with a tile source chosen by the owner; offline vector tiles optional; **map tiles are a network dependency and a privacy trade-off, so opt-in**). A reverse-geocoding step (offline gazetteer preferred over a web API) as a versioned analyzer writing place names into a `places` table for search ("photos from Lisbon"). Photos without GPS can be placed manually by the user, written to the catalogue only; writing GPS into files is a separate backup-guarded feature.

### 13.3 Import from external drives, cards and phones

- **v1 seams:** `volumes` (kind removable, filesystem UUID, last seen); `OperationManager` already handles cross-volume staged copy with verification; BLAKE3 hashes exist for duplicate checks.
- **Later:** an import wizard: detect a volume, list new media, **dedupe against the catalogue by hash before copying**, copy to a destination root using a configurable folder/name template (`{YYYY}/{MM}`, `{date}_{camera}_{seq}`), verify each file, write an import ledger so the same card is never imported twice, optionally (explicit confirm) move originals of verified files to the card's trash only where the OS supports it. An `import` operation type reuses the journal and verification. Phones (MTP/PTP) are a separate sub-feature; platform support differs sharply (Windows MTP vs macOS Image Capture) and is spiked before promising it.

### 13.4 Photo backup

- **v1 seams:** the `files` table with fingerprints and hashes is already a manifest; operations are journaled; the catalogue itself can be backed up through SQLite's online backup API.
- **Later:** backup *targets* (second drive, NAS path, and later cloud object storage) with a `backup_targets`/`backup_items` ledger recording which file version (by BLAKE3) lives where and when it was last verified. Modes: mirror (one-way, never deletes at the target without a retention rule) and versioned. A health view lists files with fewer than N verified copies. Restore is a first-class operation. A backup feature **must** be conservative: no deletion at the target by default, dry-run first, verification by hash, and clear differentiation from sync. Cloud targets add credential and cost surfaces and are a separate decision.

### 13.5 Video management

- **v1 seams:** `assets.kind = 'video'` exists; videos are recognised by extension and ignored (or listed as unsupported); `derivatives.kind` includes poster frames; the job system supports long-running work.
- **Later:** bundle `ffmpeg`/`ffprobe` as a sidecar behind `VideoProbe`/`VideoDecoder` traits (check licence build flags: LGPL vs GPL; avoid GPL-only encoders), extract duration/codec/resolution/frame rate/creation time, generate poster frames and short hover-preview strips, and play through the WebView's native `<video>` where the codec allows, else offer transcoded proxies in the cache. Duplicate logic extends to video through size+hash first; perceptual video hashing (frame sampling) is a later analyzer. Live Photos and motion photos pair via `asset_files.role = live_video`.

### 13.6 Face recognition

- **v1 seams:** generic versioned `analyzer` pattern (hash, signature, embedding tables all carry algorithm/model version); derivatives can hold face crops; embeddings are stored as normalised vectors with model revision; consent and local-only switch exist.
- **Later:** a local face detector and face embedder (open-licensed models only, verified licence and bias/accuracy notes) behind `FaceDetector`/`FaceEmbedder` traits; new tables `faces(asset_id, bbox, quality, embedding...)`, `people`, `face_clusters`, with **user-confirmed** naming and merging; no cloud face identification, ever, by default. Biometric data is the most sensitive data in the app: it is opt-in per library, local-only, deletable in one action, excluded from diagnostics, and subject to its own consent text and legal review for the user's jurisdiction (some regions regulate biometric processing). The existing image embedder is **not** reused for identity; face embedding is a separate model.

### 13.7 Cross-cutting seams (all in v1)

- Analyzer pattern: any new computation stores `(algorithm, version, source_fingerprint)` and invalidates when the source or algorithm changes.
- Migrations are additive; schema changes require a reviewed migration test from the previous release.
- Settings are namespaced per feature; feature flags exist for anything experimental and default off.
- The job and operation systems take new typed variants without changing existing ones.
- A written ADR is required before any [LATER] feature begins (`docs/adr/`), covering licence, packaging, privacy and platform differences.

---

## 14. User interface

### 14.1 Layout

Left sidebar: Library (all photos, favorites, recents), Folders (tree by root, with offline roots greyed), Albums, Tags, Duplicates, Jobs. Main pane: virtualized grid (justified or fixed square; user choice) with a size slider, a details/list view, and a timeline scrubber by date. Right panel: metadata and actions for the selection. Viewer: full-screen preview with zoom/pan, filmstrip, keyboard navigation, info overlay, "reveal in Finder/Explorer", and "find similar".

### 14.2 Interaction principles

- Everything that changes files shows a **plan preview** before it runs (§10) and a result report after.
- Multi-select with shift/cmd/ctrl; drag-and-drop to folders builds a *move plan*, it does not move instantly.
- Undo/redo surfaced for operations that support it, with honest wording when they do not.
- Keyboard-first: arrow navigation, space for quick look, F favourite, delete = move to Trash (with plan for >1 item), ⌘/Ctrl+F search, ⌘/Ctrl+K command palette.
- Empty, loading, offline-volume, partial-failure and permission-denied states are designed, not afterthoughts. A "volume offline" banner never reads as "photos deleted".
- Accessibility: keyboard reachable, focus visible, ARIA roles on grids/dialogs, respects reduced motion and system theme, contrast AA. Light/dark themes.
- Platform conventions: menu bar and window controls per OS, ⌘ vs Ctrl, native file dialogs, Finder/Explorer reveal.

### 14.3 Search

Filters: name, folder, date range, size, dimensions, orientation, camera/lens, tags, albums, favorite, has GPS, duplicate status. FTS5 for text. Semantic query box (when embeddings are ready for the roots searched) merges semantic and FTS5 results (reciprocal-rank fusion) and shows which signal matched. An optional natural-language query bar (cloud or local LLM) converts text to a structured filter, **shown to the user as editable chips before running**.

### 14.4 Settings

Library roots (with a per-root "embed for semantic search" switch, progress and ETA), cache location/size, thumbnail quality, duplicate presets, AI providers with per-provider consent and cost caps, local-only switch, embeddings model/dim/vision budget/runtime, language (English v1), theme, background-work throttling, diagnostics export.

---

## 15. Performance and resource targets

Targets for the two reference machines; they are verified by benchmarks on **both**, not asserted.

| Machine | Role | Spec |
|---|---|---|
| Mac | Primary dev machine, upper reference | Apple M4 (10-core), 16 GB RAM, macOS |
| Windows laptop | Low-end reference | Acer Swift SF314-43: AMD Ryzen 7 5700U, 8 GB RAM, 496 MB graphics, Windows 11 |

The table's targets are written for the Mac. On the laptop, record the measured number next to the target; a miss is a finding to report and fix or accept explicitly, never a silent exception. Memory is checked on the laptop first (8 GB).

| Area | Target |
|---|---|
| Cold start to usable UI | ≤ 2 s on SSD with 100k assets |
| Grid scroll | 60 fps on 100k assets (virtualized, no layout thrash) |
| First-pass scan (metadata only) | ≥ 300 files/s on SSD (JPEG), progress visible within 1 s |
| Thumbnail generation | ≥ 50/s for typical 12–24 MP JPEG on an 8-core machine (HEIC is slower: measure it) |
| Exact duplicate pass | disk-bound; only candidate sizes hashed; resumable |
| pHash candidate search | sub-quadratic; 100k assets in minutes, not hours |
| Brute-force vector search | ≤ 100 ms for 100k × 256-d on a laptop (otherwise adopt an index) |
| Memory | App ≤ ~500 MB RSS excluding model; embedding model memory reported separately |
| Embedding throughput | Measured, not targeted: about 1 s per image at budget 140 on the Mac (Python); Rust numbers on both machines from TASK-0B2; shown to the user as an ETA |
| Catalogue size | ~5–10 KB/asset including metadata; DB stays well under 1 GB at 100k assets |
| Idle | zero CPU when no jobs are running |

Benchmark harness and synthetic catalogue generator (10k/100k) are built in Phase 2 and reused afterwards. Do not optimise without a measurement.

---

## 16. Testing and quality

### 16.1 Layers

- **Rust unit tests** for path logic, fingerprints, planners, journal state machine, hashing, vector math, threshold logic.
- **Integration tests** on temp directories: scan → rescan → rename/move/delete outside the app; interrupted-scan and interrupted-operation recovery; cross-volume copy (simulated by separate temp mounts where possible); migration from every previous schema version.
- **Failure injection** at each operation boundary (after stage, after copy, after verify, before source removal, mid-journal write) with assertions that no user data is lost.
- **Frontend**: component tests (Vitest + Testing Library); Playwright against the dev build for key flows (platform differences caveat: the packaged WebView differs from Playwright's Chromium).
- **Property tests** (`proptest`) for path normalisation and plan generation (no destination escapes, no collisions).
- **Fixtures:** a committed small image set (original, resized, recompressed, rotated, cropped, screenshots, unrelated lookalikes, corrupt files, EXIF with/without offsets, HEIC, non-ASCII/long/reserved Windows names), generated by script where possible and documented. Never real personal photos. Real iPhone files are used only by the owner, on copies, outside the repository.
- **Security checks:** the grep checks in `docs/REVIEW_CHECKLIST.md`, `cargo deny`, `cargo audit`, `pnpm audit`.
- **Gate scripts** (audits, CI checks) are themselves tested against bad inputs (§3.4, D19).

### 16.2 The `pnpm verify` command

One command that runs: format check, lint, TypeScript type-check, frontend tests, `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`, migration test. Created in TASK-0A and extended per phase. A task is not done unless it passes on the author's machine and in CI.

### 16.3 Calibration

Duplicate thresholds and semantic-search defaults are tuned on labelled fixture sets and their numbers (precision/recall at each preset) are recorded in `docs/calibration.md`. Semantic search quality is judged on a small hand-labelled query set, not by vibes. The 0B fixture set (20 synthetic scenes) saturates and cannot do this job; Phase 4B needs a harder one. Near-duplicate evaluation uses group-level labels (every pair in a group is a positive), not base-to-variant pairs only.

### 16.4 Definition of done (every task)

Acceptance criteria met with evidence; `pnpm verify` green; no new warnings; dependencies justified; docs updated; report written; no scope creep; stopped.

---

## 17. Platforms, build and distribution

### 17.1 Platform matrix

| Platform | v1 status | Notes |
|---|---|---|
| macOS arm64 (Apple Silicon) | Primary dev and release target | Floor macOS 15.0 (our libvips build targets it, D20); the app's own `minos` is to be checked in Phase 7 |
| macOS x64 | Not built or tested; not claimed | Universal builds multiply native-dependency work |
| Windows 10/11 x64 | Release target; validated by CI and the owner's Windows laptop | WebView2 runtime handling decided in TASK-0C |
| Windows arm64, Linux | Not supported in v1 | The code should not preclude them |

**CI cost and hygiene rules (D12, D13):**

- Docs-only commits carry `[skip ci]`; `ci.yml` has `paths-ignore` for `docs/**` (note: the dylib-table check reads `docs/native-dependencies.md`, so a table edit alone is validated only by the next code push).
- `concurrency` cancels superseded runs; the push trigger is `task/**` and `main`.
- The macOS job runs on every push while the repository is public (free minutes). If the repository ever becomes private again, gate it to `main` and manual runs (a full run then costs about $0.32).
- Heavy dependency builds are done once, manually, and stored checksum-pinned in the private deps repo (§3.4); app CI never rebuilds them. Actions artifacts of public runs are downloadable by any signed-in GitHub user: bundles and installers that contain libde265 keep short retention (3 days), and libvips zips are deleted early.
- Dependabot pull requests get no Actions secrets and therefore fail closed on the deps download; treat as a known limit.

### 17.2 Cross-platform validation process (the owner develops on a Mac; the Windows laptop is always available)

- **CI is the first line** on both OSes (TASK-0C) but is not proof of behaviour with real folders, real drives, real antivirus, real iPhone files and real WebView2.
- **Manual Windows pass, per phase gate (and before any release):** on the Windows laptop, `git pull`, `pnpm install`, `pnpm verify`, `pnpm tauri dev`, then the phase's smoke checklist from `docs/ORCHESTRATION.md` §6: a folder with non-ASCII and very long names, an external USB drive and an unplugging scenario, Recycle Bin behaviour, locked files, high-DPI scaling, and a file operation across drives. Record results in the phase report. A phase with a platform-sensitive feature (Phases 1, 2, 5, 7) does not pass its gate on macOS evidence alone.
- **The Windows clean-machine test runs the installer, not the bare exe.** A bare exe does not carry its DLLs (TASK-0C found this the hard way). The test installs the NSIS installer, runs the smoke test from the install folder with a reduced PATH, and checks that the loaded libvips comes from the install folder.
- **The Visual C++ runtime must be accounted for:** the exe links the C runtime statically (`.cargo/config.toml`), CI's `dumpbin /dependents` must show no `VCRUNTIME`/`MSVCP`, and every installed libvips DLL is scanned the same way (they depend only on the Windows Universal CRT and system DLLs). Not scanned: the ExifTool Perl runtime (low risk).
- Antigravity may be used on the Windows laptop for Windows-only fixes, in the same branch/task discipline as on the Mac.
- A dev setup on Windows needs several environment variables (`VIPS_DIR`, `VIPS_LIB_DIR`, `EXIFTOOL_DIR`, `PKG_CONFIG_PATH`, `LIB`, `PATH`); they are documented in the setup guide. Code moves between the machines through GitHub, never through a file-sync tool.

### 17.3 Packaging

- Tauri bundles produce `.dmg`/`.app` on macOS and NSIS/MSI on Windows. Native libraries (libvips and its codecs, ExifTool + its Perl runtime on Windows, ONNX Runtime) are bundled as resources/sidecars and located from the app bundle, never from the machine (§3.4).
- **Release gate:** a clean-machine smoke test on both OSes (no developer tools or package-manager libraries installed) that launches the packaged app, scans a fixture, shows thumbnails, reads metadata, decodes a HEIC fixture, and performs a trash operation.
- **Phase 7 (not before):** macOS Developer ID signing + notarization (needs an Apple Developer account), Windows code signing (certificate cost and SmartScreen reputation), the Tauri updater with signed update manifests, crash-log opt-in, and a third-party licence listing including EmbeddingGemma 2's Apache 2.0 notice. libheif and libde265 are LGPL-3.0-or-later: ship notices and a source offer, and keep them replaceable (dynamic libraries). HEVC patent pools need a legal check before any distribution. If this is a personal-use tool, unsigned builds are acceptable for the owner's own machines; distribution to others requires the signing and legal work.
- Reproducible builds: lockfiles committed, **Rust toolchain pinned to an exact version in `rust-toolchain.toml` and in CI (1.99.1 at the time of writing)**, Node and pnpm versions pinned, CI actions pinned (the floating `dtolnay/rust-toolchain@stable` is replaced in Phase 7 at the latest).

---

## 18. Delivery plan

Each phase has a gate. Gates are real: the next phase does not start until the gate is ticked in `docs/STATUS.md` by an ACCEPT. Only the next task is written in detail. Tasks are small: one coherent change, hours not days.

| Phase | Content | Gate |
|---|---|---|
| **0 Feasibility** | **0A** scaffold, `pnpm verify`, SQLite/SQLx + migrations + pragmas, libvips and ExifTool spikes, custom protocol + CSP spike, minimal capabilities · **0B** embedding runtime spike (§9.1) · **0C** CI matrix and clean-machine packaging · **0D** HEIC investigation · **0E** Windows own libvips with HEVC decode · **0F** macOS own libvips with HEVC decode · **0B2** Rust tokenisation and preprocessing parity (before 4B; does not gate Phase 1) | Native deps proven in a clean release build on both OSes; iPhone HEIC decode proven in packaged builds on both OSes (CI fixtures, plus the owner's real files on each OS); embedding runtime decision recorded; D4 ready to decide |
| **1 Catalogue and scanning** | Volumes/roots/folders/files/assets schema, scanner with generations, jobs, watcher hints, offline-root behaviour; libvips init and error-buffer housekeeping (§3.3); ExifTool call hardening | Two roots scanned; restart mid-scan leaves no wrong "missing"; Windows manual pass |
| **2 Grid and viewer** | Thumbnail/preview pipeline (including HEIC and the RAW Tier 1 embedded preview), protocol serving, virtual grid, folder tree, viewer, synthetic-catalogue benchmark on both machines | 10k/100k synthetic scroll target; originals untouched; Windows manual pass |
| **3 Metadata and search** | ExifTool ingestion, normalised columns, FTS5, filters, favorites/tags/albums | Malformed metadata never crashes a scan; queries are paginated and parameterised |
| **4 Exact and visual duplicates** | BLAKE3 pipeline, pHash, candidate index, review groups and UI, calibration | Labelled-corpus precision/recall recorded; no auto-delete anywhere |
| **4B Local semantic search** | `EmbeddingProvider` from 0B/0B2, opt-in per-root embedding jobs with ETA, vector search, hybrid ranking, "find similar" | Needs 0B2 accepted; thresholds calibrated on a harder labelled set; memory and ETA report on both machines; advisory only |
| **5 Safe file operations** | OperationManager: plan/confirm/journal/execute/undo/recovery, Trash adapters, reorganise-by-rule, duplicate cleanup | Injected-failure tests at every boundary on both OSes; Windows manual pass with locked files and external drive |
| **6 Cloud AI** (may follow Phase 7) | Bring-your-own-key and local OpenAI-compatible endpoint first; consent, budgets, captions/tags, NL query chips; vendor adapters later | No key in bundle or logs; no upload during scan; per-provider isolation |
| **7 Hardening and release** | Performance pass, accessibility pass, packaging, signing (if distributing), licence and legal audit (HEVC, LGPL), docs | Clean-machine smoke test on both OSes; licence audit done |
| **Later (§13)** | Scheduled by the owner one at a time, each preceded by an ADR | per ADR |

Phase ordering note: Phase 5 (file operations) is deliberately after browsing, search and duplicates so the viewing/analysis half of the product is useful before anything can modify files, but the **OperationManager design (§10) is final now**, so earlier phases never invent their own ad-hoc file changes.

---

## 19. Risks and open questions

### 19.1 Risks

1. **Native packaging** (libvips + codecs, ExifTool/Perl, ONNX Runtime) self-contained on two OSes is the largest technical risk. Mitigation: Phase 0 gates, fallback ladder (§3.3), own pinned builds (§3.4).
2. **Embedding runtime maturity:** image embedding through GGUF is unproven in mainline tooling; ONNX preprocessing is undocumented outside Transformers.js. Mitigation: TASK-0B2 parity tests against the Python reference; the seam lets the app ship without semantic search.
3. **Model knowledge cutoff** (March 2026): the coding agent may use stale crate/plugin APIs. Mitigation: verify-before-use rule, checklist greps, small tasks.
4. **Windows is a second-class dev environment** for the owner. Mitigation: CI plus the manual Windows pass at each platform-sensitive gate (§17.2).
5. **Data-loss bugs** in file operations. Mitigation: §10, failure injection, Trash-only, no auto-delete, review of the journal state machine before any execution code.
6. **Metadata and perceptual-hash false positives/negatives.** Mitigation: calibration on fixtures, "similar" vs "duplicate" wording, human review.
7. **Cost/pricing drift** for cloud AI (Gemini 3.8 Flash promotional pricing ends 31 Dec 2026; API model IDs unverified). Mitigation: budgets, editable model IDs, estimates before batches; cloud AI is low priority.
8. **Scope creep toward §13 features.** Mitigation: ADR requirement; "no stubs for later features" rule.
9. **HEVC patents and LGPL obligations.** libde265 and libheif are LGPL-3.0-or-later; HEVC patent pools are separate from the library licences. Fine for personal use; legal check and notices before any distribution (Phase 7). Not a legal opinion.
10. **Own-build maintenance.** Our libvips builds float on base images and package-manager leaves when rebuilt. Mitigation: pinned digests and tarball checksums, the stored release asset as the real artifact, fail-closed CI, an expiring read-only token (renew before expiry, currently 2027-02-07).
11. **Public repository hygiene.** What is public stays public (forks, caches, history). Mitigation: D13 rules, gitleaks pre-flight, synthetic fixtures only, short artifact retention, no personal photos anywhere in the repository.
12. **Gate scripts that fail open.** Mitigation: D19 and regression tests for every gate.
13. **Embedding time on the low-end machine.** About 27 hours per 100,000 photos on the Mac; the 8 GB laptop is slower. Mitigation: opt-in per root, resumable, pausable, ETA (§9.1).

### 19.2 Open questions for the owner (defaults in brackets; the plan proceeds on the defaults)

1. Distribution: personal use only, or release to others? [personal first; design for release]
2. Typical library size and where it lives (internal SSD, external HDD, NAS)? [≤ 100k photos, mostly internal/external SSD]
3. Main file types (JPEG, HEIC from iPhone, RAW)? [JPEG + HEIC; RAW through embedded previews (D15, owner to confirm), full decode later]
4. Sidecars (XMP) and other managers (Apple Photos, Lightroom) in the mix? [ignored in v1, never modified]
5. Hardware for local AI: [answered: Mac M4 16 GB, Windows laptop Ryzen 7 5700U 8 GB, CPU only]
6. Cloud providers and budget? [answered: not a priority; bring-your-own-key and local OpenAI-compatible endpoint first]
7. App name and bundle identifier? [answered: working name "Photos Manager", `com.narayanwaraich.photosmanager`]
8. D4: generated IPC types vs hand-written DTOs? [decide in Phase 1 after 0A]
9. D15: RAW Tier 1 (embedded previews) in Phases 1 and 2? [yes unless the owner objects]

---

## 20. Glossary of identifiers

`asset` logical item · `file` physical file · `volume` disk · `root` indexed directory · `derivative` generated image · `analyzer` versioned computation · `fingerprint` size+mtime+identity · `scan_generation` per-scan UUID · `review_group` duplicate/similar set · `plan` precomputed file-operation preview · `journal` persisted per-item operation state · `ADR` architecture decision record in `docs/adr/` · `deps repo` the private repository `photos-manager-deps` holding checksum-pinned native dependency builds · `budget` the vision token budget of an image embedding.

---

## Change log

- 2026-11-02 — v2.0: merged rewrite replacing the earlier draft, the amendments file, the AI addendum and the decision records. Added the files/assets/volumes/derivatives model, versioned analyzers, similarity edges and review groups, the EmbeddingGemma 2 multimodal rules (Apache 2.0), the extension seams in §13, and the Windows validation process in §17.2.
- 2026-11-12 — v2.1: applied the eleven pending edits from `docs/STATUS.md` after Phase 0 findings. §9.1 opt-in per root with ETA, budget 140 and 256 d, budgets never mixed, runtime decided (R2); §15 reference machines (Mac M4 16 GB, Windows laptop Ryzen 7 5700U 8 GB); §9.2/§19.2/§18 cloud AI is bring-your-own-key plus local endpoint, Phase 6 may follow Phase 7; §17.3 Rust toolchain pinned exactly; §3.3/§6.5 HEIF stack (libvips only, own pinned build, no x265, patent note); §17.2 installer-based Windows clean-machine test and VC++ runtime accounting; §3.4 own-build recipe rules, digest pins and the gate-script rule (D19); §17.1 CI hygiene rules (D12); §3.3/§13.1 RAW in two tiers (D15); decision log extended to D20 (D20: no GIF writing on macOS, deployment target 15.0). Also folded in: the agent-boundary rule (D17, §0), libvips initialisation and error-buffer rules (STATUS risks 8 and 37), `raw_preview` derivative kind, the `model_revision` content, HEIC orientation status, new risks 9 to 13. D15 stays "proposed" until the owner confirms.

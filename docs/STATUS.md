# Project Status

Last updated: 2026-11-08 (evening) · Updated by the orchestrator after each review.

## Phase gates

| Phase | State | Gate evidence |
|---|---|---|
| 0 Feasibility | **In progress** (0A, 0B, 0C accepted and merged; 0D investigation reviewed, fix round open; 0E, 0F next) | reports/REVIEW-0A-0B.md, reports/REVIEW-0C.md, reports/REVIEW-0D.md. Still needed: 0D merged, 0E accepted (Windows HEIC), 0F accepted (macOS HEIC packaging), owner's full Windows checklist pass |
| 1 Catalogue and scanning | Blocked on Phase 0 (owner may relax: see pending decisions) | |
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
| 0B2 | Rust tokenisation + image preprocessing parity for the chosen ONNX files; run on both machines (proposed; write after 0D/0E/0F; before Phase 4B) | Proposed | | | |
| 0C | CI matrix and clean-machine packaging | Accepted, squash-merged to main (`7f4c31b`) | ACCEPT | reports/REPORT-0C.md | accepted/0C |
| 0D | HEIC decode: investigation (stopped on the stop condition, correctly) | Reviewed 2026-11-08: **FIX** (five evidence/overclaim items, plus CI proof for the `package-bundle.mjs` change and download retries in ci.yml). Branch `task/0D`, not yet pushed. Route decided (D10, D11). Once accepted, merge it; it carries no HEIC capability, only the investigation and the loud-failure packaging fix | FIX | reports/REPORT-0D.md, reports/REVIEW-0D.md | accepted/0D (after merge) |
| 0E | Windows: own HEVC-decode-only libvips build (MXE recipe with libde265 on, x265 off), pinned release asset, app CI consumes it, HEIC fixtures, smoke test `heic_decode_verified` | Task file final (`tasks/TASK-0E-windows-heic-libvips.md`); issue after 0D is merged | | | |
| 0F | macOS: HEIC in the packaged app (candidates: rebuild only libheif without x265 and bundle the heif module as a stopgap, or full own build; decided after 0E) | Proposed; write after 0E accepted | | | |

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
| D8 | App identifier `com.narayanwaraich.photosmanager` (owner, 2026-11-07); the app-data folder name changes with it, dev data is not migrated | Accepted; applied in 0C fix item 6 |
| D9 | iPhone HEIC decode on both OSes is a Phase 0 task | Accepted in principle (owner, 2026-11-07); route decided in D10/D11; tasks 0D (investigation), 0E (Windows), 0F (macOS) |
| D10 | libvips stays the only image engine on both OSes. No macOS ImageIO (or Windows WIC) seam in v1: a second decode path with different colour/orientation/HDR behaviour on one OS, new unsafe FFI, a spec change, and no help for Windows. Documented fallback only, and only with the owner's agreement | Accepted (orchestrator, 2026-11-08) |
| D11 | HEIC-capable libvips comes from our own pinned build recipe, Windows first: upstream MXE builder with libde265 on and x265 off, manually started workflow, checksum-pinned release asset in this repo (0E). macOS after (0F). Rejected: swapping in a clang/CMake-built `libheif.dll` inside the MXE zip | Accepted (orchestrator, 2026-11-08); owner may veto |

### Pending owner decisions (not yet in the spec)

| Topic | State |
|---|---|
| Start Phase 1 while 0E/0F run? | Default no (strict gates). Phase 1 does not decode images, so relaxing is low risk; the first Phase 1 task also needs spec v2.1 and I prefer one thing at a time. Owner may say otherwise |
| GitHub Actions minutes | Repo is private; macOS runners count 10x and Windows 2x against the free allowance. Owner to report usage (Settings, Billing); if high, CI runs less often |
| HEIF/HEIC decode route | Decided: D10/D11. HEVC patent pools are separate from the library licence: fine for personal use, legal check before Phase 7 |
| Windows libvips binding: personal git fork of the Rust bindings (rung 1 of the §3.3 ladder) | Provisional; 0C fix item 8 documents it (crate resolves as libvips 1.7.2 via `[patch.crates-io]` in src-tauri/Cargo.toml). Revisit once the own-build zip exists (0E keeps the same layout, so the fork should keep working) |
| D4 | Open (see above) |

### Owner's answers
2026-11-06:
- Windows PC: Acer Swift SF314-43 laptop, Ryzen 7 5700U, 8 GB RAM, 496 MB graphics, Windows 11. It is the low-end reference machine.
- Cloud AI: not a priority; small local models and/or bring-your-own accounts (user's own API key or a local OpenAI-compatible endpoint).
- Defaults accepted: personal use first, library up to 100k photos on SSD, JPEG + iPhone HEIC (RAW later), sidecars and other managers ignored, name "Photos Manager".

2026-11-07:
- Identifier: `com.narayanwaraich.photosmanager`.
- Purpose confirmed: the app exists to manage iPhone photos on the Mac and the laptop, so HEIC on both is needed soon.
- A Syncthing problem must be cleaned up before the Windows checklist (code moves via GitHub, not Syncthing). Solved by the owner 2026-11-08.

### Pending spec edits (apply together as spec v2.1 when the first Phase 1 task is issued)
1. §9.1: embedding is opt-in per root, resumable, pausable, with an ETA; default budget 140 and 256 d; budgets never mixed in one index.
2. §15: name the reference machines (Mac M4 16 GB; Windows Ryzen 7 5700U 8 GB) and measure on both.
3. §9.2 / §19.2: cloud AI is bring-your-own-key plus local OpenAI-compatible endpoint first; Phase 6 may follow Phase 7.
4. §17.3: Rust toolchain pinned to an exact version (1.99.1 now, in rust-toolchain.toml and CI).
5. §3.3 / §6.5: HEIF stack: libvips stays the only engine (D10); our own pinned build with libheif + libde265 (+ libaom), no x265 (D11); HEVC decoder licence/patent note.
6. §17.2: the Windows clean-machine test runs the installer (not the bare exe), and the VC++ runtime must be accounted for.
7. §3.4: dependency builds that we produce ourselves live under `deps/`, are started manually, and are stored as checksum-pinned release assets.

## Known risks

1. Packaging libvips, ExifTool (Perl) and the embedding runtime self-contained on both OSes is the largest technical risk. Package-level tests exist and pass (installer on Windows, including on the owner's laptop; .app with Homebrew hidden on macOS). The remaining packaging risk is now concentrated in HEIC (items 12, 22).
2. Image embedding runtime: ONNX exports exist but preprocessing and input names are undocumented outside Transformers.js; image embedding through mainline llama.cpp is unproven (confirmed unusable, issue #30082).
3. The coding model's knowledge cutoff (March 2026) can produce stale crate and API usage.
4. Windows is validated through CI plus the owner's manual pass on the Windows laptop (`docs/ORCHESTRATION.md` §6); the owner develops on a Mac. First laptop evidence 2026-11-07: `pnpm verify` passes (16 tests), `pnpm tauri dev` opens with custom-protocol thumbnails. Installer test on the laptop (2026-11-07/08): first attempt used a stale artifact (my mistake) and failed with a missing DLL; two retests with newer artifacts passed (exit 0, `status: ok`, libvips from the install folder, 74 DLLs). Laptop has `LongPathsEnabled = 1` and `vcruntime140.dll` present.
5. Gemini 3.8 Flash API model ID verified in 0A; promotional pricing ends 31 Dec 2026. Cloud AI is low priority now.
6. Embedding time: about 1 s per image on an M4 CPU (budget 140), about 27 h for 100k photos; the Windows laptop is probably several times slower (not measured). Embedding must be opt-in, resumable, with ETA.
7. Rust-side tokenisation and image preprocessing for the ONNX path are unproven (0B2).
8. libvips initialisation: now done once via `OnceLock` (0C), good. Remaining (Phase 1): `.expect()` on init and `std::env::set_var` inside the lazy init (global state, racy once threads exist); move init and blocking to app start and return a `Result`. `lib.rs` keeps the standard Tauri `.expect` on startup; replace with an error dialog in Phase 1.
9. Scope creep toward later-version features (spec §13); each needs an ADR first.
10. SVG/ImageMagick loaders: **closed for Phase 0.** Blocked at runtime; CI control with the stock `vips` CLI shows the SVG loads unblocked and fails under `VIPS_BLOCK_UNTRUSTED=1` on both OSes (so the env var alone suffices; the extra in-app block calls are untested redundancy). `has_svgload: false` is a constant, not evidence. The Windows zip still ships librsvg (unused, blocked). 0E must keep the control working.
11. macOS libvips is a Homebrew harvest including GPL libraries (fftw, imagequant 4.x), acceptable for personal use only; macOS ExifTool depends on system `/usr/bin/perl`. The dylib table is script-checked in CI (67 files on the CI runner; the owner's Mac produced 65: brotli was the difference). The build is not reproducible (Homebrew floats). The CI check will fail whenever Homebrew changes the dependency set: update the table then. Goes away with 0F if the full own build is chosen.
12. **iPhone HEIC decode is verified on neither packaged OS.** Facts after the 0D investigation (all from the agent's report unless marked): macOS: Homebrew's `libheif` hard-links x265 (GPL encoder), so Homebrew's binary `libheif` cannot be bundled without shipping x265 or crashing at load; the HEIF loader is a runtime-loaded libvips module that the bundle script never finds (module lookup uses `$VIPSHOME/lib/vips-modules-8.18`); raw `otool`/`nm` output still to be pasted (0D fix 1). Windows: the prebuilt `libheif.dll` has no HEVC decoder and no plugin loading (strings evidence; no libde265/x265/dav1d DLL); upstream `build-win64-mxe` has `--with-hevc` which turns on libde265 AND x265 together (reviewer verified in the README), and the prebuilt zips exclude both. The owner's Mac with Homebrew visible decodes a real iPhone HEIC, so `pnpm tauri dev` on the Mac is not blocked. The Windows laptop cannot decode HEIC even in dev until 0E. Fix path: D10/D11, tasks 0E (Windows) then 0F (macOS).
13. The Windows libvips binding is a single-maintainer git fork (0C fix 8; revisit after 0E).
14. 8 GB Windows laptop: cold clippy took 7 min and tests 2 min with `CARGO_BUILD_JOBS=2`; app memory target (≤ ~500 MB RSS excluding model) should be checked there.
15. Visual C++ runtime: **resolved for the exe** (static CRT via `.cargo/config.toml`; CI `dumpbin /dependents` shows no VCRUNTIME/MSVCP) and for the 38 libvips DLLs (reviewer's `objdump` of the zip: only the Windows Universal CRT and system DLLs). Round 3 extended the CI scan to all installed libvips DLLs. Not scanned: the ExifTool Perl runtime under `resources\exiftool\exiftool_files\` (low risk). 0E must scan the new DLLs (libde265) too.
16. Windows kill-on-timeout for ExifTool is validated on macOS only (the two Unix-only tests). Measured in CI (stay_open mode, 2 s): `exiftool.exe` is a single process with no child on Windows. Still no Windows test that a timeout kills it: add a Windows variant of `test_child_killed_after_timeout` with the metadata work in Phase 1. The long-path test depends on `LongPathsEnabled`, which is `0x1` on both the CI runner and the owner's laptop.
17. HEVC patent pools: separate from the library licences; fine for personal use, get a legal check before any distribution (Phase 7). Same for the GPL libraries in item 11.
18. Dev setup on Windows needs several environment variables (`VIPS_DIR`, `VIPS_LIB_DIR`, `EXIFTOOL_DIR`, `PKG_CONFIG_PATH`, `LIB`, `PATH`); the doc omitted them (0C fix 2). Syncing the repo through Syncthing between OSes is unsafe; use GitHub.
19. Carry-forward list from the 0C review (none blocks Phase 0): `vips.rs` `.expect()` and `set_var` in the lazy init (Phase 1); `package-bundle.mjs` swallowed failures of `install_name_tool`/`codesign`/`ditto` (**fixed in 0D, pending CI proof**); CI pins (floating `dtolnay/rust-toolchain@stable`, Node 20 action deprecation warnings) and `--smoke-test` shipped in the production binary (Phase 7); stray `.gitkeep` in the Windows installer; GitHub-hosted licence URLs in §4.2 were not independently re-checked by the reviewer (spot-check before any distribution; 0E rewrites §4.2).
20. **CI downloads have no retries** (found 2026-11-08): run 42464971330 (the push of the 0C merge to `main`) had `windows-x64` fail after 12 s with `curl: (22) ... 504` fetching the libvips zip from GitHub Releases; the script kept going and failed misleadingly at `Get-FileHash`. A transient upstream error, not a regression. Fix: 0D fix item 6 (retries, stop on first failure, same for the SourceForge ExifTool download). Rerun of that run pending (NEXT_STEPS step 1).
21. **GitHub Actions minutes** (private repo): macOS runners cost 10x and Windows 2x of the free allowance; 0C used 10+ runs. Owner to check usage. Heavy builds (0E's libvips build) must be `workflow_dispatch` only.
22. **Own libvips build recipe** (0E/0F) is new maintenance surface: we pin an upstream commit and carry a small patch; MXE builds are long (estimate 1-2 h on Linux, not measured). Mitigation: built once, stored as a release asset with a checksum; the app CI never rebuilds it.

## Log

- 2026-11-02 — Pack v1 created.
- 2026-11-02 — Pack v2: spec rewritten as one merged file (v2.0) with extension seams for RAW, maps, import, backup, video, faces; amendments, AI addendum and ADR files folded in and removed; Windows PC validation added; tasks 0A/0B/0C revised; ANTIGRAVITY_SETUP and three skills added.
- 2026-11-03 — Reports 0A and 0B reviewed (reports/REVIEW-0A-0B.md): 0A FIX (evidence), 0B provisional ACCEPT of ONNX Runtime Q8 + FIX (evidence); 0B2 proposed.
- 2026-11-05 — Round 2 review (reports/REVIEW-0A-0B.md): concrete fix prompts issued for 0A and 0B; git cleanup plan given (prep commit on main, merge main into each branch, squash-merge on ACCEPT); owner's terminal lacks cargo on PATH.
- 2026-11-05 — Round 3: 0A and 0B ACCEPTED. App runs under `pnpm tauri dev` (screenshot), 15 tests pass on the owner's machine. Next: merge both to main, run TASK-0C. Owner asked for `docs/NEXT_STEPS.md` (single step-by-step file, rewritten each conversation) and a Rust/Git learning track (`docs/LEARNING.md`).
- 2026-11-05 (evening) — 0A confirmation checks passed. NEXT_STEPS rewritten.
- 2026-11-06 — 0A and 0B squash-merged to `main`, tagged, pushed. TASK-0C ran: 10 CI runs, run #9 green on macOS arm64, Windows x64 and cargo deny. Reviewed: FIX, nine items. Owner's answers recorded. NEXT_STEPS and LEARNING (lesson 2) updated.
- 2026-11-07 — TASK-0C round 2 reviewed against the repo at 16d37ad (reports/REVIEW-0C.md): FIX, nine items. Corrected my own round-1 errors (Windows HEIC claim; Step 1 ran a bare exe without DLLs, so the owner's laptop smoke test printed nothing). Owner's laptop: `pnpm verify` and `pnpm tauri dev` pass on Windows 11; first installer test used a stale artifact and failed with a missing DLL (retest pending). Owner decided the identifier and asked for HEIC on both OSes (TASK-0D proposed). NEXT_STEPS (installer-based laptop test, Syncthing steps, fix prompt) and LEARNING (lesson 3) updated.
- 2026-11-08 — TASK-0C round 3 reviewed (reports/REVIEW-0C.md): FIX (6 small items). I opened the libvips Windows zip: no `manifest.txt`, no `licenses/`; the docs' §4.2 versions were invented. Verified the fork diffstat against upstream (exact). Owner's laptop installer test passed twice (after the stale-artifact failure). Owner's Mac `vips -l` lists `heifload`. TASK-0D drafted. Syncthing issue solved by the owner.
- 2026-11-08 (later) — TASK-0C round 3 re-checked at `20fb0ee`: all six items done with real CI output (dylib table script 67 = 67; SVG control on both OSes; Windows exiftool single process; 38/38 DLLs clean; real diffstats; macOS zip size mystery explained). **0C ACCEPTED.** Owner decoded a real iPhone HEIC on the Mac with Homebrew libvips. TASK-0D finalised; merge and docs-sync steps given.
- 2026-11-08 (evening) — 0C squash-merged to `main` (`7f4c31b`, tag `accepted/0C`). TASK-0D ran and stopped on its stop condition: investigation report on `task/0D`. Reviewed (reports/REVIEW-0D.md): **FIX** (no raw macOS evidence, circumstantial Windows evidence, an unexplained "real HEIC" claim, two overclaimed acceptance rows, one unproven code change). Verified upstream: `build-win64-mxe --with-hevc` builds libde265 and x265 together; prebuilt zips contain neither. Decisions D10 (libvips only, no ImageIO seam) and D11 (own pinned build, Windows first). The `main` CI run 42464971330 had a transient 504 in the Windows download step (risk 20). TASK-0E written; 0F proposed. NEXT_STEPS and LEARNING (lesson 4) updated.

# TASK-0A: Scaffold, database and native-dependency spikes

**Phase:** 0 · **Branch:** `task/0A` · **Depends on:** nothing
**Orchestrator note:** This replaces the old `ANTIGRAVITY_FIRST_TASK.md`. It proves the foundation on the local OS (the owner's primary dev machine is a Mac). The owner may also run `pnpm verify` on a Windows PC and report the result; do not claim Windows support from your own run. CI and clean-machine packaging are TASK-0C. Embeddings are TASK-0B and must not be started here.

## Read first
- `AGENTS.md`
- `IMPLEMENTATION_SPEC.md` — read the **whole file once** for this task; pay most attention to §2, §3 (including the fallback ladder in §3.3 and release gate in §3.4), §5.1, §7.2, §12.3

## Goal
A minimal Tauri 2 + React + TypeScript + Vite app that builds and runs locally; SQLx/SQLite initialises in the app-data directory through one versioned migration; a libvips proof of concept makes an orientation-correct thumbnail; an ExifTool proof of concept returns normalized metadata; and the exact versions, licenses, codecs and packaging plan are written down. No product features.

## In scope
1. Inspect the workspace. If it holds only these docs, scaffold the smallest Tauri 2 + React + TypeScript + Vite app with pnpm. Record the exact template or command used. Preserve the existing doc files and folders.
2. **Verify, do not recall:** current compatible versions and install requirements for Tauri 2, Rust (stable), SQLx + SQLite, the chosen Rust libvips binding, libvips itself, ExifTool, and the Gemini API model ID and image-input request shape (read-only check against official docs, no runtime integration). Put findings in `docs/native-dependencies.md` with the URL and date of each source.
3. SQLx connection with WAL, foreign keys on, busy timeout, and **one** migration creating `app_settings` (spec §5.2, `app_settings` only). Resolve the path with Tauri's path API. A Rust test creates the DB in a temp dir, runs the migration and reads a row back.
4. libvips PoC: read one fixture JPEG that has a non-trivial EXIF orientation and one PNG; write a 320 px-max-edge WebP thumbnail with orientation applied; test that output dimensions and orientation are correct. Put it behind the `ImageProcessor` trait from spec §3.3 (trait + one implementation only). If the chosen binding is unsuitable, document the exact problem and test one alternative from the fallback ladder in spec §3.3. Do not change the architecture silently.
5. ExifTool PoC: invoke the executable **without a shell**, argument array only, with a timeout and an output-size cap. Read one fixture image, parse JSON, return a normalized struct (width, height, date taken as string + parse status, camera make/model). Tests cover: success, missing file, malformed output, timeout.
6. Packaging investigation (research plus a notes section, not a full bundle yet): how libvips shared libraries and ExifTool (including its Perl runtime requirements on each OS) can be bundled and located at runtime in a release package on Windows x64 and macOS arm64; license obligations; which codecs the bundled libvips has (JPEG, PNG, WebP, TIFF, HEIC, AVIF). State plainly that system-installed copies are not sufficient.
7. Fixtures: a script that **generates** test images (including EXIF-rotated JPEGs) into `tests/fixtures/generated/` and `docs/test-fixtures.md` describing them. No real photos.
8. `pnpm verify` script that runs: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`, frontend typecheck, lint, production build, and a check that `AGENTS.md` is under 12,000 characters. Output is written to `reports/logs/verify-<date>.txt`.
9. `README.md`: prerequisites and exact commands to install, run, test and build, on macOS. Include Windows commands (PowerShell) but mark them "untested" unless you actually ran them; the owner's Windows PC or TASK-0C will validate them.
10. Minimal Tauri capabilities: no filesystem scope, no shell plugin. Restrictive CSP. Document what you set.
11. `.gitignore` (node_modules, target, dist, `reports/logs/`, local fixtures, `.env*`, model files).
12. **Custom protocol + CSP spike** (spec §7.2): a minimal Tauri custom URI protocol that serves one generated thumbnail by id to an `<img>` under a strict CSP, with no `fs` plugin and no `asset:` scope. Record the URL shape that works on this OS and what is expected to differ on WebView2.
13. **IPC types check (decision D4):** verify whether `tauri-specta` (or an equivalent generator) supports the Tauri 2 version in use, and write two or three factual lines in `docs/native-dependencies.md`. Do not adopt it; the owner decides at the start of Phase 1.
14. **Path-handling probe:** a Rust test that round-trips a non-UTF-8 and a non-ASCII path through the native-path storage approach in spec §5.1 (BLOB + display string). On macOS, also demonstrate NFC vs NFD names. Mark Windows cases as "to be run on Windows" if you cannot run them.

## Out of scope (do not do)
- Library scanning, folder pickers, file operations, UI beyond a blank window with a build-info line.
- Any Gemini or other AI runtime code. No API keys anywhere.
- Embeddings (TASK-0B), CI workflows and Windows builds (TASK-0C).
- RAW/LibRaw, video/ffmpeg, face detection, maps (spec §13: later versions; no stubs).
- More than one migration or table.

## Allowed paths
Repository root files (`package.json`, lockfiles, configs, `README.md`, `.gitignore`), `src/`, `src-tauri/`, `scripts/`, `tests/`, `docs/native-dependencies.md`, `docs/test-fixtures.md`, `reports/`. Do not edit the other docs.

## Acceptance criteria
- [ ] `pnpm install && pnpm tauri dev` starts the app on the local OS.
- [ ] `pnpm verify` passes and its log is in `reports/logs/`.
- [ ] DB created in the Tauri app-data directory; migration test passes.
- [ ] libvips thumbnail test passes for the rotated JPEG and the PNG; output reports its real codecs.
- [ ] ExifTool tests (success, missing, malformed, timeout) pass; no shell used (show the code path).
- [ ] `docs/native-dependencies.md` lists exact versions, licenses, OS/arch, codecs, bundling strategy, smoke-test results and unresolved risks, each with a source URL and date.
- [ ] Gemini section of that doc states the verified API model ID (or says it could not be verified) and the image-input request shape.
- [ ] Capabilities file shows no filesystem or shell permissions; CSP is set.
- [ ] Custom-protocol thumbnail renders in the app under the CSP; the URL shape is documented.
- [ ] Path-handling test passes for non-UTF-8 and non-ASCII names on this OS.
- [ ] Report names the OS actually tested and lists what remains for Windows.

## Verification commands
```
pnpm verify
cargo test --manifest-path src-tauri/Cargo.toml
rg -n "Command::new\(\"(sh|bash|cmd|powershell)" src-tauri/src   # expect no matches
rg -n "execute_sql" src-tauri/src                                  # expect no matches
wc -c AGENTS.md                                                    # must be < 12000
```

## Stop conditions
Stop and report if:
- libvips cannot be built or linked on the local OS through any rung of the spec §3.3 ladder within reasonable effort;
- a dependency's license blocks redistribution;
- ExifTool cannot be bundled without a system Perl;
- anything in the spec conflicts with what you find.

## Deliverable
`reports/REPORT-0A.md`. Then stop and wait for review.

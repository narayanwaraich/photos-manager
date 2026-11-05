# TASK-0C: CI matrix and clean-machine packaging

**Phase:** 0 · **Branch:** `task/0C` · **Depends on:** TASK-0A accepted; a GitHub repository exists and is connected
**Orchestrator note:** CI is the first Windows evidence; the owner's Windows PC is the second (§17.2). The point is to prove that a built package runs with **no developer libraries on the machine**. Do not claim success from a green build alone.

## Read first
- `AGENTS.md`
- `IMPLEMENTATION_SPEC.md` §3.3 (fallback ladder), §3.4 (native dependency release gate), §17 (platform matrix, validation, packaging), §18 Phase 0
- `docs/native-dependencies.md` (from TASK-0A)

## Known issues carried over from the 0A/0B reviews (must be handled here)
1. `src-tauri/build.rs` hard-codes `/opt/homebrew/lib` and `metadata::which_exiftool` hard-codes Homebrew/system paths. Both are dev-only; the release build must locate libvips and ExifTool from the bundle. Remove or gate them behind an explicit dev switch.
2. The libvips build to bundle must be **minimal**: JPEG, PNG, WebP, TIFF, GIF, HEIF/AVIF decode only. Do not ship `magickload` (ImageMagick) or `svgload` (librsvg): large attack surface and not v1 formats. For HEIF use a decode-only stack (libde265 / dav1d); do **not** bundle x265 (GPL-2.0, encoder) from a stock Homebrew install. Record each library's licence.
3. ExifTool packaging claims in `docs/native-dependencies.md` are unverified: the current official Windows package may be an exe **plus a support folder**, not a single file; do not assume macOS `/usr/bin/perl` exists on current macOS (check on macOS 27 and prefer a bundled Perl). Launch ExifTool from Rust with the resolved resource path; do not enable the Tauri shell plugin.
4. Test ExifTool on Windows with a non-ASCII and a >260-character path (`-charset filename=UTF8` behaviour on Windows is unproven).
5. Bundle the ONNX Runtime library inside the signed app (see `docs/embedding-spike.md`); `ort` is pinned to a release candidate, so pin the exact version with `=` and record which ONNX Runtime library version it needs. Do not integrate it; record the bundling steps.
6. Add `rust-toolchain.toml` (spec §17.3) and a `cargo deny`/`cargo audit` step.
7. The app identifier `com.photosmanager.desktop` is a placeholder; the owner confirms the real one before the first packaged build.

## Goal
A GitHub Actions workflow runs `pnpm verify` and a packaging smoke test on Windows x64 and macOS arm64 (and macOS x64 only if the project claims it), producing unsigned bundles whose native dependencies are located from the bundle, not from the machine.

## In scope
1. Workflow `.github/workflows/ci.yml` with a matrix over `windows-latest` and `macos-latest` (verify which architecture each label currently provides; document it). Cache Rust and pnpm. Pin third-party actions by version or SHA after checking them.
2. On each OS: install prerequisites exactly as documented in `README.md`, run `pnpm verify`, then `pnpm tauri build` (unsigned).
3. A **smoke binary or flag** (a small, clearly named test entry, not a product feature) that, from the built package, loads libvips, makes a thumbnail from a generated fixture, runs ExifTool, and prints versions and codec lists as JSON.
4. **Clean-environment run:** execute the smoke test with the dependency lookup environment stripped: no Homebrew paths on macOS (`env -i` plus minimal PATH), and no libvips/ExifTool/Perl on PATH on Windows. Show the evidence (the environment printed, the paths the smoke test actually loaded from).
5. Upload the unsigned bundles and smoke output as workflow artifacts.
6. Update `docs/native-dependencies.md` with results for **each** OS: loaded library paths, versions, codecs, failures, and the packaging steps that were required.
7. Write down, but do not implement, what Phase 7 needs for signing and notarization.
8. Add `docs/windows-validation.md`: the exact step-by-step manual checklist the owner runs on the Windows PC (`git pull`, install prerequisites, `pnpm verify`, `pnpm tauri dev`, run the packaged smoke test from the CI artifact on a machine/user profile without dev tools if possible). Include non-ASCII and long-path folders and an external-drive step marked "needed from Phase 1".
9. If the embedding task (0B) was accepted before this one, record in `docs/native-dependencies.md` what the chosen runtime (ONNX Runtime or llama.cpp) needs to be bundled on each OS; do not integrate it.

## Out of scope (do not do)
- Code signing, notarization, installers beyond what `tauri build` produces by default.
- New product features or any embedding/AI work.
- Secrets in the workflow. Use none.

## Allowed paths
`.github/`, `scripts/`, `src-tauri/` (smoke entry and bundle config only), `docs/native-dependencies.md`, `docs/windows-validation.md`, `README.md`, `reports/`.

## Acceptance criteria
- [ ] Both matrix jobs pass, with links or logs attached to the report.
- [ ] Clean-environment smoke test passes on **both** OSes and prints the real library paths (they must point into the bundle or its resources).
- [ ] Codec lists per OS recorded; any format that differs between OSes is called out.
- [ ] Artifacts produced and named.
- [ ] `docs/native-dependencies.md` updated per OS; unresolved risks listed.
- [ ] The report states which things are still only validated on one OS.

## Verification commands
```
gh run list --workflow ci.yml --limit 5
gh run view <run-id> --log | rg -n "smoke|libvips|exiftool"
```

## Stop conditions
Stop and report if the clean-environment run on either OS fails. Then document the failure and the next rung of the spec §3.3 ladder; do not weaken the clean-environment test to make it pass.

## Deliverable
`reports/REPORT-0C.md`. Then stop.

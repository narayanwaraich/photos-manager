# TASK-0E: Windows HEIC decode: own HEVC-decode-only libvips build

**Phase:** 0 · **Branch:** `task/0E` (branch from `main` after 0D is merged) · **Depends on:** TASK-0D accepted and merged
**Orchestrator note:** Decisions D10/D11 (see `reports/REVIEW-0D.md`): libvips stays the only image engine on both OSes (no ImageIO seam), and HEIC support comes from our own pinned build of libvips, Windows first. The owner's Windows laptop cannot decode an iPhone HEIC today, even in `pnpm tauri dev`. Do not claim success from AVIF or from a green build: prove it with a HEIC made by a real HEVC encoder, decoded by the built package with developer tools hidden. macOS is TASK-0F; do not touch macOS packaging here.

## Read first
- `AGENTS.md`
- `IMPLEMENTATION_SPEC.md` §3.3, §3.4, §17, §18 Phase 0
- `docs/native-dependencies.md` (§4.2 Windows zip contents from `versions.json`, §4.4 untrusted loaders)
- `reports/REPORT-0D.md`, `reports/REVIEW-0D.md`
- Upstream `libvips/build-win64-mxe`: README, `build.sh`, the Dockerfile and `plugins/` / package `.mk` files, at the tag `v8.18.6`

## Facts established in review (verify, do not assume)
1. Upstream's `build.sh --with-hevc` builds **libde265 (LGPL-3.0) and x265 (GPL-2.0) together**. The prebuilt release zips use neither. x265 is an encoder and GPL: it must not end up in our build.
2. The prebuilt `libheif.dll` (1.23.2) has no plugin loading (strings: "Plugins are not supported"; no `LIBHEIF_PLUGIN_PATH`) and no libde265. So the decoder must be built into the dependency set that `libheif.dll` is linked with; dropping a loose DLL next to it cannot work.
3. Current zip: `vips-dev-x64-web-8.18.6.zip`, SHA-256 `10086F2CCC8E2A861831FACABAB75FA1B80B4BD03F0A8447623EC581002AA4B2`. Our app CI, `build.rs` and docs expect its layout (`vips-dev-8.18/bin`, `lib`, `lib/pkgconfig`, `include`, `versions.json`). Keep the new zip's layout identical so the app side changes only a URL, a version and a checksum.
4. HEVC patent pools are separate from library licences: fine for personal use, legal check before distribution (Phase 7). Record it; do not decide it here.

## Goal
A pinned, reproducible Windows x64 libvips zip with libheif + libde265 (+ libaom for AVIF), **no x265**, built by a manually started workflow, stored as a release asset in this repo with a recorded SHA-256, and consumed by the app CI so that the installed Windows package decodes an iPhone-style HEIC fixture to an upright, correctly coloured WebP thumbnail with PATH reduced to OS basics.

## In scope

### Part A: the build recipe and the artifact
1. **Read before building (report section 1).** From upstream `build.sh`, the Dockerfile and the package files, write down with file paths and line numbers: how `--with-hevc` adds libde265 and x265; how libheif is configured in the recipe (CMake options, whether plugin loading is on, which decoders are linked); how to build only the `web` variant for x86_64; which commit the tag `v8.18.6` points to (`git ls-remote --tags https://github.com/libvips/build-win64-mxe`). Record the exact commit SHA you will build.
2. **Recipe in this repo, no vendored upstream source.** Create `deps/win64-vips/` with: `README.md` (what it is and how to rerun), `UPSTREAM.txt` (repository URL, pinned commit SHA, variant, architecture), `patches/*.patch` (the smallest patch that builds libde265 **without** x265 and configures libheif with the libde265 decoder and the libaom decoder; list every changed line in the report), and, if needed, a thin `build.sh` wrapper. Source tarball checksums come from upstream's package files; show the checksum line for libde265 in the report.
3. **Workflow `.github/workflows/deps-win64-vips.yml`**, `workflow_dispatch` only (never on push: it is long and runner minutes are limited), `timeout-minutes` set, `permissions: contents: write` for this workflow only. Linux runner job: clone upstream at the pinned SHA, verify the SHA, apply patches, build, upload the zip. Print `sha256sum` and the file list. Audit step that **fails** when: any x265 DLL or string `x265` appears in `bin/`; no libde265 DLL (or libde265 symbols in `libheif.dll`) is present; `versions.json` lacks libde265; any `fftw`, `imagemagick` or `poppler` component appears (the `web` variant must not have them).
4. **Windows test job** in the same workflow (`windows-latest`, needs the Linux job): unzip the artifact to a short path, reduce PATH to `C:\Windows\System32;C:\Windows` plus the zip's `bin`, then with the zip's own `vips.exe` and `vipsthumbnail.exe`: `vips.exe -l` lists `heifload`; decode the HEIC fixture (Part B step 6) to PNG and check the dimensions; run the 0C SVG control (`VIPS_BLOCK_UNTRUSTED=1` blocks the SVG fixture, unblocked loads it). Paste raw output. A job that cannot decode the fixture fails.
5. **Publish** (only when the test job passed): create a release in this repo named `deps-win64-vips-8.18.6-hevc1` and attach the zip, using `gh release create` with the workflow token. Record the asset URL and SHA-256 in `docs/native-dependencies.md` and rewrite §4.2 from the new `versions.json` (exact versions, DLL names, licences with URLs; write "not shipped in the archive; check upstream" where you cannot cite). Say plainly which libraries are LGPL and that none is GPL.
6. **Limit:** if the Linux build is not green after **two** workflow runs, stop and report with the logs. Do not iterate for hours.

### Part B: use it in the app CI
1. `ci.yml` Windows job: download the zip from the release with `gh release download` (token from `secrets.GITHUB_TOKEN`, `permissions: contents: read`), with retries and `$ErrorActionPreference = 'Stop'` as in 0D; verify the pinned SHA-256; keep the same extraction layout. Remove the old URL and checksum for the upstream zip from the Windows job. Update `build.rs` or the docs only if a path actually changed.
2. `docs/windows-validation.md`: how the owner gets the zip onto the laptop (release page download or `gh release download`, verify checksum, extract to `C:\vips`), and an owner step: "decode one of your own iPhone HEIC files (a copy, not the original) from the installed app / with `vipsthumbnail`; do not commit it".
3. **HEIC fixtures** (no personal photos in git): generate on the Mac from the existing synthetic JPEG fixtures with `sips -s format heic` (or `heif-enc` if present), one with Orientation 6 and one with a Display P3 profile (`sips -m` or an equivalent). Verify with `exiftool` that each file is HEVC (paste output showing the HEVC configuration or codec) and that orientation and profile survived. Small files only; commit them under `tests/fixtures/generated/`; a generator note in `docs/test-fixtures.md`. Windows and macOS CI must use the same committed bytes. If `sips` bakes the orientation into the pixels instead of the tag, say so and choose a method that keeps the tag.
4. **Smoke test**: extend the smoke entry (`src-tauri/src/smoke.rs`): decode the orientation-6 HEIC to a 240x320 upright WebP; sample a pixel and compare with the JPEG original within a stated tolerance; read the HEIC with the bundled ExifTool (orientation, camera make). Add `heic_decode_verified` (true/false) and a `heic_note` string to the JSON. `is_loader_supported` must report what libvips really says for `heifload` (not a constant). CI: the Windows job sets `REQUIRE_HEIC=1` and fails if `heic_decode_verified` is not true; the macOS job must **not** fail on it yet and must print `heic_decode_verified: false` with `heic_note: "not packaged yet (TASK-0F)"`.
5. Keep the 0C checks working: SVG/ImageMagick blocking control, DLL scan for VC++ runtime imports (scan the new DLLs too), the script-checked tables as they apply to Windows.

## Out of scope (do not do)
- macOS packaging, the macOS libvips, ImageIO or any platform decoder, WIC.
- HEIC/HEVC encoding, x265 in any form, any UI, scanning, database work, signing or notarization.
- iPhone HDR gain-map and Live Photo behaviour (the owner's own-HEIC step covers a first look; report what you see only if you happen to have a legal sample).
- Deciding the patent question. Editing `docs/STATUS.md`, `docs/NEXT_STEPS.md`, `AGENTS.md`, the spec.

## Allowed paths
`.github/`, `deps/win64-vips/`, `scripts/`, `src-tauri/` (image_pipeline loader reporting, smoke entry, build config only), `tests/fixtures/generated/` (HEIC only), `docs/native-dependencies.md`, `docs/windows-validation.md`, `docs/test-fixtures.md`, `README.md`, `reports/`.

## Acceptance criteria
- [ ] Report section 1 (how upstream does it, with file:line) written before any build.
- [ ] The patch is small, listed line by line, and the audit step proves: libde265 present, no x265 anywhere, no fftw/imagemagick/poppler.
- [ ] Release asset exists; its SHA-256 is in the docs and checked by CI; layout identical to the old zip.
- [ ] The Windows test job decodes the HEIC fixture with only the zip on PATH; raw output pasted; SVG control still passes.
- [ ] App CI green on Windows with `heic_decode_verified: true` (raw JSON from a downloaded artifact); macOS job green with `false` and the expected note.
- [ ] §4.2 rewritten from the new `versions.json`; licences cited or marked unknown; GPL status stated (none expected).
- [ ] HEIC fixtures verified as HEVC with exiftool output; `docs/test-fixtures.md` updated.
- [ ] The report lists what is validated on one OS only and the owner's own-HEIC steps.

## Verification commands
```
gh workflow run deps-win64-vips.yml
gh run list --workflow deps-win64-vips.yml --limit 3
gh run view <run-id>
gh release view deps-win64-vips-8.18.6-hevc1
gh run list --workflow ci.yml --branch task/0E --limit 3
gh run download <run-id> -D <folder>
```

## Stop conditions
Stop and report if: x265 cannot be excluded; libheif ends up without libde265; any GPL library is in the zip; two workflow runs do not produce a green build; the fixture cannot be decoded; or you are tempted to substitute an AVIF fixture, weaken a check, or use a copy of libheif built outside MXE. In that case give a table of the remaining options with licence, effort and risk; the orchestrator decides.

## Deliverable
`reports/REPORT-0E.md`. Then stop.

# TASK-0D: iPhone HEIC/HEIF decode on macOS and Windows

**Phase:** 0 · **Branch:** `task/0D` (branch from `main` after 0C is merged) · **Depends on:** TASK-0C accepted and merged to `main`
**Orchestrator note:** The owner builds this app to manage iPhone photos on a Mac and a Windows laptop, so decoding a real iPhone HEIC to an upright, correctly coloured thumbnail is a core capability, not a nice-to-have. Today neither packaged build can do it (facts below). Do not claim success from a green build or from AVIF: prove it with a HEIC file made by a real HEVC encoder, run from the installed package with developer tools hidden, as in 0C.

## Read first
- `AGENTS.md`
- `IMPLEMENTATION_SPEC.md` §3.3 (fallback ladder), §3.4 (native dependency release gate), §17, §18 Phase 0
- `docs/native-dependencies.md` (after 0C: §4.2 Windows zip contents from `versions.json`, §4.3 macOS dylib table, §4.4 untrusted loaders)
- `reports/REPORT-0C.md` and `reports/REVIEW-0C.md`

## Facts established in 0C review (verify, do not assume)
1. Windows: `vips-dev-x64-web-8.18.6.zip` (libvips 8.18.6, MXE build) has `libheif.dll` (libheif 1.23.2) and `libaom.dll` but **no HEVC decoder** (no libde265, x265 or dav1d DLL; `libheif.dll` contains HEVC container code only). So `heifload` there can open AVIF, not an iPhone HEIC.
2. macOS: the bundled app has no `heifload`. The agent's explanation (unverified): Homebrew builds the HEIF loader as a separate module (`vips-heif`, under a `vips-modules-*` folder) that `package-bundle.mjs` does not copy. The owner's Mac with Homebrew visible lists `heifload` in `vips -l`, and on 2026-11-08 `vips copy` decoded a real iPhone HEIC there correctly (Homebrew had libheif 1.23.x, libde265 1.1.x and x265 4.x installed). So decode works on a dev Mac; the open question is only how to package it (module, libheif, libde265, plugin lookup paths, relocation, licences). libde265 is LGPL-3.0; x265 (GPL, encoder) must not be bundled: check that decoding does not need it.
3. The macOS bundle is a harvest of Homebrew dylibs including GPL ones (fftw; imagequant 4.x GPL-3.0). Acceptable for personal use, not for distribution.
4. HEVC (H.265) is covered by patent pools that are separate from library licences (libde265 is LGPL, x265 is GPL). Fine for personal use; a legal check is needed before any distribution (Phase 7). Record this in the docs; do not decide it here.

## Goal
From the **installed package** (Windows NSIS installer, macOS unzipped `.app`) with PATH reduced to OS basics, the smoke test decodes an iPhone-style HEIC fixture to a 240×320 upright WebP thumbnail with correct colour, on both OSes in CI; ExifTool reads its metadata; and the owner confirms it with real iPhone HEIC files on both of his machines.

## In scope
1. **Investigation first (report section 1, before building anything).** (a) macOS: try the cheapest route first: copy the heif module and the libraries it needs (libheif, libde265, and whatever `otool -L` shows) into the app, make libvips find the module relative to the app (check `VIPS_MODULEDIR` and the module path libvips compiles in) and libheif find its decoder plugin (check `LIBHEIF_PLUGIN_PATH` and libheif's plugin directory), relocate with `install_name_tool`, and run the smoke test with Homebrew hidden. Also confirm why `heifload` is missing from the bundle today (list the module folder, check `package-bundle.mjs`, run `vips -l` inside the unzipped app with `VIPS_MODULEDIR` unset and set) and which libraries `vips-heif` pulls in. (b) Windows: check the libvips build-win64-mxe repository and release notes for any build that includes an HEVC decoder or a way to add libde265. Then write a table of routes with licence, effort and risk, for each OS: *(1)* our own minimal libvips build (Meson; JPEG, PNG, WebP, TIFF, GIF, HEIF with libheif + libde265 + an AV1 decoder; no x265, fftw, GPL imagequant, librsvg, ImageMagick; no dynamic modules); *(2)* the cheapest patch (macOS: bundle the heif module and its dependencies; Windows: add a libde265 DLL built from source next to `libheif.dll` as a libheif plugin); *(3)* platform decoders behind the `ImageProcessor` trait (macOS ImageIO; Windows WIC needs Microsoft's HEVC extension, which cannot be assumed). Recommend one route per OS. Also make `package-bundle.mjs` fail loudly when `install_name_tool`, `codesign` or `ditto` fails (today `runCmd` swallows errors). **Stop and report with the table if the cheapest route (2) fails on either OS or if route (1) needs more than a CI job's worth of build time; the orchestrator decides.**
2. Implement the chosen route on both OSes. Keep the untrusted-loader blocking from 0C working (`svgload`, `magickload`, and the control step) and keep `heifload` out of any blocked list.
3. **Fixtures (no personal photos in git).** Generate HEIC from the existing synthetic JPEG fixtures with macOS `sips -s format heic` (or `heif-enc` if available), including an orientation-6 case and a Display-P3 profile case. Commit only small generated files, add a generator note to `docs/test-fixtures.md`. Windows and macOS CI must both use the same committed HEIC bytes.
4. Extend the smoke test: decode the HEIC fixture to a thumbnail (size, upright, sampled pixel colour within a stated tolerance of the JPEG original), and run the bundled ExifTool on the HEIC fixture (orientation and camera make). Add `heic_decode_verified` to the JSON. Fail if either OS cannot decode.
5. Licence and provenance: extend §4.2/§4.3 with every added library (version from the build, licence from the source tree), pinned checksums for every downloaded source or archive, and a script check that every bundled library has a row (as in 0C). State plainly if any GPL library is still bundled on macOS and what remains to remove it.
6. Record behaviour (do not fix) for iPhone HDR/gain-map photos and Live Photos (the `.MOV` half is ignored in v1): what the thumbnail looks like and any warning libvips prints.
7. `docs/native-dependencies.md` and `docs/windows-validation.md`: update the codec table per OS and add an owner step: "decode one of your own iPhone HEIC files (copy, not original) from the installed app on each machine" (no files committed).

## Out of scope (do not do)
- HEIC/HEVC **encoding**, x265, any UI, scanning, database work, signing or notarization.
- Deciding the patent/legal question for distribution.
- Editing `docs/STATUS.md` or `docs/NEXT_STEPS.md`.

## Allowed paths
`.github/`, `scripts/`, `.cargo/`, `src-tauri/` (image_pipeline, smoke entry, build and bundle config only), `tests/fixtures/` (generated HEIC only), `docs/native-dependencies.md`, `docs/windows-validation.md`, `docs/test-fixtures.md`, `README.md`, `reports/`.

## Acceptance criteria
- [ ] Investigation section with the route table and the recommendation, before any build work.
- [ ] CI green on macOS arm64 and Windows x64; both smoke tests decode the HEIC fixture from the installed package with a stripped environment, with real paths printed.
- [ ] `heic_decode_verified: true` in both smoke JSONs, pasted raw from a downloaded artifact.
- [ ] ExifTool reads the HEIC fixture on both OSes.
- [ ] Licence table complete and script-checked; GPL status on macOS stated.
- [ ] SVG/ImageMagick blocking control from 0C still passes.
- [ ] The report lists what is validated on one OS only, and the owner's own-HEIC steps.

## Verification commands
```
gh run list --workflow ci.yml --branch task/0D --limit 3
gh run view <run-id>
gh run download <run-id> -D <folder>
```

## Stop conditions
Stop and report if the cheapest route fails or no route fits CI time, if a required library is GPL, or if either OS cannot decode the fixture. Do not weaken the clean-environment test to make it pass and do not substitute an AVIF fixture for a HEIC one.

## Deliverable
`reports/REPORT-0D.md`. Then stop.

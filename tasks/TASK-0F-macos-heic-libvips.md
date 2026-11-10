# TASK-0F: macOS HEIC decode: own HEVC-decode-only libvips for the packaged app

**Phase:** 0 · **Branch:** `task/0F` (branch from `main` after 0E is merged) · **Depends on:** TASK-0E accepted and merged
**Orchestrator note:** Decisions D10, D11, D13, D14, D17, D18 (see `docs/STATUS.md`). The Windows side is done (0E): our own pinned libvips with libde265 and no x265, stored as a release in the owner's private repo `narayanwaraich/photos-manager-deps`, downloaded by CI with `DEPS_TOKEN`. Do the same for macOS arm64. The macOS package today is a Homebrew harvest: 67 dylibs, floating versions, GPL libraries inside (fftw, imagequant 4.x), no `heifload` at all (0D). Proof means a HEIC made by a real HEVC encoder, decoded by the **packaged .app with Homebrew hidden**, not a green build.
You do not touch secrets, tokens, repositories, releases or settings (AGENTS.md, D17). Where a step needs one, you stop at a checkpoint and the owner does it.

## Read first
- `AGENTS.md`
- `IMPLEMENTATION_SPEC.md` §3.3, §3.4, §17, §18 Phase 0
- `docs/native-dependencies.md` (§4.2 the Windows zip as built in 0E; §4.3 the macOS dylib table and its CI check; §4.4 untrusted loaders)
- `reports/REPORT-0E.md` (how the Windows recipe, audit, checkpoint and CI download were done: copy the pattern), `reports/REPORT-0D.md` section 1.1 (macOS facts: module layout, x265 hard link)
- `deps/win64-vips/` (README, UPSTREAM.txt, audit-zip.sh), `scripts/package-bundle.mjs`, `scripts/` dylib table check, `src-tauri/build.rs`

## Facts established (verify, do not assume)
1. Homebrew's `libheif` (1.23.5 at the time of 0D) hard-links GPL `x265`; its `vips-heif.dylib` is a runtime module that the bundle script never finds. So the Homebrew route cannot give us HEIC without shipping x265. Do not retry it.
2. The Windows zip (0E) has libvips 8.18.6, libheif 1.23.2, libde265 1.1.1, libaom (decode), no x265, no fftw, no imagequant 4.x, no ImageMagick, no poppler. Match those versions on macOS so both OSes behave the same on the same fixtures.
3. The repository is public (D13). Nothing containing libde265 or HEVC code is published as a release of this repo; the built zip is an Actions artifact for 14 days, then a release in the private repo (checkpoint).
4. HEVC patent pools are separate from library licences: fine for personal use, legal check before distribution (Phase 7). Record it; do not decide it here.
5. libvips 8.18 builds `heifload` into the library when configured with `-Dmodules=disabled`; then no runtime module lookup exists to fail.

## Goal
A pinned macOS arm64 libvips distribution with libheif + libde265 (+ libaom decode for AVIF), **no x265, no fftw, no imagequant, no ImageMagick, no poppler, no librsvg**, built by a manually started workflow, stored as a release asset in the private deps repo with a recorded SHA-256, consumed by the app CI, so that the packaged `.app` decodes the iPhone-style HEIC fixtures to an upright, correctly coloured WebP thumbnail with Homebrew hidden, and the dylib table in the docs is checked against it.

## In scope

### Part A: the build recipe and the artifact
1. **Read before building (report section 1).** From the 0E recipe and upstream sources, write down: the exact tarball URL and SHA-256 for libvips 8.18.6, libheif 1.23.2, libde265 1.1.1 and libaom (use the version in the Windows `versions.json`); the CMake options that give libheif **libde265 + aom decoder, no x265, no encoders, no plugin loading, no examples** (`WITH_X265=OFF`, `WITH_LIBDE265=ON`, `WITH_AOM_DECODER=ON`, `WITH_AOM_ENCODER=OFF`, `ENABLE_PLUGIN_LOADING=OFF`, plus whatever else must be off; confirm names in the libheif 1.23.2 `CMakeLists.txt` and quote file:line); the meson options for libvips (`-Dmodules=disabled`, and explicitly `disabled` for fftw, imagequant, magick, rsvg, poppler, openslide, openexr, jpeg-xl, nifti, matio, pdfium, cfitsio, libarchive unless a Windows-zip parity reason says otherwise; `heif=enabled`; keep jpeg, png, webp, tiff, lcms, exif, cgif, highway as in the Windows zip). Do not enable LibRaw or any RAW loader (D15 Tier 2 is a later task).
2. **Recipe in this repo, no vendored source.** `deps/macos-vips/` with `README.md`, `UPSTREAM.txt` (every source URL, SHA-256, the Homebrew formula versions used for leaf libraries, runner image name and Xcode version), `build.sh` (one script that builds libde265, libaom, libheif and libvips from the pinned tarballs into a private prefix, using Homebrew only as the **source of leaf libraries and build tools**: glib, gettext, pcre2, libffi, expat, libjpeg-turbo, libpng, libwebp, libtiff and its codecs, lcms2, libexif, cgif, highway, zlib, zstd, xz, plus cmake, meson, ninja, pkg-config, nasm if needed), and `audit-dylibs.sh`.
3. **Make the zip relocatable and shaped for the app.** Layout `vips-macos-arm64-8.18.6-hevc/{lib,include,lib/pkgconfig,share,versions.json,LICENSES.md}`; every dylib's install name is `@rpath/<file>` and every dependency on another bundled dylib is rewritten to `@rpath/<file>`; after rewriting, ad-hoc `codesign` each file; nothing may reference `/opt/homebrew`, `/usr/local`, `/Cellar`, or the build prefix (check with `otool -L` on every file, paste the check). `versions.json` is generated by the script: component, version, licence, source (tarball or Homebrew formula).
4. **Audit script `audit-dylibs.sh`** (tested on a known-good input and on three known-bad inputs you construct: x265 present, fftw present, a Homebrew path left in): fails when any file name or `otool -L` target contains `x265`, `fftw`, `imagequant`, `magick`, `poppler`, `rsvg`, `openslide`; when `libde265` is missing or `libheif` does not link it; when any non-system absolute path remains; when a licence in `versions.json` is GPL or unknown (use `brew info --json=v2` licence fields for Homebrew leaves and write `unknown, check upstream` rather than guessing). Say plainly in the report which libraries are LGPL and that none is GPL. (Learn from Lesson 6: a string such as `x265` can appear as an enum name inside libvips without meaning linkage; decide rules on linkage and file names, and prove each exemption with output.)
5. **Workflow `.github/workflows/deps-macos-vips.yml`**, `workflow_dispatch` only, `timeout-minutes` set, `permissions: contents: read`, runner `macos-14` (or the arm64 label the app CI uses; record the exact image and Xcode versions): build, audit, then a **test job on a second clean runner** that downloads the artifact, hides Homebrew (`sudo mv /opt/homebrew /opt/homebrew.off`), reduces PATH to `/usr/bin:/bin:/usr/sbin:/sbin`, and with only the zip: `vips -l` lists `heifload`; HEIC fixture to PNG with correct dimensions (upright 240x320 thumbnail, 480x640 copy); the Display P3 fixture; the SVG control (`VIPS_BLOCK_UNTRUSTED=1` must fail, and unblocked SVG must also fail now because no svgload is built: say which). Remember the 0E lesson: in a pwsh/bash step make sure the exit code of the step is the exit code of the last check, and test the failure path. Upload the zip as artifact `vips-macos-arm64-hevc` (`retention-days: 14`) only after audit and test passed; print its SHA-256 and `ls -la`.
6. **Limit:** if the build is not green after **two** workflow runs, stop and report with logs and a table of remaining options (licence, effort, risk). Keep each run under 30 minutes.
7. **Checkpoint 1: stop and tell the owner** the run id and SHA-256. The owner downloads the artifact, creates the release `macos-arm64-vips-8.18.6-hevc1` in `narayanwaraich/photos-manager-deps` and confirms. You then record the SHA-256 and release name in `docs/native-dependencies.md` and continue.

### Part B: use it in the app CI and the bundle
1. `ci.yml` macOS job: download the zip exactly as the Windows job does after the round-4 fix (fail closed for every non-fork run: `DEPS_TOKEN` present, release downloaded with real retries and exit-code checks, SHA-256 matched, `REQUIRE_HEIC=1`). Fork PRs fall back to the current Homebrew path with `heic_decode_verified: false` and `heic_note: "no deps token (fork)"`. Remove the Homebrew libvips install from the non-fork path. Prove fail-closed with a throwaway branch `task/0F-negtest` (wrong release tag), paste `gh run view`, delete the branch.
2. `src-tauri/build.rs` and `scripts/package-bundle.mjs`: build against the downloaded prefix (the way Windows uses `VIPS_LIB_DIR`/`VIPS_DIR`; check what already exists and reuse it); the bundle script harvests from the prefix, keeps the existing "fail loudly" behaviour (0D), and the existing zero-Homebrew-reference checks must still pass. Do not weaken any check.
3. `docs/native-dependencies.md`: rewrite §4.3 (the dylib table) from the new `versions.json` with exact versions and licences, update the table checker if its source list changed, update §4.1 pinned downloads and the macOS part of the risk text. The CI check "bundled dylibs equal documented rows" must pass with the new count; paste both numbers.
4. **Smoke test:** the existing smoke entry already decodes the HEIC fixture and reports `heic_decode_verified`; make the macOS job set `REQUIRE_HEIC=1` for non-fork runs and fail when it is not true. Both OS jobs now require it. Paste raw JSON of `smoke-report-macos-arm64` from a downloaded artifact.
5. `docs/test-fixtures.md` / windows-validation parity: write `docs/macos-validation.md` (short): download the CI artifact zip, unquarantine (`xattr -dr com.apple.quarantine`), run with `PATH` reduced, decode **a copy of one of your own iPhone HEIC files** with the bundled `vips` or the app; do not commit it. Add a step comparing the same fixture on both OSes (dimensions and one sampled pixel).
6. Keep the 0C/0D checks: SVG blocking control, no Homebrew references, zero GPL, the script-checked table.

## Out of scope (do not do)
- Windows changes, ImageIO or any platform decoder, HEIC/HEVC encoding, x265 in any form, RAW loaders, UI, scanning, database work, signing/notarization (ad-hoc signing only), ExifTool's dependence on `/usr/bin/perl` (a separate item).
- Secrets, tokens, repositories, releases, settings (D17).
- Editing `docs/STATUS.md`, `docs/NEXT_STEPS.md`, the spec. `AGENTS.md` only if the owner asks.

## Allowed paths
`.github/`, `deps/macos-vips/`, `scripts/`, `src-tauri/` (build config, smoke entry, loader reporting only), `docs/native-dependencies.md`, `docs/macos-validation.md`, `docs/test-fixtures.md`, `README.md`, `reports/`.

## Acceptance criteria
- [ ] Report section 1 written before any build, with file:line for the options.
- [ ] `audit-dylibs.sh` tested on good and bad inputs (output pasted) and run in the workflow: libde265 present, no x265, fftw, imagequant, magick, poppler, rsvg anywhere; no non-system absolute paths.
- [ ] The test job decodes both HEIC fixtures with Homebrew hidden and only the zip; raw output pasted.
- [ ] Zip is an artifact; after Checkpoint 1 a private release with the SHA-256 recorded in the docs and checked by CI.
- [ ] App CI green on macOS **and** Windows with `heic_decode_verified: true` on both (raw JSON from downloaded artifacts); the negative test shows the macOS job failing closed.
- [ ] §4.3 rewritten, the table check passes, licences cited or marked unknown, GPL status stated.
- [ ] `docs/macos-validation.md` written; the report lists what is validated on one OS only and the owner's own-HEIC steps.

## Verification commands
```
gh workflow run deps-macos-vips.yml
gh run list --workflow deps-macos-vips.yml --limit 3
gh run view <run-id>
gh release view macos-arm64-vips-8.18.6-hevc1 --repo narayanwaraich/photos-manager-deps
gh run list --workflow ci.yml --branch task/0F --limit 3
gh run download <run-id> -D <folder>
```

## Stop conditions
Stop and report if: x265 or any GPL library cannot be excluded; libheif ends up without libde265; the build is not green after two workflow runs; the fixture cannot be decoded with Homebrew hidden; a check would have to be weakened; or you are tempted to use ImageIO, a Homebrew `libheif`, or an AVIF fixture instead. Give the options table; the orchestrator decides.

## Deliverable
`reports/REPORT-0F.md` (use `reports/_REPORT_TEMPLATE.md`; raw outputs, not summaries; honest run log from `gh run list --json`). Then stop.

# Review of TASK-0C

Four rounds. **Round 4 (this one) is the ACCEPT.** Earlier rounds follow in short form.

---

## Round 4 · 2026-11-08

### Verdict: ACCEPT
All six round-3 items are done, with real output, and I verified them against the repo at `20fb0ee` (HEAD) and the last code commit `1932e66`. CI run 42439046831 (code commit) and run 42463475881 (HEAD, three green jobs, six artifacts per your screenshot) are green. The owner's Windows laptop passed the installer test.

### What I verified
- **Item 1 (§4.2):** rewritten from `versions.json`; the 28 components, versions and DLL mapping match what I read in the zip myself; false claims about `manifest.txt` and `licenses/` removed; raw `unzip -l`, `versions.json` and `strings ... | grep -ci de265` (0) pasted. Licence URLs are cited per component; I could not open them from my environment, so they are not independently checked (see notes).
- **Item 2:** `scripts/check-dylib-table.mjs` is correct and runs in the macOS job after the unzip; CI log shows 67 files = 67 rows. The 65-versus-67 difference is explained (the owner's Mac linked 65; the CI runner also pulled `brotli`, two dylibs).
- **Item 3:** CLI control added to both jobs, with failure paths; raw output on both OSes shows unblocked success and `VIPS_BLOCK_UNTRUSTED=1` failure with "not a known file format". §4.4 now says `has_svgload` is a constant and the extra in-app block calls are untested redundancy.
- **Item 4:** measured: `exiftool.exe` is a single process (no child in `-stay_open` mode). The Job Object plan is gone from the report. Good use of a measurement.
- **Item 5:** real diffstats; `gh run view` for both runs; the "21.8 MB" question is explained correctly: the Actions page shows the outer artifact zip (22,893,523 bytes = 21.83 MiB) around the 23,078,148-byte inner zip. (My guess that this meant a different build was wrong.) The two runs' inner zips have different checksums with identical file lists; the agent attributes this to timestamps.
- **Item 6:** all 38 installed DLLs scanned, none imports the VC++ runtime; the scan's limit (the ExifTool Perl DLLs were not scanned) is stated honestly. My own `objdump` of the same DLLs agrees.
- Checks 1–10 of REVIEW_CHECKLIST §2: same results as before, production hits unchanged (`lib.rs:55, 64, 78`; `vips.rs:14`). `pnpm verify` passes on your Mac (18 tests).
- Your own tests: Windows laptop installer test passed (twice); your Mac decoded a real iPhone HEIC with Homebrew's libvips (input for TASK-0D).

### Non-blocking notes (tracked in STATUS risk 19)
- Report details: §6 still shows the smoke JSONs from the older run 42369216568, and the Round 2 text still says "65 rows". Both are history and do not affect acceptance.
- The macOS dylib check will fail whenever Homebrew changes the dependency set (it is working as designed; update the table then). The macOS build is not reproducible.
- The agent explains the checksum difference of two same-code macOS zips by timestamps; the 65-versus-67 difference shows Homebrew floats too. Both go away with the TASK-0D own build.
- Windows: no test yet that a timeout kills `exiftool.exe` (single process measured; add a Windows variant of the timeout test in Phase 1).
- Licence URLs in §4.2 were not independently opened by me; spot-check before any distribution.
- Housekeeping: Project docs were committed on the task branch (`docs/STATUS.md`, `reports/REVIEW-0C.md`, `tasks/TASK-0D-heic-decode.md`, and a stray `docs/LEARNING copy 2.md`). The merge steps in NEXT_STEPS remove the stray file and sync the real docs afterwards.
- Everything carried over from round 2 (init `.expect()`/`set_var`, `runCmd` swallowing errors, CI pins, `--smoke-test` in the production binary, installer `.gitkeep`) is on the STATUS carry-forward list.

### Owner decisions recorded
Identifier `com.narayanwaraich.photosmanager`; HEIC on both OSes is TASK-0D (next); full Windows checklist after 0D.

---

## Round 3 · 2026-11-08 (history; fixed in round 4)

### Verdict: FIX (round 3: documentation and evidence, plus two small CI additions)
The product work is done and holds up on both OSes, including your own Windows laptop. What blocks ACCEPT is not code: the Windows dependency list in the docs was **not** taken from the zip (the report says it was), and two claims have no evidence behind them. Six items, all small; after this I expect to accept.
- Scope: ok, with one nit (a stray `docs/LEARNING copy.md` was committed).
- Safety invariants: ok. Check 6 output in the report is raw and includes the test-module hits; production hits are `lib.rs:55, 64, 78` and `vips.rs:14` (same as before, nothing new). Groups 1–4, 7–10 empty; group 5 is `smoke.rs:234` plus two test-only copies.
- Evidence: mostly real now; three exceptions (items 1, 4, 5).

### What I looked at
- Your attachments: REPORT-0C.md (round 2), `git log` (HEAD `ab96f03`), `gh run list`, `pnpm verify` on the Mac (18 tests pass), the Actions page for run 42375063888 (six artifacts), your Windows laptop log of the installer test (twice), and your `vips -l | grep -i heif` output.
- The repo at `ab96f03`: `ci.yml`, `.cargo/config.toml`, `deny.toml`, `tauri.conf.json`, `build.rs`, `vips.rs`, `smoke.rs`, `native-dependencies.md`, `windows-validation.md`.
- **New this round: the zip itself.** I downloaded `vips-dev-x64-web-8.18.6.zip` (the URL in ci.yml) and read its contents, ran `strings` on `libheif.dll`, and ran `objdump -p` on its 38 DLLs. I also cloned the Windows-bindings fork and the upstream repository and re-ran the diffstat.
- Not seen: the Actions job logs and the artifacts' contents (I have no API access; run facts rest on your screenshots and the report's quoted output).

### What holds up (verified)
- **Your laptop: the installer test passed, twice** (artifacts from two runs): exit code 0, `status: ok`, `libvips-42.dll` loaded from `C:\pm-test`, ExifTool bundled, 240×320 thumbnail, 74 DLLs installed, PATH stripped. The first failure was a stale artifact I sent you to.
- Items 1, 2, 3, 5, 6, 7: done and checked in the repo. Artifacts are exactly the macOS zip and the Windows installer (`--bundles app` / `--bundles nsis`); the macOS smoke test unzips the uploaded zip; `windows-validation.md` is rewritten around the installer and has the variables, `CARGO_BUILD_JOBS=2` and the `rustup default` removal; `build.rs` now finds the link directory from `VIPS_LIB_DIR`, `VIPS_DIR` or `C:\vips\lib`; `deny.toml` has `unmaintained = "all"` and passes with no ignores; the identifier is `com.narayanwaraich.photosmanager`.
- **Static CRT works:** `.cargo/config.toml` has `+crt-static`; the CI `dumpbin /dependents` of the installed exe shows no VCRUNTIME/MSVCP import. I added my own check of the 38 libvips DLLs from the zip: they import only `api-ms-win-crt-*` (the Windows Universal CRT), `KERNEL32` and a few system DLLs. So the Visual C++ Redistributable is not needed for these. (My round-2 item 3 was right to ask.)
- **Fork diffstat is exact:** `git diff --stat 1ca5b35 80097ad` in the fork gives the same five files (35 insertions, 14 deletions) that the report pastes, and `1ca5b35` is the real merge-base with upstream.
- The Windows build really has no HEVC decoder: `libheif.dll` has HEVC container code but no libde265 (`strings` finds none), and `bin/` has no libde265, x265 or dav1d DLL. The report and docs now say HEIC is unverified on both OSes. Correct.
- SVG fixture is now required (no temp-file fallback), and the message is checked.
- The Windows long-path setting is recorded: `LongPathsEnabled = 0x1` on the CI runner and on your laptop.
- `perl_path` on macOS is now read from the bundled ExifTool's shebang line.

### Findings (all block ACCEPT; numbers match the fix prompt)
1. **Section 4.2 (Windows dependency list) is not from the zip.** The report says it was "updated with real content from `manifest.txt` and `licenses/`". The zip has neither. It contains `versions.json`, `LICENSE` (the LGPL text), a generic `README.md`, `ChangeLog`, `bin/`, `lib/`, `include/`, `etc/`, `share/`. Almost every version in 4.2 is wrong: libheif 1.18.2 (zip: 1.23.2), aom 3.9.1 (3.14.1), librsvg 2.40.21 (2.62.91), glib 2.82.2 (2.89.4), harfbuzz 9.0.0 (14.3.1), libwebp 1.4.0 (1.6.0), libpng 1.6.44 (1.6.58), zlib 1.3.1 (zlib-ng 2.3.3); `libspng` is not in the zip and the JPEG library is mozjpeg, not libjpeg-turbo. The first draft of 4.2 listed libheif 1.18.2 and libde265; round 2 removed libde265 but kept the rest of those numbers. The licence lines have no source either (the zip ships no per-component licence files). This is a provenance document: a wrong list is worse than a missing one. Rewrite from `versions.json` only, and strike the false sentence from the report.
2. **Section 4.3 is correct by assertion, not by check.** Round 2 asked for a script and a CI failure on a missing row. The document now says "65 rows = 65 files" but nothing checks it, and the earlier zip listing said 67 entries (probably non-dylib entries, unexplained).
3. **The SVG probe cannot tell "blocked" from "no SVG loader".** libvips says "is not a known file format" in both cases, and the probe, the cargo test and the smoke JSON all accept it. `has_svgload: false` is a hard-coded constant. The probe proves "this SVG cannot be loaded", which is the safe outcome but not the reason. A control with the stock `vips` CLI (unblocked loads the fixture; `VIPS_BLOCK_UNTRUSTED=1` fails) closes this without any app code. Round 2 also asked which blocking call is actually needed; it was not answered.
4. **The Windows ExifTool kill claim has no evidence.** The report states as fact that `exiftool.exe` is "a self-extracting wrapper that spawns an internal Strawberry Perl child" and that "a Job Object will be implemented in Phase 1 / 3". Nothing measures it. It may be one process (then killing it is enough) or a launcher with a child (then a timeout leaks a process). Say "unverified" or measure it.
5. **Report evidence gaps.** The header diffstat is a placeholder (`reports/REPORT-0C.md | ...`) instead of the real output; no raw `gh run view` is pasted (round 2 item 9a); and the macOS zip is 23,078,148 bytes in the code run but the Artifacts page of the later docs-only run shows 21.8 MB, which probably means the macOS build is not reproducible (Homebrew formulas float), worth confirming. A stray `docs/LEARNING copy.md` was committed.
6. **VC++ check covers only the exe.** I checked the 38 libvips DLLs myself on Linux and they are clean; the Windows CI should do the same for every installed DLL so the proof is on the real machine type.

### Non-blocking notes (unchanged from round 2 unless stated)
- `vips.rs` `.expect()` on init and `std::env::set_var` inside the lazy init: move to app start in Phase 1.
- `package-bundle.mjs` ignores failures of `install_name_tool`, `codesign`, `ditto` (`runCmd` returns null): make them throw.
- CI hygiene: floating `dtolnay/rust-toolchain@stable` ref, Node 20 action deprecation warnings (annotation on the Actions page), `ubuntu-latest` moves to Ubuntu 26 on 19 Oct 2026 (already passed; the label has moved). Pin and bump in Phase 7.
- `--smoke-test` ships in the production binary: gate it behind a Cargo feature before Phase 7.
- macOS ExifTool needs `/usr/bin/perl` (documented risk, accepted for Phase 0).
- GPL libraries in the macOS Homebrew harvest (fftw, imagequant 4.x): personal use only until the TASK-0D build replaces them.
- The installer ships a `.gitkeep` file next to the DLLs (seen in your laptop listing). Cosmetic; remove the placeholder from the bundle resources when convenient.
- Mac HEIC: your `vips -l` shows `heifload` with Homebrew, so Homebrew's libvips has the HEIF module on your machine. Whether it decodes an actual iPhone HEIC (HEVC needs libde265) is the next measurement (NEXT_STEPS Step 2).

### Round 3 ACCEPT conditions
Items 1–6 done; CI green on the last code commit; the 4.2 list matches `versions.json`; the diffstat and `gh run view` outputs are real. No new laptop test is needed unless the packaging changes (it should not).

### Fix prompt (paste into the 0C Antigravity conversation, branch task/0C)
```
Review feedback for TASK-0C (round 3: documentation and evidence, plus two small CI additions). First read AGENTS.md, tasks/TASK-0C-ci-and-packaging.md and reports/REPORT-0C.md. Work on branch task/0C only (push task/0C to run CI; never push main, never force-push). Allowed paths for this round: .github/, scripts/, docs/native-dependencies.md, docs/windows-validation.md, reports/, and deleting docs/"LEARNING copy.md". No other code changes. Do NOT add HEIC/HEVC support and do not start TASK-0D. Do not weaken any check to get green; if something cannot be made to pass, stop and report why. Add a "Round 3" part to the "Fixes" section of reports/REPORT-0C.md. Every pasted command output must be raw (no hand editing, no "..." placeholders); if you shorten it, say so.

1. Section 4.2 of docs/native-dependencies.md is not from the zip, and the report says it is. The reviewer downloaded vips-dev-x64-web-8.18.6.zip (the URL in ci.yml) and looked inside. It contains one folder, vips-dev-8.18/, with bin/ (38 DLLs plus vips.exe, vipsedit.exe, vipsheader.exe, vipsthumbnail.exe), lib/, include/, etc/, share/, ChangeLog, LICENSE (the LGPL-2.1 text), README.md (libvips's generic README, not a build note) and versions.json. It has NO manifest.txt and NO licenses/ folder, so the report line "real content from manifest.txt and licenses/" is false and must be removed. The real component list is versions.json, which reads (reviewer's copy): aom 3.14.1, archive 3.8.9, cairo 1.18.4, cgif 0.5.3, exif 0.6.26, expat 2.8.3, ffi 3.8.0, fontconfig 2.18.3, freetype 2.14.3, fribidi 1.0.16, glib 2.89.4, harfbuzz 14.3.1, heif 1.23.2, highway 1.4.0, imagequant 2.4.1, lcms 2.19.1, mozjpeg 0826579, pango 1.58.2, pixman 0.46.4, png 1.6.58, proxy-libintl 0.5, rsvg 2.62.91, tiff 4.7.2, uhdr 2.0.2, vips 8.18.6, webp 1.6.0, xml2 2.15.3, zlib-ng 2.3.3. Almost every version currently in section 4.2 differs from this (for example libheif 1.18.2, aom 3.9.1, librsvg 2.40.21, glib 2.82.2, harfbuzz 9.0.0, zlib 1.3.1, and libspng and libjpeg-turbo, which are not in the zip: the JPEG library is mozjpeg). Do it yourself, do not copy my list: download the zip, paste the raw `unzip -l` (top-level entries) and the raw versions.json into the report, then rewrite section 4.2 from versions.json only: one row per component with the exact version and the DLL(s) in bin/ that belong to it. Delete every version that is not in versions.json. The zip ships no per-component licence files: give a licence only where you can cite an upstream licence file or package page (put the URL in the cell); otherwise write "not shipped in the archive; check upstream". Keep these facts, but only the ones you can see in the zip: bin/ has no libde265, x265 or dav1d DLL; libheif.dll contains HEVC container code but no HEVC decoder (run `strings libheif.dll | grep -ci de265` and paste the count); librsvg-2-2.dll is present; libimagequant is 2.4.1. Fix the matching sentences in the report (sections 1, 5 and 7) and in docs/windows-validation.md if it repeats any of this.

2. Section 4.3 (macOS dylib table) must be checked by a script, not by a sentence. Add scripts/check-dylib-table.mjs (or a function in package-bundle.mjs): it lists every *.dylib in Contents/Frameworks of the .app that the smoke test unzipped, reads the table rows from docs/native-dependencies.md section 4.3, and exits 1 if any dylib has no row or any row has no dylib (print both counts). Run it in the macOS job after the unzip. Paste the raw output from the CI log. The document says 65 rows and 65 files; the earlier report said the zip listed 67 entries: reconcile the numbers (`unzip -l photos-manager-macos-arm64.zip | grep -c '\.dylib$'`) and say what the other entries were. Generating the table from `brew info --json=v2` is still welcome but not required this round.

3. The SVG probe cannot yet tell "blocked" from "no SVG loader". libvips gives the same message, "is not a known file format", in both cases, so the probe and the cargo test prove only that this SVG cannot be loaded. Add a control to both jobs using the stock vips command-line tool, with no app code involved: Windows uses C:\vips\bin\vips.exe (the zip's own); macOS uses the Homebrew `vips`. (a) Without VIPS_BLOCK_UNTRUSTED: `vips copy tests/fixtures/generated/fixture_blocked.svg <temp>.png` must succeed. If it fails, the job fails (the control is broken and the probe means nothing). (b) With `VIPS_BLOCK_UNTRUSTED=1` the same command must fail with a message containing "not a known file format". Paste the raw output of both steps from both OSes. Then add to section 4.4: `has_svgload: false` in the smoke JSON is a constant (is_loader_supported returns false for svgload/magickload before asking libvips) and is not evidence; the blocked probe plus this control is the evidence; and say whether the environment variable alone is enough in the CLI test (it is if (b) fails as above) and that the extra vips_block_untrusted_set and vips_operation_block_set calls in the app are untested redundancy (say "untested" unless you test them).

4. The Windows ExifTool statement has no evidence. Report section 8 and section 7 say exiftool.exe "is a self-extracting wrapper executable that spawns an internal Strawberry Perl child process" and that a Job Object "will be implemented in Phase 1 / 3". Nothing in the report measures this. Either reword it as unverified ("not measured: it is unknown whether exiftool.exe on Windows is one process or a launcher with a child; if it is a launcher, a timeout could leave a child running"), or measure it in the Windows job: start the installed resources\exiftool\exiftool.exe with `-stay_open True -@ -` and standard input kept open, wait two seconds, print `Get-CimInstance Win32_Process | Where-Object { $_.ParentProcessId -eq $p.Id } | Select-Object Name,ProcessId`, then stop it and paste the raw output. Do not claim a Job Object plan in the report; the reviewer decides the follow-up.

5. Report evidence. (a) The header diffstat is the placeholder `reports/REPORT-0C.md | ...`. Paste the real raw output of `git diff --stat <last code commit> HEAD` (this round's last code or CI commit will be a new one: use it). (b) Paste the raw `gh run view <id>` (all three jobs with durations) for the CI run of the last code commit AND for the CI run of the final HEAD. (c) The macOS zip is 23,078,148 bytes in run 42369216568 (your listing) but the Artifacts page of run 42375063888 (same code, docs-only commit) shows 21.8 MB for the macOS artifact: download both with `gh run download`, give the size and `shasum -a 256` of the macOS zip from each, and state whether the contents differ. If they do, record that the macOS build is not reproducible because Homebrew formula versions are not pinned (the smoke JSON says libvips 8.18.7), and list which dylib versions differ if you can. (d) Delete docs/"LEARNING copy.md" (a stray duplicate) with `git rm`.

6. Visual C++ runtime, last step. The dumpbin step covers only photos-manager.exe. Extend it to every top-level *.dll in the installed folder: print any DLL that imports VCRUNTIME*, MSVCP*, VCOMP* or CONCRT* (expected: none) and the count of DLLs scanned. (The reviewer ran `objdump -p` on the 38 libvips DLLs from the zip on Linux and found only api-ms-win-crt-*, KERNEL32 and a few system DLLs, no VC++ runtime; your CI output is the Windows confirmation. State that the ExifTool Perl runtime under resources\exiftool\exiftool_files was scanned too, or say it was not.)

When all six are done: run `pnpm verify`, push task/0C, wait for CI (the Windows job takes about 26 minutes), update the report, then stop. Do not start TASK-0D or anything else.
```

### Sources
- libvips build-win64-mxe release `v8.18.6`, asset `vips-dev-x64-web-8.18.6.zip` (contents inspected directly): https://github.com/libvips/build-win64-mxe/releases/tag/v8.18.6
- Fork and upstream bindings (diffstat re-run): https://github.com/kuma-00/libvips-rust-bindings-for-windows and https://github.com/olxgroup-oss/libvips-rust-bindings
- libvips `VIPS_BLOCK_UNTRUSTED` / `vips_block_untrusted_set`: https://www.libvips.org/API/current/func.block_untrusted_set.html

---

## Round 2 · 2026-11-07 (short form)
Verdict FIX, nine items. Outcome in round 3: items 1 (artifacts), 2 (windows-validation), 3 (VC++ via static CRT), 5 (deny), 6 (identifier), 7(b–d, text only; §4.2 invented: see round 3 item 1), 8(b, c) done; 4 (SVG) and 9 (evidence) partly done; 8(a) (script-checked table) not done. Also found in round 2: the report's "Windows decodes HEIC" claim was false (no HEVC decoder in the zip; the macOS bundle lacks `heifload`, probably a module not harvested); my round-1 review had repeated that claim. The owner decided the identifier `com.narayanwaraich.photosmanager`, and HEIC on both OSes became TASK-0D (proposed). Owner's laptop: `pnpm verify` and `pnpm tauri dev` pass; first installer test used a stale artifact and failed with a missing DLL (exit -1073741515); two retests with newer artifacts passed.

## Round 1 · 2026-11-06 (short form)
Verdict FIX. Nine items: smoke test from the build folder not the package; Homebrew fallback not ruled out; svgload allowed through a `PHOTOS_DEV_ALLOW_SVGLOAD` bypass; no licence/provenance inventory and no pinned checksums; macOS ExifTool and system Perl; Windows ExifTool tests only shown on macOS; floating toolchain/action pins; undocumented git fork of the libvips bindings; report accuracy. Owner's round-1 answers: Windows laptop Acer Swift SF314-43 (Ryzen 7 5700U, 8 GB, Windows 11); cloud AI not a priority; defaults accepted for the rest.

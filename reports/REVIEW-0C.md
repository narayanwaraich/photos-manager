# Review of TASK-0C (2026-11-06)

Evidence reviewed: REPORT-0C.md, `git log` of task/0C, screenshot of the Actions workflow list, the owner's answers to questions 5 and 6 (the rest use defaults).
Not seen: `ci.yml`, `deny.toml`, any source file, `docs/native-dependencies.md`, `docs/windows-validation.md`, the uploaded artifacts, the Windows `pnpm verify` log, the REVIEW_CHECKLIST §2 `rg` output.
Checked against sources: what `windows-latest` is, and ExifTool's official Windows package (see Sources).

## Verdict: FIX (round 1)
Not accepted yet; the Phase 0 gate stays open.
Scope: ok on the evidence (no product features; the smoke entry is the only new entry point).
Safety invariants: nothing violated on the evidence (this task has no file operations).
Evidence: partly missing, and one check was weakened (finding 3).

## What the evidence supports
- Run #9 (commit bd95aad) is green. The report's durations (macOS 5m7s, Windows 22m58s, cargo deny 49s) agree with the screenshot's 23m 2s total.
- Smoke JSON on both OSes: libvips loaded from inside the macOS `.app/Contents/Frameworks` and from next to the exe on Windows, ExifTool bundled, the orientation-6 fixture gives an upright 240x320 thumbnail, PATH stripped.
- A real Windows problem (non-ASCII paths through the command line) was found and solved sensibly: paths go to ExifTool through stdin (`-@ -`).
- The eight red runs before the green one are ordinary fix iterations; the commit messages say what each fixed.
- 17 tests pass on the owner's Mac, including the child-process-killed test from 0A.

## Findings (all block ACCEPT)
1. **The smoke test ran from the build folder, not from a package.** Both JSONs show paths under `src-tauri/target/release/...`. On Windows the DLLs are "adjacent to the EXE" in `target/release`, copied by `package-bundle.mjs` after the build. It is not shown that the uploaded installer/dmg contains them. The task's point is a package that runs on a machine with nothing installed.
2. **macOS: Homebrew fallback not ruled out.** Stripping PATH does not stop the loader from following an absolute `/opt/homebrew/...` library path. The evidence needed is `otool -L` output with no Homebrew paths, and a run with Homebrew hidden on the CI runner.
3. **svgload is present and the check was weakened.** TASK-0C (known issue 2) said not to ship `svgload` or `magickload`. Both JSONs list `svgload`, and `disallowed_loaders_absent` is `true` only because commit bd95aad added a `PHOTOS_DEV_ALLOW_SVGLOAD` bypass. REVIEW_CHECKLIST §4: a fix that weakens a safety check to get green is rejected. Report section 7 says "Deviations: None", which is not accurate.
4. **No licence or provenance inventory for what is bundled.** On macOS, 65 libraries were harvested from Homebrew; the task asked for each library's licence (and no GPL x265). Downloaded archives (libvips Windows zip, ExifTool) have no pinned SHA-256. The report names "SourceForge / Oliver Betz" for the Windows ExifTool, while ExifTool's official Windows package is the zip on exiftool.org. ExifTool versions differ by OS (13.55 vs 13.61).
5. **macOS ExifTool still depends on system Perl, unaddressed.** Known issue 3 asked to check `/usr/bin/perl` on macOS 27 and prefer a bundled Perl. The report is silent.
6. **Windows ExifTool non-ASCII and >260-character tests are shown only on macOS.** The 17-test log is from the Mac. The Windows job's log is needed (and the long-path test must really exercise a long path on Windows).
7. **Pins.** `rust-toolchain.toml` says `channel = "stable"`, which floats; spec §17.3 says pinned. The report's concept 5 states the opposite. `cargo-deny` is "0.20+" and the action is a moving `@v2` tag.
8. **Windows libvips binding is a personal git fork** (`kuma-00/...` at one commit). Reasonable as rung 1 of the spec §3.3 ladder and recorded as a decision, but its licence, age, diff size and whether it applies to macOS too are not stated, and `deny.toml` needs a git-source allowance.
9. **Report accuracy and hygiene.**
   - Header commit `5f73fc3` is not in the branch log (HEAD is `0a3e075`); this was raised in the 0A review.
   - The pasted Windows JSON has an invalid escape (`System32\Wbem`), so it was edited by hand rather than copied from the artifact.
   - `windows-latest` is Windows Server 2025 (migration announced for Sept 2025), not "Server 2022 / Windows 11".
   - `has_homebrew_in_path` is meaningless on Windows; the useful check is that libvips, exiftool and perl are not found on PATH.
   - The cited run is #9; HEAD (#10, docs only) was still running in the screenshot, and no per-job page was shown.
   - ONNX Runtime bundling notes (task items 5 and 9) are not mentioned.
   - The REVIEW_CHECKLIST §2 greps were not run.

## Non-blocking notes
- `--smoke-test --out <file>` ships inside the production binary. Acceptable for Phase 0; gate it behind a Cargo feature before Phase 7.
- Concept 2 in the report says the ANSI conversion happens in the C runtime of the caller. Rust passes UTF-16 to Windows, so the lossy step is most likely inside ExifTool's own Perl runtime. The fix (paths via stdin) is right either way; the explanation to the owner should say "most likely".
- The HEIC difference (Windows build has `heifload`, macOS does not) is real. It does not block Phase 1 (scanning only classifies by extension). See Decisions.

## Fix prompt (paste into the 0C Antigravity conversation, branch task/0C)
```
Review feedback for TASK-0C (round 1). First read AGENTS.md, tasks/TASK-0C-ci-and-packaging.md and reports/REPORT-0C.md. Fix only the items below on branch task/0C. You may push task/0C to run CI; never push main, never force-push. The root files rust-toolchain.toml and deny.toml are allowed for this fix. Do not weaken any check to get green: if something cannot be made to pass, stop and report why. Add a "Fixes" section to reports/REPORT-0C.md.

1. Test the package, not the build folder. The smoke test must run from a freshly installed or extracted copy of the exact artifact you upload, in a new folder outside src-tauri/target, with PATH reduced to the OS basics. The JSON must show library and ExifTool paths inside that new folder. Windows: install or extract the bundle that `tauri build` produced. macOS: copy the produced .app (or the contents of the .dmg) to a new folder. List the contents of every uploaded bundle artifact in the report (unzip -l or equivalent). If the libvips libraries, ExifTool or fixtures are copied next to the executable only AFTER the Tauri bundler ran, they are not inside the installer/dmg: change the packaging so the uploaded artifact contains them (check the current Tauri 2 docs for bundle resources), then smoke-test that artifact.
2. macOS: prove nothing comes from Homebrew. Print `otool -L` for the main binary and every bundled .dylib, filtered for /opt/homebrew and /usr/local (expect no lines). Then, IN THE CI WORKFLOW ONLY (never on the owner's Mac), run the smoke test with Homebrew hidden (for example `sudo mv /opt/homebrew /opt/homebrew.off`); if sudo is not available on the runner, say so and give DYLD_PRINT_LIBRARIES output instead. Paste the result.
3. svgload. Remove the PHOTOS_DEV_ALLOW_SVGLOAD bypass. The smoke test must fail if svgload or magickload can run. Preferred: a libvips build without librsvg and ImageMagick. If not possible now, block untrusted loaders at start-up with libvips's own blocking (the libvips docs list vips_block_untrusted_set and the VIPS_BLOCK_UNTRUSTED environment variable; check that the Rust binding exposes the function or that the generated bindings do), and add a smoke check that tries to load a generated SVG file and requires that it fails. State which option you used.
4. Provenance and licences. In docs/native-dependencies.md: macOS: a table of every bundled .dylib with version and licence taken from package-manager metadata (not from memory); stop and report if any is GPL rather than LGPL or permissive. Windows: summarise the manifest and licence files inside the vips-dev-w64-web zip. For every downloaded archive (libvips zip, ExifTool): exact URL, version and SHA-256, and make CI verify the SHA-256 and fail on mismatch. Use the official ExifTool packages from exiftool.org on both OSes, the same version on both (the report has 13.55 on macOS and 13.61 on Windows).
5. macOS ExifTool and Perl. Report whether /usr/bin/perl exists on macOS 27 (`ls -l /usr/bin/perl` and `/usr/bin/perl -v`) and whether the bundled exiftool relies on it. Add the Perl path used to the smoke JSON. If it relies on system Perl, record that as a risk in docs/native-dependencies.md.
6. Windows tests. Paste from the Windows job's verify log (artifact verify-logs-windows-x64) the lines showing test_exiftool_non_ascii_filename and test_exiftool_long_path running and passing on Windows, and the actual path length used. If either is skipped on Windows, say so and fix the test.
7. Pins. rust-toolchain.toml: `channel = "stable"` is not a pin. Use the exact version `rustc --version` reports on your machine, with the rustfmt and clippy components. Pin cargo-deny to an exact version and pin the action by commit SHA after checking it. Paste the [advisories] and [sources] sections of deny.toml: no advisory may be ignored without a written reason, and the allowed git sources must be only the one libvips fork.
8. libvips Windows binding fork. In docs/native-dependencies.md record: fork URL and pinned commit, its licence file, date of its last commit, a `git diff --stat` summary against the upstream libvips 2.3.0 bindings, whether the fork is used only on Windows (cfg) or on every OS, and the alternatives (upstream fix, our own bindings).
9. Report corrections. Header commit from `git rev-parse --short HEAD`. The windows-latest runner is Windows Server 2025 according to GitHub's changelog: verify what the job log says and state it. Replace the Windows smoke JSON by the real file (the pasted one has an invalid escape in path_var), for example `gh run download <run-id> -n smoke-report-windows-x64`. Add a Windows check that libvips, exiftool and perl are NOT found on PATH (has_homebrew_in_path means nothing on Windows). Cite the CI run for the final HEAD commit with a per-job listing. List real deviations in section 7 (at least: svgload handling, the libvips fork, the ExifTool stdin change, Homebrew-sourced macOS libraries). Point to the section of docs/native-dependencies.md that covers ONNX Runtime bundling (task items 5 and 9) or add it. Paste the output of the REVIEW_CHECKLIST section 2 `rg` commands (single quotes around the unwrap/expect pattern). Correct concept 5 (rust-toolchain) and word concept 2 as "most likely inside ExifTool's Perl runtime".
```

## Owner's answers (2026-11-06) and what they change
- **Windows PC:** Acer Swift SF314-43, Ryzen 7 5700U, 8 GB RAM, integrated Radeon (496 MB graphics), Windows 11. It is the low-end reference machine for Phases 2 and 4B (spec §15 currently names no reference machines). A first Rust build may be slow or memory-tight on 8 GB; if the compiler runs out of memory, limit parallel jobs (`CARGO_BUILD_JOBS=2`).
- **Embedding time:** 0B measured about 1 s per image at budget 140 on the M4. This laptop will probably be several times slower (not measured). "Embed everything" must not be the default; opt-in per root, resumable, with an ETA (already proposed after the 0B review). 0B2 should run the ONNX probe on this laptop, including whether DirectML helps on the integrated Radeon.
- **Cloud AI:** not a priority now. Plan: small local models and/or letting users connect their own accounts. In practice "link your own account" means the user pastes their own API key (spec §9.3, key in the OS credential store) or points to a local OpenAI-compatible endpoint (Ollama, LM Studio). A real "sign in with provider" login needs a separate app registration per provider and is not planned unless the owner asks. Phase 6 can shrink to that and may be scheduled after Phase 7.
- **Defaults accepted for:** personal use first, library up to 100k photos on internal/external SSD, JPEG + iPhone HEIC (RAW later), XMP sidecars and other managers ignored and never modified, app name "Photos Manager".

## Decisions needed from the owner
1. **App identifier** (default: placeholder `com.photosmanager.desktop` stays). Reviewer recommends choosing the real one before Phase 1: the app-data folder (database, cache) is derived from it, so changing it later moves the catalogue. For personal use it need not be a domain you own; for distribution it should be.
2. **HEIF on macOS** (the agent's question 1): no decision needed for Phase 1. iPhone HEIC is a main format, so both OSes need it by Phase 2. Rely-on-Apple-ImageIO on macOS only would break "same behaviour on both OSes" (spec §1.1 principle 8) and is outside the §3.3 ladder, so it would be a spec change. Reviewer's lean: one task before Phase 2 to produce our own minimal libvips build for both OSes (HEIF/AVIF decode only, no librsvg, no ImageMagick, licences inventoried); this also resolves findings 3 and 4 for good. Decide when that task is written.
3. **Windows testing schedule** (the agent's question 2): quick artifact test now (NEXT_STEPS step 1), full `docs/windows-validation.md` pass on the final artifact before the Phase 0 gate is ticked. Phase 1 waits for the gate.

## Status updates
See docs/STATUS.md: 0C is FIX; pending owner decisions and pending spec edits listed.

## Next task
Wait for the fix round. After 0C is ACCEPTED and the Windows pass is done: tick the Phase 0 gate, write TASK-0B2 and the first Phase 1 task, and issue spec v2.1 (pending edits are listed in STATUS).

## Sources
- GitHub changelog, windows-latest migration to Windows Server 2025: https://github.blog/changelog/2025-07-31-github-actions-new-apis-and-windows-latest-migration-notice/
- ExifTool install page (Windows executable zip with `exiftool(-k).exe` and `exiftool_files`; the Windows installer by Oliver Betz is a separate package): https://exiftool.org/install.html
- libvips `vips_block_untrusted_set` and `VIPS_BLOCK_UNTRUSTED` (8.17 docs; the page does not say since which version): https://www.libvips.org/API/8.17/func.block_untrusted_set.html

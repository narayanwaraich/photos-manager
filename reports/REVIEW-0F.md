# Review of TASK-0F (round 1: stopped at the two-run gate)

Date: 2026-11-11 · Branch reviewed: `task/0F` at `d3a4535` (last code commit `631a85b`)

## Verdict: FIX. The stop was correct; 0F continues with Option 1, widened (at most two more runs).

The agent stopped after two runs, exactly as the task said, and the library itself looks good: run 42806945658 built and audited the zip (26 Mach-O files, 16 components, no GPL, libde265 linked into libheif), and the clean-Mac test then printed `VipsForeignLoadHeifFile (heifload)` with Homebrew hidden. What failed is a shell check in the test job. Reading the scripts I found the same kind of shell construct in places that matter more than a red run: the check that no `svgload` or `magickload` is present, and the x265 gates of both audit scripts (macOS, and Windows from 0E). Those can fail open. Nothing unsafe has shipped: there is no macOS release yet, and the Windows zip was built and audited in CI and contains no x265 file.

## What I looked at
- `reports/REPORT-0F.md` at `d3a4535`, and the agent's closing message (screenshot of its options table).
- Your raw output: `gh run list --branch task/0F --limit 4`, `gh run view` of CI run 42806945665, and two more screenshots (Actions list; run summary with its artifacts).
- The files on `task/0F` at `d3a4535`, cloned read-only from the public repo: `deps-macos-vips.yml` (all 231 lines), `audit-dylibs.sh` (all 272), the option lines, licence mapping and checks in `build.sh` (not all 769 lines), `UPSTREAM.txt`, the README, and `deps/win64-vips/audit-zip.sh` (grep only).
- Upstream: libvips 8.18.6 `meson.build` and `meson_options.txt` (to check the cgif claim).
- Two small experiments (findings 1 and 2). Nothing from the repo was executed.
- Not seen: the run logs beyond the report's excerpts, `build.sh` outside the parts named, and I did not re-run `pnpm verify`.

## What is confirmed
1. **The run log matches your raw output.** Runs 42806945658 (X, 3m14s) and 42804778977 (X, 3m35s), commits `631a85b` and `f9cb8aa`; the screenshot times 4:36 PM and 4:52 PM IST are 11:06 and 11:22 UTC, as in the report's JSON. Unlike 0E, the run log needed no correction.
2. **Order and scope.** `5fd6cab` (report section 1.1) comes before `f9cb8aa` (recipe and workflow). Over all four commits only `.github/workflows/deps-macos-vips.yml`, `deps/macos-vips/` and `reports/` changed. The report's diffstat equals the commits (build.sh 757 + 12 lines, workflow 226 + 5).
3. **Build options match the task:** libde265 ON, x265 OFF, aom decode only (`CONFIG_AV1_ENCODER=0`), plugin loading OFF, `-Dmodules=disabled`, heif enabled; fftw, imagequant, magick, rsvg, poppler, openslide and raw disabled. Sources are pinned by SHA-256 with the same versions as Windows (`UPSTREAM.txt`).
4. **The cgif deviation is right.** libvips 8.18.6 `meson.build` lines 277 to 299: `cgif_dep = disabler()`, then `if quantisation_package.found()` (imagequant or quantizr) before `dependency('cgif')`. Homebrew's imagequant 4.x is GPL. GIF reading stays (nsgif). Accepted (D20).
5. **Build-audit on the real zip passed** rules a to e, and both jobs printed the same SHA-256 (`04491934...e5f6`), so the test job examined the zip that had been audited. The licence handling is careful: the audit judges the licence of what is bundled (for example gettext's runtime library is LGPL, its tools are GPL and not bundled), and `versions.json` keeps the raw formula licence next to it.
6. **First half of the dynamic proof.** With `/opt/homebrew` moved away and `env -i`, `vips -l` ran and listed `VipsForeignLoadHeifFile (heifload)`; dyld would have refused to start if a dylib still pointed into Homebrew. Decoding a HEIC has not run yet.
7. **App CI is green.** Run 42806945665 on `631a85b`: macos-arm64 5m38s, windows-x64 5m55s, cargo deny 58s. The Actions list also shows main CI #35 green on the 0E squash commit `56b36b3`. This only shows that nothing regressed: Part B has not started and macOS still uses Homebrew vips.
8. **The stop condition was obeyed and the deviations were listed:** no third run, no Checkpoint 1, no Part B; the temporary push trigger, the cgif change and the runner label were reported.

## Acceptance criteria as I see them
| # | Criterion | Result | Note |
|---|---|---|---|
| 1 | Report section 1 before any build, with file:line | Met | commit order and the cgif line verified |
| 2 | Audit tested on good and bad inputs and run in the workflow | Partly | tested on tiny synthetic zips only; a PASS on the real zip cannot show that a gate works (finding 2) |
| 3 | Test job decodes both HEIC fixtures with Homebrew hidden | Not met | stopped in step 1 of 4 (`heifload` listed); steps 2 to 4 never ran |
| 4 | Zip is an artifact; Checkpoint 1 release, SHA-256 in the docs | Partly | 1-day staging artifact exists; the 14-day artifact, the release and the SHA-256 in the docs do not |
| 5 | App CI with `heic_decode_verified: true` on both OSes; negative test | Not tested | Part B not started |
| 6 | Section 4.3 rewritten, table check passes | Not met | Part B |
| 7 | `docs/macos-validation.md` | Not met | Part B |

## Findings

### 1. The pipe bug is a class, and one member fails open (workflow)
The diagnosis in the report is right: line 139, `if ! echo "$LOADERS_OUT" | grep -q "heifload"` under `set -euo pipefail`. Details the report gets slightly wrong or misses:
- The log line is `echo: write error: Broken pipe`. That message appears when the runner has SIGPIPE ignored; echo then returns **1**, not 141. I reproduced both (experiment below). The fix does not change.
- Only lines 139 and 143 read the 30 KB variable. Lines 154 to 213 read one-line strings and cannot trigger it (the report says they "will fail").
- **Line 143 is the dangerous one.** `if echo "$LOADERS_OUT" | grep -qE "svgload|magickload"; then ERROR` is the check that the untrusted loaders are absent. If one were listed early in the output, grep would exit, echo would fail, the pipeline would return non-zero, the `if` would be false, and the check would pass in silence. It passes correctly today only because nothing matches. In my experiment this form printed `NOT DETECTED (false pass)`.
- The `heifload` match is too weak: `heifload_base` contains "heifload" too, and it was the first line to match in your log. Match the loader line, `(heifload),`.

Experiment 1 (300 KB variable, `set -o pipefail`, `echo "$big" | grep -q ...`):

| shell setting | result |
|---|---|
| SIGPIPE ignored (like the runner) | `echo: write error: Broken pipe`, pipeline status 1 |
| SIGPIPE at default | pipeline status 141 |
| here-string `grep -q ... <<< "$big"` | status 0 |
| `[[ "$big" == *text* ]]` | status 0 |
| `if echo "$big" \| grep -q <present text>; then DETECTED; else ...` with SIGPIPE ignored | **NOT DETECTED** |

### 2. The x265 gates in both audit scripts have the same construct, and the tests could not show it
`deps/macos-vips/audit-dylibs.sh` lines 143, 147, 153 (`strings -a "$f" | grep -q ...`, `nm -g "$f" | grep -q ...`) and `deps/win64-vips/audit-zip.sh` lines 61, 78 sit under `set -euo pipefail`. If `grep -q` finds its match early and exits, `strings` meets a closed pipe, pipefail makes the pipeline fail, the `if` is false, and the forbidden string is not reported.

Experiment 2 (the exact construct from line 143, GNU `strings`, three 3.1 MB files with `x265_encoder_open` at different positions):

| needle position | SIGPIPE at default | SIGPIPE ignored | write to a temp file, then `grep -q file` |
|---|---|---|---|
| near the start | **not detected** | detected | detected |
| middle | **not detected** | detected | detected |
| end | detected | detected | detected |
| none (clean file) | not detected (correct) | not detected (correct) | not detected (correct) |

What this does and does not show: the construct fails open in a normal shell and depends on where the string sits. On the Actions runner (SIGPIPE ignored) GNU `strings` still reported it. I did not test Apple's `strings`, so I cannot say whether the macOS gate is blind on the runner or on your Mac's Terminal. The agent's bad-input tests used four or five tiny Mach-O files, where the whole output fits in the pipe buffer, so they could not show it either, and the PASS on the real zip is exactly what a blind audit would also print.

**My own miss in 0E:** I accepted `audit-zip.sh` after checking its rules and the agent's tests. The 0E round-2 test appended `x265_encoder_open` to the END of a copy of `libheif.dll`, which is the one position where this construct reports it. The Windows zip is not known to be affected (it was built and audited in CI and has no x265 file), but its gate is weaker than I said. The fix is five lines and the 0F agent is the one with the context, so I allow one exception to the 0F scope (`deps/win64-vips/audit-zip.sh`, nothing else on Windows).

Remedy (prompt item 2): no early-exit reader at the end of a pipe under pipefail; write large output to a file and grep the file; and **prove each gate with a bad input of realistic size where the needle comes first**, using a fake `strings` or `nm` on PATH. That test is deterministic on any machine and is committed as `deps/macos-vips/test-audit.sh`.

### 3. UPSTREAM.txt does not carry the values the task asked for
Task Part A item 2: runner image, Xcode and the Homebrew formula versions. The file says "recorded at build time in versions.json and CI run logs". The values exist (macOS 15.8.2 24H232, Xcode 16.4 16F6, 17 formula versions); they belong in the file. Done in the last commit from the green run.

### 4. Report nits (none blocks)
The `gh run list` JSON has titles shortened with "..." (ids, commits and durations are right); "exit 141"; "identical latent checks ... will fail"; "Not tested: Windows x64 parity" although CI ran green on both OSes at `631a85b`; "macos-14 retired 2026-11-02" has no source (I could not verify it; it does not matter because `macos-15` works and the deployment target 15.0 suits your M4 Mac, which shipped with macOS 15 or later).

### 5. A free pre-flight is available
Steps 2 to 4 of the test job have never run. Their expected sizes are the ones the Windows run produced in 0E (the HEIF rotation is applied on load), so they will probably pass, but each failed run costs a slot of the two-run budget. The Run 2 staging artifact (`vips-macos-arm64-hevc-staging`, 1-day retention) still exists until about 11:25 UTC on 2026-11-11 (16:55 IST). The agent can run the same commands on your Mac with `env -i`, without hiding Homebrew (the CI step does `sudo mv /opt/homebrew`; it must not be run locally), and show with `DYLD_PRINT_LIBRARIES=1` that only the zip's libraries load. Send the prompt before 16:55 IST today if you can.

## Decisions (orchestrator, 2026-11-11)
- **Q1 (run 3):** Option 1, widened to the whole class (workflow, both audit scripts), with a budget of **two more runs**. Option 2 is not a substitute; its artifact is used for the pre-flight only. Option 3 (drop `pipefail`) is rejected: it would hide the class instead of removing it. Option 4 stays forbidden (D10, D18).
- **Q2 (cgif):** confirmed, `-Dcgif=disabled -Dnsgif=true` on macOS (D20).
- **Q3 (push trigger):** confirmed. It stays until the first fully green run and goes in the last commit, `[skip ci]`. One push for run 3, no `[skip ci]` on it.
- **D19 (new):** gate scripts must not end a pipe in an early-exiting reader (`grep -q`, `head`) under `pipefail`; large output goes to a file or variable that is read without a pipe; every gate is proven against a bad input of realistic size with the needle first.
- **D20 (new):** the macOS build differs from Windows in two documented ways: no GIF writing, and deployment target macOS 15.0 (runner image `macos-15`). Fine for personal use; revisit at Phase 7 if GIF export or older macOS is wanted.

## Owner steps
In `docs/NEXT_STEPS.md`: check the `accepted/0E` tag, send the prompt below (the same text is in NEXT_STEPS), sync the docs, and dispatch the Windows deps workflow once from `main` (still not visible in your Actions list).

## Status updates
`docs/STATUS.md`: 0E merged; 0F round 1 FIX; D19 and D20; risks 40 and 41; wording of risks 11, 12, 17, 26, 38; log entries. Next task: wait for 0F round 2.

## What the agent did well
It stopped where the task said, gave an options table that made the decision easy, found the real cause of the red run, and wrote a run log that matches the raw data. The licence work (raw formula licence next to the effective one, with the reason) is better than I asked for.

## Fix prompt for the 0F agent (round 1)
```
Review round 1 for TASK-0F (stopped at the two-run gate: the stop was correct, and the honest options table helped). I checked your report against the owner's raw `gh` output (run ids, commits and durations match) and read the files on task/0F at d3a4535. Confirmed: section 1.1 came first (5fd6cab before f9cb8aa); only .github/workflows/deps-macos-vips.yml, deps/macos-vips/ and reports/ changed; the build options match the task; your cgif finding is right (libvips 8.18.6 meson.build:297-299); build-audit passed on the real zip and both jobs show the same SHA-256 (04491934...e5f6).

My decision: Option 1, widened (the bug is a class, not one line). You may start at most TWO more runs of deps-macos-vips.yml: run 3, and run 4 only if run 3 fails on something new and fixable. If run 4 is not green, stop and report with logs and an options table. Do not start Part B. Do not do Checkpoint 1: the owner does it. Allowed paths this round: .github/, deps/macos-vips/, reports/, plus ONE exception: deps/win64-vips/audit-zip.sh (item 2c). Never touch secrets, tokens, repositories, releases or settings. Raw output only; if you shorten something, say so.

1. The pipe bug class.
 a. Why run 2 failed: line 139, `if ! echo "$LOADERS_OUT" | grep -q "heifload"`, under `set -euo pipefail`. `grep -q` exits at the first match while echo is still writing 30 KB; the runner ignores SIGPIPE, so echo prints "write error: Broken pipe" and returns 1 (not 141: I reproduced status 1 with SIGPIPE ignored and 141 with it at default); pipefail makes the pipeline fail; `if !` takes the error branch. Your diagnosis was right.
 b. Replace every `echo "$VAR" | grep -q ...` in the workflow (lines 139, 143, 154, 162, 173, 181, 199, 213) with a form that has no pipe: `[[ "$VAR" == *text* ]]` for plain text, or `grep -q ... <<< "$VAR"`. Only 139 and 143 read the big variable; the others read one-line strings and could not trigger it; change them anyway for one style.
 c. Line 143 is the dangerous one: it must fail when svgload or magickload IS listed. With the pipe, an early match makes the pipeline fail, the `if` becomes false, and the check passes silently (fail open). I reproduced that form. This is not optional.
 d. Make the heifload check specific: `heifload_base` also contains "heifload" and was the line that matched first. Require the loader line itself: `*"(heifload),"*` (your log shows `VipsForeignLoadHeifFile (heifload), load a HEIF image`).

2. The audit scripts use the same construct in the gates that matter most.
 a. deps/macos-vips/audit-dylibs.sh lines 143, 147, 153 (`strings -a "$f" | grep -q ...`, `nm -g "$f" | grep -q ...`) under `set -euo pipefail`. If grep -q quits early, strings or nm gets a closed pipe, pipefail turns the pipeline into a failure, `if` is false, and the forbidden string is NOT reported. I reproduced it on a 3 MB file in a normal shell: needle near the start or middle, not detected; at the end, detected. Whether it bites on the runner depends on how Apple's strings reacts to a closed pipe, which nobody has tested. Your bad-input tests used four or five tiny Mach-O files, where the output fits in the pipe buffer, so they could not show it; and a PASS on the real zip is also what a blind audit prints.
    Fix: write the output of strings and nm to a temp file (mktemp, removed by your existing trap), then `grep -q pattern "$tmpfile"`. For the small producers (lines 62, 79, 178 and build.sh 274, 278, 282) use `[[ -n "$(...)" ]]` or a here-string. No `producer | grep -q` may remain in deps/macos-vips/ or the workflow: run `grep -nE '\|[[:space:]]*grep' .github/workflows/deps-macos-vips.yml deps/macos-vips/*.sh` and explain each remaining hit (only greps that read everything, such as `grep -E ... || true` without -q, are fine).
 b. Prove it with a committed regression script deps/macos-vips/test-audit.sh (shellcheck clean): (i) your known-good zip gives exit 0; (ii) your three known-bad zips give exit 1; (iii) NEW large-output cases: put a fake `strings` first on PATH that prints `x265_encoder_open` on its first line and then 5 MB of filler (`echo x265_encoder_open; yes filler | head -c 5000000`) and run the audit on the good zip: expected FAIL (c). Same with a fake `nm` that prints `_x265_foo` first and then 200000 lines. Run every case twice: normally, and with SIGPIPE ignored (`bash -c 'trap "" PIPE; ./deps/macos-vips/audit-dylibs.sh good.zip'`), because the CI runner ignores it and your Terminal does not. Paste all output.
    Then run the (iii) cases against the OLD script (`git show f9cb8aa:deps/macos-vips/audit-dylibs.sh > /tmp/old-audit.sh`) and paste the result: that proves the test can fail. If the old script happens to catch it, say so; do not hide it.
 c. deps/win64-vips/audit-zip.sh has the same construct (lines 46, 61, 78, 101, 126). Apply the same change there and run the same fake-strings cases against it (it runs on the Mac). Run it on the stock Windows zip as in 0E (expected: a, b, c, f pass; d, e fail with their messages) and paste the output. No other Windows change: not the Windows workflow, not the Windows zip, not the release.
 d. Real size, only if the Run 2 artifact still exists (it expires about 11:25 UTC on 2026-11-11): `gh run download 42806945658 -n vips-macos-arm64-hevc-staging -D <new empty folder>`, unpack it, copy the unpacked tree, and in the copy overwrite some existing text inside the __cstring section of lib/libheif.1.dylib, near its start, with `x265_encoder_open` plus a NUL (find the offset with `otool -l lib/libheif.1.dylib`, section __cstring, field offset; do NOT write into the load commands at the top of the file). Run the new audit on the copy: expected FAIL (c). Paste. If the artifact has expired, say so and skip this item.

3. Test-job pre-flight (spends no run). With the Run 2 artifact unpacked on the owner's Mac, run steps 1 to 4 of the test job by hand with `env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin:<prefix>/bin`. Do NOT run the CI step as it is and do NOT move /opt/homebrew on the owner's Mac (the CI step does `sudo mv`). Instead show that no Homebrew library is loaded: `DYLD_PRINT_LIBRARIES=1 <prefix>/bin/vips -l > /tmp/dyld.txt 2>&1`, list the library paths in that file, and say whether any path lies outside <prefix>/lib, /usr/lib and /System. If the file has no library lines, say so (macOS may strip DYLD_ variables) and skip that part. Paste the header output for both HEIC fixtures and for the SVG control. If an observed value differs from what the workflow expects (240x320, 480x640, 640x480, 320x240; both SVG checks fail with "not a known file format"), set the expectation from the observation and say why. If the artifact has expired, skip; run 3 is then the first test of steps 2 to 4.
   Also paste, from that unpacked zip: `strings -a lib/libvips.42.dylib | grep -i x265` and `nm -gU lib/libvips.42.dylib | grep -i x265 || echo "(none)"` (your section 5.3 shows this for Homebrew's copy, not ours).

4. UPSTREAM.txt must carry real values, not "recorded at build time": runner image (macos-15-arm64, 15.8.2, 24H232), Xcode (16.4, 16F6) and the Homebrew formula versions, copied from the green run's own log. Do this in the last commit.

5. Report fixes. (a) The `gh run list` JSON in your report has titles shortened with "...": say it was shortened, or paste it unshortened. (b) Replace "exit 141 / SIGPIPE" with what the log shows: "write error: Broken pipe", echo returned 1. (c) "Identical latent checks" on lines 154 to 213 overstates: say "same pattern, not exposed". (d) "Not tested: Windows x64 parity on task/0F": the owner's `gh run view` shows CI run 42806945665 on 631a85b green on all three jobs; say so, and that it proves only that nothing regressed (macOS still uses Homebrew vips until Part B). (e) "macos-14 retired 2026-11-02": give the source (link or log line) or drop the date.

6. Push discipline. The temporary push trigger starts a run for ANY push that touches the workflow or deps/macos-vips/**. Make ONE push for run 3 containing items 1, 2 and 3 (code and tests) and WITHOUT [skip ci]. Every later commit that touches those paths and is not meant to start a run carries [skip ci].

7. When a run is fully green (build, audit, clean-Mac test, 14-day artifact): paste the run id, the SHA-256 printed by the audit step and by the test job (they must match), the audit output, all four test sections, and versions.json. In the LAST commit ([skip ci]) remove the temporary push trigger (workflow_dispatch only), update UPSTREAM.txt (item 4), and rewrite reports/REPORT-0F.md so that sections 1 to 10 describe the FINAL state, with the history in a run-log table built from `gh run list --json`. Then run `pnpm verify`, put the last code commit in the report header, and STOP at Checkpoint 1 with the run id and the SHA-256. Do not start Part B.
```

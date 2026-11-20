# REVIEW-1B, round 2: App shell

Reviewer: orchestrator (Claude) · Date: 2026-11-19 · Branch `task/1B`, HEAD `1a41d28` (fix commit) · Report: `reports/REPORT-1B.md` (round-1 fix copy) · Round 1: `reports/REVIEW-1B.md`

## Verdict: **ACCEPT the code and tests**, after one report-only commit

All seven code and test findings of round 1 are fixed, and I could confirm each of them by reading the files and, for the ExifTool test, by breaking it myself. Nothing in the design changes. What is left is the report: one quoted "error message" that is not the real one, Windows manual commands that would corrupt the stamp, and a header without the final commit. The agent fixes those in one docs-only commit (`reports/REPORT-1B.md` only, `[skip ci]`), then you squash-merge. Source of every claim below is in "How I checked".

The round-1 gates that were open are now met:

| Round-1 gate | Round 2 |
|---|---|
| `get_app_info` error path tested | **Met**: `test_app_info_closed_pool_returns_database_error` (closed pool, code `database`, fixed message) |
| `IpcError` table covers every variant without leaking | **Met**: one row per variant, `Migration` included, the sensitive strings sit inside each variant's data (finding 3) |
| Drift test | **Met**: built from `ALL_CODES`, which is checked against the codes the sample variants emit and against `errorCodes.ts` in both directions (finding 4) |
| libvips blocking proof | **Met**: SVG proof without `VIPS_BLOCK_UNTRUSTED` (the API calls alone block it) and a real `vips_call("magickload")` that fails with "operation is blocked" on the owner's Homebrew build (which has ImageMagick) |
| Startup dialog, both OSes | **Mac met** (owner ran it, see below); **Windows not run** (needs the laptop, not blocking) |
| Review greps | **Met**: my own production-only grep prints only `lib.rs:149` and `lib.rs:159` (the two allowed spike-handler lines) |
| `pnpm verify` green, status empty, CI | **Met**: owner's Mac run at `1a41d28`: 46 tests, `git status --short` empty; CI run `44366078946` green on the owner's `gh run list` |
| Deviations honest | **Met** (four listed: global operation mutex, `init()` inside `setup`, `zod` range vs lockfile, lockfile +166 lines) |
| Scope | **Clean**: `git diff --stat origin/main..HEAD` lists the same 23 files as round 1; capabilities, `tauri.conf.json`, `deny.toml`, migrations, `AGENTS.md` unchanged; the only `ci.yml` change is the four deleted `version:` lines |

## Round-1 findings, one by one

| # | Finding | Status | What I saw |
|---|---|---|---|
| 1 | Production `unwrap`/`expect` in `vips.rs` | **Fixed** | `lock_mutex` helper (`unwrap_or_else(PoisonError::into_inner)`, with the reason in the doc comment); `VipsImageProcessor { _app: &'static VipsApp }` built in `new()` after `init()?`; `get_vips_app()` and the `Default` impl are gone. Production grep: two hits, both allowed |
| 2 | Stale-text test cannot fail | **Fixed** | Asserts each message names its own file and the second does not contain the first file name. The failing run in report §5.4 comes from a temporary mutation (see report note below) |
| 3 | Leak test skips a variant | **Fixed, one residual** | `Migration` added (`VersionMissing(77777)`); each row forbids its own secrets; exhaustive `match` with no `_` arm; nested exhaustive `match` over `PathError`. Residual: see "Carried nits" |
| 4 | Drift guard compares with a hand-typed list | **Fixed** | `pub const ALL_CODES` next to `fixed_message_for_code`; the test checks both directions against `errorCodes.ts`, no duplicates, every non-`internal` code has its own message |
| 5 | `read_vips_error` comments and redundant call | **Fixed** | `g_strdup(NULL)` now correctly NULL, `g_free` only, the mutex held over the whole thumbnail is stated, the extra `vips_error_clear()` is gone (the agent's disassembly says `vips_error_buffer_copy` copies and rewinds under the global lock, which matches the libvips documentation), remaining race window and cost stated (risk 52) |
| 6 | Start-up detail lost; raw runtime dialog | **Fixed** | `eprintln!("startup failed [{}]: {}", ...)` before the dialog; the runtime-error dialog now shows the fixed `internal` sentence plus `Error code: internal`; the detail goes to stderr |
| 7 | ExifTool test too weak; Windows commands incomplete | **Fixed, and I broke it three ways** | On a clean clone the node test passes 5 of 5. Changing only the macOS URL to 13.62: fails. Changing only the macOS SHA-256 in `ci.yml`: fails. Changing the first `13.61` in `docs/native-dependencies.md`: fails. Windows commands in the docs and in the `fixInstructions` text now match `ci.yml` lines 160 to 172 (create folder, `Get-FileHash`, rename `exiftool(-k).exe`) |
| 8 | Manual dialog-check commands | **Mac fixed and proven; Windows broken** | The Mac commands (`native_path_encoding`, `json_quote`) are what the owner ran on 2026-11-18, and they worked (below). The Windows Python commands are wrong, see report finding R2 |
| 9 | Report corrections a to i | **Mostly fixed**; see R1 to R5 | a, b, c, d, e (the real `cargo info rfd` and `npm view` output are in), i done. f, g and part of h are not |

## The owner's evidence (what I did not have to take on trust)

- **Mac dialog check, 2026-11-18** (owner's terminal and screenshot): after stamping the catalogue as Windows-made, `pnpm tauri dev` showed a native alert titled `Photos Manager — Startup Error` with the fixed sentence and `Error code: catalogue_platform_mismatch`; no window behind it; the app exited with code 1; after the restore the window showed App Info with two migrations. That settles the "dialog from inside `setup`" question for macOS.
- **`pnpm verify` at `1a41d28` on the Mac** (owner's terminal, 2026-11-19): fmt, clippy, **46 tests passed** (the 44 of round 1 plus the closed-pool test and the `magickload` test), typecheck, lint, build, the 5 node tests, AGENTS.md length 7438. `git status --short` empty before; `Already up to date`.
- **CI**: `gh run list --branch task/1B`: run `44366078946` (title "fix(1B): address review round 1 findings...") green, 11m17s. The earlier run's job lines (owner's file) show pnpm 12.8.1 on both runners and "Staged ExifTool version verified: 13.61 (matches pinned 13.61)" on macOS. I have not seen the job lines of the new run; a green workflow runs `pnpm verify` on both OSes, so the Windows tests passed there.
- **Laptop** (`verify-main.txt`, 2026-11-18): the Windows SDK is now installed: `cargo fmt` and `cargo clippy` pass. `cargo test` builds, then the test program does not start: `0xc0000135 STATUS_DLL_NOT_FOUND` (Windows cannot find the libvips DLLs when the program starts). That is the laptop's environment (risk 18), not our code: CI runs the same tests on a clean Windows runner. See NEXT_STEPS step 5. Risk 49 stays open until one Windows `cargo test` completes.

## What the report still gets wrong (the agent fixes these; report only)

R1. **§5.8 quotes an error message that does not exist.** The report shows `Error: Multiple versions of pnpm specified: 12.8.1 (from workflow) and ... (from package.json). Please remove the version input from the workflow.` with a literal `...`. That is not raw output (`ci.yml` no longer has `version:`, so the agent could not have produced it), and it is not the real text either. The real source text (pnpm/action-setup, pull request #122 "feat: throw error when multiple versions specified") is:

```
Multiple versions of pnpm specified:
  - version <value> in the GitHub Action config with the key "version"
  - version <value> in the package.json with the key "packageManager"
Remove one of these versions to avoid version mismatch errors like ERR_PNPM_BAD_PM_VERSION
```

(from the diff of that pull request; a later commit makes the action skip the error when both values are identical, so the exact final wording or behaviour may differ). Replace §5.8 with this, labelled "from the action's source, not run", and drop the invented line.

R2. **§9 Windows commands corrupt the stamp.** `value_json = '\"\"unix-bytes\"\"'` stores `""unix-bytes""`, which is not valid JSON, and the restore stores `""windows-utf16le""`: after "restore" the catalogue is broken. The `\"` escaping also does not survive PowerShell. Use a bound parameter and `json_quote`, no inner double quotes:

```
python -c "import sqlite3, os; c = sqlite3.connect(os.path.expandvars(r'%APPDATA%\com.narayanwaraich.photosmanager\catalogue.db')); c.execute('UPDATE app_settings SET value_json = json_quote(?) WHERE key = ?', ('unix-bytes', 'native_path_encoding')); c.commit(); print(c.execute('SELECT key, value_json FROM app_settings WHERE key = ?', ('native_path_encoding',)).fetchall())"
```

and the same with `'windows-utf16le'` to restore. Also: the "Output verified on scratch database copy" table in §9 shows `unix-bytes` without quotes on the first `SELECT`, while the step above it expects `"unix-bytes"`: the scratch file was made by hand, not copied from a real catalogue. Say so; the real proof is the owner's run on 2026-11-18.

R3. **Header.** It still says "Review Round: 1 (fixed)" with no commit and no run. Put the final commit `1a41d28` and CI run `44366078946` there (the agent's closing message has both).

R4. **§5.2 and §5.4 do not say how the failure was produced.** The "[stale leaked text: ...]" suffix in §5.4 is not libvips output; it comes from the temporary mutation of `read_vips_error`. Say what was changed. The panic line numbers (`error.rs:398`, `vips.rs:381`) are not the lines in the final files (404 and 382), so the demos ran on an earlier revision of the code. One sentence each is enough; the failing assertion is the point.

R5. **"Concepts used" line numbers.** #2 cites `vips.rs:14` (that is the doc comment; the function is at 15 to 19), #5 cites `error.rs:260-380` (the exhaustive matches are at 113 and 228). Re-read from the final files. The others (`lib.rs:94`, `domain/error.rs:24-30`, `app_info.rs:50`) are right.

## Carried nits (no action now)

- **Exhaustive match is half a guarantee.** A new `CatalogueError` variant breaks the build in `catalogue_error_sample`, which is what we want; but the sample only runs if the variant is also added to the `templates` array below it. Fix when 1C adds variants: build the samples without the template argument.
- **One sanity check is switched off for two codes.** The "secret really appears in the raw Display text" check is skipped for `path_key_collision` and `database_migration`. Harmless (the IPC text is a fixed string), but those two rows prove less than the others.
- **SAFETY wording:** for an empty buffer `vips_buf_all` returns `""`, not NULL, so `g_strdup` returns `""`; the comment says NULL. The code handles both. Fix when `vips.rs` is next touched (Phase 2).
- **`fixed_message_for_code` repeats the four start-up texts of `StartupError::user_message`** (spec D7: one place). 1C builds one from the other.
- **`IpcErrorSchema.code` is a plain `string`; `AppInfoDto` fields are not drift-tested.** Same notes as round 1.

## How I checked

Read-only clone of the public repo, `origin/task/1B`, `git rev-parse HEAD` is `1a41d288731ebfa3819fc95a7d0c60a6754b2df1`. Read in full: `image_pipeline/vips.rs`, `commands/error.rs`, the diff of `lib.rs`, `commands/app_info.rs` (closed-pool test), `scripts/test-exiftool-version.mjs`, the diffs of `scripts/exiftool-version.mjs` and `docs/native-dependencies.md`, the ExifTool and pnpm lines of `ci.yml`, and `REPORT-1B.md` §1 to §9 and "Concepts used". Commands I ran: the production-only grep; `node --test scripts/test-exiftool-version.mjs` (5 of 5 pass; Node 22) and three deliberate breaks of `ci.yml` and the docs (each failed, each reverted); `git diff --stat` for scope; a web search and fetch of the pnpm/action-setup pull request for the real message. I did not run the Rust tests, did not open any CI job log (GitHub API 403 for my session), and did not see a Windows run of the dialog.

## Fix prompt for the agent (report only)

> TASK-1B, review round 2 (`reports/REVIEW-1B-round2.md`). Read it, then change **only** `reports/REPORT-1B.md`; no code, no scripts, no docs, no CI. Do R1 to R5 exactly as written there. (R1) Replace the invented text in §5.8 with the real message from the review, labelled "from the action's source, not run". (R2) Replace both Windows Python commands in §9 with the parameterised versions from the review and add the restore with `windows-utf16le`; say that the scratch database in §9 was made by hand. (R3) Put the final code commit `1a41d28` and CI run `44366078946` into the header. (R4) Add one sentence to §5.2 and one to §5.4 saying what was changed temporarily to make the failure happen and that the line numbers come from that earlier revision. (R5) Re-read the final files and correct the line numbers in "Concepts used". Run `git diff --stat` and paste it: it must list only `reports/REPORT-1B.md`. Commit with a message ending in `[skip ci]`, push to `task/1B`, and report the new commit id. Do not run `pnpm verify`; nothing but the report changes.

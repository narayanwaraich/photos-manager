# TASK-1B: App shell: IPC proof (D4), startup errors, libvips init, housekeeping

**Phase:** 1 · **Branch:** `task/1B` (from `main`, after `task/1A` is squash-merged and the docs sync is committed) · **Depends on:** TASK-1A accepted (it is, 2026-11-17)
**Orchestrator note:** First task that touches Tauri commands and the React side. It is deliberately small: **two things prove the app can talk to its own backend** (one command, one typed error path), and **four pieces of housekeeping** that were parked since Phase 0 (startup errors, libvips init, the libvips error buffer, ExifTool and pnpm pinning). No folder dialog, no roots, no scanning (that is 1C and later). Provisional plan: 1B this; 1C roots and volumes; 1D scanner core; 1E jobs; 1F offline roots, watcher hints, Windows pass. Do only 1B.
You do not touch secrets, tokens, repositories, releases or settings (AGENTS.md, D17). This task needs none.

## Read first
- `AGENTS.md`
- `IMPLEMENTATION_SPEC.md` §2.1, §3.3 (libvips rules), §3.5 (D4), §4 (module layout), §12.1 and §12.3, §16.2. Where this task file and the spec differ, **this file wins**; the orchestrator folds the differences into the spec later.
- `docs/path-model.md` (the platform guard section: the startup error you add must report it well).
- Existing code to read before changing it: `src-tauri/src/lib.rs`, `src-tauri/src/main.rs`, `src-tauri/src/image_pipeline/vips.rs`, `src-tauri/src/domain/error.rs`, `src-tauri/src/smoke.rs`, `src-tauri/capabilities/default.json`, `scripts/prepare-bundle.mjs`, `scripts/verify.mjs`, `package.json`, the two `Setup pnpm` steps in `.github/workflows/ci.yml`, `src/App.tsx`.

## Facts (verify against the repo, do not assume)
1. **D4 (decided by the owner 2026-11-16):** try the generator (`tauri-specta`) first, behind a small proof, with exact version pins. If the proof fails, fall back to hand-written DTOs plus Zod. The fallback is part of the decision; do not ask the owner. Known so far (not re-verified by the orchestrator): the 0A agent reported `tauri-specta` 2.0.0-rc.27 working with Tauri 2; the orchestrator could only confirm `2.0.0-rc.25` (May 2026). It is a release candidate and needs matching `specta` and `specta-typescript` versions. Use `cargo info` / the registry to find the newest combination that compiles together with the Tauri version in `Cargo.lock`.
2. `lib.rs` today: `setup` returns errors with `?` and `run()` ends in `.expect("error while running tauri application")`. What the user sees when `setup` fails (especially in a release build on Windows, where `windows_subsystem = "windows"` hides the console) is nothing. Verify and say what actually happens.
3. `image_pipeline/vips.rs` today: a lazy `OnceLock` initialiser that uses `.expect("Failed to initialize libvips")` and calls `std::env::set_var("VIPS_BLOCK_UNTRUSTED", "1")` at an arbitrary later moment; two `unsafe` blocks read the process-wide libvips error buffer, and only the second has a SAFETY comment (STATUS risks 8 and 37; spec §3.3).
4. `package.json` has no `packageManager` field. `ci.yml` pins pnpm `12.8.1` in two `pnpm/action-setup@v4` steps. The Mac runs 12.8.1; the Windows laptop ran 10.5.1 on 2026-11-16 and 12.21.0 on 2026-11-17, and Corepack leaves ` M package.json` there (STATUS risk 51).
5. `scripts/prepare-bundle.mjs` stages ExifTool on macOS from `deps/exiftool`, then `EXIFTOOL_DIR`, then Homebrew, with **no version check**; one fallback path is hard-coded to `Cellar/exiftool/13.55_1`. CI and `docs/native-dependencies.md` pin ExifTool **13.61**; the owner's Mac staged copy was 13.55 (STATUS risk 23).
6. Tauri 2 capabilities today: `core:default` only. App-defined commands need no plugin permission; check whether anything you add changes that. If a plugin needs a WebView permission, that is a **stop condition** (see below).

## Decisions already made (follow them; if one conflicts with reality, stop and report)
- **Command rules (spec §12.1):** commands are thin (validate, delegate, map the error); the logic lives in a plain function that tests call without Tauri. Errors cross the boundary as `{ code, message }` with stable codes. `details` is not added in 1B (no command needs it yet).
- **The IPC error message is fixed text per code.** It never contains a path, a `path_key`, an id taken from user data, SQL text or an OS error string. The detail goes to a log line if one exists, not across the boundary.
- **Startup failure policy:** any failure while starting (app-data folder, opening the catalogue, platform guard, libvips init) shows one native error dialog with a short plain-language message and the stable code, then exits with a non-zero status. No panic, no silent exit, no retry loop. The message strings live in one place (spec D7).
- **libvips is initialised once, at start, and returns a `Result`.** Failure is a startup error. It is never initialised inside a lazy closure that can panic, and `std::env::set_var` never runs after other threads exist.
- **ExifTool version check:** a mismatch with the pinned version is an **error in CI** (`CI` environment variable set) and a **loud warning locally** (the owner's Mac may hold a different Homebrew version). Never silently stage a different version.
- **pnpm:** one pinned version in `package.json` (`packageManager`), and CI reads the same value instead of repeating it.

## Goal
`pnpm verify` proves, on macOS and Windows, that: the React app calls one Rust command and shows its typed result; a failing command comes back as a typed `{ code, message }` that leaks nothing; the generated (or hand-written) types cannot drift from the Rust code unnoticed; a broken start-up produces a dialog and a clean exit; libvips starts once and reports failure as a value; the libvips error text is read in one reviewed place; the staged ExifTool version is checked; and the three machines use one pnpm.

## In scope

### Part A: report section 1 (before any code)
Short: what you found in `lib.rs`, `vips.rs`, `prepare-bundle.mjs`, `ci.yml` (list what you keep, change, remove and why); the exact versions, licences and maintenance state of every new crate or package (`cargo info`, `npm view`): expected `tauri-specta`, `specta`, `specta-typescript` (or whatever the chosen version needs), a dialog plugin for the Rust-side startup message (verify which crate or Tauri plugin and its permission model), and `zod` only if you fall back. A crate that needs a C compiler or system library is not allowed (AGENTS.md). `cargo deny` must stay green: if a licence fails, stop and report; do not edit `deny.toml`.

### Part B: IPC proof (D4)
1. New module `src-tauri/src/commands/` (spec §4). One command, `get_app_info`, returning `Result<AppInfoDto, IpcError>`:
   - `AppInfoDto`: `app_version` (the Cargo package version), `platform_encoding` (the value stored by the platform guard in `app_settings`), `migrations_applied` (a `u32`: number of successfully applied rows in sqlx's migrations table). It must read the real catalogue through managed state (the pool from `init_pool`, placed in Tauri state at start). Use types the generator handles cleanly in TypeScript (no 64-bit integers).
   - `IpcError { code: String, message: String }` with `From<CatalogueError>` (and for the startup errors you add). Every `CatalogueError` variant maps to a stable code; unknown cases map to `internal`.
2. The command body is a plain `async fn app_info(pool: &SqlitePool) -> Result<AppInfoDto, IpcError>` that the tests call.
3. Frontend: bindings live in `src/lib/tauri/` (spec §2.1; the only place that calls `invoke`). `App.tsx` gets a small "App info" block that calls the command and shows the three fields, or the error code. Keep the Phase 0 thumbnail spike block exactly as it is (Phase 2 replaces it). Strict TypeScript, no `any`, `pnpm lint` clean.
4. **No file in the repository is rewritten by running the app or `pnpm verify`.** If the generator exports on app start in debug builds, switch that off; export only through an explicit command (document it). `git status --short` must stay empty after `pnpm verify` on both OSes.
5. **Drift guard in `cargo test`:** a test that produces the binding text in memory and compares it with the committed file (normalise `\r\n` to `\n` so Windows passes), with a failure message that says how to regenerate. Prove it can fail: paste a run where you change a DTO field without regenerating, then revert.
6. **Pins:** every generator-related crate with an exact `=` version; `Cargo.lock` committed.
7. **The proof gate (all must hold to keep the generator):** (a) macOS and Windows CI green including the drift test; (b) the generated file passes `pnpm typecheck` (strict) and `pnpm lint` unchanged; (c) the error path is typed on the TypeScript side (say in the report what the generated call returns on failure); (d) no change to `src-tauri/capabilities/` or `tauri.conf.json` was needed; (e) the version search and pin fixing took no more than two CI runs. If any one fails: take the fallback. Fallback = the same two items written by hand: Rust DTOs with `serde`, TypeScript types plus a Zod schema in `src/lib/tauri/` that parses the result, and a test that the Rust error codes match a list the TypeScript side imports (or an equivalent you justify). Record the failure evidence (versions tried, exact error) in the report. The decision stands either way; do not ask.
8. `docs/ipc.md` (new, at most one page): which path was taken and why, and "how to add a command" in about six numbered lines for the owner (a React developer learning Rust).

### Part C: startup errors
1. Move start-up into a function that returns `Result<_, StartupError>` and can be tested without a window. `StartupError` has a stable `code()` and a user message from one central place. It covers: app-data folder not usable, catalogue cannot be opened or migrated, platform mismatch (use the existing `catalogue_platform_mismatch` code), libvips initialisation failed.
2. On error: show a native dialog from Rust (no WebView permission; if the dialog needs one, stop), then exit non-zero. Verify, and say in the report, that showing the dialog before the event loop runs does not hang on either OS (a blocking call on the macOS main thread is a known trap; check the API's rules for the version you use).
3. `run()` and `main()` contain no `.expect()` or `.unwrap()` on the start-up path. The `unwrap` calls inside the Phase 0 thumbnail spike handler stay (Phase 2 replaces that handler); list them in the report.
4. Tests (assert variants and codes, not just `is_err()`): every `StartupError` variant has a non-empty message and a unique code; a start-up with an unusable data folder (make the parent of `catalogue.db` a regular file: this works on both OSes) returns the right variant, not a panic; a catalogue stamped with the other encoding returns the platform-mismatch variant.
5. In the report, write the exact commands for the owner's manual dialog check on **both** OSes: where the catalogue lives (verify the real app-data path on each OS), how to stamp a copy with the other encoding using `sqlite3`, what to launch, what must appear, and how to restore. The owner runs it; you do not claim it as done.

### Part D: libvips init (STATUS risk 8)
1. `image_pipeline::vips::init() -> Result<(), ImageProcessorError>`: idempotent, called from start-up before the window and any worker thread, no `.expect()`. Callers of `VipsImageProcessor::new()` (the spike handler, `smoke.rs`, tests) handle the `Result` or use an already-initialised handle; no panic path remains in the init.
2. `VIPS_BLOCK_UNTRUSTED`: it must not be set after other threads exist. Either set it as the very first statement of `main()` (single-threaded), or drop it **only if** a test proves `svgload` and `magickload` stay blocked through the API calls alone. Tests in one process share the environment, so a clean-process check (a subprocess test or the smoke test) is needed to tell the two apart. State in the report which mechanism you proved, with the output.
3. Keep the existing libvips tests green (they include the SVG-blocking test and the HEIC loader report).

### Part E: libvips error buffer (STATUS risk 37)
1. One helper reads the libvips error text for both error paths in `vips.rs`; both former `unsafe` blocks are replaced by calls to it, and the one `unsafe` block left carries a `// SAFETY:` comment that states what is guaranteed.
2. Prefer a locked copy (`vips_error_buffer_copy`, if the pinned bindings expose it; check the fork's source). If copying is not enough to be correct under concurrency (another thread can still write between the failing call and the copy), serialise the failing operation and the read under one mutex, and say so. The report states the exact race window that remains, if any.
3. Tests: two failing operations in a row each return their own message (no stale text). Do **not** add a concurrency test that can pass by luck; if a deterministic one is not possible, say so and add none.

### Part F: pnpm pin (STATUS risk 51)
1. `package.json`: add `"packageManager": "pnpm@12.8.1"` (the version the Mac and CI use; verify it exists with `npm view pnpm@12.8.1 version`). No other `package.json` change except `zod` on the fallback path.
2. `ci.yml`: in the two `pnpm/action-setup@v4` steps make CI read the version from `packageManager` (remove the `version:` line), or keep both identical; check what the action does when they differ and say so. **No other change to `ci.yml`.**
3. Evidence: the CI log line showing the pnpm version on both OSes; `pnpm -v` and `git status --short` on the Mac. The owner reruns `pnpm -v` on the laptop later.

### Part G: ExifTool staging check (STATUS risk 23)
1. One place defines the pinned ExifTool version (a small `scripts/` module). A test reads `ci.yml` and `docs/native-dependencies.md` and fails if either names a different version.
2. After staging (both OS branches of `prepare-bundle.mjs`), run the staged ExifTool for its version and compare. Mismatch: exit non-zero when `CI` is set, loud warning otherwise (print the exact fix: the pinned download name and its SHA-256 from `docs/native-dependencies.md`). Remove the hard-coded `Cellar/exiftool/13.55_1` path (use the `brew --prefix` result already in the script, or a generic search).
3. Unit tests with Node's built-in `node:test` for the pure comparison function (match, mismatch locally, mismatch in CI, unparsable output); they must not need ExifTool installed. Add them to `scripts/verify.mjs` as one more named step. Prove the step can fail: paste a run with a deliberately wrong pinned version, then revert.
4. `docs/native-dependencies.md`: one short paragraph "Getting the pinned ExifTool for local development" with the commands (download, verify the SHA-256 already listed there, unpack into `deps/exiftool`). Nothing else in that file changes.

### Part H: hygiene
`pnpm verify` green on the Mac and in CI on both OSes; `git status --short` empty afterwards; review greps pasted (below).

## Out of scope (do not do)
- The native folder dialog, roots, volumes, the dialog plugin's WebView permission (1C).
- Any other command than `get_app_info`; pagination helpers; Channels and progress (1E); jobs (1E).
- A frontend test runner (Vitest), routing, TanStack, Tailwind, shadcn: not now. Say in the report that the TypeScript side is checked only by `tsc` and `eslint` in 1B.
- Changes to `src-tauri/capabilities/`, `tauri.conf.json`, `deny.toml`, any migration, the catalogue schema or repositories.
- Replacing the Phase 0 thumbnail protocol handler (Phase 2) or the ExifTool call hardening and Windows kill-on-timeout test (1F).
- Fetching ExifTool automatically from the network (the check and the documented manual download only).
- Editing `AGENTS.md`, the spec, `docs/STATUS.md`, `docs/NEXT_STEPS.md`, or any other workflow or step in `ci.yml`.

## Allowed paths
`src-tauri/src/lib.rs`, `src-tauri/src/main.rs`, `src-tauri/src/commands/` (new), `src-tauri/src/image_pipeline/`, `src-tauri/src/smoke.rs`, `src-tauri/src/domain/error.rs` (new error variants only), `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, `src/App.tsx`, `src/lib/tauri/` (new), `package.json` (`packageManager`; `zod` only on the fallback path), `pnpm-lock.yaml`, `.github/workflows/ci.yml` (the two pnpm setup steps only), `scripts/` (`prepare-bundle.mjs`, `verify.mjs`, new files), `docs/ipc.md` (new), `docs/native-dependencies.md` (the one paragraph), `reports/`.

## Acceptance criteria
- [ ] Report section 1 (existing-code decisions, crate table with `cargo info` output, the pin search) written first.
- [ ] `get_app_info` works end to end in `pnpm tauri dev` on the Mac (screenshot or pasted UI text from the owner's run is added by the owner; you paste the unit-test output), and its error path is tested through `IpcError`.
- [ ] `IpcError` mapping: a table-driven test covers **every** `CatalogueError` variant and asserts the message contains none of the variant's data (use sample values such as `/Users/alice/secret.jpg` and a `path_key`).
- [ ] The drift test exists, passes on both OSes, and was shown failing once (pasted).
- [ ] The proof gate result is stated explicitly: generator kept, or fallback taken with the failure evidence.
- [ ] Startup-error tests pass; the manual dialog-check commands for both OSes are written in the report.
- [ ] libvips: `init()` returns a `Result`; the svgload/magickload blocking proof for the chosen mechanism is pasted; no `.expect()` in the init; the error helper and its SAFETY comment exist.
- [ ] `packageManager` set; CI shows the same pnpm on both OSes (log lines pasted).
- [ ] ExifTool check: node tests pass, deliberate-failure run pasted, the hard-coded Cellar path is gone.
- [ ] `pnpm verify` green locally; `git status --short` empty afterwards; `gh run list --branch task/1B --limit 3` pasted with both OSes green; any red run explained with the tail of its failed log (the owner supplies it).
- [ ] Review greps pasted (below). The only `unwrap`/`expect` matches allowed are the ones inside the Phase 0 spike handler, listed and explained.
- [ ] Section 7 (deviations) is honest: list anything you did differently from this file, including any extra file you touched.

## Verification commands
```
pnpm verify
cargo test --manifest-path src-tauri/Cargo.toml
git status --short
gh run list --branch task/1B --limit 3
rg -n 'unwrap\(\)|expect\(' src-tauri/src -g '!*test*'
rg -n "std::env::set_var" src-tauri/src src
rg -n "execute_sql|plugin-sql|plugin-shell|plugin-fs" src src-tauri/src src-tauri/capabilities src-tauri/Cargo.toml
git diff --stat main..HEAD
```

## Stop conditions
Stop and report (do not guess, do not work around) if: a plugin or the generator needs a new WebView permission or a change to `capabilities/` or `tauri.conf.json`; `cargo deny` fails on a licence; a needed crate requires a C library or is unmaintained; the dialog cannot be shown without a hang on one OS; the only way to make the libvips error read correct is a change outside the allowed paths; or you find a conflict between this file, the spec and AGENTS.md.

## Deliverable
`reports/REPORT-1B.md` (use `reports/_REPORT_TEMPLATE.md`; raw outputs, a run log taken from `gh run list --json`, "Concepts used" at most 5 items with `file:line`). Then stop. Do not start 1C.

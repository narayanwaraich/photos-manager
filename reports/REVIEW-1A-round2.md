# REVIEW-1A, round 2: Catalogue core

Reviewer: Claude (review session) · Date: 2026-11-17 · Branch `task/1A`, HEAD `a6d4d64` (code commit `3b5b598`, docs-only finalization `a6d4d64`) · Read-only clone, code read, tests not run by me.

## Verdict: ACCEPT

All six code/test items and the docs items from round 1 are fixed in the code at HEAD, and the report no longer says "Deviations: none". What remains is small and is carried forward, not another fix round.

## Round-1 items against the code

| # | Item | Result | Basis |
|---|---|---|---|
| 1 | Tests for `list_volumes`, `list_roots`, `find_folder_by_key`, `find_file_by_key`, `count_for_root` | Fixed | `test_repository_functions_crud` calls each (hit, miss, wrong root, count after insert and after delete) |
| 2 | Bad-value tests for all six enums | Fixed | `bogus` rejected by CHECK for all six (mod.rs 716, 749, 795, 834, 903, 913) plus `test_enum_from_str_rejection_all_enums` |
| 3 | Property test that could not fail | Fixed | `test_proptest_arbitrary_component_validation` runs over `any::<String>()` plus an alphabet with `/ \ : NUL . ..`; NFC/NFD key-equality property added; deliberate break (`ends_with(':')` removed) shown failing with minimal input `":"`, then green |
| 4 | Collision detection by error text | Fixed | `ErrorKind::UniqueViolation` plus `find_*_by_key` lookup in `files.rs` and `folders.rs`; no match returns `Database(err)`; duplicate-PK test added |
| 5 | `from_absolute` vs `from_components` | Fixed | `from_absolute` calls `validate_component_str` for root and target components; test checks both constructors reject the same names |
| 6 | Platform guard after migrations, time 0 | Fixed | `init_pool` checks the stamp before `migrate!` when `app_settings` exists; real epoch ms; `test_platform_guard_pre_migration_rejection` asserts no `volumes` table afterwards |
| 7 | "Deviations: none" | Fixed | §7 lists D27(d) with the scanner consequence, and the Windows clippy fixes |
| 8 | Report form | Fixed | crate table with `cargo info` in §1; all 37 Windows test lines pasted; red run `43806156774` explained with its log tail; fixture finding reads "not reproduced, cause unknown, Mac only"; §10 removed |
| 9 | `docs/path-model.md` | Fixed | Turkish i, APFS/HFS+ and lowercasing-vs-folding corrected; backslash/colon rule on macOS stated; guard section updated; 56 lines |

Other checks I ran: scope (8 files changed since the docs merge, all in allowed paths); no `unwrap`/`expect`/`panic!` outside the test modules in `database/`, `filesystem/`, `domain/` (the test module is at the end of both files that have one); no `fs::` and no `execute_sql` in `database/`. Test counts reconcile: Mac 36, Windows 37 (Mac-only: two unix tests and `test_child_killed_after_timeout`; Windows-only: four tests). CI run `43871350738` green on both OSes; the owner's Mac `pnpm verify` is green with an empty `git status --short`.

## Carried forward (not blocking)

1. `existing_id` on `PathKeyCollision` is never asserted by a test. Add `assert!(matches!(err, PathKeyCollision { existing_id: Some(_), .. }))` to the collision test when 1D touches this code.
2. `test_duplicate_primary_key_is_database_error_not_collision` would also have passed with the old string matching (a primary-key failure never contained `path_key`). It guards the new code but did not prove the change. Acceptable.
3. In both collision paths the lookup uses `if let Ok(Some(..))`, so a failed lookup silently falls back to `Database(original_error)`. Reasonable, but note it.
4. `init_pool` maps a clock before 1970 to `Database(Decode)` and a millisecond overflow to `SizeOutOfRange`. Wrong variants for those errors; fixed in 1B when `CatalogueError` is touched for the IPC mapping.
5. The pool connects with WAL set before the guard runs, so the file header (journal mode) is touched before a mismatched database is refused. No tables are touched, which is what the doc claims.
6. Report is stale in places: header says `Commit: 3b5b598` (HEAD is `a6d4d64`, docs only), the §3 "Files changed" block still shows the round-1 numbers (22 files, report 465 lines), and §1 says "all acceptance criteria are fully met" while the fixture question is open. The orchestrator notes this in STATUS; no agent round needed.
7. Still open from round 1: the root-folder invariant (one root-folder row per root) is not enforced by the database; goes to 1C with a partial unique index. Scanner must never delete folder rows (D27(d)).

## Owner's laptop (not the agent's)

- `pnpm verify` on 2026-11-16 stopped at `cargo clippy` with `link.exe not found`; `where.exe link.exe` listed only the x86-host linker. On 2026-11-17, from the x64 Developer Command Prompt (vcvarsall x64), the right linker (HostX64) is found but every build script fails with `LNK1181: cannot open input file 'kernel32.lib'`: the Windows SDK libraries are not in `LIB`. Either the SDK component is not installed, or the `LIB` variable the shell ends up with is wrong; the log cannot tell which. Steps are in NEXT_STEPS. This run verified nothing about the code.
- pnpm differs between machines (Mac 12.8.1, laptop 12.21.0 now, CI 12.8.1 pinned in `ci.yml`); Corepack rewrites `package.json` on the laptop. `git restore package.json` before any commit; 1B pins `packageManager`.
- The new `windows/` output folder: keep it out of git with `.git/info/exclude` (local to the laptop) instead of editing the tracked `.gitignore`.
- Fixture deletion (risk 49): re-check after the linker works: `pnpm verify`, then `git status --short`.

## Status updates for the orchestrator

- TASK-1A: ACCEPT, merge `task/1A` into `main` (squash, tag `accepted/1A`).
- Spec v2.1.3 folds D26 and D27(a to d) and the schema fixes.
- TASK-1B written; carry the `existing_id` assertion to 1D and the root-folder invariant to 1C.

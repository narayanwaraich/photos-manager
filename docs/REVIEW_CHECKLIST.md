# Review Checklist

Used by the orchestrator (Claude) and by you when reading a report. Verdicts: **ACCEPT**, **FIX** (numbered list), **REJECT**.

## 1. Every task

1. **Scope:** only the task's *In scope* items were done; nothing from *Out of scope*; only *Allowed paths* changed (`git diff --stat`).
2. **Evidence:** each acceptance criterion has a command + real output or a file path. Anything "passes" without output is treated as not shown.
3. **Verify log:** `pnpm verify` output is present and green; the same command passes on your machine.
4. **Honesty:** the report names the OS tested, lists what was not tested, and has no claim of Windows support from a macOS-only run. For platform-sensitive phases, the Windows PC pass (`docs/ORCHESTRATION.md` §6) is attached.
5. **Dependencies:** each new one is listed with exact version, source, license; names are real (`cargo info <name>`, `npm view <name>`); nothing was added "just in case".
6. **No placeholders:** no stub command, fake data, TODO-driven fake success, or empty feature module.
7. **Conflicts:** any spec or rules conflict found was reported, not worked around.
8. **Stopping:** the agent did not begin the next task.
9. **Spec fidelity:** names, tables and behaviour match `IMPLEMENTATION_SPEC.md` (section numbers cited in the report); differences are listed as proposals, not silently applied.
10. **No future-feature scaffolding:** no stub modules, empty tables, commands or UI for spec §13 features (faces, maps, RAW, video, import, backup).
11. **Repo hygiene:** no secrets, weights, large binaries or private photos committed (`git ls-files -z | xargs -0 du -ck | awk '$2=="total"{s+=$1} END{printf "%.1f MB\n", s/1024}'`; extension scan below). Do not use `xargs du -ch | tail -1`: xargs splits long file lists and `tail -1` then shows only the last batch..

## 2. Mechanical checks (run from the repo root; paste the output into the review)

```
# Frontend must not own SQL, shell or broad filesystem access
rg -n "execute_sql|sql\.execute|plugin-sql" src src-tauri/src
rg -n "plugin-shell|shell:|\"fs:|fs:allow|fs:scope" src-tauri/capabilities src-tauri/tauri.conf.json src-tauri/Cargo.toml
rg -n "from \"@tauri-apps/plugin-fs\"|@tauri-apps/plugin-shell" src

# No shell invocation of native tools
rg -n "Command::new\(\"(sh|bash|zsh|cmd|powershell)\"|\.arg\(\"-c\"\)|shell=True" src-tauri/src

# Permanent deletion only inside the operations module (and ideally nowhere in Phase 0-4)
rg -n "remove_file|remove_dir|fs::rename|fs::copy" src-tauri/src

# Panics in non-test code (use SINGLE quotes in zsh: double quotes break on "!")
rg -n 'unwrap\(\)|expect\(|panic!|todo!|unimplemented!' src-tauri/src -g '!*test*'

# Secrets and keys
rg -n "AIza[0-9A-Za-z_-]{20,}|sk-[A-Za-z0-9]{20,}|api[_-]?key\s*[:=]" -g '!*.lock' -g '!docs/**' .

# Model weights or big binaries committed
git ls-files | rg -i "\.(gguf|onnx|safetensors|bin|dylib|dll|so)$"

# Float16 for embeddings (allowed only where TASK-0B explicitly validated it)
rg -n -i "float16|torch\.half|\.half\(\)|fp16" spikes src-tauri/src

# Leftover references to removed documents
rg -n "SPEC_AMENDMENTS|AI_ADDENDUM|docs/decisions|ADR-000" . -g '!reports/**' -g '!docs/ORCHESTRATION.md' -g '!docs/REVIEW_CHECKLIST.md'

# Rule file size
wc -c AGENTS.md
```

Expected: no matches in the first six groups except where the task explicitly allows them, and `wc -c` under 12000. A match is not automatically a failure; it needs an explanation in the report.

## 3. Phase-specific focus

| Phase | Look hardest at |
|---|---|
| 0 | Real versions verified (not recalled); clean-environment smoke test; capabilities minimal; ExifTool invoked without a shell; WAL + FK pragmas; migration test; custom-protocol thumbnail under CSP; path probe; for 0B: reference parity, real ONNX input names, no fp16 recommendation, no weights committed, licence reading by the owner |
| 1 | `scan_generation` logic (nothing marked missing after a cancelled/failed scan); non-UTF-8 path handling; symlinks off; bounded batches; job state persisted |
| 2 | Pagination caps; virtualization; thumbnail atomic writes; protocol/CSP scope; no image bytes over IPC |
| 3 | Parameterized SQL only; FTS5 sync; metadata size caps; malformed-file isolation |
| 4 | Full-file BLAKE3 before grouping; pHash versioning; no O(n²); no transitive merging; no auto-delete; fixture-based thresholds |
| 4B | Prefixes on text only, none on images; truncate then L2-normalise; dims never mixed; vectors keyed by model revision; preprocessing parity test against the Python reference; vision encoder on demand; memory report; cosine unit tests; advisory only |
| 5 | Plan → confirm → journal → execute; revalidation of fingerprints at execute; verify-before-remove; no overwrite; Trash only; recovery on restart; injected-failure tests |
| 6 | Per-provider consent; payload stripping; key only in OS credential store; no paths in prompts; Rust-side validation; budgets; provider failure isolation |
| 7 | Clean-machine install on both OSes; licenses and notices; logging redaction |

## 4. Reading a report critically

- A report that says "all tests pass" with no output: ask for output.
- "Works on both platforms" with one platform tested: reject the claim, not necessarily the task.
- Dependency introduced to avoid writing 20 lines: ask for justification.
- Large diff for a small task: suspect scope creep or generated boilerplate.
- Version numbers that look familiar but old: the model may have recalled rather than verified; check against the registry.
- Fixes that weaken a test or a safety check to get green: reject.

## 5. Verdict template

```
Verdict: ACCEPT | FIX | REJECT
Scope: ok / issues
Evidence: ok / missing: <list>
Safety invariants: ok / violated: <which>
Fixes required (numbered, minimal):
1.
Decisions needed from owner:
1.
Status updates: <STATUS.md changes>
Next task: <ID or "wait">
```

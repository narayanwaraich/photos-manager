# Orchestration Playbook

How this project is run. Three roles, one loop.

| Role | Who | Does |
|---|---|---|
| Product owner | Narayan | Decides scope, approves risky decisions, runs the tools (Mac primary; Windows PC for validation), commits and tags accepted work |
| Orchestrator / reviewer | Claude (in the "Photo Manager" Project) | Writes task files, reviews reports and diffs against the spec and checklist, updates `docs/STATUS.md`, issues the next task |
| Implementer | Antigravity with Gemini 3.8 Flash | Executes exactly one task file, writes a report, stops |

**One place to look:** at the end of every Claude conversation, Claude overwrites `docs/NEXT_STEPS.md` with exact step-by-step instructions for you (commands, what you should see, what to do if not). Open that file first. Lessons on Rust and Git accumulate in `docs/LEARNING.md`; the current lesson is also inside NEXT_STEPS, so you never have to hunt.

Claude cannot see your repository or run your code. Reviews are only as good as the evidence you bring back (section 4).

---

## 1. Day-one runbook

1. Create a new folder `photos-manager` and copy this whole pack into it (keep the structure). Delete the old `ANTIGRAVITY_FIRST_TASK.md` and old `IMPLEMENTATION_SPEC.md` if you have them; the files in this pack replace them. There is now exactly one spec file.
2. `git init`, commit the docs as "docs: initial pack" on `main`. Create a private GitHub repo and push (TASK-0C needs it).
3. Open the folder as a workspace in Antigravity. Start a new conversation scoped to that folder.
4. Rules check. Ask: *"What rules are you following in this workspace? List the safety invariants."* If it cannot name them, the rules did not load. Check `AGENTS.md` and `.agents/rules/00-project.md` are in the workspace root, and that you have no other rule files or a conflicting `~/.gemini/GEMINI.md`. Loading behaviour is reported by third-party guides; confirm in your Antigravity version.
5. Run **TASK-0A** with the start prompt below.
6. In a **separate** conversation (and separate branch), run **TASK-0B**. It does not depend on 0A.
7. Bring both reports back here (section 4). After both are accepted, run **TASK-0C**.
8. When you set up the Windows PC (any time before TASK-0C review), follow section 6 once to install prerequisites and clone the repo. Optional skills setup is in `docs/ANTIGRAVITY_SETUP.md`.

Do the 0A review before anything else. If 0A fails its stop conditions, the whole plan may change (spec §3.3 fallback ladder).

## 2. Prompts

**Start a task** (new Antigravity conversation each time):

```
Read AGENTS.md and tasks/TASK-<ID>-*.md. Do exactly that task on branch task/<ID>.
Do not start any other task. When finished, write reports/REPORT-<ID>.md and stop.
```

**Send back for fixes** (same conversation, after review):

```
Review feedback for TASK-<ID>. Fix only the items below on the same branch,
re-run pnpm verify, and update reports/REPORT-<ID>.md (add a "Fixes" section). Do not add scope.
<paste the numbered fix list from the review>
```

**Ask Claude to review** (paste into this Project's chat):

```
Review TASK-<ID>. Evidence below.
<REPORT-<ID>.md>
<git diff --stat main...task/<ID>>
<pnpm verify log tail>
<any files I list as "also needed">
```

**Start a new Claude conversation (inside this Project)** — recommended for each review, so chats stay short. The Project docs persist; the old chat does not. Paste:

```
Orchestrator session. Read docs/NEXT_STEPS.md and docs/STATUS.md, then the files STATUS.md points to for the current task.
Review TASK-<ID>. Evidence attached: <REPORT-<ID>.md, verify log tail, any files requested>.
```

Everything Claude needs to carry over must live in the Project docs: `docs/STATUS.md` (state), `reports/REPORT-<ID>.md` and `reports/REVIEW-<ID>.md` (evidence and verdicts), and the spec. Claude updates STATUS and writes the review file at the end of each review; if a decision was made only in chat, ask Claude to write it into the docs before you close the chat.

## 3. The loop

```
Claude issues TASK ──► you start Antigravity ──► agent works on branch task/<ID>
        ▲                                                   │
        │                                                   ▼
   Claude updates STATUS,               agent writes REPORT-<ID>.md and stops
   issues next TASK  ◄── verdict ◄── you bring evidence to Claude
        │
   ACCEPT → you merge to main and tag  `accepted/<ID>`
   FIX    → fix-prompt back to Antigravity, repeat
   REJECT → discard branch, Claude rewrites the task
```

Rules of the loop:

- **One task per conversation, one branch per task.** Long conversations drift; fresh ones re-read `AGENTS.md`.
- **Never merge before ACCEPT.** The agent's own "done" is a claim, not a result.
- **Tasks are small.** If a task needs more than a day of review, Claude splits it.
- **Run `pnpm verify` yourself** before bringing a report back. A report that says "passes" while your run fails is the most important finding in any review.
- **Gates are real.** A phase does not start until the previous gate in `docs/STATUS.md` is ticked by an ACCEPT.
- **Spec changes are proposals.** If the agent finds the spec wrong, it says so in the report. Claude decides and issues a revised `IMPLEMENTATION_SPEC.md` (one file, edited in place with a dated change-log line at the bottom); there are no side documents or amendment files.

## 4. What to bring back for a review

Always: `reports/REPORT-<ID>.md`, `git diff --stat main...task/<ID>`, the tail of the `pnpm verify` log.

Add when relevant:
- new or changed `Cargo.toml` / `package.json` and the lockfile diff summary (`git diff --stat -- '*.lock' pnpm-lock.yaml`);
- the files Claude asks for by name (usually the module that touches the filesystem, the DB, the capabilities file, or a migration);
- raw output of the grep checks in `docs/REVIEW_CHECKLIST.md` §2;
- screenshots for UI tasks.

Do not paste the whole repo. Do not paste API keys, tokens or personal photos.

## 5. Working with Gemini 3.8 Flash

- Google's model card shows strength on long-horizon coding and weaker results on a harder general terminal-agent benchmark. The workflow reflects that: small explicit tasks, written acceptance criteria, command-verified claims.
- Its knowledge ends in March 2026. The most common failure will be confident use of an old crate API, a wrong Tauri 2 plugin name, or an invented flag. `AGENTS.md` forbids this, and the checklist checks for it.
- Use Antigravity's planning mode for tasks that touch more than a few files, if your version offers it. Raise the model's effort setting for concurrency, the operation journal, and crash recovery (Phases 1 and 5); keep the default for UI work. Setting names vary; the model card only says effort levels are customizable.
- If the agent keeps failing the same step, stop. Bring the log to Claude; the task may be mis-specified.
- Never grant the agent permission to run destructive commands automatically against anything outside the project folder.

## 6. Windows PC validation (the Mac is primary; the Windows PC is always available)

CI proves a build works; the Windows PC proves it behaves. Run this **at the end of every phase whose gate says "Windows manual pass"** (Phases 1, 2, 5, 7 and any task touching paths, drives, Trash or packaging), and once during Phase 0.

One-time setup: install Git, Node LTS, pnpm, Rust (MSVC toolchain) with Visual Studio Build Tools, WebView2 runtime (present on Win 11); then clone the private repo. The exact commands live in `README.md` and `docs/windows-validation.md` (written in TASK-0C).

Per validation (about 20–30 minutes):

1. `git fetch && git checkout task/<ID>` (or `main` after merge), `pnpm install`, `pnpm verify`. Paste the tail of the log into the report or the review.
2. `pnpm tauri dev` and run the **smoke checklist**:
   - folder with non-ASCII names, emoji, and a path longer than 260 characters;
   - add a root on a second drive or USB stick; unplug it mid-scan; plug it back in (expect `offline`, then recovery, never "missing");
   - HEIC/AVIF/WebP/TIFF samples (what the bundled libvips actually supports);
   - Recycle Bin: trash a file, restore it; trash from the external drive (expect refusal or documented behaviour);
   - a file locked by another program (open in an editor) during a move;
   - 125% and 150% display scaling; dark and light theme;
   - idle CPU is near zero after jobs finish.
3. Record **pass / fail / not applicable yet** per line and any screenshots in `reports/WINDOWS-<phase>.md`; bring it to the review. Windows-only bug fixes go through the same task/branch loop (Antigravity can run on the Windows PC); do not fix on `main`.
4. A Windows failure blocks the gate exactly like a failure on the Mac.

## 7. Branching and tagging

- `main` always passes `pnpm verify`.
- Branch `task/<ID>`; squash-merge on ACCEPT; tag `accepted/<ID>`.
- Phase tags: `phase-0-complete`, etc., only after the gate is ticked.

## 8. Phase map

| Phase | Tasks | Gate |
|---|---|---|
| 0 Feasibility | 0A scaffold + DB + libvips/ExifTool + protocol/CSP · 0B embedding runtime spike · 0C CI + clean-machine packaging | All three accepted; native deps proven on both OSes; embedding runtime decision recorded (or semantic search deferred); Windows PC pass done once |
| 1 Catalogue and scanning | Claude writes after Phase 0 | Two roots scanned; restart mid-scan leaves no wrong "missing" records; Windows manual pass |
| 2 Grid and viewer | | 10k/100k synthetic catalogue; no originals modified; Windows manual pass |
| 3 Metadata and search | | Malformed metadata never crashes a scan |
| 4 Exact and visual duplicates | | Labelled corpus test; no auto-delete |
| 4B Local embeddings and semantic search | | Needs 0B; thresholds calibrated on fixtures |
| 5 Safe file operations | | Injected-failure tests at every boundary on both OSes; Windows manual pass (locked files, external drive) |
| 6 Cloud AI (multi-provider) | | No key in bundle or logs; no upload during scan |
| 7 Hardening and release | | Clean-machine smoke test on both OSes; license audit |
| Later versions (spec §13) | RAW previews · import · backup · maps · video · faces, one at a time, each after an ADR | Per ADR |

Only the next task is written in detail. Later tasks are written when the previous gate passes, so they use what was learned.

## 9. Escalation rules

Stop and ask the owner (do not let the agent or Claude decide alone) when:
- a stack change (spec §3.3 step 4, Electron) is proposed;
- a license or model-distribution question appears (spec D5/D6: bundling or hosting model weights, redistribution);
- a later-version feature (spec §13) is about to start (needs its ADR first);
- a feature would send user photos off-device in a new way;
- any file operation could delete or overwrite user data without the dry-run and journal;
- the cost of a cloud AI feature is non-trivial (promotional Gemini pricing ends 31 Dec 2026).

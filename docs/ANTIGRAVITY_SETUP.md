# Antigravity Setup and Skills Guide

Antigravity versions change quickly. Paths and limits below come from Google documentation and third-party guides read in November 2026; confirm them in your installed version (ask the agent: "Which rules and skills are loaded in this workspace?").

## 1. Rules (already in the pack)

- `AGENTS.md` (workspace root) holds the rules. `.agents/rules/00-project.md` points to it. Keep each rule file under 12,000 characters.
- Do not add a second rule file with different instructions. Do not keep a conflicting global `~/.gemini/GEMINI.md`.

## 2. Skills in one paragraph

A skill is a folder with a `SKILL.md` (name + description in frontmatter, instructions in the body). The agent sees only the descriptions up front and loads the full body when a task matches, so skills cost almost nothing until used. Global skills live in `~/.gemini/antigravity/skills/<name>/`; workspace skills in `./.agent/skills/<name>/` (some builds use `.agents/skills/`; check yours). Workspace skills override global ones with the same name.

**Rules vs skills:** safety invariants stay in `AGENTS.md` (always loaded). Skills are procedures the agent follows when relevant. Never put a safety rule only in a skill.

## 3. Project skills (ready to copy)

The pack includes three in `docs/antigravity-skills/`. Copy each folder into your workspace skills path:

| Skill | Use |
|---|---|
| `verify-before-use` | Stops the model from using stale or invented crate, plugin, flag or model-ID knowledge (its largest known weakness) |
| `write-task-report` | Makes every task end with `pnpm verify`, real evidence and a stop |
| `fileop-safety-review` | Self-review checklist for any code touching user files (Phase 5 especially) |

Good next ones to add when the need appears (ask Claude to draft them): `add-sqlx-migration` (additive migration + migration test), `tauri-capability-audit` (permissions minimal, CSP intact), `windows-path-review` (long paths, reserved names, case-insensitivity, NFC/NFD), `make-synthetic-fixtures`.

## 4. General coding skills worth having globally

Prefer a small, well-chosen set (the sweet spot is roughly 10–30 total; more dilutes selection):

- **Systematic debugging:** reproduce, isolate, hypothesise, one change at a time, no shotgun fixes.
- **Test-first / test-writing:** failing test first; property tests for path and plan logic.
- **Code review (self):** correctness, security, error handling, scope creep, before reporting.
- **Git hygiene:** small commits, conventional messages, never force-push.
- **Security review:** injection, path traversal, secrets, dependency risk.
- **Rust conventions:** error handling with `thiserror`, no `unwrap` in non-test code, `clippy` clean.
- **React/TypeScript conventions:** strict types, no `any`, accessibility basics.

## 5. Where to find skills, and how to vet them

- Anthropic's public skills repository and community collections (for example "awesome" lists for Antigravity/agent skills) are starting points. Treat third-party skills as code you are about to run: read the whole `SKILL.md` and every script in the folder.
- Reject skills that run shell commands you do not understand, fetch remote scripts, ask for secrets, disable safety prompts, or tell the agent to ignore rules.
- Prefer instruction-only skills. Scripts inside skills may be blocked by the sandbox anyway.
- Re-read a skill after any update. Pin by copying, not by auto-sync.
- After installing, test with: "List the skills you can use for this workspace and say when you would use each."

## 6. Optional: effort and planning settings

Use planning mode for tasks that touch more than a few files, and a higher effort setting for concurrency, the operation journal and crash recovery (Phases 1 and 5). Setting names vary by version.

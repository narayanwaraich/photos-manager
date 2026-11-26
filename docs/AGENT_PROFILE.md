# Coding agent profile: Gemini 4 Hydrogen

Written 2026-11-25 by the orchestrator. For task planning it replaces §5 of `docs/ORCHESTRATION.md`. Recorded in `docs/STATUS.md` as D31 and in spec v2.1.7 (sync-26, 2026-11-26).

**Owner decisions, 2026-11-25:** Hydrogen replaces Gemini 3.8 Flash entirely. Plan as if the agent quota is unlimited (the owner will say if that changes). Plan bigger tasks, but keep the evidence rules and the stop conditions. Treat every claim about Hydrogen as unproven until our own reviews show it, and learn as we go. The owner is trying effort `high` for a while.

## 1. What the sources say (fetched 2026-11-25)

Labels: **V** = Google's own statement. **T** = third party, method not checked by me. I ran nothing myself.

| Topic | Gemini 3.8 Flash | Gemini 4 Hydrogen | Source |
|---|---|---|---|
| Context | 1M input, 64K output, separate budgets | 1,048,576 shared by input, thinking and output; 65,536 per synchronous reply, up to 1M with long decode | V (AI Studio docs) |
| Knowledge cutoff | March 2026 | March 2026, "in some domains less up to date" | V (both model cards) |
| Effort levels | customisable | low, medium, high, xhigh. API default is high; the Antigravity blog says medium is the default there | V |
| Terminal-Bench 4.1 | 18.2% | 57.0% (cheating-adjusted). Claude Sonnet 5.5 is 63.9% in the same table | V (model card) |
| Speed | | 2.5x faster at high | V. T: 1.3 to 3.7 min per task (Artificial Analysis, seen only through a summary) |
| Fabricated claims | | "more than 5x" fewer | V. T: Puter, 16 prompts x 3 runs: said what was missing 48/48 vs 40/48; all of 3.8 Flash's false answers were about software (a nonexistent npm flag) |
| Shortcutting and boundaries | | misaligned actions and task shortcutting 12x lower; overreach up to 81% lower; up to 2x less likely to bypass permission boundaries; 96.7% on an internal honesty eval | V only |
| Coding sessions | 42.2%, $1.83, 18.4 min | 71.6%, $1.28, 6.9 min | T: bito.ai blog, method not checked |
| Effort scaling | | AutomationBench: low 30.6%, medium 46.7%, high 51.1% at $0.18, $0.41, $0.82 per task | T: Zapier |
| Refusals | | unjustified refusals +5.57 points vs 3.8 Flash | V (model card) |
| Stated limits | hallucinations, slowness | hallucinations, occasional slowness or timeouts, more tokens at higher levels; xhigh "can cause overthinking on simpler prompts" | V |

Cautions:
- Benchmark doubts: Epoch's review of Terminal-Bench 4.0 gave the verdict "Flawed" (4.1 hardens the verifiers); a forum thread asks DeepMind about mixed result sources and best-of-three reporting for Hydrogen's table; a Bloomberg report (via 9to5Google) said Gemini 4 Argon did worse in real coding than its benchmarks (about Argon, not Hydrogen).
- Plan and quota: the Antigravity blog says Hydrogen rolls out to paid Pro and Ultra subscribers, while the owner's picker shows Hydrogen usable and Carbon, Argon and Neon marked "Paid Pro". I do not know the plan. Forums complain about Antigravity quota burn. Per the owner, planning assumes no quota limit.
- Antigravity's "Orchestrator" (Carbon plans, Hydrogen executes) is not in the owner's picker. Our orchestrator is Claude. Ignored.

## 2. What follows from it

- The cutoff is unchanged, so `AGENTS.md` "do not trust memory for versions" stays in full.
- The vendor's best claims (fewer invented facts, fewer shortcuts, respects boundaries) hit exactly the failures we saw in 1A to 1E. They are Google's numbers, so they are hypotheses H1 to H3 in §4.
- Higher effort costs time and tokens, which are free to us now. Our recurring defects (hollow tests, report claims without a source) are about care, not speed.

## 3. Planning rules from now on (starting values: my judgement, not measured)

**Effort.**
- `high` for every task and every fix round. Reasons: the docs recommend it for complex coding and agentic work; Zapier's data shows +4.4 points from medium to high at about twice the cost per task; quota is not our constraint.
- `xhigh` only when the task file says so: tasks whose gate is concurrency, crash recovery or file safety (Phase 5 journal and recovery), or a fix round where the same finding failed twice at `high`. The docs warn about overthinking and cost; we have no evidence of benefit.
- No `low` or `medium` for tasks.
- The owner sets the level in the picker. The agent cannot see it, so the start prompt names it and the report header records "Effort (owner-reported)".

**Task size.**
- One task may cover what used to be two adjacent tasks of one phase, if they share a gate or a risk theme.
- Caps: at most 2 risk themes, each with its own named gate test and mutation demo; at most 12 numbered decisions; at most about 6,000 changed lines excluding lockfiles and the report (for scale: 1D was about 5,000 with its report, 1E about 4,100); at most one new migration; no new crate unless the task names it.
- Never: cross a phase gate or a Windows manual pass; combine a native dependency build with feature code.
- Phase 5 (file operations) stays split at plan, journal, execute and recovery until two Hydrogen tasks have been accepted in 2 rounds or fewer, and the failure-injection design has been reviewed first.
- A review above about 5,000 lines may be split in two sessions (code, then tests and report). The owner's evidence list stays the same.

**Task file shape.**
- Keep: goal, numbered decisions with reasons, gates (named tests plus mutation demos), out of scope, allowed paths, stop conditions, report rules, the owner's manual check.
- Shorten: "Facts" lists the agent can read from the repo; step-by-step parts for routine code (state the outcome and constraints); lists of files to read. Aim for roughly 60% of the old length.
- Add a **self-audit part** before the report: for every sentence that states a result (a count, an id, a line number, "passes"), the agent notes the command or log line it came from; anything without one is deleted or marked "inferred". Header ids come from `git log` and `gh run list`. This targets STATUS risk 26.
- Say what the agent may decide alone (names, internal structure, test layout) and what ends in a stop (the stop conditions).

**Unchanged.** `AGENTS.md` invariants and D17 (no secrets, tokens, repos, releases or settings); verify-before-use; raw output only, "from source, not run" labels; one task per conversation and per branch; no merge before ACCEPT; `pnpm verify` plus the laptop run as a gate; the agent never edits `AGENTS.md`, the spec, STATUS or NEXT_STEPS; read only the spec sections the task lists (the 1M window is not a reason to load everything: Hydrogen's long-context reliability is unproven); no parallel agents, `/swarm` or worktrees until the owner opts in (one branch per task keeps reviews traceable).

## 4. Calibration: learn as we go

**Baseline (Gemini 3.8 Flash, from STATUS):** review rounds to accept: 1A 2, 1B 2, 1C 2, 1C2 1, 1D 2 plus one small final commit, 1E 2 (code accepted in round 2, laptop run pending). Recurring blocking finding classes:
- tests that cannot fail or do not test what their name says (1A, 1B, 1D, 1E);
- report statements without a source (every task);
- real defects found only by review: production `unwrap` (1B), deferred transaction (1C), cancel hang and a dropped join error (1D), cancel race (1E).
Red CI with no code defect (advisories) is not the agent's.

**Record for each Hydrogen task** (in its REVIEW file): rounds to accept; blocking findings by class above; count of report statements without a source; real code defects; failures found only on the laptop; whether it stopped at a stop condition; any touch of forbidden paths or secrets.

**Hypotheses to test (vendor claims, unproven):** H1 fewer unsupported report statements; H2 fewer weakened or hollow tests and shortcuts; H3 respects the stop conditions and D17; H4 handles the larger scope in 2 rounds or fewer; H5 the cutoff is unchanged, so stale crate or API use still appears.

**Decision rule (my proposal).** After two Hydrogen tasks: if both were accepted in 2 rounds or fewer and no new defect class appeared, allow the next step up (three former tasks, about 8,000 lines). If any task needs 3 rounds or shows invented evidence, go back to the previous size and shorten the task file less.

## 5. Edits that need the owner or a later sync

- `AGENTS.md` (repo copy is authoritative; the Project copy is stale): replace "(the Gemini 3.8 Flash model card gives a March 2026 cutoff)" with "(the Gemini 4 Hydrogen model card gives a March 2026 cutoff)". The owner applies it (sync-26 step 1: one `sed` line, the file grows from 7,449 to 7,450 bytes); keep the file under 12,000 characters.
- `docs/STATUS.md`: D31 and the log entry are done (sync-26, 2026-11-26).
- `docs/ANTIGRAVITY_SETUP.md`: the repo copy at `c61dad7` contains no model name (`grep -n "3.8 Flash\|Gemini"` found nothing); the Project copy was not re-checked. `docs/ORCHESTRATION.md` (§5 and the roles table) already says Hydrogen in the Project copy and goes to the repo with sync-26.

Sources (fetched 2026-11-25): blog.google/intl/en-in/products/introducing-gemini-4-hydrogen; aistudio.google.com/docs/models/gemini-4-hydrogen and /docs/whats-new-gemini-4-hydrogen; deepmind.google/models/model-cards/gemini-4-hydrogen and /gemini-3-8-flash; antigravity.google/blog/gemini-4-hydrogen-in-google-antigravity; third-party: developer.puter.com (Hydrogen review), bito.ai, zapier.com/benchmarks, epoch.ai/benchmarks/terminal-bench-4/review, 9to5google.com (Gemini 4 report), discuss.ai.google.dev (benchmark thread).

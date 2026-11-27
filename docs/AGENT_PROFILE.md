# Coding agent profile: Gemini 4 Hydrogen

Written 2026-11-25 by the orchestrator. **Updated 2026-11-26 (§6: owner decision "go big first", after the TASK-1F review).** For task planning it replaces §5 of `docs/ORCHESTRATION.md`. Recorded in `docs/STATUS.md` as D31 and D34 and in spec v2.1.7 (D31) and v2.1.8 (D34).

**Owner decisions, 2026-11-25:** Hydrogen replaces Gemini 3.8 Flash entirely. Plan as if the agent quota is unlimited (the owner will say if that changes). Plan bigger tasks, but keep the evidence rules and the stop conditions. Treat every claim about Hydrogen as unproven until our own reviews show it, and learn as we go. The owner is trying effort `high` for a while.

**Owner decision, 2026-11-26:** give Hydrogen really big tasks first and find its limits from there; go smaller only if we hit hurdles (§6). This **supersedes the size caps in §3 and the decision rule in §4**; everything else in §3 and §4 stays.

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

## 3. Planning rules (starting values: my judgement, not measured)

**Effort.**
- `high` for every task and every fix round. Reasons: the docs recommend it for complex coding and agentic work; Zapier's data shows +4.4 points from medium to high at about twice the cost per task; quota is not our constraint.
- `xhigh` only when the task file says so: tasks whose gate is concurrency, crash recovery or file safety (Phase 5 journal and recovery), or a fix round where the same finding failed twice at `high`. The docs warn about overthinking and cost; we have no evidence of benefit. (Changing size and effort in the same task would hide which one mattered: one variable at a time.)
- No `low` or `medium` for tasks.
- The owner sets the level in the picker. The agent cannot see it, so the start prompt names it and the report header records "Effort (owner-reported)".

**Task size. (Superseded on 2026-11-26 by §6; kept for the record.)** One task may cover what used to be two adjacent tasks of one phase; at most 2 risk themes; at most 12 numbered decisions; about 6,000 changed lines; one migration; no new crate unless named. Never: cross a phase gate or a Windows manual pass; combine a native dependency build with feature code. Phase 5 file operations stay split (see §6). A review above about 5,000 lines may be split in two sessions (code, then tests and report).

**Task file shape.**
- Keep: goal, numbered decisions with reasons, gates (named tests plus mutation demos), out of scope, allowed paths, stop conditions, report rules, the owner's manual check.
- Shorten: "Facts" lists the agent can read from the repo; step-by-step parts for routine code (state the outcome and constraints); lists of files to read.
- The **self-audit part** (Claims table) before the report stays: for every sentence that states a result (a count, an id, a line number, "passes"), the agent notes the command or log line it came from; anything without one is deleted or marked "inferred". Header ids come from `git log` and `gh run list`. This targets STATUS risk 26.
- Say what the agent may decide alone (names, internal structure, test layout) and what ends in a stop (the stop conditions).

**Unchanged.** `AGENTS.md` invariants and D17 (no secrets, tokens, repos, releases or settings); verify-before-use; raw output only, "from source, not run" labels; one task per conversation and per branch; no merge before ACCEPT; `pnpm verify` plus the laptop run as a gate; the agent never edits `AGENTS.md`, the spec, STATUS or NEXT_STEPS; read only the spec sections the task lists (the 1M window is not a reason to load everything: Hydrogen's long-context reliability is unproven); no parallel agents, `/swarm` or worktrees until the owner opts in (one branch per task keeps reviews traceable).

## 4. Calibration: learn as we go

**Baseline (Gemini 3.8 Flash, from STATUS):** review rounds to accept: 1A 2, 1B 2, 1C 2, 1C2 1, 1D 2 plus one small final commit, 1E 2. Recurring blocking finding classes:
- tests that cannot fail or do not test what their name says (1A, 1B, 1D, 1E);
- report statements without a source (every task);
- real defects found only by review: production `unwrap` (1B), deferred transaction (1C), cancel hang and a dropped join error (1D), cancel race (1E).
Red CI with no code defect (advisories) is not the agent's.

**Record for each Hydrogen task** (in its REVIEW file): rounds to accept; blocking findings by class above; count of report statements without a source; real code defects; failures found only on the laptop; whether it stopped at a stop condition; any touch of forbidden paths or secrets; **and, since §6: size in changed lines per theme, which theme (if any) was left half done, what the agent said was hard.**

**Hypotheses (vendor claims, unproven):** H1 fewer unsupported report statements; H2 fewer weakened or hollow tests and shortcuts; H3 respects the stop conditions and D17; H4 handles the larger scope in 2 rounds or fewer; H5 the cutoff is unchanged, so stale crate or API use still appears.

**Results so far.**
- **1F (2026-11-26, reports/REVIEW-1F.md):** accepted in 1 round, about 3,900 changed lines, two themes, no code finding, no invented output seen, four small report slips, one stop-free honest deviation caused by an inconsistency in my task file. H1 supported, H3 supported, H4 supported, H2 not contradicted but not established (I read few test bodies), H5 no sign. One data point.

**Decision rule (the 2026-11-25 proposal) is replaced by §6.**

## 5. Edits that need the owner or a later sync

- `AGENTS.md` (repo copy is authoritative; the Project copy is stale): the cutoff sentence now says Hydrogen (done in sync-26, 7,450 bytes; the verify logs show 7,439 characters, the same text counted in characters instead of bytes).
- `docs/STATUS.md` D31 is done. D34 (this update) and the 1F results are in `docs/STATUS-sync-27-edits.md` until the next full STATUS rewrite.

## 6. Owner decision 2026-11-26: go big first, shrink on hurdles (D34)

**Policy.** Tasks are sized to find Hydrogen's limit, not to avoid it. The first big task is TASK-1G. Limits for a big task, as a starting point (my judgement, to be moved by evidence):

| Item | Was (§3) | Now |
|---|---|---|
| Risk themes per task | 2 | up to 3 (1G has 3) |
| Numbered decisions | 12 | about 20 |
| Changed lines (without lockfiles and the report) | about 6,000 | about 10,000; the task says what to do at the limit |
| Migrations | 1 | 1 |
| New crates | none unless named | only those the task names, each with its licence and an advisory check (`cargo deny`) |
| Review | one session | split in two sessions above about 5,000 lines (code, then tests and report) |

**How a big task is built so that it can be reviewed and shrunk.**
- One commit per theme, each with `pnpm verify` green and pushed, in the order the task lists (smallest and most boundary-touching first, the new-crate theme last). If the agent must stop, it stops after a green theme commit and says what is left; the owner can then merge a smaller task.
- Every theme keeps its named gate tests and its mutation demo (shown failing, then reverted). The Claims table, raw outputs, report rules, stop conditions and D17 are unchanged.
- The report records, per theme, its size and what was hard; I record in the REVIEW file what I did not read.

**Never "big" (unchanged):** work that moves, renames or deletes the owner's photos (Phase 5: plan, journal, execute, recovery, undo, Trash) stays designed first and split, with `xhigh` when the task says so; no task crosses a phase gate or the Windows manual pass; no task mixes a native-dependency build with feature code; no parallel agents.

**Shrink triggers** (any one moves the next task back one size; two in one task move it back to the 1F size, about 4,000 lines, two themes):
1. three or more review rounds;
2. any invented evidence (an output the agent did not run, a quote that is not real);
3. a weakened or hollow test found, or a test deleted to get green;
4. a stop condition ignored, or a forbidden path, secret or token touched (D17);
5. a theme left half done while the agent moved on to the next;
6. a review that cannot be finished in two sessions.
A hurdle that is mine (an inconsistent task file, a wrong fact) does not count; I record it as mine.

**Grow rule.** If a big task is accepted in 2 rounds or fewer with none of the triggers, the next one may be bigger again (more themes or about 15,000 lines), still under the "never" list. The owner may also say "bigger" or "smaller" at any time; that overrides this table.

**What would change my mind about the review limit.** If I find, in a task of this size, a real defect that I missed in one session and the owner's gates did not catch, the limit is my reading, and the next task gets more gates (not fewer lines).

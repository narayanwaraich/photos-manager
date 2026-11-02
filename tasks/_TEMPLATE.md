# TASK-<ID>: <short title>

**Phase:** <n> · **Branch:** `task/<ID>` · **Depends on:** <accepted tasks>
**Orchestrator note:** <one or two lines of context>

## Read first
- `AGENTS.md`
- Spec sections: <e.g. §3.3, §3.4, §5.1>
- Other docs: <e.g. docs/native-dependencies.md>

## Goal
<One paragraph. What is true when this task is done?>

## In scope
1.
2.

## Out of scope (do not do)
-

## Allowed paths
<Folders/files the agent may create or edit>

## Steps
1.
2.

## Acceptance criteria
- [ ]
- [ ]

## Verification commands
```
pnpm verify
<task-specific commands>
```

## Stop conditions
Stop and report (do not work around) if:
- a spec/rule conflict appears;
- a dependency cannot be verified or does not meet AGENTS.md;
- <task-specific>.

## Deliverable
`reports/REPORT-<ID>.md` from the template. Then stop.

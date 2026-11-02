---
name: write-task-report
description: Use at the end of a task to run pnpm verify and produce reports/REPORT-<ID>.md from the template with real command output as evidence. Then stop.
---

# Write the task report

1. Run `pnpm verify`. If it fails, fix within scope or report the failure; never weaken a test or check to get green.
2. Run each command in the task file's "Verification commands" and keep the real output.
3. Copy `reports/_REPORT_TEMPLATE.md` to `reports/REPORT-<ID>.md` and fill it in. Every acceptance criterion needs a command plus output, or a file path. Write "not tested" where true.
4. State which OS you tested and what remains for the other OS. List new dependencies with exact version, source and license. List anything you did not do and any spec conflict or proposed spec change.
5. Run `git diff --stat main...HEAD` and make sure only Allowed paths changed.
6. Stop. Do not begin another task.

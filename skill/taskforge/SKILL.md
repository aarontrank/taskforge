---
name: taskforge
description: Manage local tasks with the taskforge CLI — create, inspect, search, and drive tasks through the eleven-state workflow (pending, running, in-review, changes-requested, merged, waiting-on-schedule, stuck, done, failed, cancelled). Use when asked to create/update/list/search tasks, track multi-step or multi-agent work, record what was done, or check what is blocked, overdue, or awaiting review. Also use when asked what to work on next.
allowed-tools: Bash(taskforge:*)
---

# TaskForge

You drive tasks through the `taskforge` CLI. Data lives in `~/.taskforge` (override with
`TASKFORGE_ROOT`) as plain markdown, so a human can read and edit it with any tool.

## Non-negotiables

1. **Always pass `--json`.** Every command emits the same envelope: `ok`, `command`,
   `taskforge_version`, `data`, `warnings`, `errors`. Branch on the **exit status**, then read
   `errors[0].code` — never parse prose.
2. **Read before mutating.** `taskforge task show --id <ID> --json` tells you the status,
   `version`, `blocked_by`, and `review_required` you are about to act against.
3. **Never edit `task.md` by hand.** Use the patch commands. Hand edits bypass the audit log,
   the version counter, and every guard below.
4. **Pass `--actor <you>`** on mutations. It is what the audit log records.
5. **Log your work** with `task add-worklog` after meaningful progress — what was done, what
   remains, what blocked you. Worklogs are execution records; `add-comment` is discussion.
6. **On `CONFLICT_VERSION_MISMATCH`,** re-read the task and retry only if the action still
   applies. Something else changed it under you.

## The status model, and the two rules that matter

```
open ─→ pending ─→ running ─→ in-review ─→ merged ─→ done
                      │           │                   ▲
                      │           └→ changes-requested┘ (back to running)
                      ├→ waiting-on-schedule
                      └→ stuck / failed / cancelled
```

- **`merged` is NOT done.** A merged review still awaits human acceptance. Moving to `done`
  is `task accept`, and that is a *human's* call — do not accept your own work unless asked.
- **`done` means merged AND accepted.** It is the only success terminal.

Consequences you will hit:

- A blocker sitting in `merged` does **not** release its dependent. `task start` refuses with
  `BLOCKED_BY_OPEN_TASK` until every blocker is `done`.
- `task complete` on a `review_required` task fails with `REVIEW_REQUIRED`. Use
  `task request-review` instead.
- `in-review` within its `expected_by` window is **waiting, not stuck**. Do not escalate it and
  do not mark it `stuck`. Only a wait that has passed `expected_by` is `stuck`.

Reaching each state: `task start`, `request-review`, `reject`, `merge`, `accept`, `complete`,
plus `pending`, `wait`, `block`, `fail`, `cancel` — or `task set-status --status <name>` for any
of them. All are guarded by the transition table.

Three fields carry the scheduling state: `review_id` (which review gates this), `expected_by`
(when the wait becomes overdue), `worker` (who holds it). Set them with `task set-review`.

## The loop

```bash
taskforge task list --status pending --json          # what is ready
taskforge task show --id TASK-0001 --json            # inspect before acting
taskforge task start --id TASK-0001 --actor me --json
taskforge task add-worklog --id TASK-0001 --actor me --text "Ported storage. Next: hooks." --json
taskforge task request-review --id TASK-0001 --actor me --json   # if review_required
```

## Reference

- Full command surface, every flag, and the error-code table: [reference.md](reference.md)
- Worked agent scenarios, including blocked work and review handoff: [workflows.md](workflows.md)

Read those files when you need a flag you do not know. They are not loaded until you do.

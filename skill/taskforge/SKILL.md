---
name: taskforge
description: Manage local tasks with the taskforge CLI — create, inspect, search, and drive tasks through the eleven-state workflow (open, pending, running, in-review, changes-requested, merged, waiting-on-schedule, stuck, done, failed, cancelled). Use when asked to create/update/list/search tasks, track multi-step or multi-agent work, record what was done, or check what is blocked, overdue, or awaiting review. Also use when asked what to work on next.
allowed-tools: Bash(taskforge:*)
---

# TaskForge

You drive tasks through the `taskforge` CLI. Data lives in `~/.taskforge` (override with
`TASKFORGE_ROOT`) as plain markdown, so a human can read and edit it with any tool.

## Non-negotiables

1. **Always pass `--json`.** Every command emits the same envelope: `ok`, `command`,
   `taskforge_version`, `data`, `warnings`, `errors`. Branch on the **exit status**, then read
   `errors[0].code` — never parse prose.
2. **Read `warnings` on success too.** A command can exit 0 and still tell you something went
   wrong *after* the change committed — a notification hook that never fired, an audit line that
   could not be written. `ok: true` means the mutation happened, not that nothing went wrong.
3. **Read before mutating.** `taskforge task show --id <ID> --json` tells you the status,
   `version`, `blocked_by`, and `review_required` you are about to act against.
4. **Never edit `task.md` by hand.** Use the patch commands. Hand edits bypass the audit log,
   the version counter, and every guard below.
5. **Pass `--actor <you>`** on mutations. Every command that accepts it records it in the audit
   log, against the name of the action — so `task audit` answers who changed what.
6. **Log your work** with `task add-worklog` after meaningful progress — what was done, what
   remains, what blocked you. Worklogs are execution records; `add-comment` is discussion.
7. **On `CONFLICT_VERSION_MISMATCH`,** re-read the task and retry only if the action still
   applies. Something else changed it under you. Every command that changes the task record takes
   `--version` — the status transitions and the field setters alike. (`add-worklog` and
   `add-comment` do not: they append to their own files and carry no version to conflict on.)
8. **`IO_ERROR` means nothing was stored.** The task is unchanged on disk; whatever you were
   reporting upward did not happen. Never treat it as a partial success.

## The status model, and the two rules that matter

```
open ─→ pending ─→ running ─→ in-review ─→ merged ─→ done
                      │           │                   ▲
                      │           └→ changes-requested┘ (back to running)
                      ├→ waiting-on-schedule
                      └→ stuck / failed
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
- **In-flight work cannot be cancelled directly.** `cancel` is legal only from `open`,
  `pending`, `stuck`, and `failed`. To abandon a `running` or `in-review` task, `block` it first
  (→ `stuck`), then `cancel`. Deliberate: it makes dropping live work a two-step decision. Once a
  review has **merged**, even that route closes — a merged task takes only `accept` or `fail`.
- **One level of nesting.** `--parent` on a task that is already a subtask is refused with
  `INVALID_PARENT`.

Reaching each state: `task start`, `request-review`, `reject`, `merge`, `accept`, `complete`,
plus `pending`, `wait`, `block`, `fail`, `cancel` — or `task set-status --status <name>` for any
of them. All are guarded by the same transition table, `set-status` included, so it is a
shorthand and not a way around the guards. The full legal-move matrix is in
[reference.md](reference.md).

Four fields carry the execution state: `reviews` (which reviews gate this — plural, because work
crossing package boundaries needs one each) and `expected_by`
(when the wait becomes overdue) via `task set-review`; `worker` (who holds it) and `checkout`
(which dev workspace they hold it in) via `task set-worker`. Note `checkout` is the *development*
workspace — distinct from the global `--workspace` flag, which selects a taskforge partition.

Three fields say what the work *is*, for reporting: `kind` (a **closed** set — `feature`, `bug`,
`chore`, `investigation`, `oncall`, `doc`; anything else is `INVALID_KIND`), `tags` (open-ended
projects and themes), and `ticket` (the external tracker item, an opaque string). Set them at
creation with `--kind`/`--ticket`/`--tag` or later with `set-kind`/`set-ticket`/`add-tag`. A task
with no kind is unclassified, which is honest — do not guess one to fill the field.

## Finding what stalled

```bash
taskforge task list --overdue --json      # past its expected_by
taskforge task list --stale 7d --json     # untouched for over a week
```

Both skip finished tasks and both *include* `merged` — the state that rots while waiting on a
human to accept it. Use these instead of eyeballing the whole board.

If either of those errors with *unexpected argument*, **do not conclude the flag does not exist**:

```bash
taskforge doctor --json                   # state: fresh | stale | unknown
```

`stale` means the installed binary predates the source and `note` carries the reinstall command.
These two flags shipped and then sat unreachable for a week for exactly that reason, while
`--version` reported the same number for both builds. One command settles it.

## Owners are a registry

`--owner` and `--reviewer` must name an **already-registered** owner, or the command fails with
`OWNER_NOT_FOUND`. The names are per-machine, so never assume one from an example — read them:

```bash
taskforge owner list --json
taskforge owner add --name <name> --type human|agent --json   # if the one you need is missing
```

`--actor` is free text and needs no registration; it is only recorded in the audit log.

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

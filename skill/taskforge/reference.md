# taskforge CLI reference

Every command accepts `--json` (**agents should always pass it**), `--root <path>` (defaults to
`$TASKFORGE_ROOT`, else `~/.taskforge`), and `--workspace <name>` (defaults to `main`).
Exit status is 0 on success, 1 on error.

Without `--json` the output is human-readable text: `task list` renders a board-shaped table,
`task show` a field block, a mutation a single confirmation line, and an error goes to **stderr**
as `error: CODE: message`. Never parse that form — it is for people.

## Response envelope

```json
{
  "ok": true,
  "command": "task start",
  "taskforge_version": "0.2.0",
  "data": { "...": "the task, or a list, or a small ack object" },
  "warnings": [],
  "errors": []
}
```

On failure `ok` is `false`, `data` is `null`, and `errors[0]` carries `{ code, message }`.

`warnings` uses the same `{ code, message }` shape and appears on **successful** responses. It
reports what went wrong *after* the mutation committed, which cannot fail the command without
denying a change that really happened. Some entries add a `detail` object with structured
context. Currently emitted:

| Warning code | Meaning | `detail` |
|---|---|---|
| `HOOK_FAILED` | A hook exited non-zero, timed out, or could not be started | `hook_id`, `exit_code`, `timed_out`, `started` |
| `AUDIT_WRITE_FAILED` | The change is stored; its `audit.log` line is not | — |
| `RECURRENCE_WRITE_FAILED` | A recurring task completed; its successor was not created | — |

```json
{
  "ok": true,
  "command": "task start",
  "data": { "status": "running" },
  "warnings": [
    {
      "code": "HOOK_FAILED",
      "message": "hook \"notify\" timed out",
      "detail": { "hook_id": "notify", "exit_code": -1, "timed_out": true, "started": true }
    }
  ],
  "errors": []
}
```

## Setup

```bash
taskforge init --json
taskforge owner add --name aaron --type human --json
taskforge owner add --name agent --type agent --json
taskforge owner list --json
taskforge workspace add --name side --json
taskforge workspace list --json
```

Owners are validated: `--owner` and `--reviewer` must name a registered owner, or the command
fails with `OWNER_NOT_FOUND`.

## Creating and reading

```bash
taskforge task create --title "Port storage" --owner agent --actor aaron --json \
  [--description "..."] [--review-required] [--parent TASK-0001] \
  [--kind bug] [--ticket T-99] [--tag parser] [--tag backend]

taskforge task show   --id TASK-0001 --json
taskforge task list   --json [--status <status>] [--archived] [--overdue] [--stale 7d]
taskforge task search --text "storage" --json      # matches title and description
taskforge task tree   --id TASK-0001 --json        # task plus its subtasks
taskforge task audit  --id TASK-0001 --json        # mutation history: creation, status changes, field patches
```

`--parent` makes a subtask. One level of nesting only. `task list` hides archived and
soft-deleted tasks unless `--archived` is passed.

## Status transitions

Each takes `--id`, `--actor`, optional `--version <n>` for optimistic concurrency.

| Command | Legal from | Notes |
|---|---|---|
| `task start` | open, pending, changes-requested, waiting-on-schedule, stuck, failed | → running. Refuses while any blocker is not `done` |
| `task request-review` | running, waiting-on-schedule, stuck | → in-review |
| `task reject` | in-review | → changes-requested. Feedback landed |
| `task merge` | in-review | → merged. **Not** terminal |
| `task accept` | merged, running | → done. Human acceptance; the only success terminal. From `running` only when no review was demanded |
| `task complete` | running, merged | → done. Fails with `REVIEW_REQUIRED` if review was demanded and the task never went through it |
| `task pending` | open, stuck | → pending. Planned into a wave, not yet dispatched |
| `task wait` | running, stuck | → waiting-on-schedule. Slow non-review step, still on schedule |
| `task block` | pending, running, in-review, changes-requested, waiting-on-schedule | → stuck. Needs a decision, or a wait passed its `expected_by`. Only from states where work is live or queued — not from `open`, and not from a settled `merged`/`failed` |
| `task fail` | running, in-review, changes-requested, merged, waiting-on-schedule, stuck | → failed. `task start` retries it |
| `task cancel` | **open, pending, stuck, failed only** | → cancelled. In-flight work must be `block`ed first — see below |
| `task set-status --status <name>` | any legal move | Shorthand for the above; same guard |

**`cancel` cannot abandon in-flight work directly.** It is refused from `running`, `in-review`,
`changes-requested`, `waiting-on-schedule`, and `merged` with `INVALID_STATUS_TRANSITION`. To
drop a live task: `task block` (→ `stuck`), then `task cancel`. This is deliberate — walking away
from work someone may already be reviewing takes two decisions, not one.

**A `merged` task cannot be blocked or cancelled either** — its only moves are `accept`
(→ `done`) and `fail` (→ `failed`). So the `block`-then-`cancel` route does not apply once a
review has merged: to abandon work at that point, `fail` it. The reasoning is the same as for
`cancel`, one step further along — a merged review is a result, so discarding it is a decision
about that result rather than about a task in flight.

`done` and `cancelled` are immutable: no transition leaves either. `failed` is terminal in the
sense that no worker holds it, but `start` still retries it.

```bash
taskforge task start      --id TASK-0001 --actor agent --version 3 --json
taskforge task set-status --id TASK-0001 --status stuck --actor agent --json
```

All eleven statuses are reachable from the CLI. `set-status` is guarded by the same transition
table as the named commands, so it is a shorthand, not an escape hatch: an illegal move still
returns `INVALID_STATUS_TRANSITION`, and an unknown name returns `INVALID_STATUS`.

## Classification

What a task *is*, for reporting. Separate from its execution state.

```bash
taskforge task set-kind    --id TASK-0001 --kind bug        --actor aaron --json
taskforge task set-ticket  --id TASK-0001 --ticket T-99     --actor aaron --json
taskforge task add-tag     --id TASK-0001 --tag parser      --actor aaron --json
taskforge task remove-tag  --id TASK-0001 --tag parser      --actor aaron --json
```

`kind` is a **closed set**: `feature`, `bug`, `chore`, `investigation`, `oncall`, `doc`.
Anything else is refused with `INVALID_KIND`, and the error names the legal values. The set is
closed on purpose — free text drifts into `bug`/`bugfix`/`Bug` and the drift only surfaces as a
wrong number in a report months later. A task with no kind is *unclassified*, which is a
reportable bucket rather than a guess.

`tags` are open-ended: projects, themes, components. Adding a tag twice is a no-op. Use these
for anything that would otherwise want a new `kind`.

`ticket` is the external tracker item this task delivers — an opaque string. taskforge does not
know or care which tracker it came from, so it never validates or resolves it.

## Reviews

A task can be gated on **several** reviews, because work crossing package ownership boundaries
needs one review per package.

```bash
taskforge task add-review    --id TASK-0001 --review-id PR-1 --actor agent --json
taskforge task remove-review --id TASK-0001 --review-id PR-1 --actor agent --json
```

`set-review --review-id` **replaces** the whole list; `add-review` appends. Adding the same
review twice is a no-op.

Task files written before this field became plural carry a singular `review_id`. Those still
load — the old value becomes a one-element list — and writing the task back stores the plural
form, so reading and writing migrates it.

## Board fields

```bash
taskforge task set-review --id TASK-0001 --actor agent --json \
  [--review-id PR-4821] [--expected-by 2026-09-03T17:00:00Z]

`--review-id` here sets the review list to exactly that one review. To gate a task on more than
one, use `add-review`.

taskforge task set-worker --id TASK-0001 --actor agent --json \
  [--worker review-worker-1] [--checkout 3]
```

Omitted flags leave the current value alone, so one field can be set without clearing the others.

**`--checkout` is not `--workspace`.** `--checkout` records the *development* workspace the work
happens in — a numbered dev workspace, a git worktree name, a sandbox id. The global
`--workspace` flag selects which taskforge *partition* the task is filed in (`main`, `side`).
Putting a per-stream checkout id in `--workspace` would file every stream in its own partition
and break `task list`.

## Finding work that stopped moving

```bash
taskforge task list --overdue --json          # past its expected_by
taskforge task list --stale 7d --json         # untouched for over a week
taskforge task list --stale 12h --status running --json    # filters compose
```

`--stale` takes `12h`, `7d`, `2w`, or a bare number meaning days. Anything else — including `0`
and negatives — is refused with `INVALID_AGE` rather than silently matching everything.

Both filters **ignore the terminal statuses**. A task finished in January has not been touched
since, and that is correct rather than abandoned; surfacing it would bury the tasks that do need
attention.

`merged` is deliberately *not* ignored. It is the state that rots silently while waiting for a
human to `accept`, so it is exactly what these queries are for.

## Patch-style updates

```bash
taskforge task set-title            --id TASK-0001 --title "New title"        --actor aaron --json
taskforge task set-description      --id TASK-0001 --description "..."        --actor aaron --json
taskforge task assign               --id TASK-0001 --owner agent              --actor aaron --json
taskforge task set-reviewer         --id TASK-0001 --reviewer aaron           --actor aaron --json
taskforge task set-due              --id TASK-0001 --due-at 2026-12-01T00:00:00Z --actor aaron --json
taskforge task set-review-required  --id TASK-0001 --value true               --actor aaron --json
taskforge task add-blocker          --id TASK-0001 --blocked-by TASK-0007     --actor aaron --json
taskforge task remove-blocker       --id TASK-0001 --blocked-by TASK-0007     --actor aaron --json
taskforge task archive              --id TASK-0001 --actor aaron --json
taskforge task soft-delete          --id TASK-0001 --actor aaron --json
```

`--value` on `set-review-required` takes an explicit `true` or `false`.

**Every command here also takes `--version <n>`,** the same optimistic-concurrency guard the
status transitions use. Pass the version you read and a racing writer is refused with
`CONFLICT_VERSION_MISMATCH` instead of being silently clobbered. It is optional: omit it and the
patch applies unguarded.

```bash
taskforge task set-worker --id TASK-0001 --worker w1 --actor agent --version 7 --json
```

Every one of them records an audit entry naming `--actor` and the action — `set_title`,
`assign`, `add_blocker`, `set_kind`, `add_tag`, `add_review`, `archive`, and so on.

The classification and review setters above (`set-kind`, `set-ticket`, `add-tag`, `remove-tag`,
`add-review`, `remove-review`) are patch commands too: same `--version` guard, same audit entry.

`archive` and `soft-delete` set flags; they do **not** change `status`. An archived task keeps
whatever status it had.

## Notes and files

```bash
taskforge task add-worklog --id TASK-0001 --actor agent --text "Did X. Next: Y." --json
taskforge task add-comment --id TASK-0001 --actor aaron --text "Why this shape?"  --json

taskforge task add-attachment --id TASK-0001 --path ./spec.pdf   --mode copy --actor aaron --json
taskforge task add-artifact   --id TASK-0001 --path ./dist/out.json --mode link --actor agent --json
```

`--mode copy` duplicates the file into the task folder; `--mode link` records the path only.
A missing source file is an error (`ATTACHMENT_NOT_FOUND` / `ARTIFACT_NOT_FOUND`) rather than a
dangling reference.

`add-attachment` and `add-artifact` record a reference **on the task**, so they behave like the
patch commands: both take `--version` and both append an audit entry (`add_attachment` /
`add_artifact`).

`add-worklog` and `add-comment` do **not** take `--version`. They append to `worklog.md` and
`comments.md` and never touch the task record, so there is no version to conflict on and two
agents writing at once cannot lose each other's entry. Each entry carries its own `--actor` and
timestamp in the file.

## Recurrence

```bash
taskforge task set-recurrence --id TASK-0001 --frequency weekly --interval 1 \
  --due-strategy absolute --actor aaron --json
taskforge task clear-recurrence --id TASK-0001 --actor aaron --json
```

Completing a recurring task generates its successor automatically and returns
`next_occurrence_id` in `data`. The successor carries `prior_occurrence_id` back to its
predecessor, and inherits the schedule so it keeps recurring.

`--due-strategy`: `none` (successor has no due date), `absolute` (advance from the previous due
date), `relative` (advance from completion time). Month arithmetic clamps to the last valid day:
31 January plus one month is 28 February.

## Error codes

| Code | Meaning |
|---|---|
| `TASK_NOT_FOUND` | No such task id |
| `OWNER_NOT_FOUND` | Owner is not in the registry. The message names the registered owners, or tells you how to add the first one |
| `INVALID_STATUS_TRANSITION` | That move is not legal from the current status |
| `BLOCKED_BY_OPEN_TASK` | A blocker is not `done`. The message names which |
| `REVIEW_REQUIRED` | Cannot complete directly; request review instead |
| `CONFLICT_VERSION_MISMATCH` | `--version` did not match. Re-read and retry |
| `ALREADY_ARCHIVED` | Task is already archived |
| `ATTACHMENT_NOT_FOUND` / `ARTIFACT_NOT_FOUND` | Source file does not exist |
| `INVALID_STATUS` | Unrecognized `--status` filter value |
| `INVALID_PARENT` | `--parent` names a task that is already a subtask; one level only |
| `INVALID_KIND` | `--kind` is not one of the closed set; the message names the legal values |
| `INVALID_AGE` | `--stale` could not be read as an age; try `12h`, `7d`, `2w`, or a number of days |
| `OWNER_EXISTS` | Owner name already registered |
| `IO_ERROR` | A write failed. **Nothing was stored** — the task is unchanged on disk, and any status or version in the response would have been a fiction, so none is returned |

## Hooks

Configured in `<root>/config.json`, fired **after** a mutation commits. A hook that fails,
hangs, or does not exist never rolls the change back — hooks are notifications, not gates — and
is reported as a `HOOK_FAILED` entry in the response's `warnings`, with the hook's id, exit code,
whether it timed out, and whether it started at all. The command still exits 0.

Check `warnings` on success: a hook that silently never ran is the failure mode this reporting
exists to prevent.

```json
{
  "default_workspace": "main",
  "hooks": [
    {
      "id": "notify",
      "enabled": true,
      "event": "task.status_changed",
      "command": "sh",
      "args": ["-c", "cat >> ~/task-events.ndjson"],
      "workspace_filter": ["main"],
      "timeout_ms": 10000
    }
  ]
}
```

The payload arrives as JSON on the hook's **stdin**:

```json
{ "event": "task.status_changed", "workspace": "main", "task_id": "TASK-0001", "status": "running" }
```

The payload has **no trailing newline**, so a hook appending to an NDJSON log must add one
itself — `sh -c 'cat >> log; echo >> log'` rather than `cat >> log`, or every event lands on a
single concatenated line.

A hook exceeding `timeout_ms` is killed and reported with `timed_out: true`. A hook whose
command does not exist or is not executable reports `started: false` — distinct from a hook that
ran and exited badly, which would otherwise look identical (both carry `exit_code: -1`).

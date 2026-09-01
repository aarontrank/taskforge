# taskforge CLI reference

Every command accepts `--json` (always pass it), `--root <path>` (defaults to
`$TASKFORGE_ROOT`, else `~/.taskforge`), and `--workspace <name>` (defaults to `main`).
Exit status is 0 on success, 1 on error.

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
  [--description "..."] [--review-required] [--parent TASK-0001]

taskforge task show   --id TASK-0001 --json
taskforge task list   --json [--status <status>] [--archived]
taskforge task search --text "storage" --json      # matches title and description
taskforge task tree   --id TASK-0001 --json        # task plus its subtasks
taskforge task audit  --id TASK-0001 --json        # full mutation history
```

`--parent` makes a subtask. One level of nesting only. `task list` hides archived and
soft-deleted tasks unless `--archived` is passed.

## Status transitions

Each takes `--id`, `--actor`, optional `--version <n>` for optimistic concurrency.

| Command | Moves | Notes |
|---|---|---|
| `task start` | open/pending/changes-requested/stuck/failed → running | Refuses while any blocker is not `done` |
| `task request-review` | running → in-review | For `review_required` work |
| `task reject` | in-review → changes-requested | Feedback landed |
| `task merge` | in-review → merged | **Not** terminal |
| `task accept` | merged → done | Human acceptance; the only success terminal |
| `task complete` | running → done | Fails with `REVIEW_REQUIRED` if review was demanded |
| `task pending` | open/stuck → pending | Planned into a wave, not yet dispatched |
| `task wait` | running → waiting-on-schedule | Slow non-review step, still on schedule |
| `task block` | → stuck | Needs a decision, or a wait passed its `expected_by` |
| `task fail` | → failed | Attempted and failed. `task start` retries it |
| `task cancel` | → cancelled | Will never run, e.g. a dependency failed permanently |
| `task set-status --status <name>` | any legal move | Shorthand for the above; same guard |

```bash
taskforge task start      --id TASK-0001 --actor agent --version 3 --json
taskforge task set-status --id TASK-0001 --status stuck --actor agent --json
```

All eleven statuses are reachable from the CLI. `set-status` is guarded by the same transition
table as the named commands, so it is a shorthand, not an escape hatch: an illegal move still
returns `INVALID_STATUS_TRANSITION`, and an unknown name returns `INVALID_STATUS`.

## Board fields

```bash
taskforge task set-review --id TASK-0001 --actor agent --json \
  [--review-id CR-301625168] [--expected-by 2026-09-03T17:00:00Z]

taskforge task set-worker --id TASK-0001 --actor agent --json \
  [--worker addresscr-CR-301625168] [--checkout 3]
```

Omitted flags leave the current value alone, so one field can be set without clearing the others.

**`--checkout` is not `--workspace`.** `--checkout` records the *development* workspace the work
happens in — an `imdb-next-gen` number, a git worktree name, a sandbox id. The global
`--workspace` flag selects which taskforge *partition* the task is filed in (`main`, `side`).
Putting a per-stream checkout id in `--workspace` would file every stream in its own partition
and break `task list`.

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
| `OWNER_NOT_FOUND` | Owner is not in the registry |
| `INVALID_STATUS_TRANSITION` | That move is not legal from the current status |
| `BLOCKED_BY_OPEN_TASK` | A blocker is not `done`. The message names which |
| `REVIEW_REQUIRED` | Cannot complete directly; request review instead |
| `CONFLICT_VERSION_MISMATCH` | `--version` did not match. Re-read and retry |
| `ALREADY_ARCHIVED` | Task is already archived |
| `ATTACHMENT_NOT_FOUND` / `ARTIFACT_NOT_FOUND` | Source file does not exist |
| `INVALID_STATUS` | Unrecognized `--status` filter value |
| `OWNER_EXISTS` | Owner name already registered |
| `IO_ERROR` | Filesystem failure; message carries detail |

## Hooks

Configured in `<root>/config.json`, fired **after** a mutation commits. A hook that fails,
hangs, or does not exist is reported and never rolls the change back.

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

A hook exceeding `timeout_ms` is killed and reported with `timed_out: true`.

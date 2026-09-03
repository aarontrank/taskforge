# Agent workflows

## Picking up work

```bash
taskforge task list --status pending --json
taskforge task show --id TASK-0001 --json      # check blocked_by and review_required first
taskforge task start --id TASK-0001 --actor agent --json
```

If `start` returns `BLOCKED_BY_OPEN_TASK`, the message names the blockers. Do not force it and
do not clear the blocker to get moving — report it, or work the blocker instead.

## Review-gated work, end to end

```bash
taskforge task start          --id TASK-0001 --actor agent --json
taskforge task add-worklog    --id TASK-0001 --actor agent --text "Implemented X." --json
taskforge task request-review --id TASK-0001 --actor agent --json
taskforge task set-review     --id TASK-0001 --actor agent --json \
  --review-id PR-4821 --expected-by 2026-09-03T17:00:00Z
```

Then **stop**. The task is `in-review` and inside its window: waiting, not stuck. Do not poll
it aggressively, do not escalate, and do not move it yourself.

When feedback arrives:

```bash
taskforge task add-comment --id TASK-0001 --actor aaron --text "Missing error schema" --json
taskforge task reject      --id TASK-0001 --actor aaron --json
taskforge task start       --id TASK-0001 --actor agent --json   # changes-requested -> running
```

`reject` records the transition, not the reason — it takes no `--text`. Put the feedback in
`add-comment` first, so the task carries why it came back.

When the review merges:

```bash
taskforge task merge  --id TASK-0001 --actor agent --json   # merged: NOT done
taskforge task accept --id TASK-0001 --actor aaron --json   # done: the human accepts
```

## Reporting a genuine block

```bash
taskforge task add-comment --id TASK-0001 --actor agent --text "Needs a decision on X." --json
```

Then set it `stuck` only if it truly cannot proceed, or its `expected_by` has passed. `stuck` is
a request for human attention — using it for an ordinary wait trains everyone to ignore it.

## Breaking work down

```bash
parent=$(taskforge task create --title "Migrate storage" --owner agent --actor aaron --json \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["data"]["id"])')

taskforge task create --title "Port the model" --owner agent --actor aaron --parent "$parent" --json
taskforge task create --title "Port the CLI"   --owner agent --actor aaron --parent "$parent" --json
taskforge task tree --id "$parent" --json
```

One level of nesting only, and enforced: `--parent` on a task that is already a subtask is
refused with `INVALID_PARENT`. For ordering between subtasks use `add-blocker`, which is enforced
at `start` rather than merely advisory.

## Finding work that was abandoned

The failure this catches: a worker starts a task, opens a review, and its session ends without
closing anything. Nothing errors — the task simply stops moving.

```bash
taskforge task list --stale 7d --json      # nothing has touched these in a week
taskforge task list --overdue --json       # and these blew their expected_by
```

Neither is a problem report on its own. Read each one and decide: resume it, `block` it if it
needs a decision, or `accept` it if the review actually merged and only the bookkeeping is
missing. A `merged` task showing up here means it is waiting on a human, not stuck.

## Classifying work as you go

```bash
taskforge task create --title "Fix the parser" --owner agent --actor aaron \
  --kind bug --ticket T-99 --tag parser --json
```

Set `--kind` when you know it and leave it unset when you do not — an unclassified task is
reportable as such, whereas a wrong kind is a wrong number in a report nobody will re-derive.
`--tag` is for the project or theme and can be repeated.

## Concurrency

When more than one agent may touch a task, pass the version you read. This works on **every**
mutating command — the status transitions and the field setters alike:

```bash
v=$(taskforge task show --id TASK-0001 --json | python3 -c 'import json,sys; print(json.load(sys.stdin)["data"]["version"])')
taskforge task start      --id TASK-0001 --actor agent --version "$v" --json
taskforge task set-worker --id TASK-0001 --worker w1 --actor agent --version "$v" --json
```

On `CONFLICT_VERSION_MISMATCH`, re-read and decide whether the action still applies. Do not
retry blindly in a loop.

## Abandoning work in flight

`cancel` is refused from `running` and `in-review`. Dropping live work is two steps:

```bash
taskforge task add-comment --id TASK-0001 --actor aaron --text "Premise changed; dropping." --json
taskforge task block       --id TASK-0001 --actor aaron --json   # -> stuck
taskforge task cancel      --id TASK-0001 --actor aaron --json   # -> cancelled
```

If the review already **merged**, this does not work — `block` and `cancel` are both refused, and
the only moves left are `accept` and `fail`. Abandon it with `fail`:

```bash
taskforge task fail --id TASK-0001 --actor aaron --json   # merged -> failed
```

## When a command succeeds but warns

`ok: true` with a non-empty `warnings` means the mutation is stored and something after it was
not:

```bash
taskforge task start --id TASK-0001 --actor agent --json
# ok: true, status: running, warnings: [{ code: "HOOK_FAILED", ... }]
```

Do not retry the command — that would double-apply it. The task moved. Report the warning:
`HOOK_FAILED` means a notification never went out, so whoever was supposed to hear about this
did not. `AUDIT_WRITE_FAILED` means the trail is incomplete. Neither is fixed by repeating the
mutation.

An `IO_ERROR` is the opposite case: the write failed, nothing is stored, and retrying is exactly
right once the cause (a full disk, a read-only mount, a permissions problem) is dealt with.

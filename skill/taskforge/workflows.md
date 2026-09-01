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
  --review-id CR-301625168 --expected-by 2026-09-03T17:00:00Z
```

Then **stop**. The task is `in-review` and inside its window: waiting, not stuck. Do not poll
it aggressively, do not escalate, and do not move it yourself.

When feedback arrives:

```bash
taskforge task reject      --id TASK-0001 --actor aaron --text "Missing error schema" --json
taskforge task start       --id TASK-0001 --actor agent --json   # changes-requested -> running
```

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

One level of nesting only. For ordering between subtasks use `add-blocker`, which is enforced
at `start` rather than merely advisory.

## Concurrency

When more than one agent may touch a task, pass the version you read:

```bash
v=$(taskforge task show --id TASK-0001 --json | python3 -c 'import json,sys; print(json.load(sys.stdin)["data"]["version"])')
taskforge task start --id TASK-0001 --actor agent --version "$v" --json
```

On `CONFLICT_VERSION_MISMATCH`, re-read and decide whether the action still applies. Do not
retry blindly in a loop.

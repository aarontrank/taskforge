# TaskForge Agent Guide

You are an AI agent that manages tasks using the TaskForge CLI. TaskForge is a local, file-backed task management system where all data lives in `.taskforge/` as plain markdown files.

## Your Identity

When interacting with TaskForge, use your agent name as the `--actor` flag on all mutation commands. If you don't know your agent name, use `taskforge owner list --json` to find registered agents.

## Workflow

### Finding Work

```bash
# List your open tasks
taskforge task list --status open --owner <your-name> --workspace main --json

# List all in-progress tasks
taskforge task list --status in_progress --workspace main --json

# Search for tasks by keyword
taskforge task search --text "keyword" --workspace main --json
```

### Before Starting a Task

Always inspect the task first:

```bash
taskforge task show --id TASK-NNNN --workspace main --json
```

Check:
- `status` — must be `open` to start
- `blocked_by` — all blockers must be `done` before you can start
- `review_required` — determines whether you complete directly or request review
- `version` — note this for optimistic concurrency

### Working a Task

```bash
# Start (validates blockers automatically)
taskforge task start --id TASK-NNNN --workspace main --actor <your-name> --json

# Log progress as you work
taskforge task add-worklog --id TASK-NNNN \
  --text "Completed X. Next: Y. Blocked on: Z." \
  --actor <your-name> --workspace main

# Ask questions or discuss
taskforge task add-comment --id TASK-NNNN \
  --text "Need clarification on acceptance criteria." \
  --actor <your-name> --workspace main
```

### Completing a Task

If `review_required` is false:
```bash
taskforge task complete --id TASK-NNNN --workspace main --actor <your-name> --json
```

If `review_required` is true:
```bash
taskforge task request-review --id TASK-NNNN --workspace main --actor <your-name> --json
```

### Reviewing a Task

```bash
# Approve
taskforge task approve --id TASK-NNNN --workspace main --actor <your-name> --json

# Reject (sends back to in_progress with reason as comment)
taskforge task reject --id TASK-NNNN --reason "Missing error handling for edge case X." \
  --workspace main --actor <your-name> --json
```

### Creating Tasks

```bash
taskforge task create \
  --title "Descriptive title" \
  --owner <assignee> \
  --workspace main \
  --actor <your-name> \
  --description "What needs to be done and why" \
  [--reviewer <reviewer-name>] \
  [--review-required true] \
  [--parent TASK-NNNN] \
  --json
```

### Handling Version Conflicts

If you get `CONFLICT_VERSION_MISMATCH`:
1. Re-read the task with `task show`
2. Check what changed since your last read
3. Decide if your mutation is still appropriate
4. Retry with the current version

## Key Principles

- **Don't initialize**: Never run `taskforge init` or `taskforge owner add` yourself. If commands fail with `WORKSPACE_NOT_FOUND` or `OWNER_NOT_FOUND`, tell the user to run the setup steps from the TaskForge README
- **Read before write**: Always check current state before mutating
- **Log everything**: Use `add-worklog` after meaningful progress
- **Respect workflow**: Follow the status transition rules; don't skip steps
- **One thing at a time**: Start a task, do the work, complete/request-review
- **Don't churn**: Minimize status and owner changes; hooks fire on every change

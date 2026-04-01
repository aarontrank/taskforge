# TaskForge CLI Reference

TaskForge is a local, file-backed task management system for one human and multiple AI agents. All data lives in `.taskforge/` as plain markdown files with YAML frontmatter.

## Core Concepts

- Tasks are markdown files with YAML frontmatter in `.taskforge/workspaces/<name>/tasks/TASK-NNNN/task.md`
- Owners are named entities (human or agent) registered in `.taskforge/owners.json`
- Workspaces partition tasks (default: `main`)
- Every mutation is recorded in an append-only audit log per task
- Optimistic concurrency via `version` field and `--expected-version` flag

## Global Flags

| Flag | Description |
|------|-------------|
| `--json` | JSON envelope output (always use from agents) |
| `--root <path>` | Override `.taskforge` root directory |
| `--actor <name>` | Who is performing the action (required for mutations) |
| `--expected-version <n>` | Optimistic concurrency check |

## JSON Response Envelope

```json
{
  "ok": true,
  "command": "task.create",
  "taskforge_version": "0.1.0",
  "data": {},
  "warnings": [],
  "errors": []
}
```

On error, `ok` is `false` and `errors` contains `{ code, message }` objects.

## Commands

### Init

```bash
taskforge init [--workspace <name>] [--root <path>]
```

### Workspaces

```bash
taskforge workspace create --name <name> [--description <text>]
taskforge workspace list [--json]
taskforge workspace show --name <name> [--json]
```

### Owners

```bash
taskforge owner add --name <name> --type <human|agent> [--description <text>]
taskforge owner list [--json]
taskforge owner show --name <name> [--json]
taskforge owner deactivate --name <name>
```

### Task Create

```bash
taskforge task create \
  --title "Title" \
  --owner <name> \
  --workspace <name> \
  --actor <name> \
  [--description "..."] \
  [--reviewer <name>] \
  [--review-required true] \
  [--due-at 2026-04-05T20:00:00Z] \
  [--parent TASK-NNNN] \
  [--json]
```

Use `--parent` for subtasks (one level of nesting only).

### Task Read

```bash
taskforge task show --id TASK-NNNN --workspace <name> [--json]
taskforge task list [--status <status>] [--owner <name>] [--workspace <name>] [--json]
taskforge task search --text "query" [--workspace <name>] [--json]
taskforge task tree --id TASK-NNNN --workspace <name> [--json]
```

List filters: `--status`, `--owner`, `--reviewer`, `--review-required`, `--blocked`, `--parent-task-id`, `--due-before`, `--due-after`, `--created-before`, `--created-after`, `--updated-before`, `--updated-after`, `--archived`, `--soft-deleted`, `--recurring`, `--text`

### Task Update (patch-style)

```bash
taskforge task set-title --id TASK-NNNN --title "New title" --actor <name> --workspace <name>
taskforge task set-description --id TASK-NNNN --description "..." --actor <name> --workspace <name>
taskforge task set-description --id TASK-NNNN --description-file /tmp/desc.md --actor <name> --workspace <name>
taskforge task assign --id TASK-NNNN --owner <name> --actor <name> --workspace <name>
taskforge task set-reviewer --id TASK-NNNN --reviewer <name> --actor <name> --workspace <name>
taskforge task set-due --id TASK-NNNN --due-at <ISO8601> --actor <name> --workspace <name>
taskforge task set-review-required --id TASK-NNNN --value true --actor <name> --workspace <name>
taskforge task add-blocker --id TASK-NNNN --blocked-by TASK-MMMM --actor <name> --workspace <name>
taskforge task remove-blocker --id TASK-NNNN --blocked-by TASK-MMMM --actor <name> --workspace <name>
taskforge task add-attachment --id TASK-NNNN --path /tmp/file --mode <copy|link> --actor <name> --workspace <name>
taskforge task add-artifact --id TASK-NNNN --path ./output.json --mode <copy|link> --actor <name> --workspace <name>
```

### Comments and Work Logs

```bash
taskforge task add-comment --id TASK-NNNN --text "Discussion text" --actor <name> --workspace <name>
taskforge task add-worklog --id TASK-NNNN --text "What was done" --actor <name> --workspace <name>
```

Comments are for discussion. Work logs are for execution records.

### Workflow Transitions

Status flow without review: `open → in_progress → done → archived`
Status flow with review: `open → in_progress → in_review → done → archived` (rejection goes back to `in_progress`)

```bash
taskforge task start --id TASK-NNNN --workspace <name> --actor <name> --json
taskforge task request-review --id TASK-NNNN --workspace <name> --actor <name> --json
taskforge task approve --id TASK-NNNN --workspace <name> --actor <name> --json
taskforge task reject --id TASK-NNNN --reason "..." --workspace <name> --actor <name> --json
taskforge task complete --id TASK-NNNN --workspace <name> --actor <name> --json
taskforge task archive --id TASK-NNNN --workspace <name> --actor <name> --json
taskforge task soft-delete --id TASK-NNNN --workspace <name> --actor <name> --json
```

### Recurrence

```bash
taskforge task set-recurrence --id TASK-NNNN --frequency <hourly|daily|weekly|monthly> --interval <n> --workspace <name> --actor <name> --json
taskforge task clear-recurrence --id TASK-NNNN --workspace <name> --actor <name>
taskforge task generate-next --id TASK-NNNN --workspace <name> --actor <name> --json
```

### Audit

```bash
taskforge task audit --id TASK-NNNN --workspace <name> --json
```

### Hooks

```bash
taskforge hook list [--json]
taskforge hook validate [--json]
taskforge hook test --event task.created --task-id TASK-NNNN [--workspace <name>] [--json]
```

## Error Codes

| Code | Meaning |
|------|---------|
| `TASK_NOT_FOUND` | Task ID does not exist |
| `WORKSPACE_NOT_FOUND` | Workspace does not exist |
| `OWNER_NOT_FOUND` | Owner not in registry |
| `INVALID_STATUS_TRANSITION` | Transition not allowed from current status |
| `BLOCKERS_INCOMPLETE` | Task cannot start; a blocker is not done |
| `REVIEW_REQUIRED` | Task requires review before completion |
| `NOT_IN_REVIEW` | Approve/reject called on a task not in review |
| `INVALID_PARENT` | Parent task not found or is itself a subtask |
| `SUBTASK_DEPTH_EXCEEDED` | Only one level of subtask nesting allowed |
| `CONFLICT_VERSION_MISMATCH` | Optimistic concurrency check failed |
| `VALIDATION_ERROR` | Input failed schema validation |
| `ALREADY_ARCHIVED` | Task is already archived |
| `SOFT_DELETED` | Task has been soft-deleted |
| `RECURRENCE_INVALID` | Recurrence not configured or invalid |

## Agent Rules

1. Always use `--json` for reads
2. Read before mutating — check status, version, blockers, review requirements
3. Use patch commands — never edit `task.md` directly
4. Respect the workflow — don't complete review-required tasks directly; use `request-review`
5. Log your work — use `add-worklog` to record progress
6. Handle version conflicts — on `CONFLICT_VERSION_MISMATCH`, re-read and retry
7. Avoid churn — don't flip status or owner repeatedly; hooks fire on every change

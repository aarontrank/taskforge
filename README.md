# TaskForge

A local, file-backed task management system for one human and multiple AI agents. Think of it as a lightweight Jira that lives entirely on your machine — inspectable markdown files, a CLI for agents and scripts, and a web UI for humans.

---

## Features

- **Local-first** — all data is stored in plain files on your machine
- **Human-readable** — tasks are markdown files with YAML frontmatter
- **Agent-friendly** — deterministic CLI with JSON output for AI agents
- **Patch-based writes** — agents update tasks through specific commands, not file rewrites
- **Review workflow** — tasks can require human or agent review before completion
- **Blockers & subtasks** — dependency enforcement built into the workflow
- **Recurrence** — recurring tasks auto-generate the next occurrence on completion, with a linked history
- **Hooks** — fire local scripts on task lifecycle events
- **Optimistic concurrency** — version-based conflict detection
- **Audit log** — append-only record of every mutation

---

## Installation

### Prerequisites

- Node.js 20+
- npm 10+

### Clone and install

```bash
git clone <repo-url>
cd taskforge
npm install
```

### Build and install the CLI globally

```bash
npm run install:global
```

This builds both the core library and the CLI, then installs the `taskforge` binary globally so it's available from any directory.

To uninstall:

```bash
npm run uninstall:global
```

---

## Quick Start

Initialize the TaskForge repository:

```bash
taskforge init
```

This creates `~/.taskforge/` with the default `main` workspace. All data is stored in this single location regardless of where you run commands.

Add yourself as an owner:

```bash
taskforge owner add --name aaron --type human
```

Create a task:

```bash
taskforge task create \
  --title "Write project proposal" \
  --owner aaron \
  --workspace main \
  --actor aaron
```

Start working on it:

```bash
taskforge task start --id TASK-0001 --workspace main --actor aaron
```

Complete it:

```bash
taskforge task complete --id TASK-0001 --workspace main --actor aaron
```

---

## Web UI

Start the development server:

```bash
cd /path/to/taskforge
npm run dev:web
```

Then open **http://localhost:3847**

The UI reads from `~/.taskforge/` by default — the same location the CLI uses. To override:

```bash
TASKFORGE_ROOT=/path/to/custom/.taskforge npm run dev:web
```

For production:

```bash
npm run build --workspace=packages/web
npm start --workspace=packages/web
```

---

## Data Storage

All data lives in `~/.taskforge/`, a single repository shared by the CLI and web UI:

```
~/.taskforge/
  config.json          # global config and hook definitions
  owners.json          # owner registry
  task_counter.json    # global ID sequence
  hooks.log            # hook execution log
  workspaces/
    main/
      workspace.json
      tasks/
        TASK-0001/
          task.md        # primary task file (YAML frontmatter + markdown body)
          worklog.md     # append-only execution log
          comments.md    # append-only discussion
          audit.log      # append-only mutation history (newline-delimited JSON)
          attachments/
          artifacts/
          subtasks/
            TASK-0002/
              task.md
              ...
```

Every task is a folder. Files are plain text — you can read, edit, and back them up with any standard tool.

---

## CLI Reference

### Global flags

All commands accept:

| Flag | Description |
|------|-------------|
| `--json` | Output a JSON envelope instead of human-readable text |
| `--root <path>` | Override the `~/.taskforge` root directory |

Mutation commands also accept:
- `--actor <name>` — who is performing the action (required)
- `--expected-version <n>` — for optimistic concurrency checks

### JSON response envelope

Every `--json` response uses this shape:

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

---

### Init

```bash
taskforge init [--workspace <name>] [--root <path>]
```

Initializes `~/.taskforge/` and creates the default workspace. Use `--root` to override the location.

---

### Workspaces

```bash
taskforge workspace create --name <name> [--description <text>]
taskforge workspace list [--json]
taskforge workspace show --name <name> [--json]
```

---

### Owners

Owners are any named entity — humans or AI agents — that can be assigned to tasks.

```bash
taskforge owner add --name <name> --type <human|agent> [--description <text>]
taskforge owner list [--json]
taskforge owner show --name <name> [--json]
taskforge owner deactivate --name <name>
```

---

### Tasks

#### Create

```bash
taskforge task create \
  --title "Build initial CLI" \
  --owner builder-agent \
  --workspace main \
  --actor planner-agent \
  [--description "..."] \
  [--reviewer review-agent] \
  [--review-required true] \
  [--due-at 2026-04-05T20:00:00Z] \
  [--parent TASK-0001] \
  [--json]
```

Use `--parent` to create a subtask (one level of nesting only).

#### Read

```bash
taskforge task show --id TASK-0001 --workspace main [--json]
taskforge task list [--status <status>] [--owner <name>] [--workspace <name>] [--json]
taskforge task search --text "CLI schema" [--workspace main] [--json]
taskforge task tree --id TASK-0001 --workspace main [--json]
```

`task list` filter flags: `--status`, `--owner`, `--reviewer`, `--review-required`, `--blocked`, `--parent-task-id`, `--due-before`, `--due-after`, `--created-before`, `--created-after`, `--updated-before`, `--updated-after`, `--archived`, `--soft-deleted`, `--recurring`, `--text`

#### Update (patch-style)

```bash
taskforge task set-title --id TASK-0001 --title "New title" --actor aaron --workspace main
taskforge task set-description --id TASK-0001 --description "..." --actor aaron --workspace main
taskforge task set-description --id TASK-0001 --description-file /tmp/desc.md --actor aaron --workspace main
taskforge task assign --id TASK-0001 --owner review-agent --actor planner-agent --workspace main
taskforge task set-reviewer --id TASK-0001 --reviewer review-agent --actor planner-agent --workspace main
taskforge task set-due --id TASK-0001 --due-at 2026-04-07T20:00:00Z --actor planner-agent --workspace main
taskforge task set-review-required --id TASK-0001 --value true --actor planner-agent --workspace main
taskforge task add-blocker --id TASK-0001 --blocked-by TASK-0007 --actor planner-agent --workspace main
taskforge task remove-blocker --id TASK-0001 --blocked-by TASK-0007 --actor planner-agent --workspace main
taskforge task add-attachment --id TASK-0001 --path /tmp/spec.pdf --mode copy --actor aaron --workspace main
taskforge task add-artifact --id TASK-0001 --path ./dist/output.json --mode link --actor builder-agent --workspace main
```

`--mode copy` copies the file into the task folder. `--mode link` stores the path reference only.

#### Comments and work logs

```bash
taskforge task add-comment --id TASK-0001 --text "Please clarify the output format." --actor aaron --workspace main
taskforge task add-worklog --id TASK-0001 --text "Implemented parser and validation layer." --actor builder-agent --workspace main
```

Comments are for discussion. Work logs are for execution records.

---

### Workflow

#### Status transitions

For tasks without review:

```
open → in_progress → done → archived
```

For tasks with `review_required: true`:

```
open → in_progress → in_review → done → archived
                  ↑____________|  (on rejection)
```

#### Commands

```bash
# Start a task (open → in_progress). Fails if any blocker is not done.
taskforge task start --id TASK-0001 --workspace main --actor builder-agent --json

# Request review (in_progress → in_review). Only valid when review_required = true.
taskforge task request-review --id TASK-0001 --workspace main --actor builder-agent --json

# Approve review (in_review → done).
taskforge task approve --id TASK-0001 --workspace main --actor review-agent --json

# Reject review (in_review → in_progress). Appends rejection reason as a comment.
taskforge task reject --id TASK-0001 --reason "Missing error schema." --workspace main --actor review-agent --json

# Complete a task without review (in_progress → done). Fails if review_required = true.
taskforge task complete --id TASK-0001 --workspace main --actor builder-agent --json

# Archive a task.
taskforge task archive --id TASK-0001 --workspace main --actor aaron --json

# Soft-delete (hides from normal listings, data is not removed).
taskforge task soft-delete --id TASK-0001 --workspace main --actor aaron --json
```

---

### Recurrence

```bash
# Set recurrence on a task
taskforge task set-recurrence \
  --id TASK-0010 \
  --frequency weekly \
  --interval 1 \
  --workspace main \
  --actor planner-agent \
  --json

# Clear recurrence
taskforge task clear-recurrence --id TASK-0010 --workspace main --actor planner-agent

# Manually generate the next occurrence (normally happens automatically on completion)
taskforge task generate-next --id TASK-0010 --workspace main --actor planner-agent --json
```

Supported frequencies: `hourly`, `daily`, `weekly`, `monthly`

When a recurring task is completed, a new task is automatically created with:
- a fresh ID and timestamps
- `prior_occurrence_id` pointing to the completed task, forming a linked chain

---

### Audit log

```bash
taskforge task audit --id TASK-0001 --workspace main --json
```

Returns the append-only audit history for the task as a JSON array.

---

### Optimistic concurrency

Every task has a `version` field. Read commands return it. Mutation commands accept `--expected-version`:

```bash
taskforge task set-title \
  --id TASK-0001 \
  --title "Updated title" \
  --actor aaron \
  --expected-version 3 \
  --workspace main
```

If the current version does not match, the command fails with `CONFLICT_VERSION_MISMATCH`. Re-read the task, inspect the changes, and retry if appropriate.

---

### Hooks

Hooks fire local commands in response to task lifecycle events. They are configured in `~/.taskforge/config.json` and executed after the mutation is committed — hook failures do not roll back the task change.

#### Supported events

| Event | Fires when |
|-------|-----------|
| `task.created` | A top-level task is created |
| `subtask.created` | A task with a parent is created (also fires `task.created`) |
| `task.owner_changed` | The `owner` field changes |
| `task.status_changed` | The `status` field changes |

#### Configuration

Edit `~/.taskforge/config.json`:

```json
{
  "default_workspace": "main",
  "hooks": [
    {
      "id": "notify-on-create",
      "enabled": true,
      "event": "task.created",
      "command": "node ./scripts/on-task-created.js",
      "args": [],
      "workspace_filter": ["main"],
      "timeout_ms": 10000
    },
    {
      "id": "notify-on-status-change",
      "enabled": true,
      "event": "task.status_changed",
      "command": "node ./scripts/on-status-change.js",
      "args": []
    }
  ]
}
```

The hook process receives the event payload as JSON on **stdin**. Example payload for `task.status_changed`:

```json
{
  "event": "task.status_changed",
  "timestamp": "2026-03-31T20:18:00Z",
  "taskforge_version": "0.1.0",
  "workspace": "main",
  "actor": "builder-agent",
  "task": {
    "id": "TASK-0021",
    "title": "Implement recurrence handling",
    "owner": "builder-agent",
    "review_required": true,
    "reviewer": "review-agent",
    "version": 5
  },
  "change": {
    "field": "status",
    "old": "in_progress",
    "new": "in_review"
  }
}
```

#### Hook commands

```bash
taskforge hook list [--json]
taskforge hook validate [--json]
taskforge hook test --event task.created --task-id TASK-0001 [--workspace main] [--json]
```

Hook execution is logged to `.taskforge/hooks.log`.

---

## Agent Integration

AI agents interact with TaskForge exclusively through CLI commands. Key rules:

1. **Always use `--json`** for reads — structured output is more reliable than parsing human-readable text
2. **Read before mutating** — check current status, version, blockers, and review requirements
3. **Use patch commands** — never edit `task.md` directly; use `set-title`, `assign`, `add-blocker`, etc.
4. **Respect the workflow** — do not complete a review-required task directly; call `request-review` instead
5. **Log your work** — use `add-worklog` to record what was done, what remains, and any blockers
6. **Handle version conflicts** — on `CONFLICT_VERSION_MISMATCH`, re-read and retry only if still appropriate
7. **Avoid churn** — don't flip status or owner repeatedly; hooks fire on every change

### Typical agent loop

```bash
# 1. Find tasks assigned to you
taskforge task list --status open --owner builder-agent --workspace main --json

# 2. Inspect before acting
taskforge task show --id TASK-0001 --workspace main --json

# 3. Start work (validates blockers)
taskforge task start --id TASK-0001 --workspace main --actor builder-agent --json

# 4. Log progress
taskforge task add-worklog \
  --id TASK-0001 \
  --text "Implemented validation layer. Next: error codes." \
  --actor builder-agent \
  --workspace main

# 5a. Complete (no review required)
taskforge task complete --id TASK-0001 --workspace main --actor builder-agent --json

# 5b. Or request review (review_required = true)
taskforge task request-review --id TASK-0001 --workspace main --actor builder-agent --json
```

---

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

---

## Project Structure

```
taskforge/
  packages/
    core/          # Domain library — models, storage, services, hook engine
    cli/           # taskforge CLI binary (Commander.js, bundled with esbuild)
    web/           # Next.js 14 web UI (port 3847)
  package.json     # npm workspace root
  .gitignore
```

### Development

```bash
# Build everything
npm run build

# Watch CLI for changes
npm run dev:cli

# Run web UI in dev mode
npm run dev:web

# Rebuild after core changes
npm run build --workspace=packages/core
npm run build --workspace=packages/cli

# Reinstall global CLI after changes
npm run install:global
```

---

## Task File Format

`task.md` uses YAML frontmatter followed by a markdown body:

```markdown
---
id: TASK-0001
title: Build initial CLI for task creation
status: in_progress
workspace: main
owner: builder-agent
review_required: true
reviewer: review-agent
due_at: 2026-04-05T20:00:00Z
created_at: 2026-03-31T18:00:00Z
updated_at: 2026-03-31T19:15:00Z
completed_at:
review_requested_at:
reviewed_at:
review_outcome:
parent_task_id:
prior_occurrence_id:
blocked_by:
  - TASK-0007
recurrence:
soft_deleted: false
archived: false
version: 4
attachment_refs: []
artifact_refs: []
---

## Summary

Build the initial CLI layer.

## Acceptance Criteria

- [ ] `task create` command works
- [ ] `task list` supports filters
- [ ] JSON output is supported

## Notes

This task requires review before completion.
```

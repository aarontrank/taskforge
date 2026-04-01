# TaskForge AI Agent Skills

Skills and agent configurations that teach AI coding assistants how to use the TaskForge CLI.

## Structure

```
kiro/
├── docs/                          # Shared source content (tool-agnostic)
│   ├── taskforge-cli-reference.md # Complete CLI reference
│   └── taskforge-agent-guide.md   # Agent behavior guide
├── kiro/
│   ├── install.sh                 # Kiro CLI installer
│   └── uninstall.sh               # Kiro CLI uninstaller
├── claude-code/
│   ├── install.sh                 # Claude Code installer
│   └── uninstall.sh               # Claude Code uninstaller
└── README.md
```

The `docs/` directory contains the canonical content. Each tool-specific installer wraps it with the appropriate metadata and copies it to the right location.

## Prerequisites

Before an agent can use TaskForge, you must initialize a TaskForge repository and register owners. The skill installers do **not** do this — you choose where the repo lives and who the owners are.

```bash
cd ~/my-project                                    # or wherever you want .taskforge/ to live
taskforge init                                     # creates .taskforge/ with default "main" workspace
taskforge owner add --name <you> --type human      # register yourself
taskforge owner add --name kiro --type agent       # register the AI agent
```

Without this, every `taskforge` command the agent runs will fail with `WORKSPACE_NOT_FOUND` or `OWNER_NOT_FOUND` errors. The `--owner` and `--actor` flags are validated against the owner registry.

## Installation

### Kiro CLI

```bash
cd /path/to/taskforge/kiro
./kiro/install.sh
```

This:
- Installs skills to `~/.kiro/agents/taskforge/skills/`
- Creates a dedicated `TaskForgeAgent` for task-focused sessions
- Adds TaskForge skills to your default agent so they're available in every session

Usage:
```bash
kiro-cli chat                     # default agent — skills load on demand when relevant
kiro-cli chat --agent taskforge   # dedicated TaskForge agent
```

### Claude Code

Global (all projects):
```bash
cd /path/to/taskforge/kiro
./claude-code/install.sh
```

Project-specific:
```bash
./claude-code/install.sh --project /path/to/your/project
```

Installs a skill to `~/.claude/skills/taskforge/` (or `.claude/skills/taskforge/` for project installs). Claude Code auto-invokes the skill when task management is relevant.

## Uninstalling

### Kiro CLI
```bash
./kiro/uninstall.sh
```

Removes skills, agent configs, and reverts any default agent changes.

### Claude Code
```bash
./claude-code/uninstall.sh                        # global
./claude-code/uninstall.sh --project /path/to/dir  # project
```

## Updating

Re-run the install script after pulling changes — it overwrites previous installs:

```bash
git pull
./kiro/install.sh        # for Kiro
./claude-code/install.sh  # for Claude Code
```

## Adding Support for Other Tools

To add support for a new AI coding assistant:

1. Create a new directory (e.g., `codex/`)
2. Write an `install.sh` that reads from `docs/` and outputs in the tool's expected format
3. Write an `uninstall.sh` that cleanly removes everything
4. The shared docs in `docs/` are plain markdown — adapt the metadata wrapper as needed

#!/bin/bash
set -e

# TaskForge Claude Code Skill Installation Script
# Installs skills and supporting docs to ~/.claude/
#
# Usage:
#   ./install.sh                    # Install to global ~/.claude/
#   ./install.sh --project <path>   # Install to project .claude/ directory

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DOCS_DIR="$SCRIPT_DIR/../docs"

# Parse flags
TARGET_BASE=~/.claude
IS_PROJECT=false
for arg in "$@"; do
    case "$arg" in
        --project)
            IS_PROJECT=true
            ;;
        *)
            if [ "$IS_PROJECT" = true ] && [ -z "$PROJECT_PATH" ]; then
                PROJECT_PATH="$arg"
            else
                echo "Unknown flag: $arg"
                echo "Usage: $0 [--project <path>]"
                exit 1
            fi
            ;;
    esac
done

if [ "$IS_PROJECT" = true ]; then
    if [ -z "$PROJECT_PATH" ]; then
        PROJECT_PATH="."
    fi
    TARGET_BASE="$PROJECT_PATH/.claude"
    echo "Installing TaskForge Claude Code skill to project: $TARGET_BASE"
else
    echo "Installing TaskForge Claude Code skill globally: $TARGET_BASE"
fi

SKILLS_DIR="$TARGET_BASE/skills/taskforge"

# Create target directories
mkdir -p "$SKILLS_DIR"

# Build SKILL.md: frontmatter + reference to supporting docs
echo "Building skill..."
cat > "$SKILLS_DIR/SKILL.md" <<'FRONTMATTER'
---
name: taskforge
description: Manage tasks using the TaskForge CLI. Use when the user asks to create, update, list, search, or manage tasks and workflows.
allowed-tools: Bash
---

You are a TaskForge agent. You manage tasks using the `taskforge` CLI tool.

@taskforge-cli-reference.md
@taskforge-agent-guide.md

## Key Rules

- Always use `--json` flag for reads to get structured output
- Always include `--actor` on mutation commands
- Read a task before mutating it to check status, version, and blockers
- Use patch commands (`set-title`, `assign`, etc.) — never edit task.md files directly
- Log progress with `add-worklog` after meaningful work
- Respect the workflow state machine: don't skip steps or complete review-required tasks directly
FRONTMATTER
echo "  ✓ SKILL.md"

# Copy shared docs as supporting files (Claude Code @ references resolve relative to SKILL.md)
cp "$DOCS_DIR/taskforge-cli-reference.md" "$SKILLS_DIR/taskforge-cli-reference.md"
cp "$DOCS_DIR/taskforge-agent-guide.md" "$SKILLS_DIR/taskforge-agent-guide.md"
echo "  ✓ taskforge-cli-reference.md"
echo "  ✓ taskforge-agent-guide.md"

echo ""
echo "✅ Installation complete!"
echo ""
echo "Installed to: $SKILLS_DIR"
echo ""
if [ "$IS_PROJECT" = true ]; then
    echo "The skill is now available in this project."
    echo "Claude Code will auto-invoke it when task management is relevant."
else
    echo "The skill is now available globally across all projects."
    echo "Claude Code will auto-invoke it when task management is relevant."
fi
echo ""
echo "To uninstall:"
echo "  rm -rf $SKILLS_DIR"

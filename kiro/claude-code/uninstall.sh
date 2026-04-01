#!/bin/bash
set -e

# TaskForge Claude Code Skill Uninstallation Script
# Removes skills from ~/.claude/ or project .claude/ directory.
#
# Usage:
#   ./uninstall.sh                    # Uninstall from global ~/.claude/
#   ./uninstall.sh --project <path>   # Uninstall from project .claude/ directory

# Parse flags
TARGET_BASE=~/.claude
for arg in "$@"; do
    case "$arg" in
        --project)
            IS_PROJECT=true
            ;;
        *)
            if [ "$IS_PROJECT" = true ] && [ -z "$PROJECT_PATH" ]; then
                PROJECT_PATH="$arg"
            fi
            ;;
    esac
done

if [ "$IS_PROJECT" = true ]; then
    TARGET_BASE="${PROJECT_PATH:-.}/.claude"
fi

SKILLS_DIR="$TARGET_BASE/skills/taskforge"

echo "Uninstalling TaskForge Claude Code skill from: $TARGET_BASE"

if [ -d "$SKILLS_DIR" ]; then
    rm -rf "$SKILLS_DIR"
    echo "  ✓ Removed $SKILLS_DIR"
else
    echo "  ⚠ Not found: $SKILLS_DIR"
fi

echo ""
echo "✅ Uninstallation complete!"

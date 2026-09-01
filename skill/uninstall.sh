#!/bin/bash
set -e

# Remove the taskforge Claude Code skill. Leaves the task data in ~/.taskforge alone.

TARGET_BASE="$HOME/.claude"
if [ "${1:-}" = "--project" ]; then
    [ -n "${2:-}" ] || { echo "--project needs a path" >&2; exit 1; }
    TARGET_BASE="$2/.claude"
fi

DEST="$TARGET_BASE/skills/taskforge"
if [ -d "$DEST" ]; then
    rm -rf "$DEST"
    echo "Removed $DEST"
else
    echo "Nothing to remove at $DEST"
fi
echo "Task data in ${TASKFORGE_ROOT:-$HOME/.taskforge} was left untouched."

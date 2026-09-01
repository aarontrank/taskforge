#!/bin/bash
set -e

# Install the taskforge Claude Code skill.
#
#   ./install.sh                    # ~/.claude/skills/taskforge
#   ./install.sh --project <path>   # <path>/.claude/skills/taskforge
#
# Kept as shell deliberately: this runs on a machine that may have no cargo build yet, which
# is exactly the bootstrap case a compiled installer cannot serve.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TARGET_BASE="$HOME/.claude"

if [ "${1:-}" = "--project" ]; then
    [ -n "${2:-}" ] || { echo "--project needs a path" >&2; exit 1; }
    TARGET_BASE="$2/.claude"
elif [ -n "${1:-}" ]; then
    echo "Usage: $0 [--project <path>]" >&2
    exit 1
fi

DEST="$TARGET_BASE/skills/taskforge"
mkdir -p "$DEST"

# Copy the skill verbatim. The supporting docs are referenced from SKILL.md by relative
# markdown link, which is how Claude Code loads them on demand -- an `@` import is a
# CLAUDE.md mechanism and is NOT expanded inside a SKILL.md, so using one here would leave
# the reference material unreachable.
for f in SKILL.md reference.md workflows.md; do
    cp "$SCRIPT_DIR/taskforge/$f" "$DEST/$f"
    echo "  ✓ $f"
done

echo
echo "Installed to: $DEST"
command -v taskforge >/dev/null 2>&1 \
    || echo "NOTE: the taskforge binary is not on PATH. Run: cargo install --path crates/cli"
[ -d "${TASKFORGE_ROOT:-$HOME/.taskforge}" ] \
    || echo "NOTE: no repository yet. Run: taskforge init --json"
echo "Run /reload-plugins in a live session, or start a new one, to pick it up."

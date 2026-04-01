#!/bin/bash
set -e

# TaskForge Kiro Skill Uninstallation Script
# Removes agent config, skills, and reverts default agent changes.
#
# Usage:
#   ./uninstall.sh

NAMESPACE="taskforge"
AGENTS_DIR=~/.kiro/agents
TARGET_DIR="$AGENTS_DIR/$NAMESPACE"
SKILL_URI="skill://$TARGET_DIR/skills/**/SKILL.md"
SETTINGS_FILE=~/.kiro/settings/cli.json
WRAPPER_AGENT="kiro-default-taskforge"

echo "Uninstalling TaskForge Kiro skill..."

# Determine current default agent
DEFAULT_AGENT=""
if [ -f "$SETTINGS_FILE" ]; then
    DEFAULT_AGENT=$(python3 -c "import json; d=json.load(open('$SETTINGS_FILE')); print(d.get('chat.defaultAgent',''))" 2>/dev/null || echo "")
fi

# Revert default agent changes
if [ "$DEFAULT_AGENT" = "$WRAPPER_AGENT" ]; then
    # We created this wrapper — remove it and unset the default
    rm -f "$AGENTS_DIR/$WRAPPER_AGENT.json"
    python3 -c "
import json
with open('$SETTINGS_FILE') as f:
    cfg = json.load(f)
cfg.pop('chat.defaultAgent', None)
with open('$SETTINGS_FILE', 'w') as f:
    json.dump(cfg, f, indent=2)
    f.write('\n')
"
    echo "  ✓ Removed $WRAPPER_AGENT agent and reverted default"
elif [ -n "$DEFAULT_AGENT" ] && [ "$DEFAULT_AGENT" != "kiro_default" ]; then
    # Custom default agent — remove skill URI from its resources
    AGENT_FILE="$AGENTS_DIR/$DEFAULT_AGENT.json"
    if [ -f "$AGENT_FILE" ] && grep -q "$SKILL_URI" "$AGENT_FILE" 2>/dev/null; then
        python3 -c "
import json
with open('$AGENT_FILE') as f:
    cfg = json.load(f)
res = cfg.get('resources', [])
res = [r for r in res if '$SKILL_URI' not in r]
cfg['resources'] = res
with open('$AGENT_FILE', 'w') as f:
    json.dump(cfg, f, indent=2)
    f.write('\n')
"
        echo "  ✓ Removed skill from default agent: $DEFAULT_AGENT"
    fi
fi

# Remove dedicated TaskForge agent
rm -f "$AGENTS_DIR/TaskForgeAgent.json"
echo "  ✓ Removed TaskForgeAgent.json"

# Remove skills and namespace directory
rm -rf "$TARGET_DIR"
echo "  ✓ Removed $TARGET_DIR"

echo ""
echo "✅ Uninstallation complete!"

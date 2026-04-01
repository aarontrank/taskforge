#!/bin/bash
set -e

# TaskForge Kiro Skill Installation Script
# Installs agent config and skills to ~/.kiro/, and adds skills to the default agent.
#
# Usage:
#   ./install.sh

NAMESPACE="taskforge"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DOCS_DIR="$SCRIPT_DIR/../docs"
AGENTS_DIR=~/.kiro/agents
TARGET_DIR="$AGENTS_DIR/$NAMESPACE"
SKILL_GLOB="$TARGET_DIR/skills/**/SKILL.md"
SKILL_URI="skill://$SKILL_GLOB"
SETTINGS_FILE=~/.kiro/settings/cli.json
MARKER="# taskforge-skill-injected"

echo "Installing TaskForge Kiro skill (namespace: $NAMESPACE)"

# Create target directories
mkdir -p "$TARGET_DIR/skills/taskforge"
mkdir -p "$TARGET_DIR/skills/taskforge-agent"
mkdir -p "$(dirname "$SETTINGS_FILE")"

# Build skill files: frontmatter + shared doc content
echo ""
echo "Building skills from shared docs..."

{
    echo "---"
    echo "name: taskforge-cli"
    echo "description: TaskForge CLI reference — task management commands, workflow transitions, error codes, and agent integration rules. Use when creating, updating, querying, or managing tasks with the taskforge CLI."
    echo "---"
    echo ""
    cat "$DOCS_DIR/taskforge-cli-reference.md"
} > "$TARGET_DIR/skills/taskforge/SKILL.md"
echo "  ✓ taskforge-cli skill"

{
    echo "---"
    echo "name: taskforge-agent"
    echo "description: How to behave as a TaskForge agent — workflow patterns, best practices, and common operations. Use when acting on tasks or planning task management work."
    echo "---"
    echo ""
    cat "$DOCS_DIR/taskforge-agent-guide.md"
} > "$TARGET_DIR/skills/taskforge-agent/SKILL.md"
echo "  ✓ taskforge-agent skill"

# Generate dedicated TaskForge agent config
echo ""
echo "Generating TaskForge agent config..."
cat > "$AGENTS_DIR/TaskForgeAgent.json" <<EOF
{
  "\$schema": "https://raw.githubusercontent.com/aws/amazon-q-developer-cli/refs/heads/main/schemas/agent-v1.json",
  "name": "$NAMESPACE",
  "description": "Manage tasks using the TaskForge CLI",
  "prompt": "You are a TaskForge agent. You manage tasks using the taskforge CLI tool. Always use --json for reads, always include --actor on mutations, read before mutating, use patch commands (never edit task.md directly), log progress with add-worklog, and respect the workflow state machine.",
  "resources": [
    "skill://$SKILL_GLOB"
  ],
  "tools": [
    "fs_read",
    "execute_bash"
  ],
  "allowedTools": [
    "fs_read"
  ]
}
EOF
echo "  ✓ TaskForgeAgent.json"

# Add skill to default agent
echo ""
echo "Adding TaskForge skill to default agent..."

# Determine which agent is the default
DEFAULT_AGENT=""
if [ -f "$SETTINGS_FILE" ]; then
    DEFAULT_AGENT=$(python3 -c "import json; d=json.load(open('$SETTINGS_FILE')); print(d.get('chat.defaultAgent',''))" 2>/dev/null || echo "")
fi

if [ -n "$DEFAULT_AGENT" ] && [ "$DEFAULT_AGENT" != "kiro_default" ]; then
    # Custom default agent — patch its resources array
    AGENT_FILE="$AGENTS_DIR/$DEFAULT_AGENT.json"
    if [ -f "$AGENT_FILE" ]; then
        if grep -q "$SKILL_URI" "$AGENT_FILE" 2>/dev/null; then
            echo "  ✓ Skill already present in $DEFAULT_AGENT"
        else
            # Add skill URI to resources array using python3 for safe JSON manipulation
            python3 -c "
import json, sys
with open('$AGENT_FILE') as f:
    cfg = json.load(f)
res = cfg.get('resources', [])
res.append('$SKILL_URI')
cfg['resources'] = res
with open('$AGENT_FILE', 'w') as f:
    json.dump(cfg, f, indent=2)
    f.write('\n')
"
            echo "  ✓ Added skill to existing default agent: $DEFAULT_AGENT"
        fi
    else
        echo "  ⚠ Default agent '$DEFAULT_AGENT' config not found at $AGENT_FILE"
        echo "    Add this to its resources manually: $SKILL_URI"
    fi
else
    # No custom default or using built-in kiro_default — create a wrapper
    WRAPPER_FILE="$AGENTS_DIR/kiro-default-taskforge.json"
    cat > "$WRAPPER_FILE" <<EOF
{
  "\$schema": "https://raw.githubusercontent.com/aws/amazon-q-developer-cli/refs/heads/main/schemas/agent-v1.json",
  "name": "kiro-default-taskforge",
  "description": "Default Kiro agent with TaskForge skill",
  "resources": [
    "file://README.md",
    "file://KIRO.md",
    "file://.kiro/rules/**/*.md",
    "skill://$SKILL_GLOB"
  ],
  "tools": ["*"],
  "allowedTools": ["fs_read"],
  "useLegacyMcpJson": true
}
EOF
    echo "  ✓ Created kiro-default-taskforge agent"

    # Set it as the default
    if [ -f "$SETTINGS_FILE" ]; then
        python3 -c "
import json
with open('$SETTINGS_FILE') as f:
    cfg = json.load(f)
cfg['chat.defaultAgent'] = 'kiro-default-taskforge'
with open('$SETTINGS_FILE', 'w') as f:
    json.dump(cfg, f, indent=2)
    f.write('\n')
"
    else
        echo '{"chat.defaultAgent": "kiro-default-taskforge"}' | python3 -m json.tool > "$SETTINGS_FILE"
    fi
    echo "  ✓ Set kiro-default-taskforge as default agent"
fi

echo ""
echo "✅ Installation complete!"
echo ""
echo "Installed to: $TARGET_DIR"
echo ""
echo "Usage:"
echo "  kiro-cli chat                     # default agent now has TaskForge skills"
echo "  kiro-cli chat --agent taskforge   # dedicated TaskForge agent"
echo ""
echo "To uninstall:"
echo "  $(dirname "$SCRIPT_DIR")/kiro/uninstall.sh"

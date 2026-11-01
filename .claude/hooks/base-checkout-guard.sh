#!/usr/bin/env bash
# PreToolUse hook (Edit, Write, NotebookEdit): refuse a file change inside the base checkout.
# Agents work in .claude/worktrees/<branch> (docs/process/agent-brief.md item 1). Paths in a
# linked worktree and paths outside the repository (memory, scratchpad, /tmp) are allowed.
# Exit 2 blocks the tool call with the stderr message; any other failure lets the call through.
set -uo pipefail

input="$(cat)"
command -v python3 >/dev/null 2>&1 || exit 0

cd "${CLAUDE_PROJECT_DIR:-.}" 2>/dev/null || exit 0
common="$(git rev-parse --path-format=absolute --git-common-dir 2>/dev/null)" || exit 0
base="$(dirname "$common")"

python3 -I -c '
import json, os, sys
base = os.path.realpath(sys.argv[1])
try:
    data = json.loads(sys.argv[2])
except ValueError:
    sys.exit(0)
tool_input = data.get("tool_input") or {}
path = tool_input.get("file_path") or tool_input.get("notebook_path")
if not path:
    sys.exit(0)
path = os.path.realpath(os.path.join(data.get("cwd") or os.getcwd(), path))
worktrees = os.path.join(base, ".claude", "worktrees")
inside = lambda root: path == root or path.startswith(root + os.sep)
if inside(base) and not inside(worktrees):
    print(
        f"BLOCKED: {path} is in the base checkout; work in .claude/worktrees/<branch> "
        "(docs/process/agent-brief.md item 1).",
        file=sys.stderr,
    )
    sys.exit(2)
' "$base" "$input"

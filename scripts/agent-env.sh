#!/usr/bin/env bash
# Configure environment variables and gh CLI settings for agent sessions (Issue #139).
# Prevents terminal escape sequences, background timeouts, and unstructured output.
#
# Usage:
#   source scripts/agent-env.sh
#   or:
#   . scripts/agent-env.sh

export NO_COLOR=1
export GH_FORCE_TTY=0
export GH_NO_UPDATE_NOTIFIER=1

if command -v gh >/dev/null 2>&1; then
    # shellcheck disable=SC2016  # the alias body is for gh, which expands $1, not this shell
    gh alias set run-status 'run view $1 --json status,conclusion,jobs --jq "{
        status: .status, conclusion: .conclusion,
        jobs: [.jobs[] | {name: .name, status: .status, conclusion: .conclusion}]}"' \
        --clobber 2>/dev/null || true
fi

# If executed directly instead of sourced, inform the caller:
if ! (return 0 2>/dev/null); then
    echo "Configured gh alias 'run-status'."
    echo "To export NO_COLOR=1, GH_FORCE_TTY=0, and GH_NO_UPDATE_NOTIFIER=1 into your current" \
        "shell, run:"
    echo "  source scripts/agent-env.sh"
fi

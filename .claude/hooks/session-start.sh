#!/usr/bin/env bash
# SessionStart hook: warn (stdout, which the agent reads) when this checkout is stale.
# Silent when fine or offline; never fails the session.
set -euo pipefail

cd "${CLAUDE_PROJECT_DIR:-.}" 2>/dev/null || exit 0
git rev-parse --git-dir >/dev/null 2>&1 || exit 0

# Fetch with a short timeout; skip silently when offline.
if command -v timeout >/dev/null 2>&1; then
    timeout 8 git fetch --quiet --prune origin >/dev/null 2>&1 || true
else
    git fetch --quiet --prune origin >/dev/null 2>&1 &
    pid=$!
    for _ in 1 2 3 4 5 6 7 8; do
        kill -0 "$pid" 2>/dev/null || break
        sleep 1
    done
    kill "$pid" >/dev/null 2>&1 || true
    wait "$pid" >/dev/null 2>&1 || true
fi

git rev-parse --verify --quiet origin/main >/dev/null 2>&1 || exit 0

branch="$(git symbolic-ref --quiet --short HEAD 2>/dev/null || true)"
behind="$(git rev-list --count HEAD..origin/main 2>/dev/null || echo 0)"

if [ "$behind" -gt 0 ]; then
    echo "WARNING: this checkout is $behind commit(s) behind origin/main. Review against origin/main, or run: git switch main && git pull"
fi

if [ -n "$branch" ] && [ "$branch" != "main" ]; then
    track="$(git for-each-ref --format='%(upstream:track)' "refs/heads/$branch" 2>/dev/null || true)"
    if [ "$track" = "[gone]" ]; then
        echo "WARNING: the upstream of branch $branch is gone (its PR merged?). Start new work from origin/main: git switch -c <branch> origin/main"
    elif [ "$behind" -gt 0 ] && git merge-base --is-ancestor HEAD origin/main 2>/dev/null; then
        echo "WARNING: branch $branch is already merged into origin/main. Start new work from origin/main: git switch -c <branch> origin/main"
    fi
fi
exit 0

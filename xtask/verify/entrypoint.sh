#!/bin/bash
# Entry point of the verification image (docs/06-verification.md §7, VER-056).
#
# usage: aldis-verify <commit> [cargo xtask verify inside options]
#
# Puts the pinned C++ tools on PATH, checks out <commit> from the repository mounted read-only at
# /repo, links the build directories to the cache volume at /cache, and runs the checks.
set -euo pipefail

while read -r dir; do PATH="$dir:$PATH"; done < /opt/aldis-cpp/path
export PATH
set -a
# shellcheck source=/dev/null
. /opt/aldis-cpp/env
set +a

commit="$1"
shift

# Every check sees the same flags, so no check rebuilds another's artefacts (ci.yml `env`).
# The verification run is what the spec calls CI (§7), so CI-only refusals apply: for example
# AldisToolchainCheck.cmake refuses ALDIS_ALLOW_UNPINNED_COMPILER when CI is set.
export CI=true CARGO_HOME=/cache/cargo CARGO_TERM_COLOR=never RUSTFLAGS="-D warnings" \
  RUSTDOCFLAGS="-D warnings" ALDIS_VERIFY_INSIDE=1
mkdir -p "$CARGO_HOME"

mkdir -p /work/src
cd /work/src
git init -q
git fetch -q /repo '+refs/heads/*:refs/remotes/origin/*' '+refs/tags/*:refs/tags/*'
# The commit itself too: a detached HEAD, in this or another worktree, is on no branch or tag.
git fetch -q /repo "$commit"
git -c advice.detachedHead=false checkout -q "$commit"

# Runs on one host share the cache volume, and every run builds C++ at the same paths: hold the
# volume until this run ends (the descriptor stays open across the exec below), so that no run
# deletes or reuses another's build output.
exec 9> /cache/verify.lock
if ! flock -n 9; then
  echo "verify: waiting for another verification run to release the cache volume" >&2
  flock 9
fi

# Rust build output persists between runs in the cache volume; C++ builds start clean.
for dir in target refapp/target exerciser/target fuzz/target; do
  cache="/cache/${dir//\//-}"
  mkdir -p "$cache"
  ln -s "$cache" "$dir"
done
rm -rf /cache/target/cpp

exec cargo xtask verify inside "$@"

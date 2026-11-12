#!/usr/bin/env bash
# Lockfile helpers shared by scripts/train and scripts/reverify (sourced, not run): recognise a
# lockfile, compare the package versions of two lockfiles, and finish a merge whose only
# conflicts are lockfiles. The caller sets `tmproot`, a scratch directory it removes.

is_lockfile() { [[ $1 =~ ^((refapp|exerciser|fuzz|crosscheck)/)?Cargo\.lock$ ]]; }

# lock_pairs <file>: the `name version` of every package of a Cargo.lock, sorted.
# Only the `[[package]]` tables count, not the format `version = N` at the top of the file.
lock_pairs() {
    awk '/^\[\[package\]\]/ {pkg = 1; next} /^\[/ {pkg = 0}
        pkg && /^name = / {n = $3; gsub(/"/, "", n)}
        pkg && /^version = / {v = $3; gsub(/"/, "", v); print n " " v}' "$1" | sort -u
}
# lost_versions <base> <theirs> <result> <bumps only: 0|1>: the `name version` pairs that the
# PR's side (theirs) introduced against the merge base and the result lacks, one line each. With
# bumps only, a pair counts only when the base has the package under another version (a bump,
# not a new dependency, whose version a regeneration picks).
lost_versions() {
    comm -13 <(lock_pairs "$1") <(lock_pairs "$2") | {
        if [ "$4" = 1 ]; then
            awk 'NR == FNR { seen[$1] = 1; next } ($1 in seen)' <(lock_pairs "$1") -
        else
            cat
        fi
    } | comm -23 - <(lock_pairs "$3")
}
blob() { # blob <git dir> <spec> <out>: the file at <spec> (rev:path or :stage:path), else empty
    git -C "$1" show "$2" >"$3" 2>/dev/null || : >"$3"
}

# resolve_lockfiles <worktree> <pr>: finish a conflicted merge when every conflicted path is a
# lockfile. The side already in the train is kept (it holds the versions accepted so far), then
# `cargo metadata --offline` adds what the merged manifests need: cargo changes a lockfile only
# where a manifest requires it, never upgrades a locked version, and --offline forbids the
# network, so the result follows from the merged manifests and the kept side alone. `--locked`
# then proves the lockfile is complete. A package version that the PR's side introduced against the
# merge base (`:3:` against `:1:`) and the result lacks, such as a deliberate `cargo update`, is
# a conflict: it is named on stderr and the merge is left for the caller to abort. Returns 1,
# merge left in place, on that and on anything else.
# shellcheck disable=SC2154 # tmproot is set by the sourcing script
resolve_lockfiles() {
    local wt="$1" paths p dir tmp lost
    paths="$(git -C "$wt" diff --name-only --diff-filter=U)"
    [ -n "$paths" ] || return 1
    while IFS= read -r p; do is_lockfile "$p" || return 1; done <<<"$paths"
    tmp="$(mktemp -d "$tmproot/lock.XXXXXX")" || return 1
    while IFS= read -r p; do
        dir="$(dirname "$p")"
        blob "$wt" ":1:$p" "$tmp/base"
        blob "$wt" ":3:$p" "$tmp/theirs"
        git -C "$wt" checkout --ours -- "$p" || { rm -rf "$tmp"; return 1; }
        (cd "$wt/$dir" && cargo metadata --offline --format-version 1 >/dev/null) ||
            { rm -rf "$tmp"; return 1; }
        (cd "$wt/$dir" && cargo metadata --offline --locked --format-version 1 >/dev/null) ||
            { rm -rf "$tmp"; return 1; }
        lost="$(lost_versions "$tmp/base" "$tmp/theirs" "$wt/$p" 0 | sed 's/ /@/' | paste -sd' ' -)"
        if [ -n "$lost" ]; then
            echo "#$2: $p: versions the PR locked are not in the regenerated lock ($lost):" \
                "a lockfile change that is lost is a conflict" >&2
            rm -rf "$tmp"
            return 1
        fi
        git -C "$wt" add -- "$p" || { rm -rf "$tmp"; return 1; }
        echo "#$2: $p regenerated with cargo metadata --offline after a lockfile conflict;" \
            "every version the PR locked is in the result; check the diff of the train"
    done <<<"$paths"
    rm -rf "$tmp"
    git -C "$wt" commit -q --no-edit
}

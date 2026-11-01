#!/usr/bin/env bash
# Download the CCSDS reference documents listed in manifest.tsv into specs/pdf/
# and verify each against its SHA-256 checksum. The documents are published by
# CCSDS (https://ccsds.org) and are not redistributed in this repository.
#
# Usage: specs/fetch.sh [--all | --group v1|ref] [--verify-only | --check-urls]
#   default: --group v1 (the standards implemented in v1)
#   --check-urls: only check that each URL still answers (HTTP HEAD); downloads nothing.
#                 Used by the nightly CI job, which must never download the documents.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
manifest="$here/manifest.tsv"
dest="$here/pdf"
group="v1"
verify_only=0
check_urls=0

while [ $# -gt 0 ]; do
    case "$1" in
        --all) group="" ;;
        --group) shift; group="${1:?--group needs a value}" ;;
        --verify-only) verify_only=1 ;;
        --check-urls) check_urls=1 ;;
        -h|--help) sed -n '2,10p' "$0"; exit 0 ;;
        *) echo "unknown option: $1" >&2; exit 2 ;;
    esac
    shift
done

if command -v sha256sum >/dev/null 2>&1; then
    sha256() { sha256sum "$1" | cut -d' ' -f1; }
elif command -v shasum >/dev/null 2>&1; then
    sha256() { shasum -a 256 "$1" | cut -d' ' -f1; }
else
    echo "error: need sha256sum or shasum" >&2; exit 2
fi
if { [ "$verify_only" -eq 0 ] || [ "$check_urls" -eq 1 ]; } &&
    ! command -v curl >/dev/null 2>&1; then
    echo "error: need curl" >&2; exit 2
fi

# A worktree starts without the git-ignored PDFs: link them from the main checkout when it
# has them (a symlink; a copy if linking fails) instead of downloading them again.
if [ "$check_urls" -eq 0 ] && [ "$verify_only" -eq 0 ] && [ ! -e "$dest" ] \
    && common="$(git -C "$here" rev-parse --path-format=absolute --git-common-dir \
        2>/dev/null)"; then
    main_pdf="$(dirname "$common")/specs/pdf"
    if [ "$main_pdf" != "$dest" ] && ls "$main_pdf"/*.pdf >/dev/null 2>&1; then
        if ln -s "$main_pdf" "$dest" 2>/dev/null; then
            echo "linked  specs/pdf -> $main_pdf"
        else
            cp -R "$main_pdf" "$dest"
            echo "copied  $main_pdf -> specs/pdf"
        fi
    fi
fi

[ "$check_urls" -eq 1 ] || mkdir -p "$dest"
ok=0; failed=0; mismatched=0

while IFS=$'\t' read -r g doc file sum url title; do
    case "$g" in ''|'#'*) continue ;; esac
    if [ -n "$group" ] && [ "$g" != "$group" ]; then continue; fi
    target="$dest/$file"

    if [ "$check_urls" -eq 1 ]; then
        if curl -fsSIL --retry 3 -o /dev/null "$url"; then
            printf 'ok      %-24s %s\n' "$doc" "$url"; ok=$((ok + 1))
        else
            printf 'FAILED  %-24s URL no longer answers: %s\n' "$doc" "$url"; failed=$((failed + 1))
        fi
        continue
    fi

    if [ "$verify_only" -eq 0 ] &&
        { [ ! -f "$target" ] || [ "$(sha256 "$target")" != "$sum" ]; }; then
        printf 'fetch   %-24s %s\n' "$doc" "$file"
        if ! curl -fsSL --retry 3 -o "$target.part" "$url"; then
            rm -f "$target.part"
            printf 'FAILED  %-24s download failed; get "%s" manually (see specs/README.md)\n' \
                "$doc" "$title"
            failed=$((failed + 1)); continue
        fi
        mv "$target.part" "$target"
    fi

    if [ ! -f "$target" ]; then
        printf 'MISSING %-24s %s\n' "$doc" "$file"; failed=$((failed + 1))
    elif [ "$(sha256 "$target")" = "$sum" ]; then
        printf 'ok      %-24s %s\n' "$doc" "$file"; ok=$((ok + 1))
    else
        printf 'CHANGED %-24s %s: checksum differs from manifest' "$doc" "$file"
        printf ' (CCSDS may have issued an editorial update)\n'
        mismatched=$((mismatched + 1))
    fi
done < "$manifest"

echo "verified: $ok, failed/missing: $failed, checksum changed: $mismatched"
[ "$failed" -eq 0 ] && [ "$mismatched" -eq 0 ]

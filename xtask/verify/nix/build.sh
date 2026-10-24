#!/usr/bin/env bash
# Spike driver (docs/verification/nix-image-spike.md, OI-TBD): builds the Nix verification image
# inside the nixos/nix container on the container engine and loads it there. Nothing of Nix runs
# on the host. Not used by `cargo xtask verify`.
#
# usage: xtask/verify/nix/build.sh lock            write flake.lock (resolves the flake inputs)
#        xtask/verify/nix/build.sh build [attr...] build .#<attr>... (default: image); `image`
#                                                  streams into `<engine> load`
#        xtask/verify/nix/build.sh checks <image> <commit> <check,...> <out-dir>
#                                                  run checks of <commit> in <image> as
#                                                  `cargo xtask verify --only` runs them
#        xtask/verify/nix/build.sh shell           a shell in the builder, for debugging
#
# ALDIS_NIX_STORE names the volume that holds the builder's /nix between runs (default
# aldis-nix-spike-store): the warm store. A new name gives a cold build. ALDIS_NIX_CACHE names
# the cache volume of `checks` (default aldis-nix-spike-cache).
#
# The build context is the files the image reads, copied from the working tree, as image.rs
# copies the image inputs of a commit.
set -euo pipefail

# nixos/nix 2.35.2 (2026-08-12), the multi-architecture index, pinned by digest.
builder="docker.io/nixos/nix@sha256:7a007c766426c1877758ddc5cb87a965ac131fc78c582ce0083d922d51ae945c"
engine="${ALDIS_VERIFY_ENGINE:-podman}"
store="${ALDIS_NIX_STORE:-aldis-nix-spike-store}"
repo="$(git rev-parse --show-toplevel)"
nix_dir="$repo/xtask/verify/nix"

ctx="$(mktemp -d "${TMPDIR:-/tmp}/aldis-nix-ctx.XXXXXX")"
trap 'rm -rf "$ctx"' EXIT
for path in cpp/toolchains.lock rust-toolchain.toml tooling-nightly xtask/verify/entrypoint.sh \
  xtask/verify/nix/flake.nix xtask/verify/nix/image.nix xtask/verify/nix/flake.lock; do
  if [ -e "$repo/$path" ]; then
    mkdir -p "$ctx/$(dirname "$path")"
    cp "$repo/$path" "$ctx/$path"
  fi
done

# The sandbox needs namespaces the rootless container does not give; the builds still see only
# the store and the builder's own minimal root file system.
nix_config="experimental-features = nix-command flakes
sandbox = false
filter-syscalls = false
max-jobs = auto
cores = 0"

run() {
  "$engine" run --rm -i -v "$store:/nix" -v "$ctx:/ctx" -e NIX_CONFIG="$nix_config" \
    -w /ctx/xtask/verify/nix "$builder" "$@"
}

case "${1:-}" in
  lock)
    run nix flake lock
    cp "$ctx/xtask/verify/nix/flake.lock" "$nix_dir/flake.lock"
    ;;
  build)
    attr="${2:-image}"
    if [ "$attr" = image ]; then
      # Build, then stream the image to stdout in a second, quiet step.
      run sh -c 'nix build -L --no-link --print-out-paths "path:/ctx?dir=xtask/verify/nix#image" >&2 \
        && "$(nix build --no-link --print-out-paths "path:/ctx?dir=xtask/verify/nix#image")"' \
        | "$engine" load
    else
      shift
      refs=()
      for a in "$@"; do refs+=("path:/ctx?dir=xtask/verify/nix#$a"); done
      run nix build -L --keep-going --no-link --print-out-paths "${refs[@]}"
    fi
    ;;
  checks)
    # What image.rs `run_checks` runs, with another image and cache volume: the Rust build output
    # of one image must not be reused by another (its build scripts name the image's loader).
    image="$2" commit="$3" only="$4" out="$5"
    git_dir="$(git rev-parse --path-format=absolute --git-common-dir)"
    mkdir -p "$out"
    "$engine" run --rm -v "$git_dir:/repo:ro" -v "${ALDIS_NIX_CACHE:-aldis-nix-spike-cache}:/cache" \
      -v "$out:/out" "$image" "$commit" --out /out --only "$only"
    ;;
  shell)
    "$engine" run --rm -it -v "$store:/nix" -v "$ctx:/ctx" -e NIX_CONFIG="$nix_config" \
      -w /ctx/xtask/verify/nix "$builder" sh
    ;;
  *)
    sed -n '2,19p' "$0" >&2
    exit 2
    ;;
esac

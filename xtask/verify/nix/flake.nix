# Spike, not the gate (docs/verification/nix-image-spike.md, OI-TBD): the verification image of
# `cargo xtask verify`, built with Nix instead of xtask/verify/Containerfile. The checks still
# run in a container, through the same entry point (xtask/verify/entrypoint.sh).
#
# Built by build.sh in this directory, inside the nixos/nix container on the podman machine;
# `nix build .#image` gives a script that streams the image to `podman load`.
#
# The pins come from the files the Containerfile reads, so that both images have one source of
# truth: rust-toolchain.toml and tooling-nightly (RS-001, RS-002), and cpp/toolchains.lock
# (CPP-001), whose archives are fetched by the same URL and checked against the same SHA-256.
# Tools that toolchains.lock takes from Ubuntu (GCC, QEMU) come from the nixpkgs revision that
# last carried the pinned upstream version; the utilities come from nixos-26.05. flake.lock pins
# every input by content hash.
{
  description = "Aldis verification image built with Nix (spike, OI-TBD)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
    # QEMU 8.2.2, QEMU_VERSION of cpp/toolchains.lock.
    nixpkgs-qemu.url = "github:NixOS/nixpkgs/92d295f588631b0db2da509f381b4fb1e74173c5";
    # GCC 14.2.0, GCC_VERSION of cpp/toolchains.lock.
    nixpkgs-gcc.url = "github:NixOS/nixpkgs/566e53c2ad750c84f6d31f9ccb9d00f823165550";
    # Rust toolchains from static.rust-lang.org, checked against the hashes of the channel
    # manifests, as rustup does.
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      nixpkgs,
      nixpkgs-qemu,
      nixpkgs-gcc,
      rust-overlay,
      ...
    }:
    let
      systems = [
        "aarch64-linux"
        "x86_64-linux"
      ];
      forSystems = f: nixpkgs.lib.genAttrs systems (system: f system);
    in
    {
      packages = forSystems (
        system:
        import ./image.nix {
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ rust-overlay.overlays.default ];
          };
          pkgsQemu = import nixpkgs-qemu { inherit system; };
          pkgsGcc = import nixpkgs-gcc { inherit system; };
          root = ../../..;
        }
      );
    };
}

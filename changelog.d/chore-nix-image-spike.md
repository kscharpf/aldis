### Added
- Verification: a spike that builds the image of `cargo xtask verify` with Nix instead of
  `xtask/verify/Containerfile`, with the checks still in a container through the same entry point
  (`xtask/verify/nix/`, `docs/verification/nix-image-spike.md`). It measures build time, image
  size, tool versions against the pins, reproducibility, and a subset of checks. It changes no
  check, no image input, and no gate; `docs/09-open-issues.md` OI-TBD asks the maintainer whether
  to adopt it.

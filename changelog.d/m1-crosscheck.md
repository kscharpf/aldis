### Added
- `crosscheck/` workspace (M1 WP-4, VER-040): runs the `spp/` vectors through `spacepackets`
  0.18.0 and fails on any disagreement; run with its own tests by the `spec` check of
  `cargo xtask verify` and traced by `cargo xtask trace` (VER-040). Its own
  `crosscheck/deny.toml` adds one advisory exception (RUSTSEC-2024-0436, `paste` unmaintained);
  evaluation, coverage, and results in `docs/verification/cross-checks.md`.

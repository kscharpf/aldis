### Added
- `crosscheck/` workspace (M1 WP-4, VER-040): runs the vectors through independent implementations,
  starting with `spacepackets` 0.18.0. Its own `crosscheck/deny.toml` adds one advisory exception
  (RUSTSEC-2024-0436, `paste` unmaintained); evaluation and coverage in
  `docs/verification/cross-checks.md`.

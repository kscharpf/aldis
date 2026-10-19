### Added
- Changelog fragments: each pull request adds one file to [`changelog.d/`](changelog.d/README.md) instead of editing `CHANGELOG.md`, so parallel pull requests do not conflict. `cargo xtask changelog check` (part of `cargo xtask ci`), `show`, and `release <version> <date>`. The entries that were under **Unreleased** moved to `changelog.d/unreleased-before-fragments.md`.

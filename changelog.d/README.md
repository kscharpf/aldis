# Changelog fragments

Each pull request records its changes in **one new file in this directory** instead of editing [`CHANGELOG.md`](../CHANGELOG.md). Parallel pull requests then never conflict on the changelog. At a release, `cargo xtask changelog release <version> <YYYY-MM-DD>` moves every fragment into a new section of `CHANGELOG.md` and deletes the files.

## Format

- **Name:** the branch name with `/` replaced by `-`, e.g. `m2-tm-codec.md` for branch `m2/tm-codec`. Lowercase letters, digits, `-`, `_`, and `.` only.
- **Content:** one or more [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) sections, `### Added`, `### Changed`, `### Deprecated`, `### Removed`, `### Fixed`, or `### Security`, each with at least one `- ` entry. An entry may continue on lines indented by two spaces. Nothing else.
- **Links** are written relative to the repository root, as they will appear in `CHANGELOG.md` (for example `docs/08-roadmap.md`, not `../docs/08-roadmap.md`).
- Mark breaking changes with **Breaking:** at the start of the entry, and call out every fix to a deviation from a CCSDS Blue Book under `### Fixed` (GEN-080).

```markdown
### Added
- `aldis-tm`: TM Transfer Frame codec (TM-001…TM-012).

### Fixed
- `aldis-spp`: the Packet Sequence Count now wraps modulo 2^14 (133.0 §4.1.3.4.3.4).
```

## Commands

| Command | Does |
|---|---|
| `cargo xtask changelog check` | Checks every fragment (also part of `cargo xtask ci`) |
| `cargo xtask changelog show` | Prints the unreleased changes as the next release will list them |
| `cargo xtask changelog release <version> <YYYY-MM-DD>` | Moves the fragments into `CHANGELOG.md` and deletes them |

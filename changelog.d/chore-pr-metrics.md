### Added
- `cargo xtask metrics`: per merged pull request, the cycle time, size and areas, verification
  runs and minutes per check group, the checks the final record ran, skipped, and reused, merges
  from `main` with the paths that conflicted (`git merge-tree`), reviews and fixes after them, and
  follow-up issues; aggregated per period, with the maintainer's decision log counted per
  category; as Markdown or JSON (docs/06-verification.md §9, VER-063). Needs `gh` and the network,
  so it is not part of `cargo xtask ci`.
- Nightly job `metrics` (`.github/workflows/nightly.yml`): keeps the report of the last 30 days in
  the pinned issue "Process metrics" and the JSON as an artefact, without failing the night or
  opening the nightly-failure issue (VER-063).
- The milestone exit reviews the process metrics and opens the top friction item as a follow-up
  (`docs/milestones/checklists.md`).

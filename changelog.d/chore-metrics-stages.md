### Added
- `cargo xtask metrics`: where each pull request's time went. A stage timeline (first commit,
  opened, first passing record, first review, last maintainer decision, approval, final passing
  record, merged) with the time between stages, raw and in the maintainer's active time (within
  30 min of a maintainer comment, review, or merge); the raw wait for the first review is labelled
  not actionable. The approval is the first `APPROVED` review, else the maintainer's last word if
  only merges followed it, else the merge (docs/06-verification.md §9, VER-063).
- `cargo xtask metrics`: the process-created waits, per pull request and per period: order
  inversions (a pull request approved while one it depends on by branch ancestry was unreviewed),
  verification after approval, failed runs with the time spent after their first gating failure
  (from the run's artefact), cancelled runs, rework after a maintainer decision, and repeated
  conflicts. A "Recommendations input" section compares them with fixed thresholds and with the
  previous period of the same length (`--no-previous` skips it). The wording of VER-063 for these
  is proposed, pending the maintainer.

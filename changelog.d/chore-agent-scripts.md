### Added
- `scripts/pr-status` (is a pull request ready to merge: required contexts, the newest verify run, the latest review, a `READY` or `NOT READY` verdict), `scripts/cleanup-merged` (remove the worktree and branches of a merged pull request, only when its tip is on `origin/main`), and `docs/process/agent-brief.md` (the conventions for briefing and reporting of a dispatched agent).

### Changed
- A change to `scripts/` alone runs only `spec` in a verification run, as documentation does: no check runs or reads those scripts (VER-056).

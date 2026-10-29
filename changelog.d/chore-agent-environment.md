### Added
- `cargo xtask check`: format, clippy, tests, the bare-metal clippy build, and `cargo xtask ci`, plus the extras of each touched directory, in one command.
- `.claude/hooks/session-start.sh` (SessionStart hook): warns when the checkout is behind `origin/main` or its branch is merged or its upstream gone.
- `scripts/reverify` (merge `origin/main`, check, push, and dispatch the verification run for open PRs) and `scripts/prune-agent-branches` (dry-run cleanup of finished agent worktrees and branches).

### Changed
- `specs/fetch.sh` links `specs/pdf` from the main checkout when it has the PDFs; `.gitignore` ignores `specs/pdf` also as a symlink.
- spec-lint: a withdrawn requirement starts `Withdrawn: <reason>` and in no other form; `trace` and the assurance check recognise only that form (00-introduction.md §5.2).
- `docs/06-verification.md` §7 holds the merge-gate commands that AGENTS.md carried.

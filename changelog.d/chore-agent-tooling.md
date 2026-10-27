### Added
- `scripts/agent-env.sh`: helper script configuring agent shell environment variables (`NO_COLOR=1`, `GH_FORCE_TTY=0`, `GH_NO_UPDATE_NOTIFIER=1`) and `gh run-status` alias (#139).

### Changed
- `AGENTS.md`: added conventions on workspace search boundaries (avoiding `.claude/`, `.git/`, and external caches; preferring `git grep`) and structured GitHub CLI queries to prevent runaway scans and background timeouts (#139).

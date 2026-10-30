### Added
- `scripts/reverify` takes a lock per pull request (shared by all worktrees, released however the script ends): a second run of the same PR is refused with the holder's pid, `--wait` queues behind it, a lock whose process is gone is taken over. `--stale-retries <n>` (default 2) brings a STALE PR up to date and verifies it again; a FAILED one is never retried. `--status <pr>` prints the lock or the last final line in one line with no network, and `--status --wait <pr>` waits for it. `scripts/test-reverify` tests the script with no network, container, or real `gh` and `cargo`.

### Changed
- `docs/process/agent-brief.md` carries the rules for running `scripts/reverify` (one run per PR, leave the branch alone while it runs, wait for the final line, STALE retries); `AGENTS.md` says that implementation agents commit and stop unless the brief grants the gate.

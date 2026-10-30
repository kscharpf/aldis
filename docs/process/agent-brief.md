# Briefing an agent

For the coordinator who dispatches an agent, and for the agent. A dispatched fork or subagent inherits [AGENTS.md](../../AGENTS.md); this file holds the conventions AGENTS.md does not carry. A brief names the task and links here.

## The brief

Name the goal, the base, what done means, and what the agent leaves alone: other pull requests, other tracks' directories ([roadmap §6](../08-roadmap.md)). Add what the agent cannot find in the repository: a decision already made in chat, a measurement to compare against, a pull request to stack on.

## Conventions for the agent

1. **Worktree.** Work in `.claude/worktrees/<branch>`, the path `scripts/reverify` and `scripts/cleanup-merged` expect, created from a fresh `git fetch origin`. The search boundary of AGENTS.md says what to search, not where to create a worktree. Run `source scripts/agent-env.sh`. Put temporary files in the session scratchpad.
2. **Foreground.** Wait for a run in bounded foreground calls: `perl -e 'alarm 170; exec @ARGV' gh run watch <id> --exit-status`, repeated until the run ends. Exit status 142 (SIGALRM) means the call timed out: watch again. Any other nonzero status means the watch ended: read the conclusion with `gh run view <id> --json status,conclusion`. The harness moves a command that runs longer than 180 s to the background and blocks a foreground `sleep`, and a turn that ends while work is pending notifies the coordinator, who must answer it. End the turn with the final report only.
3. **Gate.** Run `cargo xtask check`, push, and stop: the coordinator brings the pull request up to date and verifies it (AGENTS.md). A brief that gives you the gate says so; then merge `origin/main`, re-run `cargo xtask check`, push, run `scripts/reverify <pr>`, and confirm `cargo xtask verify check`. When `main` moves during the work, merge it again before the final run.
4. **Pushes.** Confirm the pull request is open (`gh pr view <n> --json state`) before every push to its branch.
5. **Boundaries.** Stop at an open pull request, with a published record when the brief gave you the gate. Merging is the maintainer's. Leave other pull requests alone, and leave the worktree in place: the coordinator removes it after the merge.
6. **Measurement.** Measure on a throwaway branch: one full run of every check, the branch and its worktree deleted after the results are read. The record notes the run publishes under `refs/notes/verify` stay.

## The report

End with: the pull request URL; the evidence (command output, timings against the stated baseline, and the `verify check` result when you had the gate); **Judgement calls** (decisions the agent made under [decision-rights.md](decision-rights.md), each with its reason); **For the maintainer** (what only the maintainer can decide, as one question with a recommended option).

## For the coordinator

- Merge readiness of a pull request: `scripts/pr-status <n>...` prints the required contexts, the newest verify run, the review text, and a `READY` or `NOT READY: <reasons>` verdict.
- After a merge to `main`: refresh the next pull request with `scripts/reverify <n>`.
- After a pull request merges: `scripts/cleanup-merged <branch>` shows what it would remove, and `--apply` removes the worktree and branches. It keeps a branch whose tip is not an ancestor of `origin/main`, a dirty worktree, and a branch without a merged pull request, and it stops a branch at the first step that fails. `scripts/test-cleanup-merged` tests it with no network and no real `gh`. `scripts/prune-agent-branches` clears leftovers in bulk.

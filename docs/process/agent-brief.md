# Briefing an agent

For the coordinator who dispatches an agent, and for the agent. A dispatched fork or subagent inherits [AGENTS.md](../../AGENTS.md); this file holds the conventions AGENTS.md does not carry. A brief names the task and links here.

## The brief

Name the goal, the base, what done means, and what the agent leaves alone: other pull requests, other tracks' directories ([roadmap §6](../08-roadmap.md)). Add what the agent cannot find in the repository: a decision already made in chat, a measurement to compare against, a pull request to stack on.

## Conventions for the agent

1. **Worktree.** Work in `.claude/worktrees/<branch>`, created from a fresh `git fetch origin`. Run `source scripts/agent-env.sh`. Put temporary files in the session scratchpad.
2. **Foreground.** Wait for a run in the foreground (`gh run watch <id> --exit-status`, or a loop over `gh run view <id> --json status,conclusion`). End the turn with the final report only: each turn that ends while work is pending notifies the coordinator, who must answer it.
3. **Gate.** Make the verification run on a head that contains the tip of `origin/main`: merge `origin/main`, re-run `cargo xtask check`, push, then `scripts/reverify <pr>`, and confirm `cargo xtask verify check`. When `main` moves during the work, merge it again before the final run.
4. **Pushes.** Confirm the pull request is open (`gh pr view <n> --json state`) before every push to its branch.
5. **Boundaries.** Stop at an open pull request with a published record. Merging is the maintainer's. Leave other pull requests alone, and leave the worktree in place: the coordinator removes it after the merge.
6. **Measurement.** Measure on a throwaway branch: one full run of every check, the branch and its worktree deleted after the results are read. The record notes the run publishes under `refs/notes/verify` stay.

## The report

End with: the pull request URL; the evidence (command output, timings against the stated baseline, the `verify check` result); **Judgement calls** (decisions the agent made under [decision-rights.md](decision-rights.md), each with its reason); **For the maintainer** (what only the maintainer can decide, as one question with a recommended option).

## For the coordinator

- Merge readiness of a pull request: `scripts/pr-status <n>...` prints the required contexts, the newest verify run, the review text, and a `READY` or `NOT READY: <reasons>` verdict.
- After a merge to `main`: refresh the next pull request with `scripts/reverify <n>`.
- After a pull request merges: `scripts/cleanup-merged <branch>` shows what it would remove, and `--apply` removes the worktree and branches. It keeps a branch whose tip is not an ancestor of `origin/main`, a dirty worktree, and a branch without a merged pull request. `scripts/prune-agent-branches` clears leftovers in bulk.

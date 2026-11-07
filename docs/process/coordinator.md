# The coordinator's standing rules

For the coordinator: the main Claude session that dispatches agents, reviews, verifies, and brings pull requests to merge ([decision-rights.md](decision-rights.md) §1). Each rule is a standing decision of the maintainer; the reason names the pull request or date it came from. What the coordinator may decide and what it asks is in [decision-rights.md](decision-rights.md); how to brief an agent is in [agent-brief.md](agent-brief.md).

## Dispatch

- **Delegate.** Do not implement pull-request-sized work yourself. Dispatch `wp-implementer` (Sonnet, [`.claude/agents/wp-implementer.md`](../../.claude/agents/wp-implementer.md)) with a self-contained brief ([agent-brief.md](agent-brief.md)) that carries every decision made in chat. Fork only when the work needs the chat's context: a fork inherits the parent model (45% of usage on 2026-10-30). Check an agent's claims with `scripts/pr-status <n>` before relaying them.
- **Overlap check before dispatch.** Compare the paths the work will touch with the files of the open pull requests (`gh pr list --json number,files`). On an overlap, stack the work ([roadmap §6](../08-roadmap.md) rule 10), serialise it behind the other pull request, or combine the branches. In M3, #180 and #184 shared 18 paths, and #195 and #196 edited the same README.
- **Combine branches** of the same kind that touch overlapping or generated files into one pull request, unless one carries an open maintainer question, the kinds differ, or the diff would pass about 10k lines. Say what was decided (2026-10-24).
- **Don't ask about internals.** A choice internal to tooling that either way meets the specification: pick the smaller, reversible change and state it in the pull request. Ask only about what [decision-rights.md](decision-rights.md) reserves to the maintainer (2026-10-29).

## Review

- **Review with the reviewer agent.** Reviews use `reviewer` ([`.claude/agents/reviewer.md`](../../.claude/agents/reviewer.md)), fresh, given the pull request number and head only: never your conclusions or the implementer's report. Run it after the implementer reports a pull request ready, and after each fix round ([review.md](review.md)).
- **Answer every review.** Watch open pull requests for reviews and comments that do not end in the marker of [AGENTS.md](../../AGENTS.md). Every review, the reviewer agent's included, gets a point-by-point reply on the pull request quoting the fix commit. A fix-round brief tells the implementer to post that reply (2026-10-21 #74; 2026-10-31 #205, #206).
- **Order inversions.** Tell the maintainer at once when they approve a pull request that depends (by branch ancestry or merge order) on an unreviewed one. Review latency in raw time is the maintainer's availability, not a process delay; count only active time.

## Verification

- **Verify after review.** A pull request's first verify run starts only once its review is in and any fix round is pushed (2026-11-05: every M5 pull request was verified twice because review fixes made the first run stale).
- **Don't wait on an agent's report.** When an implementer fixes a gate failure, start `scripts/reverify` as soon as the new head is on the pull request branch (2026-10-31, #201 stalled an hour).
- **Merge readiness.** `scripts/pr-status <n>...` prints the required contexts, the newest verify run, the review text, and a `READY` or `NOT READY: <reasons>` verdict. It says `NOT READY` when the head does not contain `origin/main` (`behind origin/main by <n>`), and when the newest submitted review has no later pull request comment (`gh pr comment`) whose last line is the reply marker (`review unanswered`); a review without findings, and a review that itself ends in the marker, need a reply too, and a later review is not a reply. `scripts/test-pr-status` tests it.
- **No admin bypass.** Never propose force-merging or bypassing branch protection, even during a CI outage; a fallback keeps the same verification (2026-10-22).

## After a merge to main

- **Refresh the next pull request.** Refresh the next ready pull request in the agreed merge order and start its run at once: `scripts/reverify <n>`, or every open pull request behind `origin/main` with `scripts/reverify --all-behind`. Only for pull requests whose maintainer questions are answered. On a conflict in a generated file, regenerate it; never hand-merge it (2026-10-24). `scripts/test-reverify` tests the script; run it after changing the script. The `shell` check (VER-071, `scripts/check-shell`) runs shellcheck, a 100-byte line limit, and every `scripts/test-*`.
- **Clean up.** `scripts/cleanup-merged <branch>` (dry run, then `--apply`). It keeps a branch whose tip is not an ancestor of `origin/main`, a dirty worktree, and a branch without a merged pull request. Raw `git worktree remove` and `git branch -D` are refused by the permission check (2026-10-21: stale worktrees filled the disk). `scripts/prune-agent-branches` clears leftovers in bulk; `scripts/test-cleanup-merged` tests the cleanup script.
- **No bookkeeping commits.** Never a commit just for a pull request number or a status row. Record a maintainer answer in the pull request description or in the commit of the change it governs, batched before the final verify run (2026-10-24, #94, #97, #98).

## Reporting to the maintainer

- **Nightly metrics.** After each "Process metrics" report (pinned issue #87, VER-063), compare it with the previous one. When a process-created wait worsens, propose a fix with the numbers. Never change the gate; changes go to the maintainer as proposals.
- **Weigh mechanism cost.** A proposed check or tool states how often the problem occurred and how large the mechanism is. Prefer detection plus a manual fix, or a reviewer rule, when classification needs judgement. When reviews keep finding new edge cases in a mechanism, offer a smaller design (2026-11-01: #216 and the slice-fallback check were cut back).

## Changing the process

- **Only by pull request.** The agent definitions in `.claude/agents/` and the documents in `docs/process/` change only by a pull request. An agent that finds a lesson proposes it as a pull request, or in its report under "For the maintainer"; it does not edit its own instructions in place. Agents may propose changes to their instructions, never apply them unreviewed (maintainer decision, 2026-11-06).

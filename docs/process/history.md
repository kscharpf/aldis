# Why the process rules are what they are

For people. Agents are not told to read this file: the rules are in [AGENTS.md](../../AGENTS.md), [agent-brief.md](agent-brief.md), [review.md](review.md), [coordinator.md](coordinator.md), [decision-rights.md](decision-rights.md) and [pr-body.md](pr-body.md), each with at most a short decision tag. This file keeps the reasons, the dated incidents and the worked examples that those files used to carry, keyed by the rule they justify. Nothing here is a rule; if this file and a rule disagree, the rule wins.

## AGENTS.md

- **Coverage waiver shrink.** The train's coverage check said "waived but only N missed; shrink the waiver" without the command, so AGENTS.md and agent-brief item 3 carried it: run `cargo xtask coverage -p <crate>` with `ALDIS_COVERAGE_SHRINK=1` and commit the result. The error string in `xtask/src/coverage.rs` now names the command, and both sentences were removed.
- **README first.** AGENTS.md used to say "read README.md first"; the "Where things go" table maps the workspace, so a focused task does not need the README on turn 1.
- **Independent review section.** It repeated the bullets of review.md and of `.claude/agents/reviewer.md`; the protocol now has one copy, [review.md](review.md). `.claude/agents/reviewer.md` no longer reads decision-rights.md; the definition of blocking from its section 2.6 moved to review.md word for word.

## agent-brief.md

### Item 1, worktree and shell

- **Hooks.** The `PreToolUse` hook `.claude/hooks/base-checkout-guard.sh` (tested by `scripts/test-base-checkout-guard`) refuses Edit, Write and NotebookEdit on a file in the base checkout outside `.claude/worktrees/`, for the coordinator as well. The `PreToolUse` hook on `Bash`, `.claude/hooks/detach-guard.sh` (tested by `scripts/test-detach-guard`), refuses a command that detaches a process (`&`, `nohup`, `disown`, `setsid`). Both print their own `BLOCKED:` message with the remedy, so agents are not told the script paths.

### Item 2, waiting

- **Why no poll loop, no `sleep`, no read while waiting.** The harness blocks a foreground `sleep` and moves a command that runs longer than 180 s to the background anyway; a hand-made wait that outlived its command has started a second run on top of the first.
- **Tiers.** `--quick` (about a minute) runs format and clippy with the gate's lints of the root, `exerciser/`, `fuzz/` and `crosscheck/`, and reports every error at once. The default stops at the first failure. `--full` adds coverage, `features`, `stack`, `big-endian` and the exerciser's QEMU run when the gate would run them for the change; a missing tool is reported as skipped with the way to install it. The steps are chosen by the inputs the gate's checks declare, so a change of `Cargo.lock` or `xtask/` runs them too. The skipped steps and the hints come before the last line (`passed with skips` when a tool is missing). The steps run under the gate's `RUSTFLAGS` and `RUSTDOCFLAGS` (`-D warnings`), so the first run after a change of those flags rebuilds. VER-068 to VER-070 require the tool to print all of this.
- **One check at a time.** The lock is per worktree, in its `target/`; a second run is refused with the holder's pid. The auto-mode permission check refuses `pkill` and kill, and a killed run leaves a stale lock that the next run takes over.
- **`--from`.** Skips the steps before the named one; its last `check: partial` line says `not a pass`, because the exit code is 0 for a passing partial run.

### Item 3, gate

- **Why the first and final round only.** The train's verify still decides the merge; the default tier on every round was cost without a gate effect.
- **One reverify per PR.** The lock is per pull request, across worktrees, so that two sessions do not run the same verification twice.
- **STALE.** `scripts/reverify` retries a STALE line at most twice, because `main` moves during a ten-minute run; a FAILED line is never retried.
- **`--cancel`.** The cancelled run publishes no record, so the next merge of `main` re-runs the checks whose inputs changed and reuses nothing from it (VER-060). That is why it is opt-in and not used for a run whose head contains `main`, or when only documentation changed and a stacked refresh could reuse the record.
- **Timeouts.** A waiter behind another reverify follows the holder's deadline, not a pid, because a pid may be the lock's, a wrapper's, or the waiter's own. The mechanics are in the header of `scripts/reverify`.

### Item 4, pushes

- **Read the last line of the check run.** On 2026-11-07 an agent pushed a head that failed the `shell` step because it read a tailed log that ended earlier (#340).
- **A refused command.** On 2026-11-07 an agent opened a pull request through `gh api` after Claude Code's worktree-isolation guard refused `gh pr create` (the implementer of #332 reported it; the pull request does not record it). Editing a file inside your own worktree with the Edit tool is the ordinary way to change it, not a way around a refused command.
- **A hung `shell` step.** `scripts/test-reverify` can hang inside the `shell` step (#261; once for 4 h on 2026-11-07).

### Item 5, build directories

A build directory takes 7 to 10 GiB per worktree; on 2026-11-07 the disk filled twice and broke a check run (maintainer decision, 2026-11-07). A later fix round rebuilds, which is the cost accepted.

### The report

- **Interim notes.** On 2026-11-07 two of five agents ended with a report about a check run alone, with no pushed head, and the coordinator sent each back.
- **Decisions met.** In M3, #174 hand-typed an enum that an accepted convention ruled out.

## review.md

- **Slice-fallback proof.** No check finds the `unwrap_or_default` / `.min(` / `unwrap_or(0)` pattern, because telling a violation from an allowed use is a judgement call (maintainer, 2026-11-01).
- **Doc/code nits as one batch.** The implementer fixes them in the same round as the code findings, so they cost no round of their own (OI-63).

### Convergence of reviews

Follow-ups #364 (on #362) and #370 (on #366) were nitpick findings that outlived the three-review cap: each final review still raised new non-blocking items, on lines earlier reviews had read. The maintainer decided on 2026-11-12 that review 1 is the only full review, later reviews check the fixes and the changed lines, non-blocking findings start no review round, a review gives at most 5 nits as replacement text, a documentation-only pull request is reviewed only for contradictions, unactionable instructions and broken references, and every finding states its consequence.

## coordinator.md

### Dispatch

- **Fork.** A fork inherits the parent model, which was 45% of usage on 2026-10-30.
- **Overlap check.** In M3, #180 and #184 shared 18 paths, and #195 and #196 edited the same README.
- **At most 2 subagents.** On 2026-11-09 parallel agents drove the load average above 100 and filled the disk, and `cargo xtask check --full` failed with "No space left on device".

### Review

- **Answer every review.** Recorded in 2026-10-21 (#74) and 2026-10-31 (#205, #206).
- **Dispatch `reviewer` at once** (2026-11-07).

### A comments-only fix round

In the A1 retrospective follow-up of 2026-11-07/08 every fix round got a fresh `reviewer`; #349 took six review rounds and #350 five, each fresh reviewer raising new nits on the previous round's nit fixes, while the last rounds of #349, #352 and #353, which changed only comments, doc comments, or Markdown prose, were read by the coordinator without a comment recording the read on the pull request (which the rule requires), and readied for the train (#356; maintainer decision, 2026-11-08). Under the list of allowed changes, #352 (code comments), #353 (one doc comment in `xtask/src/consistency.rs`) and #349 (doc comments in `refapp/refapp-scenario/src/table.rs`) would still qualify.

### Verification

- **Verify after review.** On 2026-11-05 every M5 pull request was verified twice because review fixes made the first run stale.
- **Don't wait on an agent's report.** On 2026-10-31, #201 stalled an hour.

### After a merge to main

- **Cleanup.** On 2026-10-21 stale worktrees filled the disk, which is why the auto-mode permission check refuses raw `git worktree remove` and `git branch -D`. The closed-pull-request cases were maintainer decisions of 2026-11-08 and 2026-11-09. The tests of the script are in its header and in `scripts/test-cleanup-merged`.
- **Base checkout.** On 2026-11-10 the base checkout was 4 commits behind and agents got the pre-#420 check rule.
- **No bookkeeping commits.** Recorded 2026-10-24 (#94, #97, #98).

### Merge trains

- **Measurement behind the decision** (maintainer, 2026-11-07). Every merge to `main` made every other open pull request stale, and each needed a fresh `scripts/reverify` (about 10 minutes hosted); #316 needed four verify runs and #313 three, mostly because `main` moved.
- **Only the maintainer assigns pull requests** (maintainer, 2026-11-09). On 2026-11-08 two sessions built overlapping trains, #368 and #369, 13 s apart.

### Reporting

- **Weigh mechanism cost.** 2026-11-01: #216 and the slice-fallback check were cut back.

### Multiple coordinators

Decision of 2026-11-06.

## decision-rights.md

Examples that followed the tables of section 2:

- **2.1.** OI-35 items 1 and 8 (notes only; 231.0 against 732.1) and AOS-018's limits went to the maintainer; items 2, 3, and 5-7 were answered from the text in the same PR (#61).
- **2.2.** USLP-016 was a new requirement for a SHALL in scope (#61). Adding TV-023 to the M2 exit table (#74) was the maintainer's: it comes from no frame standard.
- **2.3.** M2 decisions D1-D17 (#56).
- **2.4.** The negative status codes and `struct_size` first in `aldis_error_detail_t` (#72); `Ocf` closed (#68); `packet_len` kept as one code (#60).
- **2.5.** The C++ loader stays stricter than the Rust runner (#66, follow-up #67) was the maintainer's; moving the FHEC miscorrection case to `fhec_miscorrection.json` (#64) is routine under the file-name rule.
- **2.6.** The OID and CRC check fix in `cpp/vectors/` (e73db7d, #66), and the follow-up issues #65, #67, #69, are routine. The maintainer's words of 2026-11-08, given in the coordinating session: "we should strive for max 3 review rounds and open non-blocking items at that point become follow-on issues."
- **2.7.** The cleanup and review-watch policies, and holding the `xtask` split (#75).
- **2.8.** `cbindgen` 0.29.4 was checked against VER-055 and recorded (#72).
- **2.9.** The tag `v0.2.0-m1` (#54), the repository made public (#57), the GHCR package `aldis-verify` made public (#63).
- **2.11.** The kept branch of the `xtask` split (#75).

### 2.10, merge trains

The section used to restate [coordinator.md](coordinator.md#merge-trains): each pull request keeps its own independent review and reply and needs no verify record, the merge gate (VER-058) applies to the train PR, one train for all scopes, a pull request that waits on a maintainer decision stays out (label `needs-maintainer`), a failing train drops its cause, a conflict leaves out the later pull request. Train assignment (maintainer, 2026-11-09) and the cap of 2 concurrent subagents (maintainer, 2026-11-09) are in coordinator.md as well.

### 6, the questions of 2026-10-20 and 2026-10-21

The coordinator asked the maintainer about each of these. Those that were the maintainer's are recorded in the pull requests and open issues that settled them. These were routine, and under section 2 would not have been asked:

| Question | Category | Decided under these rights by | Rule |
|---|---|---|---|
| Move the FHEC miscorrection case to its own file (#64) | `vectors` | Agent | A file name is a promise about its contents; the case was not released (2.5) |
| Fix the OID and CRC agreement checks of `cpp/vectors/` (e73db7d, #66) | `review` | Coordinator | A finding that changes no behaviour, spec, or ABI (2.6) |
| Defer non-blocking review items to follow-up issues (#65, #67, #69) | `review` | Coordinator | Non-blocking findings go to a follow-up issue named in the reply (2.6) |
| Merge `main` into the open pull requests and re-verify | `housekeeping` | Coordinator | 2.11 |
| Delete the worktrees of merged pull requests | `housekeeping` | Coordinator | The cleanup policy, itself a maintainer decision (2.7, 2.11) |
| Fix a review finding that changes no behaviour, spec, or ABI | `review` | Coordinator or agent | 2.6 |

## Script tests

The older coordinator.md named the offline tests of the scripts: `scripts/test-pr-status` (no network, a stub `gh`), `scripts/test-reverify` (no network, no container, no real `gh` or `cargo`), `scripts/test-cleanup-merged` (no network, no real `gh`), `scripts/test-train` (no network, a stub `gh`). The rule to run them is in coordinator.md, "Refresh the next pull request".

## pr-body.md

The parsing of merge commits by `cargo xtask changelog` is documented in the module docs of `xtask/src/changelog.rs`. The older text said a hand-built train is resolved with `gh pr list --state merged --head <b>`; the code uses `--search <sha>`, and the module docs say so.

# Independent review

Protocol and requirements for independent review of pull requests on Aldis: the one copy of the protocol. The reviewer agent ([`.claude/agents/reviewer.md`](../../.claude/agents/reviewer.md)) and [AGENTS.md](../../AGENTS.md) point here. Reasons: [history.md](history.md).

A reviewer of a pull request does **not** re-run the verification checks: `cargo build`, `cargo check`, `cargo test`, `cargo clippy`, `cargo fmt`, `cargo xtask ci`, coverage, the exerciser, fuzzing, or `cargo xtask verify`. They are the merge gate's job (VER-056…VER-058), and the gate already ran them on the head commit. Read the evidence instead: `cargo xtask verify check <head>` re-checks the published record, and `scripts/pr-status <n>` shows it, and a check that does not gate (`cpp-leon3`, `rust-sparc-leon3`) is read from the record, not assumed to pass. `kani` is nightly-only: a pull request's record shows it as not run ("nightly only (VER-059)"), and its results are in the nightly run on `main`, whose failure opens the nightly issue. A pull request that rides a merge train ([coordinator.md](coordinator.md#merge-trains)) has no record of its own: its review reads the code, and the record of the train's head covers it once the train is verified. The review and the implementer's reply stay per pull request (a comments-only fix round may be read by the coordinator instead of reviewed, [coordinator.md](coordinator.md#a-comments-only-fix-round)); only verification moves to the train. The reply to a review names the head it answers (a hex prefix of at least 7 characters, as in "Fix round, head 81db08a2"): `scripts/pr-status --train` checks that the reply names the current head, so a push after the reply sends the pull request back for a new reply.

Spend the review on what no check covers: the reading of the Blue Book behind each decision (cite the clause), the requirements and whether the tests and vectors really cover them, the soundness of every `unsafe` block, the shape of the public API and C ABI, and the decisions and judgement calls the pull request lists. Read every suppression the change adds, because the gate passes *because of* it: each `#[expect(…, reason = "…")]` (RS-016), `// NOLINT(<check>): <reason>` (CPP-042), entry of `coverage-waivers.toml` (VER-024), and mutant triaged as equivalent in [mutants-triage.md](../verification/mutants-triage.md) (VER-025). Check that the reason is true and the region is the smallest one; a reason that does not hold is a finding. Report findings as the section "Reporting findings" below describes, each with the file and line and why it matters; to show a defect, a failing test or vector case is the clearest form.

**Every `unwrap_or_default`, `.min(` used as a range bound, and `unwrap_or(0)`-style default that the diff adds on a wire-input path needs its proof,** and the reviewer reports each site without one. The proof is one of: an explicit error (`Incomplete`, `LengthMismatch`, `InvalidField`, `Malformed`); a length check that the reviewer can point to with `path:line`; or the allowed case of an absent optional or variable-length field, where an empty slice or 0 is the value. A clamp such as `start.min(len)..end.min(len).max(start)` that only satisfies `clippy::indexing_slicing` is none of these. No check finds the pattern, because telling a violation from an allowed use is a judgement call (maintainer, 2026-11-01); a clamp that only keeps a `copy_within` from panicking on a length it already proved is not a fallback. The rule is in [AGENTS.md](../../AGENTS.md).

**Every statement about the implementation cites the file and line it rests on** (`path:line`), quoting the code where the point depends on it, and a claim about a dependency, an algorithm, or a target names where it is in the code. A statement the reviewer cannot point to is not made. A review whose description of the code does not match the code does not count as a review of that pull request: the coordinator posts the corrections and the review is done again.

**Mismatches between documentation and code, or between two pieces of documentation, are reported together as one batch of nits at the end of the review.** The implementer fixes them in the same round as the code findings, so they cost no round of their own (OI-63).

**A pull request gets at most three reviews by `reviewer`;** the cap, what happens to the findings left after the third, and the case of a blocking finding there are in [coordinator.md](coordinator.md) ("At most three reviews").

## Reporting findings

**Blocking** means a failing check, a broken rule of AGENTS.md, or a defect against a requirement or the Blue Book; anything else is non-blocking. Fixing code to meet the existing specification is not a behaviour change.

- Findings first, most severe first, each marked **blocking** or **non-blocking**, then the batch of nits.
- Check whether the tests and vectors with a `// req:` comment verify the behaviour and not just mention it.
- Check the diff against each accepted decision or convention that the pull request body, its plan, or the brief names, and report each as met or not met with `path:line`.
- Check that the pull request respects any process freeze the plan names (no changes to `xtask/`, `scripts/`, `.github/`, `docs/process/` unless the plan allows).
- Cite Blue Book clauses by document number and clause, never from memory; never commit or quote Blue Book text ([AGENTS.md](../../AGENTS.md)).

## Method and posting

- Read the diff file by file: `gh pr diff <n> --name-only`, then `git diff origin/main...<head> -- <path>` for each file. Never load the whole diff at once.
- Before posting, check that you have not already posted a review of this head (`gh pr view <n> --json reviews`).
- Post with `gh pr review <n> --comment` only: `--request-changes` and `--approve` fail on the maintainer's own pull requests, so say in the body which findings block the merge. End the body with the marker line `🤖 Generated with [Claude Code](https://claude.com/claude-code)`.
- Return a short summary: findings ranked by severity, each with `path:line`, and the nit batch.

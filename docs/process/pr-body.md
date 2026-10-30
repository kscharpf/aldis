# The pull request body

A pull request body has three sections and no others: **Summary**, **Evidence**, **Merge Danger**. It links; it does not restate (OI-63).

- **Summary**: what changes and why, in a few lines, with the requirement IDs it touches.
- **Evidence**: what the reviewer cannot read off the diff: before and after counts, command output, timings against a stated baseline, and the `cargo xtask verify check` result when the author had the gate.
- **Merge Danger**: what can go wrong at merge: conflicts with open pull requests, rows left for another pull request, an order the merges must follow.

**The title** is the changelog entry and the key of the milestone tables: a conventional commit, `type(scope)!: description`. A pull request of a milestone work package is titled `feat(m3/wp-4): Implement <description of work product 4 in milestone 3>`: the type is a conventional-commit type, and the scope `m<N>/wp-<n>[letter]` names the milestone and the work package. The title scope decides; the branch name (`m3/wp-3b-farm1`) is read only for a title without such a scope, which covers older pull requests.

`feat` lists under Added, `fix` under Fixed (a fix of a Blue Book deviation is called out, GEN-080), `perf` and `refactor` under Changed, and `!` marks a breaking change; `docs`, `chore`, `test`, `ci`, and `build` are not listed. `cargo xtask changelog` and `cargo xtask changelog release` read the titles from the merge commits of `main` (a scope with `/` is not printed in the entry); `cargo xtask status-table [--write]` reads GitHub. Both are coordinator tools, run at a release or when the tables are due, and no check of the gate.

A decision lives where the project keeps decisions: in its open issue ([09-open-issues.md](../09-open-issues.md)) or its requirement row, and the body links to it. There is no "Decisions" section that repeats open issues. A question for the maintainer, and a judgement call made under [decision-rights.md](decision-rights.md), go in the report of the agent ([agent-brief.md](agent-brief.md)), not in the body. A body of an agent ends with the line `🤖 Generated with [Claude Code](https://claude.com/claude-code)`.

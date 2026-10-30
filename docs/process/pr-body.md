# The pull request body

A pull request body has three sections and no others: **Summary**, **Evidence**, **Merge Danger**. It links; it does not restate (OI-63).

- **Summary**: what changes and why, in a few lines, with the requirement IDs it touches.
- **Evidence**: what the reviewer cannot read off the diff: before and after counts, command output, timings against a stated baseline, and the `cargo xtask verify check` result when the author had the gate.
- **Merge Danger**: what can go wrong at merge: conflicts with open pull requests, rows left for another pull request, an order the merges must follow.

A decision lives where the project keeps decisions: in its open issue ([09-open-issues.md](../09-open-issues.md)) or its requirement row, and the body links to it. There is no "Decisions" section that repeats open issues. A question for the maintainer, and a judgement call made under [decision-rights.md](decision-rights.md), go in the report of the agent ([agent-brief.md](agent-brief.md)), not in the body. A body of an agent ends with the line `🤖 Generated with [Claude Code](https://claude.com/claude-code)`.

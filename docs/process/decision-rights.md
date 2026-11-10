# Decision rights

Who may decide what on Aldis, so that routine questions don't wait for the maintainer, and the questions that do reach the maintainer arrive with an answer ready to accept. Requirement: VER-062 ([06 §10](../06-verification.md#10-decision-rights)). This document is itself the maintainer's decision; changing it is a process decision (§2.7). Worked examples and the questions that led to these rights: [history.md](history.md).

## 1. Roles and where each records its decisions

| Role | Who | Records a decision in |
|---|---|---|
| **Maintainer** | The project owner | Where the rules below say: the OI, the plan's Decisions table, the requirement, the PR's "Maintainer decisions" section; a decision that has none of these goes in the description or commit message of the change it governs |
| **Coordinator** | The Claude session that plans the work, dispatches agents, opens and readies pull requests, and answers reviews (several may run at once, each for its own scope: [coordinator.md](coordinator.md#multiple-coordinators)) | The PR description, under **Coordinator decisions**, or its reply to the review |
| **Agent** | A session dispatched for one work package or task, in its own worktree and branch | Its report, under **Judgement calls**; the coordinator copies them into the PR description |

A contributor who is neither coordinator nor agent decides as an agent for their own pull request and asks the maintainer as §3 says.

The coordinator briefs an agent as [agent-brief.md](agent-brief.md) says and follows the standing rules of [coordinator.md](coordinator.md).

The rules of [AGENTS.md](../../AGENTS.md), the [checklists](../milestones/checklists.md), and the checks of `cargo xtask` decide most questions already; nobody asks about those (§4). What follows covers what they leave open.

## 2. Who decides what

"Instead of asking" is what the role that may not decide does.

### 2.1 Spec interpretation

| Maintainer only | Coordinator | Agent in a work package |
|---|---|---|
| A reading the Blue Book does not settle: it is silent, says it only in notes or informative annexes, or two Blue Books disagree. Any departure from a Blue Book | A reading the normative text settles: answer the OI "from <clause>", write it into the standard document, and say so in the PR | The same as the coordinator, for the clauses of its work package |

Instead of asking: add or update the OI with the clauses, the options, and a recommendation (roadmap §6 rule 8), and continue as §5 says. Never settle it silently in code.

### 2.2 Requirements and scope

| Maintainer only | Coordinator | Agent in a work package |
|---|---|---|
| A milestone's exit table or entry gate, beyond requirements of the standards it already covers; removing or weakening a requirement; a new requirement area; moving work between milestones | A new requirement for a SHALL of a Blue Book in the milestone's scope that no requirement covers, with the next free ID, added to the exit table; wording that changes no behaviour | Proposing such a requirement in its report; renumbering after an ID clash (roadmap §6 rule 7) |

Instead of asking: write the requirement, leave it out of the exit table, and ask in the PR description.

### 2.3 Plan decisions

| Maintainer only | Coordinator | Agent in a work package |
|---|---|---|
| Accepting a milestone plan and its D-numbered decisions, or changing an accepted one; starting or closing a milestone; the order of work: which tasks start next | Drafting plans and proposing decisions with their alternatives; splitting a work package into pull requests (WP-0a, -0b, -0c); the work-package status rows; starting the next task of the order the maintainer last gave, once its dependencies have merged | How to build what an accepted decision says: module layout, private helpers, test organisation |

Instead of asking: the plan PR lists each decision as "Proposed", and no work package that depends on it starts.

### 2.4 API and ABI shape

| Maintainer only | Coordinator | Agent in a work package |
|---|---|---|
| C ABI layout and conventions: the sign of status codes, struct versioning, ownership; a public Rust item that departs from the plan or the profile; whether a public enum is closed (GEN-081); new error kinds; merging or splitting field codes | Shapes an accepted decision or a profile already fixes; public names that follow the profile's conventions | Private items; public items in the shape a decision fixes; a new field code for a Blue Book field that has none (one code per field, M2 D2); which catalogued error kind an error is (a judgement call) |

Instead of asking: build the recommended shape, and put the question in the PR description under "For the maintainer"; the PR is not readied until it is answered.

### 2.5 Test vectors

| Maintainer only | Coordinator | Agent in a work package |
|---|---|---|
| The vector format, schema, or provenance kinds; what the loaders of every implementation accept or reject; a released case that is wrong and that TV-021 gives no way to correct | Adding cases; organising files not yet released, including moving a case to the file whose name describes it; superseding a released case with a new one (TV-021) | The same as the coordinator, for its work package's files |

A file name is a promise about its contents: a case that does not fit its file's name moves to one that does, if no release holds it. Instead of asking: never edit a released case; add a new one and mark the old `superseded`.

### 2.6 Review findings

| Maintainer only | Coordinator | Agent in a work package |
|---|---|---|
| A finding whose fix changes behaviour the specification defines, a requirement, the public API or C ABI, or a released vector; rejecting a finding the reviewer still holds after the reply | Whether a finding is blocking; fixing a finding that changes none of those and re-verifying; moving a non-blocking finding to a follow-up issue named in the reply (mandatory after the third review, below); the point-by-point reply | Fixing such findings on its own branch |

**Blocking** is defined in [review.md](review.md#reporting-findings). Instead of asking: fix or defer, and say which and why in the reply. Standing maintainer decision (2026-11-08): the coordinator reads a fix round instead of dispatching a fresh review when it changes only what a list of allowed changes names (code comments and a few Markdown files); the conditions are in [coordinator.md](coordinator.md#a-comments-only-fix-round). A finding of an architecture review (a design review outside a pull request) that is not fixed in the same pull request is opened as a GitHub issue by the coordinator, or by the reviewing agent when the coordinator asks; a finding that exists only in chat is lost.

Review rounds (maintainer, 2026-11-08): a hard cap of three reviews by `reviewer` per pull request; after the third, the coordinator opens the remaining non-blocking findings and nits as follow-up issues, and a blocking finding left then goes to the maintainer as one question ([coordinator.md](coordinator.md), "At most three reviews").

### 2.7 Process and tooling

| Maintainer only | Coordinator | Agent in a work package |
|---|---|---|
| The merge gate and the verification policy (VER-056…VER-060); these decision rights; the non-negotiable rules of AGENTS.md; the toolchain pin (RS-001, OI-07); the lint set (RS-016, RS-019); standing policies for agents (cleanup, review watch); how often approvals happen; keeping or dropping a restructure that splits a crate or moves a directory | A tooling fix, or a new check that enforces a rule already written, that loosens no gate; a check's declared inputs; documentation that restates existing rules | The registrations its work package needs (the checklists); fixing a check that is wrong for its change (a judgement call) |

Instead of asking: build it on a branch, measure what it changes, and propose it with the measurement.

### 2.8 Dependencies and licences

| Maintainer only | Coordinator | Agent in a work package |
|---|---|---|
| A new third-party dependency or tool that no accepted decision names; a licence that is not plainly open source, or a free licence for public repositories only (VER-055, OI-22); reading code under a restrictive licence | Checking a dependency an accepted decision names against VER-055 and recording it; a version bump of an existing dependency under the same licence | None: it uses what is there, and asks the coordinator |

Layers 0–2 take no third-party runtime dependency at all (GEN-001); that is a rule, not a decision. Instead of asking: name the tool, its licence, and what the requirement needs, in the plan's Decisions table.

### 2.9 Releases, tags, visibility, and anything public or external

| Maintainer only | Coordinator | Agent in a work package |
|---|---|---|
| Tags and releases; version numbers a consumer reads; publishing to crates.io or a registry, reserving names (OI-02); the visibility of the repository and its packages; repository settings, branch protection, and secrets; anything posted outside this repository | Preparing the release PR (`cargo xtask changelog release`, the version bump) and asking for the tag | None |

Instead of asking: prepare everything up to the external step, and ask for that step only.

### 2.10 Merges to `main`

| Maintainer only | Coordinator | Agent in a work package |
|---|---|---|
| Every merge into `main`, and any push to it | Opening pull requests, merging `main` into them, publishing the verification record (VER-058), marking them ready, and saying which are ready | Commits on its own branch; a push when the coordinator asks for one |

GitHub records merges. A merge that also decides something, such as merging over an open question, says so in the pull request description.

Merge trains, train assignment, and the cap of 2 concurrent subagents (maintainer, 2026-11-07 and 2026-11-09): [coordinator.md](coordinator.md#merge-trains), [Dispatch](coordinator.md#dispatch).

### 2.11 Repository housekeeping

| Maintainer only | Coordinator | Agent in a work package |
|---|---|---|
| Deleting work that is on no remote and not merged, or a branch kept on purpose; rewriting pushed history | Merging `main` into open pull requests and re-verifying (with several coordinators, see [coordinator.md](coordinator.md#multiple-coordinators)); verifying and rebuilding a merge train the maintainer assigned ([coordinator.md](coordinator.md#merge-trains)); deleting the worktrees and branches of merged pull requests under the cleanup policy; closing issues a merge finished; keeping the review watch running | Its own worktree |

Build directories (maintainer, 2026-11-07): an agent runs `cargo clean` in its worktree, and in the separate workspaces it built, after the final push of each round ([agent-brief.md](agent-brief.md) item 5).

### 2.12 Approvals

| Maintainer only | Coordinator | Agent in a work package |
|---|---|---|
| Accepting a milestone exit | Recording the acceptance in the milestone plan | — |


## 3. Asking the maintainer

Ask one question per decision, numbered, in this shape:

> **Q2 (`api`, #72). Which sign do the C ABI status codes take?**
> **Recommended:** negative, with 0 for success, as CABI-020 has it.
> **Alternatives:** positive codes, which would change CABI-020 and the generated header.
> **Meanwhile:** the PR implements the recommendation and is not readied; nothing else waits.

- Put the recommended option first, with its reason in a line; then the alternatives and what each costs; then what continues meanwhile (§5).
- **Batch** the questions that block nothing: ask them together when a pull request is ready, or when a round of agents has reported, as one numbered list.
- **Ask at once** only when work stops without the answer and nothing else can go ahead, or before an external or irreversible step (§2.9).
- Never ask about what §2 gives the coordinator or an agent, or what §4 says needs no asking. When unsure, decide under the narrower right, and record it as a judgement call.
- When the maintainer answers, the coordinator records the answer where §1 says, in the same pull request when there is one.

## 4. What never needs asking

Anything a rule or a check already decides:

- the checks: `cargo xtask ci`, `cargo xtask verify`, Clippy, the coverage gate, and the other gates of AGENTS.md. A failing check is fixed, never argued;
- the rules of AGENTS.md: the Blue Book governs, nothing is copied from it, no protocol value from memory, open issues are never resolved silently, released vectors are never edited, the conventional-commit PR title, the next free ID, the branch prefixes, one track per pull request;
- the definitions of done in the [checklists](../milestones/checklists.md);
- a Blue Book reading the normative text settles (§2.1);
- whether to verify locally or on a GitHub-hosted runner (VER-060), and re-verifying after merging `main`;
- anything §2 gives to the coordinator or an agent.

## 5. While a question waits

- Work that does not depend on the answer continues: other work packages, and the parts of this one the answer does not touch.
- A pull request that depends on an open question may implement the recommended option, provided its description and the OI say so. It is not readied for merge until the answer is recorded, and is changed if the answer differs.
- A plan decision that is not accepted starts no work that depends on it.
- An external or irreversible step (§2.9) waits; everything before it is prepared.
- Pull requests stay open and up to date; none merges without the maintainer (§2.10).
- No answer is never a yes. Questions still open are asked again in the next batch.

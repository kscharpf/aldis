# Decision rights

Who may decide what on Aldis, so that routine questions don't wait for the maintainer, and the questions that do reach the maintainer arrive with an answer ready to accept. Requirement: VER-062 ([06 §9](../06-verification.md#9-decision-rights)). This document is itself the maintainer's decision; changing it is a `process` decision (§2.7).

## 1. Roles and where each records its decisions

| Role | Who | Records a decision in |
|---|---|---|
| **Maintainer** | The project owner | The [decision log](decisions.md), one line per decision, appended by the coordinator; and wherever the rules below say (the OI, the plan's Decisions table, the requirement, the PR's "Maintainer decisions" section) |
| **Coordinator** | The Claude session that plans the work, dispatches agents, opens and readies pull requests, and answers reviews | The PR description, under **Coordinator decisions**, or its reply to the review |
| **Agent** | A session dispatched for one work package or task, in its own worktree and branch | Its report, under **Judgement calls**; the coordinator copies them into the PR description |

A contributor who is neither coordinator nor agent decides as an agent for their own pull request and asks the maintainer as §3 says.

The rules of [AGENTS.md](../../AGENTS.md), the [checklists](../milestones/checklists.md), and the checks of `cargo xtask` decide most questions already; nobody asks about those (§4). What follows covers what they leave open.

## 2. Who decides what

Each category has a slug, used in the [decision log](decisions.md). "Instead of asking" is what the role that may not decide does.

### 2.1 Spec interpretation (`spec`)

| Maintainer only | Coordinator | Agent in a work package |
|---|---|---|
| A reading the Blue Book does not settle: it is silent, says it only in notes or informative annexes, or two Blue Books disagree. Any departure from a Blue Book | A reading the normative text settles: answer the OI "from <clause>", write it into the standard document, and say so in the PR | The same as the coordinator, for the clauses of its work package |

Example: OI-35 items 1 and 8 (notes only; 231.0 against 732.1) and AOS-018's limits went to the maintainer; items 2, 3, and 5–7 were answered from the text in the same PR (#61). Instead of asking: add or update the OI with the clauses, the options, and a recommendation (roadmap §6 rule 8), and continue as §5 says. Never settle it silently in code.

### 2.2 Requirements and scope (`scope`)

| Maintainer only | Coordinator | Agent in a work package |
|---|---|---|
| A milestone's exit table or entry gate, beyond requirements of the standards it already covers; removing or weakening a requirement; a new requirement area; moving work between milestones | A new requirement for a SHALL of a Blue Book in the milestone's scope that no requirement covers, with the next free ID, added to the exit table (USLP-016, #61); wording that changes no behaviour | Proposing such a requirement in its report; renumbering after an ID clash (roadmap §6 rule 7) |

Example: adding TV-023 and VER-061 to the M2 exit table (#74) was the maintainer's: neither comes from a frame standard. Instead of asking: write the requirement, leave it out of the exit table, and ask in the PR description.

### 2.3 Plan decisions (`plan`)

| Maintainer only | Coordinator | Agent in a work package |
|---|---|---|
| Accepting a milestone plan and its D-numbered decisions, or changing an accepted one; starting or closing a milestone; the order of work: which tasks start next | Drafting plans and proposing decisions with their alternatives; splitting a work package into pull requests (WP-0a, -0b, -0c); the work-package status rows; starting the next task of the order the maintainer last gave, once its dependencies have merged | How to build what an accepted decision says: module layout, private helpers, test organisation |

Example: M2 decisions D1–D17 (#56). Instead of asking: the plan PR lists each decision as "Proposed", and no work package that depends on it starts.

### 2.4 API and ABI shape (`api`)

| Maintainer only | Coordinator | Agent in a work package |
|---|---|---|
| C ABI layout and conventions: the sign of status codes, struct versioning, ownership; a public Rust item that departs from the plan or the profile; whether a public enum is closed (GEN-081); new error kinds; merging or splitting field codes | Shapes an accepted decision or a profile already fixes; public names that follow the profile's conventions | Private items; public items in the shape a decision fixes; a new field code for a Blue Book field that has none (one code per field, M2 D2); which catalogued error kind an error is (a judgement call) |

Example: the negative status codes and `struct_size` first in `aldis_error_detail_t` (#72); `Ocf` closed (#68); `packet_len` kept as one code (#60). Instead of asking: build the recommended shape, and put the question in the PR description under "For the maintainer"; the PR is not readied until it is answered.

### 2.5 Test vectors (`vectors`)

| Maintainer only | Coordinator | Agent in a work package |
|---|---|---|
| The vector format, schema, or provenance kinds; what the loaders of every implementation accept or reject; a released case that is wrong and that TV-021 gives no way to correct | Adding cases; organising files not yet released, including moving a case to the file whose name describes it; superseding a released case with a new one (TV-021) | The same as the coordinator, for its work package's files |

A file name is a promise about its contents: a case that does not fit its file's name moves to one that does, if no release holds it. Example: the C++ loader stays stricter than the Rust runner (#66, follow-up #67) was the maintainer's; moving the FHEC miscorrection case to `fhec_miscorrection.json` (#64) is routine under this rule. Instead of asking: never edit a released case; add a new one and mark the old `superseded`.

### 2.6 Review findings (`review`)

| Maintainer only | Coordinator | Agent in a work package |
|---|---|---|
| A finding whose fix changes behaviour the specification defines, a requirement, the public API or C ABI, or a released vector; rejecting a finding the reviewer still holds after the reply | Whether a finding is blocking; fixing a finding that changes none of those and re-verifying; moving a non-blocking finding to a follow-up issue named in the reply; the point-by-point reply | Fixing such findings on its own branch |

**Blocking** means a failing check, a broken rule of AGENTS.md, or a defect against a requirement or the Blue Book; anything else is non-blocking. Fixing code to meet the existing specification is not a behaviour change. Example: the OID and CRC check fix in `cpp/vectors/` (e73db7d, #66), and the follow-up issues #65, #67, #69, are routine. Instead of asking: fix or defer, and say which and why in the reply.

### 2.7 Process and tooling (`process`)

| Maintainer only | Coordinator | Agent in a work package |
|---|---|---|
| The merge gate and the verification policy (VER-056…VER-061); these decision rights; the non-negotiable rules of AGENTS.md; the toolchain pin (RS-001, OI-07); the lint set (RS-016, RS-019); standing policies for agents (cleanup, review watch); how often approvals happen; keeping or dropping a restructure that splits a crate or moves a directory | A tooling fix, or a new check that enforces a rule already written, that loosens no gate; a check's declared inputs; documentation that restates existing rules | The registrations its work package needs (the checklists); fixing a check that is wrong for its change (a judgement call) |

Example: inspection approval once per milestone (VER-061, #74), the cleanup and review-watch policies, and holding the `xtask` split (#75). Instead of asking: build it on a branch, measure what it changes, and propose it with the measurement.

### 2.8 Dependencies and licences (`deps`)

| Maintainer only | Coordinator | Agent in a work package |
|---|---|---|
| A new third-party dependency or tool that no accepted decision names; a licence that is not plainly open source, or a free licence for public repositories only (VER-055, OI-22); reading code under a restrictive licence | Checking a dependency an accepted decision names against VER-055 and recording it (`cbindgen` 0.29.4, #72); a version bump of an existing dependency under the same licence | None: it uses what is there, and asks the coordinator |

Layers 0–2 take no third-party runtime dependency at all (GEN-001); that is a rule, not a decision. Instead of asking: name the tool, its licence, and what the requirement needs, in the plan's Decisions table.

### 2.9 Releases, tags, visibility, and anything public or external (`release`)

| Maintainer only | Coordinator | Agent in a work package |
|---|---|---|
| Tags and releases; version numbers a consumer reads; publishing to crates.io or a registry, reserving names (OI-02); the visibility of the repository and its packages; repository settings, branch protection, and secrets; anything posted outside this repository | Preparing the release PR (`cargo xtask changelog release`, the version bump) and asking for the tag | None |

Example: the tag `v0.2.0-m1` (#54), the repository made public (#57), the GHCR package `aldis-verify` made public (#63). Instead of asking: prepare everything up to the external step, and ask for that step only.

### 2.10 Merges to `main` (`merge`)

| Maintainer only | Coordinator | Agent in a work package |
|---|---|---|
| Every merge into `main`, and any push to it | Opening pull requests, merging `main` into them, publishing the verification record (VER-058), marking them ready, and saying which are ready | Commits on its own branch; a push when the coordinator asks for one |

Merges are not logged: GitHub records them. A merge that also decides something, such as merging over an open question, is logged under that question's category.

### 2.11 Repository housekeeping (`housekeeping`)

| Maintainer only | Coordinator | Agent in a work package |
|---|---|---|
| Deleting work that is on no remote and not merged, or a branch kept on purpose (the `xtask` split, #75); rewriting pushed history | Merging `main` into open pull requests and re-verifying; deleting the worktrees and branches of merged pull requests under the cleanup policy; closing issues a merge finished; keeping the review watch running | Its own worktree |

### 2.12 Approvals (`approval`)

| Maintainer only | Coordinator | Agent in a work package |
|---|---|---|
| Approving inspection records (VER-061), once per milestone at the exit review or before a release; accepting a milestone exit | Listing the pending revisions for the exit review; writing the approval into each `Approved` row once given | Preparing a revision, "pending exit review"; never editing an `Approved` row |

Example: the GEN-022/023 approvals of 2026-10-20 and 2026-10-21 (#54, #60, #64, #68), which VER-061 now collects into the exit review. Instead of asking: leave the revision pending; no pull request waits for an approval.

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
- When the maintainer answers, the coordinator records the answer where §1 says, in the same pull request when there is one, and appends the line to the [decision log](decisions.md).

## 4. What never needs asking

Anything a rule or a check already decides:

- the checks: `cargo xtask ci`, `cargo xtask verify`, Clippy, the coverage gate, and the other gates of AGENTS.md. A failing check is fixed, never argued;
- the rules of AGENTS.md: the Blue Book governs, nothing is copied from it, no protocol value from memory, open issues are never resolved silently, released vectors are never edited, the changelog fragment, the next free ID, the branch prefixes, one track per pull request;
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

## 6. The questions of 2026-10-20 and 2026-10-21

The coordinator asked the maintainer about each of these. Those that were the maintainer's are in the [decision log](decisions.md). These were routine, and under §2 would not have been asked:

| Question | Category | Decided under these rights by | Rule |
|---|---|---|---|
| Move the FHEC miscorrection case to its own file (#64) | `vectors` | Agent | A file name is a promise about its contents; the case was not released (§2.5) |
| Fix the OID and CRC agreement checks of `cpp/vectors/` (e73db7d, #66) | `review` | Coordinator | A finding that changes no behaviour, spec, or ABI (§2.6) |
| Defer non-blocking review items to follow-up issues (#65, #67, #69) | `review` | Coordinator | Non-blocking findings go to a follow-up issue named in the reply (§2.6) |
| Merge `main` into the open pull requests and re-verify | `housekeeping` | Coordinator | §2.11 |
| Delete the worktrees of merged pull requests | `housekeeping` | Coordinator | The cleanup policy, itself a maintainer decision (§2.7, §2.11) |
| Fix a review finding that changes no behaviour, spec, or ABI | `review` | Coordinator or agent | §2.6 |

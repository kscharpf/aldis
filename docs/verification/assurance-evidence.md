# The assurance evidence check

What `cargo xtask assurance` checks in the [hazard analysis](hazards.md) and the [assurance case](assurance-case.md), so that it can be built without design choices. Requirements: VER-065, VER-066 ([06 §11](../06-verification.md#11-hazard-analysis-and-assurance-case)). This file specifies the check; it is not itself read by it.

## 1. What it reads

- `docs/verification/hazards.md`, the **cross-codec file**, codec `X`;
- every `.md` file of `docs/verification/hazards/`, the **codec files**: `tm.md` (codec `TM`), `tc.md` (`TC`), `aos.md` (`AOS`), `uslp.md` (`USLP`), `cop1.md` (`COP1`); any other file there is a problem;
- `docs/verification/assurance-case.md`, the **case**;
- the **prose files**, which only the references (§2) and the code-name rule (§2.5) apply to: every `.md` file of `docs/verification/records/`, `docs/verification/cross-checks.md`, and `docs/verification/formal-proofs.md`.

Together they are the **analysed files**. Dated history is not analysed, because it describes the code as it was: `docs/verification/reviews/` and `docs/milestones/`. Line numbers in messages are 1-based lines of the file.

## 2. References

### 2.1 Syntax

A **reference** is a Markdown code span (text between single backticks) whose whole content matches

```
^([a-z]+(-[a-z]+)*):([^:].*)$
```

with the first group a **kind** of §2.3 and the third the **target**. `core::fmt` is not a reference (the colon is doubled; §2.5 makes it a problem), and neither is `` `stack` `` (no colon). A code span that matches the pattern with a kind not in §2.3 is a problem in an `Evidence` cell (§3, §4) and is ignored elsewhere.

In an `Evidence` cell (the `Evidence` row of a hazard table, the first column of a claim's evidence table, the `References` column of the defeaters table), every code span must be a reference, and the cell must hold at least one; the only other content allowed is the text between references (`; ` and words), and the cell `TODO` on its own (§3.2, §4.2). Code spans are found by splitting the cell at backticks: the odd-numbered pieces are code spans. A cell with an odd number of backticks is a problem.

### 2.2 Requirement IDs

In a `Requirements` row or column, requirement IDs are written as plain text, found by `md::scan_ids` with the areas of `Spec::load`, ranges (`TC-001…008`) expanded by `Spec::expand`. Each ID, and each end of a range, must be a requirement of a requirement table, and none may be **deleted**: a requirement whose `Requirement` cell begins `Withdrawn:` ([00-introduction.md](../00-introduction.md) §5.2). A range that expands to no requirement is a problem. The cell must hold at least one ID, or be `TODO` where §3.2 allows it. Elsewhere in the analysed files, an undefined ID is already reported by `cargo xtask spec-lint`, which reads every Markdown file.

### 2.3 Kinds and how each resolves

The target is taken as written: no trimming inside the code span, no wildcard.

| Kind | Target | Resolves when | Reuse |
|---|---|---|---|
| `file` | a path relative to the repository root, with no `..`, no leading `/`, and no `#` | it is a file (not a directory) that `repo::files` lists, so not under a skipped directory such as `target/` | `repo::files`, `repo::rel` |
| `test` | `<path>::<name>`: a `.rs` path as for `file`, then `::`, then a Rust identifier | the file defines a test `<name>` (§2.4) that is **not** ignored | `rs020::tokens`, `rs020::group_end`, `rs020::test_only` (its attribute reading) |
| `nightly-test` | as `test` | the file defines a test `<name>` that **is** ignored (`#[ignore …]`), so only a nightly job runs it | as `test` |
| `proof` | as `test` | the file defines a function `<name>` whose attributes include `kani::proof` (§2.4 (a)) | as `test` |
| `vector` | a vector case ID, such as `tc-val-011` | a codec case or engine scenario of a vector file has that ID and is not superseded | `vectors::is_vector_file`, `aldis_testkit::VectorFile::from_json`, as in `trace::artefacts` |
| `superseded-vector` | as `vector` | the case exists and is superseded | as `vector` |
| `record` | a requirement ID, such as `VER-011` | `records::load` returns a record whose file is `docs/verification/records/<ID>.md` | `records::load`, `records::DIR` |
| `check` | a check name of `cargo xtask verify list`, such as `stack` | `verify::checks::all()` has a check of that name with `gating` true and `nightly` false | `verify::checks::all` |
| `nightly-check` | as `check` | the check exists and has `gating` false or `nightly` true (today `kani`, `rust-sparc-leon3`) | `verify::checks::all` |
| `ci` | a step name of `cargo xtask ci`, such as `loops` or `trace --scope m2` | it is the name of a step of `CI` in `xtask/src/main.rs` (make `CI` `pub(crate)` or move it to its own module) | `CI` |
| `job` | a job of the nightly workflow, such as `bit-flips` | `.github/workflows/nightly.yml` has, after the line `jobs:`, a line that is exactly two spaces, the name, and `:` | none: read the file as text (`consistency::matrix_targets` reads the same file for one job's matrix) |
| `fuzz` | a fuzz target, such as `tc_decode` | `fuzz/Cargo.toml` has a `[[bin]]` table whose `name = "<target>"` | `consistency::bin_targets` (make it `pub(crate)`) |
| `oi` | `OI-NN` | a row of a table of `docs/09-open-issues.md` has `OI-NN` as its first cell; its **status** is the last cell | `md::tables` |
| `gh` | `#N`, a GitHub issue or pull request | never resolved: the check runs offline. Counted and listed in the report (§5) | — |
| `hz` | a hazard ID, such as `HZ-TC-01` | an analysed file defines it (§3.1) | — |
| `ac` | a claim ID, such as `AC-4.2` | the case defines it (§4.1) | — |
| `asm` | `ASM-n` or `CTX-n` | the case's assumptions table has the row (§4.3) | — |
| `df` | `DF-n` | the case's defeaters table has the row (§4.4) | — |
| `proposal` | a proposal ID, such as `P-TC-01` | a proposals table of an analysed file has the row (§3.4) | — |
| `sym` | an `aldis_` name of the C ABI, such as `aldis_tc_stream_next` | `include/aldis/aldis.h` holds it as an identifier (token lookup: the maximal runs of letters, digits, and `_`) | — |

### 2.4 When a file defines a test

Tokenize the file with `rs020::tokens`, which removes comments and literals. A file defines a test `<name>` when any of these holds:

- **(a) A test function.** The tokens `fn <name>` are preceded, after skipping the keywords `pub`, `const`, `async`, `unsafe`, and `extern` and any visibility group `pub(…)`, by a run of outer attributes `#[…]`, one of which is `#[test]`. The test is **ignored** when an attribute of the run starts with `ignore`. For `proof:`, the run must contain `#[kani::proof]` instead of `#[test]`.
- **(b) A property test.** The tokens `fn <name>` lie inside the delimited group of an invocation `proptest ! { … }` (or with `(` or `[`). It is ignored when its attribute run contains `ignore`.
- **(c) A test made by a macro.** The file defines `macro_rules! M` whose body holds the tokens `# [ test ]`, and the file invokes `M ! (` (or `[`, `{`) with `<name>` as the first token inside the group, as `vector_test!(tc_validation_failures, …)` does in `crates/aldis-ffi/tests/vectors.rs`. It is ignored when the macro body holds the token `ignore`.

A name that the file defines more than once (in different modules) resolves when every definition agrees on whether it is ignored; otherwise the reference is a problem ("`<name>` is both ignored and not ignored in `<path>`").

### 2.5 Code names

Prose goes stale when it names code. In every analysed file, outside fenced code, a code span that is not a reference of a kind of §2.3 is a problem when it holds any of:

- a word that starts with `aldis_` (a C ABI symbol: cite it as `sym:`, or describe the function);
- a path ending in `.rs`, `.h`, or `.toml`, with or without a `:N` line or range (cite `file:`; a line or range is never stated);
- a `::` path (cite `test:` for a test; otherwise state the property).

State the property and the requirement or clause instead of the mechanism; put a mechanism that is evidence in the `Evidence` cell as `test:`, `file:`, or `sym:`. Names of other projects (`spacepackets`, Kani) are written without a code span when they carry a `::`.

### 2.5a Code names in requirements

A requirement states an obligation, in protocol or API-model terms (OI-63). In the `Requirement` cell of every requirement row outside `docs/profiles/`, a code span that is not a reference of a kind of §2.3 is a problem when it holds any of §2.5, or:

- a signature (`->`, or the keyword `fn`);
- a function or method call: a lowercase name, with dots, followed by `(`;
- a Rust type or variant: a name that starts with an upper-case letter, has a lower-case letter, and is not an error kind of the catalogue of [03-api-model.md](../03-api-model.md) §9, with or without a `<…>`, `(…)`, or `{…}` group.

A bare lowercase word (`vc_id`, `decode`, `std`) is not told apart from a field, a key, a lint, a feature, or a command, and is not a problem. The requirements of `docs/profiles/` are about the conventions of a language, and name its constructs. Name an anomaly, an action, or an input in plain words; cite a file or a C ABI symbol as `file:` or `sym:`.

## 3. The hazard files

### 3.1 Hazard sections

A hazard is a heading line matching `^### (HZ-(X|TM|TC|AOS|USLP|COP1)-[0-9]{2}) (.+)$`. Its codec must be the file's codec (§1). IDs are unique across the analysed files, and within a file their numbers strictly increase in file order (a gap is allowed: an ID is never reused).

The first table after the heading, before the next heading, is the **hazard table**. Its header row has two empty cells (`| | |`, as the records of [analysis-records.md](analysis-records.md) have) and its rows are exactly, in this order: `Category`, `Status`, `Severity`, `Failure mode`, `Cause`, `Mission effect`, `Requirements`, `Evidence`, `Residual risk`, `Assumptions`, `Proposals`, `Claims`. A missing, extra, or misplaced row is a problem naming it.

### 3.2 Hazard rows

| Row | Rule |
|---|---|
| `Category` | One code span holding a slug of [hazards.md](hazards.md) §1.2: `corrupt-accepted`, `valid-rejected`, `misrouting`, `truncation`, `wrong-value`, `nonconformant-output`, `unbounded-time`, `memory`, `panic`, `state-corruption`, `abi-misuse`, `hidden-deviation` |
| `Status` | One code span: `todo`, `controlled`, `residual`, or `gap` |
| `Severity` | `S1`, `S2`, `S3`, or `S4` |
| `Failure mode`, `Cause`, `Mission effect` | Not empty |
| `Requirements` | §2.2; `TODO` only when the status is `todo` |
| `Evidence` | An Evidence cell (§2.1) whose references all resolve; `TODO` only when the status is `todo` |
| `Residual risk` | Not empty. `—` is a problem when the status is `residual` |
| `Assumptions` | `—`, or one or more `asm:` references and nothing else but separators |
| `Proposals` | `—`, or one or more `proposal:` references. `—` is a problem when the status is `gap`; a reference is a problem when the status is `controlled`. When the status is `gap`, at least one proposal it names must have the status `proposed`: a gap whose proposals are all decided must be re-rated |
| `Claims` | One or more `ac:` references, all resolving; `TODO` only when the status is `todo` |

References may also appear in the other cells (for example `hz:` in a residual) and must then resolve.

### 3.3 Coverage tables

Each codec file (not the cross-codec file) has a heading `## Coverage` followed by a table with the header `| Category | Hazards | State |` and exactly twelve rows, one per slug of §3.2 in that order, the slug in a code span.

- `Hazards` is `—` or a list of hazard IDs separated by `, `, each defined in this file with this category. Every hazard of the file must be listed in the row of its category.
- `State` is `done`, `TODO`, or `n/a: ` followed by a reason that is not empty. `done` needs at least one hazard in the row and no hazard of the row with the status `todo`. `n/a` needs `—`.

### 3.4 Proposals tables

Every hazard file has a heading `## Proposals` followed by a table with the header `| ID | Kind | Proposal | Status |`, which may have no rows.

- `ID` matches `^P-(X|TM|TC|AOS|USLP|COP1)-[0-9]{2}$` with the file's codec; unique across the analysed files.
- `Kind` is one code span: `requirement`, `oi`, `test`, `doc`, or `tooling`.
- `Proposal` is not empty.
- `Status` begins with one code span, `proposed`, `accepted`, or `rejected`; after `accepted` or `rejected` the cell must hold more text (where it was written, or why).
- Every proposal is named by the `Proposals` row of at least one hazard.

## 4. The case

### 4.1 Claims

A claim is a heading line matching `^(#{2,5}) (AC-[0-9]+(\.[0-9]+)*) (.+)$`. `AC-0` and the IDs of one number (`AC-1`) take `##`; an ID of k numbers (k ≥ 2) takes k + 1 `#`. IDs are unique. The **parent** of `AC-n` (n ≥ 1) is `AC-0`; the parent of `AC-n.….m` is the ID without its last number, and must be defined above it. A claim is a **leaf** when no claim names it as parent.

A claim's **section** runs from its heading to the next claim heading or the heading `## Assumptions and context`, whichever comes first.

- Every section holds a line beginning `**Claim.**` followed by text.
- A claim that is not a leaf holds a line beginning `**Strategy.**` and no evidence table.
- A leaf holds exactly one table with the header `| Evidence | Shows | Requirements |` and at least one row.

### 4.2 Evidence tables

Each row: `Evidence` is an Evidence cell (§2.1) whose references resolve, or `TODO`; `Shows` is not empty; `Requirements` follows §2.2.

### 4.3 Assumptions

The heading `## Assumptions and context` is followed by a table with the header `| ID | Assumption | Basis | Integrator obligation |`. IDs match `^(ASM|CTX)-[0-9]+$` and are unique; `Assumption` and `Basis` are not empty. Every row is named by at least one `asm:` reference in the analysed files.

### 4.4 Defeaters

The heading `## Defeaters` is followed by a table with the header `| ID | Defeater | Weakens | References | Status |`. IDs match `^DF-[0-9]+$` and are unique. `Weakens` holds one or more `ac:` references; `References` is an Evidence cell (§2.1); `Status` is one code span, `open` or `closed`. An `open` defeater whose `References` name an `oi:` whose status (§2.3) begins with `Resolved` is a problem ("DF-n is open but OI-NN is resolved"), so that the list does not outlive what it names.

### 4.5 Across the files

- Every `hz:`, `ac:`, `asm:`, `df:`, and `proposal:` reference, wherever it is, resolves.
- Every hazard whose status is not `todo` names at least one claim (§3.2), so each hazard's controls are evidence for a claim; the reverse (claims → hazards) is reported, not required (§5).

## 5. Modes, output, and joining the gate

**Command.** `cargo xtask assurance` and `cargo xtask assurance --strict`, in a new module `xtask/src/assurance.rs`, dispatched from `run` in `xtask/src/main.rs` and listed in `HELP`.

**Modes.** Without `--strict`, `TODO` is allowed where §3 and §4 allow it, and is counted, not reported as a problem: a hazard with the status `todo`, a coverage row with the state `TODO`, an evidence row `TODO`. With `--strict`, each of them is a problem. Every other rule is a problem in both modes. As `trace --scope m2` became `--strict` at the M2 exit (M2 plan D1), the strict mode joins `cargo xtask ci` and the verify check `spec` at the M2 exit, in the exit pull request.

**Output.** One line per problem, `error: <path>:<line>: <message>`, as `spec-lint` and `trace` print them, then one summary line:

```
assurance: <h> hazards (<t> todo), <c> claims (<l> leaves, <r> TODO rows), <n> references (<g> gh, unchecked), <p> problem(s)
```

It writes `target/spec/assurance.json` (an object with `hazards`, each with `id`, `file`, `line`, `category`, `status`, `severity`, `claims`, and `proposals`; `claims`, each with `id`, `parent`, `leaf`, `todo_rows`, and `hazards`, the hazards that name it; and `gh`, every `gh:` target with the file and line that names it) and `target/spec/assurance.md`, a table of every claim with the hazards that name it and its evidence references. It returns the number of problems, as the other checks do, so that `main` exits non-zero on any.

**Joining `cargo xtask ci`.** Add the step `("assurance", || assurance::run(false))` to `CI` in `xtask/src/main.rs`, after `loops`, and name it in the `ci` paragraph of `HELP` (the test `ci_checks_the_generated_files` requires every step in the help).

**Joining `cargo xtask verify`.** Add `cargo xtask assurance` to the script `SPEC` of `xtask/src/verify/checks.rs`, after `cargo xtask loops`, with the comment line `# Every reference of the hazard analysis and the assurance case resolves (VER-065, VER-066).`. The check `spec` reads every path (`Inputs::Everything`), so no input list changes: a change to a test, a vector, a record, or a check that the analysed files name runs `spec`, and a broken reference fails it on the same commit.

**Records and changelog.** VER-066 is traced by the check `spec` (`CHECK_REQUIREMENTS` in `xtask/src/trace.rs`).

## 6. What a passing check means

**Checked:**
- every reference resolves to something that exists, of the kind it says;
- a `check:` names a gating check that is not nightly-only, so that it ran (or was soundly skipped or reused) in the published record of every commit merged since the reference was written (VER-058); a `test:` names a test that is not ignored, so that the check `test` ran it on the same terms; a `vector:` names a live case, which the vector tests run (`cargo xtask consistency` requires every vector file to have a test);
- night-only evidence says so by its kind (`nightly-test:`, `nightly-check:`, `job:`, `fuzz:`, `proof:`), and a gating kind cannot name it;
- the structure of §3 and §4, and that every hazard supports a claim.

**Not checked**, left to the review of each pull request that changes the analysed files and to the milestone exit review (VER-065):
- that a piece of evidence shows what its `Shows` cell or the hazard's text says, or covers the requirement named beside it;
- that a record's text is true, or that its check still does what the record says;
- that a night-only job passed last night (a nightly failure opens an issue, VER-059);
- the state of a GitHub issue or pull request (`gh:`);
- that the analysis is complete: the coverage tables make the categories explicit, but whether a codec has failure modes that no category names is a matter of review.

## 7. Tests the implementation must have

Unit tests in `xtask/src/assurance.rs`, each on small fixture texts and a fixture `Spec` where needed, each with `// req: VER-066` (and `VER-065` for the hazard format):

1. Each kind of §2.3 resolves a target that exists and rejects one that does not.
2. `test:` resolves a `#[test] fn`, a `fn` in `proptest! { }`, and a name made by a local `macro_rules!` with `#[test]`; rejects a function without `#[test]`; rejects an ignored test cited as `test:` and a test that is not ignored cited as `nightly-test:`; reports a name defined both ignored and not.
3. `proof:` resolves a `#[kani::proof]` function and rejects a `#[test]` one.
4. `vector:` rejects a superseded case, and `superseded-vector:` a live one.
5. `check:` rejects `kani` and `rust-sparc-leon3`; `nightly-check:` rejects `stack`.
6. `stack` is not a reference; an unknown kind is a problem in an Evidence cell and ignored in a `Failure mode` cell; an odd number of backticks is a problem.
7. Requirement IDs: an undefined ID, a range with an undefined end, a range that expands to nothing, and a deleted requirement are problems.
8. Hazard tables: a missing row, rows out of order, an unknown category, status, or severity; `gap` without a proposal; `gap` whose proposals are all `accepted`; `controlled` with a proposal; `residual` with `—`; `TODO` evidence with a status other than `todo`; an ID whose codec is not the file's; a duplicate ID across files; numbers that do not increase.
9. Coverage tables: a missing or misplaced slug; `done` with no hazard; `done` with a `todo` hazard; a hazard missing from its category's row; a hazard listed under another category; `n/a:` without a reason; `n/a` with a hazard.
10. Proposals: a proposal no hazard names; an ID whose codec is not the file's; `accepted` with no further text.
11. Claims: a missing parent; a wrong heading level; a leaf without an evidence table; a non-leaf with one; a missing `**Claim.**` or `**Strategy.**`; a `TODO` row passes without `--strict` and fails with it.
12. Defeaters: an `open` defeater naming a resolved OI is a problem, a `closed` one is not; an assumption that nothing names is a problem.
13. Strict mode counts each `todo` status, `TODO` coverage state, and `TODO` evidence row as a problem, and non-strict mode counts none.
14. On the repository: `run(false)` reports no problem.
14a. Code names (§2.5, and §2.5a in the requirement rows): an `aldis_` symbol, a `.rs`, `.h`, or `.toml` path with or without a line, and a `::` path are problems in any analysed file, also in a prose file; a reference of a kind of §2.3 is not; `sym:` resolves only a name the header holds.
15. `CI` contains `assurance` and `SPEC` contains `cargo xtask assurance`.

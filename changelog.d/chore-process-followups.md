### Added
- `cargo xtask metrics`: the process overhead. Each pull request's changed paths, and those of its
  commits other than merges, are product (the code, the vectors, the specification) or process
  (tooling, workflows, plans, process and verification records, the changelog, the root files);
  the report gives the shares of pull requests, commits, and verification minutes that are
  process-only, product-only, or mixed, and the bookkeeping-only commits (only the decision log, a
  plan's status rows, or verification records). The "Recommendations input" lists the process-only
  pull requests when they take 33 % or more of the verification minutes, and each pull request
  with two or more bookkeeping-only commits (docs/06-verification.md §9, VER-063).
- `.gitattributes`: `docs/process/decisions.md` merges with `merge=union`, keeping the lines of
  both pull requests; `cargo xtask decisions` still reports a line out of date order or repeated.

### Changed
- Field codes are allocated in blocks (M2 plan D2, amended 2026-10-24): codes 1–106 stay as they
  are, each new crate gets a block of 50 when its plan is accepted (`aldis-cop1` 150–199,
  `aldis-link` 200–249), and fields shared by several crates take the next code of the common
  block 107–149. `cargo xtask fields` checks that every code lies in its owner's block
  (docs/03-api-model.md §9.1, API-082).
- `cargo xtask verify`: the workspace builds of the checks pass `--locked`, and `verify inside`
  refuses a checkout that building xtask changed, so a commit with a stale `Cargo.lock` fails
  instead of being resolved again in the container (VER-056).
- `cargo xtask verify`: a change to `.gitignore` alone runs only `spec`, as documentation does
  (VER-056).

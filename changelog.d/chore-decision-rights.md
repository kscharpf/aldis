### Added
- Decision rights, [docs/process/decision-rights.md](docs/process/decision-rights.md): who decides
  what among the maintainer, the coordinating session, and the agent of a work package, for twelve
  categories (spec readings, scope, plans, API and ABI shape, vectors, review findings, process and
  tooling, dependencies and licences, releases and anything external, merges to `main`,
  housekeeping, approvals), each with an example from 2026-10-20 and 2026-10-21 and what to do
  instead of asking; how to ask the maintainer (one question, a recommended option first, the
  alternatives, what continues meanwhile; batched unless work is blocked); what never needs
  asking; and what continues while a question waits.
- The decision log, [docs/process/decisions.md](docs/process/decisions.md): one line per
  maintainer decision, `- YYYY-MM-DD · <category> · <#N or chat> · <decision>`, appended by the
  coordinator, backfilled with the decisions of 2026-10-20 and 2026-10-21.
- New VER-062 ([docs/06-verification.md](docs/06-verification.md) §10): decisions are taken by the
  role the decision rights give them to, and every maintainer decision is logged.
  `cargo xtask decisions` checks the log's format (known category slugs, each defined in the
  decision rights; real dates in order; a `#N` or `chat` source), in `cargo xtask ci`, the verify
  check `spec`, and the nightly `ci.yml` job `spec`.

### Changed
- `AGENTS.md` has a "Decision rights" section; `docs/README.md` and `docs/glossary.md` list the
  decision rights and the decision log; the checklists and the pull request template point to
  them where an item involves a maintainer decision.

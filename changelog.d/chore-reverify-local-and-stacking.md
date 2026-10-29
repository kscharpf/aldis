### Changed
- `scripts/reverify` verifies a pull request locally, in seconds, when its merged head needs no container, as for a stacked pull request whose base has merged; other heads still go to the hosted run.
- `docs/08-roadmap.md` §6 gains a stacking rule (rule 10) and a test (`scope.rs`) shows that merging `main` into a stacked branch reuses every check (VER-056).

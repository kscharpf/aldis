### Changed
- `scripts/reverify` verifies a pull request locally, in seconds, when its merged head needs no container, as for a stacked pull request whose base has merged; other heads still go to the hosted run.
- `docs/08-roadmap.md` §6 gains a stacking rule (rule 10) and a test (`scope.rs`) shows that merging `main` into a stacked branch reuses every check (VER-056).
- `cargo xtask verify --publish` and `verify part` refuse a head that does not contain the tip of `origin/main` before running any check (`--stale-ok` runs it anyway); the first step of `verify.yml` stops such a run within seconds, and a run that finishes behind `main` ends with a `STALE` line, a `Behind main` annotation, and a job-summary headline (VER-058, VER-060).
- `scripts/reverify` labels a run that did not succeed STALE or FAILED by whether the head contains the tip of `origin/main`.

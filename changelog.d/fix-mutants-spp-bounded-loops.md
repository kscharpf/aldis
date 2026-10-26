### Fixed
- CI / `aldis-spp`: bound `Segmenter::next_packet` test loops in `segment.rs` and `vectors.rs` to prevent heap exhaustion when mutated, resolving OOM terminations in nightly mutation testing (issue #125, VER-025); set `PROPTEST_CASES: 32` in the nightly `mutants` job.

### Fixed
- CI / `aldis-spp`: bound `Segmenter::next_packet` test loops in `segment.rs` and `vectors.rs` to prevent heap exhaustion when mutated, resolving OOM terminations in nightly mutation testing (issue #125, VER-025); shard the nightly `mutants` job into a per-crate matrix with `PROPTEST_CASES: 32` to run within timeout bounds.

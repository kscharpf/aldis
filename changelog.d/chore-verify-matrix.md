### Added
- `cargo xtask verify part <group>` and `verify assemble`: the checks of a hosted verification run in four parts, assembled into one record under the unchanged merge gate (VER-067).

### Changed
- The typical durations that order a verification run are the measured durations of a hosted run, so that the quickest checks still run first.

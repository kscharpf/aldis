### Added
- A `README.md` in every crate under `crates/`: purpose and standard, status and milestone, layer
  and dependencies, features, and, for the implemented crates, an example and links to the
  verification evidence. The crate-level rustdoc stays the API reference (RS-015). The examples in
  `aldis-core`, `aldis-time`, `aldis-spp`, `aldis-epp`, and `aldis` run as doctests, and each
  crate's `Cargo.toml` names its README. The root `README.md` links them.

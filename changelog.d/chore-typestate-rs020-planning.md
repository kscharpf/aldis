### Added
- Specification: RS-021 (Rust profile). A state machine makes invalid states and transitions
  unrepresentable: typestate where the caller's call sequence decides the next state, and a closed
  enum with a transition function generated from or checked against the state table where an
  external event decides it. M2 WP-7 plans a mechanical RS-020 check and a property test for every
  restricted type.

### Added
- Verification: the hazard and failure-mode analysis (`docs/verification/hazards.md`, one file per
  frame codec in `docs/verification/hazards/`), the assurance case
  (`docs/verification/assurance-case.md`), and the specification of the check
  `cargo xtask assurance` (`docs/verification/assurance-evidence.md`), with the new requirements
  VER-065 and VER-066 (M2 WP-8). The analysis finds that `validate_frame` detects a TC or USLP
  Frame Length error only when the frame's boundary does not come from the delimiter, and that a
  bit error in the USLP End of Frame Primary Header Flag can make a frame read as a truncated
  frame, which has no FECF; both are raised as proposals.

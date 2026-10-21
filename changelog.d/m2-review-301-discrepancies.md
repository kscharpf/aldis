### Added
- `docs/verification/reviews/human-301-discrepancies.md`: a review packet for the maintainer to
  answer, from CCSDS 301.0-B-4 alone, the four readings on which `spacepackets` and Aldis differ
  (cross-checks.md §3, D1–D4), with hand exercises against the named vector cases and a record to
  fill in.
- Open issue OI-39: CUC P-field octet 2, 301.0 §3.1.1 against §3.2.2, raised by the review's Q3;
  deferred with octet 2 still accepted, and tracked in #84.
- Open issue OI-40: rounding nanoseconds to CUC fine time, raised by the review's Q4, where the
  maintainer chose to round down and TIME-011 rounds up.

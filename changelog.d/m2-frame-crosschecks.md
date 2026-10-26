### Added
- [docs/verification/cross-checks.md](docs/verification/cross-checks.md) §4.7–§4.10: evaluated seven
  independent frame candidates (`eu.dariolucia.ccsds`, Yamcs, NASA CryptoLib, `spacepackets`,
  `gr-satellites`, Wireshark dissectors, `ccsds` crate) and Orekit across TM, TC, AOS, and USLP (VER-040,
  M2 D15, WP-6). None is wired into `crosscheck/`: Java candidates require a JVM runtime (deferred under
  OI-32) or are AGPL-3.0 server frameworks; NASA CryptoLib covers SDLS (355.0) only; `spacepackets` covers
  no TM/TC/AOS and only partial unsegmented USLP full frames without insert zone, SDLS, truncated frames, or
  delimiting/validation; deframers (`gr-satellites`, `ccsds` crate) lack frame header codecs; upstream
  Wireshark has no frame dissectors. Frame vectors keep `hand-derived` provenance, verified by written
  derivations, blind Python models (TV-023), and the approved second review ([m2-vectors.md](docs/verification/reviews/m2-vectors.md)).
- [docs/09-open-issues.md](docs/09-open-issues.md) OI-32: updated with the M2 frame candidate evaluation
  findings and status `Open (JVM harness)`.

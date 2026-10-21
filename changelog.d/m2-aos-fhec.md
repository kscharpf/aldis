### Added
- `aldis-aos`: the AOS Frame Header Error Control (732.0-B-5 EC1 §4.1.2.6.5, the same code as
  732.0-B-4; AOS-013, AOS-014; M2 plan D10). `fhec::encode` and `fhec::write` compute the four
  check symbols by division by g(x) over GF(2⁴); `fhec::decode` returns a `CorrectedHeader` with up
  to two corrected symbols, reported by `corrected_symbols()`, or `HeaderUncorrectable`. The
  decoder finds the error locator in closed form, searches only the ten transmitted positions, and
  takes the error values from Forney's formula.
- `aldis-testkit`: `oracle::fhec`, an independent table model of the FHEC built from every pattern
  of at most two symbol errors; the decoder agrees with it on all 65 536 syndromes (unit test) and
  in the new fuzz target `aos_fhec_model`, which joins the nightly fuzz matrix.
- Vectors `test-vectors/aos/{fhec_encode,fhec_correct_1_symbol,fhec_correct_2_symbols,fhec_uncorrectable}.json`,
  with one encoding and one correction worked by hand in
  `docs/verification/derivations/aos-fhec.md`; the exerciser runs them on the three bare-metal
  targets. `aldis-aos` joins the coverage gate and the nightly mutation testing, and has a
  GEN-022/023 inspection record.

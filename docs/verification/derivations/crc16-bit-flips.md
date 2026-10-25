# Derivation: what the CRC-16 FECF detects, and the bit-flip suites

The bound behind the bit-flip completeness suites of the codecs with an FECF (VER-064; [M2 plan](../../milestones/m2-frame-codecs.md) WP-7, "Bit-flip completeness"): which single- and double-bit errors the FECF is certain to detect, and up to which frame length. Prepared 2026-10-24 on `m2/wp7-tests`.

## 1. The code

The FECF of TM, TC, AOS, and USLP is the same CRC-16: the generating polynomial G(X) = X¹⁶ + X¹² + X⁵ + 1, the shift register preset to all ones, the 16 check bits appended (132.0-B-3 §4.1.6.2.2, 232.0-B-4 §4.1.4.2.2, 732.0-B-5 §4.1.6.2.2, 732.1-B-3 Annex B1.1.3; CMN-001). A truncated USLP frame has no FECF (732.1 Annex D1.2.2 note 1). None of the four books states which errors the code is certain to detect, so the bound is derived here.

## 2. The bound

1. The decoding syndrome is S(X) = [X¹⁶·C*(X) + Xⁿ·L(X)] mod G(X), with C*(X) = C(X) + E(X) (132.0 §4.1.6.3; the same in the other books). The syndrome of a valid codeword is zero and the map is affine, so S(X) = X¹⁶·E(X) mod G(X). G(X) has a constant term, so it is prime to X: an error is **undetected exactly when G(X) divides E(X)**.
2. G(1) = 1 + 1 + 1 + 1 = 0, so X + 1 divides G(X), and G(X) divides no polynomial of odd weight: **every error of an odd number of bits is detected, in a block of any length**. In particular every single-bit error.
3. A double-bit error at bits i < j is E(X) = Xⁱ·(X^(j−i) + 1). It is undetected exactly when G(X) divides X^d + 1, d = j − i, that is when the order e of X modulo G(X) divides d.
4. G(X) = (X + 1)·P(X) with P(X) = X¹⁵ + X¹⁴ + X¹³ + X¹² + X⁴ + X³ + X² + X + 1, and the order of X modulo P(X) is 2¹⁵ − 1 = 32 767, so P(X) is primitive and e = lcm(1, 32 767) = **32 767**.
5. So **every double-bit error is detected in a block of at most 32 767 bits**, FECF included, and in no longer block: two errors 32 767 bits apart are not. In whole octets the longest such frame has **4 095 octets** (32 760 bits).

| Standard | Longest frame | Every double-bit error detected |
|---|---|---|
| TM | 2 048 octets (131.0-B-6 §11.3) | yes, every frame |
| TC | 1 024 octets (232.0 §4.1.2.7.3) | yes, every frame |
| AOS | 65 536 octets (131.0 §11.3) | frames of at most 4 095 octets |
| USLP | 65 536 octets (732.1 §4.1.2.7, 131.0 §11.3) | frames of at most 4 095 octets |

## 3. How the numbers were computed

Twice, by programs that share no code with `aldis-core`'s CRC, over GF(2) polynomials held as integers:

- First, a separate Python program (below, run with Python 3 on 2026-10-24). It is not kept in the repository; its output is.
- Then `aldis_testkit::bitflip` (`crates/aldis-testkit/src/bitflip.rs`), whose test `the_double_bit_bound_is_the_order_of_x` recomputes the order of X modulo G(X) and modulo P(X), the divisibility by X + 1, and the octet bound, and fails if any differs.

```python
G = (1 << 16) | (1 << 12) | (1 << 5) | 1

def mod(a, m):
    dm = m.bit_length() - 1
    while a.bit_length() - 1 >= dm:
        a ^= m << (a.bit_length() - 1 - dm)
    return a

x, e = 2, 1                      # order of X modulo G
while x != 1:
    x, e = mod(x << 1, G), e + 1
d = 1                            # smallest d with G | X^d + 1, by brute force
while mod((1 << d) | 1, G) != 0:
    d += 1
print(e, d, e // 8)
```

Output: `32767 32767 4095`. The quotient G(X)/(X + 1) was computed by the same long division (`0b1111000000011111`, remainder 0), with order 32 767.

## 4. The suites

`crates/aldis-{tm,tc,aos,uslp}/tests/bit_flips.rs` flip bits of fixed valid frames, built by each crate's encoder, and check the decoder's outcome for every error they enumerate. Each file's module documentation states the outcome per region of the frame:

- **TM:** every error is `CrcMismatch`, in both modes: the FECF is checked before any field (TM-005), and the frame length is fixed.
- **TC and USLP:** every error is rejected by the Frame Validation Checks, `validate_frame` (TC-021, USLP-014). An error in the Frame Length field (TC bits 22–31, 232.0 §4.1.2.7.1; USLP bits 32–47, 732.1 §4.1.2.7.1) changes the length the frame claims; `validate_frame` compares it with the octets present (check (d)), but `decode`, which reads a frame from the start of a longer input (API-011), then checks the FECF of a shorter frame, which the bound of §2 does not cover. For those errors the suites assert only that `decode` never returns a frame of the input's whole length; the TC suite builds a frame for which `decode` does return the shorter frame.
- **AOS:** with the FHEC, an error only in the FHEC's symbols (header bits 0–15 and 40–63, AOS-014) is corrected and the frame decodes to exactly the frame sent, with `corrected_symbols()` 1 or 2 (two bit errors touch at most two symbols, AOS-013); every other error is `CrcMismatch`, the FECF being checked over the corrected header (732.0 §4.3.7.2–3).
- **AOS and USLP above 4 095 octets:** a test flips two data bits 32 767 apart in a 65 536-octet frame and shows it decoded as a different frame, so the suites assert no more than §2 gives.

Gated (every `cargo test`): every single- and double-bit error of the short frames (up to 64 octets), every single-bit error of the longest TM and TC frames, and a representative set of the long frames' errors (`bitflip::sampled_bits`, `bitflip::sampled_pairs`: the header, the FECF, the edges, a stride, and the 32 767-bit separation). The four suites take about 4 s with the table CRC and about 20 s with the bitwise CRC, in a debug build.

Nightly (`#[ignore]`, the job `bit-flips` of `.github/workflows/nightly.yml`, release mode, on every core): every double-bit error of the longest TM frame (134 million) and TC frame (33.5 million), every single-bit error of the 4 095- and 65 536-octet AOS and USLP frames, and every double-bit error of their 4 095-octet frames (537 million each).

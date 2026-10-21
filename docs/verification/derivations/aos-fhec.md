# Derivation: AOS Frame Header Error Control vectors

The worked derivation that [07-test-vectors.md](../../07-test-vectors.md) TV-020 asks for a `hand-derived` vector file, for `test-vectors/aos/fhec_*.json` (AOS-013, AOS-014; [M2 plan](../../milestones/m2-frame-codecs.md) D10, WP-4). 732.0 prints no worked FHEC example, and no independent implementation has been run against these vectors yet (D15), so one encoding and one correction are worked here by hand, step by step. The other cases' values come from the independent Python model the provenance notes describe; this derivation checks that model on two cases.

## 1. The code

All parameters are from 732.0-B-5 EC1 §4.1.2.6.5 (and, identically, 732.0-B-4 §4.1.2.6.5):

- J = 4 bits per symbol (a); E = 2 correctable symbols (b).
- Field generator F(x) = x⁴ + x + 1 (c). With F(α) = 0, α⁴ = α + 1. A symbol `b₀b₁b₂b₃` is b₀α³ + b₁α² + b₂α + b₃, so α³ = `1000` (d, e).
- Code generator g(x) = (x + α⁶)(x + α⁷)(x + α⁸)(x + α⁹) = x⁴ + α³x³ + αx² + α³x + 1 (d).
- Five symbols of zero virtual fill at the start, not transmitted (f); systematic (g).
- Symbols 6–9 are header bits 0–15, symbols 10–11 bits 40–47, symbols 12–15 the FHEC, bits 48–63 (h). Bit 0 is the most significant bit of octet 0 (§1.6.3), and a symbol is sent from its left bit (e), so each symbol is one nibble of octets 0, 1, 5, 6, 7, upper nibble first.

The powers of α, from α^(i+1) = α·α^i reduced with α⁴ = α + 1:

| i | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12 | 13 | 14 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| α^i | 0001 | 0010 | 0100 | 1000 | 0011 | 0110 | 1100 | 1011 | 0101 | 1010 | 0111 | 1110 | 1111 | 1101 | 1001 |

The rows for α⁰, α, α³, α⁶…α⁹ are the values §4.1.2.6.5 d prints. Products are α^(i+j mod 15); sums are XOR.

Symbol k is the coefficient of x^(15−k). §4.1.2.6.5 does not state the order of the coefficients, but g(x) is its own reciprocal, so the set of codewords is the same either way, and with the fill and the systematic symbols fixed the parity symbols are unique. The encoder's output does not depend on the choice.

## 2. Encoding `aos-fhec-enc-002`: header `40 01 00 00 00 00`

Octets 0, 1, and 5 are `40`, `01`, `00`: Transfer Frame Version Number `01`, Spacecraft ID 0, Virtual Channel ID 1, Signaling Field and cycle 0. The systematic symbols 6–11 are `0100 0000 0000 0001 0000 0000` = α², 0, 0, 1, 0, 0, so

m(x) = α²x⁵ + x², and the parity is the remainder of m(x)·x⁴ = α²x⁹ + x⁶ divided by g(x).

Division by g(x) as a shift register: the remainder r₃x³ + r₂x² + r₁x + r₀ starts at zero; for each systematic symbol m, highest first, the feedback is f = m + r₃, and then r₃ ← r₂ + α³f, r₂ ← r₁ + αf, r₁ ← r₀ + α³f, r₀ ← f.

| Symbol | m | f = m + r₃ | α³f | αf | new r₃ = r₂ + α³f | new r₂ = r₁ + αf | new r₁ = r₀ + α³f | new r₀ = f |
|---|---|---|---|---|---|---|---|---|
| 6 | α² | α² | α⁵ = 0110 | α³ = 1000 | 0 + 0110 = α⁵ | 0 + 1000 = α³ | 0 + 0110 = α⁵ | α² = 0100 |
| 7 | 0 | α⁵ | α⁸ = 0101 | α⁶ = 1100 | 1000 + 0101 = 1101 = α¹³ | 0110 + 1100 = 1010 = α⁹ | 0100 + 0101 = 0001 = 1 | α⁵ = 0110 |
| 8 | 0 | α¹³ | α¹⁶ = α = 0010 | α¹⁴ = 1001 | 1010 + 0010 = 1000 = α³ | 0001 + 1001 = 1000 = α³ | 0110 + 0010 = 0100 = α² | α¹³ = 1101 |
| 9 | 1 | 1 + α³ = 1001 = α¹⁴ | α¹⁷ = α² = 0100 | α¹⁵ = 1 = 0001 | 1000 + 0100 = 1100 = α⁶ | 0100 + 0001 = 0101 = α⁸ | 1101 + 0100 = 1001 = α¹⁴ | α¹⁴ = 1001 |
| 10 | 0 | α⁶ | α⁹ = 1010 | α⁷ = 1011 | 0101 + 1010 = 1111 = α¹² | 1001 + 1011 = 0010 = α | 1001 + 1010 = 0011 = α⁴ | α⁶ = 1100 |
| 11 | 0 | α¹² | α¹⁵ = 1 = 0001 | α¹³ = 1101 | 0010 + 0001 = 0011 = α⁴ | 0011 + 1101 = 1110 = α¹¹ | 1100 + 0001 = 1101 = α¹³ | α¹² = 1111 |

The remainder is α⁴x³ + α¹¹x² + α¹³x + α¹², so symbols 12–15 are `0011 1110 1101 1111` and the FHEC is **`3EDF`**: the header is `40 01 00 00 00 00 3E DF`, as `aos-fhec-enc-002` expects.

**Check.** The codeword is c(x) = α²x⁹ + x⁶ + α⁴x³ + α¹¹x² + α¹³x + α¹². At x = α⁶: α²·α⁵⁴ = α⁵⁶ = α¹¹; α³⁶ = α⁶; α⁴·α¹⁸ = α²² = α⁷; α¹¹·α¹² = α²³ = α⁸; α¹³·α⁶ = α¹⁹ = α⁴; α¹². The sum 1110 + 1100 + 1011 + 0101 + 0011 + 1111 = 0000, so α⁶ is a root, as it must be.

## 3. Correcting `aos-fhec-c1-001`: header `50 01 00 00 00 00 3E DF`

This is the codeword of §2 with the error `0001` in symbol 6 (`4` received as `5`). Symbol 6 is the coefficient of x⁹, so the error is e(x) = x⁹: location X = α⁹, value Y = 1. The syndromes of the received word are those of the error, S_j = Y·X^j = α^(9j mod 15):

S₆ = α⁵⁴ = α⁹, S₇ = α⁶³ = α³, S₈ = α⁷² = α¹², S₉ = α⁸¹ = α⁶.

The decoder of `aldis_aos::fhec::decode`:

1. Not all syndromes are zero, so the header is not a codeword.
2. The two-error determinant S₇² + S₆S₈ = α⁶ + α²¹ = α⁶ + α⁶ = 0, so at most one error is possible.
3. Λ₁ = S₇/S₆ = α^(3−9) = α⁹, and the consistency check S₉ = Λ₁·S₈ holds: α⁹·α¹² = α²¹ = α⁶. The locator is Λ(x) = 1 + α⁹x.
4. Root search over the transmitted positions p = 0…9, z = α^(−p): Λ(z) = 0 at z = α⁻⁹ = α⁶, that is p = 9, symbol 15 − 9 = 6. No other transmitted position is a root, and the degree is 1, so exactly one root is needed.
5. Forney: Ω(x) = S₆ + (S₇ + S₆Λ₁)x = α⁹ + (α³ + α¹⁸)x = α⁹, since α¹⁸ = α³. The value is Y = z⁵·Ω(z)/Λ₁ = α³⁰·α⁹/α⁹ = 1.
6. Symbol 6 becomes `0101` + `0001` = `0100`: the header is `40 01 00 00 00 00 3E DF`, with one corrected symbol, as the case expects.

## 4. Who checked it

Derived 2026-10-20 in a Claude Code session (M2 WP-4, branch `m2/aos-fhec`), by hand with the α table above; every step of §2 was also compared with the intermediate register values of the Python model, and the results with the Rust codec. All three agree. A second review of the hand-derived frame vectors, by a reviewer who did not derive them, is WP-6's (`docs/verification/reviews/m2-vectors.md`, not yet written).

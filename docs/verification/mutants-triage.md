# Mutation testing triage (VER-025)

Surviving mutants from `cargo mutants -p aldis-core --all-features` (cargo-mutants 27.1.0), triaged per VER-025. The nightly job `mutants` publishes the full report. Update this file when a new survivor appears.

## 2026-10-17, branch `m1/core` (M1 WP-0)

`cargo mutants -p aldis-core --all-features --file crates/aldis-core/src/anomaly.rs --file crates/aldis-core/src/mode.rs`: 41 mutants: 19 caught, 20 unviable, 0 missed, 2 timeouts.

| Mutant | Verdict | Action |
|---|---|---|
| `anomaly.rs:142` `!=` → `==` in `Iter::next` (`while self.bits != 0`) | Caught by timeout: on the empty set the loop never ends (`trailing_zeros` is 32, `from_bit` rejects it, and the bits stay 0), so `empty_and_default` hangs | None needed |
| `anomaly.rs:144` `&=` → `\|=` in `Iter::next` (clearing the lowest set bit) | Caught by timeout: the bits never shrink, so every non-empty iteration hangs | None needed |

The GEN-022 bound on this loop (inspection, 32 passes) depends on exactly these two operators; the timeouts show that the tests exercise it.

## 2026-10-16, branch `m0/gates`

288 mutants: 198 caught, 81 unviable (do not compile), 9 missed, 0 timeouts.

| Mutant | Verdict | Action |
|---|---|---|
| `container.rs:56` `FixedQueue::is_full` → `true` | Test gap: `is_full` was only asserted on a zero-capacity queue | `fixed_queue` asserts `is_full` before and after filling; caught |
| `container.rs:320` `FixedVec::is_empty` → `false` | Test gap: `is_empty` was only asserted on a non-empty vector | `fixed_vec` asserts `is_empty` when new and after `clear`; caught |
| `security.rs:72` `&&` → `\|\|` in `SecurityLayout::is_none` | Test gap: no case had exactly one of header and trailer empty | `layout_limits` checks header-only and trailer-only layouts; caught |
| `bits.rs:69`, `bits.rs:90` `\|` → `^` in `get`/`set` (`(acc << 8) \| octet`) | Equivalent: the shifted accumulator's low 8 bits are zero, so OR and XOR agree | None possible |
| `bits.rs:92` `\|` → `^` in `set` (`(acc & !field_mask) \| (value << shift)`) | Equivalent: the field bits are cleared first, so OR and XOR agree | None possible |
| `oid.rs:29` `\|` → `^` in `next_bit` (`(r << 1) \| new`) | Equivalent: bit 0 of `r << 1` is zero | None possible |
| `oid.rs:35`, `oid.rs:85` `\|` → `^` in `next_octet` (`(octet << 1) \| bit`) | Equivalent: bit 0 of `octet << 1` is zero | None possible |

After the fixes, a re-run on `container.rs` and `security.rs` found no survivors (89 mutants: 71 caught, 18 unviable). The six equivalent mutants remain and are expected in every report.

These test gaps survived 100 % line, region, and branch coverage: the code was executed, but the result was never checked in the distinguishing state.

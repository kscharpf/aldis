//! Arbitrary data units through the Frame Delimiting and Fill Removal Procedure, against an
//! independent model of TC-020: frames by Frame Length while the next octets form a valid
//! header (version `00`, at least six octets declared); a declared length beyond the unit
//! discards the rest as a truncated frame; anything else is fill. The items cover the unit
//! exactly, in order (232.0 §4.4.9.2; TC-020, GEN-020, GEN-022).
#![no_main]

// req: TC-020, GEN-020, GEN-022, VER-022

use aldis_tc::{Delimited, FrameDelimiter, next_frame};
use libfuzzer_sys::fuzz_target;

/// The model: (kind, offset, length, declared length) of each item.
fn model(unit: &[u8]) -> Vec<(u8, usize, usize, usize)> {
    let mut items = Vec::new();
    let mut at = 0;
    while at < unit.len() {
        let rest = &unit[at..];
        let declared = if rest.len() >= 5 && rest[0] >> 6 == 0 {
            (usize::from(rest[2] & 3) << 8 | usize::from(rest[3])) + 1
        } else {
            0
        };
        if declared < 6 {
            items.push((b'f', at, rest.len(), 0));
            break;
        }
        if declared > rest.len() {
            items.push((b't', at, rest.len(), declared));
            break;
        }
        items.push((b'F', at, declared, 0));
        at += declared;
    }
    items
}

fuzz_target!(|unit: &[u8]| {
    let mut at = 0;
    let mut got = Vec::new();
    for item in FrameDelimiter::new(unit) {
        assert_eq!(item.octets(), &unit[at..at + item.len()]);
        got.push(match item {
            Delimited::Frame(f) => (b'F', at, f.len(), 0),
            Delimited::TruncatedFrame {
                octets,
                declared_len,
            } => (b't', at, octets.len(), declared_len),
            Delimited::Fill(f) => (b'f', at, f.len(), 0),
        });
        at += item.len();
    }
    assert_eq!(at, unit.len());
    assert_eq!(got, model(unit));
    // The stateless call agrees with the iterator.
    assert_eq!(next_frame(unit).map(|i| i.len()), got.first().map(|g| g.2));
});

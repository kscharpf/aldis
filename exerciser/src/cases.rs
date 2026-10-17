//! Case types filled in by `build.rs` from `test-vectors/core/`.

/// `crc16.json`: CRC-16 of `input`.
pub struct CrcCase {
    pub input: &'static [u8],
    pub value: u16,
}

/// `errors.json`: catalogue name and code.
pub struct ErrorCase {
    pub name: &'static str,
    pub code: u16,
}

/// `bits.json` get case; `expect: None` means `OutOfRange`.
pub struct BitsGetCase {
    pub input: &'static [u8],
    pub offset: usize,
    pub width: u8,
    pub expect: Option<u64>,
}

/// `bits.json` set case; `expect: None` means `OutOfRange`.
pub struct BitsSetCase {
    pub input: &'static [u8],
    pub offset: usize,
    pub width: u8,
    pub value: u64,
    pub expect: Option<&'static [u8]>,
}

/// Expected outcome of a `counters.json` case.
pub enum CounterExpect {
    Value(u64),
    InSequence,
    Repeat,
    Gap(u64),
    Backwards,
    OutOfRange,
}

/// `counters.json`: increment (`a` = value) or classify (`a` = prev, `b` = next).
pub struct CounterCase {
    pub classify: bool,
    pub bits: u8,
    pub a: u64,
    pub b: u64,
    pub expect: CounterExpect,
}

/// `layout.json`: sizes = frame, primary header, secondary, security header, security trailer;
/// expect = seven [start, end) pairs, or `None` for `InvalidConfig`.
pub struct LayoutCase {
    pub sizes: [usize; 5],
    pub ocf: bool,
    pub fecf: bool,
    pub expect: Option<[usize; 14]>,
}

#[expect(
    clippy::unreadable_literal,
    reason = "machine-generated vector data from build.rs"
)]
mod generated {
    use super::{
        BitsGetCase, BitsSetCase, CounterCase, CounterExpect, CrcCase, ErrorCase, LayoutCase,
    };
    include!(concat!(env!("OUT_DIR"), "/vectors.rs"));
}
pub use generated::{BITS_GET, BITS_SET, COUNTERS, CRC, ERRORS, LAYOUTS, OID_START};

//! The range type of the views (CABI-012; OI-57 convention 7).

use core::ops::Range;

/// A part of a buffer: `len` octets from `offset` octets after its start (CABI-012, API-061).
///
/// The type of every offset and length pair in a view. A range is relative to the buffer the
/// call was given, never a pointer, so it stays valid if the caller moves the buffer. A
/// range of length 0 is an absent part, at the offset where it would start.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct aldis_range_t {
    /// Octets from the start of the buffer to the first octet of the range.
    pub offset: usize,
    /// Octets in the range.
    pub len: usize,
}

impl From<&Range<usize>> for aldis_range_t {
    fn from(r: &Range<usize>) -> aldis_range_t {
        aldis_range_t {
            offset: r.start,
            len: r.len(),
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    #[test]
    fn a_range_is_an_offset_and_a_length() {
        // req: CABI-012
        assert_eq!(
            aldis_range_t::from(&(6..10)),
            aldis_range_t { offset: 6, len: 4 }
        );
        assert_eq!(
            aldis_range_t::from(&(7..7)),
            aldis_range_t { offset: 7, len: 0 }
        );
        assert_eq!(size_of::<aldis_range_t>(), 2 * size_of::<usize>());
    }
}

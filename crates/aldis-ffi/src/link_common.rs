//! Helper shared by the link engines of the C ABI: the C form of an output range (API-061).

use aldis::core::engine::OutRange;

use crate::range::aldis_range_t;

/// The C form of a range of `out`.
pub(crate) const fn range(r: OutRange) -> aldis_range_t {
    aldis_range_t {
        offset: r.start(),
        len: r.len(),
    }
}

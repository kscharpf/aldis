//! The frame protocols of a case (docs/reference-application.md §4.3, APP-187).
//!
//! The nodes take a [`Variant`] when they are made. The type is closed, so a node matches it
//! with no wildcard arm and a new variant fails to compile where a node has not handled it
//! (RS-021). The scenario file's own type, with its serde and schema derives, is
//! `refapp_harness::scenario::Variant`, which converts to this one.

use core::fmt;

/// The frame protocols of a case, independent of its link level except as APP-187 says: which
/// frames the downlink and the uplink carry (§4.3, D2 and D3 of the A4 plan).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Variant {
    /// TM frames down and TC frames up (§4.1, §4.2): the variant of A1 to A3.
    TmTc,
    /// AOS frames down (§4.3.1) and TC frames up (§4.2). At the levels `frames` and `cop1`.
    Aos,
    /// USLP frames down (§4.3.2) and USLP frames up (§4.3.3). At the level `cop1` only.
    Uslp,
}

impl Variant {
    /// The name the scenario file gives the variant (§6.5).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::TmTc => "tm_tc",
            Self::Aos => "aos",
            Self::Uslp => "uslp",
        }
    }
}

impl fmt::Display for Variant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

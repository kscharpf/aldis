//! The kind of a frame under COP-1: Type-AD, Type-BD, or Type-BC (232.1-B-2 §3.2; 232.0-B-4
//! Table 4-1; 732.1-B-3 Table 4-1; COP-001, COP-010, LNK-001).
//!
//! One enum serves the TC codec (its frame type), the USLP codec (its frame kind converts to
//! and from it), the link engines (the mux), and FOP-1 (the frame a lower procedure is ready
//! for), so the three flags of a frame are stated once (M6 plan WP-2b, OI-96).

/// The kind of a frame by its Bypass and Control Command Flags (232.0 §4.1.2.3, Table 4-1;
/// 732.1 §4.1.2.8, Table 4-1; TC-002, USLP-001).
///
/// The combination Bypass `0` with Control Command `1` is reserved and has no variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CopFrameKind {
    /// `00`: Type-AD, a Frame Data Unit under the Sequence-Controlled Service.
    Ad,
    /// `10`: Type-BD, a Frame Data Unit under the Expedited Service.
    Bd,
    /// `11`: Type-BC, Control Commands (232.0 §4.1.3.3).
    Bc,
}

impl CopFrameKind {
    /// The kind of the two flags, or `None` for the reserved Bypass `0` with Control Command
    /// `1` (232.0 Table 4-1; 732.1 Table 4-1).
    ///
    /// Requirements: TC-002, USLP-001, COP-001.
    #[must_use]
    pub const fn from_flags(bypass: bool, control_command: bool) -> Option<CopFrameKind> {
        match (bypass, control_command) {
            (false, false) => Some(CopFrameKind::Ad),
            (true, false) => Some(CopFrameKind::Bd),
            (true, true) => Some(CopFrameKind::Bc),
            (false, true) => None,
        }
    }

    /// The Bypass Flag (TC) or Bypass/Sequence Control Flag (USLP): `true` for a Type-B frame
    /// (232.0 §4.1.2.3.1; 732.1 §4.1.2.8.1).
    ///
    /// Requirements: TC-002, USLP-001, COP-001.
    #[must_use]
    pub const fn bypass_flag(self) -> bool {
        !matches!(self, CopFrameKind::Ad)
    }

    /// The Control Command Flag (TC) or Protocol Control Command Flag (USLP): `true` for a
    /// Type-BC frame (232.0 §4.1.2.3.2; 732.1 §4.1.2.8.2).
    ///
    /// Requirements: TC-002, USLP-001, COP-001.
    #[must_use]
    pub const fn control_command_flag(self) -> bool {
        matches!(self, CopFrameKind::Bc)
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::CopFrameKind;

    #[test]
    fn flags_and_kind_round_trip() {
        // req: TC-002, USLP-001
        for kind in [CopFrameKind::Ad, CopFrameKind::Bd, CopFrameKind::Bc] {
            assert_eq!(
                CopFrameKind::from_flags(kind.bypass_flag(), kind.control_command_flag()),
                Some(kind)
            );
        }
        assert_eq!(CopFrameKind::from_flags(false, true), None);
    }
}

//! Operational Control Field (CMN-010, CMN-011).
//!
//! The OCF is four octets, carried by TM (132.0-B-3 §4.1.5), AOS (732.0-B-5 §4.1.5), and USLP
//! (732.1-B-3 §4.1.5) Transfer Frames; with SDLS it follows the Security Trailer (132.0 §6.3.7,
//! 732.0 §6.3.7, 732.1 §6.3.7). Its first two bits say what report it holds:
//!
//! | Bit 0 (Type Flag) | Bit 1 | Report | Type |
//! |---|---|---|---|
//! | `0` | part of the report | Type-1 Report: a CLCW (232.0 §4.2), or on USLP a CLCW or a PLCW | [`Type1Report`] |
//! | `1` | `0` | Type-2 Report, project-specific | [`ProjectSpecificReport`] |
//! | `1` | `1` | Type-2 Report, SDLS Frame Security Report (355.1) | [`FsrReport`] |
//!
//! The Type Flag is defined by 132.0 §4.1.5.4, 732.0 §4.1.5.4, and 732.1 §4.1.5.2.1; bit 1 of a
//! Type-2 Report by 132.0 §4.1.5.5, 732.0 §4.1.5.5, and 732.1 §4.1.5.2.2. All four values of the
//! two bits have a meaning, so every four octets are an OCF and [`Ocf::decode`] cannot fail.
//!
//! Each report type holds the four octets with its flag bits checked (RS-020), and nothing
//! more: `aldis-core` decodes no CLCW field (CMN-011). The CLCW is decoded by `aldis-tc`
//! (232.0 §4.2), the FSR by SDLS (355.1). Writers build a report with its checked constructor
//! and write [`Ocf::to_octets`].

use crate::error::{Error, ErrorDetail, ErrorKind, Result};
use crate::field;

/// Length of the Operational Control Field in octets (132.0 §4.1.5.1, 732.0 §4.1.5.1,
/// 732.1 §4.1.5.2): the constant the frame layout uses (CMN-040).
pub use crate::layout::OCF_LEN;

/// Bit 0 of the OCF, the Type Flag, in its first octet.
const TYPE_FLAG: u8 = 0x80;
/// Bit 1 of the OCF, which tells the use of a Type-2 Report, in its first octet.
const REPORT_USE: u8 = 0x40;

/// The value (0 or 1) of the bit `mask` selects in the first octet of `octets`. [`Ocf::decode`]
/// is the only caller: every report type is checked through it.
const fn bit(octets: [u8; OCF_LEN], mask: u8) -> u8 {
    let [first, ..] = octets;
    if first & mask == 0 { 0 } else { 1 }
}

/// `InvalidField` for an OCF bit that holds `found` where the report type needs `expected`.
const fn wrong_bit(field: crate::FieldId, expected: u8, found: u8) -> Error {
    Error::new(ErrorKind::InvalidField)
        .with_field(field)
        .with_detail(ErrorDetail::Expected {
            expected: crate::error::u8_to_u64(expected),
            found: crate::error::u8_to_u64(found),
        })
}

/// A Type-1 Report: four OCF octets whose Type Flag (bit 0) is `0` (132.0 §4.1.5.4 a,
/// 732.0 §4.1.5.4 a, 732.1 §4.1.5.2.1 a; CMN-010).
///
/// It holds a Communications Link Control Word (232.0 §4.2), or on USLP a CLCW or a PLCW. The
/// octets are opaque here: `aldis-tc` decodes the CLCW (CMN-011).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Type1Report([u8; OCF_LEN]);

impl Type1Report {
    /// The report, or `None` if the Type Flag of `octets` is `1` (CMN-010).
    #[must_use]
    pub const fn new(octets: [u8; OCF_LEN]) -> Option<Type1Report> {
        match Ocf::decode(octets) {
            Ocf::Clcw(report) => Some(report),
            Ocf::ProjectSpecific(_) | Ocf::SdlsFsr(_) => None,
        }
    }

    /// The four octets, as they are sent (CMN-010).
    #[must_use]
    pub const fn to_octets(self) -> [u8; OCF_LEN] {
        self.0
    }
}

impl TryFrom<[u8; OCF_LEN]> for Type1Report {
    type Error = Error;

    /// # Errors
    /// `InvalidField` naming [`field::OCF_TYPE_FLAG`] if the Type Flag is `1`.
    fn try_from(octets: [u8; OCF_LEN]) -> Result<Type1Report> {
        match Ocf::decode(octets) {
            Ocf::Clcw(report) => Ok(report),
            Ocf::ProjectSpecific(_) | Ocf::SdlsFsr(_) => Err(wrong_bit(field::OCF_TYPE_FLAG, 0, 1)),
        }
    }
}

impl From<Type1Report> for [u8; OCF_LEN] {
    fn from(report: Type1Report) -> [u8; OCF_LEN] {
        report.0
    }
}

/// A project-specific Type-2 Report: four OCF octets whose Type Flag (bit 0) is `1` and whose
/// bit 1 is `0` (132.0 §4.1.5.5 a, 732.0 §4.1.5.5 a, 732.1 §4.1.5.2.2 a; CMN-010).
///
/// The other 30 bits are defined by the project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProjectSpecificReport([u8; OCF_LEN]);

impl ProjectSpecificReport {
    /// The report, or `None` unless bits 0–1 of `octets` are `10` (CMN-010).
    #[must_use]
    pub const fn new(octets: [u8; OCF_LEN]) -> Option<ProjectSpecificReport> {
        match Ocf::decode(octets) {
            Ocf::ProjectSpecific(report) => Some(report),
            Ocf::Clcw(_) | Ocf::SdlsFsr(_) => None,
        }
    }

    /// The four octets, as they are sent (CMN-010).
    #[must_use]
    pub const fn to_octets(self) -> [u8; OCF_LEN] {
        self.0
    }
}

impl TryFrom<[u8; OCF_LEN]> for ProjectSpecificReport {
    type Error = Error;

    /// # Errors
    /// `InvalidField` naming [`field::OCF_TYPE_FLAG`] if the Type Flag is `0`, or
    /// [`field::OCF_REPORT_USE`] if bit 1 is `1`.
    fn try_from(octets: [u8; OCF_LEN]) -> Result<ProjectSpecificReport> {
        match Ocf::decode(octets) {
            Ocf::ProjectSpecific(report) => Ok(report),
            Ocf::Clcw(_) => Err(wrong_bit(field::OCF_TYPE_FLAG, 1, 0)),
            Ocf::SdlsFsr(_) => Err(wrong_bit(field::OCF_REPORT_USE, 0, 1)),
        }
    }
}

impl From<ProjectSpecificReport> for [u8; OCF_LEN] {
    fn from(report: ProjectSpecificReport) -> [u8; OCF_LEN] {
        report.0
    }
}

/// A Type-2 Report that holds an SDLS Frame Security Report: four OCF octets whose Type Flag
/// (bit 0) is `1` and whose bit 1 is `1` (132.0 §4.1.5.5 b, 732.0 §4.1.5.5 b,
/// 732.1 §4.1.5.2.2 b; CMN-010).
///
/// The FSR is defined by SDLS Extended Procedures (355.1); its fields are not decoded here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FsrReport([u8; OCF_LEN]);

impl FsrReport {
    /// The report, or `None` unless bits 0–1 of `octets` are `11` (CMN-010).
    #[must_use]
    pub const fn new(octets: [u8; OCF_LEN]) -> Option<FsrReport> {
        match Ocf::decode(octets) {
            Ocf::SdlsFsr(report) => Some(report),
            Ocf::Clcw(_) | Ocf::ProjectSpecific(_) => None,
        }
    }

    /// The four octets, as they are sent (CMN-010).
    #[must_use]
    pub const fn to_octets(self) -> [u8; OCF_LEN] {
        self.0
    }
}

impl TryFrom<[u8; OCF_LEN]> for FsrReport {
    type Error = Error;

    /// # Errors
    /// `InvalidField` naming [`field::OCF_TYPE_FLAG`] if the Type Flag is `0`, or
    /// [`field::OCF_REPORT_USE`] if bit 1 is `0`.
    fn try_from(octets: [u8; OCF_LEN]) -> Result<FsrReport> {
        match Ocf::decode(octets) {
            Ocf::SdlsFsr(report) => Ok(report),
            Ocf::Clcw(_) => Err(wrong_bit(field::OCF_TYPE_FLAG, 1, 0)),
            Ocf::ProjectSpecific(_) => Err(wrong_bit(field::OCF_REPORT_USE, 1, 0)),
        }
    }
}

impl From<FsrReport> for [u8; OCF_LEN] {
    fn from(report: FsrReport) -> [u8; OCF_LEN] {
        report.0
    }
}

/// The content of an Operational Control Field, classified by bits 0–1 (132.0 §4.1.5.4–5,
/// 732.0 §4.1.5.4–5, 732.1 §4.1.5.2.1–2; CMN-010).
///
/// The two bits have no reserved value, so the three variants cover every OCF. TM, AOS, and USLP
/// views return an `Ocf`; the caller decodes a Type-1 Report with `aldis-tc` (CMN-011).
///
/// ```
/// use aldis_core::ocf::Ocf;
///
/// let ocf = Ocf::decode([0x01, 0x14, 0x0C, 0x2A]);
/// let Ocf::Clcw(clcw) = ocf else { unreachable!() };
/// assert_eq!(clcw.to_octets(), [0x01, 0x14, 0x0C, 0x2A]);
/// assert!(matches!(Ocf::decode([0xC0, 0, 0, 0]), Ocf::SdlsFsr(_)));
/// assert_eq!(ocf.to_octets(), [0x01, 0x14, 0x0C, 0x2A]);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Ocf {
    /// Type Flag `0`: a Type-1 Report, the CLCW (or, on USLP, a CLCW or a PLCW).
    Clcw(Type1Report),
    /// Type Flag `1`, bit 1 `0`: a project-specific Type-2 Report.
    ProjectSpecific(ProjectSpecificReport),
    /// Type Flag `1`, bit 1 `1`: a Type-2 Report holding an SDLS Frame Security Report.
    SdlsFsr(FsrReport),
}

impl Ocf {
    /// Classifies four OCF octets by bits 0–1 (CMN-010). Every value is an OCF.
    #[must_use]
    pub const fn decode(octets: [u8; OCF_LEN]) -> Ocf {
        match (bit(octets, TYPE_FLAG), bit(octets, REPORT_USE)) {
            (0, _) => Ocf::Clcw(Type1Report(octets)),
            (_, 0) => Ocf::ProjectSpecific(ProjectSpecificReport(octets)),
            _ => Ocf::SdlsFsr(FsrReport(octets)),
        }
    }

    /// The four octets to write into a frame's OCF (CMN-010).
    #[must_use]
    pub const fn to_octets(self) -> [u8; OCF_LEN] {
        match self {
            Ocf::Clcw(r) => r.to_octets(),
            Ocf::ProjectSpecific(r) => r.to_octets(),
            Ocf::SdlsFsr(r) => r.to_octets(),
        }
    }
}

impl From<[u8; OCF_LEN]> for Ocf {
    fn from(octets: [u8; OCF_LEN]) -> Ocf {
        Ocf::decode(octets)
    }
}

impl From<Ocf> for [u8; OCF_LEN] {
    fn from(ocf: Ocf) -> [u8; OCF_LEN] {
        ocf.to_octets()
    }
}

impl From<Type1Report> for Ocf {
    fn from(report: Type1Report) -> Ocf {
        Ocf::Clcw(report)
    }
}

impl From<ProjectSpecificReport> for Ocf {
    fn from(report: ProjectSpecificReport) -> Ocf {
        Ocf::ProjectSpecific(report)
    }
}

impl From<FsrReport> for Ocf {
    fn from(report: FsrReport) -> Ocf {
        Ocf::SdlsFsr(report)
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{FsrReport, OCF_LEN, Ocf, ProjectSpecificReport, Type1Report};
    use crate::error::{ErrorDetail, ErrorKind};
    use crate::field;
    use proptest::prelude::*;

    /// The CLCW of 232.0 figure 4-6 with COP-1 in effect, VCID 5, Retransmit set, FARM-B
    /// Counter 2, and Report Value 42.
    const CLCW: [u8; OCF_LEN] = [0x01, 0x14, 0x0C, 0x2A];

    fn invalid(e: crate::Error, f: crate::FieldId, expected: u64, found: u64) -> bool {
        e.kind() == ErrorKind::InvalidField
            && e.field() == Some(f)
            && e.detail() == ErrorDetail::Expected { expected, found }
    }

    #[test]
    fn bits_0_and_1_select_the_report() {
        // req: CMN-010
        assert_eq!(Ocf::decode(CLCW), Ocf::Clcw(Type1Report(CLCW)));
        assert!(matches!(Ocf::decode([0x00; 4]), Ocf::Clcw(_)));
        // Bit 1 of a Type-1 Report belongs to the report, not to the classification.
        assert!(matches!(
            Ocf::decode([0x7F, 0xFF, 0xFF, 0xFF]),
            Ocf::Clcw(_)
        ));
        assert!(matches!(Ocf::decode([0x40, 0, 0, 0]), Ocf::Clcw(_)));
        assert!(matches!(
            Ocf::decode([0x80, 0, 0, 0]),
            Ocf::ProjectSpecific(_)
        ));
        assert!(matches!(
            Ocf::decode([0xBF, 0xFF, 0xFF, 0xFF]),
            Ocf::ProjectSpecific(_)
        ));
        assert!(matches!(Ocf::decode([0xC0, 0, 0, 0]), Ocf::SdlsFsr(_)));
        assert!(matches!(Ocf::decode([0xFF; 4]), Ocf::SdlsFsr(_)));
    }

    #[test]
    fn the_clcw_stays_opaque() {
        // req: CMN-011
        // Values the CLCW forbids (version '11', spare bits set) are still a Type-1 Report:
        // checking them is aldis-tc's job (232.0 §4.2.1.3, §4.2.1.7, §4.2.1.10).
        let odd = [0x7F, 0xFF, 0xFF, 0xFF];
        assert_eq!(Type1Report::new(odd).map(Type1Report::to_octets), Some(odd));
        assert_eq!(Ocf::decode(odd).to_octets(), odd);
    }

    #[test]
    fn report_constructors_check_their_bits() {
        // req: CMN-010
        let ps = [0x80, 0x12, 0x34, 0x56];
        let fsr = [0xC1, 0x23, 0x45, 0x67];
        assert_eq!(Type1Report::try_from(CLCW).map(<[u8; 4]>::from), Ok(CLCW));
        assert!(Type1Report::new(ps).is_none() && Type1Report::new(fsr).is_none());
        let e = Type1Report::try_from(ps).unwrap_err();
        assert!(invalid(e, field::OCF_TYPE_FLAG, 0, 1));

        assert_eq!(
            ProjectSpecificReport::try_from(ps).map(<[u8; 4]>::from),
            Ok(ps)
        );
        assert_eq!(
            ProjectSpecificReport::new(ps).map(ProjectSpecificReport::to_octets),
            Some(ps)
        );
        assert!(ProjectSpecificReport::new(CLCW).is_none());
        assert!(ProjectSpecificReport::new(fsr).is_none());
        let e = ProjectSpecificReport::try_from(CLCW).unwrap_err();
        assert!(invalid(e, field::OCF_TYPE_FLAG, 1, 0));
        let e = ProjectSpecificReport::try_from([0x40, 0, 0, 0]).unwrap_err();
        assert!(invalid(e, field::OCF_TYPE_FLAG, 1, 0));
        let e = ProjectSpecificReport::try_from(fsr).unwrap_err();
        assert!(invalid(e, field::OCF_REPORT_USE, 0, 1));

        assert_eq!(FsrReport::try_from(fsr).map(<[u8; 4]>::from), Ok(fsr));
        assert_eq!(FsrReport::new(fsr).map(FsrReport::to_octets), Some(fsr));
        assert!(FsrReport::new(CLCW).is_none() && FsrReport::new(ps).is_none());
        assert!(FsrReport::new([0x40, 0, 0, 0]).is_none());
        let e = FsrReport::try_from([0x40, 0, 0, 0]).unwrap_err();
        assert!(invalid(e, field::OCF_TYPE_FLAG, 1, 0));
        let e = FsrReport::try_from(ps).unwrap_err();
        assert!(invalid(e, field::OCF_REPORT_USE, 1, 0));
    }

    #[test]
    fn writers_round_trip_through_the_ocf() {
        // req: CMN-010
        let ps = ProjectSpecificReport::new([0xA5, 0x5A, 0x00, 0xFF]).unwrap();
        let fsr = FsrReport::new([0xE0, 0x01, 0x02, 0x03]).unwrap();
        let clcw = Type1Report::new(CLCW).unwrap();
        for (ocf, octets) in [
            (Ocf::from(clcw), CLCW),
            (Ocf::from(ps), ps.to_octets()),
            (Ocf::from(fsr), fsr.to_octets()),
        ] {
            assert_eq!(ocf.to_octets(), octets);
            assert_eq!(<[u8; 4]>::from(ocf), octets);
            assert_eq!(Ocf::from(octets), ocf);
        }
        assert_eq!(Ocf::from(ps), Ocf::ProjectSpecific(ps));
        assert_eq!(Ocf::from(fsr), Ocf::SdlsFsr(fsr));
    }

    proptest! {
        #[test]
        fn classification_matches_the_flag_bits(octets in any::<[u8; 4]>()) {
            // req: CMN-010
            let ocf = Ocf::decode(octets);
            prop_assert_eq!(ocf.to_octets(), octets);
            let (type_flag, use_bit) = (octets[0] >> 7, (octets[0] >> 6) & 1);
            let expected = match (type_flag, use_bit) {
                (0, _) => Ocf::Clcw(Type1Report::new(octets).unwrap()),
                (_, 0) => Ocf::ProjectSpecific(ProjectSpecificReport::new(octets).unwrap()),
                _ => Ocf::SdlsFsr(FsrReport::new(octets).unwrap()),
            };
            prop_assert_eq!(ocf, expected);
            // Exactly one report type accepts the octets.
            let accepted = [
                Type1Report::new(octets).is_some(),
                ProjectSpecificReport::new(octets).is_some(),
                FsrReport::new(octets).is_some(),
            ];
            prop_assert_eq!(accepted.iter().filter(|&&a| a).count(), 1);
        }
    }
}

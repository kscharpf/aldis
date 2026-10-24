//! Communications Link Control Word (232.0 §4.2; TC-030, TC-031, CMN-011).
//!
//! The CLCW travels as a Type-1 Report in the Operational Control Field of TM, AOS, and USLP
//! frames, which `aldis-core` classifies without decoding ([`Ocf`], CMN-010, CMN-011).
//! [`Clcw::decode_report`] decodes it; [`Clcw::to_ocf`] encodes it for a frame writer.

use aldis_core::ids::FrameSeqNum;
use aldis_core::ocf::{OCF_LEN, Ocf, Type1Report};
use aldis_core::{AnomalySet, DecodeMode, Error, ErrorDetail, ErrorKind, FieldId, Result};

use crate::anomaly::TcAnomaly;
use crate::field;
use crate::header::{TcVcId, reserved};

/// Defines a CLCW field of `BITS` bits (`$max` = 2^BITS − 1) with a checked constructor.
macro_rules! clcw_field {
    ($(#[$doc:meta])* $name:ident, $max:literal, $field:expr) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
        pub struct $name(u8);

        impl $name {
            /// Largest value.
            pub const MAX: u8 = $max;

            /// The value, or `None` if it does not fit the field (API-001).
            #[must_use]
            pub const fn new(value: u8) -> Option<$name> {
                if value <= $name::MAX { Some($name(value)) } else { None }
            }

            /// The numeric value.
            #[must_use]
            pub const fn get(self) -> u8 {
                self.0
            }

            /// The low bits of `bits`, as the decoder reads them.
            const fn masked(bits: u8) -> $name {
                $name(bits & $name::MAX)
            }
        }

        impl TryFrom<u8> for $name {
            type Error = Error;

            /// # Errors
            /// `OutOfRange` naming the field if `value` does not fit.
            fn try_from(value: u8) -> Result<$name> {
                $name::new(value).ok_or(Error::out_of_range(
                    $field,
                    u64::from($name::MAX),
                    u64::from(value),
                ))
            }
        }

        impl From<$name> for u8 {
            fn from(v: $name) -> u8 {
                v.0
            }
        }
    };
}

clcw_field!(
    /// Status Field, 3 bits, mission-specific and passed through unchanged (232.0 §4.2.1.4;
    /// TC-031).
    ClcwStatus, 0b111, field::CLCW_STATUS
);
clcw_field!(
    /// COP in Effect, 2 bits: `01` is COP-1 (232.0 §4.2.1.5). The decoder accepts and exposes
    /// every value; FOP-1 rejects the others itself (TC-030).
    CopInEffect, 0b11, field::COP_IN_EFFECT
);
clcw_field!(
    /// FARM-B Counter, 2 bits: the two low bits of the FARM-B counter (232.0 §4.2.1.9).
    FarmBCounter, 0b11, field::FARM_B_COUNTER
);

impl CopInEffect {
    /// COP-1, `01` (232.0 §4.2.1.5.2).
    pub const COP_1: CopInEffect = CopInEffect(0b01);
}

/// A Communications Link Control Word (232.0 §4.2.1; TC-030, TC-031).
///
/// The Control Word Type, the CLCW Version Number, and the two Reserved Spare fields are not
/// held: the encoder writes them as `0`, `00`, and zeros (GEN-052).
#[expect(
    clippy::struct_excessive_bools,
    reason = "the five independent one-bit Flags of 232.0 §4.2.1.8, each a field of the CLCW"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Clcw {
    /// Status Field (232.0 §4.2.1.4; TC-031).
    pub status: ClcwStatus,
    /// COP in Effect (232.0 §4.2.1.5).
    pub cop_in_effect: CopInEffect,
    /// Virtual Channel Identification (232.0 §4.2.1.6).
    pub vc_id: TcVcId,
    /// No RF Available Flag (232.0 §4.2.1.8.2).
    pub no_rf_available: bool,
    /// No Bit Lock Flag (232.0 §4.2.1.8.3).
    pub no_bit_lock: bool,
    /// Lockout Flag (232.0 §4.2.1.8.4).
    pub lockout: bool,
    /// Wait Flag (232.0 §4.2.1.8.5).
    pub wait: bool,
    /// Retransmit Flag (232.0 §4.2.1.8.6).
    pub retransmit: bool,
    /// FARM-B Counter (232.0 §4.2.1.9).
    pub farm_b_counter: FarmBCounter,
    /// Report Value N(R), FARM-1's V(R) (232.0 §4.2.1.11).
    pub report_value: FrameSeqNum,
}

/// `mask` if `set`, else 0: one flag bit of the CLCW.
const fn flag(set: bool, mask: u8) -> u8 {
    if set { mask } else { 0 }
}

/// `InvalidField` or `UnsupportedVersion` naming `field`, a fixed CLCW field that holds
/// `found`.
fn fixed(kind: ErrorKind, field: FieldId, found: u8) -> Error {
    Error::new(kind)
        .with_field(field)
        .with_detail(ErrorDetail::Expected {
            expected: 0,
            found: u64::from(found),
        })
}

impl Clcw {
    /// Decodes the four CLCW octets (232.0 §4.2.1; TC-030, TC-031). Any COP in Effect is
    /// accepted and exposed; the Status Field is passed through.
    ///
    /// # Errors
    /// - `InvalidField` (`control_word_type`) for a Control Word Type of `1`, which is not a
    ///   CLCW, and `UnsupportedVersion` (`clcw_version`) for a version other than `00`, in both
    ///   modes (API-032).
    /// - `ReservedValue` (`clcw_spare_14_15`, `clcw_spare_23`) in strict mode for a Reserved
    ///   Spare that is not zero; lenient mode reports [`TcAnomaly::ClcwSpareSet`] in the
    ///   returned set instead.
    pub fn decode(
        octets: [u8; OCF_LEN],
        mode: DecodeMode,
    ) -> Result<(Clcw, AnomalySet<TcAnomaly>)> {
        let [b0, b1, b2, b3] = octets;
        if b0 & 0x80 != 0 {
            return Err(fixed(ErrorKind::InvalidField, field::CONTROL_WORD_TYPE, 1));
        }
        let version = (b0 >> 5) & 0b11;
        if version != 0 {
            return Err(fixed(
                ErrorKind::UnsupportedVersion,
                field::CLCW_VERSION,
                version,
            ));
        }
        let mut anomalies = AnomalySet::empty();
        for (set, fld) in [
            (b1 & 0b11 != 0, field::CLCW_SPARE_14_15),
            (b2 & 0x01 != 0, field::CLCW_SPARE_23),
        ] {
            if set {
                match mode {
                    DecodeMode::Strict => return Err(reserved(fld)),
                    DecodeMode::Lenient => anomalies.insert(TcAnomaly::ClcwSpareSet),
                }
            }
        }
        TcVcId::new(b1 >> 2)
            .map(|vc_id| {
                let clcw = Clcw {
                    status: ClcwStatus::masked(b0 >> 2),
                    cop_in_effect: CopInEffect::masked(b0),
                    vc_id,
                    no_rf_available: b2 & 0x80 != 0,
                    no_bit_lock: b2 & 0x40 != 0,
                    lockout: b2 & 0x20 != 0,
                    wait: b2 & 0x10 != 0,
                    retransmit: b2 & 0x08 != 0,
                    farm_b_counter: FarmBCounter::masked(b2 >> 1),
                    report_value: FrameSeqNum::new(b3),
                };
                (clcw, anomalies)
            })
            .ok_or(Error::new(ErrorKind::InvalidField).with_field(field::VC_ID))
    }

    /// Decodes the CLCW of a Type-1 Report taken from an Operational Control Field (232.0
    /// §4.2.1; TC-030, CMN-011).
    ///
    /// # Errors
    /// The errors of [`decode`](Self::decode).
    pub fn decode_report(
        report: Type1Report,
        mode: DecodeMode,
    ) -> Result<(Clcw, AnomalySet<TcAnomaly>)> {
        Clcw::decode(report.to_octets(), mode)
    }

    /// The four CLCW octets: Control Word Type `0`, version `00`, spares zero (232.0 §4.2.1;
    /// TC-030, TC-031, GEN-052).
    #[must_use]
    pub const fn to_octets(&self) -> [u8; OCF_LEN] {
        [
            (self.status.get() << 2) | self.cop_in_effect.get(),
            self.vc_id.get() << 2,
            flag(self.no_rf_available, 0x80)
                | flag(self.no_bit_lock, 0x40)
                | flag(self.lockout, 0x20)
                | flag(self.wait, 0x10)
                | flag(self.retransmit, 0x08)
                | (self.farm_b_counter.get() << 1),
            self.report_value.get(),
        ]
    }

    /// The CLCW as a 32-bit word, bit 0 the most significant (232.0 §4.2.1; M2 plan D14).
    #[must_use]
    pub const fn to_word(&self) -> u32 {
        u32::from_be_bytes(self.to_octets())
    }

    /// The Operational Control Field that carries this CLCW, a Type-1 Report (232.0 §4.2.1.2;
    /// CMN-010, CMN-011).
    #[must_use]
    pub const fn to_ocf(&self) -> Ocf {
        Ocf::decode(self.to_octets())
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{Clcw, ClcwStatus, CopInEffect, FarmBCounter};
    use crate::anomaly::TcAnomaly;
    use crate::field;
    use crate::header::TcVcId;
    use aldis_core::ids::FrameSeqNum;
    use aldis_core::ocf::{Ocf, Type1Report};
    use aldis_core::{DecodeMode, ErrorKind};

    fn example() -> Clcw {
        Clcw {
            status: ClcwStatus::default(),
            cop_in_effect: CopInEffect::COP_1,
            vc_id: TcVcId::new(5).unwrap(),
            no_rf_available: false,
            no_bit_lock: false,
            lockout: false,
            wait: false,
            retransmit: true,
            farm_b_counter: FarmBCounter::new(2).unwrap(),
            report_value: FrameSeqNum::new(42),
        }
    }

    #[test]
    fn encode_and_decode() {
        // req: TC-030, TC-031, CMN-011
        let c = example();
        assert_eq!(c.to_octets(), [0x01, 0x14, 0x0C, 0x2A]);
        assert_eq!(c.to_word(), 0x0114_0C2A);
        let Ocf::Clcw(report) = c.to_ocf() else {
            panic!("a CLCW is a Type-1 Report")
        };
        let (d, anomalies) = Clcw::decode_report(report, DecodeMode::Strict).unwrap();
        assert_eq!(d, c);
        assert!(anomalies.is_empty());
        let all = Clcw {
            status: ClcwStatus::new(7).unwrap(),
            cop_in_effect: CopInEffect::new(3).unwrap(),
            vc_id: TcVcId::new(63).unwrap(),
            no_rf_available: true,
            no_bit_lock: true,
            lockout: true,
            wait: true,
            retransmit: true,
            farm_b_counter: FarmBCounter::new(3).unwrap(),
            report_value: FrameSeqNum::new(255),
        };
        assert_eq!(all.to_octets(), [0x1F, 0xFC, 0xFE, 0xFF]);
        assert_eq!(
            Clcw::decode(all.to_octets(), DecodeMode::Strict).unwrap().0,
            all
        );
    }

    #[test]
    fn decode_errors_and_anomalies() {
        // req: TC-030, API-031, API-032, GEN-041
        for mode in [DecodeMode::Strict, DecodeMode::Lenient] {
            let e = Clcw::decode([0x81, 0, 0, 0], mode).unwrap_err();
            assert_eq!(
                (e.kind(), e.field()),
                (ErrorKind::InvalidField, Some(field::CONTROL_WORD_TYPE))
            );
            let e = Clcw::decode([0x41, 0, 0, 0], mode).unwrap_err();
            assert_eq!(
                (e.kind(), e.field()),
                (ErrorKind::UnsupportedVersion, Some(field::CLCW_VERSION))
            );
        }
        let e = Clcw::decode([0x01, 0x01, 0, 0], DecodeMode::Strict).unwrap_err();
        assert_eq!(e.field(), Some(field::CLCW_SPARE_14_15));
        let e = Clcw::decode([0x01, 0, 0x01, 0], DecodeMode::Strict).unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::ReservedValue, Some(field::CLCW_SPARE_23))
        );
        let (c, a) = Clcw::decode([0x01, 0x03, 0x01, 9], DecodeMode::Lenient).unwrap();
        assert!(a.iter().eq([TcAnomaly::ClcwSpareSet]));
        assert_eq!((c.vc_id.get(), c.report_value.get()), (0, 9));
        let report = Type1Report::new([0x01, 0x14, 0x0C, 0x2A]).unwrap();
        assert!(Clcw::decode_report(report, DecodeMode::Strict).is_ok());
    }

    #[test]
    fn field_types() {
        // req: TC-030, API-001
        assert_eq!(ClcwStatus::new(8), None);
        assert_eq!(CopInEffect::new(4), None);
        assert_eq!(FarmBCounter::new(4), None);
        assert_eq!(CopInEffect::COP_1.get(), 1);
        let e = ClcwStatus::try_from(9).unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::OutOfRange, Some(field::CLCW_STATUS))
        );
        assert_eq!(
            CopInEffect::try_from(4).unwrap_err().field(),
            Some(field::COP_IN_EFFECT)
        );
        assert_eq!(
            FarmBCounter::try_from(5).unwrap_err().field(),
            Some(field::FARM_B_COUNTER)
        );
        assert_eq!(u8::from(ClcwStatus::try_from(5).unwrap()), 5);
        assert_eq!(u8::from(FarmBCounter::masked(0xFF)), 3);
    }
}

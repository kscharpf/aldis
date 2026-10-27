//! The Transfer Frame Primary Header, decoded after the FHEC has corrected it (732.0-B-5
//! §4.1.2, §4.3.7.2; 732.0-B-4 §4.1.2; AOS-002, AOS-010…AOS-014, API-014).
//!
//! ```text
//! bits 0–1   Transfer Frame Version Number, 01
//! bits 2–9   Spacecraft ID, 8 least significant bits
//! bits 10–15 Virtual Channel ID
//! bits 16–39 Virtual Channel Frame Count
//! bit  40    Replay Flag
//! bit  41    VC Frame Count Cycle Use Flag
//! bits 42–43 Spacecraft ID, 2 most significant bits (Issue 5); reserved spare 00 (Issue ≤4)
//! bits 44–47 VC Frame Count Cycle
//! bits 48–63 Frame Header Error Control (optional)
//! ```

use aldis_core::ids::{SpacecraftId, VcId};
use aldis_core::{Error, ErrorDetail, ErrorKind, Result};

use crate::config::{AosEdition, AosPhysicalChannelConfig};
use crate::fhec::{self, CorrectedSymbols};
use crate::field;

/// Length of the Transfer Frame Primary Header without the FHEC, in octets (732.0 §4.1.2.1).
pub const PRIMARY_HEADER_LEN: usize = 6;

/// Length of the Frame Header Error Control, in octets (732.0 §4.1.2.6.1).
pub const FHEC_LEN: usize = 2;

/// Transfer Frame Version Number of an AOS Transfer Frame, `01` (732.0 §4.1.2.2.2.2; AOS-011).
pub const VERSION: u8 = 0b01;

/// VCID 63, all ones: the VCID of OID Transfer Frames (732.0 §4.1.4.1.5.1, Table 5-2 note 2;
/// AOS-016).
pub const OID_VCID: VcId<6> = VcId::new(63).expect("63 fits in 6 bits");

/// `usize` to `u64` for an error's [`ErrorDetail::Expected`], in `const` context.
#[expect(
    clippy::as_conversions,
    reason = "supported targets are 32- or 64-bit, so usize fits in u64"
)]
pub(crate) const fn len_u64(n: usize) -> u64 {
    n as u64
}

/// An error of `kind` for `field`, with the value expected and the value found (GEN-041).
pub(crate) const fn mismatch(
    kind: ErrorKind,
    field: aldis_core::FieldId,
    expected: u64,
    found: u64,
) -> Error {
    Error::new(kind)
        .with_field(field)
        .with_detail(ErrorDetail::Expected { expected, found })
}

/// The Virtual Channel Frame Count, 24 bits, modulo 16 777 216 (732.0 §4.1.2.4; AOS-019,
/// API-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct VcFrameCount(u32);

impl VcFrameCount {
    /// Largest value, 2²⁴ − 1 (732.0 §4.1.2.4.2).
    pub const MAX: u32 = 0x00FF_FFFF;

    /// The count, or `None` if `value` needs more than 24 bits (API-001).
    #[must_use]
    pub const fn new(value: u32) -> Option<VcFrameCount> {
        if value <= VcFrameCount::MAX {
            Some(VcFrameCount(value))
        } else {
            None
        }
    }

    /// The numeric value (732.0 §4.1.2.4).
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl TryFrom<u32> for VcFrameCount {
    type Error = Error;

    /// # Errors
    /// `OutOfRange` (`vcf_count`) above 2²⁴ − 1.
    fn try_from(value: u32) -> Result<VcFrameCount> {
        VcFrameCount::new(value).ok_or_else(|| {
            Error::out_of_range(
                field::VCF_COUNT,
                u64::from(VcFrameCount::MAX),
                u64::from(value),
            )
        })
    }
}

impl From<VcFrameCount> for u32 {
    fn from(count: VcFrameCount) -> u32 {
        count.0
    }
}

/// The Virtual Channel Frame Count Cycle, 4 bits (732.0 §4.1.2.5.5; AOS-012, AOS-019, API-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct FrameCountCycle(u8);

impl FrameCountCycle {
    /// Largest value, `1111` (732.0 §4.1.2.5.5.1).
    pub const MAX: u8 = 0x0F;

    /// The cycle, or `None` if `value` needs more than 4 bits (API-001).
    #[must_use]
    pub const fn new(value: u8) -> Option<FrameCountCycle> {
        if value <= FrameCountCycle::MAX {
            Some(FrameCountCycle(value))
        } else {
            None
        }
    }

    /// The numeric value (732.0 §4.1.2.5.5).
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

impl TryFrom<u8> for FrameCountCycle {
    type Error = Error;

    /// # Errors
    /// `OutOfRange` (`vc_frame_count_cycle`) above 15.
    fn try_from(value: u8) -> Result<FrameCountCycle> {
        FrameCountCycle::new(value).ok_or_else(|| {
            Error::out_of_range(
                field::VC_FRAME_COUNT_CYCLE,
                u64::from(FrameCountCycle::MAX),
                u64::from(value),
            )
        })
    }
}

impl From<FrameCountCycle> for u8 {
    fn from(cycle: FrameCountCycle) -> u8 {
        cycle.0
    }
}

/// The value of SCID 0, the fallback of a masked field that always fits (GEN-020).
const SCID_ZERO: SpacecraftId<10> = SpacecraftId::new(0).expect("0 fits");
/// The value of VCID 0, the fallback of a masked field that always fits (GEN-020).
const VCID_ZERO: VcId<6> = VcId::new(0).expect("0 fits");

/// An AOS Transfer Frame Primary Header as owned values, after FHEC correction (732.0 §4.1.2;
/// AOS-002, AOS-010…AOS-014, API-014).
///
/// Built by [`decode`](Self::decode) and by the writers. The fields are private because the
/// edition relates them: in `Issue4Compatible` the SCID has 8 bits and bits 42–43 are a spare,
/// which [`issue4_spare`](Self::issue4_spare) reports as received (RS-020).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AosPrimaryHeader {
    edition: AosEdition,
    scid: SpacecraftId<10>,
    vcid: VcId<6>,
    vc_frame_count: VcFrameCount,
    replay_flag: bool,
    cycle_use_flag: bool,
    cycle: FrameCountCycle,
    spare: u8,
    fhec: Option<u16>,
    corrected: CorrectedSymbols,
}

/// The header octets after the FHEC check: the first six or eight octets of the frame, with at
/// most two symbols corrected (732.0 §4.3.7.2; AOS-014).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Received {
    /// Octets 0–7; octets 6–7 are zero without the FHEC.
    pub(crate) octets: [u8; 8],
    /// 6 or 8.
    pub(crate) len: usize,
    /// How many symbols the FHEC corrected; zero without the FHEC.
    pub(crate) corrected: CorrectedSymbols,
}

impl Received {
    /// The header octets as received after correction.
    pub(crate) fn as_slice(&self) -> &[u8] {
        let len = self.len.min(self.octets.len());
        #[expect(
            clippy::indexing_slicing,
            reason = "clamped to self.octets.len() so slice is guaranteed in-bounds"
        )]
        &self.octets[..len]
    }
}

/// Reads the header octets at the start of `prefix` and, with the FHEC configured, corrects up
/// to two symbols before any field is read (732.0 §4.1.2.6.5, §4.3.7.2; AOS-014).
///
/// # Errors
/// `Incomplete` (`needed` = 6 or 8) if `prefix` is shorter than the header;
/// `HeaderUncorrectable` from the FHEC.
pub(crate) fn correct(prefix: &[u8], pc: &AosPhysicalChannelConfig) -> Result<Received> {
    let len = pc.primary_header_len();
    let mut octets = [0_u8; 8];
    let head = prefix.get(..len).ok_or(Error::incomplete(len))?;
    for (o, p) in octets.iter_mut().zip(head) {
        *o = *p;
    }
    let corrected = if pc.fhec() {
        let c = fhec::decode(&octets)?;
        octets = c.octets();
        c.corrected_symbols()
    } else {
        CorrectedSymbols::Zero
    };
    Ok(Received {
        octets,
        len,
        corrected,
    })
}

/// `UnsupportedVersion` (`frame_version`) unless the first header octet has version `01`
/// (732.0 §4.1.2.2.2.2; AOS-011).
pub(crate) const fn check_version(octet0: u8) -> Result<()> {
    let version = octet0 >> 6;
    if version == VERSION {
        Ok(())
    } else {
        Err(mismatch(
            ErrorKind::UnsupportedVersion,
            field::FRAME_VERSION,
            VERSION_U64,
            version_u64(version),
        ))
    }
}

/// `VERSION` as the `u64` of an error's detail.
const VERSION_U64: u64 = 1;

/// A 2-bit version as the `u64` of an error's detail, in `const` context.
#[expect(clippy::as_conversions, reason = "lossless widening u8 -> u64")]
const fn version_u64(version: u8) -> u64 {
    version as u64
}

impl AosPrimaryHeader {
    /// Decodes the Primary Header at the start of `prefix` with the parameters of `pc`; octets
    /// after it are ignored (732.0 §4.1.2; AOS-010…AOS-014, API-014).
    ///
    /// With the FHEC configured, up to two symbol errors in header bits 0–15 and 40–63 are
    /// corrected before any field is read, and the count is reported by
    /// [`corrected_symbols`](Self::corrected_symbols) (AOS-014). The SCID is read as `pc`'s
    /// edition defines it (AOS-002). Checks that depend on the decoding mode, the VC Frame
    /// Count Cycle while unused (AOS-012) and the spare bits 42–43 of Issue 4 (AOS-002), are
    /// made by [`AosFrame::decode`](crate::view::AosFrame::decode); here they are exposed raw.
    ///
    /// # Errors
    /// - `Incomplete` (`needed` = 6, or 8 with the FHEC) if `prefix` is shorter than the
    ///   header;
    /// - `HeaderUncorrectable` if the FHEC detects more errors than it can correct (AOS-014);
    /// - `UnsupportedVersion` (`frame_version`) if the Transfer Frame Version Number is not
    ///   `01` (AOS-011).
    pub fn decode(prefix: &[u8], pc: &AosPhysicalChannelConfig) -> Result<AosPrimaryHeader> {
        let received = correct(prefix, pc)?;
        check_version(received.octets[0])?;
        Ok(AosPrimaryHeader::parse(&received, pc.edition()))
    }

    /// The fields of corrected header octets whose version is checked.
    pub(crate) fn parse(received: &Received, edition: AosEdition) -> AosPrimaryHeader {
        let [o0, o1, o2, o3, o4, o5, o6, o7] = received.octets;
        let lsb = (u16::from(o0 & 0x3F) << 2) | u16::from(o1 >> 6);
        let bits_42_43 = (o5 >> 4) & 0b11;
        let (scid, spare) = match edition {
            AosEdition::Issue5 => ((u16::from(bits_42_43) << 8) | lsb, 0),
            AosEdition::Issue4Compatible => (lsb, bits_42_43),
        };
        AosPrimaryHeader {
            edition,
            scid: SpacecraftId::new(scid).unwrap_or(SCID_ZERO),
            vcid: VcId::new(o1 & 0x3F).unwrap_or(VCID_ZERO),
            vc_frame_count: VcFrameCount(u32::from_be_bytes([0, o2, o3, o4])),
            replay_flag: o5 & 0x80 != 0,
            cycle_use_flag: o5 & 0x40 != 0,
            cycle: FrameCountCycle(o5 & 0x0F),
            spare,
            fhec: (received.len > PRIMARY_HEADER_LEN).then_some(u16::from_be_bytes([o6, o7])),
            corrected: received.corrected,
        }
    }

    /// The header a writer puts in a frame of `pc`: spare bits zero, the FHEC computed when
    /// configured, nothing corrected (732.0 §4.1.2, §4.2.7.3; AOS-002, AOS-019, GEN-052).
    ///
    /// The caller has checked that `scid` fits the edition.
    pub(crate) fn for_writer(
        pc: &AosPhysicalChannelConfig,
        scid: SpacecraftId<10>,
        vcid: VcId<6>,
        vc_frame_count: VcFrameCount,
        replay_flag: bool,
        cycle: Option<FrameCountCycle>,
    ) -> AosPrimaryHeader {
        let mut header = AosPrimaryHeader {
            edition: pc.edition(),
            scid,
            vcid,
            vc_frame_count,
            replay_flag,
            cycle_use_flag: cycle.is_some(),
            cycle: cycle.unwrap_or_default(),
            spare: 0,
            fhec: None,
            corrected: CorrectedSymbols::Zero,
        };
        if pc.fhec() {
            let [o0, o1, o2, o3, o4, o5, _, _] = header.to_octets();
            header.fhec = Some(u16::from_be_bytes(fhec::encode(&[o0, o1, o2, o3, o4, o5])));
        }
        header
    }

    /// The edition the header was read in (AOS-001).
    #[must_use]
    pub const fn edition(&self) -> AosEdition {
        self.edition
    }

    /// Spacecraft Identifier: 10 bits in `Issue5`, the 8 bits 2–9 in `Issue4Compatible`
    /// (732.0 §4.1.2.2.3; AOS-002).
    #[must_use]
    pub const fn scid(&self) -> SpacecraftId<10> {
        self.scid
    }

    /// Virtual Channel Identifier; 63 for an OID frame (732.0 §4.1.2.3).
    #[must_use]
    pub const fn vcid(&self) -> VcId<6> {
        self.vcid
    }

    /// Virtual Channel Frame Count; not protected by the FHEC and never corrected (732.0
    /// §4.1.2.4, §4.1.2.6.1 note 2; AOS-014).
    #[must_use]
    pub const fn vc_frame_count(&self) -> VcFrameCount {
        self.vc_frame_count
    }

    /// Replay Flag: `false` realtime, `true` replay (732.0 §4.1.2.5.2).
    #[must_use]
    pub const fn replay_flag(&self) -> bool {
        self.replay_flag
    }

    /// Virtual Channel Frame Count Cycle Use Flag (732.0 §4.1.2.5.3).
    #[must_use]
    pub const fn cycle_use_flag(&self) -> bool {
        self.cycle_use_flag
    }

    /// Bits 44–47 as received, whether or not the Use Flag is set (732.0 §4.1.2.5.5).
    #[must_use]
    pub const fn cycle_field(&self) -> FrameCountCycle {
        self.cycle
    }

    /// The Virtual Channel Frame Count Cycle if its Use Flag is set, `None` if the field is not
    /// used and so ignored (732.0 §4.1.2.5.3.2; AOS-012).
    #[must_use]
    pub const fn vc_frame_count_cycle(&self) -> Option<FrameCountCycle> {
        if self.cycle_use_flag {
            Some(self.cycle)
        } else {
            None
        }
    }

    /// In `Issue4Compatible`, bits 42–43, the Reserved Spare, as received; always 0 in
    /// `Issue5`, where they are the SCID's two most significant bits (732.0-B-4 §4.1.2.5.4;
    /// AOS-002).
    #[must_use]
    pub const fn issue4_spare(&self) -> u8 {
        self.spare
    }

    /// The Frame Header Error Control after correction, if the physical channel has one (732.0
    /// §4.1.2.6).
    #[must_use]
    pub const fn fhec(&self) -> Option<u16> {
        self.fhec
    }

    /// How many symbols the FHEC corrected: 0 without the FHEC (AOS-014, M2 plan D9).
    #[must_use]
    pub const fn corrected_symbols(&self) -> CorrectedSymbols {
        self.corrected
    }

    /// Whether the frame is an OID Transfer Frame: VCID 63 (732.0 §4.1.4.1.5.1; AOS-016).
    #[must_use]
    pub const fn is_oid(&self) -> bool {
        self.vcid.get() == OID_VCID.get()
    }

    /// Header length: 6 octets, or 8 with the FHEC (732.0 §4.1.2.1).
    #[must_use]
    pub const fn len(&self) -> usize {
        if self.fhec.is_some() {
            PRIMARY_HEADER_LEN.saturating_add(FHEC_LEN)
        } else {
            PRIMARY_HEADER_LEN
        }
    }

    /// Always `false`: a header has six or eight octets.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        false
    }

    /// The header octets, after correction: the first [`len`](Self::len) of the eight, the
    /// rest zero (732.0 §4.1.2; API-014).
    #[must_use]
    pub const fn to_octets(&self) -> [u8; 8] {
        let [_, hi, lo] = scid_octets(self.scid.get());
        let bits_42_43 = match self.edition {
            AosEdition::Issue5 => hi & 0b11,
            AosEdition::Issue4Compatible => self.spare & 0b11,
        };
        let [_, c2, c3, c4] = self.vc_frame_count.0.to_be_bytes();
        let o5 = flag(self.replay_flag, 7)
            | flag(self.cycle_use_flag, 6)
            | (bits_42_43 << 4)
            | (self.cycle.0 & 0x0F);
        let [f0, f1] = match self.fhec {
            Some(f) => f.to_be_bytes(),
            None => [0, 0],
        };
        [
            (VERSION << 6) | (lo >> 2),
            ((lo & 0b11) << 6) | (self.vcid.get() & 0x3F),
            c2,
            c3,
            c4,
            o5,
            f0,
            f1,
        ]
    }
}

/// `[0, high, low]` of a 16-bit SCID, in `const` context.
const fn scid_octets(scid: u16) -> [u8; 3] {
    let [hi, lo] = scid.to_be_bytes();
    [0, hi, lo]
}

/// `1 << bit` if `set`, else 0.
const fn flag(set: bool, bit: u32) -> u8 {
    if set { 1 << bit } else { 0 }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{
        AosPrimaryHeader, FrameCountCycle, OID_VCID, VcFrameCount, check_version, correct,
    };
    use crate::config::AosEdition;
    use crate::config::tests::{pc, scid, vcid};
    use crate::fhec::CorrectedSymbols;
    use crate::field;
    use aldis_core::{ErrorDetail, ErrorKind};

    #[test]
    fn counts_are_range_checked() {
        // req: API-001, AOS-019
        assert_eq!(
            VcFrameCount::new(0xFF_FFFF).map(VcFrameCount::get),
            Some(0xFF_FFFF)
        );
        assert_eq!(VcFrameCount::new(0x100_0000), None);
        assert_eq!(VcFrameCount::try_from(5).map(u32::from), Ok(5));
        let e = VcFrameCount::try_from(0x100_0000).unwrap_err();
        assert_eq!(
            (e.kind(), e.field(), e.detail()),
            (
                ErrorKind::OutOfRange,
                Some(field::VCF_COUNT),
                ErrorDetail::Expected {
                    expected: 0xFF_FFFF,
                    found: 0x100_0000
                }
            )
        );
        assert_eq!(FrameCountCycle::new(15).map(FrameCountCycle::get), Some(15));
        assert_eq!(FrameCountCycle::new(16), None);
        assert_eq!(FrameCountCycle::try_from(7).map(u8::from), Ok(7));
        let e = FrameCountCycle::try_from(16).unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::OutOfRange, Some(field::VC_FRAME_COUNT_CYCLE))
        );
        assert_eq!(VcFrameCount::default().get(), 0);
        assert_eq!(FrameCountCycle::default().get(), 0);
    }

    #[test]
    fn header_bits_follow_figure_4_2() {
        // req: AOS-002, AOS-010, AOS-011, AOS-012, API-014
        // Version 01, SCID 0x2A5 = 10 1010 0101: LSBs 1010 0101 in bits 2–9, MSBs 10 in bits
        // 42–43; VCID 0x15; count 0x123456; replay 1, use flag 1, cycle 9.
        // Octet 0 = 01 101001, octet 1 = 01 010101, octet 5 = 1 1 10 1001.
        let octets = [0x69, 0x55, 0x12, 0x34, 0x56, 0xE9];
        let phy = pc(16, false, 0, false, AosEdition::Issue5);
        let h = AosPrimaryHeader::decode(&octets, &phy).unwrap();
        assert_eq!((h.scid(), h.vcid()), (scid(0x2A5), vcid(0x15)));
        assert_eq!(h.vc_frame_count().get(), 0x12_3456);
        assert!(h.replay_flag() && h.cycle_use_flag());
        assert_eq!(h.vc_frame_count_cycle(), FrameCountCycle::new(9));
        assert_eq!(
            (h.cycle_field().get(), h.issue4_spare(), h.fhec()),
            (9, 0, None)
        );
        assert_eq!(
            (h.edition(), h.corrected_symbols()),
            (AosEdition::Issue5, CorrectedSymbols::Zero)
        );
        assert_eq!((h.len(), h.is_empty(), h.is_oid()), (6, false, false));
        assert_eq!(h.to_octets(), [0x69, 0x55, 0x12, 0x34, 0x56, 0xE9, 0, 0]);
        // Issue 4: the same octets have SCID 0xA5 and the spare 10.
        let p4 = pc(16, false, 0, false, AosEdition::Issue4Compatible);
        let h4 = AosPrimaryHeader::decode(&octets, &p4).unwrap();
        assert_eq!((h4.scid(), h4.issue4_spare()), (scid(0xA5), 0b10));
        assert_eq!(h4.edition(), AosEdition::Issue4Compatible);
        assert_eq!(h4.to_octets(), h.to_octets());
        // Use flag 0: the cycle field is ignored, but kept raw.
        let unused = AosPrimaryHeader::decode(&[0x40, 0x3F, 0, 0, 0, 0x05], &phy).unwrap();
        assert_eq!(
            (unused.vc_frame_count_cycle(), unused.cycle_field().get()),
            (None, 5)
        );
        assert!(unused.is_oid() && !unused.replay_flag());
        assert_eq!(unused.vcid(), OID_VCID);
    }

    #[test]
    fn fhec_corrects_before_fields_are_read() {
        // req: AOS-013, AOS-014, AOS-010
        // 40 01 00 00 00 00 3E DF (test-vectors/aos/fhec_encode.json, aos-fhec-enc-002).
        let phy = pc(16, true, 0, false, AosEdition::Issue5);
        let clean = [0x40, 0x01, 0xAB, 0xCD, 0xEF, 0x00, 0x3E, 0xDF];
        let h = AosPrimaryHeader::decode(&clean, &phy).unwrap();
        assert_eq!((h.vcid(), h.fhec(), h.len()), (vcid(1), Some(0x3EDF), 8));
        assert_eq!(h.corrected_symbols(), CorrectedSymbols::Zero);
        assert_eq!(h.to_octets(), clean);
        // The version symbol and a VCID symbol hit: corrected, frame count untouched.
        let mut bad = clean;
        bad[0] = 0xF0;
        bad[1] = 0x0E;
        let h = AosPrimaryHeader::decode(&bad, &phy).unwrap();
        assert_eq!(
            (h.vcid(), h.corrected_symbols()),
            (vcid(1), CorrectedSymbols::Two)
        );
        assert_eq!(h.vc_frame_count().get(), 0xAB_CDEF);
        assert_eq!(h.to_octets(), clean);
        let received = correct(&bad, &phy).unwrap();
        assert_eq!((received.as_slice(), received.len), (&clean[..], 8));
        let e = AosPrimaryHeader::decode(&[0x51, 0x11, 0, 0, 0, 0, 0x3E, 0xDF], &phy).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::HeaderUncorrectable);
    }

    #[test]
    fn header_rejects_version_and_short_input() {
        // req: AOS-011, GEN-041, API-013
        let phy = pc(16, false, 0, false, AosEdition::Issue5);
        for (octet, found) in [(0x00, 0), (0x80, 2), (0xC0, 3)] {
            let e = AosPrimaryHeader::decode(&[octet, 0, 0, 0, 0, 0], &phy).unwrap_err();
            assert_eq!(
                (e.kind(), e.field(), e.detail()),
                (
                    ErrorKind::UnsupportedVersion,
                    Some(field::FRAME_VERSION),
                    ErrorDetail::Expected { expected: 1, found }
                )
            );
        }
        assert!(check_version(0x7F).is_ok());
        let e = AosPrimaryHeader::decode(&[0x40; 5], &phy).unwrap_err();
        assert_eq!(
            (e.kind(), e.detail()),
            (ErrorKind::Incomplete, ErrorDetail::Needed(6))
        );
        let with_fhec = pc(16, true, 0, false, AosEdition::Issue5);
        let e = AosPrimaryHeader::decode(&[0x40; 7], &with_fhec).unwrap_err();
        assert_eq!(
            (e.kind(), e.detail()),
            (ErrorKind::Incomplete, ErrorDetail::Needed(8))
        );
    }

    #[test]
    fn writer_header_computes_the_fhec() {
        // req: AOS-019, AOS-002, GEN-052
        let phy = pc(16, true, 0, false, AosEdition::Issue5);
        let h = AosPrimaryHeader::for_writer(
            &phy,
            scid(0),
            vcid(1),
            VcFrameCount::new(0).unwrap(),
            false,
            None,
        );
        assert_eq!(h.to_octets(), [0x40, 0x01, 0, 0, 0, 0, 0x3E, 0xDF]);
        assert_eq!(AosPrimaryHeader::decode(&h.to_octets(), &phy), Ok(h));
        // Issue 4: bits 42–43 are written 00 whatever the SCID's upper bits.
        let p4 = pc(16, false, 0, false, AosEdition::Issue4Compatible);
        let cycle = FrameCountCycle::new(3);
        let h4 = AosPrimaryHeader::for_writer(
            &p4,
            scid(0xFF),
            vcid(63),
            VcFrameCount::new(1).unwrap(),
            true,
            cycle,
        );
        assert_eq!(h4.to_octets(), [0x7F, 0xFF, 0, 0, 1, 0xC3, 0, 0]);
        assert_eq!(AosPrimaryHeader::decode(&h4.to_octets(), &p4), Ok(h4));
        let p5 = pc(16, false, 0, false, AosEdition::Issue5);
        let h5 = AosPrimaryHeader::for_writer(
            &p5,
            scid(0x3FF),
            vcid(0),
            VcFrameCount::new(0).unwrap(),
            false,
            cycle,
        );
        assert_eq!(h5.to_octets()[..6], [0x7F, 0xC0, 0, 0, 0, 0x73]);
    }
}

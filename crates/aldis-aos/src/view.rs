//! Zero-copy AOS Transfer Frame view (732.0-B-5 §4.1, §4.3.7, §6.3; AOS-010…AOS-017, API-010,
//! API-012…API-014, API-030…API-032, API-072, GEN-013, GEN-043).

use core::ops::Range;

use aldis_core::crc::{crc16, crc16_update};
use aldis_core::ids::{SpacecraftId, VcId};
use aldis_core::layout::{FECF_LEN, FrameLayout, OCF_LEN};
use aldis_core::ocf::Ocf;
use aldis_core::security::SecurityLayout;
use aldis_core::{AnomalySet, DecodeMode, Error, ErrorKind, Result};

use crate::anomaly::{AosAnomaly, Deviations};
use crate::bpdu::Bpdu;
use crate::config::{
    AosPhysicalChannelConfig, DataFieldContent, VcLookup, frame_layout, unknown_channel,
};
use crate::fhec::CorrectedSymbols;
use crate::field;
use crate::header::{
    AosPrimaryHeader, FrameCountCycle, Received, VcFrameCount, check_version, correct, len_u64,
    mismatch,
};
use crate::mpdu::Mpdu;

/// The content of a frame's data field, as its virtual channel's Data Field Content says
/// (732.0 §4.1.4.1.3–5; AOS-017).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DataUnit<'a> {
    /// An `M_PDU` (§4.1.4.2).
    Mpdu(Mpdu<'a>),
    /// A `B_PDU` (§4.1.4.3).
    Bpdu(Bpdu<'a>),
    /// A `VCA_SDU`, the whole data field (§4.1.4.1.3).
    VcaSdu(&'a [u8]),
    /// Idle Data, the whole data field: an OID frame's (VCID 63, §4.1.4.1.5) or that of a VC
    /// whose content is Idle Data (Table 5-3).
    IdleData(&'a [u8]),
}

impl<'a> DataUnit<'a> {
    /// The Data Field Content this unit is (Table 5-3).
    #[must_use]
    pub const fn content(&self) -> DataFieldContent {
        match self {
            DataUnit::Mpdu(_) => DataFieldContent::Mpdu,
            DataUnit::Bpdu(_) => DataFieldContent::Bpdu,
            DataUnit::VcaSdu(_) => DataFieldContent::VcaSdu,
            DataUnit::IdleData(_) => DataFieldContent::IdleData,
        }
    }

    /// The octets after the `M_PDU` or `B_PDU` Header: the Packet Zone, the Bitstream Data Zone, or
    /// the whole data field of a `VCA_SDU` or Idle Data (GEN-013).
    #[must_use]
    pub const fn zone(&self) -> &'a [u8] {
        match self {
            DataUnit::Mpdu(m) => m.packet_zone(),
            DataUnit::Bpdu(b) => b.data_zone(),
            DataUnit::VcaSdu(octets) | DataUnit::IdleData(octets) => octets,
        }
    }
}

/// A decoded AOS Transfer Frame borrowing its fields from the input (732.0 §4.1, §6.3;
/// AOS-010…AOS-017, GEN-013).
///
/// Holding one guarantees that the frame has the configured length, a header the FHEC accepted
/// (if configured), a correct FECF over the corrected header (if configured), version `01`, a
/// valid master and virtual channel, a non-empty data field, and an `M_PDU` or `B_PDU` pointer
/// inside its zone (API-010).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AosFrame<'a> {
    bytes: &'a [u8],
    header: AosPrimaryHeader,
    layout: FrameLayout,
    unit: DataUnit<'a>,
    anomalies: AnomalySet<AosAnomaly>,
}

impl<'a> AosFrame<'a> {
    /// Decodes one frame of the physical channel `pc`, taking the parameters of its virtual
    /// channel from `vcs` (732.0 §4.1, §4.3.7, §6.3; AOS-010…AOS-017, API-012, API-070).
    ///
    /// The receiving end corrects the header with the FHEC first and then checks the FECF over
    /// the frame with the corrected header (§4.3.7.2–3; AOS-014). An OID frame (VCID 63) needs
    /// only a valid SCID: it has no `VC_OCF` and no SDLS fields (§4.1.4.1.5; OI-43 item 1).
    ///
    /// # Errors
    /// In this order:
    /// - `LengthMismatch` (`frame_len`) if `bytes` is not the configured frame length (API-012);
    /// - `HeaderUncorrectable` if the FHEC is configured and detects more errors than it
    ///   corrects (AOS-014);
    /// - `CrcMismatch` if the FECF is configured and wrong (§4.3.7.3);
    /// - `UnsupportedVersion` (`frame_version`) for a version other than `01` (AOS-011);
    /// - `UnknownChannel` (`spacecraft_id` or `vc_id`) for a channel `vcs` does not hold
    ///   (§4.3.5.4, §4.3.6.3);
    /// - `Malformed` (`frame_data_field`) if the fields leave no data octet, or an `M_PDU` or
    ///   `B_PDU` no octet after its header, which a `vcs` built for `pc` never allows (CMN-041);
    /// - the pointer errors of [`Mpdu::decode`] and [`Bpdu::decode`], in both modes (AOS-017);
    /// - in strict mode, for the first deviation found: `InvalidField` (`vc_frame_count_cycle`)
    ///   for a cycle other than `0000` with its Use Flag `0` (AOS-012); `ReservedValue`
    ///   (`aos_header_spare`) for Issue 4 bits 42–43 other than `00` (AOS-002); the spare errors
    ///   of the `M_PDU` and `B_PDU`. Lenient mode reports them as [`AosAnomaly`] flags (API-032).
    pub fn decode<L: VcLookup + ?Sized>(
        bytes: &'a [u8],
        pc: &AosPhysicalChannelConfig,
        vcs: &L,
        mode: DecodeMode,
    ) -> Result<AosFrame<'a>> {
        let header = AosFrame::check(bytes, pc)?;
        let (content, security, ocf) = if header.is_oid() {
            if !vcs.has_spacecraft(header.scid()) {
                return Err(Error::new(ErrorKind::UnknownChannel).with_field(field::SPACECRAFT_ID));
            }
            (DataFieldContent::IdleData, SecurityLayout::NONE, false)
        } else {
            let vc = vcs
                .vc(header.scid(), header.vcid())
                .ok_or_else(|| unknown_channel(vcs, header.scid()))?;
            (vc.content(), vc.security(), vc.ocf())
        };
        AosFrame::parse(bytes, pc, header, content, security, ocf, mode)
    }

    /// Length of the frame that starts `prefix`: the configured frame length (732.0 §4.1.1.2;
    /// API-013, M2 plan D14). The length depends on no field, so any prefix will do; the header
    /// it holds is checked as [`decode`](Self::decode) checks it, so the two never disagree on a
    /// header: with the FHEC configured once all eight header octets are present, corrected
    /// first, and without it the version as soon as the first octet is present.
    ///
    /// # Errors
    /// `HeaderUncorrectable` (AOS-014); `UnsupportedVersion` (`frame_version`) if the version is
    /// not `01` (AOS-011).
    pub fn peek_length(prefix: &[u8], pc: &AosPhysicalChannelConfig) -> Result<usize> {
        match AosPrimaryHeader::decode(prefix, pc) {
            Ok(_) => Ok(pc.frame_len()),
            Err(e) if e.kind() == ErrorKind::Incomplete => {
                if !pc.fhec()
                    && let Some(&octet) = prefix.first()
                {
                    check_version(octet)?;
                }
                Ok(pc.frame_len())
            }
            Err(e) => Err(e),
        }
    }

    /// The length check, the FHEC, the FECF over the corrected header, and the version
    /// (API-012, AOS-011, AOS-014). Not generic, like [`parse`](Self::parse), so that one copy
    /// of each is measured (VER-024).
    fn check(bytes: &[u8], pc: &AosPhysicalChannelConfig) -> Result<AosPrimaryHeader> {
        if bytes.len() != pc.frame_len() {
            return Err(mismatch(
                ErrorKind::LengthMismatch,
                field::FRAME_LEN,
                len_u64(pc.frame_len()),
                len_u64(bytes.len()),
            ));
        }
        let received = correct(bytes, pc)?;
        if pc.fecf() {
            check_fecf(bytes, &received)?;
        }
        check_version(received.octets[0])?;
        Ok(AosPrimaryHeader::parse(&received, pc.edition()))
    }

    /// Everything after the channel lookup.
    fn parse(
        bytes: &'a [u8],
        pc: &AosPhysicalChannelConfig,
        header: AosPrimaryHeader,
        content: DataFieldContent,
        security: SecurityLayout,
        ocf: bool,
        mode: DecodeMode,
    ) -> Result<AosFrame<'a>> {
        let layout = frame_layout(pc, security, ocf)
            .map_err(|_| Error::new(ErrorKind::Malformed).with_field(field::FRAME_DATA_FIELD))?;
        let mut deviations = Deviations::new(mode);
        let cycle = header.cycle_field();
        if !header.cycle_use_flag() && cycle != FrameCountCycle::default() {
            deviations.note(
                AosAnomaly::CycleNonZeroWhileUnused,
                mismatch(
                    ErrorKind::InvalidField,
                    field::VC_FRAME_COUNT_CYCLE,
                    0,
                    u64::from(cycle.get()),
                ),
            );
        }
        let spare = header.issue4_spare();
        if spare != 0 {
            deviations.note(
                AosAnomaly::Issue4SpareSet,
                mismatch(
                    ErrorKind::ReservedValue,
                    field::AOS_HEADER_SPARE,
                    0,
                    u64::from(spare),
                ),
            );
        }
        let data = bytes.get(layout.data()).unwrap_or_default();
        let unit = match content {
            DataFieldContent::Mpdu => {
                DataUnit::Mpdu(Mpdu::parse(data, pc.edition(), &mut deviations)?)
            }
            DataFieldContent::Bpdu => DataUnit::Bpdu(Bpdu::parse(data, &mut deviations)?),
            DataFieldContent::VcaSdu => DataUnit::VcaSdu(data),
            DataFieldContent::IdleData => DataUnit::IdleData(data),
        };
        let anomalies = deviations.finish()?;
        Ok(AosFrame {
            bytes,
            header,
            layout,
            unit,
            anomalies,
        })
    }

    /// The octets of `range`, which lies inside the frame by construction (CMN-040).
    fn slice(&self, range: Range<usize>) -> &'a [u8] {
        self.bytes.get(range).unwrap_or_default()
    }

    /// The Primary Header as owned values, after FHEC correction (732.0 §4.1.2; API-014).
    #[must_use]
    pub const fn header(&self) -> AosPrimaryHeader {
        self.header
    }

    /// Spacecraft Identifier (732.0 §4.1.2.2.3; AOS-002).
    #[must_use]
    pub const fn scid(&self) -> SpacecraftId<10> {
        self.header.scid()
    }

    /// Virtual Channel Identifier (732.0 §4.1.2.3).
    #[must_use]
    pub const fn vcid(&self) -> VcId<6> {
        self.header.vcid()
    }

    /// Virtual Channel Frame Count (732.0 §4.1.2.4).
    #[must_use]
    pub const fn vc_frame_count(&self) -> VcFrameCount {
        self.header.vc_frame_count()
    }

    /// Replay Flag (732.0 §4.1.2.5.2).
    #[must_use]
    pub const fn replay_flag(&self) -> bool {
        self.header.replay_flag()
    }

    /// The Virtual Channel Frame Count Cycle, `None` if its Use Flag is `0` (AOS-012).
    #[must_use]
    pub const fn vc_frame_count_cycle(&self) -> Option<FrameCountCycle> {
        self.header.vc_frame_count_cycle()
    }

    /// How many header symbols the FHEC corrected, in both modes (AOS-014).
    #[must_use]
    pub const fn corrected_symbols(&self) -> CorrectedSymbols {
        self.header.corrected_symbols()
    }

    /// Whether the frame is an OID Transfer Frame, VCID 63 (732.0 §4.1.4.1.5; AOS-016).
    #[must_use]
    pub const fn is_oid(&self) -> bool {
        self.header.is_oid()
    }

    /// The Insert Zone, of the configured length; empty without one (732.0 §4.1.3; AOS-015).
    #[must_use]
    pub fn insert_zone(&self) -> &'a [u8] {
        self.slice(self.layout.secondary())
    }

    /// The Security Header, opaque; empty without SDLS (732.0 §6.3.4; API-072).
    #[must_use]
    pub fn security_header(&self) -> &'a [u8] {
        self.slice(self.layout.security_header())
    }

    /// The Transfer Frame Data Field, never empty (732.0 §4.1.4.1; GEN-013).
    #[must_use]
    pub fn data_field(&self) -> &'a [u8] {
        self.slice(self.layout.data())
    }

    /// The data field as its virtual channel's content (AOS-017).
    #[must_use]
    pub const fn data_unit(&self) -> DataUnit<'a> {
        self.unit
    }

    /// The Security Trailer, opaque; empty without SDLS (732.0 §6.3.6; API-072).
    #[must_use]
    pub fn security_trailer(&self) -> &'a [u8] {
        self.slice(self.layout.security_trailer())
    }

    /// The Operational Control Field, classified, if the virtual channel has one (732.0 §4.1.5;
    /// CMN-010). A Type-1 Report is decoded with `aldis-tc` (CMN-011).
    #[must_use]
    pub fn ocf(&self) -> Option<Ocf> {
        <[u8; OCF_LEN]>::try_from(self.slice(self.layout.ocf()))
            .ok()
            .map(Ocf::decode)
    }

    /// The Frame Error Control Field, if the physical channel has one (732.0 §4.1.6).
    #[must_use]
    pub fn fecf(&self) -> Option<u16> {
        <[u8; FECF_LEN]>::try_from(self.slice(self.layout.fecf()))
            .ok()
            .map(u16::from_be_bytes)
    }

    /// The octet ranges of the frame's fields, for a security hook (CMN-040, API-074).
    #[must_use]
    pub const fn layout(&self) -> &FrameLayout {
        &self.layout
    }

    /// The whole frame as received; its header octets may differ from the corrected
    /// [`header`](Self::header) (732.0 §4.1.1).
    #[must_use]
    pub const fn as_bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// Octets consumed: the frame length (732.0 §4.1.1.2; API-012).
    #[must_use]
    pub const fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Always `false`: a frame holds at least its Primary Header and one data octet.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        false
    }

    /// Lenient-mode anomalies of the header and the data unit; always empty after a strict
    /// decode (API-032, GEN-043).
    #[must_use]
    pub const fn anomalies(&self) -> AnomalySet<AosAnomaly> {
        self.anomalies
    }
}

/// `CrcMismatch` unless the FECF in the last two octets of `bytes` is the CRC of the corrected
/// header followed by the rest of the frame (732.0 §4.1.6.3, §4.3.7.2–3; AOS-014).
fn check_fecf(bytes: &[u8], received: &Received) -> Result<()> {
    let end = bytes.len().saturating_sub(FECF_LEN);
    let rest = bytes.get(received.len..end).unwrap_or_default();
    let crc = crc16_update(crc16(received.as_slice()), rest);
    let fecf = bytes
        .get(end..)
        .and_then(|f| <[u8; FECF_LEN]>::try_from(f).ok())
        .map(u16::from_be_bytes);
    if fecf == Some(crc) {
        Ok(())
    } else {
        Err(Error::new(ErrorKind::CrcMismatch))
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
pub(crate) mod tests {
    extern crate std;
    use std::vec;
    use std::vec::Vec;

    use super::{AosFrame, DataUnit};
    use crate::anomaly::AosAnomaly;
    use crate::bpdu::BitstreamPointer;
    use crate::config::tests::{pc, scid, vc, vcid};
    use crate::config::{AosEdition, DataFieldContent, VcTable};
    use crate::fhec::{self, CorrectedSymbols};
    use crate::field;
    use crate::header::FrameCountCycle;
    use crate::mpdu::PacketPointer;
    use aldis_core::crc::write_fecf;
    use aldis_core::ocf::Ocf;
    use aldis_core::security::SecurityLayout;
    use aldis_core::{AnomalySet, DecodeMode, ErrorDetail, ErrorKind, FieldId, Result};

    const STRICT: DecodeMode = DecodeMode::Strict;
    const LENIENT: DecodeMode = DecodeMode::Lenient;
    const ISSUE5: AosEdition = AosEdition::Issue5;
    const ISSUE4: AosEdition = AosEdition::Issue4Compatible;

    /// The six header octets of an Issue 5 frame: version 01, `scid`, `vcid`, `count`, and
    /// `flags` in bits 40–41 and 44–47 of octet 5.
    pub(crate) fn hdr(scid: u16, vcid: u8, count: u32, flags: u8) -> Vec<u8> {
        let lsb = u8::try_from(scid & 0xFF).unwrap();
        let msb = u8::try_from(scid >> 8).unwrap();
        let [_, c2, c3, c4] = count.to_be_bytes();
        vec![
            0x40 | (lsb >> 2),
            ((lsb & 3) << 6) | vcid,
            c2,
            c3,
            c4,
            flags | (msb << 4),
        ]
    }

    fn err(r: Result<AosFrame<'_>>) -> (ErrorKind, Option<FieldId>) {
        let e = r.unwrap_err();
        (e.kind(), e.field())
    }

    /// 16-octet frames, no FHEC, Insert Zone, or FECF; SCID 0x2A5 with VC 5 (`M_PDUs`), VC 6
    /// (`B_PDUs`), VC 7 (`VCA_SDUs`), VC 8 (Idle Data).
    fn plain() -> VcTable<4> {
        VcTable::new(
            pc(16, false, 0, false, ISSUE5),
            &[
                vc(0x2A5, 5, DataFieldContent::Mpdu),
                vc(0x2A5, 6, DataFieldContent::Bpdu),
                vc(0x2A5, 7, DataFieldContent::VcaSdu),
                vc(0x2A5, 8, DataFieldContent::IdleData),
            ],
        )
        .unwrap()
    }

    /// A frame of `plain()`'s VC `v`: the header, then `data`.
    fn frame(v: u8, flags: u8, data: &[u8]) -> Vec<u8> {
        let mut f = hdr(0x2A5, v, 0x0102, flags);
        f.extend_from_slice(data);
        f
    }

    #[test]
    fn minimal_frame_accessors() {
        // req: AOS-010, AOS-017, API-010, API-012, API-014, GEN-013
        let t = plain();
        let phy = t.physical_channel();
        let bytes = frame(
            5,
            0,
            &[0x00, 0x00, 0xD0, 0xD1, 0xD2, 0xD3, 0xD4, 0xD5, 0xD6, 0xD7],
        );
        let f = AosFrame::decode(&bytes, &phy, &t, STRICT).unwrap();
        assert_eq!((f.scid(), f.vcid()), (scid(0x2A5), vcid(5)));
        assert_eq!(f.vc_frame_count().get(), 0x0102);
        assert!(!f.replay_flag() && !f.is_oid());
        assert_eq!(f.vc_frame_count_cycle(), None);
        assert_eq!(f.corrected_symbols(), CorrectedSymbols::Zero);
        assert_eq!(f.data_field(), &bytes[6..]);
        let DataUnit::Mpdu(m) = f.data_unit() else {
            panic!("an M_PDU")
        };
        assert_eq!(m.pointer(), PacketPointer::StartsAt(0));
        assert_eq!(f.data_unit().zone(), &bytes[8..]);
        assert_eq!(
            f.data_unit().zone().as_ptr(),
            bytes[8..].as_ptr(),
            "zero-copy"
        );
        assert_eq!(f.data_unit().content(), DataFieldContent::Mpdu);
        assert!(
            f.insert_zone().is_empty()
                && f.security_header().is_empty()
                && f.security_trailer().is_empty()
        );
        assert_eq!((f.ocf(), f.fecf()), (None, None));
        assert_eq!(
            (f.len(), f.is_empty(), f.as_bytes()),
            (16, false, &bytes[..])
        );
        assert_eq!(f.anomalies(), AnomalySet::empty());
        assert_eq!(f.layout().data(), 6..16);
        assert_eq!(f.header().to_octets()[..6], bytes[..6]);
    }

    #[test]
    fn every_content() {
        // req: AOS-017, AOS-010, GEN-013
        let t = plain();
        let phy = t.physical_channel();
        let bytes = frame(6, 0, &[0x00, 0x09, 0xFF, 0xC0, 0, 0, 0, 0, 0, 0]);
        let f = AosFrame::decode(&bytes, &phy, &t, STRICT).unwrap();
        let DataUnit::Bpdu(b) = f.data_unit() else {
            panic!("a B_PDU")
        };
        assert_eq!(
            (b.pointer(), f.data_unit().content()),
            (BitstreamPointer::ValidBits(10), DataFieldContent::Bpdu)
        );
        assert_eq!(f.data_unit().zone(), &bytes[8..]);
        for (v, content) in [
            (7, DataFieldContent::VcaSdu),
            (8, DataFieldContent::IdleData),
        ] {
            let bytes = frame(v, 0, &[0xFF; 10]);
            let f = AosFrame::decode(&bytes, &phy, &t, STRICT).unwrap();
            assert_eq!(f.data_unit().content(), content);
            assert_eq!(f.data_unit().zone(), &bytes[6..]);
        }
        // An OID frame needs only a valid SCID.
        let oid = frame(63, 0, &[0x5A; 10]);
        let f = AosFrame::decode(&oid, &phy, &t, STRICT).unwrap();
        assert!(f.is_oid());
        assert_eq!(f.data_unit(), DataUnit::IdleData(&oid[6..]));
    }

    #[test]
    fn length_fhec_and_fecf_come_first() {
        // req: API-012, AOS-013, AOS-014, AOS-011, CMN-003
        // 24-octet frames with FHEC and FECF; SCID 0, VC 1 carrying VCA_SDUs.
        let t = VcTable::<1>::new(
            pc(24, true, 0, true, ISSUE5),
            &[vc(0, 1, DataFieldContent::VcaSdu)],
        )
        .unwrap();
        let phy = t.physical_channel();
        let mut good = [0_u8; 24];
        good[..6].copy_from_slice(&[0x40, 0x01, 0xAB, 0xCD, 0xEF, 0x00]);
        good[8..22].fill(0x33);
        let mut h: [u8; 8] = good[..8].try_into().unwrap();
        fhec::write(&mut h);
        good[..8].copy_from_slice(&h);
        write_fecf(&mut good).unwrap();
        let f = AosFrame::decode(&good, &phy, &t, STRICT).unwrap();
        assert_eq!(
            (f.header().fhec(), f.fecf()),
            (Some(0x3EDF), Some(u16::from_be_bytes([good[22], good[23]])))
        );
        assert_eq!(f.data_field(), &[0x33; 14]);
        let e = AosFrame::decode(&good[..23], &phy, &t, STRICT).unwrap_err();
        assert_eq!(
            (e.kind(), e.field(), e.detail()),
            (
                ErrorKind::LengthMismatch,
                Some(field::FRAME_LEN),
                ErrorDetail::Expected {
                    expected: 24,
                    found: 23
                }
            )
        );
        // Two header symbols hit: corrected, and the FECF holds over the corrected header.
        let mut two = good;
        two[0] = 0xC0;
        two[5] = 0x07;
        let f = AosFrame::decode(&two, &phy, &t, STRICT).unwrap();
        assert_eq!(
            (f.corrected_symbols(), f.vcid()),
            (CorrectedSymbols::Two, vcid(1))
        );
        assert_eq!(f.header().to_octets(), h);
        assert_eq!(f.as_bytes()[0], 0xC0, "the input is not changed");
        // Three symbols: uncorrectable, before the FECF.
        // Three symbols that no pattern of two explains (test-vectors/aos/fhec_uncorrectable).
        let mut three = good;
        three[..8].copy_from_slice(&[0x51, 0x11, 0xAB, 0xCD, 0xEF, 0x00, 0x3E, 0xDF]);
        assert_eq!(
            err(AosFrame::decode(&three, &phy, &t, STRICT)).0,
            ErrorKind::HeaderUncorrectable
        );
        // A data octet hit: the FECF fails.
        let mut data = good;
        data[10] ^= 1;
        assert_eq!(
            err(AosFrame::decode(&data, &phy, &t, STRICT)),
            (ErrorKind::CrcMismatch, None)
        );
        // A wrong version under a valid FHEC and FECF.
        let mut version = good;
        version[0] = 0x00;
        let mut h: [u8; 8] = version[..8].try_into().unwrap();
        fhec::write(&mut h);
        version[..8].copy_from_slice(&h);
        write_fecf(&mut version).unwrap();
        assert_eq!(
            err(AosFrame::decode(&version, &phy, &t, STRICT)),
            (ErrorKind::UnsupportedVersion, Some(field::FRAME_VERSION))
        );
    }

    #[test]
    fn unknown_channels() {
        // req: AOS-010, AOS-016, GEN-041
        let t = plain();
        let phy = t.physical_channel();
        let mut other_scid = frame(5, 0, &[0; 10]);
        other_scid[0] ^= 0x01;
        assert_eq!(
            err(AosFrame::decode(&other_scid, &phy, &t, STRICT)),
            (ErrorKind::UnknownChannel, Some(field::SPACECRAFT_ID))
        );
        assert_eq!(
            err(AosFrame::decode(&frame(9, 0, &[0; 10]), &phy, &t, LENIENT)),
            (ErrorKind::UnknownChannel, Some(field::VC_ID))
        );
        let mut oid = frame(63, 0, &[0; 10]);
        oid[0] ^= 0x01;
        assert_eq!(
            err(AosFrame::decode(&oid, &phy, &t, STRICT)),
            (ErrorKind::UnknownChannel, Some(field::SPACECRAFT_ID))
        );
    }

    #[test]
    fn insert_zone_security_and_ocf() {
        // req: AOS-015, API-070, API-072, CMN-010
        // 30 octets: 6 header, 3 Insert Zone, 2 Security Header, 2 M_PDU Header and 10 zone,
        // 1 Security Trailer, 4 OCF, 2 FECF.
        let t = VcTable::<1>::new(
            pc(30, false, 3, true, ISSUE5),
            &[vc(0x2A5, 5, DataFieldContent::Mpdu)
                .with_ocf()
                .with_security(SecurityLayout::new(2, 1).unwrap())],
        )
        .unwrap();
        let phy = t.physical_channel();
        let mut bytes = frame(5, 0, &[0x1A, 0x1B, 0x1C, 0xA5, 0xA5, 0xFF, 0xFF]);
        bytes.extend([0xD0; 10]);
        bytes.extend([0x5A, 0x01, 0x02, 0x03, 0x04, 0, 0]);
        write_fecf(&mut bytes).unwrap();
        let f = AosFrame::decode(&bytes, &phy, &t, STRICT).unwrap();
        assert_eq!(f.insert_zone(), &[0x1A, 0x1B, 0x1C]);
        assert_eq!(
            (f.security_header(), f.security_trailer()),
            (&[0xA5, 0xA5][..], &[0x5A][..])
        );
        assert_eq!(f.data_field().len(), 12);
        assert_eq!(f.data_unit().zone(), &[0xD0; 10]);
        assert_eq!(f.ocf(), Some(Ocf::decode([1, 2, 3, 4])));
        // An OID frame on the same channel keeps the Insert Zone and has no OCF or SDLS.
        let mut oid = frame(63, 0, &[0x1A, 0x1B, 0x1C]);
        oid.extend([0x77; 19]);
        oid.extend([0, 0]);
        write_fecf(&mut oid).unwrap();
        let f = AosFrame::decode(&oid, &phy, &t, STRICT).unwrap();
        assert_eq!(f.insert_zone(), &[0x1A, 0x1B, 0x1C]);
        assert_eq!((f.data_field(), f.ocf()), (&[0x77; 19][..], None));
        assert!(f.security_header().is_empty() && f.security_trailer().is_empty());
    }

    #[test]
    fn pointers_outside_their_zone() {
        // req: AOS-017, API-032
        let t = plain();
        let phy = t.physical_channel();
        for mode in [STRICT, LENIENT] {
            // Packet Zone of 8 octets: offset 8 is outside; even with the cycle deviation.
            let m = frame(5, 0x03, &[0x00, 0x08, 0, 0, 0, 0, 0, 0, 0, 0]);
            assert_eq!(
                err(AosFrame::decode(&m, &phy, &t, mode)),
                (ErrorKind::Malformed, Some(field::FIRST_HEADER_POINTER))
            );
            // Bitstream Data Zone of 64 bits: index 63 is the last bit, 64 outside.
            let b = frame(6, 0, &[0xC0, 63, 0, 0, 0, 0, 0, 0, 0, 0]);
            assert_eq!(
                err(AosFrame::decode(&b, &phy, &t, mode)),
                (ErrorKind::InvalidField, Some(field::BITSTREAM_DATA_POINTER))
            );
            let b = frame(6, 0, &[0x00, 64, 0, 0, 0, 0, 0, 0, 0, 0]);
            assert_eq!(
                err(AosFrame::decode(&b, &phy, &t, mode)),
                (ErrorKind::Malformed, Some(field::BITSTREAM_DATA_POINTER))
            );
        }
    }

    #[test]
    fn strict_and_lenient_deviations() {
        // req: AOS-012, AOS-002, AOS-003, API-030, API-031, API-032, GEN-043
        let t = plain();
        let phy = t.physical_channel();
        // Use Flag 0 with cycle 0101.
        let cycle = frame(5, 0x05, &[0xFF, 0xFF, 0, 0, 0, 0, 0, 0, 0, 0]);
        let e = AosFrame::decode(&cycle, &phy, &t, STRICT).unwrap_err();
        assert_eq!(
            (e.kind(), e.field(), e.detail()),
            (
                ErrorKind::InvalidField,
                Some(field::VC_FRAME_COUNT_CYCLE),
                ErrorDetail::Expected {
                    expected: 0,
                    found: 5
                }
            )
        );
        let f = AosFrame::decode(&cycle, &phy, &t, LENIENT).unwrap();
        assert_eq!(
            f.anomalies(),
            AnomalySet::from_flags(&[AosAnomaly::CycleNonZeroWhileUnused])
        );
        assert_eq!(
            (f.vc_frame_count_cycle(), f.header().cycle_field().get()),
            (None, 5)
        );
        // Use Flag 1: any cycle; replay set.
        let used = frame(5, 0xC5, &[0xFF, 0xFF, 0, 0, 0, 0, 0, 0, 0, 0]);
        let f = AosFrame::decode(&used, &phy, &t, STRICT).unwrap();
        assert_eq!(
            (f.vc_frame_count_cycle(), f.replay_flag()),
            (FrameCountCycle::new(5), true)
        );
        // B_PDU spare and the cycle together: strict reports the header's first.
        let both = frame(6, 0x01, &[0x7F, 0xFF, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(
            err(AosFrame::decode(&both, &phy, &t, STRICT)),
            (ErrorKind::InvalidField, Some(field::VC_FRAME_COUNT_CYCLE))
        );
        let f = AosFrame::decode(&both, &phy, &t, LENIENT).unwrap();
        assert_eq!(
            f.anomalies(),
            AnomalySet::from_flags(&[
                AosAnomaly::CycleNonZeroWhileUnused,
                AosAnomaly::BpduSpareSet
            ])
        );
        let spare_only = frame(6, 0, &[0x7F, 0xFF, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(
            err(AosFrame::decode(&spare_only, &phy, &t, STRICT)),
            (ErrorKind::ReservedValue, Some(field::BPDU_SPARE))
        );
    }

    #[test]
    fn issue_4_spares() {
        // req: AOS-002, AOS-003, GEN-062, API-032
        let t = VcTable::<1>::new(
            pc(12, false, 0, false, ISSUE4),
            &[vc(0xA5, 5, DataFieldContent::Mpdu)],
        )
        .unwrap();
        let phy = t.physical_channel();
        // SCID 0xA5, bits 42–43 = 00, M_PDU spare 00000, FHP 0x7FE.
        let mut good = hdr(0xA5, 5, 0, 0);
        good.extend([0x07, 0xFE, 1, 2, 3, 4]);
        let f = AosFrame::decode(&good, &phy, &t, STRICT).unwrap();
        assert_eq!(f.scid(), scid(0xA5));
        let DataUnit::Mpdu(m) = f.data_unit() else {
            panic!("an M_PDU")
        };
        assert_eq!(
            (m.pointer(), m.first_header_pointer()),
            (PacketPointer::OnlyIdleData, 0x7FE)
        );
        // Bits 42–43 = 01: in Issue 5 an SCID MSB, in Issue 4 a spare.
        let mut header_spare = good.clone();
        header_spare[5] |= 0x10;
        let e = AosFrame::decode(&header_spare, &phy, &t, STRICT).unwrap_err();
        assert_eq!(
            (e.kind(), e.field(), e.detail()),
            (
                ErrorKind::ReservedValue,
                Some(field::AOS_HEADER_SPARE),
                ErrorDetail::Expected {
                    expected: 0,
                    found: 1
                }
            )
        );
        let f = AosFrame::decode(&header_spare, &phy, &t, LENIENT).unwrap();
        assert_eq!((f.scid(), f.header().issue4_spare()), (scid(0xA5), 1));
        assert_eq!(
            f.anomalies(),
            AnomalySet::from_flags(&[AosAnomaly::Issue4SpareSet])
        );
        let mut mpdu_spare = good.clone();
        mpdu_spare[6] |= 0x08;
        assert_eq!(
            err(AosFrame::decode(&mpdu_spare, &phy, &t, STRICT)),
            (ErrorKind::ReservedValue, Some(field::MPDU_SPARE))
        );
        let f = AosFrame::decode(&mpdu_spare, &phy, &t, LENIENT).unwrap();
        assert_eq!(
            f.anomalies(),
            AnomalySet::from_flags(&[AosAnomaly::Issue4SpareSet])
        );
    }

    #[test]
    fn channels_that_do_not_fit_the_physical_channel() {
        // req: CMN-041, API-010, GEN-020
        let t = VcTable::<2>::new(
            pc(32, false, 0, false, ISSUE5),
            &[
                vc(1, 1, DataFieldContent::VcaSdu).with_ocf(),
                vc(1, 2, DataFieldContent::Mpdu).with_ocf(),
            ],
        )
        .unwrap();
        // Decoded with a physical channel of 10-octet frames: no room for the OCF and data,
        // or for an M_PDU Header and a zone octet.
        let small = pc(10, false, 0, false, ISSUE5);
        let mut one = hdr(1, 1, 0, 0);
        one.extend([0; 4]);
        assert_eq!(
            err(AosFrame::decode(&one, &small, &t, STRICT)),
            (ErrorKind::Malformed, Some(field::FRAME_DATA_FIELD))
        );
        let eleven = pc(12, false, 0, false, ISSUE5);
        let mut two = hdr(1, 2, 0, 0);
        two.extend([0; 6]);
        assert_eq!(
            err(AosFrame::decode(&two, &eleven, &t, LENIENT)),
            (ErrorKind::Malformed, Some(field::FRAME_DATA_FIELD))
        );
    }

    #[test]
    fn peek_length_checks_the_header_as_decode_does() {
        // req: API-013, AOS-011, AOS-014
        let phy = pc(1115, false, 0, true, ISSUE5);
        assert_eq!(AosFrame::peek_length(&[], &phy), Ok(1115));
        assert_eq!(AosFrame::peek_length(&[0x7F], &phy), Ok(1115));
        assert_eq!(AosFrame::peek_length(&hdr(1, 1, 0, 0), &phy), Ok(1115));
        let e = AosFrame::peek_length(&[0xC0], &phy).unwrap_err();
        assert_eq!(
            (e.kind(), e.field(), e.detail()),
            (
                ErrorKind::UnsupportedVersion,
                Some(field::FRAME_VERSION),
                ErrorDetail::Expected {
                    expected: 1,
                    found: 3
                }
            )
        );
        let with_fhec = pc(1115, true, 0, true, ISSUE5);
        // Fewer than eight octets: nothing to check yet, not even a wrong version.
        assert_eq!(
            AosFrame::peek_length(&[0xC0, 0, 0, 0, 0, 0, 0], &with_fhec),
            Ok(1115)
        );
        // A correctable version error is corrected first.
        assert_eq!(
            AosFrame::peek_length(&[0xC0, 0x01, 0, 0, 0, 0, 0x3E, 0xDF], &with_fhec),
            Ok(1115)
        );
        assert_eq!(
            AosFrame::peek_length(&[0x51, 0x11, 0, 0, 0, 0, 0x3E, 0xDF], &with_fhec)
                .map_err(|e| e.kind()),
            Err(ErrorKind::HeaderUncorrectable)
        );
    }
}

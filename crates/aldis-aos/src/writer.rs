//! Encoders: one-shot [`encode`], the two-phase [`AosFrameWriter`], and OID frames.
//!
//! 732.0-B-5 §4.1, §4.1.4.1.5, §4.2.7, §6.3; AOS-002, AOS-003, AOS-015, AOS-016, AOS-019,
//! API-020…API-023, API-072, GEN-014, GEN-042, GEN-052.

use core::ops::Range;

use aldis_core::crc::crc16;
use aldis_core::ids::{SpacecraftId, VcId};
use aldis_core::layout::FrameLayout;
use aldis_core::ocf::Ocf;
use aldis_core::oid::OidGenerator;
use aldis_core::security::SecurityLayout;
use aldis_core::{Error, ErrorKind, Result};

use crate::bpdu::BitstreamPointer;
use crate::config::{
    AosPhysicalChannelConfig, DataFieldContent, VcLookup, frame_layout, pdu_header_len,
    unknown_channel,
};
use crate::field;
use crate::header::{AosPrimaryHeader, FrameCountCycle, OID_VCID, VcFrameCount, len_u64, mismatch};
use crate::mpdu::PacketPointer;

/// The `M_PDU` or `B_PDU` Header the writer puts at the start of the data field, or none (732.0
/// §4.1.4.2.2, §4.1.4.3.3; AOS-019).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PduHeader {
    /// An `M_PDU` Header with this First Header Pointer, on a VC that carries `M_PDUs`.
    Mpdu(PacketPointer),
    /// A `B_PDU` Header with this Bitstream Data Pointer, on a VC that carries `B_PDUs`.
    Bpdu(BitstreamPointer),
    /// None: the data field is a `VCA_SDU` or Idle Data, written by the caller, or the OID
    /// data of [`AosFrameWriter::begin_oid`].
    None,
}

impl PduHeader {
    /// Whether this is the data unit header of the frames of a VC carrying `content`.
    const fn fits(self, content: DataFieldContent) -> bool {
        matches!(
            (self, content),
            (PduHeader::Mpdu(_), DataFieldContent::Mpdu)
                | (PduHeader::Bpdu(_), DataFieldContent::Bpdu)
                | (
                    PduHeader::None,
                    DataFieldContent::VcaSdu | DataFieldContent::IdleData
                )
        )
    }
}

/// The Primary Header fields and the data unit header a caller chooses (732.0 §4.1.2; AOS-019).
///
/// The Transfer Frame Version Number, the spare bits, and the FHEC are the writer's (API-021).
/// Frame counts are kept by the engines ([04](../../docs/04-engines.md)).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AosFrameFields {
    /// Spacecraft Identifier of the frame's master channel (732.0 §4.1.2.2.3).
    pub scid: SpacecraftId<10>,
    /// Virtual Channel Identifier: a configured VC for [`AosFrameWriter::begin`] and [`encode`],
    /// 63 for [`AosFrameWriter::begin_oid`] (732.0 §4.1.2.3).
    pub vcid: VcId<6>,
    /// Virtual Channel Frame Count (732.0 §4.1.2.4).
    pub vc_frame_count: VcFrameCount,
    /// Replay Flag (732.0 §4.1.2.5.2).
    pub replay_flag: bool,
    /// The Virtual Channel Frame Count Cycle, which sets the Use Flag; `None` writes the Use
    /// Flag `0` and the cycle `0000` (732.0 §4.1.2.5.3, §4.1.2.5.5.3; AOS-012).
    pub cycle: Option<FrameCountCycle>,
    /// The `M_PDU` or `B_PDU` Header (AOS-017, AOS-019).
    pub pdu: PduHeader,
}

impl AosFrameFields {
    /// A realtime frame of (`scid`, `vcid`) with count `vc_frame_count`, no frame count cycle,
    /// and data unit header `pdu` (AOS-019).
    #[must_use]
    pub const fn new(
        scid: SpacecraftId<10>,
        vcid: VcId<6>,
        vc_frame_count: VcFrameCount,
        pdu: PduHeader,
    ) -> AosFrameFields {
        AosFrameFields {
            scid,
            vcid,
            vc_frame_count,
            replay_flag: false,
            cycle: None,
            pdu,
        }
    }

    /// A realtime OID frame of `scid`: VCID 63, no data unit header (732.0 §4.1.4.1.5;
    /// AOS-016).
    #[must_use]
    pub const fn oid(scid: SpacecraftId<10>, vc_frame_count: VcFrameCount) -> AosFrameFields {
        AosFrameFields::new(scid, OID_VCID, vc_frame_count, PduHeader::None)
    }
}

/// The variable fields of a frame for [`encode`], each exactly the length the configuration
/// gives it (732.0 §4.1, §6.3; AOS-015, AOS-019, API-072).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AosFrameContent<'d> {
    /// The Insert Zone, an `IN_SDU`; empty without one (732.0 §4.1.3).
    pub insert_zone: &'d [u8],
    /// The Security Header, as a security hook filled it; empty without SDLS (732.0 §6.3.4).
    pub security_header: &'d [u8],
    /// The octets after the `M_PDU` or `B_PDU` Header, or the whole data field of a `VCA_SDU` or
    /// Idle Data, filling it exactly (732.0 §4.1.4).
    pub zone: &'d [u8],
    /// The Security Trailer; empty without SDLS (732.0 §6.3.6).
    pub security_trailer: &'d [u8],
    /// The OCF, present exactly when the VC has one (732.0 §4.1.5).
    pub ocf: Option<Ocf>,
}

impl<'d> AosFrameContent<'d> {
    /// A frame holding `zone` and no other variable field.
    #[must_use]
    pub const fn new(zone: &'d [u8]) -> AosFrameContent<'d> {
        AosFrameContent {
            insert_zone: &[],
            security_header: &[],
            zone,
            security_trailer: &[],
            ocf: None,
        }
    }
}

/// A frame checked against its configuration, not yet written.
struct Plan {
    header: AosPrimaryHeader,
    layout: FrameLayout,
    /// The `M_PDU` or `B_PDU` Header octets, if any.
    pdu: Option<[u8; 2]>,
}

impl Plan {
    /// Checks `fields` against `pc` and `vcs` and derives the header (AOS-019).
    fn new<L: VcLookup + ?Sized>(
        pc: &AosPhysicalChannelConfig,
        vcs: &L,
        fields: &AosFrameFields,
    ) -> Result<Plan> {
        if fields.vcid == OID_VCID {
            return Err(mismatch(ErrorKind::InvalidField, field::VC_ID, 62, 63));
        }
        let vc = vcs
            .vc(fields.scid, fields.vcid)
            .ok_or_else(|| unknown_channel(vcs, fields.scid))?;
        if !fields.pdu.fits(vc.content()) {
            return Err(Error::new(ErrorKind::InvalidField).with_field(field::DATA_FIELD_CONTENT));
        }
        let layout = frame_layout(pc, vc.security(), vc.ocf())?;
        let data = layout.data().len();
        let header_len = pdu_header_len(vc.content());
        if data <= header_len {
            return Err(mismatch(
                ErrorKind::InvalidConfig,
                field::FRAME_DATA_FIELD,
                len_u64(header_len.saturating_add(1)),
                len_u64(data),
            ));
        }
        let zone = data.saturating_sub(header_len);
        let pdu = match fields.pdu {
            PduHeader::Mpdu(pointer) => Some(mpdu_header(pointer, zone, pc)?),
            PduHeader::Bpdu(pointer) => Some(bpdu_header(pointer, zone)?),
            PduHeader::None => None,
        };
        Plan::finish(pc, fields, layout, pdu)
    }

    /// The plan of an OID frame of `fields.scid` (732.0 §4.1.4.1.5; AOS-016).
    fn oid<L: VcLookup + ?Sized>(
        pc: &AosPhysicalChannelConfig,
        vcs: &L,
        fields: &AosFrameFields,
    ) -> Result<Plan> {
        if fields.vcid != OID_VCID {
            return Err(mismatch(
                ErrorKind::InvalidField,
                field::VC_ID,
                63,
                u64::from(fields.vcid.get()),
            ));
        }
        if fields.pdu != PduHeader::None {
            return Err(Error::new(ErrorKind::InvalidField).with_field(field::DATA_FIELD_CONTENT));
        }
        if !vcs.has_spacecraft(fields.scid) {
            return Err(Error::new(ErrorKind::UnknownChannel).with_field(field::SPACECRAFT_ID));
        }
        let layout = frame_layout(pc, SecurityLayout::NONE, false)?;
        Plan::finish(pc, fields, layout, None)
    }

    /// The SCID check against the edition, and the header (AOS-002).
    fn finish(
        pc: &AosPhysicalChannelConfig,
        fields: &AosFrameFields,
        layout: FrameLayout,
        pdu: Option<[u8; 2]>,
    ) -> Result<Plan> {
        let max = pc.edition().max_scid();
        if fields.scid.get() > max {
            return Err(Error::out_of_range(
                field::SPACECRAFT_ID,
                u64::from(max),
                u64::from(fields.scid.get()),
            ));
        }
        Ok(Plan {
            header: AosPrimaryHeader::for_writer(
                pc,
                fields.scid,
                fields.vcid,
                fields.vc_frame_count,
                fields.replay_flag,
                fields.cycle,
            ),
            layout,
            pdu,
        })
    }
}

/// The `M_PDU` Header for `pointer` in a Packet Zone of `zone` octets (AOS-003, AOS-019).
///
/// # Errors
/// `OutOfRange` (`first_header_pointer`) for an offset outside the zone or above the edition's
/// largest, with the largest valid offset.
fn mpdu_header(
    pointer: PacketPointer,
    zone: usize,
    pc: &AosPhysicalChannelConfig,
) -> Result<[u8; 2]> {
    let edition = pc.edition();
    if let PacketPointer::StartsAt(n) = pointer {
        let max = usize::from(PacketPointer::max_offset(edition));
        let limit = zone.saturating_sub(1).min(max);
        if usize::from(n) > limit {
            return Err(Error::out_of_range(
                field::FIRST_HEADER_POINTER,
                len_u64(limit),
                u64::from(n),
            ));
        }
    }
    Ok(pointer.header_octets(edition))
}

/// The `B_PDU` Header for `pointer` in a Bitstream Data Zone of `zone` octets (AOS-019).
///
/// # Errors
/// `OutOfRange` (`bitstream_data_pointer`) for `ValidBits(n)` with `n` 0 or not below the
/// zone's length in bits (a zone of valid data only is `AllValid`, §4.1.4.3.3.3), with the
/// largest valid count.
fn bpdu_header(pointer: BitstreamPointer, zone: usize) -> Result<[u8; 2]> {
    if let BitstreamPointer::ValidBits(n) = pointer {
        let limit = zone
            .saturating_mul(8)
            .saturating_sub(1)
            .min(usize::from(BitstreamPointer::ONLY_IDLE_RAW));
        if n == 0 || usize::from(n) > limit {
            return Err(Error::out_of_range(
                field::BITSTREAM_DATA_POINTER,
                len_u64(limit),
                u64::from(n),
            ));
        }
    }
    Ok(pointer.header_octets())
}

/// Copies `src` into `buf` from octet `at`, as far as both reach.
fn write_at(buf: &mut [u8], at: usize, src: &[u8]) {
    for (d, s) in buf.iter_mut().skip(at).zip(src) {
        *d = *s;
    }
}

/// Two-phase frame writer (732.0 §4.1; AOS-019, API-020…API-023, API-072).
///
/// [`begin`](Self::begin) writes the Primary Header with its FHEC and the `M_PDU` or `B_PDU`
/// Header, the caller fills the other fields in place, and [`finish`](Self::finish) computes
/// the FECF.
///
/// `begin` zero-fills every octet after the headers, so until `finish` the frame is defined: its
/// headers final, the fields the caller has not filled zero, and the FECF, if configured, zero
/// (API-023). The FECF is computed last, after a security hook has filled the Security Header and
/// Trailer (API-074).
#[derive(Debug)]
pub struct AosFrameWriter<'b> {
    frame: &'b mut [u8],
    layout: FrameLayout,
    zone_start: usize,
}

impl<'b> AosFrameWriter<'b> {
    /// Starts a frame of (`fields.scid`, `fields.vcid`) in the first frame-length octets of
    /// `out` (AOS-019).
    ///
    /// # Errors
    /// On error `out` is unchanged (API-022, GEN-042):
    /// - `InvalidField` (`vc_id`) for VCID 63, whose frames [`begin_oid`](Self::begin_oid)
    ///   writes;
    /// - `UnknownChannel` (`spacecraft_id` or `vc_id`) if the VC is not in `vcs`;
    /// - `InvalidField` (`data_field_content`) if `fields.pdu` is not the VC's data unit
    ///   (732.0 §4.1.4.1.4);
    /// - `InvalidConfig` (`frame_data_field`) if `pc` leaves the VC no data octet after the
    ///   `M_PDU` or `B_PDU` Header, which a `vcs` built for `pc` never allows;
    /// - `OutOfRange` (`first_header_pointer`) for a packet offset outside the Packet Zone or
    ///   above `0xFFFD` (`0x7FD` in Issue 4), or (`bitstream_data_pointer`) for `ValidBits(n)`
    ///   with `n` 0 or not below the zone's bits, with the largest valid value (AOS-017);
    /// - `OutOfRange` (`spacecraft_id`) for an SCID above 255 in `Issue4Compatible` (AOS-002);
    /// - `BufferTooSmall` (`needed` = frame length) if `out` is shorter than a frame.
    pub fn begin<L: VcLookup + ?Sized>(
        out: &'b mut [u8],
        pc: &AosPhysicalChannelConfig,
        vcs: &L,
        fields: &AosFrameFields,
    ) -> Result<AosFrameWriter<'b>> {
        let plan = Plan::new(pc, vcs, fields)?;
        AosFrameWriter::start(out, plan)
    }

    /// Starts an OID Transfer Frame: VCID 63, no OCF or SDLS fields, and the whole data field
    /// filled with the next octets of `oid` (732.0 §4.1.4.1.5; AOS-016, CMN-020, CMN-021). The
    /// Insert Zone, if configured, is still filled by the caller (AOS-015).
    ///
    /// # Errors
    /// On error neither `out` nor `oid` is changed (GEN-042):
    /// - `InvalidField` (`vc_id`) unless `fields.vcid` is 63, and (`data_field_content`) unless
    ///   `fields.pdu` is [`PduHeader::None`] (see [`AosFrameFields::oid`]);
    /// - `UnknownChannel` (`spacecraft_id`) for an SCID with no VC in `vcs` (§4.3.6.3);
    /// - `OutOfRange` (`spacecraft_id`) for an SCID above 255 in `Issue4Compatible`;
    /// - `BufferTooSmall` (`needed` = frame length) if `out` is shorter than a frame.
    pub fn begin_oid<L: VcLookup + ?Sized>(
        out: &'b mut [u8],
        pc: &AosPhysicalChannelConfig,
        vcs: &L,
        fields: &AosFrameFields,
        oid: &mut OidGenerator,
    ) -> Result<AosFrameWriter<'b>> {
        let plan = Plan::oid(pc, vcs, fields)?;
        let mut writer = AosFrameWriter::start(out, plan)?;
        oid.fill(writer.zone_mut());
        Ok(writer)
    }

    /// Writes a checked plan into `out`.
    fn start(out: &'b mut [u8], plan: Plan) -> Result<AosFrameWriter<'b>> {
        let frame_len = plan.layout.frame_len();
        let frame = out
            .get_mut(..frame_len)
            .ok_or(Error::buffer_too_small(frame_len))?;
        frame.fill(0);
        let header = plan.header.to_octets();
        write_at(
            frame,
            0,
            header.get(..plan.header.len()).unwrap_or_default(),
        );
        let data = plan.layout.data();
        let mut zone_start = data.start;
        if let Some(pdu) = plan.pdu {
            write_at(frame, data.start, &pdu);
            zone_start = data.start.saturating_add(pdu.len());
        }
        Ok(AosFrameWriter {
            frame,
            layout: plan.layout,
            zone_start,
        })
    }

    /// The octets of `range`, which lies inside the frame by construction (CMN-040).
    fn field_mut(&mut self, range: Range<usize>) -> &mut [u8] {
        self.frame.get_mut(range).unwrap_or_default()
    }

    /// The Insert Zone to fill in place with an `IN_SDU`; empty without one (732.0 §4.1.3;
    /// AOS-015).
    pub fn insert_zone_mut(&mut self) -> &mut [u8] {
        self.field_mut(self.layout.secondary())
    }

    /// The Security Header to fill in place; empty without SDLS (732.0 §6.3.4; API-072).
    pub fn security_header_mut(&mut self) -> &mut [u8] {
        self.field_mut(self.layout.security_header())
    }

    /// The octets after the `M_PDU` or `B_PDU` Header to fill in place: the Packet Zone, the
    /// Bitstream Data Zone, or the whole data field of a `VCA_SDU` or Idle Data; never empty
    /// (732.0 §4.1.4; GEN-014).
    pub fn zone_mut(&mut self) -> &mut [u8] {
        let end = self.layout.data().end;
        self.field_mut(self.zone_start..end)
    }

    /// The Security Trailer to fill in place; empty without SDLS (732.0 §6.3.6; API-072).
    pub fn security_trailer_mut(&mut self) -> &mut [u8] {
        self.field_mut(self.layout.security_trailer())
    }

    /// Writes the Operational Control Field (732.0 §4.1.5; AOS-019).
    ///
    /// # Errors
    /// `InvalidField` (`ocf`) if frames of this VC carry no OCF (732.0 §4.1.5.2, Table 5-3).
    pub fn set_ocf(&mut self, ocf: Ocf) -> Result<()> {
        let range = self.layout.ocf();
        if range.is_empty() {
            return Err(mismatch(ErrorKind::InvalidField, field::OCF, 0, 1));
        }
        write_at(self.frame, range.start, &ocf.to_octets());
        Ok(())
    }

    /// The octet ranges of the frame's fields, for a security hook (CMN-040, API-074).
    #[must_use]
    pub const fn layout(&self) -> &FrameLayout {
        &self.layout
    }

    /// Completes the frame: computes the FECF, if configured, over every other octet; returns
    /// the frame length (732.0 §4.1.6.2, §4.2.7.4; AOS-019, API-021).
    #[must_use = "the frame length"]
    pub fn finish(self) -> usize {
        let fecf = self.layout.fecf();
        if !fecf.is_empty() {
            let crc = crc16(self.frame.get(..fecf.start).unwrap_or_default());
            write_at(self.frame, fecf.start, &crc.to_be_bytes());
        }
        self.layout.frame_len()
    }
}

/// Encodes one frame of (`fields.scid`, `fields.vcid`) into the start of `out`; returns the
/// frame length (732.0 §4.1, §6.3; AOS-019, API-020…API-022).
///
/// The writer computes the FHEC and the FECF, and writes the `M_PDU` or `B_PDU` Header from
/// `fields.pdu` (API-021).
///
/// # Errors
/// On error `out` is unchanged (API-022). The errors of [`AosFrameWriter::begin`], checked
/// first, and then:
/// - `LengthMismatch` (`insert_zone_len`, `security_header_len`, `frame_data_field`, or
///   `security_trailer_len`) for a field of `content` whose length is not the configured one,
///   with that length;
/// - `InvalidField` (`ocf`) for an OCF given where none is configured, or missing where one is
///   (732.0 §4.1.5.3).
pub fn encode<L: VcLookup + ?Sized>(
    pc: &AosPhysicalChannelConfig,
    vcs: &L,
    fields: &AosFrameFields,
    content: &AosFrameContent<'_>,
    out: &mut [u8],
) -> Result<usize> {
    let plan = Plan::new(pc, vcs, fields)?;
    let layout = &plan.layout;
    let pdu_len = plan.pdu.map_or(0, |p| p.len());
    let zone_len = layout.data().len().saturating_sub(pdu_len);
    for (given, expected, field) in [
        (
            content.insert_zone.len(),
            layout.secondary().len(),
            field::INSERT_ZONE_LEN,
        ),
        (
            content.security_header.len(),
            layout.security_header().len(),
            field::SECURITY_HEADER_LEN,
        ),
        (content.zone.len(), zone_len, field::FRAME_DATA_FIELD),
        (
            content.security_trailer.len(),
            layout.security_trailer().len(),
            field::SECURITY_TRAILER_LEN,
        ),
    ] {
        if given != expected {
            return Err(mismatch(
                ErrorKind::LengthMismatch,
                field,
                len_u64(expected),
                len_u64(given),
            ));
        }
    }
    let has_ocf = !layout.ocf().is_empty();
    if content.ocf.is_some() != has_ocf {
        return Err(mismatch(
            ErrorKind::InvalidField,
            field::OCF,
            u64::from(has_ocf),
            u64::from(content.ocf.is_some()),
        ));
    }
    let writer = AosFrameWriter::start(out, plan)?;
    // Each field has the length checked above, so it fills its range exactly.
    let layout = &writer.layout;
    for (at, src) in [
        (layout.secondary().start, content.insert_zone),
        (layout.security_header().start, content.security_header),
        (writer.zone_start, content.zone),
        (layout.security_trailer().start, content.security_trailer),
    ] {
        write_at(writer.frame, at, src);
    }
    if let Some(ocf) = content.ocf {
        write_at(writer.frame, layout.ocf().start, &ocf.to_octets());
    }
    Ok(writer.finish())
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    extern crate std;
    use std::vec;

    use super::{AosFrameContent, AosFrameFields, AosFrameWriter, PduHeader, encode};
    use crate::bpdu::BitstreamPointer;
    use crate::config::tests::{pc, scid, vc, vcid};
    use crate::config::{AosEdition, AosVirtualChannelConfig, DataFieldContent, VcLookup, VcTable};
    use crate::field;
    use crate::header::{FrameCountCycle, OID_VCID, VcFrameCount};
    use crate::mpdu::PacketPointer;
    use crate::view::{AosFrame, DataUnit};
    use aldis_core::crc::syndrome;
    use aldis_core::ids::{SpacecraftId, VcId};
    use aldis_core::ocf::Ocf;
    use aldis_core::oid::OidGenerator;
    use aldis_core::security::SecurityLayout;
    use aldis_core::{DecodeMode, ErrorDetail, ErrorKind, FieldId};

    const STRICT: DecodeMode = DecodeMode::Strict;

    fn count(n: u32) -> VcFrameCount {
        VcFrameCount::new(n).unwrap()
    }

    /// 40-octet frames with FHEC, a 4-octet Insert Zone, and FECF; SCID 0x0AB: VC 0 `M_PDUs` with
    /// an OCF and SDLS 2/1, VC 1 `B_PDUs`, VC 2 `VCA_SDUs`.
    fn table() -> VcTable<3> {
        VcTable::new(
            pc(40, true, 4, true, AosEdition::Issue5),
            &[
                vc(0x0AB, 0, DataFieldContent::Mpdu)
                    .with_ocf()
                    .with_security(SecurityLayout::new(2, 1).unwrap()),
                vc(0x0AB, 1, DataFieldContent::Bpdu),
                vc(0x0AB, 2, DataFieldContent::VcaSdu),
            ],
        )
        .unwrap()
    }

    fn mpdu(pointer: PacketPointer) -> AosFrameFields {
        AosFrameFields {
            replay_flag: true,
            cycle: FrameCountCycle::new(9),
            ..AosFrameFields::new(
                scid(0x0AB),
                vcid(0),
                count(0x12_3456),
                PduHeader::Mpdu(pointer),
            )
        }
    }

    fn rejected(
        t: &VcTable<3>,
        fields: &AosFrameFields,
    ) -> (ErrorKind, Option<FieldId>, ErrorDetail) {
        let mut out = [0xEE_u8; 40];
        let e = AosFrameWriter::begin(&mut out, &t.physical_channel(), t, fields).unwrap_err();
        assert!(out.iter().all(|&b| b == 0xEE), "out changed");
        (e.kind(), e.field(), e.detail())
    }

    #[test]
    fn two_phase_writer_fills_every_field() {
        // req: AOS-019, AOS-013, AOS-015, API-020, API-021, API-072, GEN-014
        let t = table();
        let phy = t.physical_channel();
        let mut out = [0xEE_u8; 48];
        let mut w =
            AosFrameWriter::begin(&mut out, &phy, &t, &mpdu(PacketPointer::StartsAt(3))).unwrap();
        assert_eq!(w.layout().frame_len(), 40);
        w.insert_zone_mut().copy_from_slice(&[1, 2, 3, 4]);
        w.security_header_mut().copy_from_slice(&[0xA5, 0xA5]);
        // 40 − 8 − 4 − 2 − 1 − 4 − 2 = 19 data octets: the M_PDU Header and 17.
        assert_eq!(w.zone_mut().len(), 17);
        w.zone_mut().fill(0xD0);
        w.security_trailer_mut().copy_from_slice(&[0x5A]);
        w.set_ocf(Ocf::decode([0x01, 0, 0, 0])).unwrap();
        assert_eq!(w.finish(), 40);
        assert_eq!(out[40..], [0xEE; 8], "nothing written past the frame");
        assert_eq!(syndrome(&out[..40]), 0);
        // SCID 0x0AB: LSBs 1010 1011, MSBs 00; VCID 0; replay 1, use 1, cycle 9.
        assert_eq!(out[..6], [0x6A, 0xC0, 0x12, 0x34, 0x56, 0xC9]);
        assert_eq!(out[14..16], [0x00, 0x03], "the M_PDU Header");
        let f = AosFrame::decode(&out[..40], &phy, &t, STRICT).unwrap();
        assert_eq!(
            f.header().fhec().map(u16::to_be_bytes),
            Some([out[6], out[7]])
        );
        assert_eq!(f.insert_zone(), &[1, 2, 3, 4]);
        assert_eq!(
            (f.security_header(), f.security_trailer()),
            (&[0xA5, 0xA5][..], &[0x5A][..])
        );
        assert_eq!(f.ocf(), Some(Ocf::decode([0x01, 0, 0, 0])));
        assert_eq!(
            (f.replay_flag(), f.vc_frame_count_cycle()),
            (true, FrameCountCycle::new(9))
        );
        let DataUnit::Mpdu(m) = f.data_unit() else {
            panic!("an M_PDU")
        };
        assert_eq!(
            (m.pointer(), m.packet_zone()),
            (PacketPointer::StartsAt(3), &[0xD0; 17][..])
        );
    }

    #[test]
    fn unfinished_frame_is_defined() {
        // req: API-023
        let t = table();
        let mut out = [0xEE_u8; 40];
        let fields = AosFrameFields::new(
            scid(0x0AB),
            vcid(1),
            count(0),
            PduHeader::Bpdu(BitstreamPointer::OnlyIdle),
        );
        assert!(AosFrameWriter::begin(&mut out, &t.physical_channel(), &t, &fields).is_ok());
        assert_eq!(out[..6], [0x6A, 0xC1, 0, 0, 0, 0]);
        assert_eq!(out[12..14], [0x3F, 0xFE], "the B_PDU Header");
        assert!(out[14..].iter().all(|&b| b == 0));
    }

    #[test]
    fn begin_rejects_without_writing() {
        // req: AOS-019, AOS-016, AOS-017, API-022, GEN-042
        let t = table();
        let mut fields = mpdu(PacketPointer::NoPacketStart);
        fields.vcid = OID_VCID;
        assert_eq!(
            rejected(&t, &fields),
            (
                ErrorKind::InvalidField,
                Some(field::VC_ID),
                ErrorDetail::Expected {
                    expected: 62,
                    found: 63
                }
            )
        );
        fields.vcid = vcid(3);
        assert_eq!(rejected(&t, &fields).1, Some(field::VC_ID));
        fields.vcid = vcid(0);
        fields.scid = scid(0x0AC);
        assert_eq!(rejected(&t, &fields).1, Some(field::SPACECRAFT_ID));
        for (v, pdu) in [
            (0, PduHeader::None),
            (0, PduHeader::Bpdu(BitstreamPointer::AllValid)),
            (1, PduHeader::Mpdu(PacketPointer::NoPacketStart)),
            (2, PduHeader::Mpdu(PacketPointer::NoPacketStart)),
        ] {
            let f = AosFrameFields::new(scid(0x0AB), vcid(v), count(0), pdu);
            assert_eq!(rejected(&t, &f).1, Some(field::DATA_FIELD_CONTENT));
        }
        // Packet Zone of 17 octets: offsets 0–16.
        assert_eq!(
            rejected(&t, &mpdu(PacketPointer::StartsAt(17))),
            (
                ErrorKind::OutOfRange,
                Some(field::FIRST_HEADER_POINTER),
                ErrorDetail::Expected {
                    expected: 16,
                    found: 17
                }
            )
        );
        // Bitstream Data Zone of 40 − 8 − 4 − 2 − 2 = 24 octets, 192 bits: counts 1–191.
        for n in [0, 192] {
            let f = AosFrameFields::new(
                scid(0x0AB),
                vcid(1),
                count(0),
                PduHeader::Bpdu(BitstreamPointer::ValidBits(n)),
            );
            assert_eq!(
                rejected(&t, &f),
                (
                    ErrorKind::OutOfRange,
                    Some(field::BITSTREAM_DATA_POINTER),
                    ErrorDetail::Expected {
                        expected: 191,
                        found: u64::from(n)
                    }
                )
            );
        }
        let mut short = [0xEE_u8; 39];
        let e = AosFrameWriter::begin(
            &mut short,
            &t.physical_channel(),
            &t,
            &mpdu(PacketPointer::StartsAt(16)),
        )
        .unwrap_err();
        assert_eq!(
            (e.kind(), e.detail()),
            (ErrorKind::BufferTooSmall, ErrorDetail::Needed(40))
        );
        assert!(short.iter().all(|&b| b == 0xEE));
        let f = AosFrameFields::new(
            scid(0x0AB),
            vcid(1),
            count(0),
            PduHeader::Bpdu(BitstreamPointer::ValidBits(191)),
        );
        let mut out = [0_u8; 40];
        assert!(AosFrameWriter::begin(&mut out, &t.physical_channel(), &t, &f).is_ok());
        // A physical channel the table was not built for: no data octet at all, then only the
        // two of the M_PDU Header.
        let small = pc(21, true, 4, true, AosEdition::Issue5);
        let e = AosFrameWriter::begin(&mut out, &small, &t, &mpdu(PacketPointer::NoPacketStart))
            .unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::InvalidConfig, Some(field::FRAME_DATA_FIELD))
        );
        let tiny = pc(23, true, 4, true, AosEdition::Issue5);
        let e = AosFrameWriter::begin(&mut out, &tiny, &t, &mpdu(PacketPointer::NoPacketStart))
            .unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::InvalidConfig, Some(field::FRAME_DATA_FIELD))
        );
    }

    /// A lookup that holds one VC whatever it is asked, as a lookup built without `VcTable`
    /// might.
    struct Any(AosVirtualChannelConfig);

    impl VcLookup for Any {
        fn has_spacecraft(&self, _: SpacecraftId<10>) -> bool {
            true
        }

        fn vc(&self, _: SpacecraftId<10>, _: VcId<6>) -> Option<AosVirtualChannelConfig> {
            Some(self.0)
        }
    }

    #[test]
    fn issue_4_frames() {
        // req: AOS-002, AOS-003, GEN-052, GEN-062
        let p4 = pc(2100, false, 0, false, AosEdition::Issue4Compatible);
        let t = VcTable::<1>::new(p4, &[vc(0xFF, 7, DataFieldContent::Mpdu)]).unwrap();
        let mut out = vec![0_u8; 2100];
        let fields =
            |pointer| AosFrameFields::new(scid(0xFF), vcid(7), count(1), PduHeader::Mpdu(pointer));
        let w = AosFrameWriter::begin(&mut out, &p4, &t, &fields(PacketPointer::StartsAt(0x7FD)))
            .unwrap();
        assert_eq!(w.finish(), 2100);
        // SCID 0xFF in bits 2–9, bits 42–43 00; spare 00000 and FHP 0x7FD.
        assert_eq!(out[..8], [0x7F, 0xC7, 0, 0, 1, 0x00, 0x07, 0xFD]);
        let f = AosFrame::decode(&out, &p4, &t, STRICT).unwrap();
        assert_eq!(
            (f.scid(), f.header().edition()),
            (scid(0xFF), AosEdition::Issue4Compatible)
        );
        let e = AosFrameWriter::begin(&mut out, &p4, &t, &fields(PacketPointer::StartsAt(0x7FE)))
            .unwrap_err();
        assert_eq!(
            (e.kind(), e.detail()),
            (
                ErrorKind::OutOfRange,
                ErrorDetail::Expected {
                    expected: 0x7FD,
                    found: 0x7FE
                }
            )
        );
        // Issue 5 accepts the offset, and writes 16 bits.
        let p5 = pc(2100, false, 0, false, AosEdition::Issue5);
        let t5 = VcTable::<1>::new(p5, &[vc(0xFF, 7, DataFieldContent::Mpdu)]).unwrap();
        let w = AosFrameWriter::begin(&mut out, &p5, &t5, &fields(PacketPointer::StartsAt(0x7FE)))
            .unwrap();
        assert_eq!(w.finish(), 2100);
        assert_eq!(out[6..8], [0x07, 0xFE]);
        // An SCID above 8 bits from a lookup that did not check it.
        let wide = Any(vc(0x100, 7, DataFieldContent::Mpdu));
        let mut f = fields(PacketPointer::NoPacketStart);
        f.scid = scid(0x100);
        let e = AosFrameWriter::begin(&mut out, &p4, &wide, &f).unwrap_err();
        assert_eq!(
            (e.kind(), e.field(), e.detail()),
            (
                ErrorKind::OutOfRange,
                Some(field::SPACECRAFT_ID),
                ErrorDetail::Expected {
                    expected: 255,
                    found: 256
                }
            )
        );
        let e = AosFrameWriter::begin_oid(
            &mut out,
            &p4,
            &wide,
            &AosFrameFields::oid(scid(0x100), count(0)),
            &mut OidGenerator::new(),
        )
        .unwrap_err();
        assert_eq!(e.field(), Some(field::SPACECRAFT_ID));
    }

    #[test]
    fn encode_checks_every_length() {
        // req: AOS-019, AOS-015, API-021, API-022
        let t = table();
        let phy = t.physical_channel();
        let mut out = [0xEE_u8; 40];
        let zone = [0x11; 17];
        let ok = AosFrameContent {
            insert_zone: &[1, 2, 3, 4],
            security_header: &[0, 0],
            zone: &zone,
            security_trailer: &[0],
            ocf: Some(Ocf::decode([0; 4])),
        };
        let fields = mpdu(PacketPointer::StartsAt(0));
        for (content, f, expected, found) in [
            (
                AosFrameContent {
                    insert_zone: &[1, 2, 3],
                    ..ok
                },
                field::INSERT_ZONE_LEN,
                4,
                3,
            ),
            (
                AosFrameContent {
                    security_header: &[0],
                    ..ok
                },
                field::SECURITY_HEADER_LEN,
                2,
                1,
            ),
            (
                AosFrameContent {
                    zone: &zone[1..],
                    ..ok
                },
                field::FRAME_DATA_FIELD,
                17,
                16,
            ),
            (
                AosFrameContent {
                    security_trailer: &[],
                    ..ok
                },
                field::SECURITY_TRAILER_LEN,
                1,
                0,
            ),
        ] {
            let e = encode(&phy, &t, &fields, &content, &mut out).unwrap_err();
            assert_eq!(
                (e.kind(), e.field(), e.detail()),
                (
                    ErrorKind::LengthMismatch,
                    Some(f),
                    ErrorDetail::Expected { expected, found }
                )
            );
        }
        let e = encode(
            &phy,
            &t,
            &fields,
            &AosFrameContent { ocf: None, ..ok },
            &mut out,
        )
        .unwrap_err();
        assert_eq!(
            (e.kind(), e.field(), e.detail()),
            (
                ErrorKind::InvalidField,
                Some(field::OCF),
                ErrorDetail::Expected {
                    expected: 1,
                    found: 0
                }
            )
        );
        let e = encode(&phy, &t, &mpdu(PacketPointer::StartsAt(17)), &ok, &mut out).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::OutOfRange);
        let e = encode(&phy, &t, &fields, &ok, &mut out[..10]).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::BufferTooSmall);
        assert!(out.iter().all(|&b| b == 0xEE), "out unchanged on error");
        assert_eq!(encode(&phy, &t, &fields, &ok, &mut out), Ok(40));
        let f = AosFrame::decode(&out, &phy, &t, STRICT).unwrap();
        assert_eq!(f.data_unit().zone(), &zone);
    }

    #[test]
    fn encode_vca_sdu() {
        // req: AOS-019, API-020, API-021
        let t = table();
        let phy = t.physical_channel();
        let mut out = [0xEE_u8; 40];
        // A VCA_SDU fills the whole data field: 40 − 8 − 4 − 2 = 26 octets.
        let sdu = [0x22; 26];
        let vca = AosFrameFields::new(scid(0x0AB), vcid(2), count(5), PduHeader::None);
        let content = AosFrameContent {
            insert_zone: &[9; 4],
            ..AosFrameContent::new(&sdu)
        };
        assert_eq!(encode(&phy, &t, &vca, &content, &mut out), Ok(40));
        let f = AosFrame::decode(&out, &phy, &t, STRICT).unwrap();
        assert_eq!((f.data_unit(), f.ocf()), (DataUnit::VcaSdu(&sdu), None));
        let e = encode(
            &phy,
            &t,
            &vca,
            &AosFrameContent {
                ocf: Some(Ocf::decode([0; 4])),
                ..content
            },
            &mut out,
        )
        .unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::InvalidField, Some(field::OCF))
        );
    }

    #[test]
    fn set_ocf_without_an_ocf() {
        // req: AOS-019
        let t = table();
        let phy = t.physical_channel();
        let mut out = [0_u8; 40];
        let fields = AosFrameFields::new(scid(0x0AB), vcid(2), count(0), PduHeader::None);
        let mut w = AosFrameWriter::begin(&mut out, &phy, &t, &fields).unwrap();
        let e = w.set_ocf(Ocf::decode([0; 4])).unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::InvalidField, Some(field::OCF))
        );
        assert!(w.security_header_mut().is_empty() && w.security_trailer_mut().is_empty());
        assert_eq!(w.zone_mut().len(), 26);
        assert_eq!(w.finish(), 40);
        // Without FHEC and FECF: nothing computed after the header.
        let plain = pc(12, false, 0, false, AosEdition::Issue5);
        let t = VcTable::<1>::new(plain, &[vc(1, 1, DataFieldContent::IdleData)]).unwrap();
        let mut out = [0_u8; 12];
        let f = AosFrameFields::new(scid(1), vcid(1), count(0), PduHeader::None);
        let w = AosFrameWriter::begin(&mut out, &plain, &t, &f).unwrap();
        assert!(w.layout().fecf().is_empty());
        assert_eq!(w.finish(), 12);
        assert_eq!(out, [0x40, 0x41, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn oid_frames_continue_the_sequence() {
        // req: AOS-016, AOS-015, CMN-020, CMN-021, GEN-042
        let t = table();
        let phy = t.physical_channel();
        let mut oid = OidGenerator::new();
        let mut first = [0_u8; 40];
        let mut second = [0_u8; 40];
        let mut w = AosFrameWriter::begin_oid(
            &mut first,
            &phy,
            &t,
            &AosFrameFields::oid(scid(0x0AB), count(1)),
            &mut oid,
        )
        .unwrap();
        w.insert_zone_mut().copy_from_slice(&[7, 7, 7, 7]);
        assert_eq!(w.finish(), 40);
        let w = AosFrameWriter::begin_oid(
            &mut second,
            &phy,
            &t,
            &AosFrameFields::oid(scid(0x0AB), count(2)),
            &mut oid,
        )
        .unwrap();
        assert_eq!(w.finish(), 40);
        // 40 − 8 − 4 − 2 = 26 data octets per frame.
        let mut reference = OidGenerator::new();
        let mut expected = [0_u8; 52];
        reference.fill(&mut expected);
        let one = AosFrame::decode(&first, &phy, &t, STRICT).unwrap();
        let two = AosFrame::decode(&second, &phy, &t, STRICT).unwrap();
        assert!(one.is_oid() && two.is_oid());
        assert_eq!(
            (one.data_field(), two.data_field()),
            (&expected[..26], &expected[26..])
        );
        assert_eq!(one.data_field()[..4], [0xFF; 4]);
        assert_eq!(
            (one.insert_zone(), two.insert_zone()),
            (&[7; 4][..], &[0; 4][..])
        );
        assert_eq!(one.vc_frame_count().get(), 1);
        // Rejected without advancing the generator.
        let before = oid.clone();
        let mut third = [0xEE_u8; 40];
        for (fields, f) in [
            (
                AosFrameFields::new(scid(0x0AB), vcid(2), count(3), PduHeader::None),
                field::VC_ID,
            ),
            (
                AosFrameFields {
                    pdu: PduHeader::Mpdu(PacketPointer::OnlyIdleData),
                    ..AosFrameFields::oid(scid(0x0AB), count(3))
                },
                field::DATA_FIELD_CONTENT,
            ),
        ] {
            let e = AosFrameWriter::begin_oid(&mut third, &phy, &t, &fields, &mut oid).unwrap_err();
            assert_eq!((e.kind(), e.field()), (ErrorKind::InvalidField, Some(f)));
        }
        let e = AosFrameWriter::begin_oid(
            &mut third,
            &phy,
            &t,
            &AosFrameFields::oid(scid(0x0AC), count(3)),
            &mut oid,
        )
        .unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::UnknownChannel, Some(field::SPACECRAFT_ID))
        );
        let e = AosFrameWriter::begin_oid(
            &mut third[..39],
            &phy,
            &t,
            &AosFrameFields::oid(scid(0x0AB), count(3)),
            &mut oid,
        )
        .unwrap_err();
        assert_eq!(e.kind(), ErrorKind::BufferTooSmall);
        assert_eq!(oid, before);
        assert!(third.iter().all(|&b| b == 0xEE));
    }
}

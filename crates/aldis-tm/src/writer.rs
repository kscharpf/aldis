//! Encoders: one-shot [`encode`], the two-phase [`TmFrameWriter`], and OID frames
//! ([`TmFrameWriter::begin_oid`]) (132.0-B-3 §4.1, §4.1.4.6, §6.3; TM-009, TM-010,
//! API-020…API-023, API-072, GEN-014, GEN-042, GEN-052).

use core::ops::Range;

use aldis_core::crc::crc16;
use aldis_core::ids::VcId;
use aldis_core::layout::FrameLayout;
use aldis_core::ocf::Ocf;
use aldis_core::oid::OidGenerator;
use aldis_core::{Error, ErrorKind, Result};

use crate::config::TmMasterChannelConfig;
use crate::field;
use crate::header::{
    DataFieldStatus, FirstHeaderPointer, FshLen, PRIMARY_HEADER_LEN, PacketPointer,
    SegmentLengthId, TmPrimaryHeader, len_u64, mismatch,
};

/// The Primary Header fields a caller chooses (132.0 §4.1.2; TM-009).
///
/// The SCID, the OCF Flag, and the Secondary Header Flag come from the configuration (API-021).
/// Frame counts are kept by the engines ([04](../../docs/04-engines.md)).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TmFrameFields {
    /// Virtual Channel Identifier, which must be configured (132.0 §4.1.2.3).
    pub vcid: VcId<3>,
    /// Master Channel Frame Count (132.0 §4.1.2.5).
    pub mc_frame_count: u8,
    /// Virtual Channel Frame Count (132.0 §4.1.2.6).
    pub vc_frame_count: u8,
    /// Synchronization Flag and the fields that depend on it (132.0 §4.1.2.7.3–6).
    pub status: DataFieldStatus,
}

impl TmFrameFields {
    /// The fields of a frame of `vcid` with the given counts and status (TM-009).
    #[must_use]
    pub const fn new(
        vcid: VcId<3>,
        mc_frame_count: u8,
        vc_frame_count: u8,
        status: DataFieldStatus,
    ) -> TmFrameFields {
        TmFrameFields {
            vcid,
            mc_frame_count,
            vc_frame_count,
            status,
        }
    }
}

/// The variable fields of a frame for [`encode`], each exactly the length the configuration
/// gives it (132.0 §4.1, §6.3; TM-009, API-072).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TmFrameContent<'d> {
    /// The Secondary Header Data Field, after the Identification Field the writer computes;
    /// empty without a Secondary Header (132.0 §4.1.3.3).
    pub secondary_header_data: &'d [u8],
    /// The Security Header, as a security hook filled it; empty without SDLS (132.0 §6.3.4).
    pub security_header: &'d [u8],
    /// The Transfer Frame Data Field, filling it exactly (132.0 §4.1.4.2).
    pub data: &'d [u8],
    /// The Security Trailer; empty without SDLS (132.0 §6.3.6).
    pub security_trailer: &'d [u8],
    /// The OCF, present exactly when the configuration has one (132.0 §4.1.5).
    pub ocf: Option<Ocf>,
}

impl<'d> TmFrameContent<'d> {
    /// A frame holding `data` and no other variable field.
    #[must_use]
    pub const fn new(data: &'d [u8]) -> TmFrameContent<'d> {
        TmFrameContent {
            secondary_header_data: &[],
            security_header: &[],
            data,
            security_trailer: &[],
            ocf: None,
        }
    }
}

/// A frame checked against its configuration, not yet written.
struct Plan {
    header: TmPrimaryHeader,
    fsh: Option<FshLen>,
    layout: FrameLayout,
}

impl Plan {
    /// Checks `fields` against `mc` and derives the header (TM-009, TM-011).
    fn new(mc: &TmMasterChannelConfig, fields: &TmFrameFields) -> Result<Plan> {
        let entry = mc
            .entry(fields.vcid)
            .ok_or(Error::new(ErrorKind::UnknownChannel).with_field(field::VC_ID))?;
        let vc = &entry.config;
        let sync = fields.status.sync_flag();
        if sync != vc.content().sync_flag() {
            return Err(mismatch(
                ErrorKind::InvalidField,
                field::SYNC_FLAG,
                u64::from(vc.content().sync_flag()),
                u64::from(sync),
            ));
        }
        let (packet_order_flag, segment_length_id, first_header_pointer) = match fields.status {
            DataFieldStatus::Packets(pointer) => {
                if let PacketPointer::StartsAt(n) = pointer {
                    let limit = entry
                        .layout
                        .data()
                        .len()
                        .min(usize::from(FirstHeaderPointer::ONLY_IDLE_DATA.get()))
                        .saturating_sub(1);
                    if usize::from(n) > limit {
                        return Err(Error::out_of_range(
                            field::FIRST_HEADER_POINTER,
                            len_u64(limit),
                            u64::from(n),
                        ));
                    }
                }
                (false, SegmentLengthId::NO_SEGMENTS, pointer.to_raw())
            }
            DataFieldStatus::VcaSdu {
                packet_order_flag,
                segment_length_id,
                first_header_pointer,
            } => (packet_order_flag, segment_length_id, first_header_pointer),
        };
        let fsh = mc.fsh_of(vc);
        Ok(Plan {
            header: TmPrimaryHeader {
                scid: mc.scid(),
                vcid: fields.vcid,
                ocf_flag: mc.ocf_of(vc),
                mc_frame_count: fields.mc_frame_count,
                vc_frame_count: fields.vc_frame_count,
                fsh_flag: fsh.is_some(),
                sync_flag: sync,
                packet_order_flag,
                segment_length_id,
                first_header_pointer,
            },
            fsh,
            layout: entry.layout.clone(),
        })
    }
}

/// Copies `src` into `buf` from octet `at`, as far as both reach.
fn write_at(buf: &mut [u8], at: usize, src: &[u8]) {
    for (d, s) in buf.iter_mut().skip(at).zip(src) {
        *d = *s;
    }
}

/// Two-phase frame writer (132.0 §4.1; TM-009, API-020…API-023, API-072).
///
/// [`begin`](Self::begin) writes the Primary Header and the Secondary Header Identification
/// Field, the caller fills the other fields in place, and [`finish`](Self::finish) computes the
/// FECF.
///
/// `begin` zero-fills every octet after the headers, so until `finish` the frame is defined: its
/// headers final, the fields the caller has not filled zero, and the FECF, if configured, zero
/// (API-023). The FECF is computed last, after a security hook has filled the Security Header and
/// Trailer (API-074).
#[derive(Debug)]
pub struct TmFrameWriter<'b> {
    frame: &'b mut [u8],
    layout: FrameLayout,
}

impl<'b> TmFrameWriter<'b> {
    /// Starts a frame of `fields.vcid` in the first frame-length octets of `out` (TM-009).
    ///
    /// # Errors
    /// On error `out` is unchanged (API-022, GEN-042):
    /// - `UnknownChannel` (`vc_id`) if the VC is not configured;
    /// - `InvalidField` (`sync_flag`) if `fields.status` does not match the VC's data field
    ///   content (132.0 §4.1.2.7.3.3, §4.1.4.4);
    /// - `OutOfRange` (`first_header_pointer`) for a packet offset outside the data field or
    ///   above `0x7FD`, with the largest valid offset (132.0 §4.1.2.7.6.3);
    /// - `BufferTooSmall` (`needed` = frame length) if `out` is shorter than a frame.
    pub fn begin(
        out: &'b mut [u8],
        mc: &TmMasterChannelConfig,
        fields: &TmFrameFields,
    ) -> Result<TmFrameWriter<'b>> {
        let plan = Plan::new(mc, fields)?;
        TmFrameWriter::start(out, plan, true)
    }

    /// Takes up a frame that [`begin`](Self::begin) or [`begin_oid`](Self::begin_oid) started in
    /// `out` with the same `mc` and `fields`, so that a writer which keeps no state between
    /// calls, the C ABI's `aldis_tm_writer_finish`, can finish it (API-023; M2 plan D4).
    ///
    /// Checks `mc` and `fields` as `begin` does and rewrites the Primary Header and the
    /// Secondary Header Identification Field; every other octet stays as the caller filled it.
    /// For an OID frame, `fields` has the status `Packets(OnlyIdleData)` that `begin_oid` uses.
    ///
    /// # Errors
    /// As for [`begin`](Self::begin); on error `out` is unchanged (API-022, GEN-042).
    pub fn resume(
        out: &'b mut [u8],
        mc: &TmMasterChannelConfig,
        fields: &TmFrameFields,
    ) -> Result<TmFrameWriter<'b>> {
        let plan = Plan::new(mc, fields)?;
        TmFrameWriter::start(out, plan, false)
    }

    /// Starts an Only Idle Data frame on `vcid`: First Header Pointer `0x7FE` and the data field
    /// filled with the next octets of `oid` (132.0 §4.1.4.6; TM-010, CMN-020, CMN-021). The
    /// Secondary Header and OCF may still be filled with valid data (132.0 §4.1.4.6.3 note 1).
    ///
    /// # Errors
    /// As for [`begin`](Self::begin); `InvalidField` (`sync_flag`) if the VC carries `VCA_SDUs`,
    /// since an OID frame uses a VC that transfers Packets (132.0 §4.1.4.6.3). On error neither
    /// `out` nor `oid` is changed (GEN-042).
    pub fn begin_oid(
        out: &'b mut [u8],
        mc: &TmMasterChannelConfig,
        vcid: VcId<3>,
        mc_frame_count: u8,
        vc_frame_count: u8,
        oid: &mut OidGenerator,
    ) -> Result<TmFrameWriter<'b>> {
        let fields = TmFrameFields::new(
            vcid,
            mc_frame_count,
            vc_frame_count,
            DataFieldStatus::Packets(PacketPointer::OnlyIdleData),
        );
        let mut writer = TmFrameWriter::begin(out, mc, &fields)?;
        oid.fill(writer.data_field_mut());
        Ok(writer)
    }

    /// Writes the headers of a checked plan into `out`, after zero-filling the frame if `zero`.
    fn start(out: &'b mut [u8], plan: Plan, zero: bool) -> Result<TmFrameWriter<'b>> {
        let frame_len = plan.layout.frame_len();
        let frame = out
            .get_mut(..frame_len)
            .ok_or(Error::buffer_too_small(frame_len))?;
        if zero {
            frame.fill(0);
        }
        write_at(frame, 0, &plan.header.to_octets());
        if let Some(fsh) = plan.fsh {
            write_at(frame, PRIMARY_HEADER_LEN, &[fsh.id_octet()]);
        }
        Ok(TmFrameWriter {
            frame,
            layout: plan.layout,
        })
    }

    /// The octets of `range`, which lies inside the frame by construction (CMN-040).
    fn field_mut(&mut self, range: Range<usize>) -> &mut [u8] {
        self.frame.get_mut(range).unwrap_or_default()
    }

    /// The Secondary Header Data Field to fill in place, after the Identification Field; empty
    /// without a Secondary Header (132.0 §4.1.3.3; TM-009).
    pub fn secondary_header_data_mut(&mut self) -> &mut [u8] {
        let fsh = self.layout.secondary();
        let start = fsh.start.saturating_add(1).min(fsh.end);
        self.field_mut(start..fsh.end)
    }

    /// The Security Header to fill in place; empty without SDLS (132.0 §6.3.4; API-072).
    pub fn security_header_mut(&mut self) -> &mut [u8] {
        self.field_mut(self.layout.security_header())
    }

    /// The Transfer Frame Data Field to fill in place, never empty (132.0 §4.1.4; GEN-014).
    pub fn data_field_mut(&mut self) -> &mut [u8] {
        self.field_mut(self.layout.data())
    }

    /// The Security Trailer to fill in place; empty without SDLS (132.0 §6.3.6; API-072).
    pub fn security_trailer_mut(&mut self) -> &mut [u8] {
        self.field_mut(self.layout.security_trailer())
    }

    /// Writes the Operational Control Field (132.0 §4.1.5; TM-009).
    ///
    /// # Errors
    /// `InvalidField` (`ocf_flag`) if frames of this VC carry no OCF (132.0 §4.1.2.4.3).
    pub fn set_ocf(&mut self, ocf: Ocf) -> Result<()> {
        let range = self.layout.ocf();
        if range.is_empty() {
            return Err(mismatch(ErrorKind::InvalidField, field::OCF_FLAG, 0, 1));
        }
        write_at(self.frame, range.start, &ocf.to_octets());
        Ok(())
    }

    /// The octet ranges of the frame's fields, for a security hook (CMN-040, API-074).
    #[must_use]
    pub const fn layout(&self) -> &FrameLayout {
        &self.layout
    }

    /// Completes the frame: computes the FECF, if configured, over every other octet; returns the
    /// frame length (132.0 §4.1.6.2; TM-009, API-021).
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

/// Encodes one frame of `fields.vcid` into the start of `out`; returns the frame length
/// (132.0 §4.1, §6.3; TM-009, API-020…API-022).
///
/// The writer computes the Secondary Header Identification Field, the OCF Flag, and the FECF
/// from the configuration (API-021).
///
/// # Errors
/// On error `out` is unchanged (API-022). The errors of [`TmFrameWriter::begin`], checked
/// first, and then:
/// - `LengthMismatch` (`fsh_len`, `security_header_len`, `frame_data_field`, or
///   `security_trailer_len`) for a field of `content` whose length is not the configured one,
///   with that length;
/// - `InvalidField` (`ocf_flag`) for an OCF given where none is configured, or missing where
///   one is (132.0 §4.1.2.4.3, §4.1.5.3).
pub fn encode(
    mc: &TmMasterChannelConfig,
    fields: &TmFrameFields,
    content: &TmFrameContent<'_>,
    out: &mut [u8],
) -> Result<usize> {
    let plan = Plan::new(mc, fields)?;
    let layout = &plan.layout;
    let fsh_data_len = layout.secondary().len().saturating_sub(1);
    for (given, expected, field) in [
        (
            content.secondary_header_data.len(),
            fsh_data_len,
            field::FSH_LEN,
        ),
        (
            content.security_header.len(),
            layout.security_header().len(),
            field::SECURITY_HEADER_LEN,
        ),
        (
            content.data.len(),
            layout.data().len(),
            field::FRAME_DATA_FIELD,
        ),
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
    if content.ocf.is_some() != plan.header.ocf_flag {
        return Err(mismatch(
            ErrorKind::InvalidField,
            field::OCF_FLAG,
            u64::from(plan.header.ocf_flag),
            u64::from(content.ocf.is_some()),
        ));
    }
    let writer = TmFrameWriter::start(out, plan, true)?;
    // Each field has the length checked above, so it fills its range exactly.
    let layout = &writer.layout;
    for (at, src) in [
        (
            layout.secondary().start.saturating_add(1),
            content.secondary_header_data,
        ),
        (layout.security_header().start, content.security_header),
        (layout.data().start, content.data),
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

    use super::{TmFrameContent, TmFrameFields, TmFrameWriter, encode};
    use crate::config::{
        DataFieldContent, TmMasterChannelConfig, TmPhysicalChannelConfig, TmVirtualChannelConfig,
    };
    use crate::field;
    use crate::header::{
        DataFieldStatus, FirstHeaderPointer, FshLen, PacketPointer, SegmentLengthId,
    };
    use crate::view::TmFrame;
    use aldis_core::crc::syndrome;
    use aldis_core::ids::{SpacecraftId, VcId};
    use aldis_core::ocf::Ocf;
    use aldis_core::oid::OidGenerator;
    use aldis_core::security::SecurityLayout;
    use aldis_core::{DecodeMode, ErrorDetail, ErrorKind};

    fn vcid(n: u8) -> VcId<3> {
        VcId::new(n).unwrap()
    }

    /// 32-octet frames with FECF; MC 0x0AB with an `MC_OCF`; VC 0 packets with a 4-octet `VC_FSH` and
    /// SDLS 2/1, VC 1 `VCA_SDUs`.
    fn mc() -> TmMasterChannelConfig {
        let pc = TmPhysicalChannelConfig::new(32, true).unwrap();
        TmMasterChannelConfig::new(pc, SpacecraftId::new(0x0AB).unwrap(), None, true)
            .with_vc(
                TmVirtualChannelConfig::new(vcid(0), DataFieldContent::Packets)
                    .with_fsh(FshLen::new(4).unwrap())
                    .with_security(SecurityLayout::new(2, 1).unwrap()),
            )
            .unwrap()
            .with_vc(TmVirtualChannelConfig::new(
                vcid(1),
                DataFieldContent::VcaSdu,
            ))
            .unwrap()
    }

    fn packets(vc: u8, pointer: PacketPointer) -> TmFrameFields {
        TmFrameFields::new(vcid(vc), 7, 9, DataFieldStatus::Packets(pointer))
    }

    #[test]
    fn two_phase_writer_fills_every_field() {
        // req: TM-009, API-020, API-021, API-072, GEN-014
        let mc = mc();
        let mut out = [0xEE_u8; 40];
        let mut w =
            TmFrameWriter::begin(&mut out, &mc, &packets(0, PacketPointer::StartsAt(3))).unwrap();
        assert_eq!(w.layout().frame_len(), 32);
        w.secondary_header_data_mut().copy_from_slice(&[1, 2, 3]);
        w.security_header_mut().copy_from_slice(&[0xA5, 0xA5]);
        assert_eq!(w.data_field_mut().len(), 32 - 6 - 4 - 2 - 1 - 4 - 2);
        w.data_field_mut().fill(0xD0);
        w.security_trailer_mut().copy_from_slice(&[0x5A]);
        w.set_ocf(Ocf::decode([0x01, 0, 0, 0])).unwrap();
        assert_eq!(w.finish(), 32);
        assert_eq!(out[32..], [0xEE; 8], "nothing written past the frame");
        assert_eq!(syndrome(&out[..32]), 0);
        // SCID 0x0AB, VC 0, OCF 1; FSH 1, sync 0, POF 0, SLID 11, FHP 3; FSH ID 0x03.
        assert_eq!(out[..7], [0x0A, 0xB1, 7, 9, 0x98, 0x03, 0x03]);
        let pc = *mc.pc();
        let f = TmFrame::decode_with(&out[..32], &pc, &mc, DecodeMode::Strict).unwrap();
        assert_eq!(f.secondary_header_data(), Some(&[1, 2, 3][..]));
        assert_eq!(f.security_header(), &[0xA5, 0xA5]);
        assert_eq!(f.security_trailer(), &[0x5A]);
        assert_eq!(f.ocf(), Some(Ocf::decode([0x01, 0, 0, 0])));
        assert_eq!(
            f.data_field_status(),
            DataFieldStatus::Packets(PacketPointer::StartsAt(3))
        );
        f.validate(&mc).unwrap();
    }

    #[test]
    fn unfinished_frame_is_defined() {
        // req: API-023
        let mc = mc();
        let mut out = [0xEE_u8; 32];
        TmFrameWriter::begin(&mut out, &mc, &packets(0, PacketPointer::NoPacketStart)).unwrap();
        assert_eq!(out[..7], [0x0A, 0xB1, 7, 9, 0x9F, 0xFF, 0x03]);
        assert!(out[7..].iter().all(|&b| b == 0));
    }

    #[test]
    fn resume_keeps_what_the_caller_filled() {
        // req: TM-009, TM-010, API-022, API-023, GEN-042
        let mc = mc();
        let fields = packets(0, PacketPointer::StartsAt(3));
        let fill = |w: &mut TmFrameWriter<'_>| {
            w.secondary_header_data_mut().copy_from_slice(&[1, 2, 3]);
            w.data_field_mut().fill(0xD0);
            w.set_ocf(Ocf::decode([0x01, 0, 0, 0])).unwrap();
        };
        let mut one = [0_u8; 32];
        let mut w = TmFrameWriter::begin(&mut one, &mc, &fields).unwrap();
        fill(&mut w);
        assert_eq!(w.finish(), 32);
        // The same frame in two steps that keep no writer, with the headers overwritten between.
        let mut two = [0_u8; 32];
        fill(&mut TmFrameWriter::begin(&mut two, &mc, &fields).unwrap());
        two[..7].fill(0xEE);
        let w = TmFrameWriter::resume(&mut two, &mc, &fields).unwrap();
        assert_eq!(w.finish(), 32);
        assert_eq!(two, one);
        // An OID frame: `begin_oid`, then `resume` with the status it uses.
        let mut oid = OidGenerator::new();
        let mut three = [0_u8; 32];
        let data = TmFrameWriter::begin_oid(&mut three, &mc, vcid(0), 7, 9, &mut oid)
            .unwrap()
            .layout()
            .data();
        let idle = packets(0, PacketPointer::OnlyIdleData);
        let w = TmFrameWriter::resume(&mut three, &mc, &idle).unwrap();
        assert_eq!(w.finish(), 32);
        assert_eq!(three[data.start..data.start + 4], [0xFF; 4]);
        assert_eq!(syndrome(&three), 0);
        // Rejected as `begin` rejects them, with the buffer left as it was.
        let mut four = [0xEE_u8; 32];
        let e = TmFrameWriter::resume(&mut four, &mc, &packets(2, PacketPointer::NoPacketStart))
            .unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::UnknownChannel, Some(field::VC_ID))
        );
        let e = TmFrameWriter::resume(&mut four[..31], &mc, &idle).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::BufferTooSmall);
        assert!(four.iter().all(|&b| b == 0xEE));
    }

    #[test]
    fn begin_rejects_without_writing() {
        // req: TM-009, TM-011, API-022, GEN-042
        let mc = mc();
        let mut out = [0xEE_u8; 32];
        let check = |out: &mut [u8], fields: &TmFrameFields, kind, field| {
            let e = TmFrameWriter::begin(out, &mc, fields).unwrap_err();
            assert_eq!((e.kind(), e.field()), (kind, field));
            assert!(out.iter().all(|&b| b == 0xEE));
            e
        };
        check(
            &mut out,
            &packets(2, PacketPointer::NoPacketStart),
            ErrorKind::UnknownChannel,
            Some(field::VC_ID),
        );
        let e = check(
            &mut out,
            &packets(1, PacketPointer::NoPacketStart),
            ErrorKind::InvalidField,
            Some(field::SYNC_FLAG),
        );
        assert_eq!(
            e.detail(),
            ErrorDetail::Expected {
                expected: 1,
                found: 0
            }
        );
        let vca = TmFrameFields::new(
            vcid(0),
            0,
            0,
            DataFieldStatus::VcaSdu {
                packet_order_flag: false,
                segment_length_id: SegmentLengthId::NO_SEGMENTS,
                first_header_pointer: FirstHeaderPointer::NO_PACKET_START,
            },
        );
        check(
            &mut out,
            &vca,
            ErrorKind::InvalidField,
            Some(field::SYNC_FLAG),
        );
        // Data field of VC 0: 13 octets, so offsets 0–12.
        let e = check(
            &mut out,
            &packets(0, PacketPointer::StartsAt(13)),
            ErrorKind::OutOfRange,
            Some(field::FIRST_HEADER_POINTER),
        );
        assert_eq!(
            e.detail(),
            ErrorDetail::Expected {
                expected: 12,
                found: 13
            }
        );
        let e = TmFrameWriter::begin(
            &mut out[..31],
            &mc,
            &packets(0, PacketPointer::StartsAt(12)),
        )
        .unwrap_err();
        assert_eq!(
            (e.kind(), e.detail()),
            (ErrorKind::BufferTooSmall, ErrorDetail::Needed(32))
        );
        assert!(
            TmFrameWriter::begin(&mut out, &mc, &packets(0, PacketPointer::StartsAt(12))).is_ok()
        );
    }

    #[test]
    fn offsets_stop_below_the_reserved_pointer_values() {
        // req: TM-009, TM-008
        let pc = TmPhysicalChannelConfig::new(2100, false).unwrap();
        let mc = TmMasterChannelConfig::new(pc, SpacecraftId::new(1).unwrap(), None, false)
            .with_vc(TmVirtualChannelConfig::new(
                vcid(0),
                DataFieldContent::Packets,
            ))
            .unwrap();
        let mut out = vec![0; 2100];
        assert!(
            TmFrameWriter::begin(&mut out, &mc, &packets(0, PacketPointer::StartsAt(0x7FD)))
                .is_ok()
        );
        let e = TmFrameWriter::begin(&mut out, &mc, &packets(0, PacketPointer::StartsAt(0x7FE)))
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
    }

    #[test]
    fn vca_sdu_fields_are_written_raw() {
        // req: TM-009, TM-007
        let mc = mc();
        let mut out = [0_u8; 32];
        let fields = TmFrameFields::new(
            vcid(1),
            0xFF,
            0x80,
            DataFieldStatus::VcaSdu {
                packet_order_flag: true,
                segment_length_id: SegmentLengthId::new(2).unwrap(),
                first_header_pointer: FirstHeaderPointer::new(0x123).unwrap(),
            },
        );
        let data = [0x55; 20];
        let n = encode(
            &mc,
            &fields,
            &TmFrameContent {
                ocf: Some(Ocf::decode([0x80, 1, 2, 3])),
                ..TmFrameContent::new(&data)
            },
            &mut out,
        )
        .unwrap();
        assert_eq!(n, 32);
        // SCID 0x0AB, VC 1, OCF 1; FSH 0, sync 1, POF 1, SLID 10, FHP 0x123.
        assert_eq!(out[..6], [0x0A, 0xB3, 0xFF, 0x80, 0x71, 0x23]);
        assert_eq!(out[26..30], [0x80, 1, 2, 3]);
        let f = TmFrame::decode(&out, mc.pc(), DecodeMode::Strict).unwrap();
        assert_eq!(f.data_field_status(), fields.status);
        assert_eq!(f.data_field(), &data);
    }

    #[test]
    fn encode_checks_every_length() {
        // req: TM-009, API-021, API-022
        let mc = mc();
        let mut out = [0xEE_u8; 32];
        let data = [0; 13];
        let ok = TmFrameContent {
            secondary_header_data: &[1, 2, 3],
            security_header: &[0, 0],
            data: &data,
            security_trailer: &[0],
            ocf: Some(Ocf::decode([0; 4])),
        };
        let fields = packets(0, PacketPointer::StartsAt(0));
        for (content, f, expected, found) in [
            (
                TmFrameContent {
                    secondary_header_data: &[1, 2],
                    ..ok
                },
                field::FSH_LEN,
                3,
                2,
            ),
            (
                TmFrameContent {
                    security_header: &[0],
                    ..ok
                },
                field::SECURITY_HEADER_LEN,
                2,
                1,
            ),
            (
                TmFrameContent {
                    data: &data[1..],
                    ..ok
                },
                field::FRAME_DATA_FIELD,
                13,
                12,
            ),
            (
                TmFrameContent {
                    security_trailer: &[],
                    ..ok
                },
                field::SECURITY_TRAILER_LEN,
                1,
                0,
            ),
        ] {
            let e = encode(&mc, &fields, &content, &mut out).unwrap_err();
            assert_eq!(
                (e.kind(), e.field(), e.detail()),
                (
                    ErrorKind::LengthMismatch,
                    Some(f),
                    ErrorDetail::Expected { expected, found }
                )
            );
        }
        let e = encode(&mc, &fields, &TmFrameContent { ocf: None, ..ok }, &mut out).unwrap_err();
        assert_eq!(
            (e.kind(), e.field(), e.detail()),
            (
                ErrorKind::InvalidField,
                Some(field::OCF_FLAG),
                ErrorDetail::Expected {
                    expected: 1,
                    found: 0
                }
            )
        );
        let e = encode(
            &mc,
            &packets(3, PacketPointer::NoPacketStart),
            &ok,
            &mut out,
        )
        .unwrap_err();
        assert_eq!(e.kind(), ErrorKind::UnknownChannel);
        let e = encode(&mc, &fields, &ok, &mut out[..10]).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::BufferTooSmall);
        assert!(out.iter().all(|&b| b == 0xEE), "out unchanged on error");
        assert_eq!(encode(&mc, &fields, &ok, &mut out), Ok(32));
    }

    #[test]
    fn set_ocf_without_an_ocf() {
        // req: TM-009
        let pc = TmPhysicalChannelConfig::new(16, false).unwrap();
        let mc = TmMasterChannelConfig::new(pc, SpacecraftId::new(1).unwrap(), None, false)
            .with_vc(TmVirtualChannelConfig::new(
                vcid(0),
                DataFieldContent::Packets,
            ))
            .unwrap();
        let mut out = [0_u8; 16];
        let mut w =
            TmFrameWriter::begin(&mut out, &mc, &packets(0, PacketPointer::NoPacketStart)).unwrap();
        let e = w.set_ocf(Ocf::decode([0; 4])).unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::InvalidField, Some(field::OCF_FLAG))
        );
        assert_eq!(w.secondary_header_data_mut().len(), 0);
        assert_eq!(w.finish(), 16);
        let f = TmFrame::decode(&out, &pc, DecodeMode::Strict).unwrap();
        assert_eq!((f.fecf(), f.ocf()), (None, None));
        let mut again = [0_u8; 16];
        let content = TmFrameContent::new(&out[6..]);
        let n = encode(
            &mc,
            &packets(0, PacketPointer::NoPacketStart),
            &content,
            &mut again,
        );
        assert_eq!((n, again), (Ok(16), out));
    }

    #[test]
    fn oid_frames_continue_the_sequence() {
        // req: TM-010, CMN-020, CMN-021, CMN-023, GEN-042
        let mc = mc();
        let mut oid = OidGenerator::new();
        let mut first = [0_u8; 32];
        let mut second = [0_u8; 32];
        let writer = TmFrameWriter::begin_oid(&mut first, &mc, vcid(0), 1, 1, &mut oid).unwrap();
        assert_eq!(writer.finish(), 32);
        let writer = TmFrameWriter::begin_oid(&mut second, &mc, vcid(0), 2, 2, &mut oid).unwrap();
        assert_eq!(writer.finish(), 32);
        let mut reference = OidGenerator::new();
        let mut expected = [0_u8; 26];
        reference.fill(&mut expected);
        let one = TmFrame::decode_with(&first, mc.pc(), &mc, DecodeMode::Strict).unwrap();
        let two = TmFrame::decode_with(&second, mc.pc(), &mc, DecodeMode::Strict).unwrap();
        assert_eq!(one.data_field(), &expected[..13]);
        assert_eq!(two.data_field(), &expected[13..]);
        assert_eq!(one.data_field()[..4], [0xFF; 4]);
        assert_eq!(
            one.data_field_status(),
            DataFieldStatus::Packets(PacketPointer::OnlyIdleData)
        );
        // Rejected on a `VCA_SDU` channel without advancing the generator.
        let before = oid.clone();
        let mut third = [0_u8; 32];
        let e = TmFrameWriter::begin_oid(&mut third, &mc, vcid(1), 3, 3, &mut oid).unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::InvalidField, Some(field::SYNC_FLAG))
        );
        assert_eq!(oid, before);
    }
}

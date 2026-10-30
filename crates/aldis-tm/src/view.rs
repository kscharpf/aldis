//! Zero-copy TM Transfer Frame view (132.0-B-3 §4.1, §6.3; TM-001…TM-008, API-010, API-012…014,
//! API-072, GEN-013, GEN-043, GEN-053).

use core::ops::Range;

use aldis_core::crc::verify_fecf;
use aldis_core::ids::{SpacecraftId, VcId};
use aldis_core::layout::{FrameLayout, LayoutSpec, OCF_LEN};
use aldis_core::ocf::Ocf;
use aldis_core::security::SecurityLayout;
use aldis_core::{AnomalySet, DecodeMode, Error, ErrorKind, Lenient, Result};

use crate::anomaly::TmAnomaly;
use crate::config::{NoSdls, TmPhysicalChannelConfig, VcLookup};
use crate::field;
use crate::header::{
    DataFieldStatus, FSH_VERSION, FshLen, PRIMARY_HEADER_LEN, PacketPointer, SegmentLengthId,
    TmPrimaryHeader, VERSION, len_u64, mismatch,
};

/// A decoded TM Transfer Frame borrowing its fields from the input (132.0 §4.1, §6.3; TM-001…
/// TM-008, GEN-013).
///
/// Holding one guarantees that the frame has the configured length, a correct FECF if the channel
/// has one, version `00`, a well-formed Secondary Header, a non-empty data field, and a First
/// Header Pointer inside the data field (API-010). A frame whose FECF was not checked is an
/// [`UncheckedTmFrame`], which becomes a `TmFrame` only through
/// [`check_fecf`](UncheckedTmFrame::check_fecf) (API-015). A strict decode returns the frame alone;
/// a lenient one returns it in a [`Lenient`] with its anomalies (API-030).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmFrame<'a> {
    bytes: &'a [u8],
    header: TmPrimaryHeader,
    fsh_len: Option<FshLen>,
    layout: FrameLayout,
    status: DataFieldStatus,
}

impl<'a> TmFrame<'a> {
    /// Decodes a frame of a physical channel on which no virtual channel uses SDLS (132.0 §4.1;
    /// TM-001…TM-008), in strict mode. The same as [`decode_with`](Self::decode_with) and
    /// [`NoSdls`].
    ///
    /// It has no knowledge of the virtual channels, so it cannot tell whether an Only Idle Data
    /// frame is on a channel that uses SDLS, and never reports that (TM-010, HZ-TM-07, OI-54);
    /// use [`decode_with`](Self::decode_with) for such a channel.
    ///
    /// # Errors
    /// As for [`decode_with`](Self::decode_with), except `UnknownChannel`.
    pub fn decode(bytes: &'a [u8], pc: &TmPhysicalChannelConfig) -> Result<TmFrame<'a>> {
        TmFrame::decode_with(bytes, pc, &NoSdls)
    }

    /// Decodes a frame as [`decode`](Self::decode) does, in lenient mode: the deviations that
    /// [`decode_with_lenient`](Self::decode_with_lenient) lists are accepted and reported
    /// (TM-007, API-032).
    ///
    /// # Errors
    /// As for [`decode`](Self::decode), except the deviations that are anomalies.
    pub fn decode_lenient(
        bytes: &'a [u8],
        pc: &TmPhysicalChannelConfig,
    ) -> Result<Lenient<TmFrame<'a>, TmAnomaly>> {
        TmFrame::decode_with_lenient(bytes, pc, &NoSdls)
    }

    /// Decodes one frame in strict mode, taking the Security Header and Trailer lengths of its
    /// virtual channel from `vcs` (132.0 §4.1, §6.3; TM-001…TM-008, API-012, API-070).
    ///
    /// The Secondary Header and the OCF are placed by the frame's own flags; whether they match
    /// the configuration is checked by [`validate`](Self::validate) (TM-011).
    ///
    /// # Errors
    /// In this order:
    /// - `LengthMismatch` (`frame_len`) if `bytes` is not the configured frame length (TM-001,
    ///   API-012);
    /// - `CrcMismatch` if the FECF is configured and wrong, before any field is read (TM-005);
    /// - `UnsupportedVersion` (`frame_version`) for a version other than `00` (TM-002);
    /// - `UnknownChannel` (`spacecraft_id` or `vc_id`) if `vcs` has no such channel (API-070);
    /// - `UnsupportedVersion` (`fsh_version`) for a Secondary Header version other than `00`,
    ///   `InvalidField` (`fsh_len`) for a Secondary Header Length of 0, which leaves no data
    ///   octet, and `Malformed` (`fsh_len`) for a Secondary Header that runs past the frame
    ///   (TM-003);
    /// - `Malformed` (`frame_data_field`) if the fields leave no data octet (TM-006, CMN-041);
    /// - `Malformed` (`first_header_pointer`) with the Synchronization Flag `0` for a pointer
    ///   outside the data field that is neither `0x7FE` nor `0x7FF` (TM-008);
    /// - with the Synchronization Flag `0`, `ReservedValue` (`packet_order_flag`) for a Packet
    ///   Order Flag `1` and `InvalidField` (`segment_length_id`) for a Segment Length Identifier
    ///   other than `11` (TM-007);
    /// - last, with the Synchronization Flag `0` and the First Header Pointer `0x7FE` (Only Idle
    ///   Data) on a virtual channel whose security layout is not empty, `InvalidField` (`vc_id`)
    ///   (132.0 §6.4.6.2; TM-010, OI-54).
    pub fn decode_with<L: VcLookup + ?Sized>(
        bytes: &'a [u8],
        pc: &TmPhysicalChannelConfig,
        vcs: &L,
    ) -> Result<TmFrame<'a>> {
        TmFrame::decode_in(bytes, pc, vcs, pc.fecf(), DecodeMode::Strict).map(|(frame, _)| frame)
    }

    /// Decodes one frame as [`decode_with`](Self::decode_with) does, in lenient mode: a Packet
    /// Order Flag `1` and a Segment Length Identifier other than `11`, both with the
    /// Synchronization Flag `0`, and an Only Idle Data frame on a virtual channel that uses
    /// SDLS are accepted and reported as [`TmAnomaly`] flags (TM-007, TM-010, API-032).
    ///
    /// # Errors
    /// As for [`decode_with`](Self::decode_with), except the deviations that are anomalies.
    pub fn decode_with_lenient<L: VcLookup + ?Sized>(
        bytes: &'a [u8],
        pc: &TmPhysicalChannelConfig,
        vcs: &L,
    ) -> Result<Lenient<TmFrame<'a>, TmAnomaly>> {
        TmFrame::decode_in(bytes, pc, vcs, pc.fecf(), DecodeMode::Lenient)
            .map(|(frame, anomalies)| Lenient::new(frame, anomalies))
    }

    /// Decodes one frame as [`decode_with`](Self::decode_with) does, in strict mode, but without
    /// checking the FECF: for diagnostics of frames that failed it (TM-005, API-015). The result
    /// is an [`UncheckedTmFrame`], which is not a `TmFrame` until
    /// [`check_fecf`](UncheckedTmFrame::check_fecf) has verified the FECF.
    ///
    /// # Errors
    /// As for [`decode_with`](Self::decode_with), except `CrcMismatch`.
    pub fn decode_unchecked<L: VcLookup + ?Sized>(
        bytes: &'a [u8],
        pc: &TmPhysicalChannelConfig,
        vcs: &L,
    ) -> Result<UncheckedTmFrame<'a>> {
        TmFrame::decode_in(bytes, pc, vcs, false, DecodeMode::Strict)
            .map(|(frame, _)| UncheckedTmFrame { frame })
    }

    /// Decodes one frame as [`decode_unchecked`](Self::decode_unchecked) does, in lenient mode
    /// (TM-005, API-032, API-015).
    ///
    /// # Errors
    /// As for [`decode_with_lenient`](Self::decode_with_lenient), except `CrcMismatch`.
    pub fn decode_unchecked_lenient<L: VcLookup + ?Sized>(
        bytes: &'a [u8],
        pc: &TmPhysicalChannelConfig,
        vcs: &L,
    ) -> Result<Lenient<UncheckedTmFrame<'a>, TmAnomaly>> {
        TmFrame::decode_in(bytes, pc, vcs, false, DecodeMode::Lenient)
            .map(|(frame, anomalies)| Lenient::new(UncheckedTmFrame { frame }, anomalies))
    }

    /// The decode of every entry point: the length and Primary Header checks, the FECF check if
    /// `fecf`, the security layout of the virtual channel, and the rest.
    fn decode_in<L: VcLookup + ?Sized>(
        bytes: &'a [u8],
        pc: &TmPhysicalChannelConfig,
        vcs: &L,
        fecf: bool,
        mode: DecodeMode,
    ) -> Result<(TmFrame<'a>, AnomalySet<TmAnomaly>)> {
        let header = TmFrame::check(bytes, pc, fecf)?;
        let security = vcs.security_layout(header.scid, header.vcid)?;
        TmFrame::parse(bytes, pc, header, security, mode)
    }

    /// Length of the frame that starts `prefix`: the configured frame length (132.0 §4.1.1.2;
    /// API-013, M2 plan D14). The length depends on no field, so any prefix will do; if it holds
    /// the first octet, its Transfer Frame Version Number is checked as
    /// [`decode`](Self::decode) checks it, so the two never disagree on a header.
    ///
    /// # Errors
    /// `UnsupportedVersion` (`frame_version`) if the version is not `00` (TM-002).
    pub fn peek_length(prefix: &[u8], pc: &TmPhysicalChannelConfig) -> Result<usize> {
        if let Some(&octet) = prefix.first()
            && octet >> 6 != VERSION
        {
            return Err(mismatch(
                ErrorKind::UnsupportedVersion,
                field::FRAME_VERSION,
                u64::from(VERSION),
                u64::from(octet >> 6),
            ));
        }
        Ok(pc.frame_len())
    }

    /// The length check, the FECF check if `fecf`, and the Primary Header (TM-001, TM-002,
    /// TM-005). Not generic, like [`parse`](Self::parse), so that one copy of each is measured
    /// (VER-024).
    fn check(bytes: &[u8], pc: &TmPhysicalChannelConfig, fecf: bool) -> Result<TmPrimaryHeader> {
        check_len(bytes, pc)?;
        if fecf {
            verify_fecf(bytes)?;
        }
        TmPrimaryHeader::decode(bytes)
    }

    /// Everything after the Primary Header and the security layout.
    fn parse(
        bytes: &'a [u8],
        pc: &TmPhysicalChannelConfig,
        header: TmPrimaryHeader,
        security: SecurityLayout,
        mode: DecodeMode,
    ) -> Result<(TmFrame<'a>, AnomalySet<TmAnomaly>)> {
        let fsh_len = if header.fsh_flag {
            Some(fsh_id(bytes)?)
        } else {
            None
        };
        let layout = FrameLayout::compute(&LayoutSpec {
            frame_len: bytes.len(),
            primary_header_len: PRIMARY_HEADER_LEN,
            secondary_len: fsh_len.map_or(0, FshLen::octets),
            security,
            ocf: header.ocf_flag,
            fecf: pc.fecf(),
        })
        .map_err(|_| Error::new(ErrorKind::Malformed).with_field(field::FRAME_DATA_FIELD))?;
        let mut anomalies = AnomalySet::empty();
        let status = if header.sync_flag {
            DataFieldStatus::VcaSdu {
                packet_order_flag: header.packet_order_flag,
                segment_length_id: header.segment_length_id,
                first_header_pointer: header.first_header_pointer,
            }
        } else {
            let pointer =
                PacketPointer::classify(header.first_header_pointer, layout.data().len())?;
            if header.packet_order_flag {
                match mode {
                    DecodeMode::Strict => {
                        return Err(mismatch(
                            ErrorKind::ReservedValue,
                            field::PACKET_ORDER_FLAG,
                            0,
                            1,
                        ));
                    }
                    DecodeMode::Lenient => anomalies.insert(TmAnomaly::PacketOrderFlagSet),
                }
            }
            if header.segment_length_id != SegmentLengthId::NO_SEGMENTS {
                match mode {
                    DecodeMode::Strict => {
                        return Err(mismatch(
                            ErrorKind::InvalidField,
                            field::SEGMENT_LENGTH_ID,
                            u64::from(SegmentLengthId::MAX),
                            u64::from(header.segment_length_id.get()),
                        ));
                    }
                    DecodeMode::Lenient => anomalies.insert(TmAnomaly::SegmentLengthIdNot11),
                }
            }
            // An Only Idle Data frame uses a VCID that does not use SDLS (132.0 §6.4.6.2; TM-010).
            if pointer == PacketPointer::OnlyIdleData && !security.is_none() {
                match mode {
                    DecodeMode::Strict => {
                        return Err(Error::new(ErrorKind::InvalidField).with_field(field::VC_ID));
                    }
                    DecodeMode::Lenient => anomalies.insert(TmAnomaly::OnlyIdleDataOnSdlsVc),
                }
            }
            DataFieldStatus::Packets(pointer)
        };
        let frame = TmFrame {
            bytes,
            header,
            fsh_len,
            layout,
            status,
        };
        Ok((frame, anomalies))
    }

    /// The octets of `range`, which lies inside the frame by construction (CMN-040).
    fn slice(&self, range: Range<usize>) -> &'a [u8] {
        self.bytes.get(range).unwrap_or_default()
    }

    /// The Primary Header as plain values (132.0 §4.1.2; API-014).
    #[must_use]
    pub const fn header(&self) -> TmPrimaryHeader {
        self.header
    }

    /// Spacecraft Identifier (132.0 §4.1.2.2.3).
    #[must_use]
    pub const fn scid(&self) -> SpacecraftId<10> {
        self.header.scid
    }

    /// Virtual Channel Identifier (132.0 §4.1.2.3).
    #[must_use]
    pub const fn vcid(&self) -> VcId<3> {
        self.header.vcid
    }

    /// Master Channel Frame Count (132.0 §4.1.2.5).
    #[must_use]
    pub const fn mc_frame_count(&self) -> u8 {
        self.header.mc_frame_count
    }

    /// Virtual Channel Frame Count (132.0 §4.1.2.6).
    #[must_use]
    pub const fn vc_frame_count(&self) -> u8 {
        self.header.vc_frame_count
    }

    /// The Synchronization Flag with the First Header Pointer classified (Synchronization Flag
    /// `0`) or the undefined fields raw (flag `1`) (132.0 §4.1.2.7.3–6; TM-007, TM-008).
    #[must_use]
    pub const fn data_field_status(&self) -> DataFieldStatus {
        self.status
    }

    /// Total Secondary Header length, if the frame has one (132.0 §4.1.3.2.3; TM-003).
    #[must_use]
    pub const fn fsh_len(&self) -> Option<FshLen> {
        self.fsh_len
    }

    /// The Secondary Header Data Field, after the Identification Field; `None` if the frame has
    /// no Secondary Header (132.0 §4.1.3.3; TM-003, GEN-013).
    #[must_use]
    pub fn secondary_header_data(&self) -> Option<&'a [u8]> {
        self.fsh_len.map(|_| {
            self.slice(self.layout.secondary())
                .get(1..)
                .unwrap_or_default()
        })
    }

    /// The Security Header, opaque; empty without SDLS (132.0 §6.3.4; API-072).
    #[must_use]
    pub fn security_header(&self) -> &'a [u8] {
        self.slice(self.layout.security_header())
    }

    /// The Transfer Frame Data Field, never empty (132.0 §4.1.4, §6.3.5; TM-006, GEN-013).
    #[must_use]
    pub fn data_field(&self) -> &'a [u8] {
        self.slice(self.layout.data())
    }

    /// The Security Trailer, opaque; empty without SDLS (132.0 §6.3.6; API-072).
    #[must_use]
    pub fn security_trailer(&self) -> &'a [u8] {
        self.slice(self.layout.security_trailer())
    }

    /// The Operational Control Field, classified, if the OCF Flag is set (132.0 §4.1.5; TM-004,
    /// CMN-010). A Type-1 Report is decoded with `aldis-tc` (CMN-011).
    #[must_use]
    pub fn ocf(&self) -> Option<Ocf> {
        <[u8; OCF_LEN]>::try_from(self.slice(self.layout.ocf()))
            .ok()
            .map(Ocf::decode)
    }

    /// The Frame Error Control Field, if the physical channel has one (132.0 §4.1.6; TM-005).
    #[must_use]
    pub fn fecf(&self) -> Option<u16> {
        <[u8; 2]>::try_from(self.slice(self.layout.fecf()))
            .ok()
            .map(u16::from_be_bytes)
    }

    /// The octet ranges of the frame's fields, for a security hook (CMN-040, API-074).
    #[must_use]
    pub const fn layout(&self) -> &FrameLayout {
        &self.layout
    }

    /// The whole frame (132.0 §4.1.1).
    #[must_use]
    pub const fn as_bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// Octets consumed: the frame length (132.0 §4.1.1.2; API-012).
    #[must_use]
    pub const fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Always `false`: a frame holds at least its Primary Header and one data octet.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        false
    }
}

/// A TM Transfer Frame whose FECF was not checked (132.0 §4.1.6; TM-005, API-015).
///
/// [`TmFrame::decode_unchecked`] returns it for diagnostics of frames that failed the check. It
/// offers the read accessors of a [`TmFrame`] and nothing that takes a checked frame: it
/// becomes a `TmFrame` only through [`check_fecf`](Self::check_fecf). The other way, a `TmFrame`
/// converts into an `UncheckedTmFrame` with `From`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UncheckedTmFrame<'a> {
    frame: TmFrame<'a>,
}

impl<'a> UncheckedTmFrame<'a> {
    /// Verifies the FECF of a channel that has one, and returns the frame as a [`TmFrame`]
    /// (132.0 §4.1.6; TM-005, API-015). The same check as [`TmFrame::decode_with`] makes.
    ///
    /// # Errors
    /// `CrcMismatch` if the FECF is wrong.
    pub fn check_fecf(self) -> Result<TmFrame<'a>> {
        if self.frame.fecf().is_some() {
            verify_fecf(self.frame.bytes)?;
        }
        Ok(self.frame)
    }

    /// The Primary Header as plain values (132.0 §4.1.2; API-014).
    #[must_use]
    pub const fn header(&self) -> TmPrimaryHeader {
        self.frame.header()
    }

    /// Spacecraft Identifier (132.0 §4.1.2.2.3).
    #[must_use]
    pub const fn scid(&self) -> SpacecraftId<10> {
        self.frame.scid()
    }

    /// Virtual Channel Identifier (132.0 §4.1.2.3).
    #[must_use]
    pub const fn vcid(&self) -> VcId<3> {
        self.frame.vcid()
    }

    /// Master Channel Frame Count (132.0 §4.1.2.5).
    #[must_use]
    pub const fn mc_frame_count(&self) -> u8 {
        self.frame.mc_frame_count()
    }

    /// Virtual Channel Frame Count (132.0 §4.1.2.6).
    #[must_use]
    pub const fn vc_frame_count(&self) -> u8 {
        self.frame.vc_frame_count()
    }

    /// The Synchronization Flag with the First Header Pointer classified or raw, as for
    /// [`TmFrame::data_field_status`] (TM-007, TM-008).
    #[must_use]
    pub const fn data_field_status(&self) -> DataFieldStatus {
        self.frame.data_field_status()
    }

    /// Total Secondary Header length, if the frame has one (132.0 §4.1.3.2.3; TM-003).
    #[must_use]
    pub const fn fsh_len(&self) -> Option<FshLen> {
        self.frame.fsh_len()
    }

    /// The Secondary Header Data Field; `None` if the frame has no Secondary Header (132.0
    /// §4.1.3.3; TM-003).
    #[must_use]
    pub fn secondary_header_data(&self) -> Option<&'a [u8]> {
        self.frame.secondary_header_data()
    }

    /// The Security Header, opaque; empty without SDLS (132.0 §6.3.4; API-072).
    #[must_use]
    pub fn security_header(&self) -> &'a [u8] {
        self.frame.security_header()
    }

    /// The Transfer Frame Data Field, never empty (132.0 §4.1.4; TM-006).
    #[must_use]
    pub fn data_field(&self) -> &'a [u8] {
        self.frame.data_field()
    }

    /// The Security Trailer, opaque; empty without SDLS (132.0 §6.3.6; API-072).
    #[must_use]
    pub fn security_trailer(&self) -> &'a [u8] {
        self.frame.security_trailer()
    }

    /// The Operational Control Field, classified, if the OCF Flag is set (132.0 §4.1.5; TM-004).
    #[must_use]
    pub fn ocf(&self) -> Option<Ocf> {
        self.frame.ocf()
    }

    /// The Frame Error Control Field as received, if the physical channel has one, whether or
    /// not it is correct (132.0 §4.1.6; TM-005).
    #[must_use]
    pub fn fecf(&self) -> Option<u16> {
        self.frame.fecf()
    }

    /// The octet ranges of the frame's fields (CMN-040).
    #[must_use]
    pub const fn layout(&self) -> &FrameLayout {
        self.frame.layout()
    }

    /// The whole frame (132.0 §4.1.1).
    #[must_use]
    pub const fn as_bytes(&self) -> &'a [u8] {
        self.frame.as_bytes()
    }

    /// Octets consumed: the frame length (132.0 §4.1.1.2; API-012).
    #[must_use]
    pub const fn len(&self) -> usize {
        self.frame.len()
    }

    /// Always `false`: a frame holds at least its Primary Header and one data octet.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        false
    }
}

impl<'a> From<TmFrame<'a>> for UncheckedTmFrame<'a> {
    /// Forgets that the FECF was checked, to read a checked frame through the accessors of
    /// the unchecked one (API-015).
    fn from(frame: TmFrame<'a>) -> UncheckedTmFrame<'a> {
        UncheckedTmFrame { frame }
    }
}

/// `LengthMismatch` (`frame_len`) unless `bytes` is one frame of `pc` (TM-001, API-012).
const fn check_len(bytes: &[u8], pc: &TmPhysicalChannelConfig) -> Result<()> {
    if bytes.len() == pc.frame_len() {
        Ok(())
    } else {
        Err(mismatch(
            ErrorKind::LengthMismatch,
            field::FRAME_LEN,
            len_u64(pc.frame_len()),
            len_u64(bytes.len()),
        ))
    }
}

/// The Secondary Header length from its Identification Field (132.0 §4.1.3.2; TM-003).
fn fsh_id(bytes: &[u8]) -> Result<FshLen> {
    let id = bytes.get(PRIMARY_HEADER_LEN).copied().unwrap_or_default();
    let version = id >> 6;
    if version != FSH_VERSION {
        return Err(mismatch(
            ErrorKind::UnsupportedVersion,
            field::FSH_VERSION,
            u64::from(FSH_VERSION),
            u64::from(version),
        ));
    }
    let total = (id & 0x3F).saturating_add(1);
    let len = FshLen::new(total).ok_or(mismatch(
        ErrorKind::InvalidField,
        field::FSH_LEN,
        u64::from(FshLen::MIN),
        u64::from(total),
    ))?;
    if PRIMARY_HEADER_LEN.saturating_add(len.octets()) > bytes.len() {
        return Err(mismatch(
            ErrorKind::Malformed,
            field::FSH_LEN,
            len_u64(bytes.len().saturating_sub(PRIMARY_HEADER_LEN)),
            u64::from(total),
        ));
    }
    Ok(len)
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    extern crate std;
    use std::vec;
    use std::vec::Vec;

    use super::{TmFrame, UncheckedTmFrame};
    use crate::anomaly::TmAnomaly;
    use crate::config::{
        DataFieldContent, NoSdls, TmMasterChannelConfig, TmPhysicalChannelConfig,
        TmVirtualChannelConfig,
    };
    use crate::field;
    use crate::header::tests::starts_at;
    use crate::header::{
        DataFieldStatus, FirstHeaderPointer, FshLen, PacketPointer, SegmentLengthId,
    };
    use aldis_core::crc::write_fecf;
    use aldis_core::ids::{SpacecraftId, VcId};
    use aldis_core::ocf::Ocf;
    use aldis_core::security::SecurityLayout;
    use aldis_core::{AnomalySet, ErrorDetail, ErrorKind};

    fn pc(len: usize, fecf: bool) -> TmPhysicalChannelConfig {
        TmPhysicalChannelConfig::new(len, fecf).unwrap()
    }

    /// A 16-octet frame without FECF: SCID 0x2A5, VC 5, counts 1 and 2, FHP 0, data `D0..`.
    fn plain() -> Vec<u8> {
        let mut f = vec![0x2A, 0x5A, 0x01, 0x02, 0x18, 0x00];
        f.extend(0xD0..0xDA_u8);
        f
    }

    fn err<T: core::fmt::Debug>(
        r: aldis_core::Result<T>,
    ) -> (ErrorKind, Option<aldis_core::FieldId>) {
        let e = r.unwrap_err();
        (e.kind(), e.field())
    }

    #[test]
    fn minimal_frame_accessors() {
        // req: TM-001, TM-006, TM-008, API-010, API-014, GEN-013
        let bytes = plain();
        let f = TmFrame::decode(&bytes, &pc(16, false)).unwrap();
        assert_eq!(f.scid(), SpacecraftId::new(0x2A5).unwrap());
        assert_eq!(f.vcid(), VcId::new(5).unwrap());
        assert_eq!((f.mc_frame_count(), f.vc_frame_count()), (1, 2));
        assert_eq!(
            f.data_field_status(),
            DataFieldStatus::Packets(starts_at(0))
        );
        assert_eq!(f.data_field(), &bytes[6..]);
        assert_eq!(f.data_field().as_ptr(), bytes[6..].as_ptr(), "zero-copy");
        assert_eq!((f.fsh_len(), f.secondary_header_data()), (None, None));
        assert!(f.security_header().is_empty() && f.security_trailer().is_empty());
        assert_eq!((f.ocf(), f.fecf()), (None, None));
        assert_eq!(
            (f.len(), f.is_empty(), f.as_bytes()),
            (16, false, &bytes[..])
        );
        assert_eq!(f.layout().data(), 6..16);
        assert_eq!(f.header().to_octets(), bytes[..6]);
    }

    #[test]
    fn length_and_fecf_are_checked_first() {
        // req: TM-001, TM-005, API-012, CMN-003
        let mut bytes = plain();
        bytes.extend([0, 0]);
        write_fecf(&mut bytes).unwrap();
        let channel = pc(18, true);
        let f = TmFrame::decode(&bytes, &channel).unwrap();
        assert_eq!(f.fecf(), Some(u16::from_be_bytes([bytes[16], bytes[17]])));
        assert_eq!(f.data_field(), &bytes[6..16]);
        let e = TmFrame::decode(&bytes[..17], &channel).unwrap_err();
        assert_eq!(
            (e.kind(), e.field(), e.detail()),
            (
                ErrorKind::LengthMismatch,
                Some(field::FRAME_LEN),
                ErrorDetail::Expected {
                    expected: 18,
                    found: 17
                }
            )
        );
        assert_eq!(
            err(TmFrame::decode_unchecked(&bytes[..17], &channel, &NoSdls)).0,
            ErrorKind::LengthMismatch
        );
        // A corrupted version is reported as a CRC failure: the FECF is checked first.
        let mut bad = bytes.clone();
        bad[0] |= 0x40;
        assert_eq!(
            err(TmFrame::decode(&bad, &channel)).0,
            ErrorKind::CrcMismatch
        );
        assert_eq!(
            err(TmFrame::decode_unchecked(&bad, &channel, &NoSdls)),
            (ErrorKind::UnsupportedVersion, Some(field::FRAME_VERSION))
        );
        bad[0] = bytes[0];
        bad[10] ^= 1;
        let f = TmFrame::decode_unchecked(&bad, &channel, &NoSdls).unwrap();
        assert_eq!(f.data_field()[4], bytes[10] ^ 1);
        // API-015: the unchecked frame is a `TmFrame` only after the FECF check.
        assert_eq!(err(f.check_fecf()).0, ErrorKind::CrcMismatch);
        let ok = TmFrame::decode_unchecked(&bytes, &channel, &NoSdls).unwrap();
        assert_eq!(ok.check_fecf(), TmFrame::decode(&bytes, &channel));
        let short = plain();
        let no_fecf = TmFrame::decode_unchecked(&short, &pc(16, false), &NoSdls).unwrap();
        assert_eq!(no_fecf.fecf(), None);
        assert!(no_fecf.check_fecf().is_ok());
    }

    #[test]
    fn the_unchecked_frame_reads_like_the_checked_one() {
        // req: API-015, API-014, TM-003, TM-004, TM-006, API-072
        let mut bytes = vec![0x00, 0x13, 0x07, 0x08, 0x98, 0x00, 0x02, 0xAA, 0xBB];
        bytes.extend([0xD0, 0xD1, 0xD2, 0xD3, 0xD4, 0x01, 0x02, 0x03, 0x04]);
        let channel = pc(18, false);
        let checked = TmFrame::decode(&bytes, &channel).unwrap();
        let u = UncheckedTmFrame::from(checked.clone());
        assert_eq!(
            u,
            TmFrame::decode_unchecked(&bytes, &channel, &NoSdls).unwrap()
        );
        assert_eq!(u.header(), checked.header());
        assert_eq!((u.scid(), u.vcid()), (checked.scid(), checked.vcid()));
        assert_eq!((u.mc_frame_count(), u.vc_frame_count()), (7, 8));
        assert_eq!(u.data_field_status(), checked.data_field_status());
        assert_eq!(u.fsh_len(), checked.fsh_len());
        assert_eq!(u.secondary_header_data(), Some(&[0xAA, 0xBB][..]));
        assert_eq!(
            (u.security_header(), u.security_trailer()),
            (&[][..], &[][..])
        );
        assert_eq!(u.data_field(), &[0xD0, 0xD1, 0xD2, 0xD3, 0xD4]);
        assert_eq!(u.ocf(), Some(Ocf::decode([1, 2, 3, 4])));
        assert_eq!(u.fecf(), None);
        assert_eq!(u.layout(), checked.layout());
        assert_eq!(
            (u.as_bytes(), u.len(), u.is_empty()),
            (&bytes[..], 18, false)
        );
        assert_eq!(u.check_fecf(), Ok(checked));
    }

    #[test]
    fn secondary_header_and_ocf_follow_the_flags() {
        // req: TM-003, TM-004, TM-006
        // FSH of 3 octets (ID 0x02, data AA BB), data 5 octets, OCF 01 02 03 04.
        let mut bytes = vec![0x00, 0x13, 0x00, 0x00, 0x98, 0x00, 0x02, 0xAA, 0xBB];
        bytes.extend([0xD0, 0xD1, 0xD2, 0xD3, 0xD4, 0x01, 0x02, 0x03, 0x04]);
        let f = TmFrame::decode(&bytes, &pc(18, false)).unwrap();
        assert_eq!(f.fsh_len().map(FshLen::get), Some(3));
        assert_eq!(f.secondary_header_data(), Some(&[0xAA, 0xBB][..]));
        assert_eq!(f.data_field(), &[0xD0, 0xD1, 0xD2, 0xD3, 0xD4]);
        assert_eq!(f.ocf(), Some(Ocf::decode([1, 2, 3, 4])));
        assert!(f.header().ocf_flag && f.header().fsh_flag);
    }

    #[test]
    fn secondary_header_errors() {
        // req: TM-003, GEN-041
        let channel = pc(12, false);
        let frame = |id: u8| {
            let mut b = vec![0x00, 0x00, 0x00, 0x00, 0x98, 0x00, id];
            b.extend([0; 5]);
            b
        };
        let e = TmFrame::decode(&frame(0x41), &channel).unwrap_err();
        assert_eq!(
            (e.kind(), e.field(), e.detail()),
            (
                ErrorKind::UnsupportedVersion,
                Some(field::FSH_VERSION),
                ErrorDetail::Expected {
                    expected: 0,
                    found: 1
                }
            )
        );
        let e = TmFrame::decode(&frame(0x00), &channel).unwrap_err();
        assert_eq!(
            (e.kind(), e.field(), e.detail()),
            (
                ErrorKind::InvalidField,
                Some(field::FSH_LEN),
                ErrorDetail::Expected {
                    expected: 2,
                    found: 1
                }
            )
        );
        // 7 octets of FSH in a 12-octet frame run past its end.
        let e = TmFrame::decode(&frame(0x06), &channel).unwrap_err();
        assert_eq!(
            (e.kind(), e.field(), e.detail()),
            (
                ErrorKind::Malformed,
                Some(field::FSH_LEN),
                ErrorDetail::Expected {
                    expected: 6,
                    found: 7
                }
            )
        );
        // 6 octets fit, but leave no data octet.
        assert_eq!(
            err(TmFrame::decode(&frame(0x05), &channel)),
            (ErrorKind::Malformed, Some(field::FRAME_DATA_FIELD))
        );
        assert!(TmFrame::decode(&frame(0x04), &channel).is_ok());
    }

    #[test]
    fn ocf_flag_can_leave_no_data_field() {
        // req: TM-006, CMN-041
        let bytes = [0x00, 0x01, 0, 0, 0x18, 0x00, 1, 2, 3, 4];
        assert_eq!(
            err(TmFrame::decode(&bytes, &pc(10, false))),
            (ErrorKind::Malformed, Some(field::FRAME_DATA_FIELD))
        );
    }

    #[test]
    fn pointer_values() {
        // req: TM-008, API-032
        let channel = pc(16, false);
        let with_fhp = |fhp: u16| {
            let mut b = plain();
            b[4] = 0x18 | u8::try_from(fhp >> 8).unwrap();
            b[5] = u8::try_from(fhp & 0xFF).unwrap();
            b
        };
        for (fhp, expected) in [
            (9, starts_at(9)),
            (0x7FE, PacketPointer::OnlyIdleData),
            (0x7FF, PacketPointer::NoPacketStart),
        ] {
            let b = with_fhp(fhp);
            let f = TmFrame::decode(&b, &channel).unwrap();
            assert_eq!(f.data_field_status(), DataFieldStatus::Packets(expected));
        }
        for e in [
            TmFrame::decode(&with_fhp(10), &channel).unwrap_err(),
            TmFrame::decode_lenient(&with_fhp(10), &channel).unwrap_err(),
        ] {
            assert_eq!(
                (e.kind(), e.field(), e.detail()),
                (
                    ErrorKind::Malformed,
                    Some(field::FIRST_HEADER_POINTER),
                    ErrorDetail::Expected {
                        expected: 9,
                        found: 10
                    }
                )
            );
        }
    }

    #[test]
    fn sync_flag_0_fixed_fields() {
        // req: TM-007, API-031, API-032, GEN-043
        let channel = pc(16, false);
        let mut pof = plain();
        pof[4] |= 0x20;
        let e = TmFrame::decode(&pof, &channel).unwrap_err();
        assert_eq!(
            (e.kind(), e.field(), e.detail()),
            (
                ErrorKind::ReservedValue,
                Some(field::PACKET_ORDER_FLAG),
                ErrorDetail::Expected {
                    expected: 0,
                    found: 1
                }
            )
        );
        let f = TmFrame::decode_lenient(&pof, &channel).unwrap();
        assert_eq!(
            f.anomalies(),
            AnomalySet::from_flags(&[TmAnomaly::PacketOrderFlagSet])
        );
        let mut slid = plain();
        slid[4] = 0x08; // Segment Length Identifier 01
        let e = TmFrame::decode(&slid, &channel).unwrap_err();
        assert_eq!(
            (e.kind(), e.field(), e.detail()),
            (
                ErrorKind::InvalidField,
                Some(field::SEGMENT_LENGTH_ID),
                ErrorDetail::Expected {
                    expected: 3,
                    found: 1
                }
            )
        );
        slid[4] |= 0x20;
        let f = TmFrame::decode_lenient(&slid, &channel).unwrap();
        assert_eq!(
            f.anomalies(),
            AnomalySet::from_flags(&[
                TmAnomaly::PacketOrderFlagSet,
                TmAnomaly::SegmentLengthIdNot11
            ])
        );
        assert_eq!(f.header().segment_length_id.get(), 1);
        assert_eq!(
            f.data_field_status(),
            DataFieldStatus::Packets(starts_at(0))
        );
    }

    #[test]
    fn sync_flag_1_fields_are_raw() {
        // req: TM-007, TM-008
        let mut b = plain();
        b[4] = 0x67; // sync 1, POF 1, SLID 00, FHP 0x7xx
        b[5] = 0xFD;
        let strict = TmFrame::decode(&b, &pc(16, false)).unwrap();
        let lenient = TmFrame::decode_lenient(&b, &pc(16, false)).unwrap();
        for f in [&strict, &*lenient] {
            assert_eq!(
                f.data_field_status(),
                DataFieldStatus::VcaSdu {
                    packet_order_flag: true,
                    segment_length_id: SegmentLengthId::new(0).unwrap(),
                    first_header_pointer: FirstHeaderPointer::new(0x7FD).unwrap(),
                }
            );
        }
        assert!(lenient.anomalies().is_empty());
    }

    #[test]
    fn security_layout_places_the_data_field() {
        // req: TM-006, API-070, API-072
        let channel = pc(20, true);
        let vc = TmVirtualChannelConfig::new(VcId::new(5).unwrap(), DataFieldContent::Packets)
            .with_security(SecurityLayout::new(3, 2).unwrap());
        let mc =
            TmMasterChannelConfig::new(channel, SpacecraftId::new(0x2A5).unwrap(), None, false)
                .with_vc(vc)
                .unwrap();
        let mut bytes = plain()[..6].to_vec();
        bytes.extend([0xA5; 3]);
        bytes.extend([0xD0, 0xD1, 0xD2, 0xD3, 0xD4, 0xD5, 0xD6]);
        bytes.extend([0x5A; 2]);
        bytes.extend([0, 0]);
        write_fecf(&mut bytes).unwrap();
        let f = TmFrame::decode_with(&bytes, &channel, &mc).unwrap();
        assert_eq!(f.security_header(), &[0xA5; 3]);
        assert_eq!(f.data_field(), &bytes[9..16]);
        assert_eq!(f.security_trailer(), &[0x5A; 2]);
        // An FHP of 7 points past a 7-octet data field.
        let mut other = bytes.clone();
        other[5] = 7;
        write_fecf(&mut other).unwrap();
        assert_eq!(
            err(TmFrame::decode_with(&other, &channel, &mc)),
            (ErrorKind::Malformed, Some(field::FIRST_HEADER_POINTER))
        );
        // Without SDLS the same octets decode with a 10-octet data field.
        assert_eq!(
            TmFrame::decode(&other, &channel)
                .unwrap()
                .data_field()
                .len(),
            12
        );
        let mut unknown = bytes.clone();
        unknown[1] = 0x58; // VC 4
        write_fecf(&mut unknown).unwrap();
        assert_eq!(
            err(TmFrame::decode_with(&unknown, &channel, &mc)),
            (ErrorKind::UnknownChannel, Some(field::VC_ID))
        );
        assert_eq!(
            err(TmFrame::decode_unchecked(&unknown, &channel, &mc)),
            (ErrorKind::UnknownChannel, Some(field::VC_ID))
        );
        assert_eq!(
            err(TmFrame::decode_with(&bytes[..19], &channel, &mc)),
            (ErrorKind::LengthMismatch, Some(field::FRAME_LEN))
        );
        assert_eq!(
            err(TmFrame::decode_unchecked(&bytes[..19], &channel, &mc)),
            (ErrorKind::LengthMismatch, Some(field::FRAME_LEN))
        );
        assert!(TmFrame::decode_unchecked(&bytes, &channel, &mc).is_ok());
    }

    #[test]
    fn only_idle_data_on_an_sdls_vc_is_refused_or_flagged() {
        // req: TM-010, TM-011, API-031, API-032, GEN-043 (OI-54)
        let channel = pc(20, true);
        let sdls = |n| SecurityLayout::new(n, 0).unwrap();
        let mc = |layout| {
            TmMasterChannelConfig::new(channel, SpacecraftId::new(0x2A5).unwrap(), None, false)
                .with_vc(
                    TmVirtualChannelConfig::new(VcId::new(5).unwrap(), DataFieldContent::Packets)
                        .with_security(layout),
                )
                .unwrap()
        };
        // VC 5, SDLS Header of 3 octets, FHP 0x7FE (Only Idle Data), Segment Length ID 11.
        let mut oid = plain()[..4].to_vec();
        oid.extend([0x1F, 0xFE]);
        oid.extend([0xA5; 3]);
        oid.extend([0x55; 9]);
        oid.extend([0, 0]);
        write_fecf(&mut oid).unwrap();
        let with = mc(sdls(3));
        let oid_flag = AnomalySet::from_flags(&[TmAnomaly::OnlyIdleDataOnSdlsVc]);
        let strict = [
            err(TmFrame::decode_with(&oid, &channel, &with)),
            err(TmFrame::decode_unchecked(&oid, &channel, &with)),
        ];
        assert_eq!(strict, [(ErrorKind::InvalidField, Some(field::VC_ID)); 2]);
        let f = TmFrame::decode_with_lenient(&oid, &channel, &with).unwrap();
        assert_eq!(f.anomalies(), oid_flag);
        assert_eq!(
            f.data_field_status(),
            DataFieldStatus::Packets(PacketPointer::OnlyIdleData)
        );
        assert_eq!(f.security_header(), &[0xA5; 3]);
        let u = TmFrame::decode_unchecked_lenient(&oid, &channel, &with).unwrap();
        assert_eq!(u.anomalies(), oid_flag);
        assert_eq!(u.header(), f.header());
        // A Security Trailer alone also makes the VC use SDLS.
        let trailer = SecurityLayout::new(0, 3).unwrap();
        let e = TmFrame::decode_with(&oid, &channel, &mc(trailer)).unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::InvalidField, Some(field::VC_ID))
        );
        // `decode` has no table, so it reads a VC without SDLS and flags nothing.
        let f = TmFrame::decode_lenient(&oid, &channel).unwrap();
        assert_eq!(f.anomalies(), AnomalySet::empty());
        // The pointer decides: no packet start (0x7FF) and a start at 0 are no OID frame, and a
        // Synchronization Flag of 1 makes the pointer undefined.
        for (octet4, octet5) in [(0x1F, 0xFF), (0x18, 0x00), (0x60 | 0x07, 0xFE)] {
            let mut other = oid.clone();
            other[4..6].copy_from_slice(&[octet4, octet5]);
            write_fecf(&mut other).unwrap();
            let f = TmFrame::decode_with_lenient(&other, &channel, &with);
            assert_eq!(f.unwrap().anomalies(), AnomalySet::empty());
        }
        // Without SDLS on the VC, the frame is an ordinary OID frame, as in strict mode.
        let none = mc(SecurityLayout::NONE);
        let mut plain_oid = oid.clone();
        plain_oid[6..9].fill(0x55);
        write_fecf(&mut plain_oid).unwrap();
        let f = TmFrame::decode_with(&plain_oid, &channel, &none).unwrap();
        assert_eq!(
            f.data_field_status(),
            DataFieldStatus::Packets(PacketPointer::OnlyIdleData)
        );
        // The Packet Order Flag and Segment Length ID errors come first.
        let mut order = oid.clone();
        order[4] |= 0x20;
        write_fecf(&mut order).unwrap();
        assert_eq!(
            err(TmFrame::decode_with(&order, &channel, &with)),
            (ErrorKind::ReservedValue, Some(field::PACKET_ORDER_FLAG))
        );
    }

    #[test]
    fn peek_length_returns_the_configured_length() {
        // req: API-013, TM-002
        let channel = pc(1115, true);
        assert_eq!(TmFrame::peek_length(&[], &channel), Ok(1115));
        assert_eq!(TmFrame::peek_length(&[0x3F], &channel), Ok(1115));
        let e = TmFrame::peek_length(&[0xC0], &channel).unwrap_err();
        assert_eq!(
            (e.kind(), e.field(), e.detail()),
            (
                ErrorKind::UnsupportedVersion,
                Some(field::FRAME_VERSION),
                ErrorDetail::Expected {
                    expected: 0,
                    found: 3
                }
            )
        );
    }
}

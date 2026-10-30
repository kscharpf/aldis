//! Encoders: one-shot [`encode`] and the two-phase [`AosFrameWriter`].
//!
//! The writer has a finishing typestate and writes OID frames.
//!
//! 732.0-B-5 §4.1, §4.1.4.1.5, §4.2.7, §6.3; AOS-002, AOS-003, AOS-015, AOS-016, AOS-019,
//! API-020…API-023, API-072, API-074, RS-023, GEN-014, GEN-042, GEN-052.

use core::marker::PhantomData;
use core::ops::Range;

use aldis_core::crc::crc16;
use aldis_core::finishing::{
    Finalized, FrameState, Protected, Unprotected, require_layout, require_unsecured,
};
use aldis_core::ids::{SpacecraftId, VcId};
use aldis_core::layout::FrameLayout;
use aldis_core::ocf::Ocf;
use aldis_core::oid::{OidGenerator, OidGeneratorGalois};
use aldis_core::security::{FrameCtx, SecuredFields, SecurityHook, SecurityLayout};
use aldis_core::{Error, ErrorKind, Result};

use crate::bpdu::{BitstreamPointer, ValidBitCount};
use crate::config::{
    AosPhysicalChannelConfig, DataFieldContent, VcLookup, frame_layout, pdu_header_len,
    unknown_channel,
};
use crate::fhec;
use crate::field;
use crate::header::{
    AosPrimaryHeader, FrameCountCycle, OID_VCID, PRIMARY_HEADER_LEN, VcFrameCount, len_u64,
    mismatch,
};
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
        // The physical channel always leaves an OID frame a data octet (its constructor), so
        // the layout is never an error; `and_then` keeps that branch out of this crate's code.
        frame_layout(pc, SecurityLayout::NONE, false)
            .and_then(|layout| Plan::finish(pc, fields, layout, None))
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
    let last = zone.saturating_sub(1);
    let limit = last.min(usize::from(PacketPointer::max_offset(edition)));
    // An offset above the edition's largest has no header octets; one past the zone, no place.
    let (in_zone, offset) = match pointer {
        PacketPointer::StartsAt(n) => (usize::from(n.get()) <= last, n.get()),
        PacketPointer::NoPacketStart | PacketPointer::OnlyIdleData => (true, 0),
    };
    match pointer.header_octets(edition) {
        Some(octets) if in_zone => Ok(octets),
        _ => Err(Error::out_of_range(
            field::FIRST_HEADER_POINTER,
            len_u64(limit),
            u64::from(offset),
        )),
    }
}

/// The `B_PDU` Header for `pointer` in a Bitstream Data Zone of `zone` octets (AOS-019).
///
/// # Errors
/// `OutOfRange` (`bitstream_data_pointer`) for `ValidBits(n)` with `n` not below the zone's
/// length in bits (a zone of valid data only is `AllValid`, §4.1.4.3.3.3), with the largest
/// valid count. The count is at least 1 and has a pointer value by its type (RS-020).
fn bpdu_header(pointer: BitstreamPointer, zone: usize) -> Result<[u8; 2]> {
    if let BitstreamPointer::ValidBits(n) = pointer {
        let limit = zone
            .saturating_mul(8)
            .saturating_sub(1)
            .min(usize::from(ValidBitCount::MAX));
        if usize::from(n.get()) > limit {
            return Err(Error::out_of_range(
                field::BITSTREAM_DATA_POINTER,
                len_u64(limit),
                u64::from(n.get()),
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

/// Two-phase frame writer (732.0 §4.1; AOS-019, API-020…API-023, API-072, API-074, RS-023).
///
/// [`begin`](Self::begin) writes the Primary Header, without its FHEC, and the `M_PDU` or
/// `B_PDU` Header and returns the writer [`Unprotected`]; the caller fills the other fields in
/// place and applies SDLS with [`protect`](AosFrameWriter::protect), or
/// [`unsecured`](AosFrameWriter::unsecured) without it, which gives the writer [`Protected`];
/// [`finalize`](AosFrameWriter::finalize) computes the FHEC and then the FECF, last, and gives
/// it [`Finalized`], the only state that exposes the frame's octets. Each step consumes the
/// writer, so a step out of order does not compile (RS-023, RS-021).
///
/// `begin` zero-fills every octet after the headers, so until `finalize` the frame is defined:
/// its headers final but the FHEC, the fields the caller has not filled zero, and the FECF, if
/// configured, zero (API-023).
#[derive(Debug)]
pub struct AosFrameWriter<'b, S: FrameState = Unprotected> {
    frame: &'b mut [u8],
    layout: FrameLayout,
    zone_start: usize,
    fhec: bool,
    state: PhantomData<S>,
}

impl<'b, S: FrameState> AosFrameWriter<'b, S> {
    /// The same frame in state `T`: the one place a state changes.
    const fn into_state<T: FrameState>(self) -> AosFrameWriter<'b, T> {
        AosFrameWriter {
            frame: self.frame,
            layout: self.layout,
            zone_start: self.zone_start,
            fhec: self.fhec,
            state: PhantomData,
        }
    }
}

impl<'b> AosFrameWriter<'b, Unprotected> {
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
    ///   with `n` not below the zone's bits, with the largest valid value (AOS-017);
    /// - `OutOfRange` (`spacecraft_id`) for an SCID above 255 in `Issue4Compatible` (AOS-002);
    /// - `BufferTooSmall` (`needed` = frame length) if `out` is shorter than a frame.
    pub fn begin<L: VcLookup + ?Sized>(
        out: &'b mut [u8],
        pc: &AosPhysicalChannelConfig,
        vcs: &L,
        fields: &AosFrameFields,
    ) -> Result<AosFrameWriter<'b, Unprotected>> {
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
    ) -> Result<AosFrameWriter<'b, Unprotected>> {
        let plan = Plan::oid(pc, vcs, fields)?;
        let mut writer = AosFrameWriter::start(out, plan)?;
        oid.fill(writer.zone_mut());
        Ok(writer)
    }

    /// Takes up the frame of a checked plan in `out`: zero-fills it and writes the Primary
    /// Header and the `M_PDU` or `B_PDU` Header.
    fn start(out: &'b mut [u8], plan: Plan) -> Result<AosFrameWriter<'b, Unprotected>> {
        let frame_len = plan.layout.frame_len();
        let frame = out
            .get_mut(..frame_len)
            .ok_or(Error::buffer_too_small(frame_len))?;
        let header_octets = plan.header.to_octets();
        let header = header_octets.get(..plan.header.len()).unwrap_or_default();
        let data = plan.layout.data();
        let pdu: &[u8] = plan.pdu.as_ref().map_or(&[], |p| p.as_slice());
        let zone_start = data.start.saturating_add(pdu.len());
        // The FHEC, if any, is computed last, by `finalize` (RS-023).
        let fhec = header.len() > PRIMARY_HEADER_LEN;
        frame.fill(0);
        write_at(
            frame,
            0,
            header.get(..PRIMARY_HEADER_LEN).unwrap_or_default(),
        );
        write_at(frame, data.start, pdu);
        Ok(AosFrameWriter {
            frame,
            layout: plan.layout,
            zone_start,
            fhec,
            state: PhantomData,
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

    /// Applies `hook` to the Security Header, the data field, and the Security Trailer, after
    /// every other field except the FHEC and the FECF is final (355.0 §2.3; API-074, RS-023).
    /// `ctx` identifies the channel for the hook's Security Association.
    ///
    /// # Errors
    /// `InvalidField` (`security_header_len` or `security_trailer_len`) if the hook is for
    /// another layout than the VC's ([`SecurityHook::layout`]), such as `NoSecurity` on a VC with
    /// SDLS; the hook's error, `Security`; the frame is then dropped and must not be
    /// transmitted.
    pub fn protect<H: SecurityHook + ?Sized>(
        self,
        hook: &mut H,
        ctx: &FrameCtx,
    ) -> Result<AosFrameWriter<'b, Protected>> {
        let fields = SecuredFields::from_layout(&self.layout);
        let result = require_layout(
            hook.layout(),
            self.layout.security_header().len(),
            self.layout.security_trailer().len(),
        )
        .and_then(|()| hook.protect(ctx, &mut *self.frame, &fields));
        self.protected(result)
    }

    /// The writer [`Protected`] if the hook succeeded; the part of `protect` that does not
    /// depend on the hook type.
    fn protected(self, result: Result<()>) -> Result<AosFrameWriter<'b, Protected>> {
        result.map(|()| self.into_state())
    }

    /// Moves to [`Protected`] on a channel without SDLS, the `NoSecurity` path (API-073,
    /// RS-023).
    ///
    /// # Errors
    /// `InvalidField` (`security_header_len` or `security_trailer_len`) if the VC reserves a
    /// Security Header or Trailer, which need [`protect`](Self::protect).
    pub fn unsecured(self) -> Result<AosFrameWriter<'b, Protected>> {
        require_unsecured(
            self.layout.security_header().len(),
            self.layout.security_trailer().len(),
        )?;
        Ok(self.into_state())
    }
}

impl<'b> AosFrameWriter<'b, Protected> {
    /// Completes the frame: computes the FHEC, if configured, then the FECF, if configured,
    /// over every other octet, last (732.0 §4.1.2.6, §4.1.6.2, §4.2.7.4; AOS-013, AOS-019,
    /// API-021, API-074, RS-023).
    #[must_use = "the finalized frame"]
    pub fn finalize(self) -> AosFrameWriter<'b, Finalized> {
        if self.fhec {
            // The frame holds the eight header octets, so this is the one chunk.
            self.frame
                .first_chunk_mut::<8>()
                .into_iter()
                .for_each(fhec::write);
        }
        let fecf = self.layout.fecf();
        if !fecf.is_empty() {
            let crc = crc16(self.frame.get(..fecf.start).unwrap_or_default());
            write_at(self.frame, fecf.start, &crc.to_be_bytes());
        }
        self.into_state()
    }
}

impl<'b> AosFrameWriter<'b, Finalized> {
    /// The frame length (732.0 §4.1; AOS-019).
    #[must_use]
    pub const fn len(&self) -> usize {
        self.layout.frame_len()
    }

    /// Always `false`: a frame holds at least its Primary Header.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        false
    }

    /// The octets of the finalized frame, for transmission (RS-023).
    #[must_use]
    pub fn octets(&self) -> &[u8] {
        self.frame
            .get(..self.layout.frame_len())
            .unwrap_or_default()
    }

    /// The octets of the finalized frame, borrowed from the buffer `begin` took, for
    /// transmission (RS-023).
    #[must_use]
    pub fn into_octets(self) -> &'b [u8] {
        let len = self.layout.frame_len();
        self.frame.get(..len).unwrap_or_default()
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
    // The one-shot encoders take the Security Header and Trailer as given (API-072).
    Ok(writer.into_state::<Protected>().finalize().len())
}

/// Encodes an OID Transfer Frame of `fields.scid` into the start of `out`; returns its length
/// (732.0 §4.1.4.1.5; AOS-016, AOS-021, CMN-020, CMN-021, API-020).
///
/// The data field is filled with the next octets of `oid`.
///
/// The frame has no OCF or SDLS fields; `insert_zone` fills the Insert Zone, which is empty
/// when none is configured (AOS-015). Build `fields` with [`AosFrameFields::oid`].
///
/// # Errors
/// The errors of [`AosFrameWriter::begin_oid`], checked first, and then `LengthMismatch`
/// (`insert_zone_len`) for an `insert_zone` of another length than the configured one. On error
/// neither `out` nor `oid` is changed (GEN-042).
pub fn encode_oid<L: VcLookup + ?Sized>(
    pc: &AosPhysicalChannelConfig,
    vcs: &L,
    fields: &AosFrameFields,
    insert_zone: &[u8],
    oid: &mut OidGenerator,
    out: &mut [u8],
) -> Result<usize> {
    oid_frame(pc, vcs, fields, insert_zone, out, |zone| {
        oid.fill(zone);
    })
}

/// Encodes an OID frame as [`encode_oid`] does, with the OID generator in its Galois form
/// (732.0 §4.1.4.1.5; AOS-016, CMN-022).
///
/// The two forms give the same sequence, so the frames are the same. For a caller that keeps
/// the Galois form, such as the C ABI's `aldis_aos_encode_oid` with a generator initialized as
/// `ALDIS_OID_GALOIS`.
///
/// # Errors
/// As for [`encode_oid`]. On error neither `out` nor `oid` is changed (GEN-042).
pub fn encode_oid_galois<L: VcLookup + ?Sized>(
    pc: &AosPhysicalChannelConfig,
    vcs: &L,
    fields: &AosFrameFields,
    insert_zone: &[u8],
    oid: &mut OidGeneratorGalois,
    out: &mut [u8],
) -> Result<usize> {
    oid_frame(pc, vcs, fields, insert_zone, out, |zone| {
        oid.fill(zone);
    })
}

/// The OID frame of [`encode_oid`], its data field filled by `fill` once every check has
/// passed, so that a rejected frame leaves the generator unchanged (GEN-042).
fn oid_frame<L: VcLookup + ?Sized>(
    pc: &AosPhysicalChannelConfig,
    vcs: &L,
    fields: &AosFrameFields,
    insert_zone: &[u8],
    out: &mut [u8],
    fill: impl FnOnce(&mut [u8]),
) -> Result<usize> {
    let plan = Plan::oid(pc, vcs, fields)?;
    let expected = plan.layout.secondary().len();
    if insert_zone.len() != expected {
        return Err(mismatch(
            ErrorKind::LengthMismatch,
            field::INSERT_ZONE_LEN,
            len_u64(expected),
            len_u64(insert_zone.len()),
        ));
    }
    let mut writer = AosFrameWriter::start(out, plan)?;
    write_at(writer.frame, writer.layout.secondary().start, insert_zone);
    fill(writer.zone_mut());
    // The one-shot encoders take the Security Header and Trailer as given (API-072).
    Ok(writer.into_state::<Protected>().finalize().len())
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    extern crate std;
    use std::vec;

    use super::{
        AosFrameContent, AosFrameFields, AosFrameWriter, PduHeader, encode, encode_oid,
        encode_oid_galois,
    };
    use crate::bpdu::{BitstreamPointer, ValidBitCount};
    use crate::config::tests::{pc, scid, starts_at, vc, vcid};
    use crate::config::{AosEdition, AosVirtualChannelConfig, DataFieldContent, VcLookup, VcTable};
    use crate::field;
    use crate::header::{FrameCountCycle, OID_VCID, VcFrameCount};
    use crate::mpdu::PacketPointer;
    use crate::view::{AosFrame, DataUnit};
    use aldis_core::crc::syndrome;
    use aldis_core::ids::{SpacecraftId, VcId};
    use aldis_core::ocf::Ocf;
    use aldis_core::oid::{OidGenerator, OidGeneratorGalois};
    use aldis_core::security::{
        FrameCtx, HookLayout, NoSecurity, PreFilledSecurity, SecuredFields, SecurityHook,
        SecurityLayout,
    };
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

    /// A hook that checks the frame state it is called in, and fills the Security fields.
    struct OrderHook;

    impl SecurityHook for OrderHook {
        fn layout(&self) -> HookLayout {
            HookLayout::Any
        }

        fn protect(
            &mut self,
            _: &FrameCtx,
            frame: &mut [u8],
            fields: &SecuredFields,
        ) -> aldis_core::Result<()> {
            // Neither the FHEC nor the FECF is computed yet.
            assert_eq!(frame[6..8], [0, 0]);
            assert_eq!(frame.last_chunk::<2>(), Some(&[0, 0]));
            frame[fields.header()].fill(0xA5);
            frame[fields.trailer()].fill(0x5A);
            Ok(())
        }

        fn verify(
            &mut self,
            _: &FrameCtx,
            _: &mut [u8],
            fields: &SecuredFields,
        ) -> aldis_core::Result<core::ops::Range<usize>> {
            Ok(fields.data())
        }
    }

    /// A hook whose protection fails.
    struct Refuse;

    impl SecurityHook for Refuse {
        fn layout(&self) -> HookLayout {
            HookLayout::Any
        }

        fn protect(
            &mut self,
            _: &FrameCtx,
            _: &mut [u8],
            _: &SecuredFields,
        ) -> aldis_core::Result<()> {
            Err(aldis_core::Error::new(ErrorKind::Security))
        }

        fn verify(
            &mut self,
            _: &FrameCtx,
            _: &mut [u8],
            fields: &SecuredFields,
        ) -> aldis_core::Result<core::ops::Range<usize>> {
            Ok(fields.data())
        }
    }

    #[test]
    fn no_security_is_rejected_on_a_channel_with_sdls() {
        // req: RS-023, API-070, API-073
        let t = table();
        let phy = t.physical_channel();
        let ctx = FrameCtx::Aos {
            scid: scid(0xAB),
            vcid: vcid(0),
        };
        let mut out = [0xEE_u8; 40];
        let w = AosFrameWriter::begin(&mut out, &phy, &t, &mpdu(starts_at(3))).unwrap();
        let e = w.protect(&mut NoSecurity, &ctx).unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::InvalidField, Some(field::SECURITY_HEADER_LEN))
        );
    }

    #[test]
    fn a_failed_protect_gives_no_frame() {
        // req: RS-023, API-073
        let t = table();
        let phy = t.physical_channel();
        let ctx = FrameCtx::Aos {
            scid: scid(0xAB),
            vcid: vcid(0),
        };
        let mut out = [0xEE_u8; 40];
        let w = AosFrameWriter::begin(&mut out, &phy, &t, &mpdu(starts_at(3))).unwrap();
        let e = w.protect(&mut Refuse, &ctx).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::Security);
    }

    #[test]
    fn protect_runs_before_the_fhec_and_the_fecf() {
        // req: RS-023, API-074, API-073, AOS-013
        let t = table();
        let phy = t.physical_channel();
        let ctx = FrameCtx::Aos {
            scid: scid(0xAB),
            vcid: vcid(0),
        };
        let mut out = [0xEE_u8; 40];
        let w = AosFrameWriter::begin(&mut out, &phy, &t, &mpdu(starts_at(3))).unwrap();
        let octets = w
            .protect(&mut OrderHook, &ctx)
            .unwrap()
            .finalize()
            .into_octets();
        assert_eq!(syndrome(octets), 0);
        let f = AosFrame::decode(octets, &phy, &t, STRICT).unwrap();
        assert_eq!(
            f.header().fhec().map(u16::to_be_bytes),
            Some([octets[6], octets[7]])
        );
        assert_ne!(f.header().fhec(), Some(0));
        assert_eq!(
            (f.security_header(), f.security_trailer()),
            (&[0xA5; 2][..], &[0x5A][..])
        );
    }

    #[test]
    fn unsecured_needs_a_channel_without_sdls() {
        // req: RS-023, API-070
        let t = table();
        let phy = t.physical_channel();
        let mut out = [0xEE_u8; 40];
        let w = AosFrameWriter::begin(&mut out, &phy, &t, &mpdu(starts_at(3))).unwrap();
        let e = w.unsecured().unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::InvalidField, Some(field::SECURITY_HEADER_LEN))
        );
    }

    #[test]
    fn two_phase_writer_fills_every_field() {
        // req: AOS-019, AOS-013, AOS-015, API-020, API-021, API-072, GEN-014
        let t = table();
        let phy = t.physical_channel();
        let mut out = [0xEE_u8; 48];
        let mut w = AosFrameWriter::begin(&mut out, &phy, &t, &mpdu(starts_at(3))).unwrap();
        assert_eq!(w.layout().frame_len(), 40);
        w.insert_zone_mut().copy_from_slice(&[1, 2, 3, 4]);
        w.security_header_mut().copy_from_slice(&[0xA5, 0xA5]);
        // 40 − 8 − 4 − 2 − 1 − 4 − 2 = 19 data octets: the M_PDU Header and 17.
        assert_eq!(w.zone_mut().len(), 17);
        w.zone_mut().fill(0xD0);
        w.security_trailer_mut().copy_from_slice(&[0x5A]);
        w.set_ocf(Ocf::decode([0x01, 0, 0, 0])).unwrap();
        let ctx = FrameCtx::Aos {
            scid: scid(0xAB),
            vcid: vcid(0),
        };
        let done = w.protect(&mut PreFilledSecurity, &ctx).unwrap().finalize();
        assert_eq!((done.len(), done.octets().len()), (40, 40));
        assert!(!done.is_empty());
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
            (starts_at(3), &[0xD0; 17][..])
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
        assert_eq!(
            out[6..8],
            [0, 0],
            "the FHEC is computed by `finalize` (RS-023)"
        );
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
            rejected(&t, &mpdu(starts_at(17))),
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
        for n in [192, ValidBitCount::MAX] {
            let f = AosFrameFields::new(
                scid(0x0AB),
                vcid(1),
                count(0),
                PduHeader::Bpdu(BitstreamPointer::ValidBits(ValidBitCount::new(n).unwrap())),
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
        let e = AosFrameWriter::begin(&mut short, &t.physical_channel(), &t, &mpdu(starts_at(16)))
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
            PduHeader::Bpdu(BitstreamPointer::ValidBits(
                ValidBitCount::new(191).unwrap(),
            )),
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
        let w = AosFrameWriter::begin(&mut out, &p4, &t, &fields(starts_at(0x7FD))).unwrap();
        assert_eq!(w.unsecured().unwrap().finalize().len(), 2100);
        // SCID 0xFF in bits 2–9, bits 42–43 00; spare 00000 and FHP 0x7FD.
        assert_eq!(out[..8], [0x7F, 0xC7, 0, 0, 1, 0x00, 0x07, 0xFD]);
        let f = AosFrame::decode(&out, &p4, &t, STRICT).unwrap();
        assert_eq!(
            (f.scid(), f.header().edition()),
            (scid(0xFF), AosEdition::Issue4Compatible)
        );
        let e = AosFrameWriter::begin(&mut out, &p4, &t, &fields(starts_at(0x7FE))).unwrap_err();
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
        let w = AosFrameWriter::begin(&mut out, &p5, &t5, &fields(starts_at(0x7FE))).unwrap();
        assert_eq!(w.unsecured().unwrap().finalize().len(), 2100);
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
        let fields = mpdu(starts_at(0));
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
        let e = encode(&phy, &t, &mpdu(starts_at(17)), &ok, &mut out).unwrap_err();
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
        assert_eq!(w.unsecured().unwrap().finalize().len(), 40);
        // Without FHEC and FECF: nothing computed after the header.
        let plain = pc(12, false, 0, false, AosEdition::Issue5);
        let t = VcTable::<1>::new(plain, &[vc(1, 1, DataFieldContent::IdleData)]).unwrap();
        let mut out = [0_u8; 12];
        let f = AosFrameFields::new(scid(1), vcid(1), count(0), PduHeader::None);
        let w = AosFrameWriter::begin(&mut out, &plain, &t, &f).unwrap();
        assert!(w.layout().fecf().is_empty());
        assert_eq!(w.unsecured().unwrap().finalize().len(), 12);
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
        assert_eq!(w.unsecured().unwrap().finalize().len(), 40);
        let w = AosFrameWriter::begin_oid(
            &mut second,
            &phy,
            &t,
            &AosFrameFields::oid(scid(0x0AB), count(2)),
            &mut oid,
        )
        .unwrap();
        assert_eq!(w.unsecured().unwrap().finalize().len(), 40);
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

    #[test]
    fn encode_oid_matches_the_two_phase_frame() {
        // req: AOS-021, AOS-016, AOS-015, CMN-020, CMN-021, CMN-022, API-020
        let table = table();
        let phy = table.physical_channel();
        let fields = AosFrameFields::oid(scid(0x0AB), count(1));
        let (mut fib, mut gal, mut two_phase) = (
            OidGenerator::new(),
            OidGeneratorGalois::new(),
            OidGenerator::new(),
        );
        let (mut fib_frame, mut gal_frame, mut two_frame) =
            ([0xEE_u8; 44], [0xEE_u8; 40], [0_u8; 40]);
        let iz = [7, 6, 5, 4];
        assert_eq!(
            encode_oid(&phy, &table, &fields, &iz, &mut fib, &mut fib_frame),
            Ok(40)
        );
        assert_eq!(
            encode_oid_galois(&phy, &table, &fields, &iz, &mut gal, &mut gal_frame),
            Ok(40)
        );
        let mut writer =
            AosFrameWriter::begin_oid(&mut two_frame, &phy, &table, &fields, &mut two_phase)
                .unwrap();
        writer.insert_zone_mut().copy_from_slice(&iz);
        assert_eq!(writer.unsecured().unwrap().finalize().len(), 40);
        assert_eq!(fib_frame[..40], two_frame);
        assert_eq!(gal_frame, two_frame);
        assert_eq!(fib_frame[40..], [0xEE; 4], "nothing written past the frame");
        assert_eq!(fib, two_phase);
        assert_eq!(gal.next_octet(), fib.next_octet());
        assert_eq!(syndrome(&two_frame), 0);
        let frame = AosFrame::decode(&two_frame, &phy, &table, STRICT).unwrap();
        assert_eq!(frame.insert_zone(), &iz);
    }

    #[test]
    fn encode_oid_rejects_without_writing() {
        // req: AOS-021, AOS-016, API-022, GEN-042
        let t = table();
        let phy = t.physical_channel();
        let mut oid = OidGenerator::new();
        let before = oid.clone();
        let mut out = [0xEE_u8; 40];
        let kf = |e: super::Error| (e.kind(), e.field());
        let ok = AosFrameFields::oid(scid(0x0AB), count(1));
        let e = encode_oid(&phy, &t, &ok, &[0; 3], &mut oid, &mut out).unwrap_err();
        assert_eq!(
            kf(e),
            (ErrorKind::LengthMismatch, Some(field::INSERT_ZONE_LEN))
        );
        let not_oid = AosFrameFields::new(scid(0x0AB), vcid(1), count(1), PduHeader::None);
        let e = encode_oid(&phy, &t, &not_oid, &[0; 4], &mut oid, &mut out).unwrap_err();
        assert_eq!(kf(e), (ErrorKind::InvalidField, Some(field::VC_ID)));
        let other = AosFrameFields::oid(scid(0x0AC), count(1));
        let e = encode_oid(&phy, &t, &other, &[0; 4], &mut oid, &mut out).unwrap_err();
        assert_eq!(
            kf(e),
            (ErrorKind::UnknownChannel, Some(field::SPACECRAFT_ID))
        );
        let e = encode_oid(&phy, &t, &ok, &[0; 4], &mut oid, &mut out[..39]).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::BufferTooSmall);
        assert_eq!(out, [0xEE; 40]);
        assert_eq!(oid, before);
    }
}

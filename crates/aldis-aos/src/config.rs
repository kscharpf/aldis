//! Managed parameters of the AOS Space Data Link Protocol that the codec reads, and the edition
//! (732.0-B-5 §5, §6.6; AOS-001, AOS-018, GEN-060, GEN-061, GEN-062, API-040, API-041, API-070,
//! RS-012).
//!
//! | Level | Type | Parameters kept |
//! |---|---|---|
//! | Physical Channel (Table 5-1) | [`AosPhysicalChannelConfig`] | Transfer Frame Length, presence of the FHEC, Insert Zone length, presence of the FECF, and the [`AosEdition`] |
//! | Master Channel (Table 5-2) | the [`AosVirtualChannelConfig`] entries of one SCID | Spacecraft ID, valid VCIDs 0–62; VCID 63 is always valid for OID frames |
//! | Virtual Channel (Tables 5-3, 6-1) | [`AosVirtualChannelConfig`] | SCID, VCID 0–62, Data Field Content, presence of the `VC_OCF`, Security Header and Trailer lengths |
//!
//! The decoder and the writers look the channels up through [`VcLookup`]; [`VcTable`] is its
//! fixed-capacity implementation (M2 plan D14). The Valid Spacecraft IDs of the physical channel
//! (Table 5-1) are the SCIDs of its virtual channels. The MC and VC Multiplexing Schemes and the
//! Packet Transfer parameters of Table 5-4 belong to the engines (M4).
//!
//! A frame on VCID 63, an OID Transfer Frame (§4.1.4.1.5), has no virtual channel parameters:
//! Table 5-3 configures VCIDs 0–62 only, so its frames carry no `VC_OCF` (§4.1.5.2) and no
//! Security Header or Trailer (Table 6-1); the Insert Zone and the FECF, physical channel
//! parameters, are present when configured (§4.1.3.3, §4.1.6.1.3; OI-43 item 1).

use aldis_core::ids::{SpacecraftId, VcId};
use aldis_core::layout::{FECF_LEN, FrameLayout, LayoutSpec};
use aldis_core::security::SecurityLayout;
use aldis_core::{Error, ErrorKind, Result};

use crate::bpdu::{BPDU_HEADER_LEN, MAX_BITSTREAM_FRAME_LEN, MAX_BITSTREAM_ZONE_LEN};
use crate::field;
use crate::header::{FHEC_LEN, OID_VCID, PRIMARY_HEADER_LEN, len_u64, mismatch};
use crate::mpdu::MPDU_HEADER_LEN;

/// The issue of 732.0-B whose wire format a physical channel follows (732.0-B-5 §4.1.2.2.3
/// notes; AOS-001, GEN-062).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AosEdition {
    /// 732.0-B-5: a 10-bit SCID with its two most significant bits in Primary Header bits
    /// 42–43, and a 16-bit First Header Pointer (§4.1.2.2.3, §4.1.4.2.1.4). The default
    /// (GEN-062).
    #[default]
    Issue5,
    /// 732.0-B-4 and earlier: an 8-bit SCID, bits 42–43 a reserved spare `00`, and an `M_PDU`
    /// Header of a 5-bit reserved spare and an 11-bit First Header Pointer (732.0-B-4
    /// §4.1.2.2.3, §4.1.2.5.4, §4.1.4.2.2–3; AOS-002, AOS-003). The FHEC is the same code
    /// (AOS-013, M2 plan D11).
    Issue4Compatible,
}

impl AosEdition {
    /// The largest Spacecraft ID of the edition: 1023 (10 bits) in Issue 5, 255 (8 bits) in
    /// Issue 4 (AOS-002).
    #[must_use]
    pub const fn max_scid(self) -> u16 {
        match self {
            AosEdition::Issue5 => 0x3FF,
            AosEdition::Issue4Compatible => 0xFF,
        }
    }
}

/// The managed parameters of a Physical Channel that the codec reads (732.0 §5.2 Table 5-1;
/// AOS-001, AOS-015, GEN-060, API-041, RS-012).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AosPhysicalChannelConfig {
    frame_len: usize,
    fhec: bool,
    insert_zone_len: usize,
    fecf: bool,
    edition: AosEdition,
}

impl AosPhysicalChannelConfig {
    /// A physical channel of `frame_len`-octet frames (732.0 §4.1.1.2), with the Frame Header
    /// Error Control if `fhec` (§4.1.2.6.2), an Insert Zone of `insert_zone_len` octets, none if
    /// 0 (§4.1.3), the Frame Error Control Field if `fecf` (§4.1.6.1.2), and the wire format of
    /// `edition` (AOS-001).
    ///
    /// 732.0 sets no upper bound on the frame length; the channel coding of the physical layer
    /// does (§4.1.1.2).
    ///
    /// # Errors
    /// `InvalidConfig` (`frame_len`) if a frame cannot hold the Primary Header (6 octets, 8 with
    /// the FHEC), the Insert Zone, one data octet, and the FECF (§4.1.1.1; CMN-041), with that
    /// minimum.
    pub const fn new(
        frame_len: usize,
        fhec: bool,
        insert_zone_len: usize,
        fecf: bool,
        edition: AosEdition,
    ) -> Result<AosPhysicalChannelConfig> {
        // The header, one data octet, and the FECF: at most 11 octets, so no overflow.
        let fixed = primary_header_len(fhec)
            .saturating_add(1)
            .saturating_add(if fecf { FECF_LEN } else { 0 });
        if frame_len < fixed || frame_len.saturating_sub(fixed) < insert_zone_len {
            return Err(mismatch(
                ErrorKind::InvalidConfig,
                field::FRAME_LEN,
                len_u64(fixed.saturating_add(insert_zone_len)),
                len_u64(frame_len),
            ));
        }
        Ok(AosPhysicalChannelConfig {
            frame_len,
            fhec,
            insert_zone_len,
            fecf,
            edition,
        })
    }

    /// Transfer Frame Length in octets (732.0 §4.1.1.2, Table 5-1; API-012).
    #[must_use]
    pub const fn frame_len(&self) -> usize {
        self.frame_len
    }

    /// Whether every Primary Header ends with the Frame Header Error Control (732.0 §4.1.2.6.3;
    /// AOS-013).
    #[must_use]
    pub const fn fhec(&self) -> bool {
        self.fhec
    }

    /// Insert Zone length in octets, 0 without an Insert Zone (732.0 §4.1.3.4.1, Table 5-1;
    /// AOS-015).
    #[must_use]
    pub const fn insert_zone_len(&self) -> usize {
        self.insert_zone_len
    }

    /// Whether every frame ends with a Frame Error Control Field (732.0 §4.1.6.1.3).
    #[must_use]
    pub const fn fecf(&self) -> bool {
        self.fecf
    }

    /// The wire format (AOS-001, GEN-062).
    #[must_use]
    pub const fn edition(&self) -> AosEdition {
        self.edition
    }

    /// Primary Header length: 6 octets, or 8 with the FHEC (732.0 §4.1.2.1).
    #[must_use]
    pub const fn primary_header_len(&self) -> usize {
        primary_header_len(self.fhec)
    }
}

/// 6 octets, or 8 with the FHEC (732.0 §4.1.1.1 a).
const fn primary_header_len(fhec: bool) -> usize {
    if fhec {
        PRIMARY_HEADER_LEN.saturating_add(FHEC_LEN)
    } else {
        PRIMARY_HEADER_LEN
    }
}

/// What a virtual channel's data fields carry, static per VC (732.0 §4.1.4.1.3–4, Table 5-3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DataFieldContent {
    /// One Multiplexing Protocol Data Unit per frame (§4.1.4.2).
    Mpdu,
    /// One Bitstream Protocol Data Unit per frame (§4.1.4.3; AOS-018).
    Bpdu,
    /// One `VCA_SDU` per frame, opaque to the codec (§4.1.4.1.3).
    VcaSdu,
    /// Idle Data (§4.1.4.1.3, Table 5-3).
    IdleData,
}

/// The managed parameters of one Virtual Channel that the codec reads (732.0 §5.4 Table 5-3,
/// §6.6.2 Table 6-1; GEN-060, API-040, API-070, RS-012).
///
/// The Spacecraft ID names the Master Channel the Virtual Channel belongs to; the valid VCIDs of
/// a Master Channel are those of its entries, and VCID 63 (Table 5-2 note 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AosVirtualChannelConfig {
    scid: SpacecraftId<10>,
    vcid: VcId<6>,
    content: DataFieldContent,
    ocf: bool,
    security: SecurityLayout,
}

impl AosVirtualChannelConfig {
    /// A virtual channel carrying `content`, without `VC_OCF` or SDLS fields (Table 5-3).
    ///
    /// # Errors
    /// `InvalidConfig` (`vc_id`) for VCID 63, which is reserved for OID frames (732.0
    /// §4.1.4.1.5.1, Table 5-3 note 2; API-040), with the largest data VCID, 62.
    pub const fn new(
        scid: SpacecraftId<10>,
        vcid: VcId<6>,
        content: DataFieldContent,
    ) -> Result<AosVirtualChannelConfig> {
        if vcid.get() == OID_VCID.get() {
            return Err(mismatch(
                ErrorKind::InvalidConfig,
                field::VC_ID,
                MAX_DATA_VCID,
                OID_VCID_U64,
            ));
        }
        Ok(AosVirtualChannelConfig {
            scid,
            vcid,
            content,
            ocf: false,
            security: SecurityLayout::NONE,
        })
    }

    /// The same channel with a `VC_OCF` (732.0 §4.1.5.2–3, Table 5-3).
    #[must_use]
    pub const fn with_ocf(self) -> AosVirtualChannelConfig {
        AosVirtualChannelConfig { ocf: true, ..self }
    }

    /// The same channel with a Security Header and Trailer (732.0 §6.3, Table 6-1; API-070).
    #[must_use]
    pub const fn with_security(self, security: SecurityLayout) -> AosVirtualChannelConfig {
        AosVirtualChannelConfig { security, ..self }
    }

    /// Spacecraft ID of the virtual channel's master channel (Table 5-3).
    #[must_use]
    pub const fn scid(&self) -> SpacecraftId<10> {
        self.scid
    }

    /// Virtual Channel Identifier, 0–62 (Table 5-3).
    #[must_use]
    pub const fn vcid(&self) -> VcId<6> {
        self.vcid
    }

    /// Data Field Content (732.0 §4.1.4.1.4, Table 5-3).
    #[must_use]
    pub const fn content(&self) -> DataFieldContent {
        self.content
    }

    /// Whether the frames of the VC carry an Operational Control Field (Table 5-3).
    #[must_use]
    pub const fn ocf(&self) -> bool {
        self.ocf
    }

    /// Security Header and Trailer lengths (Table 6-1; API-070).
    #[must_use]
    pub const fn security(&self) -> SecurityLayout {
        self.security
    }
}

/// VCID 62, the largest a data VC may have (Table 5-3), for an error's detail.
const MAX_DATA_VCID: u64 = 62;
/// VCID 63 (§4.1.4.1.5.1), for an error's detail.
const OID_VCID_U64: u64 = 63;

/// The layout of a frame of `pc` with the SDLS fields of `security` and an OCF if `ocf`: the
/// Insert Zone in the secondary slot (732.0 §4.1, §6.3; CMN-040).
///
/// # Errors
/// `InvalidConfig` (`frame_data_field`) if the other fields leave no data octet (CMN-041).
pub(crate) fn frame_layout(
    pc: &AosPhysicalChannelConfig,
    security: SecurityLayout,
    ocf: bool,
) -> Result<FrameLayout> {
    FrameLayout::compute(&LayoutSpec {
        frame_len: pc.frame_len,
        primary_header_len: pc.primary_header_len(),
        secondary_len: pc.insert_zone_len,
        security,
        ocf,
        fecf: pc.fecf,
    })
}

/// The length of the `M_PDU` or `B_PDU` Header of a VC's data fields, 0 for a `VCA_SDU` or Idle
/// Data (732.0 §4.1.4.2.1.3, §4.1.4.3.1.3).
pub(crate) const fn pdu_header_len(content: DataFieldContent) -> usize {
    match content {
        DataFieldContent::Mpdu => MPDU_HEADER_LEN,
        DataFieldContent::Bpdu => BPDU_HEADER_LEN,
        DataFieldContent::VcaSdu | DataFieldContent::IdleData => 0,
    }
}

/// `UnknownChannel` for a frame of (`scid`, a VCID not configured): naming the Spacecraft ID if
/// it is not valid on the physical channel, else the VCID (732.0 §4.3.5.4, §4.3.6.3).
pub(crate) fn unknown_channel<L: VcLookup + ?Sized>(vcs: &L, scid: SpacecraftId<10>) -> Error {
    let field = if vcs.has_spacecraft(scid) {
        field::VC_ID
    } else {
        field::SPACECRAFT_ID
    };
    Error::new(ErrorKind::UnknownChannel).with_field(field)
}

/// Looks up the managed parameters of the virtual channels a frame may belong to (M2 plan D14;
/// AOS-010, GEN-060).
///
/// [`VcTable`] implements it. Another implementation must answer consistently: a virtual
/// channel that [`vc`](Self::vc) finds belongs to a Spacecraft ID that
/// [`has_spacecraft`](Self::has_spacecraft) accepts, and has the VCID it was asked for.
pub trait VcLookup {
    /// Whether `scid` is a valid Spacecraft ID of the physical channel (732.0 §4.3.6.3,
    /// Table 5-1), so that an OID frame of that SCID is accepted.
    fn has_spacecraft(&self, scid: SpacecraftId<10>) -> bool;
    /// The virtual channel (`scid`, `vcid`), or `None` if it is not valid (732.0 §4.3.5.4,
    /// Table 5-2). Never a virtual channel for VCID 63.
    fn vc(&self, scid: SpacecraftId<10>, vcid: VcId<6>) -> Option<AosVirtualChannelConfig>;
}

/// The virtual channels of one physical channel, with room for `N` of them (732.0 §5; GEN-011,
/// GEN-060, M2 plan D14).
///
/// Built only by [`new`](Self::new), so the entries are unique and every one fits the physical
/// channel (RS-020).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VcTable<const N: usize> {
    pc: AosPhysicalChannelConfig,
    vcs: [Option<AosVirtualChannelConfig>; N],
}

impl<const N: usize> VcTable<N> {
    /// The virtual channels of `pc` (732.0 §5; GEN-060, API-040).
    ///
    /// # Errors
    /// - `InvalidConfig` (`vc_id`) if `vcs` is empty: a physical channel without a virtual
    ///   channel has no Spacecraft ID to accept an OID frame for (OI-55).
    ///
    /// Then, for the first virtual channel that fails, in this order:
    /// - `InvalidConfig` (`vc_id`) for a virtual channel given twice;
    /// - `InvalidConfig` (`spacecraft_id`) for an SCID above 255 in `Issue4Compatible`, which
    ///   has 8 bits for it (AOS-002), with that limit;
    /// - `InvalidConfig` (`frame_data_field`) if the Insert Zone, the SDLS fields, the OCF, and
    ///   the FECF leave no data octet, or an `M_PDU` or `B_PDU` no octet after its 2-octet header
    ///   (732.0 §4.1.4.2.1.3 b, §4.1.4.3.1.3 b; CMN-041);
    /// - for a VC that carries `B_PDUs` (AOS-018): `InvalidConfig` (`frame_len`) in `Issue5` for
    ///   frames longer than 2 048 octets (§4.1.4.3.3.4 note), and `InvalidConfig`
    ///   (`frame_data_field`) for a Bitstream Data Zone longer than 2 047 octets, each with its
    ///   limit;
    /// - `CapacityExceeded` (`vc_id`) for more than `N` virtual channels (GEN-011).
    pub fn new(
        pc: AosPhysicalChannelConfig,
        vcs: &[AosVirtualChannelConfig],
    ) -> Result<VcTable<N>> {
        if vcs.is_empty() {
            return Err(Error::new(ErrorKind::InvalidConfig).with_field(field::VC_ID));
        }
        let mut slots = [None; N];
        fill(&mut slots, &pc, vcs)?;
        Ok(VcTable { pc, vcs: slots })
    }

    /// The physical channel (Table 5-1).
    #[must_use]
    pub const fn physical_channel(&self) -> AosPhysicalChannelConfig {
        self.pc
    }

    /// The virtual channels, in the order given (Table 5-3).
    pub fn vcs(&self) -> impl Iterator<Item = &AosVirtualChannelConfig> {
        self.vcs.iter().flatten()
    }

    /// The layout of every frame of (`scid`, `vcid`) on the table's physical channel, if the
    /// channel is valid: for VCID 63, that of an OID frame (732.0 §4.1, §6.3; CMN-040).
    #[must_use]
    pub fn frame_layout(&self, scid: SpacecraftId<10>, vcid: VcId<6>) -> Option<FrameLayout> {
        slots_layout(&self.pc, &self.vcs, scid, vcid)
    }
}

// The work of `VcTable` is done over its slots as a slice, so that one copy of each function is
// compiled and measured whatever the capacity (VER-024).

/// Fills `slots` with `vcs` in order, checking each as [`VcTable::new`] documents.
fn fill(
    slots: &mut [Option<AosVirtualChannelConfig>],
    pc: &AosPhysicalChannelConfig,
    vcs: &[AosVirtualChannelConfig],
) -> Result<()> {
    for (i, vc) in vcs.iter().enumerate() {
        if find(slots, vc.scid, vc.vcid).is_some() {
            return Err(Error::new(ErrorKind::InvalidConfig).with_field(field::VC_ID));
        }
        check_vc(pc, vc)?;
        let slot = slots
            .get_mut(i)
            .ok_or(Error::new(ErrorKind::CapacityExceeded).with_field(field::VC_ID))?;
        *slot = Some(*vc);
    }
    Ok(())
}

/// The virtual channel (`scid`, `vcid`) of `slots`.
fn find(
    slots: &[Option<AosVirtualChannelConfig>],
    scid: SpacecraftId<10>,
    vcid: VcId<6>,
) -> Option<AosVirtualChannelConfig> {
    slots
        .iter()
        .flatten()
        .find(|vc| vc.scid == scid && vc.vcid == vcid)
        .copied()
}

/// Whether `slots` hold a virtual channel of `scid`.
fn has(slots: &[Option<AosVirtualChannelConfig>], scid: SpacecraftId<10>) -> bool {
    slots.iter().flatten().any(|vc| vc.scid == scid)
}

/// [`VcTable::frame_layout`] over the slots.
fn slots_layout(
    pc: &AosPhysicalChannelConfig,
    slots: &[Option<AosVirtualChannelConfig>],
    scid: SpacecraftId<10>,
    vcid: VcId<6>,
) -> Option<FrameLayout> {
    let (security, ocf) = if vcid == OID_VCID {
        if !has(slots, scid) {
            return None;
        }
        (SecurityLayout::NONE, false)
    } else {
        let vc = find(slots, scid, vcid)?;
        (vc.security, vc.ocf)
    };
    frame_layout(pc, security, ocf).ok()
}

/// The checks of [`VcTable::new`] for one virtual channel after the duplicate check.
fn check_vc(pc: &AosPhysicalChannelConfig, vc: &AosVirtualChannelConfig) -> Result<()> {
    let max_scid = pc.edition.max_scid();
    if vc.scid.get() > max_scid {
        return Err(mismatch(
            ErrorKind::InvalidConfig,
            field::SPACECRAFT_ID,
            u64::from(max_scid),
            u64::from(vc.scid.get()),
        ));
    }
    let layout = frame_layout(pc, vc.security, vc.ocf)?;
    let header = pdu_header_len(vc.content);
    let data = layout.data().len();
    if data <= header {
        return Err(mismatch(
            ErrorKind::InvalidConfig,
            field::FRAME_DATA_FIELD,
            len_u64(header.saturating_add(1)),
            len_u64(data),
        ));
    }
    if vc.content == DataFieldContent::Bpdu {
        if pc.edition == AosEdition::Issue5 && pc.frame_len > MAX_BITSTREAM_FRAME_LEN {
            return Err(mismatch(
                ErrorKind::InvalidConfig,
                field::FRAME_LEN,
                len_u64(MAX_BITSTREAM_FRAME_LEN),
                len_u64(pc.frame_len),
            ));
        }
        let zone = data.saturating_sub(header);
        if zone > MAX_BITSTREAM_ZONE_LEN {
            return Err(mismatch(
                ErrorKind::InvalidConfig,
                field::FRAME_DATA_FIELD,
                len_u64(MAX_BITSTREAM_ZONE_LEN),
                len_u64(zone),
            ));
        }
    }
    Ok(())
}

impl<const N: usize> VcLookup for VcTable<N> {
    fn has_spacecraft(&self, scid: SpacecraftId<10>) -> bool {
        has(&self.vcs, scid)
    }

    fn vc(&self, scid: SpacecraftId<10>, vcid: VcId<6>) -> Option<AosVirtualChannelConfig> {
        find(&self.vcs, scid, vcid)
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
pub(crate) mod tests {
    extern crate std;
    use std::vec::Vec;

    use super::{
        AosEdition, AosPhysicalChannelConfig, AosVirtualChannelConfig, DataFieldContent, VcLookup,
        VcTable,
    };
    use crate::field;
    use aldis_core::ids::{SpacecraftId, VcId};
    use aldis_core::security::SecurityLayout;
    use aldis_core::{Error, ErrorDetail, ErrorKind};

    pub(crate) fn scid(n: u16) -> SpacecraftId<10> {
        SpacecraftId::new(n).unwrap()
    }

    pub(crate) fn vcid(n: u8) -> VcId<6> {
        VcId::new(n).unwrap()
    }

    pub(crate) fn starts_at(n: u16) -> crate::mpdu::PacketPointer {
        crate::mpdu::PacketPointer::StartsAt(crate::mpdu::PacketOffset::new(n).unwrap())
    }

    pub(crate) fn vc(s: u16, v: u8, content: DataFieldContent) -> AosVirtualChannelConfig {
        AosVirtualChannelConfig::new(scid(s), vcid(v), content).unwrap()
    }

    pub(crate) fn pc(
        len: usize,
        fhec: bool,
        iz: usize,
        fecf: bool,
        edition: AosEdition,
    ) -> AosPhysicalChannelConfig {
        AosPhysicalChannelConfig::new(len, fhec, iz, fecf, edition).unwrap()
    }

    fn invalid(
        r: Result<impl core::fmt::Debug, Error>,
    ) -> (ErrorKind, Option<aldis_core::FieldId>, ErrorDetail) {
        let e = r.unwrap_err();
        (e.kind(), e.field(), e.detail())
    }

    #[test]
    fn edition_defaults_to_issue_5() {
        // req: AOS-001, GEN-062
        assert_eq!(AosEdition::default(), AosEdition::Issue5);
        assert_eq!(AosEdition::Issue5.max_scid(), 1023);
        assert_eq!(AosEdition::Issue4Compatible.max_scid(), 255);
    }

    #[test]
    fn physical_channel_minimum() {
        // req: AOS-001, API-040, CMN-041, RS-012
        const PC: AosPhysicalChannelConfig =
            match AosPhysicalChannelConfig::new(1115, true, 8, true, AosEdition::Issue5) {
                Ok(pc) => pc,
                Err(_) => panic!("valid"),
            };
        assert_eq!(
            (PC.frame_len(), PC.fhec(), PC.insert_zone_len(), PC.fecf()),
            (1115, true, 8, true)
        );
        assert_eq!(
            (PC.edition(), PC.primary_header_len()),
            (AosEdition::Issue5, 8)
        );
        assert_eq!(
            pc(7, false, 0, false, AosEdition::Issue5).primary_header_len(),
            6
        );
        // 6 (+2 FHEC) + Insert Zone + 1 data octet (+2 FECF).
        for (len, fhec, iz, fecf, min) in [
            (6, false, 0, false, 7),
            (8, true, 0, false, 9),
            (10, false, 4, false, 11),
            (14, true, 3, true, 14),
        ] {
            assert!(AosPhysicalChannelConfig::new(min, fhec, iz, fecf, AosEdition::Issue5).is_ok());
            if len < min {
                assert_eq!(
                    invalid(AosPhysicalChannelConfig::new(
                        len,
                        fhec,
                        iz,
                        fecf,
                        AosEdition::Issue5
                    )),
                    (
                        ErrorKind::InvalidConfig,
                        Some(field::FRAME_LEN),
                        ErrorDetail::Expected {
                            expected: u64::try_from(min).unwrap(),
                            found: u64::try_from(len).unwrap()
                        }
                    )
                );
            }
        }
        assert!(
            AosPhysicalChannelConfig::new(13, true, 3, true, AosEdition::Issue4Compatible).is_err()
        );
        assert!(
            AosPhysicalChannelConfig::new(usize::MAX, false, usize::MAX, false, AosEdition::Issue5)
                .is_err()
        );
    }

    #[test]
    fn virtual_channel_builder() {
        // req: API-040, API-070, RS-012, GEN-060
        let sec = SecurityLayout::new(6, 8).unwrap();
        let v = vc(0x2A5, 62, DataFieldContent::Bpdu)
            .with_ocf()
            .with_security(sec);
        assert_eq!(
            (v.scid(), v.vcid(), v.content()),
            (scid(0x2A5), vcid(62), DataFieldContent::Bpdu)
        );
        assert_eq!((v.ocf(), v.security()), (true, sec));
        let plain = vc(1, 0, DataFieldContent::VcaSdu);
        assert!(!plain.ocf() && plain.security().is_none());
        assert_eq!(
            invalid(AosVirtualChannelConfig::new(
                scid(1),
                vcid(63),
                DataFieldContent::IdleData
            )),
            (
                ErrorKind::InvalidConfig,
                Some(field::VC_ID),
                ErrorDetail::Expected {
                    expected: 62,
                    found: 63
                }
            )
        );
    }

    #[test]
    fn a_table_without_a_virtual_channel_is_rejected() {
        // req: AOS-020, GEN-060, API-040
        let p = pc(20, false, 2, true, AosEdition::Issue5);
        let (kind, field, _) = invalid(VcTable::<4>::new(p, &[]));
        assert_eq!(
            (kind, field),
            (ErrorKind::InvalidConfig, Some(field::VC_ID))
        );
    }

    #[test]
    fn table_rejects_what_732_forbids() {
        // req: API-040, AOS-002, GEN-060, GEN-011, CMN-041
        let p = pc(20, false, 2, true, AosEdition::Issue5);
        let ok = VcTable::<4>::new(
            p,
            &[
                vc(1, 0, DataFieldContent::Mpdu),
                vc(2, 0, DataFieldContent::VcaSdu),
            ],
        )
        .unwrap();
        assert_eq!(ok.physical_channel(), p);
        assert_eq!(ok.vcs().map(|v| v.scid().get()).collect::<Vec<_>>(), [1, 2]);
        let dup = VcTable::<4>::new(
            p,
            &[
                vc(1, 3, DataFieldContent::Mpdu),
                vc(1, 3, DataFieldContent::Bpdu),
            ],
        );
        assert_eq!(invalid(dup).1, Some(field::VC_ID));
        let full = VcTable::<1>::new(
            p,
            &[
                vc(1, 0, DataFieldContent::Mpdu),
                vc(1, 1, DataFieldContent::Mpdu),
            ],
        );
        let e = full.unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::CapacityExceeded, Some(field::VC_ID))
        );
        // Issue 4 has an 8-bit SCID.
        let p4 = pc(20, false, 0, false, AosEdition::Issue4Compatible);
        assert!(VcTable::<1>::new(p4, &[vc(255, 0, DataFieldContent::Mpdu)]).is_ok());
        assert_eq!(
            invalid(VcTable::<1>::new(p4, &[vc(256, 0, DataFieldContent::Mpdu)])),
            (
                ErrorKind::InvalidConfig,
                Some(field::SPACECRAFT_ID),
                ErrorDetail::Expected {
                    expected: 255,
                    found: 256
                }
            )
        );
        assert!(
            VcTable::<1>::new(
                pc(20, false, 0, false, AosEdition::Issue5),
                &[vc(1023, 0, DataFieldContent::Mpdu)]
            )
            .is_ok()
        );
        // 20 = 6 + 2 (Insert Zone) + 2 (FECF) + 4 (OCF) + 6 (SDLS): no data octet.
        let sdls = vc(1, 0, DataFieldContent::VcaSdu)
            .with_ocf()
            .with_security(SecurityLayout::new(3, 3).unwrap());
        assert_eq!(
            invalid(VcTable::<1>::new(p, &[sdls])).1,
            Some(field::FRAME_DATA_FIELD)
        );
        // One data octet: enough for a VCA_SDU, not for an M_PDU or B_PDU and its header.
        let tight = SecurityLayout::new(3, 2).unwrap();
        assert!(
            VcTable::<1>::new(
                p,
                &[vc(1, 0, DataFieldContent::IdleData)
                    .with_ocf()
                    .with_security(tight)]
            )
            .is_ok()
        );
        for content in [DataFieldContent::Mpdu, DataFieldContent::Bpdu] {
            let two = SecurityLayout::new(3, 1).unwrap();
            assert_eq!(
                invalid(VcTable::<1>::new(
                    p,
                    &[vc(1, 0, content).with_ocf().with_security(two)]
                )),
                (
                    ErrorKind::InvalidConfig,
                    Some(field::FRAME_DATA_FIELD),
                    ErrorDetail::Expected {
                        expected: 3,
                        found: 2
                    }
                )
            );
            let three = SecurityLayout::new(3, 0).unwrap();
            assert!(
                VcTable::<1>::new(p, &[vc(1, 0, content).with_ocf().with_security(three)]).is_ok()
            );
        }
    }

    #[test]
    fn bitstream_limits() {
        // req: AOS-018, API-040
        let bpdu = vc(1, 0, DataFieldContent::Bpdu);
        // Issue 5: frames of at most 2 048 octets; the zone is then at most 2 040 octets.
        assert!(VcTable::<1>::new(pc(2048, false, 0, false, AosEdition::Issue5), &[bpdu]).is_ok());
        assert_eq!(
            invalid(VcTable::<1>::new(
                pc(2049, false, 0, false, AosEdition::Issue5),
                &[bpdu]
            )),
            (
                ErrorKind::InvalidConfig,
                Some(field::FRAME_LEN),
                ErrorDetail::Expected {
                    expected: 2048,
                    found: 2049
                }
            )
        );
        // Other content on a longer frame is fine.
        assert!(
            VcTable::<1>::new(
                pc(4000, false, 0, false, AosEdition::Issue5),
                &[vc(1, 0, DataFieldContent::Mpdu)]
            )
            .is_ok()
        );
        // Issue 4: no frame limit, but a zone of at most 2 047 octets: 6 + 2 + 2047 = 2055.
        let p4 = |len| pc(len, false, 0, false, AosEdition::Issue4Compatible);
        assert!(VcTable::<1>::new(p4(2055), &[bpdu]).is_ok());
        assert_eq!(
            invalid(VcTable::<1>::new(p4(2056), &[bpdu])),
            (
                ErrorKind::InvalidConfig,
                Some(field::FRAME_DATA_FIELD),
                ErrorDetail::Expected {
                    expected: 2047,
                    found: 2048
                }
            )
        );
    }

    #[test]
    fn lookups_and_layouts() {
        // req: AOS-010, AOS-015, API-070, CMN-040
        let p = pc(40, true, 4, true, AosEdition::Issue5);
        let sec = SecurityLayout::new(2, 1).unwrap();
        let t = VcTable::<3>::new(
            p,
            &[
                vc(7, 1, DataFieldContent::Mpdu)
                    .with_ocf()
                    .with_security(sec),
                vc(9, 2, DataFieldContent::Bpdu),
            ],
        )
        .unwrap();
        assert!(
            t.has_spacecraft(scid(7)) && t.has_spacecraft(scid(9)) && !t.has_spacecraft(scid(8))
        );
        assert_eq!(
            t.vc(scid(7), vcid(1)).map(|v| v.content()),
            Some(DataFieldContent::Mpdu)
        );
        assert_eq!(t.vc(scid(7), vcid(2)), None);
        assert_eq!(t.vc(scid(9), vcid(63)), None);
        let l = t.frame_layout(scid(7), vcid(1)).unwrap();
        assert_eq!(
            l.ranges(),
            [0..8, 8..12, 12..14, 14..33, 33..34, 34..38, 38..40]
        );
        let oid = t.frame_layout(scid(9), vcid(63)).unwrap();
        assert_eq!(
            oid.ranges(),
            [0..8, 8..12, 12..12, 12..38, 38..38, 38..38, 38..40]
        );
        assert_eq!(t.frame_layout(scid(8), vcid(63)), None);
        assert_eq!(t.frame_layout(scid(9), vcid(3)), None);
    }
}

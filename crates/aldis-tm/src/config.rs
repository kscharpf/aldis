//! Managed parameters of the physical, master, and virtual channels (132.0-B-3 §5, §6.6; TM-001,
//! TM-011, API-040, API-041, API-070, GEN-060, GEN-061, RS-012, CMN-041).
//!
//! A [`TmPhysicalChannelConfig`] holds what every frame on the channel shares: the fixed frame
//! length and the presence of the FECF (Table 5-1). A [`TmMasterChannelConfig`] adds the SCID,
//! the `MC_FSH` and `MC_OCF` (Table 5-2), and up to eight [`TmVirtualChannelConfig`]s, one per VCID
//! (Table 5-3, Table 6-1). Building it checks every combination 132.0 forbids, so a frame layout
//! can be computed for each of its VCs without failing.
//!
//! The Valid Spacecraft IDs of a physical channel (Table 5-1) are the SCIDs of the master channel
//! configurations the receiver holds: a decoder given a slice of them ([`VcLookup`]) rejects any
//! other SCID with `UnknownChannel`. The MC and VC Multiplexing Schemes are mission-specific and
//! belong to the engines (M4).

use aldis_core::ids::{SpacecraftId, VcId};
use aldis_core::layout::{FECF_LEN, FrameLayout, LayoutSpec};
use aldis_core::security::SecurityLayout;
use aldis_core::{Error, ErrorKind, Result};

use crate::field;
use crate::header::{FshLen, PRIMARY_HEADER_LEN, len_u64, mismatch};

/// Managed parameters of a physical channel (132.0 §5.1 Table 5-1; TM-001, API-041, RS-012).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TmPhysicalChannelConfig {
    frame_len: usize,
    fecf: bool,
}

impl TmPhysicalChannelConfig {
    /// A physical channel of `frame_len`-octet frames, with or without the FECF (132.0 §4.1.1.2,
    /// §4.1.6.1.2).
    ///
    /// 132.0 sets no upper bound on the frame length; the channel coding of the physical layer
    /// does (§4.1.1.2).
    ///
    /// # Errors
    /// `InvalidConfig` (`frame_len`) if a frame cannot hold the Primary Header, one data octet,
    /// and the FECF (132.0 §4.1.1.1; CMN-041), with that minimum.
    pub const fn new(frame_len: usize, fecf: bool) -> Result<TmPhysicalChannelConfig> {
        let min = TmPhysicalChannelConfig::min_frame_len(fecf);
        if frame_len < min {
            return Err(mismatch(
                ErrorKind::InvalidConfig,
                field::FRAME_LEN,
                len_u64(min),
                len_u64(frame_len),
            ));
        }
        Ok(TmPhysicalChannelConfig { frame_len, fecf })
    }

    /// The shortest frame: the Primary Header, one data octet, and the FECF if present.
    const fn min_frame_len(fecf: bool) -> usize {
        PRIMARY_HEADER_LEN
            .saturating_add(1)
            .saturating_add(if fecf { FECF_LEN } else { 0 })
    }

    /// Transfer Frame Length in octets (132.0 §4.1.1.2, Table 5-1; API-012).
    #[must_use]
    pub const fn frame_len(&self) -> usize {
        self.frame_len
    }

    /// Whether every frame ends with a Frame Error Control Field (132.0 §4.1.6.1.2–3).
    #[must_use]
    pub const fn fecf(&self) -> bool {
        self.fecf
    }
}

/// What a virtual channel's data fields carry, static per VC (132.0 §4.1.4.3–4, Table 5-3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DataFieldContent {
    /// Packets, and Idle Data in OID frames: Synchronization Flag `0` (132.0 §4.1.2.7.3.2).
    Packets,
    /// One `VCA_SDU` per frame: Synchronization Flag `1` (132.0 §4.1.2.7.3.2).
    VcaSdu,
}

impl DataFieldContent {
    /// The Synchronization Flag of every frame on such a VC (132.0 §4.1.2.7.3.2–3; TM-011).
    #[must_use]
    pub const fn sync_flag(self) -> bool {
        matches!(self, DataFieldContent::VcaSdu)
    }
}

/// Managed parameters of a virtual channel (132.0 §5.3 Table 5-3, §6.6.2 Table 6-1; API-070,
/// RS-012).
///
/// Every field is valid by its type; whether it fits its master channel is checked by
/// [`TmMasterChannelConfig::with_vc`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TmVirtualChannelConfig {
    vcid: VcId<3>,
    content: DataFieldContent,
    fsh: Option<FshLen>,
    ocf: bool,
    security: SecurityLayout,
}

impl TmVirtualChannelConfig {
    /// A virtual channel carrying `content`, without `VC_FSH`, `VC_OCF`, or SDLS fields.
    #[must_use]
    pub const fn new(vcid: VcId<3>, content: DataFieldContent) -> TmVirtualChannelConfig {
        TmVirtualChannelConfig {
            vcid,
            content,
            fsh: None,
            ocf: false,
            security: SecurityLayout::NONE,
        }
    }

    /// The same channel with a `VC_FSH` of `len` octets (132.0 §4.1.3.1.4, Table 5-3).
    #[must_use]
    pub const fn with_fsh(self, len: FshLen) -> TmVirtualChannelConfig {
        TmVirtualChannelConfig {
            fsh: Some(len),
            ..self
        }
    }

    /// The same channel with a `VC_OCF` (132.0 §4.1.5.2, Table 5-3).
    #[must_use]
    pub const fn with_ocf(self) -> TmVirtualChannelConfig {
        TmVirtualChannelConfig { ocf: true, ..self }
    }

    /// The same channel with a Security Header and Trailer (132.0 §6.3, Table 6-1; API-070).
    #[must_use]
    pub const fn with_security(self, security: SecurityLayout) -> TmVirtualChannelConfig {
        TmVirtualChannelConfig { security, ..self }
    }

    /// Virtual Channel Identifier (132.0 §4.1.2.3, Table 5-3).
    #[must_use]
    pub const fn vcid(&self) -> VcId<3> {
        self.vcid
    }

    /// Data Field Content (132.0 §4.1.4.4, Table 5-3).
    #[must_use]
    pub const fn content(&self) -> DataFieldContent {
        self.content
    }

    /// `VC_FSH` length, if the VC has one (Table 5-3).
    #[must_use]
    pub const fn fsh(&self) -> Option<FshLen> {
        self.fsh
    }

    /// Whether the VC has a `VC_OCF` (Table 5-3).
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

/// A virtual channel of a master channel, with the layout of its frames.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct VcEntry {
    pub(crate) config: TmVirtualChannelConfig,
    pub(crate) layout: FrameLayout,
}

/// Managed parameters of a master channel and its virtual channels (132.0 §5.2 Table 5-2;
/// TM-011, API-040, API-041, RS-012, CMN-041).
///
/// Holds the physical channel it is carried on, so that the layout of every VC's frames is known
/// (CMN-040). Built by [`new`](Self::new) and [`with_vc`](Self::with_vc), which reject what
/// 132.0 forbids.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TmMasterChannelConfig {
    pc: TmPhysicalChannelConfig,
    scid: SpacecraftId<10>,
    fsh: Option<FshLen>,
    ocf: bool,
    vcs: [Option<VcEntry>; 8],
}

impl TmMasterChannelConfig {
    /// A master channel on `pc` with an `MC_FSH` of `fsh` octets if given, and an `MC_OCF` if `ocf`
    /// (Table 5-2), and no virtual channel yet.
    #[must_use]
    pub const fn new(
        pc: TmPhysicalChannelConfig,
        scid: SpacecraftId<10>,
        fsh: Option<FshLen>,
        ocf: bool,
    ) -> TmMasterChannelConfig {
        TmMasterChannelConfig {
            pc,
            scid,
            fsh,
            ocf,
            vcs: [const { None }; 8],
        }
    }

    /// The same master channel with the virtual channel `vc` added (Table 5-3).
    ///
    /// # Errors
    /// `InvalidConfig` with the field of the conflict:
    /// - `fsh_len` if both the master channel and `vc` have a Secondary Header: it belongs to
    ///   one or the other (132.0 §4.1.3.1.4);
    /// - `ocf_flag` if both have an OCF (132.0 §4.1.5.2);
    /// - `frame_data_field` if the headers, SDLS fields, OCF, and FECF leave no data octet in a
    ///   frame of the VC (132.0 §4.1.4.2, §6.3.5.2; CMN-041);
    /// - `vc_id` if the VCID is already configured.
    pub fn with_vc(mut self, vc: TmVirtualChannelConfig) -> Result<TmMasterChannelConfig> {
        if self.fsh.is_some() && vc.fsh.is_some() {
            return Err(Error::new(ErrorKind::InvalidConfig).with_field(field::FSH_LEN));
        }
        if self.ocf && vc.ocf {
            return Err(Error::new(ErrorKind::InvalidConfig).with_field(field::OCF_FLAG));
        }
        let layout = FrameLayout::compute(&LayoutSpec {
            frame_len: self.pc.frame_len,
            primary_header_len: PRIMARY_HEADER_LEN,
            secondary_len: self.fsh.or(vc.fsh).map_or(0, FshLen::octets),
            security: vc.security,
            ocf: self.ocf || vc.ocf,
            fecf: self.pc.fecf,
        })?;
        match self.vcs.get_mut(usize::from(vc.vcid.get())) {
            Some(slot @ None) => *slot = Some(VcEntry { config: vc, layout }),
            _ => return Err(Error::new(ErrorKind::InvalidConfig).with_field(field::VC_ID)),
        }
        Ok(self)
    }

    /// The physical channel (Table 5-1).
    #[must_use]
    pub const fn pc(&self) -> &TmPhysicalChannelConfig {
        &self.pc
    }

    /// Spacecraft Identifier (Table 5-2).
    #[must_use]
    pub const fn scid(&self) -> SpacecraftId<10> {
        self.scid
    }

    /// `MC_FSH` length, if the master channel has one (Table 5-2).
    #[must_use]
    pub const fn fsh(&self) -> Option<FshLen> {
        self.fsh
    }

    /// Whether the master channel has an `MC_OCF` (Table 5-2).
    #[must_use]
    pub const fn ocf(&self) -> bool {
        self.ocf
    }

    /// The virtual channel `vcid`, if configured (Valid VCIDs, Table 5-2).
    #[must_use]
    pub fn vc(&self, vcid: VcId<3>) -> Option<&TmVirtualChannelConfig> {
        self.entry(vcid).map(|e| &e.config)
    }

    /// The configured virtual channels in VCID order (Table 5-2).
    pub fn vcs(&self) -> impl Iterator<Item = &TmVirtualChannelConfig> {
        self.vcs.iter().flatten().map(|e| &e.config)
    }

    /// The layout of every frame of virtual channel `vcid`, if configured: its Secondary Header
    /// is the `MC_FSH` or `VC_FSH`, and its OCF the `MC_OCF` or `VC_OCF` (132.0 §4.1, §6.3; CMN-040).
    #[must_use]
    pub fn frame_layout(&self, vcid: VcId<3>) -> Option<&FrameLayout> {
        self.entry(vcid).map(|e| &e.layout)
    }

    /// The Secondary Header length of frames on `vc`: the `MC_FSH`, or else the `VC_FSH` (132.0
    /// §4.1.3.1.4; TM-011).
    pub(crate) const fn fsh_of(&self, vc: &TmVirtualChannelConfig) -> Option<FshLen> {
        match self.fsh {
            Some(len) => Some(len),
            None => vc.fsh,
        }
    }

    /// Whether frames on `vc` carry an OCF: the `MC_OCF`, or else the `VC_OCF` (132.0 §4.1.5.2;
    /// TM-011).
    pub(crate) const fn ocf_of(&self, vc: &TmVirtualChannelConfig) -> bool {
        self.ocf || vc.ocf
    }

    /// The configured virtual channel `vcid` with its layout.
    pub(crate) fn entry(&self, vcid: VcId<3>) -> Option<&VcEntry> {
        self.vcs
            .get(usize::from(vcid.get()))
            .and_then(Option::as_ref)
    }
}

/// Finds the Security Header and Trailer lengths of a frame's virtual channel, which a decoder
/// needs to place the data field (132.0 §6.3, Table 6-1; API-070, M2 plan D14).
///
/// Implemented by [`TmMasterChannelConfig`], by a slice of them for a physical channel that
/// carries several master channels, and by [`NoSdls`] for a channel on which no VC uses SDLS.
pub trait VcLookup {
    /// The security layout of virtual channel `vcid` of spacecraft `scid` (API-070).
    ///
    /// # Errors
    /// `UnknownChannel` (`spacecraft_id` or `vc_id`) if the channel is not configured.
    fn security_layout(&self, scid: SpacecraftId<10>, vcid: VcId<3>) -> Result<SecurityLayout>;
}

impl VcLookup for TmMasterChannelConfig {
    fn security_layout(&self, scid: SpacecraftId<10>, vcid: VcId<3>) -> Result<SecurityLayout> {
        if scid != self.scid {
            return Err(unknown_scid(scid, self.scid));
        }
        self.vc(vcid)
            .map(TmVirtualChannelConfig::security)
            .ok_or(Error::new(ErrorKind::UnknownChannel).with_field(field::VC_ID))
    }
}

impl VcLookup for [TmMasterChannelConfig] {
    fn security_layout(&self, scid: SpacecraftId<10>, vcid: VcId<3>) -> Result<SecurityLayout> {
        self.iter()
            .find(|mc| mc.scid == scid)
            .ok_or(Error::new(ErrorKind::UnknownChannel).with_field(field::SPACECRAFT_ID))?
            .security_layout(scid, vcid)
    }
}

/// `UnknownChannel` (`spacecraft_id`) for `found`, where `expected` is configured.
pub(crate) fn unknown_scid(found: SpacecraftId<10>, expected: SpacecraftId<10>) -> Error {
    mismatch(
        ErrorKind::UnknownChannel,
        field::SPACECRAFT_ID,
        u64::from(expected.get()),
        u64::from(found.get()),
    )
}

/// A physical channel on which no virtual channel uses SDLS: every frame has the layout of
/// 132.0 §4.1 (API-070).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct NoSdls;

impl VcLookup for NoSdls {
    fn security_layout(&self, _: SpacecraftId<10>, _: VcId<3>) -> Result<SecurityLayout> {
        Ok(SecurityLayout::NONE)
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    extern crate std;
    use std::vec::Vec;

    use super::{
        DataFieldContent, NoSdls, TmMasterChannelConfig, TmPhysicalChannelConfig,
        TmVirtualChannelConfig, VcLookup,
    };
    use crate::field;
    use crate::header::FshLen;
    use aldis_core::ids::{SpacecraftId, VcId};
    use aldis_core::security::SecurityLayout;
    use aldis_core::{ErrorDetail, ErrorKind};

    fn scid(n: u16) -> SpacecraftId<10> {
        SpacecraftId::new(n).unwrap()
    }

    fn vcid(n: u8) -> VcId<3> {
        VcId::new(n).unwrap()
    }

    #[test]
    fn physical_channel_minimum() {
        // req: TM-001, API-040, CMN-041, RS-012
        const PC: TmPhysicalChannelConfig = match TmPhysicalChannelConfig::new(1115, true) {
            Ok(pc) => pc,
            Err(_) => panic!("valid"),
        };
        assert_eq!((PC.frame_len(), PC.fecf()), (1115, true));
        assert!(TmPhysicalChannelConfig::new(7, false).is_ok());
        assert!(TmPhysicalChannelConfig::new(9, true).is_ok());
        for (len, fecf, min) in [(6, false, 7), (8, true, 9), (0, false, 7)] {
            let e = TmPhysicalChannelConfig::new(len, fecf).unwrap_err();
            assert_eq!(
                (e.kind(), e.field(), e.detail()),
                (
                    ErrorKind::InvalidConfig,
                    Some(field::FRAME_LEN),
                    ErrorDetail::Expected {
                        expected: min,
                        found: u64::try_from(len).unwrap()
                    }
                )
            );
        }
    }

    #[test]
    fn virtual_channel_builder() {
        // req: API-070, RS-012, GEN-060
        let fsh = FshLen::new(4).unwrap();
        let sec = SecurityLayout::new(6, 8).unwrap();
        let vc = TmVirtualChannelConfig::new(vcid(3), DataFieldContent::VcaSdu)
            .with_fsh(fsh)
            .with_ocf()
            .with_security(sec);
        assert_eq!(vc.vcid(), vcid(3));
        assert_eq!(vc.content(), DataFieldContent::VcaSdu);
        assert_eq!((vc.fsh(), vc.ocf(), vc.security()), (Some(fsh), true, sec));
        assert!(DataFieldContent::VcaSdu.sync_flag());
        assert!(!DataFieldContent::Packets.sync_flag());
        let plain = TmVirtualChannelConfig::new(vcid(0), DataFieldContent::Packets);
        assert_eq!((plain.fsh(), plain.ocf()), (None, false));
        assert!(plain.security().is_none());
    }

    #[test]
    fn master_channel_rejects_what_132_forbids() {
        // req: TM-011, API-040, CMN-041
        let pc = TmPhysicalChannelConfig::new(20, true).unwrap();
        let fsh = FshLen::new(4).unwrap();
        let packets = |n| TmVirtualChannelConfig::new(vcid(n), DataFieldContent::Packets);
        let invalid = |r: Result<TmMasterChannelConfig, aldis_core::Error>, f| {
            let e = r.unwrap_err();
            assert_eq!((e.kind(), e.field()), (ErrorKind::InvalidConfig, Some(f)));
        };
        let mc_fsh = TmMasterChannelConfig::new(pc, scid(1), Some(fsh), false);
        assert_eq!((mc_fsh.fsh(), mc_fsh.ocf()), (Some(fsh), false));
        invalid(mc_fsh.with_vc(packets(0).with_fsh(fsh)), field::FSH_LEN);
        let mc_ocf = TmMasterChannelConfig::new(pc, scid(1), None, true);
        invalid(
            mc_ocf.clone().with_vc(packets(0).with_ocf()),
            field::OCF_FLAG,
        );
        let mc = mc_ocf.with_vc(packets(0)).unwrap();
        invalid(mc.clone().with_vc(packets(0)), field::VC_ID);
        // 20 = 6 + 4 (OCF) + 2 (FECF) + 8 octets of SDLS: no data octet left.
        invalid(
            mc.clone()
                .with_vc(packets(1).with_security(SecurityLayout::new(4, 4).unwrap())),
            field::FRAME_DATA_FIELD,
        );
        let mc = mc
            .with_vc(packets(1).with_security(SecurityLayout::new(4, 3).unwrap()))
            .unwrap();
        assert_eq!(mc.frame_layout(vcid(1)).unwrap().data(), 10..11);
        assert_eq!(mc.vcs().map(|v| v.vcid().get()).collect::<Vec<_>>(), [0, 1]);
        assert_eq!(mc.vc(vcid(2)), None);
        assert_eq!(mc.frame_layout(vcid(2)), None);
        assert_eq!((mc.scid(), mc.fsh(), mc.ocf()), (scid(1), None, true));
        assert_eq!(mc.pc(), &pc);
    }

    #[test]
    fn effective_fsh_and_ocf() {
        // req: TM-011, CMN-040
        let pc = TmPhysicalChannelConfig::new(100, false).unwrap();
        let mc_fsh = FshLen::new(10).unwrap();
        let vc_fsh = FshLen::new(5).unwrap();
        let vc0 = TmVirtualChannelConfig::new(vcid(0), DataFieldContent::Packets).with_ocf();
        let mc = TmMasterChannelConfig::new(pc, scid(9), Some(mc_fsh), false)
            .with_vc(vc0)
            .unwrap();
        assert_eq!(mc.fsh_of(&vc0), Some(mc_fsh));
        assert!(mc.ocf_of(&vc0));
        let layout = mc.frame_layout(vcid(0)).unwrap();
        assert_eq!((layout.secondary(), layout.ocf()), (6..16, 96..100));
        let vc1 = TmVirtualChannelConfig::new(vcid(1), DataFieldContent::Packets).with_fsh(vc_fsh);
        let mc = TmMasterChannelConfig::new(pc, scid(9), None, true)
            .with_vc(vc1)
            .unwrap();
        assert_eq!(mc.fsh_of(&vc1), Some(vc_fsh));
        assert!(mc.ocf_of(&vc1));
    }

    #[test]
    fn lookups_find_the_security_layout() {
        // req: API-070, TM-011
        let pc = TmPhysicalChannelConfig::new(64, true).unwrap();
        let sec = SecurityLayout::new(2, 4).unwrap();
        let mc = |s| {
            TmMasterChannelConfig::new(pc, scid(s), None, false)
                .with_vc(
                    TmVirtualChannelConfig::new(vcid(2), DataFieldContent::Packets)
                        .with_security(sec),
                )
                .unwrap()
        };
        let one = mc(7);
        assert_eq!(one.security_layout(scid(7), vcid(2)), Ok(sec));
        let e = one.security_layout(scid(8), vcid(2)).unwrap_err();
        assert_eq!(
            (e.kind(), e.field(), e.detail()),
            (
                ErrorKind::UnknownChannel,
                Some(field::SPACECRAFT_ID),
                ErrorDetail::Expected {
                    expected: 7,
                    found: 8
                }
            )
        );
        let e = one.security_layout(scid(7), vcid(3)).unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::UnknownChannel, Some(field::VC_ID))
        );
        let many = [mc(7), mc(9)];
        assert_eq!(many[..].security_layout(scid(9), vcid(2)), Ok(sec));
        let e = many[..].security_layout(scid(8), vcid(2)).unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::UnknownChannel, Some(field::SPACECRAFT_ID))
        );
        assert_eq!(
            NoSdls.security_layout(scid(1023), vcid(7)),
            Ok(SecurityLayout::NONE)
        );
    }
}

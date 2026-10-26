//! Managed parameters of the TC Space Data Link Protocol that the codec reads (232.0 §5,
//! §6.6; GEN-060, GEN-061, API-040, API-041, API-070, RS-012).
//!
//! | Level | Type | Parameters kept |
//! |---|---|---|
//! | Physical Channel (Table 5-1) | [`PhysicalChannelConfig`] | Maximum Transfer Frame Length, presence of the FECF |
//! | Master Channel (Table 5-2) | the [`VcConfig`] entries of one SCID | Spacecraft ID, valid VCIDs |
//! | Virtual Channel (Tables 5-3, 6-2) | [`VcConfig`] | SCID, VCID, Maximum Transfer Frame Length, presence of the Segment Header, Security Header and Trailer lengths without Segment Header |
//! | MAP (Tables 5-4, 6-3) | [`MapConfig`] | SCID, VCID, MAP ID, Security Header and Trailer lengths |
//!
//! The decoder, the writers, and the Frame Validation Checks look the channels up through
//! [`VcLookup`]; [`VcTable`] is its fixed-capacity implementation (M2 plan D14). Parameters
//! that only the engines use (data field content, blocking, segmentation, repetitions, CLCW
//! reporting rate, the multiplexing schemes) are not kept here.

use aldis_core::layout::{FECF_LEN, FrameLayout, LayoutSpec};
use aldis_core::security::SecurityLayout;
use aldis_core::{Error, ErrorKind, Result};

use crate::field;
use crate::header::{
    MAX_FRAME_LEN, MIN_FRAME_LEN, PRIMARY_HEADER_LEN, SEGMENT_HEADER_LEN, TcMapId, TcSpacecraftId,
    TcVcId,
};

/// `InvalidConfig` naming the maximum frame length.
const fn bad_max_len() -> Error {
    Error::new(ErrorKind::InvalidConfig).with_field(field::MAX_FRAME_LEN)
}

/// The managed parameters of a Physical Channel that the codec reads (232.0 §5.1 Table 5-1;
/// GEN-060, TC-006, TC-007).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PhysicalChannelConfig {
    max_frame_len: usize,
    fecf: bool,
}

impl PhysicalChannelConfig {
    /// A Physical Channel with frames of at most `max_frame_len` octets, each with an FECF when
    /// `fecf` is set (232.0 Table 5-1, §4.1.4.1.1; GEN-060).
    ///
    /// # Errors
    /// `InvalidConfig` (`max_frame_len`) above 1024 octets, or below the shortest frame: six
    /// octets, eight with the FECF (232.0 §4.1.1, §4.1.2.7).
    pub const fn new(max_frame_len: usize, fecf: bool) -> Result<PhysicalChannelConfig> {
        let shortest = MIN_FRAME_LEN.saturating_add(if fecf { FECF_LEN } else { 0 });
        if max_frame_len < shortest || max_frame_len > MAX_FRAME_LEN {
            return Err(bad_max_len());
        }
        Ok(PhysicalChannelConfig {
            max_frame_len,
            fecf,
        })
    }

    /// Maximum Transfer Frame Length in octets (232.0 Table 5-1; TC-007).
    #[must_use]
    pub const fn max_frame_len(&self) -> usize {
        self.max_frame_len
    }

    /// Whether every frame ends with a Frame Error Control Field (232.0 §4.1.4.1; TC-006).
    #[must_use]
    pub const fn fecf(&self) -> bool {
        self.fecf
    }
}

/// Whether a Virtual Channel's Type-D frames carry a Segment Header, and where the SDLS
/// lengths are set (232.0 §4.1.3.2.2.1.2, §6.3.4.2 notes 2–3, §6.6; TC-004, API-070).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SegmentHeaderUse {
    /// No Segment Header; the Security Header and Trailer lengths of the Virtual Channel
    /// (Table 6-2), [`SecurityLayout::NONE`] without SDLS.
    Absent(SecurityLayout),
    /// A Segment Header on every Type-D frame; the Security Header and Trailer lengths are set
    /// per MAP by [`MapConfig`] (Table 6-3).
    Present,
}

/// The managed parameters of one Virtual Channel that the codec reads (232.0 §5.3 Table 5-3,
/// §6.6.1 Table 6-2; GEN-060, TC-004, API-070).
///
/// The Spacecraft ID names the Master Channel the Virtual Channel belongs to; the valid VCIDs of
/// a Master Channel are those of its entries (Table 5-2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VcConfig {
    spacecraft_id: TcSpacecraftId,
    vc_id: TcVcId,
    max_frame_len: usize,
    segment_header: SegmentHeaderUse,
}

impl VcConfig {
    /// A Virtual Channel (232.0 Table 5-3; GEN-060, API-040).
    ///
    /// # Errors
    /// `InvalidConfig` (`max_frame_len`) above 1024 octets or below six (232.0 Table 5-3,
    /// §4.1.1). Whether the Security Header and Trailer leave room for data is checked by
    /// [`VcTable::new`], which knows whether the FECF is present.
    pub const fn new(
        spacecraft_id: TcSpacecraftId,
        vc_id: TcVcId,
        max_frame_len: usize,
        segment_header: SegmentHeaderUse,
    ) -> Result<VcConfig> {
        if max_frame_len < MIN_FRAME_LEN || max_frame_len > MAX_FRAME_LEN {
            return Err(bad_max_len());
        }
        Ok(VcConfig {
            spacecraft_id,
            vc_id,
            max_frame_len,
            segment_header,
        })
    }

    /// Spacecraft ID of the Virtual Channel's Master Channel (232.0 Table 5-3).
    #[must_use]
    pub const fn spacecraft_id(&self) -> TcSpacecraftId {
        self.spacecraft_id
    }

    /// Virtual Channel Identifier (232.0 Table 5-3).
    #[must_use]
    pub const fn vc_id(&self) -> TcVcId {
        self.vc_id
    }

    /// Maximum Transfer Frame Length in octets (232.0 Table 5-3; TC-007).
    #[must_use]
    pub const fn max_frame_len(&self) -> usize {
        self.max_frame_len
    }

    /// Whether Type-D frames carry a Segment Header, and the VC's SDLS lengths without one
    /// (232.0 §4.1.3.2.2.1.2, Table 6-2; TC-004).
    #[must_use]
    pub const fn segment_header(&self) -> SegmentHeaderUse {
        self.segment_header
    }

    /// Whether Type-D frames carry a Segment Header (TC-004).
    pub(crate) const fn has_segment_header(&self) -> bool {
        matches!(self.segment_header, SegmentHeaderUse::Present)
    }

    /// The Virtual Channel's Security Header and Trailer lengths; none on a Virtual Channel
    /// with Segment Headers, whose lengths are set per MAP (232.0 §6.6; API-070).
    pub(crate) const fn security(&self) -> SecurityLayout {
        match self.segment_header {
            SegmentHeaderUse::Absent(layout) => layout,
            SegmentHeaderUse::Present => SecurityLayout::NONE,
        }
    }
}

/// The managed parameters of one MAP Channel that the codec reads (232.0 §5.4 Table 5-4,
/// §6.6.2 Table 6-3; GEN-060, TC-022, API-070).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MapConfig {
    spacecraft_id: TcSpacecraftId,
    vc_id: TcVcId,
    map_id: TcMapId,
    security: SecurityLayout,
}

impl MapConfig {
    /// A MAP Channel on the Virtual Channel (`spacecraft_id`, `vc_id`), with its Security
    /// Header and Trailer lengths (232.0 Tables 5-4, 6-3; API-070).
    #[must_use]
    pub const fn new(
        spacecraft_id: TcSpacecraftId,
        vc_id: TcVcId,
        map_id: TcMapId,
        security: SecurityLayout,
    ) -> MapConfig {
        MapConfig {
            spacecraft_id,
            vc_id,
            map_id,
            security,
        }
    }

    /// Spacecraft ID (232.0 Table 5-4).
    #[must_use]
    pub const fn spacecraft_id(&self) -> TcSpacecraftId {
        self.spacecraft_id
    }

    /// Virtual Channel Identifier (232.0 Table 5-4).
    #[must_use]
    pub const fn vc_id(&self) -> TcVcId {
        self.vc_id
    }

    /// MAP Identifier (232.0 Table 5-4).
    #[must_use]
    pub const fn map_id(&self) -> TcMapId {
        self.map_id
    }

    /// Security Header and Trailer lengths of the MAP (232.0 Table 6-3; API-070).
    #[must_use]
    pub const fn security(&self) -> SecurityLayout {
        self.security
    }
}

/// Looks up the managed parameters of the channels a frame may belong to (M2 plan D14;
/// GEN-060, TC-021, TC-022).
///
/// [`VcTable`] implements it. Another implementation must answer consistently: a VCID that
/// [`vc`](Self::vc) finds belongs to a Spacecraft ID that
/// [`has_spacecraft`](Self::has_spacecraft) accepts.
pub trait VcLookup {
    /// The Physical Channel (232.0 Table 5-1).
    fn physical_channel(&self) -> PhysicalChannelConfig;
    /// Whether `spacecraft_id` is a valid Spacecraft ID of the Physical Channel, so that the
    /// frame has an expected Master Channel Identifier (232.0 §4.4.9.3.3 b, Table 5-1).
    fn has_spacecraft(&self, spacecraft_id: TcSpacecraftId) -> bool;
    /// The Virtual Channel (`spacecraft_id`, `vc_id`), or `None` if it is not valid (232.0
    /// §4.4.7.3; TC-022).
    fn vc(&self, spacecraft_id: TcSpacecraftId, vc_id: TcVcId) -> Option<VcConfig>;
    /// The MAP (`spacecraft_id`, `vc_id`, `map_id`), or `None` if it is not valid (232.0
    /// §4.4.3.3; TC-022).
    fn map(
        &self,
        spacecraft_id: TcSpacecraftId,
        vc_id: TcVcId,
        map_id: TcMapId,
    ) -> Option<MapConfig>;
}

/// `InvalidConfig` (`frame_data_field`) unless a frame of `limit` octets with a header of
/// `secondary_len` and the SDLS lengths of `security` has a data field (CMN-041).
fn check_room(
    physical: &PhysicalChannelConfig,
    limit: usize,
    secondary_len: usize,
    security: SecurityLayout,
) -> Result<()> {
    FrameLayout::compute(&LayoutSpec {
        frame_len: limit.min(physical.max_frame_len),
        primary_header_len: PRIMARY_HEADER_LEN,
        secondary_len,
        security,
        ocf: false,
        fecf: physical.fecf,
    })
    .map(|_| ())
}

/// The channels of one Physical Channel, with room for `V` Virtual Channels and `M` MAPs
/// (232.0 §5; GEN-011, GEN-060, M2 plan D14).
///
/// Built only by [`new`](Self::new), so the entries are unique and each MAP belongs to a
/// Virtual Channel with Segment Headers (RS-020).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VcTable<const V: usize, const M: usize> {
    physical: PhysicalChannelConfig,
    vcs: [Option<VcConfig>; V],
    maps: [Option<MapConfig>; M],
}

impl<const V: usize, const M: usize> VcTable<V, M> {
    /// The channels of `physical` (232.0 §5; GEN-060, API-040).
    ///
    /// # Errors
    /// - `CapacityExceeded` (`vc_id`, `map_id`) for more than `V` Virtual Channels or `M`
    ///   MAPs (GEN-011).
    /// - `InvalidConfig` (`vc_id`) for no Virtual Channel at all (a Physical Channel with none
    ///   receives nothing, OI-55), or for a Virtual Channel given twice; (`map_id`) for a MAP
    ///   given twice, or on a Virtual Channel that is not given or has no Segment Header
    ///   (232.0 §4.1.3.2.2.1.2).
    /// - `InvalidConfig` (`frame_data_field`) for a Virtual Channel or MAP whose Segment
    ///   Header, Security Header and Trailer, and FECF leave no data octet in its longest
    ///   frame (CMN-041, API-070).
    pub fn new(
        physical: PhysicalChannelConfig,
        vcs: &[VcConfig],
        maps: &[MapConfig],
    ) -> Result<VcTable<V, M>> {
        if vcs.is_empty() {
            return Err(Error::new(ErrorKind::InvalidConfig).with_field(field::VC_ID));
        }
        let mut table = VcTable {
            physical,
            vcs: [None; V],
            maps: [None; M],
        };
        for (i, vc) in vcs.iter().enumerate() {
            if table.vc(vc.spacecraft_id, vc.vc_id).is_some() {
                return Err(Error::new(ErrorKind::InvalidConfig).with_field(field::VC_ID));
            }
            let secondary = if vc.has_segment_header() {
                SEGMENT_HEADER_LEN
            } else {
                0
            };
            check_room(&physical, vc.max_frame_len, secondary, vc.security())?;
            let slot = table
                .vcs
                .get_mut(i)
                .ok_or(Error::new(ErrorKind::CapacityExceeded).with_field(field::VC_ID))?;
            *slot = Some(*vc);
        }
        for (i, map) in maps.iter().enumerate() {
            let vc = table
                .vc(map.spacecraft_id, map.vc_id)
                .filter(VcConfig::has_segment_header)
                .ok_or(Error::new(ErrorKind::InvalidConfig).with_field(field::MAP_ID))?;
            if table
                .map(map.spacecraft_id, map.vc_id, map.map_id)
                .is_some()
            {
                return Err(Error::new(ErrorKind::InvalidConfig).with_field(field::MAP_ID));
            }
            check_room(
                &physical,
                vc.max_frame_len,
                SEGMENT_HEADER_LEN,
                map.security,
            )?;
            let slot = table
                .maps
                .get_mut(i)
                .ok_or(Error::new(ErrorKind::CapacityExceeded).with_field(field::MAP_ID))?;
            *slot = Some(*map);
        }
        Ok(table)
    }

    /// The Physical Channel (232.0 Table 5-1).
    #[must_use]
    pub const fn physical(&self) -> PhysicalChannelConfig {
        self.physical
    }

    /// The Virtual Channels, in the order given (232.0 Table 5-3).
    pub fn vcs(&self) -> impl Iterator<Item = &VcConfig> {
        self.vcs.iter().flatten()
    }

    /// The MAPs, in the order given (232.0 Table 5-4).
    pub fn maps(&self) -> impl Iterator<Item = &MapConfig> {
        self.maps.iter().flatten()
    }
}

impl<const V: usize, const M: usize> VcLookup for VcTable<V, M> {
    fn physical_channel(&self) -> PhysicalChannelConfig {
        self.physical
    }

    fn has_spacecraft(&self, spacecraft_id: TcSpacecraftId) -> bool {
        self.vcs().any(|vc| vc.spacecraft_id == spacecraft_id)
    }

    fn vc(&self, spacecraft_id: TcSpacecraftId, vc_id: TcVcId) -> Option<VcConfig> {
        self.vcs()
            .find(|vc| vc.spacecraft_id == spacecraft_id && vc.vc_id == vc_id)
            .copied()
    }

    fn map(
        &self,
        spacecraft_id: TcSpacecraftId,
        vc_id: TcVcId,
        map_id: TcMapId,
    ) -> Option<MapConfig> {
        self.maps()
            .find(|m| m.spacecraft_id == spacecraft_id && m.vc_id == vc_id && m.map_id == map_id)
            .copied()
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
pub(crate) mod tests {
    use super::{MapConfig, PhysicalChannelConfig, SegmentHeaderUse, VcConfig, VcLookup, VcTable};
    use crate::field;
    use crate::header::{TcMapId, TcSpacecraftId, TcVcId};
    use aldis_core::ErrorKind;
    use aldis_core::security::SecurityLayout;

    pub(crate) fn scid(n: u16) -> TcSpacecraftId {
        TcSpacecraftId::new(n).unwrap()
    }

    pub(crate) fn vcid(n: u8) -> TcVcId {
        TcVcId::new(n).unwrap()
    }

    pub(crate) fn mapid(n: u8) -> TcMapId {
        TcMapId::new(n).unwrap()
    }

    pub(crate) fn plain(n: u8) -> VcConfig {
        VcConfig::new(
            scid(0x155),
            vcid(n),
            1024,
            SegmentHeaderUse::Absent(SecurityLayout::NONE),
        )
        .unwrap()
    }

    pub(crate) fn segmented(n: u8) -> VcConfig {
        VcConfig::new(scid(0x155), vcid(n), 1024, SegmentHeaderUse::Present).unwrap()
    }

    pub(crate) fn map(vc: u8, map: u8, h: usize, t: usize) -> MapConfig {
        MapConfig::new(
            scid(0x155),
            vcid(vc),
            mapid(map),
            SecurityLayout::new(h, t).unwrap(),
        )
    }

    /// VC 1 plain, VC 2 with Segment Headers (MAP 0, MAP 5, MAP 9 with SDLS 3/1), VC 3 with
    /// SDLS 4/2, FECF: the configuration of the shared vectors.
    pub(crate) fn standard(fecf: bool) -> VcTable<4, 4> {
        let vc3 = VcConfig::new(
            scid(0x155),
            vcid(3),
            1024,
            SegmentHeaderUse::Absent(SecurityLayout::new(4, 2).unwrap()),
        )
        .unwrap();
        VcTable::new(
            PhysicalChannelConfig::new(1024, fecf).unwrap(),
            &[plain(1), segmented(2), vc3],
            &[map(2, 0, 0, 0), map(2, 5, 0, 0), map(2, 9, 3, 1)],
        )
        .unwrap()
    }

    #[test]
    fn physical_channel_limits() {
        // req: GEN-060, API-040, TC-007
        assert!(PhysicalChannelConfig::new(1025, false).is_err());
        assert!(PhysicalChannelConfig::new(5, false).is_err());
        assert!(PhysicalChannelConfig::new(7, true).is_err());
        let pc = PhysicalChannelConfig::new(8, true).unwrap();
        assert_eq!((pc.max_frame_len(), pc.fecf()), (8, true));
        assert_eq!(
            PhysicalChannelConfig::new(6, false)
                .unwrap()
                .max_frame_len(),
            6
        );
        let e = PhysicalChannelConfig::new(1025, true).unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::InvalidConfig, Some(field::MAX_FRAME_LEN))
        );
    }

    #[test]
    fn vc_and_map_parameters() {
        // req: GEN-060, API-040, API-041, TC-004
        for len in [5, 1025] {
            let e = VcConfig::new(scid(1), vcid(1), len, SegmentHeaderUse::Present).unwrap_err();
            assert_eq!(e.field(), Some(field::MAX_FRAME_LEN));
        }
        let vc = plain(7);
        assert_eq!(
            (vc.spacecraft_id(), vc.vc_id(), vc.max_frame_len()),
            (scid(0x155), vcid(7), 1024)
        );
        assert_eq!(
            vc.segment_header(),
            SegmentHeaderUse::Absent(SecurityLayout::NONE)
        );
        assert!(!vc.has_segment_header() && segmented(2).has_segment_header());
        assert_eq!(segmented(2).security(), SecurityLayout::NONE);
        let m = map(2, 9, 3, 1);
        assert_eq!(
            (m.spacecraft_id(), m.vc_id(), m.map_id()),
            (scid(0x155), vcid(2), mapid(9))
        );
        assert_eq!(m.security(), SecurityLayout::new(3, 1).unwrap());
    }

    #[test]
    fn table_lookups() {
        // req: GEN-060, TC-021, TC-022
        let t = standard(true);
        assert!(t.physical().fecf() && t.physical_channel().max_frame_len() == 1024);
        assert_eq!(t.vcs().count(), 3);
        assert_eq!(t.maps().count(), 3);
        assert!(t.has_spacecraft(scid(0x155)) && !t.has_spacecraft(scid(0x156)));
        assert_eq!(t.vc(scid(0x155), vcid(2)), Some(segmented(2)));
        assert_eq!(t.vc(scid(0x155), vcid(4)), None);
        assert_eq!(t.vc(scid(0x156), vcid(1)), None);
        assert_eq!(t.map(scid(0x155), vcid(2), mapid(9)), Some(map(2, 9, 3, 1)));
        assert_eq!(t.map(scid(0x155), vcid(2), mapid(1)), None);
        assert_eq!(t.map(scid(0x155), vcid(1), mapid(0)), None);
        assert_eq!(t.map(scid(0x156), vcid(2), mapid(0)), None);
    }

    #[test]
    fn a_table_without_a_virtual_channel_is_rejected() {
        // req: TC-024, GEN-060, API-040
        let pc = PhysicalChannelConfig::new(1024, true).unwrap();
        let e = VcTable::<2, 2>::new(pc, &[], &[]).unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::InvalidConfig, Some(field::VC_ID))
        );
        // A MAP does not make up for it: the VC is what is missing.
        let e = VcTable::<2, 2>::new(pc, &[], &[map(1, 0, 0, 0)]).unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::InvalidConfig, Some(field::VC_ID))
        );
    }

    #[test]
    fn table_checks() {
        // req: GEN-060, GEN-011, API-040, API-070, CMN-041
        let pc = PhysicalChannelConfig::new(1024, true).unwrap();
        let kinds = |r: Result<VcTable<2, 2>, aldis_core::Error>| {
            let e = r.unwrap_err();
            (e.kind(), e.field())
        };
        assert_eq!(
            kinds(VcTable::new(pc, &[plain(1), plain(1)], &[])),
            (ErrorKind::InvalidConfig, Some(field::VC_ID))
        );
        assert_eq!(
            kinds(VcTable::new(pc, &[plain(1), plain(2), plain(3)], &[])),
            (ErrorKind::CapacityExceeded, Some(field::VC_ID))
        );
        assert_eq!(
            kinds(VcTable::new(pc, &[plain(1)], &[map(1, 0, 0, 0)])),
            (ErrorKind::InvalidConfig, Some(field::MAP_ID))
        );
        assert_eq!(
            kinds(VcTable::new(pc, &[segmented(2)], &[map(3, 0, 0, 0)])),
            (ErrorKind::InvalidConfig, Some(field::MAP_ID))
        );
        assert_eq!(
            kinds(VcTable::new(
                pc,
                &[segmented(2)],
                &[map(2, 0, 0, 0), map(2, 0, 1, 0)]
            )),
            (ErrorKind::InvalidConfig, Some(field::MAP_ID))
        );
        assert_eq!(
            kinds(VcTable::new(
                pc,
                &[segmented(2)],
                &[map(2, 0, 0, 0), map(2, 1, 0, 0), map(2, 2, 0, 0)]
            )),
            (ErrorKind::CapacityExceeded, Some(field::MAP_ID))
        );
        let tight = VcConfig::new(
            scid(1),
            vcid(1),
            20,
            SegmentHeaderUse::Absent(SecurityLayout::new(8, 5).unwrap()),
        )
        .unwrap();
        assert_eq!(
            kinds(VcTable::new(pc, &[tight], &[])),
            (ErrorKind::InvalidConfig, Some(field::FRAME_DATA_FIELD))
        );
        let small = VcConfig::new(scid(1), vcid(2), 16, SegmentHeaderUse::Present).unwrap();
        let m = MapConfig::new(
            scid(1),
            vcid(2),
            mapid(1),
            SecurityLayout::new(6, 2).unwrap(),
        );
        assert_eq!(
            kinds(VcTable::new(pc, &[small], &[m])),
            (ErrorKind::InvalidConfig, Some(field::FRAME_DATA_FIELD))
        );
        let full = VcTable::<2, 2>::new(pc, &[plain(1), segmented(2)], &[map(2, 0, 0, 0)]);
        assert_eq!(full.map(|t| t.maps().count()), Ok(1));
        let both = VcTable::<1, 0>::new(
            PhysicalChannelConfig::new(8, true).unwrap(),
            &[plain(1)],
            &[],
        );
        assert!(both.is_ok());
    }
}

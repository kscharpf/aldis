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
//! [`ChannelLookup`]; [`VcTable`] is its fixed-capacity implementation (M2 plan D14). Parameters
//! that only the engines use (data field content, blocking, segmentation, repetitions, CLCW
//! reporting rate, the multiplexing schemes) are not kept here.

use aldis_core::PhysicalChannel;
use aldis_core::layout::{FECF_LEN, LayoutSpec};
use aldis_core::lookup::{ChannelLookup, MapLookup, PhysicalLookup};
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
    pub const fn new(max_frame_len: usize, fecf: bool) -> Result<Self> {
        let shortest = MIN_FRAME_LEN.saturating_add(if fecf { FECF_LEN } else { 0 });
        if max_frame_len < shortest || max_frame_len > MAX_FRAME_LEN {
            return Err(bad_max_len());
        }
        Ok(Self {
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

impl PhysicalChannel for PhysicalChannelConfig {
    fn frame_len(&self) -> usize {
        self.max_frame_len
    }

    fn fecf(&self) -> bool {
        self.fecf
    }

    fn insert_zone_len(&self) -> usize {
        0
    }

    fn variable_length(&self) -> bool {
        true
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
    ) -> Result<Self> {
        if max_frame_len < MIN_FRAME_LEN || max_frame_len > MAX_FRAME_LEN {
            return Err(bad_max_len());
        }
        Ok(Self {
            spacecraft_id,
            vc_id,
            max_frame_len,
            segment_header,
        })
    }

    /// The stand-in for a Virtual Channel that is not configured, which a decode reads as the
    /// Physical Channel allows: frames up to its length, no Segment Header, no SDLS fields
    /// (RS-023; the Frame Validation Checks reject such a frame, TC-022).
    pub(crate) const fn unconfigured(
        spacecraft_id: TcSpacecraftId,
        vc_id: TcVcId,
        physical: &PhysicalChannelConfig,
    ) -> Self {
        Self {
            spacecraft_id,
            vc_id,
            max_frame_len: physical.max_frame_len,
            segment_header: SegmentHeaderUse::Absent(SecurityLayout::NONE),
        }
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
    ) -> Self {
        Self {
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

/// The longest frame of a Virtual Channel or MAP: `limit` octets (at most the Physical
/// Channel's) with a secondary slot of `secondary_len` and the SDLS lengths of `security`
/// (CMN-041).
const fn longest_frame(
    physical: &PhysicalChannelConfig,
    limit: usize,
    secondary_len: usize,
    security: SecurityLayout,
) -> LayoutSpec {
    LayoutSpec {
        frame_len: if limit < physical.max_frame_len {
            limit
        } else {
            physical.max_frame_len
        },
        primary_header_len: PRIMARY_HEADER_LEN,
        secondary_len,
        security,
        ocf: false,
        fecf: physical.fecf,
    }
}

/// `InvalidConfig` or `CapacityExceeded` naming `parameter` (232.0 Tables 5-3, 5-4).
const fn table_error(kind: ErrorKind, parameter: aldis_core::FieldId) -> Error {
    Error::new(kind).with_field(parameter)
}

/// The channels of one Physical Channel, with room for `V` Virtual Channels and `M` MAPs
/// (232.0 §5; GEN-011, GEN-060, M2 plan D14).
///
/// Built only by [`new`](Self::new), so the entries are unique and each MAP belongs to a
/// Virtual Channel with Segment Headers (RS-020).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VcTable<const V: usize, const M: usize> {
    physical: PhysicalChannelConfig,
    vcs: [Option<VcConfig>; V],
    maps: [Option<MapConfig>; M],
}

impl<const V: usize, const M: usize> VcTable<V, M> {
    /// The channels of `physical` (232.0 §5; GEN-060, API-040).
    ///
    /// A `const fn`: a table of constant parameters is checked when the program is built.
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
    pub const fn new(
        physical: PhysicalChannelConfig,
        vcs: &[VcConfig],
        maps: &[MapConfig],
    ) -> Result<Self> {
        if vcs.is_empty() {
            return Err(table_error(ErrorKind::InvalidConfig, field::VC_ID));
        }
        let mut table = Self {
            physical,
            vcs: [None; V],
            maps: [None; M],
        };
        // First every entry is checked, one at a time in the order given, against those before
        // it; then the checked entries are stored. A `const fn` has no iterator.
        let mut index = 0;
        let mut rest = vcs;
        // bound: each pass drops the first entry of a slice, so at most its length.
        while let [vc, tail @ ..] = rest {
            if given_vc(vcs, index, vc.spacecraft_id, vc.vc_id) {
                return Err(table_error(ErrorKind::InvalidConfig, field::VC_ID));
            }
            let secondary = if vc.has_segment_header() {
                SEGMENT_HEADER_LEN
            } else {
                0
            };
            if let Err(e) =
                longest_frame(&physical, vc.max_frame_len, secondary, vc.security()).check_room(0)
            {
                return Err(e);
            }
            if index >= V {
                return Err(table_error(ErrorKind::CapacityExceeded, field::VC_ID));
            }
            rest = tail;
            index = index.saturating_add(1);
        }
        let mut slots = table.vcs.as_mut_slice();
        let mut rest = vcs;
        // bound: each pass drops the first entry of a slice, so at most its length.
        while let ([vc, tail @ ..], [slot, others @ ..]) = (rest, slots) {
            *slot = Some(*vc);
            rest = tail;
            slots = others;
        }
        let mut index = 0;
        let mut rest = maps;
        // bound: each pass drops the first entry of a slice, so at most its length.
        while let [map, tail @ ..] = rest {
            let Some(vc) = find_vc(&table.vcs, map.spacecraft_id, map.vc_id) else {
                return Err(table_error(ErrorKind::InvalidConfig, field::MAP_ID));
            };
            if !vc.has_segment_header() {
                return Err(table_error(ErrorKind::InvalidConfig, field::MAP_ID));
            }
            if given_map(maps, index, *map) {
                return Err(table_error(ErrorKind::InvalidConfig, field::MAP_ID));
            }
            if let Err(e) = longest_frame(
                &physical,
                vc.max_frame_len,
                SEGMENT_HEADER_LEN,
                map.security,
            )
            .check_room(0)
            {
                return Err(e);
            }
            if index >= M {
                return Err(table_error(ErrorKind::CapacityExceeded, field::MAP_ID));
            }
            rest = tail;
            index = index.saturating_add(1);
        }
        let mut slots = table.maps.as_mut_slice();
        let mut rest = maps;
        // bound: each pass drops the first entry of a slice, so at most its length.
        while let ([map, tail @ ..], [slot, others @ ..]) = (rest, slots) {
            *slot = Some(*map);
            rest = tail;
            slots = others;
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

/// Whether one of the first `before` entries of `vcs` is the Virtual Channel (`spacecraft_id`,
/// `vc_id`).
const fn given_vc(
    vcs: &[VcConfig],
    before: usize,
    spacecraft_id: TcSpacecraftId,
    vc_id: TcVcId,
) -> bool {
    let mut left = before;
    let mut rest = vcs;
    // bound: each pass drops the first entry of a slice, so at most its length.
    while let [vc, tail @ ..] = rest {
        if left == 0 {
            return false;
        }
        if vc.spacecraft_id.get() == spacecraft_id.get() && vc.vc_id.get() == vc_id.get() {
            return true;
        }
        left = left.saturating_sub(1);
        rest = tail;
    }
    false
}

/// Whether one of the first `before` entries of `maps` is the MAP `map` (same Spacecraft ID,
/// Virtual Channel, and MAP ID).
const fn given_map(maps: &[MapConfig], before: usize, map: MapConfig) -> bool {
    let mut left = before;
    let mut rest = maps;
    // bound: each pass drops the first entry of a slice, so at most its length.
    while let [m, tail @ ..] = rest {
        if left == 0 {
            return false;
        }
        if m.spacecraft_id.get() == map.spacecraft_id.get()
            && m.vc_id.get() == map.vc_id.get()
            && m.map_id.get() == map.map_id.get()
        {
            return true;
        }
        left = left.saturating_sub(1);
        rest = tail;
    }
    false
}

/// The Virtual Channel (`spacecraft_id`, `vc_id`) among the entries of a table.
const fn find_vc(
    vcs: &[Option<VcConfig>],
    spacecraft_id: TcSpacecraftId,
    vc_id: TcVcId,
) -> Option<VcConfig> {
    let mut rest = vcs;
    // bound: each pass drops the first entry of a slice, so at most its length.
    while let [entry, tail @ ..] = rest {
        if let Some(vc) = entry
            && vc.spacecraft_id.get() == spacecraft_id.get()
            && vc.vc_id.get() == vc_id.get()
        {
            return Some(*vc);
        }
        rest = tail;
    }
    None
}

/// The MAP (`spacecraft_id`, `vc_id`, `map_id`) among the entries of a table.
const fn find_map(
    maps: &[Option<MapConfig>],
    spacecraft_id: TcSpacecraftId,
    vc_id: TcVcId,
    map_id: TcMapId,
) -> Option<MapConfig> {
    let mut rest = maps;
    // bound: each pass drops the first entry of a slice, so at most its length.
    while let [entry, tail @ ..] = rest {
        if let Some(m) = entry
            && m.spacecraft_id.get() == spacecraft_id.get()
            && m.vc_id.get() == vc_id.get()
            && m.map_id.get() == map_id.get()
        {
            return Some(*m);
        }
        rest = tail;
    }
    None
}

impl<const V: usize, const M: usize> PhysicalLookup for VcTable<V, M> {
    type Physical = PhysicalChannelConfig;

    fn physical_channel(&self) -> PhysicalChannelConfig {
        self.physical
    }
}

impl<const V: usize, const M: usize> ChannelLookup<TcSpacecraftId, TcVcId> for VcTable<V, M> {
    type Vc = VcConfig;

    fn has_spacecraft(&self, spacecraft_id: TcSpacecraftId) -> bool {
        self.vcs().any(|vc| vc.spacecraft_id == spacecraft_id)
    }

    fn vc(&self, spacecraft_id: TcSpacecraftId, vc_id: TcVcId) -> Option<VcConfig> {
        find_vc(&self.vcs, spacecraft_id, vc_id)
    }
}

impl<const V: usize, const M: usize> MapLookup<TcSpacecraftId, TcVcId, TcMapId> for VcTable<V, M> {
    type Map = MapConfig;

    fn map(
        &self,
        spacecraft_id: TcSpacecraftId,
        vc_id: TcVcId,
        map_id: TcMapId,
    ) -> Option<MapConfig> {
        find_map(&self.maps, spacecraft_id, vc_id, map_id)
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
pub(crate) mod tests {
    use super::{MapConfig, PhysicalChannelConfig, SegmentHeaderUse, VcConfig, VcTable};
    use crate::field;
    use crate::header::{TcMapId, TcSpacecraftId, TcVcId};
    use aldis_core::ErrorKind;
    use aldis_core::lookup::{ChannelLookup, MapLookup, PhysicalLookup};
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

    #[test]
    fn an_entry_is_a_duplicate_only_of_the_same_spacecraft_virtual_channel_and_map() {
        // req: GEN-060, API-040, TC-024
        let pc = PhysicalChannelConfig::new(1024, true).unwrap();
        let vc = |sc: u16, n: u8| {
            VcConfig::new(scid(sc), vcid(n), 1024, SegmentHeaderUse::Present).unwrap()
        };
        let map = |sc: u16, v: u8, m: u8| {
            MapConfig::new(scid(sc), vcid(v), mapid(m), SecurityLayout::NONE)
        };
        let kinds =
            |r: Result<VcTable<4, 4>, aldis_core::Error>| r.map_err(|e| (e.kind(), e.field()));
        // The same Virtual Channel number under another Spacecraft ID is another channel, and
        // so is the same MAP number under another Spacecraft ID or Virtual Channel.
        let table = VcTable::<4, 4>::new(
            pc,
            &[vc(1, 2), vc(2, 2), vc(2, 3)],
            &[map(1, 2, 0), map(2, 2, 0), map(2, 3, 0), map(2, 2, 1)],
        )
        .unwrap();
        assert_eq!(table.vcs().count(), 3);
        assert_eq!(table.maps().count(), 4);
        assert_eq!(table.map(scid(2), vcid(3), mapid(0)), Some(map(2, 3, 0)));
        assert_eq!(table.map(scid(2), vcid(2), mapid(1)), Some(map(2, 2, 1)));
        // Only the same triple is a duplicate, wherever it is among the entries.
        assert_eq!(
            kinds(VcTable::new(
                pc,
                &[vc(1, 2), vc(2, 2), vc(2, 3)],
                &[map(1, 2, 0), map(2, 2, 0), map(2, 3, 0), map(2, 2, 0)],
            )),
            Err((ErrorKind::InvalidConfig, Some(field::MAP_ID)))
        );
        assert_eq!(
            kinds(VcTable::new(pc, &[vc(1, 2), vc(2, 2), vc(1, 2)], &[])),
            Err((ErrorKind::InvalidConfig, Some(field::VC_ID)))
        );
    }

    #[test]
    fn the_entries_before_an_index_beyond_the_slice_are_all_of_it() {
        // req: GEN-060, API-040
        let (a, b) = (plain(1), plain(2));
        assert!(super::given_vc(&[a, b], 5, scid(0x155), vcid(2)));
        assert!(!super::given_vc(&[a, b], 5, scid(0x155), vcid(3)));
        assert!(!super::given_vc(&[a, b], 1, scid(0x155), vcid(2)));
        assert!(!super::given_vc(&[], 1, scid(0x155), vcid(1)));
        let (m, n) = (map(2, 0, 0, 0), map(2, 1, 0, 0));
        assert!(super::given_map(&[m, n], 5, n));
        assert!(!super::given_map(&[m], 5, n));
        assert!(!super::given_map(&[m, n], 1, n));
        assert!(!super::given_map(&[], 1, n));
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod physical_channel_tests {
    use aldis_core::PhysicalChannel;

    #[test]
    fn the_physical_set_reports_its_managed_parameters() {
        // req: LNK-001, LNK-078
        let p = super::PhysicalChannelConfig::new(24, true).unwrap();
        assert_eq!(PhysicalChannel::frame_len(&p), 24);
        assert!(PhysicalChannel::fecf(&p));
        assert_eq!(PhysicalChannel::insert_zone_len(&p), 0);
        assert!(PhysicalChannel::variable_length(&p));
    }
}

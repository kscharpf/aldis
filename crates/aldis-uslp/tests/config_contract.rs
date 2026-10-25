//! The USLP configuration types are plain data, built by validated constructors, and cannot
//! change after construction (RS-012, API-041, GEN-060, GEN-061).
//!
//! Private fields and the absence of setters are properties of the types that no runtime test
//! can observe, and a compile-fail test is not available in this workspace. So the tests show
//! what can be shown: the types are `Copy` (or `Clone` for the table) plain data without
//! references (`'static`, `Send`, `Sync`), the validated constructors run in `const` context and
//! fill a `static` (which a linker can place in read-only memory), a change is a new value
//! while the original is untouched, and `config.rs` has no method that takes `&mut self`.
//!
//! `VcParams` is the one type with public fields: it is the record of the parameters that
//! `VcConfig::new` validates, and `VcConfig::params` returns a copy of it, so changing the copy
//! does not change the `VcConfig` (`a_configuration_cannot_change_after_construction`).

use core::fmt::Debug;

use aldis_core::security::SecurityLayout;
use aldis_uslp::{
    MapConfig, OcfUse, PhysicalChannelConfig, SecurityScope, TransferFrameType, TruncatedFrameLen,
    UslpMapId, UslpSpacecraftId, UslpVcId, VcConfig, VcParams, VcTable, VcfCountLen,
};

/// What RS-012 and API-041 ask of a value-like configuration type.
const fn assert_plain_copy<T: Copy + Clone + PartialEq + Debug + Send + Sync + 'static>() {}

/// The same for the table, which holds arrays of configurations and is `Clone` only.
const fn assert_plain_clone<T: Clone + PartialEq + Debug + Send + Sync + 'static>() {}

const SCID: UslpSpacecraftId = match UslpSpacecraftId::new(0xA5A5) {
    Some(id) => id,
    None => panic!("valid"),
};
const VCID: UslpVcId = match UslpVcId::new(1) {
    Some(id) => id,
    None => panic!("valid"),
};
const MAPID: UslpMapId = match UslpMapId::new(2) {
    Some(id) => id,
    None => panic!("valid"),
};

static PC: PhysicalChannelConfig =
    match PhysicalChannelConfig::new(TransferFrameType::Variable, 1024, 0, true) {
        Ok(pc) => pc,
        Err(_) => panic!("valid"),
    };
static PARAMS: VcParams = VcParams {
    ocf: OcfUse::Allowed,
    ..VcParams::new(TransferFrameType::Variable)
};
static VC: VcConfig = match VcConfig::new(SCID, VCID, PARAMS) {
    Ok(vc) => vc,
    Err(_) => panic!("valid"),
};
static MAP: MapConfig = MapConfig::new(SCID, VCID, MAPID, SecurityLayout::NONE);

#[test]
fn configurations_are_plain_data_built_in_const_context() {
    // req: RS-012, API-041, GEN-060
    assert_plain_copy::<PhysicalChannelConfig>();
    assert_plain_copy::<VcConfig>();
    assert_plain_copy::<VcParams>();
    assert_plain_copy::<MapConfig>();
    assert_plain_copy::<TruncatedFrameLen>();
    assert_plain_copy::<(TransferFrameType, OcfUse, SecurityScope)>();
    assert_plain_clone::<VcTable<2, 2>>();
    // The statics above are built by the validated constructors and read back through
    // accessors.
    assert_eq!(
        (
            PC.frame_type(),
            PC.frame_len(),
            PC.insert_zone_len(),
            PC.fecf()
        ),
        (TransferFrameType::Variable, 1024, 0, true)
    );
    assert_eq!((VC.spacecraft_id(), VC.vc_id()), (SCID, VCID));
    assert_eq!(VC.params(), PARAMS);
    assert_eq!(VC.ocf(), OcfUse::Allowed);
    assert_eq!(
        (
            MAP.spacecraft_id(),
            MAP.vc_id(),
            MAP.map_id(),
            MAP.security()
        ),
        (SCID, VCID, MAPID, SecurityLayout::NONE)
    );
    // The constructors validate: a variable-length channel with an Insert Zone is an error.
    assert!(PhysicalChannelConfig::new(TransferFrameType::Variable, 1024, 4, true).is_err());
    // A VC with a truncated frame length is built at run time: `TruncatedFrameLen::new` is not
    // a `const fn`.
    let truncated = VcParams {
        truncated_frame_len: Some(TruncatedFrameLen::new(12).unwrap()),
        ..PARAMS
    };
    assert!(VcConfig::new(SCID, VCID, truncated).is_ok());
    // The table is validated at run time (its checks loop over the entries), so it is not a
    // `const`; it is plain data all the same, and equal tables are equal values.
    let t = VcTable::<1, 1>::new(PC, &[VC], &[MAP]).unwrap();
    assert_eq!(t.clone(), t);
}

#[test]
fn a_configuration_cannot_change_after_construction() {
    // req: GEN-061, RS-012
    let t = VcTable::<1, 1>::new(PC, &[VC], &[MAP]).unwrap();
    let before = t.clone();
    // The table and the VC hand out copies and shared references only: a copy is a value of its
    // own, and a different configuration is a new value built by the validating constructor.
    let mut params = VC.params();
    params.ocf = OcfUse::Absent;
    params.vcf_count_len_sequence = VcfCountLen::new(2).unwrap();
    assert_eq!(VC.params(), PARAMS);
    assert_eq!(VC.ocf(), OcfUse::Allowed);
    let other_vc = VcConfig::new(SCID, VCID, params).unwrap();
    assert_ne!(other_vc, VC);
    let other = PhysicalChannelConfig::new(TransferFrameType::Variable, 512, 0, false).unwrap();
    assert_ne!(t.physical(), other);
    let other_table = VcTable::<1, 1>::new(other, &[other_vc], &[MAP]).unwrap();
    assert_ne!(other_table, t);
    assert_eq!(t.physical(), PC);
    assert_eq!(t.vcs().copied().collect::<Vec<_>>(), [VC]);
    assert_eq!(t.maps().copied().collect::<Vec<_>>(), [MAP]);
    assert_eq!(t, before);
    // No method of the configuration types takes `&mut self` (RS-012: private fields and
    // validated constructors, GEN-061: immutable after construction).
    let source = include_str!("../src/config.rs");
    assert!(!source.contains("&mut self"));
}

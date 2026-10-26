//! The TM configuration types are plain data and cannot change after construction (RS-012,
//! API-041, GEN-060, GEN-061).
//!
//! Private fields and the absence of setters are properties of the types that no runtime test
//! can observe, and a compile-fail test is not available in this workspace. So the tests show
//! what can be shown: the types are plain data without references (`'static`, `Send`, `Sync`),
//! a change is a new value (the builders take `self` by value) while the original is
//! untouched, and `config.rs` has no method that takes `&mut self`.

use core::fmt::Debug;

use aldis_core::ids::{SpacecraftId, VcId};
use aldis_core::security::SecurityLayout;
use aldis_tm::{
    DataFieldContent, FshLen, TmMasterChannelConfig, TmPhysicalChannelConfig,
    TmVirtualChannelConfig,
};

/// What RS-012 and API-041 ask of a configuration type.
const fn assert_plain_clone<T: Clone + PartialEq + Debug + Send + Sync + 'static>() {}

/// The same for a `Copy` configuration type.
const fn assert_plain_copy<T: Copy + Clone + PartialEq + Debug + Send + Sync + 'static>() {}

#[test]
fn configurations_are_plain_data() {
    // req: API-041, RS-012
    assert_plain_copy::<TmPhysicalChannelConfig>();
    assert_plain_copy::<TmVirtualChannelConfig>();
    assert_plain_copy::<DataFieldContent>();
    assert_plain_clone::<TmMasterChannelConfig>();
}

#[test]
fn a_configuration_cannot_change_after_construction() {
    // req: GEN-061, RS-012
    let vcid = VcId::<3>::new(5).unwrap();
    let pc = TmPhysicalChannelConfig::new(64, true).unwrap();
    let scid = SpacecraftId::<10>::new(0x2A5).unwrap();
    let plain = TmVirtualChannelConfig::new(vcid, DataFieldContent::Packets);
    // A builder takes the value and returns another: the original is a different value still.
    let with_ocf = plain.with_ocf();
    let with_fsh = plain.with_fsh(FshLen::new(4).unwrap());
    let with_security = plain.with_security(SecurityLayout::new(4, 8).unwrap());
    assert!(!plain.ocf() && plain.fsh().is_none() && plain.security().is_none());
    assert!(with_ocf.ocf() && with_fsh.fsh().is_some() && !with_security.security().is_none());
    assert_ne!(plain, with_ocf);
    let mc = TmMasterChannelConfig::new(pc, scid, None, false)
        .with_vc(plain)
        .unwrap();
    let before = mc.clone();
    // The master channel hands out shared references and copies only; adding a VC consumes the
    // configuration and builds another, so the clone taken before is a different value.
    let other = mc
        .clone()
        .with_vc(TmVirtualChannelConfig::new(
            VcId::<3>::new(6).unwrap(),
            DataFieldContent::Packets,
        ))
        .unwrap();
    assert_ne!(other, before);
    assert_eq!(mc, before);
    assert_eq!(mc.vcs().count(), 1);
    assert_eq!(*mc.pc(), pc);
    // No method of the configuration types takes `&mut self`.
    let source = include_str!("../src/config.rs");
    assert!(!source.contains("&mut self"));
}

//! The AOS configuration types are plain data and cannot change after construction (RS-012,
//! API-041, GEN-060, GEN-061).
//!
//! Private fields and the absence of setters are properties of the types that no runtime test
//! can observe, and a compile-fail test is not available in this workspace. So the tests show
//! what can be shown: the types are plain data without references (`'static`, `Send`, `Sync`),
//! a change is a new value (the builders take `self` by value) while the original is
//! untouched, and `config.rs` has no method that takes `&mut self`.

use core::fmt::Debug;

use aldis_aos::{
    AosEdition, AosPhysicalChannelConfig, AosVirtualChannelConfig, DataFieldContent, VcTable,
};
use aldis_core::ids::{SpacecraftId, VcId};
use aldis_core::security::SecurityLayout;

/// What RS-012 and API-041 ask of a configuration type.
const fn assert_plain_clone<T: Clone + PartialEq + Debug + Send + Sync + 'static>() {}

/// The same for a `Copy` configuration type.
const fn assert_plain_copy<T: Copy + Clone + PartialEq + Debug + Send + Sync + 'static>() {}

#[test]
fn configurations_are_plain_data() {
    // req: API-041, RS-012
    assert_plain_copy::<AosPhysicalChannelConfig>();
    assert_plain_copy::<AosVirtualChannelConfig>();
    assert_plain_copy::<AosEdition>();
    assert_plain_copy::<DataFieldContent>();
    assert_plain_clone::<VcTable<2>>();
}

#[test]
fn a_configuration_cannot_change_after_construction() {
    // req: GEN-061, RS-012
    let scid = SpacecraftId::<10>::new(0x2A5).unwrap();
    let vcid = VcId::<6>::new(9).unwrap();
    let pc = AosPhysicalChannelConfig::new(64, false, 0, true, AosEdition::Issue5).unwrap();
    let plain = AosVirtualChannelConfig::new(scid, vcid, DataFieldContent::VcaSdu).unwrap();
    // A builder takes the value and returns another: the original is a different value still.
    let with_ocf = plain.with_ocf();
    let with_security = plain.with_security(SecurityLayout::new(4, 8).unwrap());
    assert!(!plain.ocf() && plain.security().is_none());
    assert!(with_ocf.ocf() && !with_security.security().is_none());
    assert_ne!(plain, with_ocf);
    let t = VcTable::<1>::new(pc, &[plain]).unwrap();
    let before = t.clone();
    // The table hands out copies and shared references only; a different configuration is a
    // new table built by the validating constructor.
    let other = VcTable::<1>::new(pc, &[with_ocf]).unwrap();
    assert_ne!(other, t);
    assert_eq!(t, before);
    assert_eq!(t.physical_channel(), pc);
    assert_eq!(t.vcs().copied().collect::<Vec<_>>(), [plain]);
    // No method of the configuration types takes `&mut self`.
    let source = include_str!("../src/config.rs");
    assert!(!source.contains("&mut self"));
}

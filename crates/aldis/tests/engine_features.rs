//! The codec features of the facade forward to the engines of `aldis-link` and `aldis-cop1`:
//! `link`, `cop1`, `tm`, `tc`, and `spp` give the TM and TC receiver channels and `Fop1` and
//! `Farm1` without `full` (the feature forwarding of `crates/aldis/Cargo.toml`; GEN-003 for the unselected standards).
#![cfg(all(
    feature = "link",
    feature = "cop1",
    feature = "tm",
    feature = "tc",
    feature = "spp"
))]

use aldis::cop1::{Farm1, Fop1};
use aldis::link::receiver::{TcChannel, TmChannel};

#[test]
fn engines_are_nameable_through_the_facade() {
    // req: GEN-003
    let names = [
        core::any::type_name::<Fop1<1, 1, 1>>(),
        core::any::type_name::<Farm1>(),
        core::any::type_name::<TmChannel>(),
        core::any::type_name::<TcChannel>(),
    ];
    assert!(names.iter().all(|n| n.starts_with("aldis_")));
}

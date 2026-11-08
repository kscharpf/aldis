//! The codec features of the facade forward to the engines of `aldis-link` and `aldis-cop1`:
//! `link`, `cop1`, `tm`, `tc`, and `spp` give the TM and TC receiver channels and `Fop1` and
//! `Farm1` without `full` (GEN-008). The `use` lines compiling is the test; the `features` check
//! builds it with exactly those features.
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
    // req: GEN-008
    // The `use` lines above compiling is the test; these name the types so they stay used.
    let _ = core::any::type_name::<Fop1<1, 1, 1>>();
    let _ = core::any::type_name::<Farm1>();
    let _ = core::any::type_name::<TmChannel>();
    let _ = core::any::type_name::<TcChannel>();
}

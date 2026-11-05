//! The COP-1 engines are plain data that can move between threads and have no interior
//! mutability (GEN-033, RS-013). A `Cell` or `RefCell` makes a type not `Sync`; a raw pointer
//! makes it neither `Send` nor `Sync`. The crate forbids `unsafe` (GEN-070), so no manual
//! implementation can override the compiler.

use aldis_cop1::{Farm1, Fop1, Fop1Store};

/// `T` can move to another thread, can be shared by reference (so it holds no `Cell`), and owns
/// its data.
const fn plain_engine<T: Send + Sync + Clone + 'static>() {}

#[test]
fn cop1_engines_move_between_threads_and_have_no_interior_mutability() {
    // req: GEN-033, RS-013
    plain_engine::<Farm1>();
    plain_engine::<Fop1<8, 64, 16>>();
    plain_engine::<Fop1Store<8, 64>>();
}

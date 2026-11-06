//! The COP-1 engines can move between threads and can be shared by reference (GEN-033, RS-013). `Send`
//! and `Sync` hold only for a type without a `Cell`, a `RefCell`, or a raw pointer, so the
//! assertions below fail to compile for any of them; they do not exclude a `Mutex` or an atomic
//! field, which are `Sync`. The crate forbids `unsafe` (GEN-070), so no manual `Send` or `Sync`
//! implementation can override the compiler. No check keeps a `Mutex` or an atomic out of the
//! library crates; `no_std` has no `Mutex`, and GEN-030 to GEN-032 forbid global state.

use aldis_cop1::{Farm1, Fop1, Fop1Store};

/// `T` can move to another thread, can be shared by reference (so it holds no `Cell` or
/// `RefCell`), and owns its data.
const fn plain_engine<T: Send + Sync + Clone + 'static>() {}

#[test]
fn cop1_engines_move_between_threads_and_have_no_interior_mutability() {
    // req: GEN-033, RS-013
    plain_engine::<Farm1>();
    plain_engine::<Fop1<8, 64, 16>>();
    plain_engine::<Fop1Store<8, 64>>();
}

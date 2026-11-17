//! A FOP-1 engine built at compile time, in a `static` (GEN-011, GEN-012, VER-011; #254, OI-45).
//!
//! The engine owns its frame store, `Q × F` octets and one more `F`. At the `[fop1]` sizes of
//! `crates/aldis-ffi/capacities.toml`, 16 entries of 1024 octets, that is more than the 4 096
//! octets of stack that a Rust entry point may use (OI-45, option (d); `record:VER-011`). The
//! constructor `Fop1::new` is a `const fn`, with the target constructor `Fop1Target::tc` and the
//! constructors under it, so a `no_std` Rust caller builds the engine from a `const`
//! configuration in a `static` initializer, with no `unsafe`, and the store is zeroed when the
//! program is built, not on the stack at run time. The checks of the configuration run then too:
//! a refused configuration is a compile-time value here.
#![expect(
    clippy::panic,
    reason = "a const initializer fails the build with `panic!`"
)]

use aldis_cop1::{Fop1, Fop1Config, Fop1State, Fop1Target, TimeoutType, fop1_max_actions};
use aldis_core::ids::FrameSeqNum;
use aldis_core::time::Duration;
use aldis_core::{ErrorKind, Result, field};
use aldis_tc::{PhysicalChannelConfig, TcSpacecraftId, TcVcId};
use core::num::NonZeroU8;

/// The `[fop1]` section of `crates/aldis-ffi/capacities.toml`: `sent_queue_frames` and
/// `max_frame_octets`.
const Q: usize = 16;
const F: usize = 1024;
const N: usize = fop1_max_actions(Q);
type Engine = Fop1<Q, F, N>;

/// The engine does not fit the 4 096 octets of stack of a Rust entry point (VER-011, OI-45), so
/// that a `static` is the place for it.
const _: () = assert!(size_of::<Engine>() > 4096);

/// The configuration of one FOP-1 instance, with sliding window `k`, evaluated at compile time.
const fn config(k: u8) -> Fop1Config {
    let Ok(physical) = PhysicalChannelConfig::new(F, true) else {
        panic!("the physical channel is valid")
    };
    let Ok(target) = Fop1Target::tc(
        TcSpacecraftId::from_const::<0x2A5>(),
        TcVcId::from_const::<5>(),
        physical,
    ) else {
        panic!("the target is valid")
    };
    let Some(transmission_limit) = NonZeroU8::new(3) else {
        panic!("3 is not 0")
    };
    Fop1Config {
        target,
        t1_initial: Duration::from_micros(1_000_000),
        transmission_limit,
        timeout_type: TimeoutType::Tt0,
        sliding_window_k: k,
        ad_repetitions: 1,
        bc_repetitions: 1,
    }
}

/// The engine, built by the compiler: its store is zero data of the program, not a frame of the
/// stack.
static ENGINE: Engine = match Fop1::new(config(8)) {
    Ok(engine) => engine,
    Err(_) => panic!("the configuration is valid"),
};

/// The refusal of `new` for a window of 0, a value of the compile-time evaluation.
const REFUSED: Result<Engine> = Fop1::new(config(0));

#[test]
fn the_static_engine_is_the_initial_engine() {
    // req: GEN-011, VER-011
    assert_eq!(ENGINE.state(), Fop1State::Initial);
    assert_eq!(ENGINE.v_s(), FrameSeqNum::new(0));
    assert_eq!(ENGINE.nn_r(), FrameSeqNum::new(0));
    assert_eq!(ENGINE.sliding_window_k(), 8);
    assert_eq!(ENGINE.target().max_frame_len(), F);
}

#[test]
fn the_static_engine_equals_the_engine_built_at_run_time() {
    // req: GEN-011, GEN-012
    // The run-time build is on the stack of the test, which is larger than a flight task's.
    assert_eq!(&ENGINE, &Engine::new(config(8)).unwrap());
}

#[test]
fn a_refused_configuration_is_a_compile_time_value() {
    // req: GEN-011
    let e = REFUSED.unwrap_err();
    assert_eq!(
        (e.kind(), e.field()),
        (ErrorKind::InvalidConfig, Some(field::SLIDING_WINDOW_K))
    );
}

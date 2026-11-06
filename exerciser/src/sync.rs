//! Calls every public `aldis-sync` API (VER-010, M5 WP-3) and runs every `test-vectors/sync/`
//! codec case (`vector`). Inputs pass through `black_box`.
//!
//! The engine scenarios of `test-vectors/sync/scenarios/` are not embedded: `build.rs` reads the
//! codec files of the directory only, as for `cop1/` and `link/`; the host suite runs them.

use core::hint::black_box as bb;

use aldis_core::{Error, ErrorKind};
use aldis_sync::{
    ASM, ASM_LEN, AsmErrors, AsmSearch, MAX_ASM_ERRORS, RandomizerSequence, SearchInput,
    SearchState, SyncConfig, SyncProtocol, attach_asm, max_actions, max_out_len, randomize,
};

use crate::exercise::Tally;
use crate::vectors::{Case, Fail, Map, Out, Params as _, Run, Scratch, Val, copy, fresh, need};

/// An instance for frames of up to 64 octets and chunks of up to 32.
type Engine = AsmSearch<64, 32, { max_actions(32) }>;
/// The instance of `frame_capacity` 64 and 1023 of the configuration cases.
type Small = AsmSearch<64, 1, { max_actions(1) }>;
type Large = AsmSearch<1023, 1, { max_actions(1) }>;

fn protocol(name: &str) -> Result<SyncProtocol, Fail> {
    match name {
        "tm" => Ok(SyncProtocol::Tm),
        "aos" => Ok(SyncProtocol::Aos),
        "uslp" => Ok(SyncProtocol::Uslp),
        // The C ABI input that the Rust type cannot hold (SYNC-002).
        _ => Err(Fail::Api(
            Error::new(ErrorKind::InvalidConfig).with_field(aldis_core::field::SYNC_PROTOCOL),
        )),
    }
}

/// The configuration of a case, checked in the order protocol, frame length, tolerance.
fn config(c: Map) -> Result<SyncConfig, Fail> {
    let p = protocol(c.text("protocol")?)?;
    let len = c.size("frame_len")?;
    SyncConfig::new(p, len, AsmErrors::NONE)?;
    let tolerance = match c.val("asm_errors") {
        Ok(v) => need(v.num().and_then(|n| u32::try_from(n).ok()))?,
        Err(_) => 0,
    };
    let tolerance = AsmErrors::new(tolerance)?;
    Ok(SyncConfig::new(p, len, tolerance)?)
}

fn codecs(t: &mut Tally) {
    t.check(ASM_LEN == ASM.len() && u32::from_be_bytes(ASM) == 0x1ACF_FC1D);
    t.check(MAX_ASM_ERRORS == 15 && AsmErrors::new(bb(15)).is_ok_and(|e| e.get() == 15));
    t.check(AsmErrors::new(bb(16)).is_err_and(|e| e.kind() == ErrorKind::InvalidConfig));
    t.check(SyncProtocol::Tm.max_frame_len() == 2048 && SyncProtocol::Aos.max_frame_len() == 65536);
    t.check(SyncProtocol::Uslp.max_frame_len() == 65536);
    let tolerance = AsmErrors::new(bb(2)).unwrap_or_default();
    let cfg = SyncConfig::new(bb(SyncProtocol::Tm), bb(3), tolerance);
    t.check(cfg.is_ok_and(|c| {
        c.protocol() == SyncProtocol::Tm && c.frame_len() == 3 && c.asm_errors() == tolerance
    }));
    t.check(SyncConfig::new(SyncProtocol::Tm, bb(2049), tolerance).is_err());
    let Ok(cfg) = cfg else {
        t.check(false);
        return;
    };
    let mut frame = bb([1_u8, 2, 3]);
    randomize(bb(RandomizerSequence::Period131071), &mut frame);
    let randomized = frame;
    randomize(RandomizerSequence::Period255, &mut frame);
    randomize(RandomizerSequence::Period255, &mut frame);
    t.check(frame == randomized && frame != [1, 2, 3]);
    randomize(RandomizerSequence::Period131071, &mut frame);
    t.check(frame == [1, 2, 3]);
    t.check(RandomizerSequence::Period131071.period() == 131_071);
    t.check(RandomizerSequence::from_period(bb(255)) == Ok(RandomizerSequence::Period255));
    t.check(RandomizerSequence::from_period(bb(7)).is_err());
    let mut cadu = [0_u8; 7];
    t.check(attach_asm(&cfg, bb(&frame), &mut cadu) == Ok(7) && cadu[..4] == ASM);
    t.check(attach_asm(&cfg, bb(&frame[..2]), &mut cadu).is_err());
    t.check(
        attach_asm(&cfg, bb(&frame), &mut cadu[..6])
            .is_err_and(|e| e.kind() == ErrorKind::BufferTooSmall),
    );
}

fn engine(t: &mut Tally) {
    let tolerance = AsmErrors::new(bb(1)).unwrap_or_default();
    let Ok(cfg) = SyncConfig::new(SyncProtocol::Tm, bb(3), tolerance) else {
        t.check(false);
        return;
    };
    t.check(max_actions(bb(32)) == 7 && max_out_len(bb(3), bb(32)) == 15);
    t.check(Engine::MAX_OUT_LEN == 96);
    t.check(Large::new(cfg).is_ok() && Small::new(cfg).is_ok());
    t.check(
        Small::new(SyncConfig::new(SyncProtocol::Tm, bb(65), tolerance).unwrap_or(cfg)).is_err(),
    );
    let Ok(mut engine) = Engine::new(cfg) else {
        t.check(false);
        return;
    };
    // One bit of junk, the ASM with one bit flipped, a 3-octet frame, and the start of the next.
    let stream = bb([0x0D_u8, 0x67, 0xFE, 0x0E, 0x80 | 0x01, 0x02, 0x01, 0x80]);
    let mut out = [0_u8; Engine::MAX_OUT_LEN];
    t.check(engine.state() == SearchState::Searching && *engine.config() == cfg);
    match engine.step(SearchInput::Chunk(&stream[..3]), &mut out) {
        Ok(actions) => t.check(actions.is_empty()),
        Err(_) => t.check(false),
    }
    t.check(engine.state() == SearchState::Searching);
    match engine.step(SearchInput::Chunk(&stream[3..]), &mut out) {
        Ok(actions) => {
            let found = actions.get(0).is_some_and(|a| {
                a.unit().slice(&out).is_some() && a.errors().get() <= 1 && a.skipped_bits() <= 8
            });
            t.check(found && actions.len() == 1);
        }
        Err(_) => t.check(false),
    }
    let s = engine.status();
    t.check(s.units_returned() == 1 && s.skipped_bits_total() <= 8);
    t.check(s.state() == SearchState::Searching && s.partial_octets() == 0);
    t.check(
        engine
            .step(SearchInput::Chunk(&[0; 33]), &mut out)
            .is_err_and(|e| e.kind() == ErrorKind::OutOfRange),
    );
    t.check(
        engine
            .step(SearchInput::Reset { config: bb(None) }, &mut out)
            .is_ok_and(|a| a.is_empty()),
    );
    t.check(
        engine
            .step(SearchInput::Reset { config: Some(cfg) }, &mut out)
            .is_ok(),
    );
}

pub fn run(t: &mut Tally) {
    codecs(t);
    engine(t);
}

/// One `test-vectors/sync/` codec case, as `crates/aldis-sync/tests/codec_vectors.rs` runs it.
pub fn vector(case: &Case, out: &mut Out, scratch: &mut Scratch) -> Run {
    let c = case.config;
    match case.op {
        "attach" => {
            let cfg = config(c)?;
            let area = match case.params.val("out_len") {
                Ok(v) => need(v.num())
                    .and_then(|n| usize::try_from(n).map_err(|_| Fail::NotRepresentable))?,
                Err(_) => need(case.input.len().checked_add(ASM_LEN))?,
            };
            let buf = bb(fresh(&mut scratch.a, area)?);
            let n = attach_asm(&cfg, bb(case.input), buf)?;
            out.bytes("cadu_hex", need(buf.get(..n))?);
        }
        "randomize" => {
            let sequence = RandomizerSequence::from_period(c.num("randomizer_sequence")?)?;
            let buf = fresh(&mut scratch.a, case.input.len())?;
            if !copy(buf, case.input) {
                return Err(Fail::Vector);
            }
            randomize(sequence, bb(&mut *buf));
            out.bytes("output_hex", buf);
        }
        "config" => {
            let cfg = config(c)?;
            match c.val("frame_capacity").ok().and_then(Val::num) {
                None => {}
                Some(64) => {
                    bb(Small::new(cfg)?);
                }
                Some(1023) => {
                    bb(Large::new(cfg)?);
                }
                Some(_) => return Err(Fail::Vector),
            }
        }
        _ => return Err(Fail::Vector),
    }
    Ok(())
}

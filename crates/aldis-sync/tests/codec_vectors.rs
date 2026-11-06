//! Runs the codec vectors of `test-vectors/sync/` (CADU attach, the two sequences, and the
//! configuration) against the Rust API (SYNC-001…SYNC-005, SYNC-016, SYNC-020, TV-012,
//! TV-023). Every output is compared exactly.
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test-only harness: malformed vectors must fail loudly (RS-018)"
)]

use aldis_core::Error;
use aldis_sync::{
    AsmErrors, AsmSearch, RandomizerSequence, SyncConfig, SyncProtocol, attach_asm, max_actions,
    randomize,
};
use aldis_testkit::hex;
use aldis_testkit::model::CodecCase;
use aldis_testkit::runner::{Outcome, run_codec_file};
use serde_json::{Map, Value, json};

fn err(e: &Error) -> Outcome {
    Outcome::Err {
        kind: e.kind().name().to_owned(),
        field: e.field().map(aldis_core::FieldId::code),
    }
}

fn ok(pairs: &Value) -> Outcome {
    Outcome::Ok(pairs.as_object().cloned().unwrap_or_default())
}

fn num(c: &Map<String, Value>, key: &str) -> Option<u64> {
    c.get(key).map(|v| v.as_u64().unwrap())
}

fn protocol(c: &Map<String, Value>) -> Result<SyncProtocol, Error> {
    match c["protocol"].as_str().unwrap() {
        "tm" => Ok(SyncProtocol::Tm),
        "aos" => Ok(SyncProtocol::Aos),
        "uslp" => Ok(SyncProtocol::Uslp),
        // The C ABI input that the Rust type cannot hold (SYNC-002).
        _ => Err(Error::new(aldis_core::ErrorKind::InvalidConfig)
            .with_field(aldis_core::field::SYNC_PROTOCOL)),
    }
}

/// The configuration of a case, checked in the order of the vectors: protocol, frame length,
/// tolerance (OI-83 c).
fn config(c: &Map<String, Value>) -> Result<SyncConfig, Error> {
    let p = protocol(c)?;
    let len = usize::try_from(num(c, "frame_len").unwrap()).unwrap();
    SyncConfig::new(p, len, AsmErrors::NONE)?;
    let tolerance = AsmErrors::new(u32::try_from(num(c, "asm_errors").unwrap_or(0)).unwrap())?;
    SyncConfig::new(p, len, tolerance)
}

/// The capacity of an instance of `FRAME` octets (SYNC-002).
fn capacity<const FRAME: usize>(config: SyncConfig) -> Result<(), Error> {
    AsmSearch::<FRAME, 1, { max_actions(1) }>::new(config).map(|_| ())
}

fn attach(case: &CodecCase) -> Outcome {
    let c = case.config.as_ref().unwrap();
    let config = match config(c) {
        Ok(config) => config,
        Err(e) => return err(&e),
    };
    let frame = hex::decode(case.input_hex.as_deref().unwrap()).unwrap();
    let area = case
        .params
        .as_ref()
        .and_then(|p| num(p, "out_len"))
        .map_or(frame.len().saturating_add(4), |n| {
            usize::try_from(n).unwrap()
        });
    let mut out = vec![0xEE_u8; area];
    match attach_asm(&config, &frame, &mut out) {
        Ok(n) => ok(&json!({ "cadu_hex": hex::encode(out.get(..n).unwrap()) })),
        Err(e) => {
            assert!(
                out.iter().all(|&b| b == 0xEE),
                "nothing is written on error"
            );
            if let Some(needed) = case.expect.values.get("needed") {
                let needed = usize::try_from(needed.as_u64().unwrap()).unwrap();
                assert_eq!(e.detail(), aldis_core::ErrorDetail::Needed(needed));
            }
            err(&e)
        }
    }
}

fn randomizer(case: &CodecCase) -> Outcome {
    let c = case.config.as_ref().unwrap();
    let period = u32::try_from(num(c, "randomizer_sequence").unwrap()).unwrap();
    let sequence = match RandomizerSequence::from_period(period) {
        Ok(s) => s,
        Err(e) => return err(&e),
    };
    let mut buf = hex::decode(case.input_hex.as_deref().unwrap()).unwrap();
    randomize(sequence, &mut buf);
    ok(&json!({ "output_hex": hex::encode(&buf) }))
}

fn configuration(case: &CodecCase) -> Outcome {
    let c = case.config.as_ref().unwrap();
    let config = match config(c) {
        Ok(config) => config,
        Err(e) => return err(&e),
    };
    let checked = match num(c, "frame_capacity") {
        None => Ok(()),
        Some(64) => capacity::<64>(config),
        Some(1023) => capacity::<1023>(config),
        Some(other) => panic!("no instance of capacity {other}"),
    };
    match checked {
        Ok(()) => ok(&json!({})),
        Err(e) => err(&e),
    }
}

fn handler(case: &CodecCase) -> Outcome {
    match case.operation.as_str() {
        "attach" => attach(case),
        "config" => configuration(case),
        "randomize" => randomizer(case),
        _ => Outcome::Unsupported,
    }
}

fn run(name: &str) {
    let report = run_codec_file(name, handler);
    report.assert_passed();
}

#[test]
fn cadu_tm() {
    // req: SYNC-001, SYNC-002, SYNC-003, TV-012
    run("sync/cadu_tm.json");
}

#[test]
fn cadu_aos() {
    // req: SYNC-001, SYNC-002, SYNC-003, TV-012
    run("sync/cadu_aos.json");
}

#[test]
fn cadu_uslp() {
    // req: SYNC-001, SYNC-002, SYNC-003, TV-012
    run("sync/cadu_uslp.json");
}

#[test]
fn cadu_errors() {
    // req: SYNC-003, TV-012
    run("sync/cadu_errors.json");
}

#[test]
fn randomizer_131071() {
    // req: SYNC-004, SYNC-005, TV-012
    run("sync/randomizer_131071.json");
}

#[test]
fn randomizer_255() {
    // req: SYNC-004, SYNC-020, TV-012
    run("sync/randomizer_255.json");
}

#[test]
fn configuration_and_choices() {
    // req: SYNC-002, SYNC-016, SYNC-020, TV-012
    run("sync/config.json");
}

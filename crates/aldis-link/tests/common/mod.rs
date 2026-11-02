//! Helpers shared by the vector runners and the property tests of the packet engines: the
//! protocols' parameters from a scenario's `config`. Included by `#[path]` into each test.
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test-only harness: malformed vectors must fail loudly (RS-018)"
)]
#![allow(dead_code, reason = "each test uses a part of the helpers")]

use aldis_aos::AosEdition;
use aldis_core::ids::MapId;
use aldis_link::common::Qos;
use aldis_link::protocol::{Aos, AosParams, PacketProtocol, Tm, TmParams, Uslp, UslpParams};
use aldis_uslp::Upid;
use serde_json::{Map, Value};

/// A protocol of the packet engines whose parameters a scenario's `config` gives.
pub(crate) trait Proto: PacketProtocol {
    /// The name of the protocol in a scenario.
    const NAME: &'static str;
    /// The protocol's parameters from `config`.
    fn params(config: &Map<String, Value>) -> Self::Params;
}

impl Proto for Tm {
    const NAME: &'static str = "tm";
    fn params(_: &Map<String, Value>) -> TmParams {
        TmParams
    }
}

impl Proto for Aos {
    const NAME: &'static str = "aos";
    fn params(config: &Map<String, Value>) -> AosParams {
        AosParams {
            edition: match config.get("aos_issue").map(num) {
                Some(4) => AosEdition::Issue4Compatible,
                _ => AosEdition::Issue5,
            },
        }
    }
}

impl Proto for Uslp {
    const NAME: &'static str = "uslp";
    fn params(config: &Map<String, Value>) -> UslpParams {
        UslpParams::new(
            Upid::from_bits(u8::try_from(config.get("upid").map_or(0, num)).unwrap()).unwrap(),
            MapId::<4>::new(u8::try_from(config.get("map_id").map_or(0, num)).unwrap()).unwrap(),
            match config.get("qos").and_then(Value::as_str) {
                None | Some("sequence_controlled") => Qos::SequenceControlled,
                Some("expedited") => Qos::Expedited,
                Some(other) => panic!("unknown qos {other}"),
            },
        )
    }
}

/// A JSON number.
pub(crate) fn num(v: &Value) -> u64 {
    v.as_u64().unwrap_or_else(|| panic!("{v} is not a number"))
}

/// The packet version numbers of `config`.
pub(crate) fn pvns(config: &Map<String, Value>) -> Vec<u8> {
    config["valid_pvns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| u8::try_from(num(v)).unwrap())
        .collect()
}

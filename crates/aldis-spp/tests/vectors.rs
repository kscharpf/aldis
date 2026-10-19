//! Runs the shared conformance vectors in `test-vectors/spp/` against `aldis-spp` (VER-021,
//! TV-012). Every case's outputs must match exactly; unknown operations fail.
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test-only harness crate: malformed vectors must fail loudly (RS-018)"
)]

use core::num::NonZeroUsize;

use aldis_core::counter::Continuity;
use aldis_core::ids::{Apid, SeqCount};
use aldis_core::{DecodeMode, Error};
use aldis_spp::{
    Discard, IdlePacket, OctetStringEncoder, PacketFields, PacketType, Reassembler,
    SecondaryHeaderLayout, Segmenter, SequenceCounter, SequenceFlags, SequenceTracker, SpacePacket,
    SpacePacketWriter, SppConfig, Status, TimeHeaderLayout, Tracked, encode,
};
use aldis_testkit::{CodecCase, Mode, Outcome, hex, run_codec_file};
use aldis_time::{
    CcsFormat, CcsVariation, CdsDayLen, CdsEpoch, CdsFormat, CdsSubMs, CucEpoch, CucFormat,
    TimeFormat,
};
use serde_json::{Map, Value, json};

fn value<'a>(map: Option<&'a Map<String, Value>>, case: &CodecCase, name: &str) -> &'a Value {
    map.and_then(|m| m.get(name))
        .unwrap_or_else(|| panic!("{}: missing {name}", case.id))
}

fn param<'a>(case: &'a CodecCase, name: &str) -> &'a Value {
    value(case.params.as_ref(), case, name)
}

fn u64_of(v: &Value) -> u64 {
    v.as_u64().unwrap()
}

fn usize_param(case: &CodecCase, name: &str) -> usize {
    usize::try_from(u64_of(param(case, name))).unwrap()
}

fn u16_of(v: &Value) -> u16 {
    u16::try_from(u64_of(v)).unwrap()
}

fn hex_param(case: &CodecCase, name: &str) -> Vec<u8> {
    hex::decode(param(case, name).as_str().unwrap()).unwrap()
}

fn input(case: &CodecCase) -> Vec<u8> {
    hex::decode(case.input_hex.as_deref().unwrap_or("")).unwrap()
}

fn mode(case: &CodecCase) -> DecodeMode {
    match case.mode.unwrap_or_default() {
        Mode::Strict => DecodeMode::Strict,
        Mode::Lenient => DecodeMode::Lenient,
    }
}

fn config(case: &CodecCase) -> Result<SppConfig, Error> {
    case.config.as_ref().map_or(Ok(SppConfig::DEFAULT), |c| {
        SppConfig::new(usize::try_from(u64_of(&c["max_packet_len"])).unwrap())
    })
}

fn packet_type(v: &Value) -> PacketType {
    if u64_of(v) == 0 {
        PacketType::Telemetry
    } else {
        PacketType::Telecommand
    }
}

fn apid(case: &CodecCase) -> Apid {
    Apid::new(u16_of(param(case, "apid"))).unwrap()
}

fn count(v: &Value) -> SeqCount {
    SeqCount::new(u16_of(v)).unwrap()
}

/// The sender-chosen fields of an encode case, parsed from raw values (RS-020).
fn fields(case: &CodecCase) -> PacketFields {
    PacketFields {
        packet_type: packet_type(param(case, "packet_type")),
        apid: apid(case),
        sequence_flags: SequenceFlags::from_bits(
            u8::try_from(u64_of(param(case, "sequence_flags"))).unwrap(),
        ),
        sequence_count: count(param(case, "sequence_count")),
    }
}

fn anomalies(p: &SpacePacket<'_>) -> Vec<String> {
    p.anomalies().iter().map(|a| format!("{a:?}")).collect()
}

fn decoded(p: &SpacePacket<'_>) -> Value {
    json!({
        "packet_type": match p.packet_type() {
            PacketType::Telemetry => 0,
            PacketType::Telecommand => 1,
        },
        "secondary_header_flag": u8::from(p.has_secondary_header()),
        "apid": p.apid().get(),
        "idle": p.is_idle(),
        "sequence_flags": p.sequence_flags().bits(),
        "sequence_count": p.sequence_count().get(),
        "packet_name": p.packet_name().map(SeqCount::get),
        "data_len": p.header().data_len(),
        "packet_len": p.header().packet_len(),
        "data_hex": hex::encode(p.data_field()),
        "consumed": p.len(),
        "anomalies": anomalies(p),
    })
}

fn continuity(t: Tracked) -> Value {
    match t {
        Tracked::First => json!({ "continuity": "First" }),
        Tracked::Next(Continuity::InSequence) => json!({ "continuity": "InSequence" }),
        Tracked::Next(Continuity::Repeat) => json!({ "continuity": "Repeat" }),
        Tracked::Next(Continuity::Backwards) => json!({ "continuity": "Backwards" }),
        Tracked::Next(Continuity::Gap { missing }) => {
            json!({ "continuity": "Gap", "missing": missing })
        }
    }
}

macro_rules! track_with {
    ($n:literal, $events:expr) => {{
        let mut tracker = SequenceTracker::<$n>::new();
        track_events(&mut tracker, $events)
    }};
}

fn track_events<const N: usize>(tracker: &mut SequenceTracker<N>, events: &[Value]) -> Value {
    let results: Vec<Value> = events
        .iter()
        .map(|e| {
            let apid = Apid::new(u16_of(&e["apid"])).unwrap();
            match e["op"].as_str().unwrap() {
                "track" => match tracker.track(apid, count(&e["count"])) {
                    Ok(t) => continuity(t),
                    Err(err) => json!({ "error": err.kind().name() }),
                },
                "forget" => json!({ "forgot": tracker.forget(apid) }),
                op => panic!("unknown tracker op {op}"),
            }
        })
        .collect();
    json!({ "results": results })
}

fn encode_case(case: &CodecCase) -> Result<Value, Error> {
    let cfg = config(case)?;
    let sec = hex_param(case, "secondary_header_hex");
    let user = hex_param(case, "user_data_hex");
    let packet = cfg.packet(fields(case), &sec, &user)?;
    let mut out = vec![0_u8; packet.packet_len()];
    let n = encode(&packet, &mut out)?;
    // The two-phase writer must produce the same octets (API-020).
    let mut two = vec![0_u8; n];
    let mut w = SpacePacketWriter::begin(&mut two, &cfg, fields(case), sec.len(), user.len())?;
    w.secondary_header_mut().copy_from_slice(&sec);
    w.user_data_mut().copy_from_slice(&user);
    assert_eq!(w.finish(user.len()), Ok(n), "{}", case.id);
    assert_eq!(two, out[..n], "{}: two-phase writer differs", case.id);
    Ok(json!({ "output_hex": hex::encode(&out[..n]) }))
}

fn decode_stream(case: &CodecCase) -> Value {
    let bytes = input(case);
    let (mut at, mut packets, mut stop) = (0, Vec::new(), Value::Null);
    while at < bytes.len() {
        match SpacePacket::decode(&bytes[at..], mode(case)) {
            Ok(p) => {
                packets.push(json!({
                    "apid": p.apid().get(),
                    "sequence_count": p.sequence_count().get(),
                    "consumed": p.len(),
                    "anomalies": anomalies(&p),
                }));
                at += p.len();
            }
            Err(e) => {
                stop = json!(e.kind().name());
                break;
            }
        }
    }
    json!({ "packets": packets, "stop_error": stop, "offset": at })
}

fn segment_case(case: &CodecCase) -> Result<Value, Error> {
    let sdu = hex_param(case, "sdu_hex");
    let sec = hex_param(case, "secondary_header_hex");
    let ptype = packet_type(param(case, "packet_type"));
    let mut seg = Segmenter::new(
        &config(case)?,
        ptype,
        apid(case),
        &sdu,
        usize_param(case, "max_user_len"),
    )?;
    let mut counter = SequenceCounter::new(count(param(case, "sequence_count")));
    let mut packets = Vec::new();
    let mut out = vec![0_u8; 70_000];
    while let Some(n) = seg.next_packet(&mut counter, &sec, &mut out)? {
        packets.push(json!(hex::encode(&out[..n])));
        assert_eq!(seg.remaining() == 0, seg.is_done(), "{}", case.id);
    }
    assert!(seg.is_done());
    Ok(json!({ "packets_hex": packets, "next_count": counter.peek().get() }))
}

fn discard_name(d: Discard) -> &'static str {
    match d {
        Discard::Gap => "Gap",
        Discard::Restarted => "Restarted",
        Discard::Interrupted => "Interrupted",
        Discard::Orphan => "Orphan",
        Discard::Overflow => "Overflow",
        _ => panic!("unknown discard reason"),
    }
}

fn reassemble_case(case: &CodecCase) -> Value {
    let mut buf = vec![0_u8; usize_param(case, "buffer_len")];
    let mut ra = Reassembler::new(apid(case), &mut buf);
    let results: Vec<Value> = param(case, "packets_hex")
        .as_array()
        .unwrap()
        .iter()
        .map(|h| {
            let bytes = hex::decode(h.as_str().unwrap()).unwrap();
            let p = SpacePacket::decode(&bytes, DecodeMode::Strict).unwrap();
            match ra.push(&p.header(), p.data_field()) {
                Err(e) => json!({ "error": e.kind().name() }),
                Ok(o) => {
                    let discarded = o.discarded.map(discard_name);
                    match o.status {
                        Status::InProgress => json!({ "discarded": discarded, "status": "InProgress" }),
                        Status::Complete(n) => {
                            assert_eq!(ra.sdu().len(), n);
                            json!({ "discarded": discarded, "status": "Complete", "sdu_hex": hex::encode(ra.sdu()) })
                        }
                        Status::Dropped(why) => {
                            assert_eq!(ra.sdu(), <&[u8]>::default());
                            json!({ "discarded": discarded, "status": "Dropped", "reason": discard_name(why) })
                        }
                    }
                }
            }
        })
        .collect();
    json!({ "results": results })
}

fn time_format(f: &Value) -> TimeFormat {
    let u8_of = |name: &str| u8::try_from(u64_of(&f[name])).unwrap();
    match f["code"].as_str().unwrap() {
        "cuc" => TimeFormat::Cuc(
            CucFormat::new(
                if f["level2"].as_bool().unwrap() {
                    CucEpoch::Agency
                } else {
                    CucEpoch::Tai1958
                },
                u8_of("coarse"),
                u8_of("fine"),
            )
            .unwrap(),
        ),
        "cds" => TimeFormat::Cds(CdsFormat::new(
            if f["agency_epoch"].as_bool().unwrap() {
                CdsEpoch::Agency
            } else {
                CdsEpoch::Ccsds1958
            },
            if f["day24"].as_bool().unwrap() {
                CdsDayLen::Bits24
            } else {
                CdsDayLen::Bits16
            },
            match u8_of("subms") {
                0 => CdsSubMs::None,
                1 => CdsSubMs::Micros,
                _ => CdsSubMs::Picos,
            },
        )),
        "ccs" => TimeFormat::Ccs(
            CcsFormat::new(CcsVariation::MonthDay, u8_of("subsecond_octets")).unwrap(),
        ),
        code => panic!("time code {code}"),
    }
}

fn split_time_case(case: &CodecCase) -> Result<Value, Error> {
    let l = param(case, "layout");
    let layout = TimeHeaderLayout::new(
        time_format(&l["format"]),
        l["pfield"].as_bool().unwrap(),
        usize::try_from(u64_of(&l["ancillary_len"])).unwrap(),
    )?;
    let bytes = input(case);
    let s = SpacePacket::decode(&bytes, mode(case))?.split(&SecondaryHeaderLayout::Time(layout))?;
    let t = s.time_code().unwrap();
    let time_code = &s.secondary_header()[..t.len()];
    // The decoded code re-encodes to the received octets (API-020).
    let mut out = [0_u8; 32];
    let n = t.code().encode(t.pfield().is_some(), &mut out).unwrap();
    assert_eq!(
        &out[..n],
        time_code,
        "{}: time code re-encodes differently",
        case.id
    );
    Ok(json!({
        "secondary_header_hex": hex::encode(s.secondary_header()),
        "user_data_hex": hex::encode(s.user_data()),
        "time_code_hex": hex::encode(time_code),
        "ancillary_hex": hex::encode(s.ancillary_data()),
    }))
}

fn run(case: &CodecCase) -> Result<Value, Error> {
    match case.operation.as_str() {
        "decode" => {
            let bytes = input(case);
            SpacePacket::decode(&bytes, mode(case)).map(|p| decoded(&p))
        }
        "peek_length" => SpacePacket::peek_length(&input(case)).map(|n| json!({ "len": n })),
        "encode" => encode_case(case),
        "idle" => {
            let pattern = hex_param(case, "pattern_hex");
            let idle = IdlePacket::new(&config(case)?, usize_param(case, "total_len"))?
                .with_sequence_count(count(param(case, "sequence_count")))
                .with_pattern(&pattern);
            let mut out = vec![0_u8; idle.total_len()];
            let n = idle.encode(&mut out)?;
            Ok(json!({ "output_hex": hex::encode(&out[..n]) }))
        }
        "octet_string" => {
            let octets = hex_param(case, "octets_hex");
            let counter = SequenceCounter::new(count(param(case, "sequence_count")));
            let ptype = packet_type(param(case, "packet_type"));
            let mut enc = OctetStringEncoder::new(&config(case)?, ptype, apid(case), counter)?;
            let indicator = param(case, "secondary_header_indicator").as_bool().unwrap();
            let mut out = vec![0_u8; octets.len() + 6];
            let n = enc.encode(indicator, &octets, &mut out)?;
            Ok(json!({
                "output_hex": hex::encode(&out[..n]),
                "next_count": enc.counter().peek().get(),
            }))
        }
        "track" => {
            let events = param(case, "events").as_array().unwrap();
            Ok(match usize_param(case, "capacity") {
                1 => track_with!(1, events),
                2 => track_with!(2, events),
                n => panic!("{}: capacity {n}", case.id),
            })
        }
        "decode_stream" => Ok(decode_stream(case)),
        "split" => {
            let bytes = input(case);
            let layout = match param(case, "secondary_header_len") {
                Value::Null => SecondaryHeaderLayout::Absent,
                v => SecondaryHeaderLayout::Opaque(
                    NonZeroUsize::new(usize::try_from(u64_of(v)).unwrap()).unwrap(),
                ),
            };
            let s = SpacePacket::decode(&bytes, mode(case))?.split(&layout)?;
            Ok(json!({
                "secondary_header_hex": hex::encode(s.secondary_header()),
                "user_data_hex": hex::encode(s.user_data()),
            }))
        }
        "segment" => segment_case(case),
        "reassemble" => Ok(reassemble_case(case)),
        "split_time" => split_time_case(case),
        op => panic!("{}: unsupported operation {op}", case.id),
    }
}

fn handle(case: &CodecCase) -> Outcome {
    match run(case) {
        Ok(v) => Outcome::Ok(v.as_object().cloned().unwrap()),
        Err(e) => Outcome::Err(e.kind().name().to_owned()),
    }
}

macro_rules! vector_test {
    ($name:ident, $file:literal) => {
        #[test]
        fn $name() {
            // req: VER-021, TV-001, TV-002, TV-003
            run_codec_file(concat!("spp/", $file, ".json"), handle).assert_passed();
        }
    };
}

vector_test!(decode_valid, "decode_valid");
vector_test!(decode_invalid, "decode_invalid");
vector_test!(encode_packets, "encode");
vector_test!(idle_packets, "idle");
vector_test!(octet_string, "octet_string");
vector_test!(sequence_tracker, "sequence_tracker");
vector_test!(stream, "stream");
vector_test!(split, "split");
vector_test!(segment, "segment");
vector_test!(reassemble, "reassemble");
vector_test!(secondary_header_time, "secondary_header_time");

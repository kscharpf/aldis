//! Space Packet and telemetry time vectors (133.0-B-2, 301.0-B-4) against NASA cFE's `MSG`
//! module (v7.0.1), and against Aldis.
//!
//! cFE reads a header with the `CFE_MSG_Get*` functions and builds one with `CFE_MSG_Init` and
//! the `CFE_MSG_Set*` functions ([`cfe_msg`]). Each case is checked in two ways, reported
//! separately:
//!
//! - [`check_packets`]: every well-formed packet in the case (its input, its expected output, or
//!   its packet lists, each walked packet by packet as far as Aldis decodes it in lenient mode)
//!   is read by cFE and by `aldis-spp`, which must agree on every primary header field, and cFE
//!   must build the same octets from Aldis's fields and data field;
//! - [`check_spp`], [`check_secondary_header`], [`check_cuc`]: the expected values of the case
//!   that cFE can produce are compared with the vector, as the `spacepackets` check does.
//!
//! What each operation compares, and what cFE does not check, is listed in cross-checks.md §2.

use aldis_core::ids::{Apid, SeqCount};
use aldis_spp::{
    PacketFields, PacketType, SecondaryHeaderLayout, SequenceFlags, SpacePacket, SppConfig,
    TimeHeaderLayout, encode,
};
use aldis_testkit::{CodecCase, hex};
use aldis_time::{CucEpoch, CucFormat, TimeCode, TimeFormat};
use serde_json::{Map, Value, json};

use crate::cfe_msg::{self, CfeError, Fields, Header};
use crate::{Verdict, compare_produced, expected, input, param, param_hex, param_u64};

/// The primary header length (133.0 §4.1.3).
const PRIMARY_HEADER_LEN: usize = 6;

/// The longest Packet Data Field the 16-bit Packet Data Length can express (133.0 §4.1.3.5.2).
const MAX_DATA_FIELD_LEN: usize = 65_536;

/// cFE's telemetry time: a CUC T-field of 4 coarse and 2 fine octets, without a P-field, right
/// after the primary header (`option_inc/default_cfe_msg_sechdr.h`, `cfe_msg_sechdr_time.c`).
const TIME_LEN: usize = 6;

/// The skip for a time layout other than cFE's.
const NOT_CFE_TIME: &str = "cFE's telemetry time is a CUC T-field of 4 coarse and 2 fine \
                            octets without a P-field";

/// Checks the primary header of every well-formed packet in one case of a `spp/` file: cFE
/// reads what Aldis reads, and builds the same octets from Aldis's fields.
#[must_use]
pub fn check_packets(case: &CodecCase) -> Verdict {
    let packets = packets(case);
    if packets.is_empty() {
        return Verdict::Skip("no well-formed packet in the case".to_owned());
    }
    let diffs: Vec<String> = (0_usize..)
        .zip(&packets)
        .filter_map(|(index, packet)| {
            packet_agrees(packet)
                .err()
                .map(|diff| format!("packet {index}: {diff}"))
        })
        .collect();
    if diffs.is_empty() {
        Verdict::Agree
    } else {
        Verdict::Disagree(diffs.join("; "))
    }
}

/// Checks the expected values of one case of a `spp/` file other than `secondary_header_time`
/// that cFE can produce.
#[must_use]
pub fn check_spp(case: &CodecCase) -> Verdict {
    match case.operation.as_str() {
        "decode" if case.expect.ok => decode(case),
        "peek_length" if case.expect.ok => peek_length(case),
        "decode" | "peek_length" => undetected(case),
        "decode_stream" => decode_stream(case),
        "encode" => encode_case(case),
        "octet_string" => octet_string(case),
        "idle" => idle(case),
        "segment" => segment(case),
        "reassemble" => Verdict::Skip("cFE has no reassembler".to_owned()),
        "track" => Verdict::Skip("cFE has no sequence tracker".to_owned()),
        "split" => Verdict::Skip(
            "cFE has no secondary header length: its C structures fix where the payload starts"
                .to_owned(),
        ),
        other => Verdict::Skip(format!("operation {other}: no counterpart in cFE")),
    }
}

/// Checks the Time Code Field of one case of `spp/secondary_header_time.json` where the layout
/// is cFE's telemetry time.
#[must_use]
pub fn check_secondary_header(case: &CodecCase) -> Verdict {
    if case.operation == "split_time" {
        split_time(case)
    } else {
        Verdict::Skip(format!(
            "operation {}: no counterpart in cFE",
            case.operation
        ))
    }
}

/// Checks one case of `time/cuc.json` whose format is cFE's: the T-field is carried in a
/// telemetry packet, read by `CFE_MSG_GetMsgTime` and written by `CFE_MSG_SetMsgTime`.
#[must_use]
pub fn check_cuc(case: &CodecCase) -> Verdict {
    match case.operation.as_str() {
        "decode" | "decode_implicit" => cuc_decode(case),
        "encode" => cuc_encode(case),
        "cuc_to_ns" | "cuc_from_ns" => Verdict::Skip(
            "CFE_MSG converts no time; the conversions of CFE_TIME are not examined".to_owned(),
        ),
        other => Verdict::Skip(format!("operation {other}: no counterpart in cFE")),
    }
}

/// Aldis's reading of a packet, in the terms of [`cfe_msg::read_header`].
fn aldis_header(packet: &SpacePacket<'_>) -> Header {
    Header {
        // SpacePacket::decode accepts only version 000 (SPP-001).
        version: 0,
        packet_type: match packet.packet_type() {
            PacketType::Telemetry => 0,
            PacketType::Telecommand => 1,
        },
        secondary_header: packet.has_secondary_header(),
        apid: packet.apid().get(),
        sequence_flags: packet.sequence_flags().bits(),
        sequence_count: packet.sequence_count().get(),
        size: packet.len(),
    }
}

/// The fields of `header` as cFE's setters take them.
const fn fields_of(header: &Header) -> Fields {
    Fields {
        version: header.version,
        packet_type: header.packet_type,
        secondary_header: header.secondary_header,
        apid: header.apid,
        sequence_flags: header.sequence_flags,
        sequence_count: header.sequence_count,
    }
}

/// Every packet that Aldis decodes in lenient mode from the start of `octets`, one after the
/// other, up to the first error.
fn walk(octets: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut rest = octets;
    while let Ok(packet) = SpacePacket::decode_lenient(rest) {
        let (one, tail) = rest.split_at(packet.len());
        out.push(one.to_vec());
        rest = tail;
    }
    out
}

/// The hex strings of `value`: one string, or an array of them.
fn hex_strings(case: &CodecCase, value: Option<&Value>) -> Vec<Vec<u8>> {
    let strings: Vec<&Value> = match value {
        None => Vec::new(),
        Some(Value::Array(items)) => items.iter().collect(),
        Some(one) => vec![one],
    };
    strings
        .into_iter()
        .map(|text| {
            text.as_str()
                .and_then(|text| hex::decode(text).ok())
                .unwrap_or_else(|| panic!("{}: expected octets are not hex", case.id))
        })
        .collect()
}

/// The well-formed packets of a case: its input, expected output, and expected or given packet
/// lists, each walked as far as Aldis decodes it.
fn packets(case: &CodecCase) -> Vec<Vec<u8>> {
    let mut sources = vec![input(case)];
    sources.extend(hex_strings(case, case.expect.values.get("output_hex")));
    sources.extend(hex_strings(case, case.expect.values.get("packets_hex")));
    sources.extend(hex_strings(
        case,
        case.params.as_ref().and_then(|p| p.get("packets_hex")),
    ));
    sources.iter().flat_map(|octets| walk(octets)).collect()
}

/// cFE and Aldis read the same primary header from `packet`, and cFE builds `packet` again
/// from Aldis's fields and data field.
fn packet_agrees(packet: &[u8]) -> Result<(), String> {
    let aldis =
        SpacePacket::decode_lenient(packet).map_err(|e| format!("Aldis rejects it: {e}"))?;
    let want = aldis_header(&aldis);
    let got = cfe_msg::read_header(packet).map_err(|e| format!("cFE returns {e}"))?;
    if got != want {
        return Err(format!("cFE reads {got:?}, Aldis {want:?}"));
    }
    let built = cfe_msg::build(fields_of(&want), aldis.data_field())
        .map_err(|e| format!("cFE cannot build it: {e}"))?;
    if built != packet {
        return Err(format!(
            "cFE builds {} from Aldis's fields, the packet is {}",
            hex::encode(&built),
            hex::encode(packet)
        ));
    }
    Ok(())
}

/// The skip for an expected decoding error that cFE does not detect: it trusts its callers.
fn undetected(case: &CodecCase) -> Verdict {
    let reason = match case.expect.error.as_deref() {
        Some("Incomplete") => {
            "cFE does not check a packet against its buffer: CFE_MSG_GetSize reads the Packet \
             Data Length only"
        }
        Some("UnsupportedVersion") => {
            "cFE reads the Packet Version Number (CFE_MSG_GetHeaderVersion) but rejects no value"
        }
        Some("InvalidField") => "cFE has no Idle Packet rules (SPP-004)",
        _ => "an error cFE does not detect",
    };
    Verdict::Skip(reason.to_owned())
}

/// `CFE_MSG_Get*` on the input of a successful decode case.
fn decode(case: &CodecCase) -> Verdict {
    let octets = input(case);
    let header = match cfe_msg::read_header(&octets) {
        Ok(header) => header,
        Err(e) => return Verdict::Disagree(format!("cFE returns {e}")),
    };
    let Some(data) = octets.get(PRIMARY_HEADER_LEN..header.size) else {
        return Verdict::Disagree(format!(
            "cFE reads a size of {} from {} octets",
            header.size,
            octets.len()
        ));
    };
    let mut values = Map::new();
    values.insert("packet_type".into(), json!(header.packet_type));
    values.insert(
        "secondary_header_flag".into(),
        json!(u8::from(header.secondary_header)),
    );
    values.insert("apid".into(), json!(header.apid));
    values.insert("sequence_flags".into(), json!(header.sequence_flags));
    values.insert("sequence_count".into(), json!(header.sequence_count));
    values.insert("packet_len".into(), json!(header.size));
    values.insert("consumed".into(), json!(header.size));
    values.insert("data_len".into(), json!(data.len()));
    values.insert("data_hex".into(), json!(hex::encode(data)));
    compare_produced(case, &values)
}

/// `CFE_MSG_GetSize` on the header of a successful `peek_length` case.
fn peek_length(case: &CodecCase) -> Verdict {
    match cfe_msg::read_header(&input(case)) {
        Ok(header) => {
            let mut values = Map::new();
            values.insert("len".into(), json!(header.size));
            compare_produced(case, &values)
        }
        Err(e) => Verdict::Disagree(format!("cFE returns {e}")),
    }
}

/// A stream split by `CFE_MSG_GetSize`: a packet whose size exceeds the octets left, or fewer
/// than 6 octets left, stops it as `Incomplete`.
fn decode_stream(case: &CodecCase) -> Verdict {
    let stop = expected(case, "stop_error");
    if !(stop.is_null() || stop == "Incomplete") {
        return Verdict::Skip(format!(
            "the stream stops at {stop}, which cFE does not detect"
        ));
    }
    let octets = input(case);
    let mut packets = Vec::new();
    let mut offset = 0;
    let mut stop_error = Value::Null;
    while let Some(rest) = octets.get(offset..).filter(|rest| !rest.is_empty()) {
        let header = match cfe_msg::read_header(rest) {
            Ok(header) if rest.len() >= PRIMARY_HEADER_LEN && header.size <= rest.len() => header,
            Ok(_) => {
                stop_error = json!("Incomplete");
                break;
            }
            Err(e) => return Verdict::Disagree(format!("cFE returns {e} at {offset}")),
        };
        packets.push(json!({
            "apid": header.apid,
            "sequence_count": header.sequence_count,
            "consumed": header.size,
        }));
        offset += header.size;
    }
    // `anomalies` comes from Aldis's idle rules, which cFE does not have.
    let want: Vec<Value> = expected(case, "packets")
        .as_array()
        .unwrap_or_else(|| panic!("{}: packets is not an array", case.id))
        .iter()
        .map(|p| {
            let mut p = p.clone();
            if let Some(fields) = p.as_object_mut() {
                fields.remove("anomalies");
            }
            p
        })
        .collect();
    let mut diffs = Vec::new();
    if packets != want {
        diffs.push(format!(
            "packets: expected {}, cFE reads {}",
            Value::from(want),
            Value::from(packets)
        ));
    }
    let mut values = Map::new();
    values.insert("stop_error".into(), stop_error);
    values.insert("offset".into(), json!(offset));
    if let Verdict::Disagree(diff) = compare_produced(case, &values) {
        diffs.push(diff);
    }
    if diffs.is_empty() {
        Verdict::Agree
    } else {
        Verdict::Disagree(diffs.join("; "))
    }
}

/// A `u8` parameter.
fn param_u8(case: &CodecCase, key: &str) -> u8 {
    u8::try_from(param_u64(case, key)).unwrap_or_else(|_| panic!("{}: {key} out of range", case.id))
}

/// A `u16` parameter.
fn param_u16(case: &CodecCase, key: &str) -> u16 {
    u16::try_from(param_u64(case, key))
        .unwrap_or_else(|_| panic!("{}: {key} out of range", case.id))
}

/// The header fields of an encoding case, in cFE's terms.
fn param_fields(case: &CodecCase, secondary_header: bool, sequence_flags: u8) -> Fields {
    Fields {
        version: 0,
        packet_type: param_u8(case, "packet_type"),
        secondary_header,
        apid: param_u16(case, "apid"),
        sequence_flags,
        sequence_count: param_u16(case, "sequence_count"),
    }
}

/// The same fields in Aldis's terms.
fn aldis_fields(case: &CodecCase, fields: Fields) -> PacketFields {
    PacketFields {
        packet_type: if fields.packet_type == 0 {
            PacketType::Telemetry
        } else {
            PacketType::Telecommand
        },
        apid: Apid::new(fields.apid).unwrap_or_else(|| panic!("{}: apid", case.id)),
        sequence_flags: SequenceFlags::from_bits(fields.sequence_flags),
        sequence_count: SeqCount::new(fields.sequence_count)
            .unwrap_or_else(|| panic!("{}: sequence_count", case.id)),
    }
}

/// The verdict on a rejected packet: cFE's only checks on building are the size of
/// `CFE_MSG_SetSize` (7–65 542 octets) and the field widths; `rule` names what it lacks.
fn rejection(fields: Fields, data_len: usize, rule: &str) -> Verdict {
    if data_len != 0 && data_len <= MAX_DATA_FIELD_LEN {
        return Verdict::Skip(format!("a rejection rule cFE does not have: {rule}"));
    }
    match cfe_msg::build(fields, &vec![0; data_len]) {
        Err(CfeError::BadArgument) => Verdict::Agree,
        Err(e) => Verdict::Disagree(format!("expected CFE_MSG_BAD_ARGUMENT, cFE returns {e}")),
        Ok(_) => Verdict::Disagree(format!("cFE builds a {data_len}-octet data field")),
    }
}

/// Builds the packet with cFE and compares it with `output_hex`, and with `next_count` the
/// count `CFE_MSG_GetNextSequenceCount` gives after it, when the case expects one.
fn compare_built(case: &CodecCase, fields: Fields, data: &[u8]) -> Verdict {
    match cfe_msg::build(fields, data) {
        Ok(octets) => {
            let mut values = Map::new();
            values.insert("output_hex".into(), json!(hex::encode(&octets)));
            if case.expect.values.contains_key("next_count") {
                values.insert(
                    "next_count".into(),
                    json!(cfe_msg::next_sequence_count(fields.sequence_count)),
                );
            }
            compare_produced(case, &values)
        }
        Err(e) => Verdict::Disagree(format!("cFE cannot build the packet: {e}")),
    }
}

fn encode_case(case: &CodecCase) -> Verdict {
    let secondary = param_hex(case, "secondary_header_hex");
    let user = param_hex(case, "user_data_hex");
    let fields = param_fields(
        case,
        !secondary.is_empty(),
        param_u8(case, "sequence_flags"),
    );
    let data = [secondary.as_slice(), user.as_slice()].concat();
    if !case.expect.ok {
        return rejection(
            fields,
            data.len(),
            "an Idle Packet with a secondary header (SPP-005), max_packet_len (SPP-006)",
        );
    }
    let verdict = compare_built(case, fields, &data);
    if verdict != Verdict::Agree {
        return verdict;
    }
    // Aldis's encoder on the same parts must write what cFE wrote.
    let config = case
        .config
        .as_ref()
        .and_then(|config| config.get("max_packet_len"))
        .and_then(Value::as_u64)
        .and_then(|len| usize::try_from(len).ok())
        .map_or(SppConfig::DEFAULT, |len| {
            SppConfig::new(len).unwrap_or_else(|e| panic!("{}: {e}", case.id))
        });
    let aldis = config
        .packet(aldis_fields(case, fields), &secondary, &user)
        .and_then(|packet| {
            let mut out = vec![0; packet.packet_len()];
            encode(&packet, &mut out).map(|_| out)
        });
    match (aldis, cfe_msg::build(fields, &data)) {
        (Ok(aldis), Ok(cfe)) if aldis == cfe => Verdict::Agree,
        (aldis, cfe) => Verdict::Disagree(format!("Aldis encodes {aldis:?}, cFE {cfe:?}")),
    }
}

fn octet_string(case: &CodecCase) -> Verdict {
    let indicator = param(case, "secondary_header_indicator")
        .as_bool()
        .unwrap_or_else(|| panic!("{}: secondary_header_indicator", case.id));
    // 133.0 §4.2.2: the Octet String service always sends Sequence Flags 11.
    let fields = param_fields(case, indicator, 0b11);
    let octets = param_hex(case, "octets_hex");
    if !case.expect.ok {
        return rejection(
            fields,
            octets.len(),
            "the idle APID (SPP-012), max_packet_len (SPP-006)",
        );
    }
    compare_built(case, fields, &octets)
}

/// The fields of an Idle Packet: APID 2047 (133.0 §4.1.3.3.4.4), no secondary header
/// (§4.1.3.3.3.4), and Sequence Flags `11`, the SPP-010 choice the vectors encode.
fn idle_fields(case: &CodecCase) -> Fields {
    Fields {
        version: 0,
        packet_type: 0,
        secondary_header: false,
        apid: Apid::IDLE.get(),
        sequence_flags: 0b11,
        sequence_count: param_u16(case, "sequence_count"),
    }
}

fn idle(case: &CodecCase) -> Verdict {
    let fields = idle_fields(case);
    let total = usize::try_from(param_u64(case, "total_len"))
        .unwrap_or_else(|_| panic!("{}: total_len", case.id));
    if !case.expect.ok {
        return rejection(
            fields,
            total.saturating_sub(PRIMARY_HEADER_LEN),
            "max_packet_len (SPP-006)",
        );
    }
    let output = hex_strings(case, Some(expected(case, "output_hex")))
        .pop()
        .unwrap_or_default();
    match cfe_msg::read_header(&output) {
        Ok(header) if fields_of(&header) == fields && header.size == total => Verdict::Agree,
        Ok(header) => Verdict::Disagree(format!(
            "cFE reads {header:?}, expected {fields:?} with size {total}"
        )),
        Err(e) => Verdict::Disagree(format!("cFE returns {e}")),
    }
}

fn segment(case: &CodecCase) -> Verdict {
    if !case.expect.ok {
        return Verdict::Skip("cFE has no segmentation rules".to_owned());
    }
    let mut count = param_u16(case, "sequence_count");
    let secondary_header = !param_hex(case, "secondary_header_hex").is_empty();
    let packets = hex_strings(case, Some(expected(case, "packets_hex")));
    let mut diffs = Vec::new();
    for (index, packet) in packets.iter().enumerate() {
        match cfe_msg::read_header(packet) {
            Ok(h)
                if h.packet_type == param_u8(case, "packet_type")
                    && h.secondary_header == secondary_header
                    && h.apid == param_u16(case, "apid")
                    && h.sequence_count == count
                    && h.size == packet.len() => {}
            Ok(h) => diffs.push(format!("packet {index}: cFE reads {h:?}, count {count}")),
            Err(e) => diffs.push(format!("packet {index}: cFE returns {e}")),
        }
        // The count advances by one per packet, modulo 2^14 (133.0 §4.1.3.4.3.4).
        count = cfe_msg::next_sequence_count(count);
    }
    let mut values = Map::new();
    values.insert("next_count".into(), json!(count));
    if let Verdict::Disagree(diff) = compare_produced(case, &values) {
        diffs.push(diff);
    }
    if diffs.is_empty() {
        Verdict::Agree
    } else {
        Verdict::Disagree(diffs.join("; "))
    }
}

/// The member `key` of the JSON object `value`.
fn member<'a>(case: &CodecCase, value: &'a Value, key: &str) -> &'a Value {
    value
        .get(key)
        .unwrap_or_else(|| panic!("{}: no member {key}", case.id))
}

/// Whether a time code format of a vector is cFE's: CUC with 4 coarse and 2 fine octets.
/// `coarse` and `fine` name its members, which differ between files.
fn is_cfe_cuc(case: &CodecCase, format: &Value, coarse: &str, fine: &str) -> bool {
    let code = format.get("code").and_then(Value::as_str).unwrap_or("cuc");
    code == "cuc"
        && member(case, format, coarse).as_u64() == Some(4)
        && member(case, format, fine).as_u64() == Some(2)
}

/// The 6 T-field octets cFE's time stands for: the seconds, and the upper 16 bits of the
/// subseconds, which are all cFE writes (`CFE_MSG_SetMsgTime`).
fn cfe_tfield(seconds: u32, subseconds: u32) -> Vec<u8> {
    let mut out = seconds.to_be_bytes().to_vec();
    out.extend_from_slice(&subseconds.to_be_bytes()[..2]);
    out
}

/// The time of a 6-octet T-field in cFE's terms: the seconds, and the fine octets as the upper
/// 16 bits of the subseconds.
const fn cfe_time_of(tfield: &[u8]) -> Option<(u32, u32)> {
    let &[c0, c1, c2, c3, f0, f1] = tfield else {
        return None;
    };
    Some((
        u32::from_be_bytes([c0, c1, c2, c3]),
        u32::from_be_bytes([f0, f1, 0, 0]),
    ))
}

/// The `split_time` check: `CFE_MSG_GetMsgTime` reads the Time Code Field, `CFE_MSG_SetMsgTime`
/// writes it again, and Aldis's split reads the same value.
fn split_time(case: &CodecCase) -> Verdict {
    let layout = param(case, "layout");
    let format = member(case, layout, "format");
    let pfield = member(case, layout, "pfield").as_bool() == Some(true);
    if !is_cfe_cuc(case, format, "coarse", "fine") || pfield {
        return Verdict::Skip(format!("layout {layout}: {NOT_CFE_TIME}"));
    }
    let packet = input(case);
    if !case.expect.ok {
        return match (case.expect.error.as_deref(), cfe_msg::read_time(&packet)) {
            // The flag clear: cFE reads no time without a secondary header.
            (Some("InvalidField"), Err(CfeError::WrongMsgType)) => Verdict::Agree,
            (Some("InvalidField"), other) => Verdict::Disagree(format!(
                "expected CFE_MSG_WRONG_MSG_TYPE, cFE returns {other:?}"
            )),
            (Some("LengthMismatch"), _) => Verdict::Skip(
                "cFE does not check the time or the secondary header against the Packet Data \
                 Length"
                    .to_owned(),
            ),
            _ => Verdict::Skip("an error cFE does not detect".to_owned()),
        };
    }
    if packet.first().is_some_and(|b0| b0 & 0x10 != 0) {
        return Verdict::Skip(
            "a telecommand: cFE defines its time for telemetry only (CFE_MSG_GetMsgTime returns \
             CFE_MSG_WRONG_MSG_TYPE)"
                .to_owned(),
        );
    }
    let (seconds, subseconds) = match cfe_msg::read_time(&packet) {
        Ok(time) => time,
        Err(e) => return Verdict::Disagree(format!("cFE returns {e}")),
    };
    let mut values = Map::new();
    values.insert(
        "time_code_hex".into(),
        json!(hex::encode(&cfe_tfield(seconds, subseconds))),
    );
    let mut diffs = Vec::new();
    if let Verdict::Disagree(diff) = compare_produced(case, &values) {
        diffs.push(diff);
    }
    // cFE writes the vector's time code into the packet with that field cleared.
    let want = hex_strings(case, Some(expected(case, "time_code_hex")))
        .pop()
        .unwrap_or_default();
    let mut cleared = packet.clone();
    if let Some(time) = cleared.get_mut(PRIMARY_HEADER_LEN..PRIMARY_HEADER_LEN + TIME_LEN) {
        time.fill(0);
    }
    match cfe_time_of(&want).map(|(s, ss)| cfe_msg::write_time(&cleared, s, ss)) {
        Some(Ok(written)) if written == packet => {}
        Some(Ok(written)) => diffs.push(format!(
            "cFE writes the expected time code as {}, the input is {}",
            hex::encode(&written),
            hex::encode(&packet)
        )),
        Some(Err(e)) => diffs.push(format!("cFE returns {e}")),
        None => diffs.push("time_code_hex is not 6 octets".to_owned()),
    }
    // Aldis reads the same value with the case's layout.
    let aldis = aldis_cuc_value(case, layout, &packet);
    if aldis != Some((u64::from(seconds), subseconds >> 16)) {
        diffs.push(format!(
            "Aldis reads {aldis:?}, cFE ({seconds}, {:#06x})",
            subseconds >> 16
        ));
    }
    if diffs.is_empty() {
        Verdict::Agree
    } else {
        Verdict::Disagree(diffs.join("; "))
    }
}

/// Aldis's CUC value of the packet under the case's 4+2 implicit layout: the coarse time, and
/// the 2 fine octets as an integer.
fn aldis_cuc_value(case: &CodecCase, layout: &Value, packet: &[u8]) -> Option<(u64, u32)> {
    let format = member(case, layout, "format");
    let epoch = if member(case, format, "level2").as_bool() == Some(true) {
        CucEpoch::Agency
    } else {
        CucEpoch::Tai1958
    };
    let ancillary = member(case, layout, "ancillary_len")
        .as_u64()
        .and_then(|n| usize::try_from(n).ok())?;
    let format = CucFormat::new(epoch, 4, 2).ok()?;
    let layout = TimeHeaderLayout::new(TimeFormat::Cuc(format), false, ancillary).ok()?;
    let view = SpacePacket::decode(packet).ok()?;
    let split = view.split(&SecondaryHeaderLayout::Time(layout)).ok()?;
    match split.time_code()?.code() {
        TimeCode::Cuc(cuc) => {
            let fine = u32::try_from(cuc.value().fine() >> 112).ok()?;
            Some((cuc.value().coarse(), fine))
        }
        _ => None,
    }
}

/// A telemetry packet with a secondary header whose data field is `tfield`, built by cFE.
fn time_packet(tfield: &[u8]) -> Result<Vec<u8>, CfeError> {
    cfe_msg::build(
        Fields {
            version: 0,
            packet_type: 0,
            secondary_header: true,
            apid: 1,
            sequence_flags: 0b11,
            sequence_count: 0,
        },
        tfield,
    )
}

fn cuc_decode(case: &CodecCase) -> Verdict {
    if !case.expect.ok {
        return Verdict::Skip(
            "cFE reads no P-field and does not check the time against the data field".to_owned(),
        );
    }
    let format = expected(case, "format");
    if !is_cfe_cuc(case, format, "coarse_octets", "fine_octets") {
        return Verdict::Skip(format!("format {format}: {NOT_CFE_TIME}"));
    }
    let pfield_len = hex_strings(case, Some(expected(case, "pfield_hex")))
        .pop()
        .map_or(0, |p| p.len());
    let octets = input(case);
    let Some(tfield) = octets.get(pfield_len..pfield_len + TIME_LEN) else {
        return Verdict::Disagree("the input is shorter than the T-field".to_owned());
    };
    match time_packet(tfield).and_then(|packet| cfe_msg::read_time(&packet)) {
        Ok((seconds, subseconds)) => {
            let mut values = Map::new();
            values.insert("value.coarse".into(), json!(seconds));
            values.insert(
                "value.fine_hex".into(),
                json!(hex::encode(&subseconds.to_be_bytes()[..2])),
            );
            compare_produced(case, &values)
        }
        Err(e) => Verdict::Disagree(format!("cFE returns {e}")),
    }
}

fn cuc_encode(case: &CodecCase) -> Verdict {
    if !case.expect.ok {
        return Verdict::Skip("cFE checks no time value: its seconds are a uint32".to_owned());
    }
    let format = param(case, "format");
    if !is_cfe_cuc(case, format, "coarse_octets", "fine_octets") {
        return Verdict::Skip(format!("format {format}: {NOT_CFE_TIME}"));
    }
    let value = param(case, "value");
    let coarse = member(case, value, "coarse")
        .as_u64()
        .and_then(|c| u32::try_from(c).ok())
        .unwrap_or_else(|| panic!("{}: coarse", case.id));
    let fine = hex_strings(case, Some(member(case, value, "fine_hex")))
        .pop()
        .unwrap_or_default();
    let Some(&[f0, f1]) = fine.first_chunk::<2>() else {
        return Verdict::Disagree("fine_hex is not 2 octets".to_owned());
    };
    let written = time_packet(&[0; TIME_LEN]).and_then(|packet| {
        cfe_msg::write_time(&packet, coarse, u32::from_be_bytes([f0, f1, 0, 0]))
    });
    let output = hex_strings(case, Some(expected(case, "output_hex")))
        .pop()
        .unwrap_or_default();
    // cFE has no P-field: the T-field it writes must end the expected output.
    let want = output.get(output.len().saturating_sub(TIME_LEN)..);
    match written {
        Ok(packet) if packet.get(PRIMARY_HEADER_LEN..) == want => Verdict::Agree,
        Ok(packet) => Verdict::Disagree(format!(
            "output_hex: expected the T-field to end {}, cFE writes {}",
            hex::encode(&output),
            hex::encode(packet.get(PRIMARY_HEADER_LEN..).unwrap_or_default())
        )),
        Err(e) => Verdict::Disagree(format!("cFE returns {e}")),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{CodecCase, Verdict, check_cuc, check_packets, check_secondary_header, check_spp};

    fn case(value: &Value) -> CodecCase {
        serde_json::from_value(value.clone()).unwrap()
    }

    #[test]
    fn packets_are_walked_from_every_source() {
        // req: VER-040
        // Two packets in the input, then a truncated one; one in the expected output.
        let stream = json!({
            "id": "t-000", "requirements": [], "operation": "decode_stream",
            "input_hex": "0005C0070000AB1FFFC0000001CDEF0005C007",
            "expect": {"ok": true, "output_hex": "0802800100010102"},
        });
        assert_eq!(super::packets(&case(&stream)).len(), 3);
        assert_eq!(check_packets(&case(&stream)), Verdict::Agree);
        let none = json!({
            "id": "t-000", "requirements": [], "operation": "decode",
            "input_hex": "2001C000000000", "expect": {"ok": false, "error": "UnsupportedVersion"},
        });
        assert!(matches!(check_packets(&case(&none)), Verdict::Skip(_)));
    }

    #[test]
    fn a_wrong_decode_vector_disagrees() {
        // req: VER-040
        let good = json!({
            "id": "t-001", "requirements": [], "operation": "decode",
            "input_hex": "0005C0070000AB",
            "expect": {"ok": true, "packet_type": 0, "secondary_header_flag": 0, "apid": 5,
                       "sequence_flags": 3, "sequence_count": 7, "packet_len": 7, "consumed": 7,
                       "data_len": 1, "data_hex": "AB", "idle": false, "packet_name": null,
                       "anomalies": []},
        });
        assert_eq!(check_spp(&case(&good)), Verdict::Agree);
        let mut wrong = good;
        wrong["expect"]["apid"] = json!(6);
        assert_eq!(
            check_spp(&case(&wrong)),
            Verdict::Disagree("apid: expected 6, produced 5".to_owned())
        );
    }

    #[test]
    fn a_wrong_encode_vector_disagrees() {
        // req: VER-040
        let good = json!({
            "id": "t-002", "requirements": [], "operation": "encode",
            "params": {"packet_type": 1, "apid": 5, "sequence_flags": 3, "sequence_count": 7,
                       "secondary_header_hex": "", "user_data_hex": "AB"},
            "expect": {"ok": true, "output_hex": "1005C0070000AB"},
        });
        assert_eq!(check_spp(&case(&good)), Verdict::Agree);
        let mut wrong = good;
        wrong["expect"]["output_hex"] = json!("1005C0070000AC");
        assert!(matches!(check_spp(&case(&wrong)), Verdict::Disagree(_)));
    }

    #[test]
    fn a_wrong_time_vector_disagrees() {
        // req: VER-040
        let good = json!({
            "id": "t-003", "requirements": [], "operation": "split_time",
            "params": {"layout": {"format": {"code": "cuc", "coarse": 4, "fine": 2,
                                             "level2": false},
                                  "pfield": false, "ancillary_len": 0}},
            "input_hex": "0860C0000006123456788000EE",
            "expect": {"ok": true, "secondary_header_hex": "123456788000", "user_data_hex": "EE",
                       "time_code_hex": "123456788000", "ancillary_hex": ""},
        });
        assert_eq!(check_secondary_header(&case(&good)), Verdict::Agree);
        let mut wrong = good;
        wrong["expect"]["time_code_hex"] = json!("123456788001");
        assert!(matches!(
            check_secondary_header(&case(&wrong)),
            Verdict::Disagree(_)
        ));
        let encode = json!({
            "id": "t-004", "requirements": [], "operation": "encode",
            "params": {"format": {"code": "cuc", "epoch": "level1", "coarse_octets": 4,
                                  "fine_octets": 2, "mission_bits": 0, "octet2": false,
                                  "pfield_tail_hex": ""},
                       "value": {"coarse": 16_909_060, "fine_hex": "8000"},
                       "include_pfield": true},
            "expect": {"ok": true, "output_hex": "1E010203048000"},
        });
        assert_eq!(check_cuc(&case(&encode)), Verdict::Agree);
        let mut wrong = encode;
        wrong["expect"]["output_hex"] = json!("1E010203048001");
        assert!(matches!(check_cuc(&case(&wrong)), Verdict::Disagree(_)));
    }

    #[test]
    fn rejections_cfe_has_agree_and_others_skip() {
        // req: VER-040
        let empty = json!({
            "id": "t-005", "requirements": [], "operation": "encode",
            "params": {"packet_type": 0, "apid": 1, "sequence_flags": 3, "sequence_count": 0,
                       "secondary_header_hex": "", "user_data_hex": ""},
            "expect": {"ok": false, "error": "InvalidField"},
        });
        assert_eq!(check_spp(&case(&empty)), Verdict::Agree);
        let mut idle = empty;
        idle["params"]["apid"] = json!(2047);
        idle["params"]["secondary_header_hex"] = json!("01");
        assert!(matches!(check_spp(&case(&idle)), Verdict::Skip(_)));
    }
}

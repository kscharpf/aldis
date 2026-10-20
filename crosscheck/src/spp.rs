//! Space Packet vectors (133.0-B-2, `test-vectors/spp/`) against `spacepackets` 0.18.0.
//!
//! Decoding goes through `CcsdsPacketReader` (without a checksum), header-only decoding
//! through `SpHeader::from_be_bytes`, and encoding through `CcsdsPacketCreator`. Which
//! expected values each operation compares is listed in cross-checks.md §2.

use aldis_testkit::{CodecCase, hex};
use arbitrary_int::{u11, u14};
use serde_json::{Map, Value, json};
use spacepackets::{
    ByteConversionError, CcsdsPacket, CcsdsPacketCreator, CcsdsPacketReadError, CcsdsPacketReader,
    PacketType, SequenceFlags, SpHeader,
};

use crate::{Verdict, compare_produced, expected, input, param, param_hex, param_u64};

/// The longest Packet Data Field the 16-bit Packet Data Length can express (133.0 §4.1.3.5.2).
const MAX_DATA_FIELD_LEN: usize = 65_536;

/// Checks one case of a `spp/` file.
#[must_use]
pub fn check(case: &CodecCase) -> Verdict {
    match case.operation.as_str() {
        "decode" => decode(case),
        "peek_length" => peek_length(case),
        "decode_stream" => decode_stream(case),
        "encode" => encode(case),
        "idle" => idle(case),
        "octet_string" => octet_string(case),
        "segment" => segment(case),
        other => Verdict::Skip(format!("operation {other}: no counterpart in spacepackets")),
    }
}

/// The header fields and lengths `spacepackets` reads from a packet.
fn packet_values(packet: &CcsdsPacketReader<'_>) -> Map<String, Value> {
    let mut values = Map::new();
    values.insert("packet_type".into(), json!(packet.packet_type() as u8));
    values.insert(
        "secondary_header_flag".into(),
        json!(u8::from(packet.sec_header_flag())),
    );
    values.insert("apid".into(), json!(packet.apid().value()));
    values.insert(
        "sequence_flags".into(),
        json!(packet.sequence_flags() as u8),
    );
    values.insert("sequence_count".into(), json!(packet.seq_count().value()));
    values.insert("packet_len".into(), json!(packet.packet_len()));
    values.insert("consumed".into(), json!(packet.packet_len()));
    values.insert("data_len".into(), json!(packet.packet_data().len()));
    values.insert("data_hex".into(), json!(hex::encode(packet.packet_data())));
    values
}

/// Whether `spacepackets` rejected the input as too short, its counterpart of `Incomplete`.
const fn is_too_short(err: &CcsdsPacketReadError) -> bool {
    matches!(
        err,
        CcsdsPacketReadError::ByteConversion(ByteConversionError::FromSliceTooSmall { .. })
    )
}

/// The skip for an expected error that `spacepackets` does not detect.
fn undetected(case: &CodecCase) -> Verdict {
    let reason = match case.expect.error.as_deref() {
        Some("UnsupportedVersion") => "spacepackets does not check the Packet Version Number",
        Some("InvalidField") => "spacepackets has no Idle Packet rules (SPP-004)",
        _ => "an error spacepackets does not detect",
    };
    Verdict::Skip(reason.to_owned())
}

fn decode(case: &CodecCase) -> Verdict {
    let octets = input(case);
    let result = CcsdsPacketReader::new(&octets, None);
    match (case.expect.ok, case.expect.error.as_deref(), result) {
        (true, _, Ok(packet)) => compare_produced(case, &packet_values(&packet)),
        (true, _, Err(e)) => Verdict::Disagree(format!("spacepackets rejects the packet: {e}")),
        (false, Some("Incomplete"), Err(e)) if is_too_short(&e) => Verdict::Agree,
        (false, Some("Incomplete"), Err(e)) => {
            Verdict::Disagree(format!("expected Incomplete, spacepackets reports {e}"))
        }
        (false, Some("Incomplete"), Ok(_)) => {
            Verdict::Disagree("expected Incomplete, spacepackets decodes a packet".to_owned())
        }
        (false, _, _) => undetected(case),
    }
}

fn peek_length(case: &CodecCase) -> Verdict {
    let octets = input(case);
    let result = SpHeader::from_be_bytes(&octets);
    match (case.expect.ok, case.expect.error.as_deref(), result) {
        (true, _, Ok((header, _))) => {
            let mut values = Map::new();
            values.insert("len".into(), json!(header.packet_len()));
            compare_produced(case, &values)
        }
        (true, _, Err(e)) => Verdict::Disagree(format!("spacepackets rejects the header: {e}")),
        (false, Some("Incomplete"), Err(ByteConversionError::FromSliceTooSmall { .. })) => {
            Verdict::Agree
        }
        (false, Some("Incomplete"), other) => Verdict::Disagree(format!(
            "expected Incomplete, spacepackets returns {other:?}"
        )),
        (false, _, _) => undetected(case),
    }
}

fn decode_stream(case: &CodecCase) -> Verdict {
    let stop = expected(case, "stop_error");
    if !(stop.is_null() || stop == "Incomplete") {
        return Verdict::Skip(format!(
            "the stream stops at {stop}, which spacepackets does not detect"
        ));
    }
    let octets = input(case);
    let mut packets = Vec::new();
    let mut offset = 0;
    let mut stop_error = Value::Null;
    while let Some(rest) = octets.get(offset..).filter(|rest| !rest.is_empty()) {
        match CcsdsPacketReader::new(rest, None) {
            Ok(packet) => {
                packets.push(json!({
                    "apid": packet.apid().value(),
                    "sequence_count": packet.seq_count().value(),
                    "consumed": packet.packet_len(),
                }));
                offset += packet.packet_len();
            }
            Err(e) if is_too_short(&e) => {
                stop_error = json!("Incomplete");
                break;
            }
            Err(e) => return Verdict::Disagree(format!("spacepackets stops at {offset}: {e}")),
        }
    }
    // `anomalies` comes from Aldis's idle rules, which spacepackets does not have.
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
            "packets: expected {}, produced {}",
            Value::from(want),
            Value::from(packets)
        ));
    }
    let mut values = Map::new();
    values.insert("stop_error".into(), stop_error);
    values.insert("offset".into(), json!(offset));
    match compare_produced(case, &values) {
        Verdict::Disagree(diff) => diffs.push(diff),
        Verdict::Agree | Verdict::Skip(_) => {}
    }
    if diffs.is_empty() {
        Verdict::Agree
    } else {
        Verdict::Disagree(diffs.join("; "))
    }
}

fn packet_type(case: &CodecCase) -> PacketType {
    match param_u64(case, "packet_type") {
        0 => PacketType::Tm,
        1 => PacketType::Tc,
        other => panic!("{}: packet_type {other}", case.id),
    }
}

fn apid(case: &CodecCase) -> u11 {
    u16::try_from(param_u64(case, "apid"))
        .ok()
        .and_then(|raw| u11::try_new(raw).ok())
        .unwrap_or_else(|| panic!("{}: apid out of range", case.id))
}

fn sequence_count(case: &CodecCase, key: &str) -> u14 {
    u16::try_from(param_u64(case, key))
        .ok()
        .and_then(|raw| u14::try_new(raw).ok())
        .unwrap_or_else(|| panic!("{}: {key} out of range", case.id))
}

fn sequence_flags(case: &CodecCase) -> SequenceFlags {
    match param_u64(case, "sequence_flags") {
        0b00 => SequenceFlags::ContinuationSegment,
        0b01 => SequenceFlags::FirstSegment,
        0b10 => SequenceFlags::LastSegment,
        0b11 => SequenceFlags::Unsegmented,
        other => panic!("{}: sequence_flags {other}", case.id),
    }
}

/// Builds a packet with `CcsdsPacketCreator` and compares it with `output_hex`.
fn compare_built(case: &CodecCase, header: SpHeader, ptype: PacketType, data: &[u8]) -> Verdict {
    match CcsdsPacketCreator::new(header, ptype, data, None) {
        Ok(creator) => {
            let mut values = Map::new();
            values.insert("output_hex".into(), json!(hex::encode(&creator.to_vec())));
            compare_produced(case, &values)
        }
        Err(e) => Verdict::Disagree(format!("spacepackets rejects the packet: {e}")),
    }
}

fn encode(case: &CodecCase) -> Verdict {
    let mut data = param_hex(case, "secondary_header_hex");
    let has_secondary_header = !data.is_empty();
    data.extend(param_hex(case, "user_data_hex"));
    let limited = case
        .config
        .as_ref()
        .is_some_and(|config| config.contains_key("max_packet_len"));
    if !case.expect.ok {
        // spacepackets has only the Packet Data Length limit. Its creator underflows on an
        // empty data field, so that case is never passed to it.
        if data.len() <= MAX_DATA_FIELD_LEN || limited {
            return Verdict::Skip(
                "a rejection rule spacepackets does not have (empty data field, idle APID, \
                 max_packet_len)"
                    .to_owned(),
            );
        }
        let header = SpHeader::new_from_fields(
            packet_type(case),
            has_secondary_header,
            apid(case),
            sequence_flags(case),
            sequence_count(case, "sequence_count"),
            0,
        );
        return match CcsdsPacketCreator::new(header, packet_type(case), &data, None) {
            Err(_) => Verdict::Agree,
            Ok(_) => Verdict::Disagree(format!(
                "spacepackets accepts a {}-octet data field",
                data.len()
            )),
        };
    }
    let header = SpHeader::new_from_fields(
        packet_type(case),
        has_secondary_header,
        apid(case),
        sequence_flags(case),
        sequence_count(case, "sequence_count"),
        0,
    );
    compare_built(case, header, packet_type(case), &data)
}

fn octet_string(case: &CodecCase) -> Verdict {
    if !case.expect.ok {
        return Verdict::Skip("spacepackets has no Octet String service rules".to_owned());
    }
    let indicator = param(case, "secondary_header_indicator")
        .as_bool()
        .unwrap_or_else(|| panic!("{}: secondary_header_indicator", case.id));
    // 133.0 §4.2.2: the Octet String service always sends Sequence Flags 11.
    let header = SpHeader::new_from_fields(
        packet_type(case),
        indicator,
        apid(case),
        SequenceFlags::Unsegmented,
        sequence_count(case, "sequence_count"),
        0,
    );
    compare_built(
        case,
        header,
        packet_type(case),
        &param_hex(case, "octets_hex"),
    )
}

/// Decodes `output` with `spacepackets` and compares the listed header fields.
fn decoded_fields(output: &[u8], want: &[(&str, u64)]) -> Result<(), String> {
    let packet =
        CcsdsPacketReader::new(output, None).map_err(|e| format!("spacepackets rejects: {e}"))?;
    let got = packet_values(&packet);
    let diffs: Vec<String> = want
        .iter()
        .filter_map(|(key, value)| match got.get(*key) {
            None => Some(format!("{key}: expected {value}, not produced")),
            Some(produced) if *produced != json!(value) => {
                Some(format!("{key}: expected {value}, produced {produced}"))
            }
            Some(_) => None,
        })
        .collect();
    if packet.packet_len() != output.len() {
        return Err(format!(
            "spacepackets reads {} of {} octets",
            packet.packet_len(),
            output.len()
        ));
    }
    if diffs.is_empty() {
        Ok(())
    } else {
        Err(diffs.join("; "))
    }
}

fn hex_value(case: &CodecCase, value: &Value) -> Vec<u8> {
    value
        .as_str()
        .and_then(|text| hex::decode(text).ok())
        .unwrap_or_else(|| panic!("{}: expected octets are not hex", case.id))
}

fn idle(case: &CodecCase) -> Verdict {
    if !case.expect.ok {
        return Verdict::Skip("spacepackets has no Idle Packet builder".to_owned());
    }
    let output = hex_value(case, expected(case, "output_hex"));
    // APID 2047 (133.0 §4.1.3.3.4.4), no secondary header (§4.1.3.3.3.4). 133.0 sets no Sequence
    // Flags for Idle Packets; `11` is the SPP-010 choice the vectors encode.
    let want = [
        ("apid", 2047),
        ("secondary_header_flag", 0),
        ("sequence_flags", 0b11),
        ("sequence_count", param_u64(case, "sequence_count")),
        ("packet_len", param_u64(case, "total_len")),
    ];
    match decoded_fields(&output, &want) {
        Ok(()) => Verdict::Agree,
        Err(diff) => Verdict::Disagree(diff),
    }
}

fn segment(case: &CodecCase) -> Verdict {
    if !case.expect.ok {
        return Verdict::Skip("spacepackets has no segmentation rules".to_owned());
    }
    let first = u64::from(sequence_count(case, "sequence_count").value());
    let has_secondary_header = !param_hex(case, "secondary_header_hex").is_empty();
    let packets = expected(case, "packets_hex")
        .as_array()
        .unwrap_or_else(|| panic!("{}: packets_hex is not an array", case.id));
    let mut diffs = Vec::new();
    for (index, packet) in (0_u64..).zip(packets) {
        // The count advances by one per packet, modulo 2^14 (133.0 §4.1.3.4.3.4).
        let want = [
            ("packet_type", param_u64(case, "packet_type")),
            ("secondary_header_flag", u64::from(has_secondary_header)),
            ("apid", param_u64(case, "apid")),
            ("sequence_count", (first + index) % 16_384),
        ];
        if let Err(diff) = decoded_fields(&hex_value(case, packet), &want) {
            diffs.push(format!("packet {index}: {diff}"));
        }
    }
    if diffs.is_empty() {
        Verdict::Agree
    } else {
        Verdict::Disagree(diffs.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::decoded_fields;

    /// Telemetry, APID 5, unsegmented, count 7, one data octet.
    const PACKET: [u8; 7] = [0x00, 0x05, 0xC0, 0x07, 0x00, 0x00, 0xAB];

    #[test]
    fn decoded_fields_reports_every_difference() {
        // req: VER-040
        let want = [
            ("apid", 5),
            ("sequence_flags", 0b11),
            ("sequence_count", 7),
            ("packet_len", 7),
        ];
        assert_eq!(decoded_fields(&PACKET, &want), Ok(()));
        assert_eq!(
            decoded_fields(&PACKET, &[("apid", 6), ("sequence_count", 7)]),
            Err("apid: expected 6, produced 5".to_owned())
        );
        assert_eq!(
            decoded_fields(&PACKET, &[("no_such_field", 1)]),
            Err("no_such_field: expected 1, not produced".to_owned())
        );
        let mut longer = PACKET.to_vec();
        longer.push(0);
        assert_eq!(
            decoded_fields(&longer, &want),
            Err("spacepackets reads 7 of 8 octets".to_owned())
        );
        assert!(
            decoded_fields(&PACKET[..6], &want)
                .unwrap_err()
                .starts_with("spacepackets rejects")
        );
    }
}

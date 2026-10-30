//! Arbitrary fields, secondary header, user data, and Maximum Packet Length through `encode`,
//! the two-phase writer, the Octet String encoder, and the idle builder, then `decode`:
//! `decode(encode(x)) == x`, the encoders agree, and a rejected encode leaves the output buffer
//! unchanged (SPP-005, SPP-006, SPP-010, SPP-012, API-020…API-023, GEN-042).
#![no_main]

// req: SPP-005, SPP-006, SPP-007, SPP-010, SPP-012, API-020, API-021, API-022, API-023, GEN-042, VER-022

use core::num::NonZeroUsize;

use aldis_core::ids::{Apid, SeqCount};
use aldis_spp::{
    IdlePacket, OctetStringEncoder, PacketFields, PacketType, SecondaryHeaderLayout,
    SequenceCounter, SequenceFlags, SpacePacket, SpacePacketWriter, SppConfig, encode,
};
use libfuzzer_sys::arbitrary::{Result, Unstructured};
use libfuzzer_sys::fuzz_target;

/// One packet through `encode`, `decode`, `split`, and the two-phase writer.
fn check_packet(config: SppConfig, fields: PacketFields, sec: &[u8], user: &[u8], cap: usize) {
    let mut out = vec![0xA5_u8; cap];
    let mut two = vec![0x5A_u8; cap];
    let Ok(packet) = config.packet(fields, sec, user) else {
        let total = 6 + sec.len() + user.len();
        assert!(
            total == 6
                || total > config.max_packet_len()
                || (fields.apid.is_idle() && !sec.is_empty())
        );
        assert!(
            SpacePacketWriter::begin(&mut two, &config, fields, sec.len(), user.len()).is_err()
        );
        assert!(
            two.iter().all(|&b| b == 0x5A),
            "rejected begin wrote output"
        );
        return;
    };
    let Ok(len) = encode(&packet, &mut out) else {
        assert!(
            out.iter().all(|&b| b == 0xA5),
            "rejected encode wrote output"
        );
        return;
    };
    let decoded = SpacePacket::decode(&out).unwrap();
    assert_eq!((decoded.len(), decoded.header().fields()), (len, fields));
    let layout = NonZeroUsize::new(sec.len())
        .map_or(SecondaryHeaderLayout::Absent, SecondaryHeaderLayout::Opaque);
    let split = decoded.split(&layout).unwrap();
    assert_eq!((split.secondary_header(), split.user_data()), (sec, user));
    let mut writer =
        SpacePacketWriter::begin(&mut two, &config, fields, sec.len(), user.len()).unwrap();
    writer.secondary_header_mut().copy_from_slice(sec);
    writer.user_data_mut().copy_from_slice(user);
    assert_eq!(writer.finish(user.len()), Ok(len));
    assert_eq!(two[..len], out[..len]);
}

/// The Octet String encoder: flags 11, and the counter advances only on success.
fn check_octet(
    config: SppConfig,
    fields: PacketFields,
    indicator: bool,
    octets: &[u8],
    cap: usize,
) {
    let counter = SequenceCounter::new(fields.sequence_count);
    let Ok(mut enc) = OctetStringEncoder::new(&config, fields.packet_type, fields.apid, counter)
    else {
        assert!(fields.apid.is_idle());
        return;
    };
    let mut buf = vec![0xA5_u8; cap];
    match enc.encode(indicator, octets, &mut buf) {
        Err(_) => {
            assert_eq!(enc.counter(), counter);
            assert!(buf.iter().all(|&b| b == 0xA5));
        }
        Ok(len) => {
            let decoded = SpacePacket::decode(&buf).unwrap();
            assert_eq!(
                (
                    decoded.len(),
                    decoded.sequence_flags(),
                    decoded.data_field()
                ),
                (len, SequenceFlags::Unsegmented, octets)
            );
            assert_eq!(decoded.has_secondary_header(), indicator);
            assert_eq!(enc.counter().peek(), fields.sequence_count.next());
        }
    }
}

/// Idle Packets of any allowed length decode as idle, with the pattern repeated.
fn check_idle(config: SppConfig, count: SeqCount, pattern: &[u8], total: usize, cap: usize) {
    let Ok(idle) = IdlePacket::new(&config, total) else {
        assert!(total < 7 || total > config.max_packet_len());
        return;
    };
    let idle = idle.with_pattern(pattern).with_sequence_count(count);
    let mut buf = vec![0_u8; cap];
    if let Ok(len) = idle.encode(&mut buf) {
        let decoded = SpacePacket::decode(&buf[..len]).unwrap();
        assert!(decoded.is_idle() && !decoded.has_secondary_header() && decoded.len() == total);
        let fill: &[u8] = if pattern.is_empty() { &[0] } else { pattern };
        assert!(
            decoded
                .data_field()
                .iter()
                .zip(fill.iter().cycle())
                .all(|(a, b)| a == b)
        );
    }
}

fn run(input: &mut Unstructured<'_>) -> Result<()> {
    let fields = PacketFields {
        packet_type: if input.arbitrary()? {
            PacketType::Telecommand
        } else {
            PacketType::Telemetry
        },
        apid: Apid::new(input.int_in_range(0..=Apid::MAX)?).unwrap(),
        sequence_flags: SequenceFlags::from_bits(input.arbitrary()?),
        sequence_count: SeqCount::new(input.int_in_range(0..=SeqCount::MAX)?).unwrap(),
    };
    let config = SppConfig::new(input.int_in_range(7..=400)?).unwrap();
    let sec_len = usize::from(input.int_in_range(0_u16..=40)?);
    let sec = input.bytes(sec_len.min(input.len()))?.to_vec();
    let user_len = usize::from(input.int_in_range(0_u16..=400)?);
    let user = input.bytes(user_len.min(input.len()))?.to_vec();
    let cap = usize::from(input.int_in_range(0_u16..=460)?);
    let total = usize::from(input.int_in_range(0_u16..=420)?);
    check_packet(config, fields, &sec, &user, cap);
    check_octet(config, fields, !sec.is_empty(), &user, cap);
    check_idle(config, fields.sequence_count, &sec, total, cap);
    Ok(())
}

fuzz_target!(|data: &[u8]| {
    let _ = run(&mut Unstructured::new(data));
});

//! Kani proof harnesses (spike, docs/verification/formal-proofs.md, OI-41).
//!
//! Compiled only by `cargo kani` (`cfg(kani)`); not part of the library, the tests, or the
//! coverage run. The expected outcomes are written from the wire format table and the
//! requirement table of docs/standards/space-packet.md, not from the decoder.

use aldis_core::ids::{Apid, SeqCount};
use aldis_core::{DecodeMode, ErrorDetail, ErrorKind};

use crate::anomaly::SppAnomaly;
use crate::config::SppConfig;
use crate::field;
use crate::header::{PacketFields, PacketType, SequenceFlags, SpacePacketHeader};
use crate::view::SpacePacket;
use crate::writer::encode;

/// The fields of a 6-octet primary header by their bit positions in the wire format table:
/// version bits 0–2, type bit 3, flag bit 4, APID bits 5–15, sequence flags bits 16–17, count
/// bits 18–31, Packet Data Length bits 32–47.
struct Wire {
    version: u8,
    telecommand: bool,
    secondary: bool,
    apid: u16,
    flags: u8,
    count: u16,
    length: u16,
}

fn wire(h: [u8; 6]) -> Wire {
    let w = u64::from(h[0]) << 40
        | u64::from(h[1]) << 32
        | u64::from(h[2]) << 24
        | u64::from(h[3]) << 16
        | u64::from(h[4]) << 8
        | u64::from(h[5]);
    let field = |first: u32, width: u32| (w >> (48 - first - width)) & ((1 << width) - 1);
    Wire {
        version: field(0, 3) as u8,
        telecommand: field(3, 1) == 1,
        secondary: field(4, 1) == 1,
        apid: field(5, 11) as u16,
        flags: field(16, 2) as u8,
        count: field(18, 14) as u16,
        length: field(32, 16) as u16,
    }
}

/// Every one of the 2⁴⁸ primary headers: the decoder rejects exactly the versions other than
/// `000` (SPP-001), reads every field at its position, computes the lengths of SPP-002, and
/// re-encodes every accepted header to the same 6 octets. `peek_length` agrees (SPP-003).
#[kani::proof]
fn primary_header_every_input() {
    // req: SPP-001, SPP-002, SPP-003, SPP-009, API-014, GEN-020
    let bytes: [u8; 6] = kani::any();
    let w = wire(bytes);
    let peek = SpacePacket::peek_length(&bytes);
    match SpacePacketHeader::parse(&bytes) {
        Err(e) => {
            assert!(w.version != 0);
            assert_eq!(e.kind(), ErrorKind::UnsupportedVersion);
            assert_eq!(e.field(), Some(field::PACKET_VERSION));
            assert_eq!(
                e.detail(),
                ErrorDetail::Expected {
                    expected: 0,
                    found: u64::from(w.version),
                }
            );
            assert_eq!(peek, Err(e));
        }
        Ok(h) => {
            assert_eq!(w.version, 0);
            let packet_type = if w.telecommand {
                PacketType::Telecommand
            } else {
                PacketType::Telemetry
            };
            assert_eq!(h.packet_type(), packet_type);
            assert_eq!(h.has_secondary_header(), w.secondary);
            assert_eq!(h.apid().get(), w.apid);
            assert_eq!(h.is_idle(), w.apid == 2047);
            assert_eq!(h.sequence_flags().bits(), w.flags);
            assert_eq!(h.sequence_count().get(), w.count);
            assert_eq!(h.packet_name().is_some(), w.telecommand);
            assert_eq!(h.data_len(), usize::from(w.length) + 1);
            assert_eq!(h.packet_len(), usize::from(w.length) + 7);
            assert_eq!(peek, Ok(usize::from(w.length) + 7));
            assert_eq!(h.octets(), bytes);
        }
    }
}

/// Longest input of the decode harness: one octet past the shortest packet.
const DECODE_MAX: usize = 8;

/// `SpacePacket::decode` on every input of 0–8 octets, in both modes: the outcome is exactly the
/// one SPP-001, SPP-002 and SPP-004 prescribe, in the order the decoder documents (header
/// `Incomplete`, version, packet `Incomplete`, idle flag). Every header is covered; the bound is
/// on the octets after it, so Packet Data Lengths above 1 reach only the `Incomplete` branch.
/// Unwind 9: the slice comparisons run over at most 8 octets. Without a bound CBMC kept
/// unrolling the comparison of the data field, whose length it cannot bound by itself.
#[kani::proof]
#[kani::unwind(9)]
fn decode_rejects_exactly_per_spec() {
    // req: SPP-001, SPP-002, SPP-004, API-011, API-032, GEN-020
    let buf: [u8; DECODE_MAX] = kani::any();
    let len: usize = kani::any();
    kani::assume(len <= DECODE_MAX);
    let lenient: bool = kani::any();
    let mode = if lenient {
        DecodeMode::Lenient
    } else {
        DecodeMode::Strict
    };
    let bytes = &buf[..len];
    let result = SpacePacket::decode(bytes, mode);
    if len < 6 {
        let e = result.unwrap_err();
        assert_eq!(
            (e.kind(), e.detail()),
            (ErrorKind::Incomplete, ErrorDetail::Needed(6))
        );
        return;
    }
    let w = wire([buf[0], buf[1], buf[2], buf[3], buf[4], buf[5]]);
    let total = usize::from(w.length) + 7;
    let idle_flag = w.apid == 2047 && w.secondary;
    match result {
        Err(e) if w.version != 0 => assert_eq!(e.kind(), ErrorKind::UnsupportedVersion),
        Err(e) if len < total => assert_eq!(
            (e.kind(), e.detail()),
            (ErrorKind::Incomplete, ErrorDetail::Needed(total))
        ),
        Err(e) => {
            assert!(idle_flag && !lenient);
            assert_eq!(
                (e.kind(), e.field()),
                (ErrorKind::InvalidField, Some(field::SECONDARY_HEADER_FLAG))
            );
        }
        Ok(p) => {
            assert!(w.version == 0 && len >= total && !(idle_flag && !lenient));
            assert_eq!(p.len(), total);
            assert_eq!(p.data_field(), &buf[6..total]);
            assert_eq!(
                p.anomalies().contains(SppAnomaly::IdleWithSecondaryHeader),
                idle_flag
            );
        }
    }
}

/// Symbolic sender-chosen header fields: every packet type, APID, flag, and count.
fn any_fields() -> PacketFields {
    let apid = Apid::new(kani::any());
    let count = SeqCount::new(kani::any());
    kani::assume(apid.is_some() && count.is_some());
    PacketFields {
        packet_type: if kani::any() {
            PacketType::Telecommand
        } else {
            PacketType::Telemetry
        },
        apid: apid.unwrap_or(Apid::IDLE),
        sequence_flags: SequenceFlags::from_bits(kani::any()),
        sequence_count: count.unwrap_or_default(),
    }
}

/// Every valid header field combination and data field length 1–65 536: encode then decode gives
/// the same header, with version `000`.
#[kani::proof]
fn header_encode_decode_every_field() {
    // req: SPP-001, SPP-002, SPP-009, API-014, API-021, GEN-020
    let fields = any_fields();
    let secondary: bool = kani::any();
    let data_len: usize = kani::any();
    kani::assume((1..=65_536).contains(&data_len));
    let header = SpacePacketHeader::new(fields, secondary, data_len);
    let octets = header.octets();
    assert_eq!(octets[0] >> 5, 0);
    let back = SpacePacketHeader::parse(&octets);
    assert_eq!(back, Ok(header));
    if let Ok(h) = back {
        assert_eq!(h.fields(), fields);
        assert_eq!(h.has_secondary_header(), secondary);
        assert_eq!(h.data_len(), data_len);
    }
}

/// Longest secondary header and user data field of the packet harness.
const PART_MAX: usize = 3;
/// Output buffer of the packet harness: header and both parts.
const OUT_MAX: usize = 6 + 2 * PART_MAX;

/// Whole packets with secondary headers and user data of 0–3 octets each, every header field,
/// every Maximum Packet Length, and every output length up to 12: `SppConfig::packet` accepts
/// exactly what SPP-005, SPP-006 and §4.1.3.3.3.4 allow, `encode` writes exactly the packet or
/// leaves the buffer unchanged (API-022), and a strict decode gives the parts back. Unwind 13:
/// the copy loop runs at most 12 times.
#[kani::proof]
#[kani::unwind(13)]
fn packet_encode_decode() {
    // req: SPP-002, SPP-005, SPP-006, API-020, API-021, API-022, GEN-020
    let fields = any_fields();
    let max: usize = kani::any();
    let Ok(config) = SppConfig::new(max) else {
        assert!(!(7..=65_542).contains(&max));
        return;
    };
    let sec_buf: [u8; PART_MAX] = kani::any();
    let user_buf: [u8; PART_MAX] = kani::any();
    let (sec_len, user_len): (usize, usize) = (kani::any(), kani::any());
    kani::assume(sec_len <= PART_MAX && user_len <= PART_MAX);
    let (sec, user) = (&sec_buf[..sec_len], &user_buf[..user_len]);
    let data_len = sec_len + user_len;
    let total = 6 + data_len;
    let allowed = data_len > 0 && !(fields.apid.is_idle() && sec_len > 0) && total <= max;
    let packet = config.packet(fields, sec, user);
    assert_eq!(packet.is_ok(), allowed);
    let Ok(packet) = packet else { return };

    let original: [u8; OUT_MAX] = kani::any();
    let mut out = original;
    let out_len: usize = kani::any();
    kani::assume(out_len <= OUT_MAX);
    match encode(&packet, &mut out[..out_len]) {
        Err(e) => {
            assert!(out_len < total);
            assert_eq!(
                (e.kind(), e.detail()),
                (ErrorKind::BufferTooSmall, ErrorDetail::Needed(total))
            );
            assert_eq!(out, original);
        }
        Ok(n) => {
            assert_eq!(n, total);
            assert_eq!(&out[total..], &original[total..]);
            let p = SpacePacket::decode(&out[..out_len], DecodeMode::Strict).unwrap();
            assert_eq!(p.header().fields(), fields);
            assert_eq!(p.has_secondary_header(), sec_len > 0);
            assert_eq!(p.len(), total);
            assert_eq!(&p.data_field()[..sec_len], sec);
            assert_eq!(&p.data_field()[sec_len..], user);
        }
    }
}

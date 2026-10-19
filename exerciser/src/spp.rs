//! Calls every public `aldis-spp` API (VER-010, M1 D7). Inputs pass through `black_box`.

use core::hint::black_box as bb;
use core::num::NonZeroUsize;

use aldis_core::counter::Continuity;
use aldis_core::ids::{Apid, SeqCount};
use aldis_core::{DecodeMode, ErrorKind};
use aldis_spp::{
    IdlePacket, MAX_PACKET_LEN, MIN_PACKET_LEN, OctetStringEncoder, PRIMARY_HEADER_LEN,
    PacketFields, PacketType, SecondaryHeaderLayout, SequenceCounter, SequenceFlags,
    SequenceTracker, SpacePacket, SpacePacketWriter, SppAnomaly, SppConfig, Tracked, encode,
};

use crate::exercise::Tally;

fn fields(apid: u16, count: u16) -> Option<PacketFields> {
    Some(PacketFields {
        packet_type: PacketType::Telecommand,
        apid: Apid::new(apid)?,
        sequence_flags: SequenceFlags::from_bits(bb(0b01)),
        sequence_count: SeqCount::new(count)?,
    })
}

fn decode(t: &mut Tally) {
    let bytes = bb([0x18, 0x23, 0x40, 0x07, 0x00, 0x02, 0xA0, 0xB0, 0xB1, 0xEE]);
    match SpacePacket::decode(&bytes, DecodeMode::Strict) {
        Ok(p) => {
            t.check(p.packet_type() == PacketType::Telecommand && p.apid().get() == 0x023);
            t.check(p.sequence_flags() == SequenceFlags::First && p.sequence_flags().bits() == 1);
            t.check(p.sequence_count().get() == 7 && p.packet_name() == SeqCount::new(7));
            t.check(p.has_secondary_header() && !p.is_idle() && !p.is_empty() && p.len() == 9);
            t.check(p.data_field() == [0xA0, 0xB0, 0xB1] && p.anomalies().is_empty());
            let h = p.header();
            t.check(h.packet_len() == 9 && h.data_len() == 3 && h.has_secondary_header());
            t.check(h.octets() == [0x18, 0x23, 0x40, 0x07, 0x00, 0x02] && !h.is_idle());
            t.check(h.fields().apid == h.apid() && h.packet_type() == PacketType::Telecommand);
            t.check(h.sequence_flags() == SequenceFlags::First && h.packet_name().is_some());
            t.check(h.sequence_count().get() == 7);
            let layout = NonZeroUsize::new(bb(1))
                .map_or(SecondaryHeaderLayout::Absent, SecondaryHeaderLayout::Opaque);
            t.check(
                p.split(&layout)
                    .is_ok_and(|s| s.secondary_header() == [0xA0] && s.user_data() == [0xB0, 0xB1]),
            );
            t.check(
                p.split(&SecondaryHeaderLayout::Absent)
                    .is_err_and(|e| e.kind() == ErrorKind::InvalidField),
            );
        }
        Err(_) => t.check(false),
    }
    t.check(SpacePacket::peek_length(bb(&bytes[..6])) == Ok(9));
    t.check(
        SpacePacket::peek_length(bb(&[0xE0; 6]))
            .is_err_and(|e| e.kind() == ErrorKind::UnsupportedVersion),
    );
    let idle = bb([0x0F, 0xFF, 0xC0, 0x00, 0x00, 0x00, 0x00]);
    t.check(
        SpacePacket::decode(&idle, DecodeMode::Strict)
            .is_err_and(|e| e.kind() == ErrorKind::InvalidField),
    );
    t.check(
        SpacePacket::decode(&idle, DecodeMode::Lenient).is_ok_and(|p| {
            p.anomalies().contains(SppAnomaly::IdleWithSecondaryHeader) && p.is_idle()
        }),
    );
    t.check(
        SpacePacket::decode(bb(&idle[..6]), DecodeMode::Strict)
            .is_err_and(|e| e.kind() == ErrorKind::Incomplete),
    );
}

fn encoders(t: &mut Tally) {
    t.check(PRIMARY_HEADER_LEN == 6 && MIN_PACKET_LEN == 7 && MAX_PACKET_LEN == 65_542);
    t.check(SppConfig::default() == SppConfig::DEFAULT && SppConfig::new(bb(6)).is_err());
    let (Ok(cfg), Some(f)) = (SppConfig::new(bb(16)), fields(0x123, 5)) else {
        t.check(false);
        return;
    };
    t.check(cfg.max_packet_len() == 16);
    let mut out = [0_u8; 16];
    match cfg.packet(f, bb(&[0xA0]), bb(&[1, 2])) {
        Ok(p) => {
            t.check(p.fields() == f && p.secondary_header() == [0xA0] && p.user_data() == [1, 2]);
            t.check(p.packet_len() == 9);
            t.check(
                encode(&p, &mut out).is_ok_and(|n| n == 9 && out.get(..2) == Some(&[0x19, 0x23])),
            );
            t.check(
                encode(&p, bb(&mut out[..8])).is_err_and(|e| e.kind() == ErrorKind::BufferTooSmall),
            );
        }
        Err(_) => t.check(false),
    }
    t.check(
        cfg.packet(f, &[], &[])
            .is_err_and(|e| e.kind() == ErrorKind::InvalidField),
    );
    let mut two = [0_u8; 16];
    match SpacePacketWriter::begin(&mut two, &cfg, f, bb(1), bb(4)) {
        Ok(mut w) => {
            if let Some(s) = w.secondary_header_mut().first_mut() {
                *s = 0xA0;
            }
            for (d, s) in w.user_data_mut().iter_mut().zip([1, 2]) {
                *d = s;
            }
            t.check(w.finish(bb(2)) == Ok(9) && two.get(..9) == out.get(..9));
        }
        Err(_) => t.check(false),
    }
    t.check(SpacePacketWriter::begin(&mut two, &cfg, f, 0, 0).is_err());
}

fn idle_and_octets(t: &mut Tally) {
    let cfg = SppConfig::DEFAULT;
    let mut out = [0_u8; 16];
    match IdlePacket::new(&cfg, bb(10)) {
        Ok(idle) => {
            let idle = idle
                .with_pattern(bb(&[0x55, 0xAA]))
                .with_sequence_count(SeqCount::new(3).unwrap_or_default());
            t.check(idle.total_len() == 10);
            t.check(
                idle.encode(&mut out)
                    .is_ok_and(|n| n == 10 && out.get(6..10) == Some(&[0x55, 0xAA, 0x55, 0xAA])),
            );
            t.check(idle.encode(bb(&mut out[..9])).is_err());
        }
        Err(_) => t.check(false),
    }
    t.check(IdlePacket::new(&cfg, bb(6)).is_err_and(|e| e.kind() == ErrorKind::OutOfRange));
    let Some(f) = fields(0x042, 0x3FFF) else {
        t.check(false);
        return;
    };
    let counter = SequenceCounter::new(f.sequence_count);
    t.check(OctetStringEncoder::new(&cfg, f.packet_type, Apid::IDLE, counter).is_err());
    match OctetStringEncoder::new(&cfg, f.packet_type, f.apid, counter) {
        Ok(mut enc) => {
            t.check(enc.encode(bb(false), bb(&[9, 8]), &mut out) == Ok(8));
            t.check(
                SpacePacket::decode(&out, DecodeMode::Strict).is_ok_and(|p| {
                    p.sequence_flags() == SequenceFlags::Unsegmented
                        && p.sequence_count().get() == 0x3FFF
                }),
            );
            t.check(enc.counter().peek().get() == 0 && enc.encode(true, &[], &mut out).is_err());
        }
        Err(_) => t.check(false),
    }
}

fn sequences(t: &mut Tally) {
    let mut counter = SequenceCounter::default();
    t.check(counter.peek().get() == 0 && counter.advance().get() == 0 && counter.peek().get() == 1);
    let mut tracker = SequenceTracker::<2>::default();
    let (Some(one), Some(two), Some(three)) =
        (Apid::new(bb(1)), Apid::new(bb(2)), Apid::new(bb(3)))
    else {
        t.check(false);
        return;
    };
    let count = |v: u16| SeqCount::new(v).unwrap_or_default();
    t.check(
        tracker.is_empty() && tracker.capacity() == 2 && SequenceTracker::<1>::new().is_empty(),
    );
    t.check(tracker.track(one, count(5)) == Ok(Tracked::First));
    t.check(tracker.track(one, count(6)) == Ok(Tracked::Next(Continuity::InSequence)));
    t.check(tracker.track(one, count(9)) == Ok(Tracked::Next(Continuity::Gap { missing: 2 })));
    t.check(tracker.track(two, count(0)) == Ok(Tracked::First) && tracker.len() == 2);
    t.check(
        tracker
            .track(three, count(0))
            .is_err_and(|e| e.kind() == ErrorKind::CapacityExceeded),
    );
    t.check(tracker.last(one) == Some(count(9)) && tracker.forget(one) && !tracker.forget(one));
    t.check(tracker.last(one).is_none() && tracker.track(three, count(1)) == Ok(Tracked::First));
}

/// Runs the `aldis-spp` group.
pub fn run(t: &mut Tally) {
    decode(t);
    encoders(t);
    idle_and_octets(t);
    sequences(t);
}

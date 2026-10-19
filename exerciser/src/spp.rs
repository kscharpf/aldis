//! Calls every public `aldis-spp` API (VER-010, M1 D7). Inputs pass through `black_box`.

use core::hint::black_box as bb;
use core::num::NonZeroUsize;

use aldis_core::counter::Continuity;
use aldis_core::ids::{Apid, SeqCount};
use aldis_core::{DecodeMode, ErrorKind};
use aldis_spp::{
    Discard, IdlePacket, MAX_PACKET_LEN, MIN_PACKET_LEN, OctetStringEncoder, Outcome,
    PRIMARY_HEADER_LEN, PacketFields, PacketType, Reassembler, SecondaryHeaderLayout, Segmenter,
    SequenceCounter, SequenceFlags, SequenceTracker, SpacePacket, SpacePacketWriter, SppAnomaly,
    SppConfig, Status, TimeHeaderLayout, Tracked, encode,
};
use aldis_time::{CcsFormat, CcsVariation, CucEpoch, CucFormat, TimeFormat};

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

/// Encodes one packet with `user` data, decodes it, and pushes it into `ra`.
fn push_one(
    ra: &mut Reassembler<'_>,
    cfg: SppConfig,
    fields: PacketFields,
    user: &[u8],
) -> Option<Outcome> {
    let packet = cfg.packet(fields, &[], user).ok()?;
    let mut out = [0_u8; 16];
    let n = encode(&packet, &mut out).ok()?;
    let p = SpacePacket::decode(out.get(..n)?, DecodeMode::Strict).ok()?;
    ra.push(&p.header(), p.data_field()).ok()
}

fn segments(t: &mut Tally) {
    let cfg = SppConfig::DEFAULT;
    let Some(apid) = Apid::new(bb(0x050)) else {
        t.check(false);
        return;
    };
    t.check(Segmenter::new(&cfg, PacketType::Telemetry, apid, bb(&[]), 4).is_err());
    t.check(Segmenter::new(&cfg, PacketType::Telemetry, Apid::IDLE, bb(&[1]), 4).is_err());
    let sdu = bb([1_u8, 2, 3, 4, 5]);
    let mut buf = [0_u8; 8];
    let mut ra = Reassembler::new(apid, &mut buf);
    t.check(ra.apid() == apid && ra.capacity() == 8 && !ra.is_assembling());
    match Segmenter::new(&cfg, PacketType::Telemetry, apid, &sdu, bb(2)) {
        Ok(mut seg) => {
            let mut counter = SequenceCounter::default();
            let mut out = [0_u8; 16];
            let mut last = None;
            while let Ok(Some(n)) = seg.next_packet(&mut counter, &[], &mut out) {
                if let Ok(p) =
                    SpacePacket::decode(bb(out.get(..n).unwrap_or_default()), DecodeMode::Strict)
                {
                    last = ra.push(&p.header(), p.data_field()).ok();
                }
            }
            t.check(seg.is_done() && seg.remaining() == 0 && counter.peek().get() == 3);
            t.check(
                last == Some(Outcome {
                    discarded: None,
                    status: Status::Complete(5),
                }),
            );
            t.check(ra.sdu() == sdu);
        }
        Err(_) => t.check(false),
    }
    let fields = |sequence_flags| PacketFields {
        packet_type: PacketType::Telemetry,
        apid,
        sequence_flags,
        sequence_count: SeqCount::default(),
    };
    let orphan = push_one(&mut ra, cfg, fields(SequenceFlags::Continuation), bb(&[9]));
    t.check(orphan.is_some_and(|o| o.status == Status::Dropped(Discard::Orphan)));
    let first = push_one(&mut ra, cfg, fields(SequenceFlags::First), bb(&[8]));
    t.check(first.is_some_and(|o| o.status == Status::InProgress) && ra.is_assembling());
    ra.reset();
    t.check(!ra.is_assembling() && ra.sdu().is_empty());
}

fn time_header(t: &mut Tally) {
    let Ok(cuc) = CucFormat::new(CucEpoch::Tai1958, bb(4), bb(2)) else {
        t.check(false);
        return;
    };
    let format = TimeFormat::Cuc(cuc);
    match TimeHeaderLayout::new(format, bb(true), bb(1)) {
        Ok(layout) => {
            t.check(
                layout.format() == format && layout.has_pfield() && layout.ancillary_len() == 1,
            );
            // Secondary header: CUC 4+2 P-field 0x1E, T-field, 1 ancillary octet; then user data.
            let bytes = bb([
                0x08, 0x60, 0xC0, 0x00, 0x00, 0x08, 0x1E, 0x12, 0x34, 0x56, 0x78, 0x80, 0x00, 0xAB,
                0x01,
            ]);
            match SpacePacket::decode(&bytes, DecodeMode::Strict) {
                Ok(p) => {
                    let split = p.split(&SecondaryHeaderLayout::Time(layout));
                    t.check(split.is_ok_and(|s| {
                        s.ancillary_data() == [0xAB]
                            && s.user_data() == [0x01]
                            && s.time_code()
                                .is_some_and(|c| c.len() == 7 && c.pfield().is_some())
                    }));
                }
                Err(_) => t.check(false),
            }
        }
        Err(_) => t.check(false),
    }
    let ccs = CcsFormat::new(CcsVariation::MonthDay, 0).map(TimeFormat::Ccs);
    t.check(ccs.is_ok_and(|f| {
        TimeHeaderLayout::new(f, false, 0).is_err_and(|e| e.kind() == ErrorKind::InvalidConfig)
    }));
}

/// Runs the `aldis-spp` group.
pub fn run(t: &mut Tally) {
    decode(t);
    encoders(t);
    idle_and_octets(t);
    sequences(t);
    segments(t);
    time_header(t);
}

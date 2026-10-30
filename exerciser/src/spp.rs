//! Calls every public `aldis-spp` API (VER-010, M1 D7) and runs every `test-vectors/spp/` case
//! (`vector`). Inputs pass through `black_box`.

use core::hint::black_box as bb;
use core::num::NonZeroUsize;

use aldis_core::counter::Continuity;
use aldis_core::ids::{Apid, SeqCount};
use aldis_core::{AnomalySet, DecodeMode, ErrorKind};
use aldis_spp::{
    Discard, IdlePacket, MAX_PACKET_LEN, MIN_PACKET_LEN, OctetStringEncoder, Outcome,
    PRIMARY_HEADER_LEN, PacketFields, PacketType, Reassembler, ReassemblerState,
    SecondaryHeaderLayout, Segmenter, SequenceCounter, SequenceFlags, SequenceTracker, SpacePacket,
    SpacePacketWriter, SppAnomaly, SppConfig, Status, TimeHeaderLayout, Tracked, encode,
};
use aldis_time::{
    CcsFormat, CcsVariation, CdsDayLen, CdsEpoch, CdsFormat, CdsSubMs, CucEpoch, CucFormat,
    TimeFormat,
};

use crate::exercise::Tally;
use crate::vectors::{Case, Fail, Map, Out, Params as _, Run, Scratch, Val, copy, fresh, need};

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
    t.check(SpacePacket::decode_lenient(&bytes).is_ok_and(|l| l.anomalies().is_empty()));
    match SpacePacket::decode(&bytes) {
        Ok(p) => {
            t.check(p.packet_type() == PacketType::Telecommand && p.apid().get() == 0x023);
            t.check(p.sequence_flags() == SequenceFlags::First && p.sequence_flags().bits() == 1);
            t.check(p.sequence_count().get() == 7 && p.packet_name() == SeqCount::new(7));
            t.check(p.has_secondary_header() && !p.is_idle() && !p.is_empty() && p.len() == 9);
            t.check(p.data_field() == [0xA0, 0xB0, 0xB1]);
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
    t.check(SpacePacket::decode(&idle).is_err_and(|e| e.kind() == ErrorKind::InvalidField));
    t.check(
        SpacePacket::decode_lenient(&idle).is_ok_and(|p| {
            p.anomalies().contains(SppAnomaly::IdleWithSecondaryHeader) && p.is_idle()
        }),
    );
    t.check(SpacePacket::decode(bb(&idle[..6])).is_err_and(|e| e.kind() == ErrorKind::Incomplete));
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
            t.check(SpacePacket::decode(&out).is_ok_and(|p| {
                p.sequence_flags() == SequenceFlags::Unsegmented
                    && p.sequence_count().get() == 0x3FFF
            }));
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
    let p = SpacePacket::decode(out.get(..n)?).ok()?;
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
                if let Ok(p) = SpacePacket::decode(bb(out.get(..n).unwrap_or_default())) {
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
    suspended(t, cfg, apid, &sdu);
}

/// The buffer-free states of the segmenter and the reassembler (SPP-013, API-060).
fn suspended(t: &mut Tally, cfg: SppConfig, apid: Apid, sdu: &[u8]) {
    let Ok(seg) = Segmenter::new(&cfg, PacketType::Telemetry, apid, sdu, bb(3)) else {
        t.check(false);
        return;
    };
    let mut state = seg.suspend();
    t.check(state.remaining() == sdu.len() && !state.is_done());
    t.check(state.resume(bb(&[])).is_err());
    let mut buf = [0_u8; 8];
    let mut ra_state = ReassemblerState::new(apid);
    t.check(ra_state.apid() == apid && !ra_state.is_assembling() && ra_state.sdu_len() == 0);
    let mut counter = SequenceCounter::default();
    let mut out = [0_u8; 16];
    loop {
        let Ok(mut seg) = state.resume(sdu) else {
            t.check(false);
            return;
        };
        let Ok(Some(n)) = seg.next_packet(&mut counter, &[], &mut out) else {
            break;
        };
        state = seg.suspend();
        let decoded = SpacePacket::decode(bb(out.get(..n).unwrap_or_default()));
        match (decoded, ra_state.resume(&mut buf)) {
            (Ok(p), Ok(mut ra)) => {
                t.check(ra.push(&p.header(), p.data_field()).is_ok());
                ra_state = ra.suspend();
            }
            _ => t.check(false),
        }
    }
    t.check(state.is_done() && ra_state.sdu_len() == sdu.len());
    t.check(ra_state.resume(bb(&mut [0_u8; 1])).is_err());
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
            match SpacePacket::decode(&bytes) {
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

/// A `usize` as reported by the host suites.
fn wide(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

fn spp_config(case: &Case) -> Result<SppConfig, Fail> {
    if case.config.is_empty() {
        return Ok(SppConfig::DEFAULT);
    }
    Ok(SppConfig::new(bb(case.config.size("max_packet_len")?))?)
}

fn packet_type(p: Map) -> Result<PacketType, Fail> {
    Ok(if p.num::<u64>("packet_type")? == 0 {
        PacketType::Telemetry
    } else {
        PacketType::Telecommand
    })
}

fn apid_of(v: Option<u64>) -> Result<Apid, Fail> {
    need(v.and_then(|n| Apid::new(u16::try_from(n).ok()?)))
}

fn count_of(v: Option<u64>) -> Result<SeqCount, Fail> {
    need(v.and_then(|n| SeqCount::new(u16::try_from(n).ok()?)))
}

/// The sender-chosen fields of an encode case, parsed from raw values (RS-020).
fn packet_fields(p: Map) -> Result<PacketFields, Fail> {
    Ok(PacketFields {
        packet_type: packet_type(p)?,
        apid: apid_of(p.val("apid")?.num())?,
        sequence_flags: SequenceFlags::from_bits(bb(p.num("sequence_flags")?)),
        sequence_count: count_of(p.val("sequence_count")?.num())?,
    })
}

fn decoded(o: &mut Out, p: &SpacePacket<'_>, anomalies: AnomalySet<SppAnomaly>) {
    o.num(
        "packet_type",
        match p.packet_type() {
            PacketType::Telemetry => 0_u8,
            PacketType::Telecommand => 1,
        },
    );
    o.num("secondary_header_flag", u8::from(p.has_secondary_header()));
    o.num("apid", p.apid().get());
    o.flag("idle", p.is_idle());
    o.num("sequence_flags", p.sequence_flags().bits());
    o.num("sequence_count", p.sequence_count().get());
    o.opt_num("packet_name", p.packet_name().map(SeqCount::get));
    o.num("data_len", wide(p.header().data_len()));
    o.num("packet_len", wide(p.header().packet_len()));
    o.bytes("data_hex", p.data_field());
    o.num("consumed", wide(p.len()));
    o.names("anomalies", anomalies.iter());
}

fn encode_case(case: &Case, out: &mut Out, scratch: &mut Scratch) -> Run {
    let p = case.params;
    let cfg = spp_config(case)?;
    let (sec, user) = (p.bytes("secondary_header_hex")?, p.bytes("user_data_hex")?);
    let packet = cfg.packet(packet_fields(p)?, bb(sec), bb(user))?;
    let n = encode(&packet, bb(fresh(&mut scratch.a, packet.packet_len())?))?;
    // The two-phase writer must produce the same octets (API-020).
    let mut w = SpacePacketWriter::begin(
        bb(fresh(&mut scratch.b, n)?),
        &cfg,
        packet_fields(p)?,
        sec.len(),
        user.len(),
    )?;
    if !copy(w.secondary_header_mut(), sec)
        || !copy(w.user_data_mut(), user)
        || w.finish(bb(user.len())) != Ok(n)
    {
        return Err(Fail::Vector);
    }
    let (written, two) = (need(scratch.a.get(..n))?, need(scratch.b.get(..n))?);
    if written != two {
        return Err(Fail::Vector);
    }
    out.bytes("output_hex", written);
    Ok(())
}

fn decode_stream(case: &Case, o: &mut Out) {
    let mode = mode(case);
    let (mut at, mut stop) = (0_usize, None);
    let mut packets = o.list("packets");
    while let Some(rest) = case.input.get(at..).filter(|r| !r.is_empty()) {
        match packet(bb(rest), mode) {
            Ok((p, anomalies)) => {
                packets.obj(|x| {
                    x.num("apid", p.apid().get());
                    x.num("sequence_count", p.sequence_count().get());
                    x.num("consumed", wide(p.len()));
                    x.names("anomalies", anomalies.iter());
                });
                at = at.saturating_add(p.len());
            }
            Err(e) => {
                stop = Some(e.kind().name());
                break;
            }
        }
    }
    o.end(&packets);
    o.opt_text("stop_error", stop);
    o.num("offset", wide(at));
}

fn segment_case(case: &Case, o: &mut Out, s: &mut Scratch) -> Run {
    let p = case.params;
    let (sdu, sec) = (p.bytes("sdu_hex")?, p.bytes("secondary_header_hex")?);
    let mut seg = Segmenter::new(
        &spp_config(case)?,
        packet_type(p)?,
        apid_of(p.val("apid")?.num())?,
        bb(sdu),
        bb(p.size("max_user_len")?),
    )?;
    let mut counter = SequenceCounter::new(count_of(p.val("sequence_count")?.num())?);
    let mut packets = o.list("packets_hex");
    while let Some(n) = seg.next_packet(&mut counter, bb(sec), bb(&mut s.a))? {
        packets.bytes(need(s.a.get(..n))?);
        if (seg.remaining() == 0) != seg.is_done() {
            return Err(Fail::Vector);
        }
    }
    o.end(&packets);
    if !seg.is_done() {
        return Err(Fail::Vector);
    }
    o.num("next_count", counter.peek().get());
    Ok(())
}

const fn discard_name(d: Discard) -> Option<&'static str> {
    Some(match d {
        Discard::Gap => "Gap",
        Discard::Restarted => "Restarted",
        Discard::Interrupted => "Interrupted",
        Discard::Orphan => "Orphan",
        Discard::Overflow => "Overflow",
        _ => return None,
    })
}

fn reassemble_case(case: &Case, o: &mut Out, s: &mut Scratch) -> Run {
    let p = case.params;
    let mut ra = Reassembler::new(
        apid_of(p.val("apid")?.num())?,
        bb(fresh(&mut s.a, p.size("buffer_len")?)?),
    );
    let mut results = o.list("results");
    for item in p.list("packets_hex")? {
        let bytes = need(item.bytes())?;
        let packet = SpacePacket::decode(bb(bytes)).map_err(|_| Fail::Vector)?;
        let pushed = ra.push(&packet.header(), packet.data_field());
        let mut consistent = true;
        results.obj(|x| match pushed {
            Err(e) => x.text("error", e.kind().name()),
            Ok(Outcome { discarded, status }) => {
                consistent &= discarded.is_none_or(|d| discard_name(d).is_some());
                x.opt_text("discarded", discarded.and_then(discard_name));
                match status {
                    Status::InProgress => x.text("status", "InProgress"),
                    Status::Complete(n) => {
                        consistent &= ra.sdu().len() == n;
                        x.text("status", "Complete");
                        x.bytes("sdu_hex", ra.sdu());
                    }
                    Status::Dropped(why) => {
                        consistent &= ra.sdu().is_empty() && discard_name(why).is_some();
                        x.text("status", "Dropped");
                        x.opt_text("reason", discard_name(why));
                    }
                }
            }
        });
        if !consistent {
            return Err(Fail::Vector);
        }
    }
    o.end(&results);
    Ok(())
}

fn continuity(x: &mut Out, t: Tracked) {
    let name = match t {
        Tracked::First => "First",
        Tracked::Next(Continuity::InSequence) => "InSequence",
        Tracked::Next(Continuity::Repeat) => "Repeat",
        Tracked::Next(Continuity::Backwards) => "Backwards",
        Tracked::Next(Continuity::Gap { missing }) => {
            x.num("missing", missing);
            "Gap"
        }
    };
    x.text("continuity", name);
}

fn track_events<const N: usize>(events: &'static [Val], o: &mut Out) -> Run {
    let mut tracker = SequenceTracker::<N>::new();
    let mut results = o.list("results");
    for e in events {
        let e = need(e.obj())?;
        let apid = apid_of(e.val("apid")?.num())?;
        match e.text("op")? {
            "track" => {
                let outcome = tracker.track(bb(apid), count_of(e.val("count")?.num())?);
                results.obj(|x| match outcome {
                    Ok(t) => continuity(x, t),
                    Err(err) => x.text("error", err.kind().name()),
                });
            }
            "forget" => {
                let forgot = tracker.forget(bb(apid));
                results.obj(|x| x.flag("forgot", forgot));
            }
            _ => return Err(Fail::Vector),
        }
    }
    o.end(&results);
    Ok(())
}

fn time_format(f: Map) -> Result<TimeFormat, Fail> {
    Ok(match f.text("code")? {
        "cuc" => TimeFormat::Cuc(
            CucFormat::new(
                if f.flag("level2")? {
                    CucEpoch::Agency
                } else {
                    CucEpoch::Tai1958
                },
                f.num("coarse")?,
                f.num("fine")?,
            )
            .map_err(|_| Fail::Vector)?,
        ),
        "cds" => TimeFormat::Cds(CdsFormat::new(
            if f.flag("agency_epoch")? {
                CdsEpoch::Agency
            } else {
                CdsEpoch::Ccsds1958
            },
            if f.flag("day24")? {
                CdsDayLen::Bits24
            } else {
                CdsDayLen::Bits16
            },
            match f.num::<u8>("subms")? {
                0 => CdsSubMs::None,
                1 => CdsSubMs::Micros,
                _ => CdsSubMs::Picos,
            },
        )),
        "ccs" => TimeFormat::Ccs(
            CcsFormat::new(CcsVariation::MonthDay, f.num("subsecond_octets")?)
                .map_err(|_| Fail::Vector)?,
        ),
        _ => return Err(Fail::Vector),
    })
}

fn split_time_case(case: &Case, out: &mut Out) -> Run {
    let l = case.params.obj("layout")?;
    let layout = TimeHeaderLayout::new(
        time_format(l.obj("format")?)?,
        bb(l.flag("pfield")?),
        bb(l.size("ancillary_len")?),
    )?;
    let (packet, _) = packet(bb(case.input), mode(case))?;
    let layout = SecondaryHeaderLayout::Time(layout);
    let layout = bb(&layout);
    let (split, _) =
        mode(case).dispatch(|| packet.split(layout), || packet.split_lenient(layout))?;
    let t = need(split.time_code())?;
    let time_code = need(split.secondary_header().get(..t.len()))?;
    // The decoded code re-encodes to the received octets (API-020).
    let mut buf = [0_u8; 32];
    let n = t
        .code()
        .encode(t.pfield().is_some(), bb(&mut buf))
        .map_err(|_| Fail::Vector)?;
    if buf.get(..n) != Some(time_code) {
        return Err(Fail::Vector);
    }
    out.bytes("secondary_header_hex", split.secondary_header());
    out.bytes("user_data_hex", split.user_data());
    out.bytes("time_code_hex", time_code);
    out.bytes("ancillary_hex", split.ancillary_data());
    Ok(())
}

/// Decodes one packet in `mode`, with its anomalies.
fn packet(
    bytes: &[u8],
    mode: DecodeMode,
) -> Result<(SpacePacket<'_>, AnomalySet<SppAnomaly>), aldis_core::Error> {
    mode.dispatch(
        || SpacePacket::decode(bytes),
        || SpacePacket::decode_lenient(bytes),
    )
}

const fn mode(case: &Case) -> DecodeMode {
    if case.lenient {
        DecodeMode::Lenient
    } else {
        DecodeMode::Strict
    }
}

/// One `test-vectors/spp/` case, as `crates/aldis-spp/tests/vectors.rs` runs it.
pub fn vector(case: &Case, o: &mut Out, s: &mut Scratch) -> Run {
    let p = case.params;
    match case.op {
        "decode" => {
            let (p, a) = packet(bb(case.input), mode(case))?;
            decoded(o, &p, a);
        }
        "peek_length" => o.num("len", wide(SpacePacket::peek_length(bb(case.input))?)),
        "encode" => encode_case(case, o, s)?,
        "idle" => {
            let idle = IdlePacket::new(&spp_config(case)?, bb(p.size("total_len")?))?
                .with_sequence_count(count_of(p.val("sequence_count")?.num())?)
                .with_pattern(bb(p.bytes("pattern_hex")?));
            let n = idle.encode(bb(fresh(&mut s.a, idle.total_len())?))?;
            o.bytes("output_hex", need(s.a.get(..n))?);
        }
        "octet_string" => {
            let octets = p.bytes("octets_hex")?;
            let counter = SequenceCounter::new(count_of(p.val("sequence_count")?.num())?);
            let mut enc = OctetStringEncoder::new(
                &spp_config(case)?,
                packet_type(p)?,
                apid_of(p.val("apid")?.num())?,
                counter,
            )?;
            let len = need(octets.len().checked_add(6))?;
            let n = enc.encode(
                bb(p.flag("secondary_header_indicator")?),
                bb(octets),
                bb(fresh(&mut s.a, len)?),
            )?;
            o.bytes("output_hex", need(s.a.get(..n))?);
            o.num("next_count", enc.counter().peek().get());
        }
        "track" => {
            let events = p.list("events")?;
            match p.size("capacity")? {
                1 => track_events::<1>(events, o)?,
                2 => track_events::<2>(events, o)?,
                _ => return Err(Fail::Vector),
            }
        }
        "decode_stream" => decode_stream(case, o),
        "split" => {
            let len = p.val("secondary_header_len")?;
            let layout = if len.is_null() {
                SecondaryHeaderLayout::Absent
            } else {
                let n = need(len.num().and_then(|n| usize::try_from(n).ok()))?;
                SecondaryHeaderLayout::Opaque(need(NonZeroUsize::new(n))?)
            };
            let (p, _) = packet(bb(case.input), mode(case))?;
            let layout = bb(&layout);
            let (s, _) = mode(case).dispatch(|| p.split(layout), || p.split_lenient(layout))?;
            o.bytes("secondary_header_hex", s.secondary_header());
            o.bytes("user_data_hex", s.user_data());
        }
        "segment" => segment_case(case, o, s)?,
        "reassemble" => reassemble_case(case, o, s)?,
        "split_time" => split_time_case(case, o)?,
        _ => return Err(Fail::Vector),
    }
    Ok(())
}

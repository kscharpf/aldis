//! Calls every public `aldis-tc` API (VER-010, M2 D16) and runs every `test-vectors/tc/` case
//! (`vector`). Inputs pass through `black_box`.

use core::fmt::Write as _;
use core::hint::black_box as bb;

use aldis_core::ids::FrameSeqNum;
use aldis_core::ocf::{Ocf, Type1Report};
use aldis_core::security::SecurityLayout;
use aldis_core::{Anomaly, DecodeMode, Error, ErrorKind};
use aldis_tc::{
    Clcw, ClcwStatus, ControlCommand, CopInEffect, Delimited, FarmBCounter, FrameData,
    FrameDelimiter, FrameFields, FrameType, MapConfig, PhysicalChannelConfig, Reason,
    SegmentHeader, SegmentHeaderUse, SequenceFlags, TcAnomaly, TcFrame, TcFrameWriter, TcMapId,
    TcSpacecraftId, TcVcId, VcConfig, VcLookup, VcTable, encode, next_frame, validate_frame,
};

use crate::exercise::Tally;
use crate::vectors::{Case, Fail, Map, Out, Params as _, Run, Scratch, Text, Val, fresh, need};

/// Room for every channel the vectors configure.
type Table = VcTable<8, 8>;

const SCID: u16 = 0x155;

/// VC 1 without Segment Headers, VC 2 with MAP 9 (SDLS 3/1), VC 3 with SDLS 4/2.
fn standard(fecf: bool) -> Option<VcTable<3, 1>> {
    let scid = TcSpacecraftId::new(bb(SCID))?;
    let plain = SegmentHeaderUse::Absent(SecurityLayout::NONE);
    let sdls = SegmentHeaderUse::Absent(SecurityLayout::new(4, 2).ok()?);
    let vcs = [
        VcConfig::new(scid, TcVcId::new(1)?, 1024, plain).ok()?,
        VcConfig::new(scid, TcVcId::new(2)?, 1024, SegmentHeaderUse::Present).ok()?,
        VcConfig::new(scid, TcVcId::new(3)?, bb(1024), sdls).ok()?,
    ];
    let map = MapConfig::new(
        scid,
        TcVcId::new(2)?,
        TcMapId::new(9)?,
        SecurityLayout::new(3, 1).ok()?,
    );
    VcTable::new(
        PhysicalChannelConfig::new(1024, bb(fecf)).ok()?,
        &vcs,
        &[map],
    )
    .ok()
}

fn fields(frame_type: FrameType, vc: u8) -> Option<FrameFields> {
    Some(FrameFields {
        frame_type,
        spacecraft_id: TcSpacecraftId::new(SCID)?,
        vc_id: TcVcId::new(vc)?,
        sequence_number: FrameSeqNum::new(bb(7)),
    })
}

fn config_group(t: &mut Tally) {
    let Some(table) = standard(true) else {
        t.check(false);
        return;
    };
    let pc = table.physical();
    t.check(pc.max_frame_len() == 1024 && pc.fecf() && table.physical_channel() == pc);
    t.check(PhysicalChannelConfig::new(bb(1025), false).is_err());
    t.check(table.vcs().count() == 3 && table.maps().count() == 1);
    let scid = TcSpacecraftId::new(SCID);
    let vc2 = scid.zip(TcVcId::new(2)).and_then(|(s, v)| table.vc(s, v));
    t.check(vc2.is_some_and(|vc| {
        vc.segment_header() == SegmentHeaderUse::Present
            && vc.max_frame_len() == 1024
            && vc.vc_id().get() == 2
            && vc.spacecraft_id().get() == SCID
    }));
    let map = scid
        .zip(TcVcId::new(2))
        .zip(TcMapId::new(9))
        .and_then(|((s, v), m)| table.map(s, v, m));
    t.check(map.is_some_and(|m| {
        m.map_id().get() == 9
            && m.vc_id().get() == 2
            && m.spacecraft_id().get() == SCID
            && m.security().header_len() == 3
    }));
    t.check(scid.is_some_and(|s| table.has_spacecraft(s)));
    let short = scid
        .zip(TcVcId::new(1))
        .map(|(s, v)| VcConfig::new(s, v, bb(5), SegmentHeaderUse::Present));
    t.check(short.is_some_and(|r| r.is_err()));
}

fn header_group(t: &mut Tally) {
    for (b, c) in [(false, false), (true, false), (true, true), (false, true)] {
        let ft = FrameType::from_flags(bb(b), bb(c));
        t.check(ft.map(|f| (f.bypass_flag(), f.control_command_flag())) == ft.map(|_| (b, c)));
    }
    for bits in 0..4_u8 {
        t.check(SequenceFlags::from_bits(bb(bits)).bits() == bits);
    }
    let sh = TcMapId::new(5).map(|map_id| SegmentHeader {
        sequence_flags: SequenceFlags::First,
        map_id,
    });
    t.check(sh.map(SegmentHeader::octet) == Some(0x45));
}

fn frame_group(t: &mut Tally) {
    let (Some(table), Some(ad), Some(bc)) = (
        standard(true),
        fields(FrameType::Ad, 1),
        fields(FrameType::Bc, 1),
    ) else {
        t.check(false);
        return;
    };
    let mut out = [0_u8; 64];
    let data = FrameData::Fdu {
        segment_header: None,
        user_data: bb(&[1, 2, 3]),
    };
    let n = encode(&table, ad, data, &mut out);
    t.check(n == Ok(10));
    let frame = out.get(..10).unwrap_or_default();
    match TcFrame::decode(bb(frame), &table, DecodeMode::Strict) {
        Ok(f) => {
            let h = f.header();
            t.check(f.frame_type() == FrameType::Ad && f.spacecraft_id().get() == SCID);
            t.check(f.vc_id().get() == 1 && f.sequence_number().get() == 7);
            t.check(f.segment_header().is_none() && f.security_header().is_empty());
            t.check(f.data_field() == [1, 2, 3] && f.security_trailer().is_empty());
            t.check(f.fecf().is_some() && f.control_command().is_none() && f.octets() == frame);
            t.check(f.len() == 10 && !f.is_empty() && f.anomalies().is_empty());
            t.check(h.fields() == ad && h.frame_type() == FrameType::Ad && h.frame_len() == 10);
            t.check(h.spacecraft_id() == ad.spacecraft_id && h.vc_id() == ad.vc_id);
            t.check(h.sequence_number() == ad.sequence_number && h.segment_header().is_none());
            t.check(frame.get(..5) == Some(&h.primary_header_octets()[..]));
        }
        Err(_) => t.check(false),
    }
    t.check(TcFrame::peek_length(bb(frame)) == Ok(10));
    t.check(validate_frame(bb(frame), &table, DecodeMode::Strict).is_ok_and(|f| f.len() == 10));
    match validate_frame(
        bb(frame.get(..9).unwrap_or_default()),
        &table,
        DecodeMode::Strict,
    ) {
        Ok(_) => t.check(false),
        Err(r) => {
            t.check(r.reason() == Reason::LengthMismatch && r.reason().code() == 8);
            t.check(
                Error::from(r).kind() == ErrorKind::LengthMismatch && r.error() == Error::from(r),
            );
        }
    }
    t.check(Reason::from_code(bb(11)) == Some(Reason::ReservedControlCommand));
    let set = ControlCommand::SetVr(FrameSeqNum::new(bb(9)));
    t.check(ControlCommand::parse(set.to_octets().as_slice()) == Some(set));
    t.check(ControlCommand::parse(bb(&[0])) == Some(ControlCommand::Unlock));
    let n = encode(&table, bc, FrameData::Control(set), &mut out);
    t.check(n == Ok(10));
    // Two-phase, with SDLS on MAP 9 of VC 2.
    let sh = TcMapId::new(9).map(|map_id| SegmentHeader {
        sequence_flags: SequenceFlags::Unsegmented,
        map_id,
    });
    let seg = fields(FrameType::Bd, 2);
    match seg.map(|f| TcFrameWriter::begin(&mut out, &table, f, sh, bb(4))) {
        Some(Ok(mut w)) => {
            w.security_header_mut().fill(0xA5);
            w.security_trailer_mut().fill(0x5A);
            w.user_data_mut().fill(0x11);
            t.check(w.finish(bb(2)) == Ok(14));
        }
        _ => t.check(false),
    }
    let f = TcFrame::decode(bb(&out), &table, DecodeMode::Strict);
    t.check(f.is_ok_and(|f| {
        f.security_header() == [0xA5; 3]
            && f.data_field() == [0x11; 2]
            && f.security_trailer() == [0x5A]
    }));
    t.check(TcAnomaly::from_bit(bb(1)).is_some_and(|a| a.bit() == 1));
}

fn delimit_group(t: &mut Tally) {
    let unit = bb([0x31, 0x55, 0x04, 0x05, 0x00, 0x00, 0x55, 0x55]);
    let mut d = FrameDelimiter::new(&unit);
    t.check(d.remaining().len() == 8);
    let first = d.next();
    t.check(
        first.is_some_and(|i| i.len() == 6 && !i.is_empty() && matches!(i, Delimited::Frame(_))),
    );
    t.check(
        d.next()
            .is_some_and(|i| matches!(i, Delimited::Fill(f) if f.len() == 2)),
    );
    t.check(d.next().is_none());
    let cut = bb([0x01, 0x55, 0x04, 0x09, 0x00, 0x00]);
    t.check(next_frame(&cut).is_some_and(|i| {
        matches!(
            i,
            Delimited::TruncatedFrame {
                declared_len: 10,
                ..
            }
        ) && i.octets().len() == 6
    }));
}

fn clcw_group(t: &mut Tally) {
    let word = bb([0x01, 0x14, 0x0C, 0x2A]);
    match Clcw::decode(word, DecodeMode::Strict) {
        Ok((c, a)) => {
            t.check(a.is_empty() && c.to_octets() == word && c.to_word() == 0x0114_0C2A);
            t.check(c.cop_in_effect == CopInEffect::COP_1 && c.retransmit);
            t.check(matches!(c.to_ocf(), Ocf::Clcw(r) if r.to_octets() == word));
        }
        Err(_) => t.check(false),
    }
    let report = Type1Report::new(word);
    t.check(report.is_some_and(|r| Clcw::decode_report(r, DecodeMode::Lenient).is_ok()));
    t.check(Clcw::decode(bb([0x01, 0x01, 0, 0]), DecodeMode::Strict).is_err());
    t.check(ClcwStatus::new(bb(7)).map(ClcwStatus::get) == Some(7));
    t.check(ClcwStatus::try_from(bb(8)).is_err() && CopInEffect::try_from(bb(4)).is_err());
    t.check(FarmBCounter::try_from(bb(3)).map(u8::from) == Ok(3));
    t.check(FarmBCounter::new(bb(4)).is_none() && CopInEffect::new(bb(2)).is_some());
    t.check(u8::from(CopInEffect::COP_1) == 1 && u8::from(ClcwStatus::default()) == 0);
}

/// Runs the `aldis-tc` group.
pub fn run(t: &mut Tally) {
    config_group(t);
    header_group(t);
    frame_group(t);
    delimit_group(t);
    clcw_group(t);
}

// ----- the vector handler, as `crates/aldis-tc/tests/vectors.rs` runs it -----

const fn mode(case: &Case) -> DecodeMode {
    if case.lenient {
        DecodeMode::Lenient
    } else {
        DecodeMode::Strict
    }
}

fn opt_size(map: Map, key: &str) -> Result<usize, Fail> {
    match crate::vectors::get(map, key) {
        None => Ok(0),
        Some(v) => usize::try_from(need(v.num())?).map_err(|_| Fail::Vector),
    }
}

fn security(map: Map) -> Result<SecurityLayout, Fail> {
    Ok(SecurityLayout::new(
        opt_size(map, "security_header_len")?,
        opt_size(map, "security_trailer_len")?,
    )?)
}

fn scid_of(map: Map, key: &str) -> Result<TcSpacecraftId, Fail> {
    Ok(TcSpacecraftId::try_from(map.num::<u16>(key)?)?)
}

fn vcid_of(map: Map, key: &str) -> Result<TcVcId, Fail> {
    Ok(TcVcId::try_from(map.num::<u8>(key)?)?)
}

fn mapid_of(map: Map, key: &str) -> Result<TcMapId, Fail> {
    Ok(TcMapId::try_from(map.num::<u8>(key)?)?)
}

/// The channels of a case's `config`, built through the checked constructors.
fn table(case: &Case) -> Result<Table, Fail> {
    let c = case.config;
    let pc = PhysicalChannelConfig::new(c.size("max_frame_len")?, c.flag("fecf")?)?;
    let (spacecraft, channel, mux) = (
        need(TcSpacecraftId::new(0))?,
        need(TcVcId::new(0))?,
        need(TcMapId::new(0))?,
    );
    let placeholder = VcConfig::new(spacecraft, channel, 1024, SegmentHeaderUse::Present)?;
    let mut vcs = [placeholder; 8];
    let vc_list = c.list("vcs")?;
    for (slot, v) in vcs.iter_mut().zip(vc_list) {
        let v = need(v.obj())?;
        let segment = if v.flag("segment_header")? {
            SegmentHeaderUse::Present
        } else {
            SegmentHeaderUse::Absent(security(v)?)
        };
        *slot = VcConfig::new(
            scid_of(v, "scid")?,
            vcid_of(v, "vcid")?,
            v.size("max_frame_len")?,
            segment,
        )?;
    }
    let placeholder_map = MapConfig::new(spacecraft, channel, mux, SecurityLayout::NONE);
    let mut maps = [placeholder_map; 8];
    let map_list = c.list("maps")?;
    for (slot, m) in maps.iter_mut().zip(map_list) {
        let m = need(m.obj())?;
        *slot = MapConfig::new(
            scid_of(m, "scid")?,
            vcid_of(m, "vcid")?,
            mapid_of(m, "map_id")?,
            security(m)?,
        );
    }
    let vcs = need(vcs.get(..vc_list.len()))?;
    let maps = need(maps.get(..map_list.len()))?;
    Ok(Table::new(pc, vcs, maps)?)
}

const fn frame_type_name(t: FrameType) -> &'static str {
    match t {
        FrameType::Ad => "AD",
        FrameType::Bd => "BD",
        FrameType::Bc => "BC",
    }
}

fn decoded(o: &mut Out, f: &TcFrame<'_>) {
    o.text("frame_type", frame_type_name(f.frame_type()));
    o.num("bypass_flag", u8::from(f.frame_type().bypass_flag()));
    o.num(
        "control_command_flag",
        u8::from(f.frame_type().control_command_flag()),
    );
    o.num("spacecraft_id", f.spacecraft_id().get());
    o.num("vc_id", f.vc_id().get());
    o.num("frame_len", wide(f.header().frame_len()));
    o.num("sequence_number", f.sequence_number().get());
    match f.segment_header() {
        Some(sh) => o.obj("segment_header", |s| {
            s.num("sequence_flags", sh.sequence_flags.bits());
            s.num("map_id", sh.map_id.get());
        }),
        None => o.null("segment_header"),
    }
    o.bytes("security_header_hex", f.security_header());
    o.bytes("data_hex", f.data_field());
    o.bytes("security_trailer_hex", f.security_trailer());
    match f.fecf() {
        Some(v) => o.bytes("fecf_hex", &v.to_be_bytes()),
        None => o.null("fecf_hex"),
    }
    let (command, vr) = match f.control_command() {
        None => (None, None),
        Some(ControlCommand::Unlock) => (Some("unlock"), None),
        Some(ControlCommand::SetVr(v)) => (Some("set_vr"), Some(v.get())),
    };
    o.opt_text("control_command", command);
    o.opt_num("set_vr_value", vr);
    o.num("consumed", wide(f.len()));
    o.names("anomalies", f.anomalies().iter());
}

/// `usize` → `u64` for a comparison.
fn wide(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

fn frame_fields(p: Map) -> Result<FrameFields, Fail> {
    Ok(FrameFields {
        frame_type: match p.text("frame_type")? {
            "AD" => FrameType::Ad,
            "BD" => FrameType::Bd,
            "BC" => FrameType::Bc,
            _ => return Err(Fail::Vector),
        },
        spacecraft_id: scid_of(p, "spacecraft_id")?,
        vc_id: vcid_of(p, "vc_id")?,
        sequence_number: FrameSeqNum::new(p.num("sequence_number")?),
    })
}

fn encode_case(case: &Case, o: &mut Out, scratch: &mut Scratch) -> Run {
    let channels = table(case)?;
    let p = case.params;
    let fields = frame_fields(p)?;
    let out = fresh(&mut scratch.a, 1100)?;
    let n = if let Some(cmd) = crate::vectors::get(p, "control_command").and_then(Val::str) {
        let command = match cmd {
            "unlock" => ControlCommand::Unlock,
            "set_vr" => ControlCommand::SetVr(FrameSeqNum::new(p.num("set_vr_value")?)),
            _ => return Err(Fail::Vector),
        };
        encode(&channels, fields, FrameData::Control(command), bb(out))?
    } else {
        let sh = p.val("segment_header")?;
        let segment = if sh.is_null() {
            None
        } else {
            let sh = need(sh.obj())?;
            Some(SegmentHeader {
                sequence_flags: SequenceFlags::from_bits(sh.num("sequence_flags")?),
                map_id: mapid_of(sh, "map_id")?,
            })
        };
        let user = p.bytes("user_data_hex")?;
        let fdu = FrameData::Fdu {
            segment_header: segment,
            user_data: bb(user),
        };
        let n = encode(&channels, fields, fdu, bb(&mut *out))?;
        // The two-phase writer must produce the same octets (API-020).
        let two = fresh(&mut scratch.b, n)?;
        let mut w = TcFrameWriter::begin(two, &channels, fields, segment, user.len())?;
        if !crate::vectors::copy(w.user_data_mut(), user) || w.finish(user.len()) != Ok(n) {
            return Err(Fail::Vector);
        }
        if scratch.b.get(..n) != scratch.a.get(..n) {
            return Err(Fail::Vector);
        }
        n
    };
    o.bytes("output_hex", need(scratch.a.get(..n))?);
    Ok(())
}

fn validate_case(case: &Case, o: &mut Out) -> Run {
    let channels = table(case)?;
    match validate_frame(bb(case.input), &channels, mode(case)) {
        Ok(f) => {
            o.flag("accepted", true);
            o.num("frame_len", wide(f.len()));
        }
        Err(r) => {
            o.flag("accepted", false);
            let mut name = Text::new();
            if write!(name, "{:?}", r.reason()).is_err() {
                return Err(Fail::Vector);
            }
            o.bytes("reason", name.as_bytes());
            o.num("reason_code", r.reason().code());
            o.text("error_kind", r.error().kind().name());
            o.opt_num("error_field", r.error().field().map(|f| f.code().get()));
        }
    }
    Ok(())
}

fn delimit_case(case: &Case, o: &mut Out) {
    let mut list = o.list("items");
    let mut at = 0_usize;
    for item in FrameDelimiter::new(bb(case.input)) {
        let (kind, declared) = match item {
            Delimited::Frame(_) => ("frame", None),
            Delimited::TruncatedFrame { declared_len, .. } => {
                ("truncated", Some(wide(declared_len)))
            }
            Delimited::Fill(_) => ("fill", None),
        };
        list.obj(|i| {
            i.text("kind", kind);
            i.num("offset", wide(at));
            i.bytes("octets_hex", item.octets());
            i.opt_num("declared_len", declared);
        });
        at = at.saturating_add(item.len());
    }
    o.end(&list);
}

fn clcw_case(case: &Case, o: &mut Out) -> Run {
    let word = need(
        case.input
            .first_chunk::<4>()
            .filter(|_| case.input.len() == 4)
            .copied(),
    )?;
    let (c, anomalies) = Clcw::decode(bb(word), mode(case))?;
    clcw_out(o, &c);
    o.names("anomalies", anomalies.iter());
    Ok(())
}

fn clcw_out(o: &mut Out, c: &Clcw) {
    o.num("status", c.status.get());
    o.num("cop_in_effect", c.cop_in_effect.get());
    o.num("vc_id", c.vc_id.get());
    o.flag("no_rf_available", c.no_rf_available);
    o.flag("no_bit_lock", c.no_bit_lock);
    o.flag("lockout", c.lockout);
    o.flag("wait", c.wait);
    o.flag("retransmit", c.retransmit);
    o.num("farm_b_counter", c.farm_b_counter.get());
    o.num("report_value", c.report_value.get());
}

fn clcw_encode(case: &Case, o: &mut Out) -> Run {
    let p = case.params;
    let c = Clcw {
        status: ClcwStatus::try_from(p.num::<u8>("status")?)?,
        cop_in_effect: CopInEffect::try_from(p.num::<u8>("cop_in_effect")?)?,
        vc_id: vcid_of(p, "vc_id")?,
        no_rf_available: p.flag("no_rf_available")?,
        no_bit_lock: p.flag("no_bit_lock")?,
        lockout: p.flag("lockout")?,
        wait: p.flag("wait")?,
        retransmit: p.flag("retransmit")?,
        farm_b_counter: FarmBCounter::try_from(p.num::<u8>("farm_b_counter")?)?,
        report_value: FrameSeqNum::new(p.num("report_value")?),
    };
    o.bytes("output_hex", &bb(c).to_octets());
    Ok(())
}

/// One `test-vectors/tc/` case, as `crates/aldis-tc/tests/vectors.rs` runs it.
pub fn vector(case: &Case, o: &mut Out, s: &mut Scratch) -> Run {
    match case.op {
        "config" => {
            let t = table(case)?;
            o.num("vcs", wide(t.vcs().count()));
            o.num("maps", wide(t.maps().count()));
        }
        "decode" => {
            let t = table(case)?;
            decoded(o, &TcFrame::decode(bb(case.input), &t, mode(case))?);
        }
        "peek_length" => o.num("frame_len", wide(TcFrame::peek_length(bb(case.input))?)),
        "encode" => encode_case(case, o, s)?,
        "validate" => validate_case(case, o)?,
        "delimit" => delimit_case(case, o),
        "clcw_decode" => clcw_case(case, o)?,
        "clcw_encode" => clcw_encode(case, o)?,
        _ => return Err(Fail::Vector),
    }
    Ok(())
}

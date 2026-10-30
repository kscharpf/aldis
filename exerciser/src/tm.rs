//! Calls every public `aldis-tm` API (VER-010, M2 D16) and runs every `test-vectors/tm/` case
//! (`vector`). Inputs pass through `black_box`.

use core::hint::black_box as bb;

use aldis_core::ids::{SpacecraftId, VcId};
use aldis_core::ocf::Ocf;
use aldis_core::oid::{OidGenerator, OidGeneratorGalois};
use aldis_core::security::SecurityLayout;
use aldis_core::{Anomaly as _, AnomalySet, DecodeMode, ErrorKind};
use aldis_tm::{
    DataFieldContent, DataFieldStatus, FirstHeaderPointer, FshLen, NoSdls, PacketOffset,
    PacketPointer, SegmentLengthId, TmAnomaly, TmFrame, TmFrameContent, TmFrameFields,
    TmFrameWriter, TmMasterChannelConfig, TmPhysicalChannelConfig, TmPrimaryHeader,
    TmVirtualChannelConfig, UncheckedTmFrame, VcLookup as _, encode,
    encode_oid as encode_oid_frame, encode_oid_galois,
};

use crate::exercise::Tally;
use crate::vectors::{Case, Fail, Map, Out, Params as _, Run, Scratch, Val, copy, fresh, need};

/// The identifiers, the Primary Header, and the anomaly flags.
fn header_group(t: &mut Tally) {
    t.check(SegmentLengthId::new(bb(4)).is_none() && SegmentLengthId::try_from(bb(4_u8)).is_err());
    t.check(
        SegmentLengthId::try_from(bb(3_u8)).is_ok_and(|s| u8::from(s) == 3)
            && SegmentLengthId::NO_SEGMENTS.get() == SegmentLengthId::MAX,
    );
    t.check(FirstHeaderPointer::new(bb(0x800)).is_none());
    t.check(FirstHeaderPointer::try_from(bb(0x800_u16)).is_err());
    t.check(
        FirstHeaderPointer::try_from(bb(5_u16)).is_ok_and(|f| u16::from(f) == 5 && f.get() == 5),
    );
    t.check(
        FirstHeaderPointer::from(bb(PacketPointer::OnlyIdleData))
            == FirstHeaderPointer::ONLY_IDLE_DATA,
    );
    t.check(
        PacketOffset::new(bb(0x7FE)).is_none() && PacketOffset::try_from(bb(0x7FE_u16)).is_err(),
    );
    t.check(
        PacketOffset::try_from(bb(PacketOffset::MAX)).is_ok_and(|n| {
            PacketPointer::StartsAt(n).to_raw().get() == 0x7FD
                && n.get() == 0x7FD
                && u16::from(n) == 0x7FD
        }),
    );
    t.check(
        PacketPointer::classify(bb(FirstHeaderPointer::NO_PACKET_START), bb(4))
            == Ok(PacketPointer::NoPacketStart),
    );
    t.check(
        FirstHeaderPointer::try_from(bb(FirstHeaderPointer::MAX))
            .is_ok_and(|f| PacketPointer::classify(f, bb(4)).is_ok()),
    );
    t.check(PacketPointer::classify(FirstHeaderPointer::ONLY_IDLE_DATA, 0).is_ok());
    t.check(FshLen::new(bb(1)).is_none() && FshLen::try_from(bb(65_u8)).is_err());
    t.check(FshLen::try_from(bb(64_u8)).is_ok_and(|l| {
        l.get() == 64 && l.octets() == 64 && l.id_octet() == 0x3F && u8::from(l) == FshLen::MAX
    }));
    t.check(FshLen::MIN == 2);
    let header = TmPrimaryHeader::decode(bb(&[0x2A, 0x5B, 0x12, 0x34, 0x99, 0x23]));
    t.check(header.is_ok_and(|h| {
        h.scid.get() == 0x2A5
            && h.vcid.get() == 5
            && h.ocf_flag
            && h.fsh_flag
            && h.to_octets() == [0x2A, 0x5B, 0x12, 0x34, 0x99, 0x23]
    }));
    t.check(TmPrimaryHeader::decode(bb(&[0; 5])).is_err_and(|e| e.kind() == ErrorKind::Incomplete));
    t.check(!DataFieldStatus::Packets(bb(PacketPointer::NoPacketStart)).sync_flag());
    t.check(
        TmAnomaly::ALL.iter().map(|a| a.bit()).eq([0, 1, 2])
            && TmAnomaly::from_bit(bb(3)).is_none(),
    );
    t.check(AnomalySet::from_flags(bb(TmAnomaly::ALL)).bits() == 0b111);
}

/// The configuration, the lookups, the decoder, the writers, and validation.
fn frame_group(t: &mut Tally) {
    let (Some(scid), Some(vc0), Some(vc1)) = (
        SpacecraftId::<10>::new(bb(0x0AB)),
        VcId::<3>::new(bb(0)),
        VcId::<3>::new(bb(1)),
    ) else {
        t.check(false);
        return;
    };
    t.check(TmPhysicalChannelConfig::new(bb(8), bb(true)).is_err());
    let Ok(pc) = TmPhysicalChannelConfig::new(bb(32), bb(true)) else {
        t.check(false);
        return;
    };
    t.check(pc.frame_len() == 32 && pc.fecf() && DataFieldContent::VcaSdu.sync_flag());
    let Some(fsh) = FshLen::new(bb(4)) else {
        t.check(false);
        return;
    };
    let security = SecurityLayout::new(bb(2), bb(1)).unwrap_or(SecurityLayout::NONE);
    let packets = TmVirtualChannelConfig::new(vc0, DataFieldContent::Packets)
        .with_fsh(bb(fsh))
        .with_security(bb(security));
    let vca = TmVirtualChannelConfig::new(vc1, bb(DataFieldContent::VcaSdu)).with_ocf();
    t.check(packets.vcid() == vc0 && packets.fsh() == Some(fsh) && !packets.ocf());
    t.check(vca.content() == DataFieldContent::VcaSdu && vca.security().is_none());
    let mc = TmMasterChannelConfig::new(pc, scid, None, bb(false))
        .with_vc(bb(packets))
        .and_then(|mc| mc.with_vc(bb(vca)));
    let Ok(mc) = mc else {
        t.check(false);
        return;
    };
    t.check(
        mc.clone()
            .with_vc(bb(packets))
            .is_err_and(|e| e.kind() == ErrorKind::InvalidConfig),
    );
    t.check(mc.pc() == &pc && mc.scid() == scid && mc.fsh().is_none() && !mc.ocf());
    t.check(mc.vc(vc1) == Some(&vca) && mc.vcs().count() == 2);
    t.check(mc.frame_layout(vc0).is_some_and(|l| l.data().len() == 17));
    t.check(mc.security_layout(bb(scid), vc0) == Ok(security));
    let many = [mc.clone()];
    t.check(many[..].security_layout(bb(scid), vc1).is_ok());
    t.check(NoSdls.security_layout(scid, vc1) == Ok(SecurityLayout::NONE));
    frames(t, &mc, vc0, vc1, fsh);
}

/// Frames of `mc`'s VC 0 (packets, a 4-octet `VC_FSH`, SDLS 2/1) and VC 1 (`VCA_SDUs`, a `VC_OCF`):
/// the writers, the decoders, and validation.
fn frames(t: &mut Tally, mc: &TmMasterChannelConfig, vc0: VcId<3>, vc1: VcId<3>, fsh: FshLen) {
    let (pc, scid) = (*mc.pc(), mc.scid());
    let mut frame = [0_u8; 32];
    let Some(offset) = PacketOffset::new(bb(3)) else {
        t.check(false);
        return;
    };
    let fields = TmFrameFields::new(
        vc0,
        bb(1),
        bb(2),
        DataFieldStatus::Packets(PacketPointer::StartsAt(offset)),
    );
    match TmFrameWriter::begin(bb(&mut frame), mc, bb(&fields)) {
        Ok(mut w) => {
            t.check(copy(w.secondary_header_data_mut(), &[1, 2, 3]));
            t.check(copy(w.security_header_mut(), &[0xA5, 0xA5]));
            w.data_field_mut().fill(bb(0xD0));
            t.check(copy(w.security_trailer_mut(), &[0x5A]));
            t.check(w.set_ocf(Ocf::decode([0; 4])).is_err());
            t.check(w.layout().frame_len() == 32);
            t.check(w.finish() == 32);
        }
        Err(_) => t.check(false),
    }
    match TmFrame::decode_with(bb(&frame), &pc, mc) {
        Ok(f) => {
            t.check(f.scid() == scid && f.vcid() == vc0 && f.mc_frame_count() == 1);
            t.check(f.vc_frame_count() == 2 && f.fsh_len() == Some(fsh));
            t.check(f.secondary_header_data() == Some(&[1, 2, 3][..]));
            t.check(f.security_header() == [0xA5, 0xA5] && f.security_trailer() == [0x5A]);
            t.check(f.data_field().len() == 17 && f.ocf().is_none() && f.fecf().is_some());
            t.check(f.layout().data() == (12..29) && f.as_bytes().len() == 32);
            t.check(f.len() == 32 && !f.is_empty());
            t.check(
                f.data_field_status() == fields.status
                    && f.header().first_header_pointer.get() == 3,
            );
            t.check(f.validate(bb(mc)).is_ok_and(|v| v.len() == 32));
        }
        Err(_) => t.check(false),
    }
    t.check(TmFrame::decode_lenient(bb(&frame), &pc).is_ok_and(|l| l.anomalies().is_empty()));
    t.check(TmFrame::decode_with_lenient(bb(&frame), &pc, mc).is_ok());
    t.check(
        TmFrame::decode_unchecked(bb(&frame), &pc, &NoSdls).is_ok_and(|u| u.check_fecf().is_ok()),
    );
    t.check(TmFrame::decode_unchecked_lenient(bb(&frame), &pc, &NoSdls).is_ok());
    t.check(TmFrame::peek_length(bb(&frame), &pc) == Ok(32));
    let data = [0x55; 20];
    let content = TmFrameContent {
        ocf: Some(Ocf::decode(bb([0x80, 1, 2, 3]))),
        ..TmFrameContent::new(bb(&data))
    };
    let vca_fields = TmFrameFields::new(
        vc1,
        0,
        0,
        DataFieldStatus::VcaSdu {
            packet_order_flag: bb(false),
            segment_length_id: SegmentLengthId::NO_SEGMENTS,
            first_header_pointer: FirstHeaderPointer::NO_PACKET_START,
        },
    );
    t.check(encode(mc, bb(&vca_fields), bb(&content), bb(&mut frame)) == Ok(32));
    let mut oid = OidGenerator::new();
    // An OID frame needs a VC without SDLS (132.0 §6.4.6.2): VC 0 of `mc` has it, the VC of
    // `plain` does not.
    t.check(
        TmFrameWriter::begin_oid(bb(&mut frame), mc, vc0, 0, 0, &mut oid)
            .is_err_and(|e| e.kind() == ErrorKind::InvalidField),
    );
    let plain = TmVirtualChannelConfig::new(vc0, DataFieldContent::Packets).with_fsh(bb(fsh));
    let Ok(oid_mc) = TmMasterChannelConfig::new(pc, scid, None, bb(false)).with_vc(bb(plain))
    else {
        t.check(false);
        return;
    };
    t.check(
        TmFrameWriter::begin_oid(bb(&mut frame), &oid_mc, vc0, 0, 0, &mut oid)
            .is_ok_and(|w| w.finish() == 32),
    );
    t.check(frame.get(10..14) == Some(&[0xFF; 4][..]));
    // The one-shot encoders give the same frame from either form of the generator.
    let (mut fib, mut gal, mut one) = (OidGenerator::new(), OidGeneratorGalois::new(), [0_u8; 32]);
    t.check(encode_oid_frame(&oid_mc, vc0, 0, 0, &[0; 3], None, &mut fib, &mut one) == Ok(32));
    t.check(one == frame);
    one.fill(0);
    t.check(encode_oid_galois(&oid_mc, vc0, 0, 0, &[0; 3], None, &mut gal, &mut one) == Ok(32));
    t.check(one == frame);
}

/// Runs the `aldis-tm` group.
pub fn run(t: &mut Tally) {
    header_group(t);
    frame_group(t);
}

// ----- vectors -----

fn fsh_of(v: Option<Val>) -> Result<Option<FshLen>, Fail> {
    match v {
        None | Some(Val::Null) => Ok(None),
        Some(n) => {
            let len = u8::try_from(need(n.num())?).map_err(|_| Fail::Vector)?;
            Ok(Some(FshLen::try_from(len)?))
        }
    }
}

fn opt_flag(map: Map, key: &str) -> bool {
    crate::vectors::get(map, key)
        .and_then(Val::bool)
        .unwrap_or(false)
}

fn opt_size(map: Map, key: &str) -> Result<usize, Fail> {
    match crate::vectors::get(map, key) {
        None => Ok(0),
        Some(v) => usize::try_from(need(v.num())?).map_err(|_| Fail::NotRepresentable),
    }
}

/// The physical channel, and the master channel when the configuration has an SCID, built in
/// the order of `crates/aldis-tm/tests/vectors.rs`.
fn config(cfg: Map) -> Result<(TmPhysicalChannelConfig, Option<TmMasterChannelConfig>), Fail> {
    let pc = TmPhysicalChannelConfig::new(bb(cfg.size("frame_len")?), bb(cfg.flag("fecf")?))?;
    if crate::vectors::get(cfg, "scid").is_none() {
        return Ok((pc, None));
    }
    let scid = SpacecraftId::<10>::try_from(cfg.num::<u16>("scid")?)?;
    let mut mc = TmMasterChannelConfig::new(
        pc,
        scid,
        fsh_of(crate::vectors::get(cfg, "mc_fsh"))?,
        opt_flag(cfg, "mc_ocf"),
    );
    for v in cfg.list("vcs")? {
        let v = need(v.obj())?;
        let vcid = VcId::<3>::try_from(v.num::<u8>("vcid")?)?;
        let content = match v.text("content")? {
            "packets" => DataFieldContent::Packets,
            "vca_sdu" => DataFieldContent::VcaSdu,
            _ => return Err(Fail::Vector),
        };
        let mut vc = TmVirtualChannelConfig::new(vcid, content);
        if let Some(len) = fsh_of(crate::vectors::get(v, "fsh"))? {
            vc = vc.with_fsh(len);
        }
        if opt_flag(v, "ocf") {
            vc = vc.with_ocf();
        }
        let security = SecurityLayout::new(
            opt_size(v, "security_header_len")?,
            opt_size(v, "security_trailer_len")?,
        )?;
        mc = mc.with_vc(bb(vc.with_security(security)))?;
    }
    Ok((pc, Some(mc)))
}

fn status(s: Map) -> Result<DataFieldStatus, Fail> {
    if s.flag("sync")? {
        Ok(DataFieldStatus::VcaSdu {
            packet_order_flag: s.flag("packet_order_flag")?,
            segment_length_id: need(SegmentLengthId::new(s.num("segment_length_id")?))?,
            first_header_pointer: need(FirstHeaderPointer::new(s.num("first_header_pointer")?))?,
        })
    } else {
        Ok(DataFieldStatus::Packets(match s.text("pointer")? {
            "starts_at" => {
                PacketPointer::StartsAt(PacketOffset::try_from(s.num::<u16>("packet_offset")?)?)
            }
            "no_packet_start" => PacketPointer::NoPacketStart,
            "only_idle_data" => PacketPointer::OnlyIdleData,
            _ => return Err(Fail::Vector),
        }))
    }
}

fn ocf_of(v: Option<Val>) -> Result<Option<Ocf>, Fail> {
    match v {
        None | Some(Val::Null) => Ok(None),
        Some(v) => {
            let octets: [u8; 4] = need(need(v.bytes())?.try_into().ok())?;
            Ok(Some(Ocf::decode(octets)))
        }
    }
}

const fn report(ocf: Ocf) -> &'static str {
    match ocf {
        Ocf::Clcw(_) => "clcw",
        Ocf::ProjectSpecific(_) => "project_specific",
        Ocf::SdlsFsr(_) => "sdls_fsr",
    }
}

/// Every field the view exposes, as the host suite reports them (TV-002).
fn decoded(out: &mut Out, f: &UncheckedTmFrame<'_>, anomalies: AnomalySet<TmAnomaly>) {
    let h = f.header();
    out.num("scid", f.scid().get());
    out.num("vcid", f.vcid().get());
    out.flag("ocf_flag", h.ocf_flag);
    out.num("mc_frame_count", f.mc_frame_count());
    out.num("vc_frame_count", f.vc_frame_count());
    out.flag("fsh_flag", h.fsh_flag);
    out.flag("sync_flag", h.sync_flag);
    out.flag("packet_order_flag", h.packet_order_flag);
    out.num("segment_length_id", h.segment_length_id.get());
    out.num("first_header_pointer", h.first_header_pointer.get());
    let (pointer, offset) = match f.data_field_status() {
        DataFieldStatus::Packets(PacketPointer::StartsAt(n)) => (Some("starts_at"), Some(n.get())),
        DataFieldStatus::Packets(PacketPointer::NoPacketStart) => (Some("no_packet_start"), None),
        DataFieldStatus::Packets(PacketPointer::OnlyIdleData) => (Some("only_idle_data"), None),
        DataFieldStatus::VcaSdu { .. } => (None, None),
    };
    out.opt_text("pointer", pointer);
    out.opt_num("packet_offset", offset);
    out.opt_num("fsh_len", f.fsh_len().map(FshLen::get));
    match f.secondary_header_data() {
        Some(d) => out.bytes("fsh_data_hex", d),
        None => out.null("fsh_data_hex"),
    }
    out.bytes("security_header_hex", f.security_header());
    out.bytes("data_hex", f.data_field());
    out.bytes("security_trailer_hex", f.security_trailer());
    match f.ocf() {
        Some(o) => out.bytes("ocf_hex", &o.to_octets()),
        None => out.null("ocf_hex"),
    }
    out.opt_text("ocf_report", f.ocf().map(report));
    out.opt_num("fecf", f.fecf());
    out.num("consumed", u64::try_from(f.len()).unwrap_or(u64::MAX));
    out.names("anomalies", anomalies.iter());
}

/// `encode`, checked against the two-phase writer (API-020).
fn encode_case(p: Map, mc: &TmMasterChannelConfig, out: &mut Out, scratch: &mut Scratch) -> Run {
    let fields = TmFrameFields::new(
        VcId::<3>::try_from(p.num::<u8>("vcid")?)?,
        p.num("mc_frame_count")?,
        p.num("vc_frame_count")?,
        status(p.obj("status")?)?,
    );
    let content = TmFrameContent {
        secondary_header_data: p.bytes("fsh_data_hex")?,
        security_header: p.bytes("security_header_hex")?,
        data: p.bytes("data_hex")?,
        security_trailer: p.bytes("security_trailer_hex")?,
        ocf: ocf_of(crate::vectors::get(p, "ocf_hex"))?,
    };
    let len = match crate::vectors::get(p, "out_len") {
        Some(_) => p.size("out_len")?,
        None => mc.pc().frame_len(),
    };
    let n = encode(
        mc,
        bb(&fields),
        bb(&content),
        bb(fresh(&mut scratch.a, len)?),
    )?;
    let mut w = TmFrameWriter::begin(bb(fresh(&mut scratch.b, n)?), mc, &fields)
        .map_err(|_| Fail::Vector)?;
    let ok = copy(w.secondary_header_data_mut(), content.secondary_header_data)
        && copy(w.security_header_mut(), content.security_header)
        && copy(w.data_field_mut(), content.data)
        && copy(w.security_trailer_mut(), content.security_trailer)
        && content.ocf.is_none_or(|o| w.set_ocf(o).is_ok());
    if !ok || w.finish() != n {
        return Err(Fail::Vector);
    }
    let (written, two) = (need(scratch.a.get(..n))?, need(scratch.b.get(..n))?);
    if written != two {
        return Err(Fail::Vector);
    }
    out.bytes("output_hex", written);
    Ok(())
}

/// Consecutive OID frames from one generator (TM-010).
fn encode_oid(p: Map, mc: &TmMasterChannelConfig, out: &mut Out, scratch: &mut Scratch) -> Run {
    let vcid = VcId::<3>::try_from(p.num::<u8>("vcid")?)?;
    let mut oid = OidGenerator::new();
    let mut list = out.list("frames_hex");
    for f in p.list("frames")? {
        let f = need(f.obj())?;
        let buf = fresh(&mut scratch.a, mc.pc().frame_len())?;
        let mut w = TmFrameWriter::begin_oid(
            bb(buf),
            mc,
            vcid,
            f.num("mc_frame_count")?,
            f.num("vc_frame_count")?,
            &mut oid,
        )?;
        if let Some(data) = crate::vectors::get(f, "fsh_data_hex")
            && !copy(w.secondary_header_data_mut(), need(data.bytes())?)
        {
            return Err(Fail::Vector);
        }
        if let Some(ocf) = ocf_of(crate::vectors::get(f, "ocf_hex"))? {
            w.set_ocf(ocf)?;
        }
        let n = w.finish();
        list.bytes(need(scratch.a.get(..n))?);
    }
    out.end(&list);
    Ok(())
}

/// One `test-vectors/tm/` case, as `crates/aldis-tm/tests/vectors.rs` runs it.
pub fn vector(case: &Case, out: &mut Out, scratch: &mut Scratch) -> Run {
    let mode = if case.lenient {
        DecodeMode::Lenient
    } else {
        DecodeMode::Strict
    };
    let (pc, mc) = config(case.config)?;
    let input = bb(case.input);
    match case.op {
        "config" => {}
        "peek_length" => out.num(
            "len",
            u64::try_from(TmFrame::peek_length(input, &pc)?).unwrap_or(u64::MAX),
        ),
        "decode" => {
            let (f, a) = mode.dispatch(
                || TmFrame::decode(input, &pc).map(UncheckedTmFrame::from),
                || TmFrame::decode_lenient(input, &pc).map(|l| l.map(UncheckedTmFrame::from)),
            )?;
            decoded(out, &f, a);
        }
        "decode_with" => {
            let mc = need(mc.as_ref())?;
            let (f, a) = mode.dispatch(
                || TmFrame::decode_with(input, &pc, mc).map(UncheckedTmFrame::from),
                || {
                    TmFrame::decode_with_lenient(input, &pc, mc)
                        .map(|l| l.map(UncheckedTmFrame::from))
                },
            )?;
            decoded(out, &f, a);
        }
        "decode_unchecked" => {
            let (f, a) = mode.dispatch(
                || TmFrame::decode_unchecked(input, &pc, &NoSdls),
                || TmFrame::decode_unchecked_lenient(input, &pc, &NoSdls),
            )?;
            decoded(out, &f, a);
        }
        "validate" => {
            let mc = need(mc.as_ref())?;
            let (f, _) = mode.dispatch(
                || TmFrame::decode_with(input, &pc, mc),
                || TmFrame::decode_with_lenient(input, &pc, mc),
            )?;
            f.validate(mc)?;
        }
        "encode" => encode_case(case.params, need(mc.as_ref())?, out, scratch)?,
        "encode_oid" => encode_oid(case.params, need(mc.as_ref())?, out, scratch)?,
        _ => return Err(Fail::Vector),
    }
    Ok(())
}

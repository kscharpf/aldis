//! Calls every public `aldis-aos` API (VER-010, M2 D16) and runs every `test-vectors/aos/` case
//! (`vector`). Inputs pass through `black_box`.

use core::hint::black_box as bb;

use aldis_aos::fhec::{self, CorrectedSymbols};
use aldis_aos::{
    AosAnomaly, AosEdition, AosFrame, AosFrameContent, AosFrameFields, AosFrameWriter,
    AosPhysicalChannelConfig, AosPrimaryHeader, AosVirtualChannelConfig, BitstreamPointer, Bpdu,
    DataFieldContent, DataUnit, FrameCountCycle, Mpdu, OID_VCID, PacketPointer, PduHeader,
    VcFrameCount, VcLookup as _, VcTable, encode, encode_oid as encode_oid_frame,
    encode_oid_galois,
};
use aldis_aos::{PacketOffset, ValidBitCount};
use aldis_core::ids::{SpacecraftId, VcId};
use aldis_core::ocf::Ocf;
use aldis_core::oid::{OidGenerator, OidGeneratorGalois};
use aldis_core::security::SecurityLayout;
use aldis_core::{Anomaly as _, AnomalySet, DecodeMode, ErrorKind};

use crate::exercise::Tally;
use crate::vectors::{Case, Fail, Map, Out, Params as _, Run, Scratch, Val, copy, fresh, need};

/// The FHEC group: encode, write, and decode with zero, one, and two corrections and an
/// uncorrectable header (AOS-013, AOS-014).
fn fhec_group(t: &mut Tally) {
    let six = [0x40, 0x01, 0, 0, 0, 0];
    t.check(fhec::encode(bb(&six)) == [0x3E, 0xDF]);
    let mut header = [0x40, 0x01, 0, 0, 0, 0, 0xFF, 0xFF];
    fhec::write(bb(&mut header));
    t.check(header == [0x40, 0x01, 0, 0, 0, 0, 0x3E, 0xDF]);
    let clean = fhec::decode(bb(&header));
    t.check(
        clean
            .is_ok_and(|c| c.octets() == header && c.corrected_symbols() == CorrectedSymbols::Zero),
    );
    let one = fhec::decode(bb(&[0x50, 0x01, 0, 0, 0, 0, 0x3E, 0xDF]));
    t.check(one.is_ok_and(|c| c.octets() == header && c.corrected_symbols().get() == 1));
    let two = fhec::decode(bb(&[0xBF, 0x01, 0, 0, 0, 0, 0x3E, 0xDF]));
    t.check(two.is_ok_and(|c| c.corrected_symbols() == CorrectedSymbols::Two));
    let bad = fhec::decode(bb(&[0x51, 0x11, 0, 0, 0, 0, 0x3E, 0xDF]));
    t.check(bad.is_err_and(|e| e.kind() == ErrorKind::HeaderUncorrectable));
}

/// The identifiers, the `M_PDU` and `B_PDU` pointers, and the anomaly flags (AOS-003, AOS-017).
fn pointer_group(t: &mut Tally) {
    t.check(VcFrameCount::new(bb(0x100_0000)).is_none());
    t.check(VcFrameCount::try_from(bb(0x100_0000_u32)).is_err());
    t.check(VcFrameCount::try_from(bb(7_u32)).is_ok_and(|c| c.get() == 7 && u32::from(c) == 7));
    t.check(
        FrameCountCycle::new(bb(16)).is_none() && FrameCountCycle::try_from(bb(16_u8)).is_err(),
    );
    t.check(
        FrameCountCycle::try_from(bb(15_u8)).is_ok_and(|c| u8::from(c) == FrameCountCycle::MAX),
    );
    let (five, four) = (AosEdition::Issue5, AosEdition::Issue4Compatible);
    t.check(five.max_scid() == 1023 && bb(four).max_scid() == 255);
    t.check(PacketPointer::no_packet_start_raw(bb(five)) == 0xFFFF);
    t.check(PacketPointer::only_idle_data_raw(bb(four)) == 0x7FE);
    t.check(PacketPointer::max_offset(bb(four)) == 0x7FD);
    t.check(matches!(
        PacketPointer::classify(bb(3), bb(4), five),
        Ok(PacketPointer::StartsAt(n)) if n.get() == 3 && u16::from(n) == 3
    ));
    t.check(PacketPointer::classify(bb(4), bb(4), five).is_err());
    t.check(PacketPointer::classify(bb(0xFFFE), bb(70_000), four).is_err());
    t.check(
        PacketOffset::new(bb(0x7FE))
            .is_some_and(|n| PacketPointer::StartsAt(n).to_raw(four).is_none()),
    );
    t.check(
        PacketOffset::new(bb(PacketOffset::MAX)).is_some()
            && PacketOffset::new(bb(0xFFFE)).is_none(),
    );
    t.check(PacketPointer::OnlyIdleData.header_octets(bb(five)) == Some([0xFF, 0xFE]));
    t.check(PacketPointer::classify(bb(0x800), bb(0x1000), four).is_err());
    t.check(ValidBitCount::new(bb(15)).is_some_and(|n| {
        BitstreamPointer::classify(bb(14), bb(2)) == Ok(BitstreamPointer::ValidBits(n))
            && n.get() == 15
            && n.last_bit() == 14
            && u16::from(n) == 15
    }));
    t.check(
        ValidBitCount::new(bb(0)).is_none() && ValidBitCount::try_from(bb(0x3FFF_u16)).is_err(),
    );
    t.check(
        ValidBitCount::try_from(bb(ValidBitCount::MAX)).is_ok_and(|n| n.get() > ValidBitCount::MIN),
    );
    t.check(
        BitstreamPointer::classify(bb(15), bb(2))
            .is_err_and(|e| e.kind() == ErrorKind::InvalidField),
    );
    t.check(
        BitstreamPointer::classify(bb(16), bb(2)).is_err_and(|e| e.kind() == ErrorKind::Malformed),
    );
    t.check(BitstreamPointer::AllValid.to_raw() == bb(BitstreamPointer::ALL_VALID_RAW));
    t.check(BitstreamPointer::OnlyIdle.header_octets() == [0x3F, 0xFE]);
    let field = [0xA8, 0x01, 0, 0];
    t.check(Mpdu::decode(bb(&field), four, bb(DecodeMode::Strict)).is_err());
    t.check(
        Mpdu::decode(bb(&field), four, bb(DecodeMode::Lenient)).is_ok_and(|m| {
            matches!(m.pointer(), PacketPointer::StartsAt(n) if n.get() == 1)
                && m.first_header_pointer() == 1
                && m.issue4_spare() == 0x15
                && m.packet_zone().len() == 2
                && m.anomalies().contains(AosAnomaly::Issue4SpareSet)
        }),
    );
    let bfield = [0xBF, 0xFF, 0];
    t.check(Bpdu::decode(bb(&bfield), bb(DecodeMode::Strict)).is_err());
    t.check(
        Bpdu::decode(bb(&bfield), bb(DecodeMode::Lenient)).is_ok_and(|b| {
            b.pointer() == BitstreamPointer::AllValid
                && b.bitstream_data_pointer() == 0x3FFF
                && b.spare() == 2
                && b.data_zone().len() == 1
                && b.anomalies().contains(AosAnomaly::BpduSpareSet)
        }),
    );
    t.check(
        AosAnomaly::ALL.iter().map(|a| a.bit()).eq([0, 1, 2])
            && AosAnomaly::from_bit(bb(3)).is_none(),
    );
    t.check(AnomalySet::from_flags(bb(AosAnomaly::ALL)).bits() == 0b111);
}

/// The configuration, the table, the header decoder, the frame decoder, and the writers.
fn frame_group(t: &mut Tally) {
    let (Some(scid), Some(vc0), Some(vc1)) = (
        SpacecraftId::<10>::new(bb(0x0AB)),
        VcId::<6>::new(bb(0)),
        VcId::<6>::new(bb(1)),
    ) else {
        t.check(false);
        return;
    };
    t.check(AosPhysicalChannelConfig::new(bb(10), true, 4, true, AosEdition::Issue5).is_err());
    let Ok(pc) =
        AosPhysicalChannelConfig::new(bb(40), bb(true), bb(4), bb(true), AosEdition::default())
    else {
        t.check(false);
        return;
    };
    t.check(pc.frame_len() == 40 && pc.fhec() && pc.insert_zone_len() == 4 && pc.fecf());
    t.check(pc.edition() == AosEdition::Issue5 && pc.primary_header_len() == 8);
    t.check(AosVirtualChannelConfig::new(scid, bb(OID_VCID), DataFieldContent::IdleData).is_err());
    let security = SecurityLayout::new(bb(2), bb(1)).unwrap_or(SecurityLayout::NONE);
    let (Ok(mpdu), Ok(vca)) = (
        AosVirtualChannelConfig::new(scid, vc0, bb(DataFieldContent::Mpdu)),
        AosVirtualChannelConfig::new(scid, vc1, bb(DataFieldContent::VcaSdu)),
    ) else {
        t.check(false);
        return;
    };
    let mpdu = mpdu.with_ocf().with_security(bb(security));
    t.check(mpdu.scid() == scid && mpdu.vcid() == vc0 && mpdu.ocf());
    t.check(mpdu.content() == DataFieldContent::Mpdu && mpdu.security() == security);
    t.check(
        VcTable::<2>::new(pc, bb(&[mpdu, mpdu]))
            .is_err_and(|e| e.kind() == ErrorKind::InvalidConfig),
    );
    let Ok(table) = VcTable::<2>::new(pc, bb(&[mpdu, vca])) else {
        t.check(false);
        return;
    };
    t.check(table.physical_channel() == pc && table.vcs().count() == 2);
    t.check(table.has_spacecraft(bb(scid)) && table.vc(scid, vc1) == Some(vca));
    t.check(
        table
            .frame_layout(scid, vc0)
            .is_some_and(|l| l.data().len() == 19),
    );
    t.check(
        table
            .frame_layout(scid, OID_VCID)
            .is_some_and(|l| l.data().len() == 26),
    );
    frames(t, &pc, &table, scid, vc0, vc1);
}

/// Frames of `table`'s VC 0 (`M_PDUs`, an OCF, SDLS 2/1) and VC 1 (`VCA_SDUs`): the writers, the
/// decoders, and OID frames.
#[expect(
    clippy::too_many_lines,
    reason = "one call per writer, decoder, and OID encoder"
)]
fn frames(
    t: &mut Tally,
    pc: &AosPhysicalChannelConfig,
    table: &VcTable<2>,
    scid: SpacecraftId<10>,
    vc0: VcId<6>,
    vc1: VcId<6>,
) {
    let mut frame = [0_u8; 40];
    let Some(count) = VcFrameCount::new(bb(0x12_3456)) else {
        t.check(false);
        return;
    };
    let Some(offset) = PacketOffset::new(bb(3)) else {
        t.check(false);
        return;
    };
    let fields = AosFrameFields {
        replay_flag: bb(true),
        cycle: FrameCountCycle::new(bb(9)),
        ..AosFrameFields::new(
            scid,
            vc0,
            count,
            PduHeader::Mpdu(PacketPointer::StartsAt(offset)),
        )
    };
    match AosFrameWriter::begin(bb(&mut frame), pc, table, bb(&fields)) {
        Ok(mut w) => {
            t.check(copy(w.insert_zone_mut(), &[1, 2, 3, 4]));
            t.check(copy(w.security_header_mut(), &[0xA5, 0xA5]));
            w.zone_mut().fill(bb(0xD0));
            t.check(copy(w.security_trailer_mut(), &[0x5A]));
            t.check(w.set_ocf(Ocf::decode(bb([0x01, 0, 0, 0]))).is_ok());
            t.check(w.layout().frame_len() == 40);
            t.check(w.finish() == 40);
        }
        Err(_) => t.check(false),
    }
    let mut bad = frame;
    bad[0] ^= bb(0x10);
    match AosFrame::decode(bb(&bad), pc, table, bb(DecodeMode::Strict)) {
        Ok(f) => {
            t.check(f.scid() == scid && f.vcid() == vc0 && f.vc_frame_count() == count);
            t.check(f.replay_flag() && f.vc_frame_count_cycle() == FrameCountCycle::new(9));
            t.check(f.corrected_symbols() == CorrectedSymbols::One && !f.is_oid());
            t.check(f.insert_zone() == [1, 2, 3, 4] && f.security_header() == [0xA5, 0xA5]);
            t.check(f.security_trailer() == [0x5A] && f.ocf() == Some(Ocf::decode([1, 0, 0, 0])));
            t.check(
                f.data_field().len() == 19 && f.fecf().is_some() && f.layout().data() == (14..33),
            );
            t.check(
                f.data_unit().content() == DataFieldContent::Mpdu
                    && f.data_unit().zone().len() == 17,
            );
            t.check(matches!(f.data_unit(), DataUnit::Mpdu(m) if m.pointer() == PacketPointer::StartsAt(offset)));
            t.check(
                f.as_bytes().len() == 40
                    && f.len() == 40
                    && !f.is_empty()
                    && f.anomalies().is_empty(),
            );
            let h = f.header();
            t.check(
                h.edition() == AosEdition::Issue5
                    && h.cycle_use_flag()
                    && h.cycle_field().get() == 9,
            );
            t.check(h.issue4_spare() == 0 && h.fhec().is_some() && h.len() == 8 && !h.is_empty());
            t.check(h.to_octets().get(..8) == frame.get(..8));
        }
        Err(_) => t.check(false),
    }
    t.check(
        AosPrimaryHeader::decode(bb(&frame), pc).is_ok_and(|h| h.scid() == scid && h.vcid() == vc0),
    );
    t.check(AosFrame::peek_length(bb(&frame), pc) == Ok(40));
    let sdu = [0x55; 26];
    let content = AosFrameContent {
        insert_zone: &[9; 4],
        ..AosFrameContent::new(bb(&sdu))
    };
    let vca = AosFrameFields::new(scid, vc1, count, bb(PduHeader::None));
    t.check(encode(pc, table, bb(&vca), bb(&content), bb(&mut frame)) == Ok(40));
    t.check(
        AosFrame::decode(bb(&frame), pc, table, DecodeMode::Lenient)
            .is_ok_and(|f| f.data_unit() == DataUnit::VcaSdu(&sdu)),
    );
    let mut oid = OidGenerator::new();
    let Some(zero) = VcFrameCount::new(bb(0)) else {
        t.check(false);
        return;
    };
    t.check(
        AosFrameWriter::begin_oid(
            bb(&mut frame),
            pc,
            table,
            &AosFrameFields::oid(scid, zero),
            &mut oid,
        )
        .is_ok_and(|w| w.finish() == 40),
    );
    t.check(frame.get(12..16) == Some(&[0xFF; 4][..]));
    t.check(AosFrame::decode(bb(&frame), pc, table, DecodeMode::Strict).is_ok_and(|f| f.is_oid()));
    // The one-shot encoders give the same frame from either form of the generator.
    let (mut fib, mut gal, mut one) = (OidGenerator::new(), OidGeneratorGalois::new(), [0_u8; 40]);
    let fields = AosFrameFields::oid(scid, zero);
    t.check(encode_oid_frame(pc, table, bb(&fields), &[0; 4], &mut fib, &mut one) == Ok(40));
    t.check(one == frame);
    one.fill(0);
    t.check(encode_oid_galois(pc, table, bb(&fields), &[0; 4], &mut gal, &mut one) == Ok(40));
    t.check(one == frame);
}

/// Runs the `aldis-aos` group.
pub fn run(t: &mut Tally) {
    fhec_group(t);
    pointer_group(t);
    frame_group(t);
}

// ----- vectors -----

/// The table the vector handlers build, as `crates/aldis-aos/tests/vectors.rs` does.
type Table = VcTable<8>;

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

/// The physical channel and its table, built in the order of
/// `crates/aldis-aos/tests/vectors.rs`.
fn config(cfg: Map) -> Result<(AosPhysicalChannelConfig, Table), Fail> {
    let edition = match crate::vectors::get(cfg, "edition").and_then(Val::str) {
        None | Some("issue5") => AosEdition::Issue5,
        Some("issue4") => AosEdition::Issue4Compatible,
        Some(_) => return Err(Fail::Vector),
    };
    let pc = AosPhysicalChannelConfig::new(
        bb(cfg.size("frame_len")?),
        opt_flag(cfg, "fhec"),
        opt_size(cfg, "insert_zone_len")?,
        opt_flag(cfg, "fecf"),
        edition,
    )?;
    let list = match crate::vectors::get(cfg, "vcs") {
        Some(v) => need(v.list())?,
        None => &[],
    };
    // Unused slots hold a copy of a valid configuration and are never handed to the table.
    let filler = AosVirtualChannelConfig::new(
        need(SpacecraftId::new(0))?,
        need(VcId::new(0))?,
        DataFieldContent::IdleData,
    )?;
    let mut vcs = [filler; 8];
    let mut n = 0_usize;
    for v in list {
        let v = need(v.obj())?;
        let scid = SpacecraftId::<10>::try_from(v.num::<u16>("scid")?)?;
        let vcid = VcId::<6>::try_from(v.num::<u8>("vcid")?)?;
        let content = match v.text("content")? {
            "mpdu" => DataFieldContent::Mpdu,
            "bpdu" => DataFieldContent::Bpdu,
            "vca_sdu" => DataFieldContent::VcaSdu,
            "idle_data" => DataFieldContent::IdleData,
            _ => return Err(Fail::Vector),
        };
        let mut vc = AosVirtualChannelConfig::new(scid, vcid, content)?;
        if opt_flag(v, "ocf") {
            vc = vc.with_ocf();
        }
        let security = SecurityLayout::new(
            opt_size(v, "security_header_len")?,
            opt_size(v, "security_trailer_len")?,
        )?;
        *need(vcs.get_mut(n))? = vc.with_security(security);
        n = n.saturating_add(1);
    }
    let table = Table::new(pc, bb(vcs.get(..n).unwrap_or_default()))?;
    Ok((pc, table))
}

const fn report(ocf: Ocf) -> &'static str {
    match ocf {
        Ocf::Clcw(_) => "clcw",
        Ocf::ProjectSpecific(_) => "project_specific",
        Ocf::SdlsFsr(_) => "sdls_fsr",
    }
}

/// Every field of the Primary Header, as the host suite reports them (TV-002).
fn header_fields(out: &mut Out, h: &AosPrimaryHeader) {
    out.num("scid", h.scid().get());
    out.num("vcid", h.vcid().get());
    out.num("vc_frame_count", h.vc_frame_count().get());
    out.flag("replay_flag", h.replay_flag());
    out.flag("cycle_use_flag", h.cycle_use_flag());
    out.num("cycle_field", h.cycle_field().get());
    out.opt_num(
        "vc_frame_count_cycle",
        h.vc_frame_count_cycle().map(FrameCountCycle::get),
    );
    out.num("issue4_spare", h.issue4_spare());
    out.opt_num("fhec", h.fhec());
    out.num("corrected_symbols", h.corrected_symbols().get());
    out.bytes(
        "header_hex",
        h.to_octets().get(..h.len()).unwrap_or_default(),
    );
    out.flag("is_oid", h.is_oid());
}

/// Every field the view exposes, as the host suite reports them (TV-002).
fn decoded(out: &mut Out, f: &AosFrame<'_>) {
    header_fields(out, &f.header());
    out.bytes("insert_zone_hex", f.insert_zone());
    out.bytes("security_header_hex", f.security_header());
    out.bytes("data_field_hex", f.data_field());
    out.bytes("security_trailer_hex", f.security_trailer());
    match f.ocf() {
        Some(o) => out.bytes("ocf_hex", &o.to_octets()),
        None => out.null("ocf_hex"),
    }
    out.opt_text("ocf_report", f.ocf().map(report));
    out.opt_num("fecf", f.fecf());
    let (kind, raw, pointer, value, spare) = match f.data_unit() {
        DataUnit::Mpdu(m) => {
            let (p, v) = match m.pointer() {
                PacketPointer::StartsAt(n) => ("starts_at", Some(n.get())),
                PacketPointer::NoPacketStart => ("no_packet_start", None),
                PacketPointer::OnlyIdleData => ("only_idle_data", None),
            };
            (
                "mpdu",
                Some(m.first_header_pointer()),
                Some(p),
                v,
                Some(m.issue4_spare()),
            )
        }
        DataUnit::Bpdu(b) => {
            let (p, v) = match b.pointer() {
                BitstreamPointer::ValidBits(n) => ("valid_bits", Some(n.get())),
                BitstreamPointer::AllValid => ("all_valid", None),
                BitstreamPointer::OnlyIdle => ("only_idle", None),
            };
            (
                "bpdu",
                Some(b.bitstream_data_pointer()),
                Some(p),
                v,
                Some(b.spare()),
            )
        }
        DataUnit::VcaSdu(_) => ("vca_sdu", None, None, None, None),
        DataUnit::IdleData(_) => ("idle_data", None, None, None, None),
    };
    out.text("data_unit", kind);
    out.opt_num("pdu_pointer_raw", raw);
    out.opt_text("pointer", pointer);
    out.opt_num("pointer_value", value);
    out.opt_num("pdu_spare", spare);
    out.bytes("zone_hex", f.data_unit().zone());
    out.num("consumed", u64::try_from(f.len()).unwrap_or(u64::MAX));
    out.names("anomalies", f.anomalies().iter());
}

fn pdu(p: Map) -> Result<PduHeader, Fail> {
    let pointer = crate::vectors::get(p, "pointer").and_then(Val::str);
    Ok(match (p.text("kind")?, pointer) {
        ("mpdu", Some("starts_at")) => PduHeader::Mpdu(PacketPointer::StartsAt(
            PacketOffset::new(p.num("value")?).ok_or(Fail::Vector)?,
        )),
        ("mpdu", Some("no_packet_start")) => PduHeader::Mpdu(PacketPointer::NoPacketStart),
        ("mpdu", Some("only_idle_data")) => PduHeader::Mpdu(PacketPointer::OnlyIdleData),
        ("bpdu", Some("valid_bits")) => PduHeader::Bpdu(BitstreamPointer::ValidBits(
            ValidBitCount::try_from(p.num::<u16>("value")?)?,
        )),
        ("bpdu", Some("all_valid")) => PduHeader::Bpdu(BitstreamPointer::AllValid),
        ("bpdu", Some("only_idle")) => PduHeader::Bpdu(BitstreamPointer::OnlyIdle),
        ("none", _) => PduHeader::None,
        _ => return Err(Fail::Vector),
    })
}

fn cycle(p: Map) -> Result<Option<FrameCountCycle>, Fail> {
    match crate::vectors::get(p, "cycle") {
        None | Some(Val::Null) => Ok(None),
        Some(v) => {
            let n = u8::try_from(need(v.num())?).map_err(|_| Fail::Vector)?;
            Ok(Some(FrameCountCycle::try_from(n)?))
        }
    }
}

fn opt_bytes(p: Map, key: &str) -> Result<&'static [u8], Fail> {
    match crate::vectors::get(p, key) {
        None | Some(Val::Null) => Ok(&[]),
        Some(v) => need(v.bytes()),
    }
}

/// `encode`, checked against the two-phase writer (API-020).
fn encode_case(
    p: Map,
    pc: &AosPhysicalChannelConfig,
    table: &Table,
    out: &mut Out,
    scratch: &mut Scratch,
) -> Run {
    let fields = AosFrameFields {
        scid: SpacecraftId::<10>::try_from(p.num::<u16>("scid")?)?,
        vcid: VcId::<6>::try_from(p.num::<u8>("vcid")?)?,
        vc_frame_count: VcFrameCount::try_from(p.num::<u32>("vc_frame_count")?)?,
        replay_flag: opt_flag(p, "replay_flag"),
        cycle: cycle(p)?,
        pdu: pdu(p.obj("pdu")?)?,
    };
    let ocf = match crate::vectors::get(p, "ocf_hex") {
        None | Some(Val::Null) => None,
        Some(v) => Some(Ocf::decode(need(need(v.bytes())?.try_into().ok())?)),
    };
    let content = AosFrameContent {
        insert_zone: opt_bytes(p, "insert_zone_hex")?,
        security_header: opt_bytes(p, "security_header_hex")?,
        zone: opt_bytes(p, "zone_hex")?,
        security_trailer: opt_bytes(p, "security_trailer_hex")?,
        ocf,
    };
    let len = match crate::vectors::get(p, "out_len") {
        Some(_) => p.size("out_len")?,
        None => pc.frame_len(),
    };
    let n = encode(
        pc,
        table,
        bb(&fields),
        bb(&content),
        bb(fresh(&mut scratch.a, len)?),
    )?;
    let mut w = AosFrameWriter::begin(bb(fresh(&mut scratch.b, n)?), pc, table, &fields)
        .map_err(|_| Fail::Vector)?;
    let ok = copy(w.insert_zone_mut(), content.insert_zone)
        && copy(w.security_header_mut(), content.security_header)
        && copy(w.zone_mut(), content.zone)
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

/// Consecutive OID frames of one SCID from one generator (AOS-016).
fn encode_oid(
    p: Map,
    pc: &AosPhysicalChannelConfig,
    table: &Table,
    out: &mut Out,
    scratch: &mut Scratch,
) -> Run {
    let scid = SpacecraftId::<10>::try_from(p.num::<u16>("scid")?)?;
    let mut oid = OidGenerator::new();
    let mut list = out.list("frames_hex");
    for f in p.list("frames")? {
        let f = need(f.obj())?;
        let count = VcFrameCount::try_from(f.num::<u32>("vc_frame_count")?)?;
        let fields = AosFrameFields {
            cycle: cycle(f)?,
            ..AosFrameFields::oid(scid, count)
        };
        let buf = fresh(&mut scratch.a, pc.frame_len())?;
        let mut w = AosFrameWriter::begin_oid(bb(buf), pc, table, &fields, &mut oid)?;
        if let Some(iz) = crate::vectors::get(f, "insert_zone_hex")
            && !copy(w.insert_zone_mut(), need(iz.bytes())?)
        {
            return Err(Fail::Vector);
        }
        let n = w.finish();
        list.bytes(need(scratch.a.get(..n))?);
    }
    out.end(&list);
    Ok(())
}

/// The input of a case as an `N`-octet array.
fn array<const N: usize>(input: &[u8]) -> Result<[u8; N], Fail> {
    need(
        input
            .first_chunk::<N>()
            .filter(|_| input.len() == N)
            .copied(),
    )
}

/// One `test-vectors/aos/` case, as `crates/aldis-aos/tests/vectors.rs` runs it.
pub fn vector(case: &Case, out: &mut Out, scratch: &mut Scratch) -> Run {
    let input = bb(case.input);
    match case.op {
        "fhec_encode" => {
            let six: [u8; 6] = array(input)?;
            let mut header = [0xA5; 8];
            for (h, s) in header.iter_mut().zip(six) {
                *h = s;
            }
            fhec::write(bb(&mut header));
            out.bytes("fhec_hex", &fhec::encode(bb(&six)));
            out.bytes("output_hex", &header);
            return Ok(());
        }
        "fhec_decode" => {
            let corrected = fhec::decode(bb(&array(input)?))?;
            out.bytes("header_hex", &corrected.octets());
            out.num("corrected_symbols", corrected.corrected_symbols().get());
            return Ok(());
        }
        _ => {}
    }
    let mode = if case.lenient {
        DecodeMode::Lenient
    } else {
        DecodeMode::Strict
    };
    let (pc, table) = config(case.config)?;
    match case.op {
        "config" => {}
        "peek_length" => out.num(
            "len",
            u64::try_from(AosFrame::peek_length(input, &pc)?).unwrap_or(u64::MAX),
        ),
        "decode_header" => header_fields(out, &AosPrimaryHeader::decode(input, &pc)?),
        "decode" => decoded(out, &AosFrame::decode(input, &pc, &table, mode)?),
        "encode" => encode_case(case.params, &pc, &table, out, scratch)?,
        "encode_oid" => encode_oid(case.params, &pc, &table, out, scratch)?,
        _ => return Err(Fail::Vector),
    }
    Ok(())
}

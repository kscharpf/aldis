//! `AosFrame::decode`, `AosPrimaryHeader::decode`, `Mpdu::decode`, `Bpdu::decode`, and
//! `peek_length` on a virtual channel table and frame octets drawn from the input, in both
//! modes: no panic, every field inside the frame and the fields covering it in wire order, every
//! pointer inside its zone, strict ⊆ lenient, and a strict data frame with nothing corrected
//! re-encodes to the same octets (GEN-020, API-010, API-020, API-030, CMN-040).
#![no_main]

// req: GEN-020, API-010, API-012, API-013, API-020, API-030, API-032, AOS-010, AOS-011, AOS-014, AOS-017, VER-022

use aldis_aos::{
    AosEdition, AosFrame, AosFrameContent, AosFrameFields, AosPhysicalChannelConfig,
    AosPrimaryHeader, AosVirtualChannelConfig, BitstreamPointer, Bpdu, CorrectedSymbols,
    DataFieldContent, DataUnit, Mpdu, PacketPointer, PduHeader, VcTable, encode,
};
use aldis_core::ids::{SpacecraftId, VcId};
use aldis_core::security::SecurityLayout;
use aldis_core::{DecodeMode, ErrorKind};
use libfuzzer_sys::arbitrary::{Result, Unstructured};
use libfuzzer_sys::fuzz_target;

/// A physical channel and a table of its virtual channels drawn from the input; `None` if the
/// input's choices are rejected.
fn table(u: &mut Unstructured<'_>) -> Result<Option<(AosPhysicalChannelConfig, VcTable<4>)>> {
    let edition = if u.ratio(1, 4)? {
        AosEdition::Issue4Compatible
    } else {
        AosEdition::Issue5
    };
    let Ok(pc) = AosPhysicalChannelConfig::new(
        u.int_in_range(0..=300)?,
        u.arbitrary()?,
        u.int_in_range(0..=8)?,
        u.arbitrary()?,
        edition,
    ) else {
        return Ok(None);
    };
    let mut vcs = Vec::new();
    for _ in 0..u.int_in_range(1..=4)? {
        let content = match u.int_in_range(0..=3)? {
            0 => DataFieldContent::Mpdu,
            1 => DataFieldContent::Bpdu,
            2 => DataFieldContent::VcaSdu,
            _ => DataFieldContent::IdleData,
        };
        let scid = SpacecraftId::new(u.int_in_range(0..=1023)?).unwrap();
        let Ok(mut vc) = AosVirtualChannelConfig::new(
            scid,
            VcId::new(u.int_in_range(0..=63)?).unwrap(),
            content,
        ) else {
            continue;
        };
        if u.ratio(1, 4)? {
            vc = vc.with_ocf();
        }
        let security = SecurityLayout::new(u.int_in_range(0..=8)?, u.int_in_range(0..=8)?).unwrap();
        let mut with = vcs.clone();
        with.push(vc.with_security(security));
        if VcTable::<4>::new(pc, &with).is_ok() {
            vcs = with;
        }
    }
    Ok(VcTable::new(pc, &vcs).ok().map(|vcs_table| (pc, vcs_table)))
}

fn run(u: &mut Unstructured<'_>) -> Result<()> {
    let Some((pc, vcs_table)) = table(u)? else {
        return Ok(());
    };
    let len = if u.ratio(7, 8)? {
        pc.frame_len()
    } else {
        u.int_in_range(0..=320)?
    };
    let bytes = u.bytes(len.min(u.len()))?;
    let strict = AosFrame::decode(bytes, &pc, &vcs_table, DecodeMode::Strict);
    let lenient = AosFrame::decode(bytes, &pc, &vcs_table, DecodeMode::Lenient);
    match (&strict, &lenient) {
        (Ok(s), Ok(l)) => assert_eq!(s, l),
        (Ok(_), Err(e)) => panic!("lenient rejected a strict frame: {e}"),
        (Err(s), Ok(l)) => {
            assert!(matches!(
                s.kind(),
                ErrorKind::ReservedValue | ErrorKind::InvalidField
            ));
            assert!(!l.anomalies().is_empty());
        }
        (Err(s), Err(l)) => assert_eq!(s, l),
    }
    let peek = AosFrame::peek_length(bytes, &pc);
    let header = AosPrimaryHeader::decode(bytes, &pc);
    for mode in [DecodeMode::Strict, DecodeMode::Lenient] {
        let _ = Mpdu::decode(bytes, pc.edition(), mode);
        let _ = Bpdu::decode(bytes, mode);
    }
    if let Ok(f) = &lenient {
        assert_eq!(peek, Ok(f.len()));
        assert_eq!(header, Ok(f.header()));
        let mut at = 0;
        for range in f.layout().ranges() {
            assert_eq!(range.start, at);
            at = range.end;
        }
        assert_eq!(at, bytes.len());
        assert!(f.data_field().ends_with(f.data_unit().zone()));
        assert_ne!(f.data_unit().zone().len(), 0);
        if let DataUnit::Mpdu(m) = f.data_unit()
            && let PacketPointer::StartsAt(n) = m.pointer()
        {
            assert!(usize::from(n) < m.packet_zone().len());
        }
        if let DataUnit::Bpdu(b) = f.data_unit()
            && let BitstreamPointer::ValidBits(n) = b.pointer()
        {
            assert!(usize::from(n.get()) < b.data_zone().len() * 8);
        }
    }
    if let Ok(f) = strict
        && !f.is_oid()
        && f.corrected_symbols() == CorrectedSymbols::Zero
    {
        assert!(f.anomalies().is_empty());
        let fields = AosFrameFields {
            scid: f.scid(),
            vcid: f.vcid(),
            vc_frame_count: f.vc_frame_count(),
            replay_flag: f.replay_flag(),
            cycle: f.vc_frame_count_cycle(),
            pdu: match f.data_unit() {
                DataUnit::Mpdu(m) => PduHeader::Mpdu(m.pointer()),
                DataUnit::Bpdu(b) => PduHeader::Bpdu(b.pointer()),
                DataUnit::VcaSdu(_) | DataUnit::IdleData(_) => PduHeader::None,
            },
        };
        let content = AosFrameContent {
            insert_zone: f.insert_zone(),
            security_header: f.security_header(),
            zone: f.data_unit().zone(),
            security_trailer: f.security_trailer(),
            ocf: f.ocf(),
        };
        let mut out = vec![0_u8; bytes.len()];
        assert_eq!(
            encode(&pc, &vcs_table, &fields, &content, &mut out),
            Ok(bytes.len())
        );
        assert_eq!(out, bytes);
    }
    Ok(())
}

fuzz_target!(|data: &[u8]| {
    let _ = run(&mut Unstructured::new(data));
});

//! Property tests of `aldis-aos` (VER-023), drawing their inputs from the shared strategies of
//! `aldis-testkit` (VER-054). They are an integration test so that the strategies and the tests
//! see the same copy of the crate (M2 plan D6).
#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "property helpers outside #[test] fail loudly (RS-018)"
)]

use aldis_aos::{
    AosFrame, AosFrameContent, AosFrameFields, AosFrameWriter, CorrectedSymbols, DataUnit,
    PduHeader, VcLookup, encode,
};
use aldis_core::oid::OidGenerator;
use aldis_core::security::{FrameCtx, NoSecurity};
use aldis_core::{DecodeMode, ErrorKind};
use aldis_testkit::strategies::aos::{
    AosTable, aos_frame, aos_frame_decoder_input, vc_frame_count, vc_table,
};
use aldis_testkit::strategies::proptest::prelude::*;
use aldis_testkit::strategies::proptest::test_runner::TestCaseError;
use aldis_testkit::strategies::{self, octets};

/// Failing cases are saved here and replayed first.
const REGRESSIONS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/proptest-regressions/properties.txt"
);

/// The data unit header the writer takes for a decoded data unit (AOS-019).
const fn pdu_of(unit: DataUnit<'_>) -> PduHeader {
    match unit {
        DataUnit::Mpdu(m) => PduHeader::Mpdu(m.pointer()),
        DataUnit::Bpdu(b) => PduHeader::Bpdu(b.pointer()),
        DataUnit::VcaSdu(_) | DataUnit::IdleData(_) => PduHeader::None,
    }
}

/// A strict decode of a data frame with nothing corrected re-encodes to exactly the octets
/// decoded, and lenient decodes it the same (API-020, API-010, GEN-020).
fn strict_re_encode_property(table: &AosTable, bytes: &[u8]) -> Result<(), TestCaseError> {
    let pc = table.physical_channel();
    let Ok(f) = AosFrame::decode(bytes, &pc, table, DecodeMode::Strict) else {
        return Ok(());
    };
    prop_assert!(f.anomalies().is_empty());
    let lenient = AosFrame::decode(bytes, &pc, table, DecodeMode::Lenient).unwrap();
    prop_assert_eq!(&lenient, &f);
    if f.is_oid() || f.corrected_symbols() != CorrectedSymbols::Zero {
        return Ok(());
    }
    let fields = AosFrameFields {
        scid: f.scid(),
        vcid: f.vcid(),
        vc_frame_count: f.vc_frame_count(),
        replay_flag: f.replay_flag(),
        cycle: f.vc_frame_count_cycle(),
        pdu: pdu_of(f.data_unit()),
    };
    let content = AosFrameContent {
        insert_zone: f.insert_zone(),
        security_header: f.security_header(),
        zone: f.data_unit().zone(),
        security_trailer: f.security_trailer(),
        ocf: f.ocf(),
    };
    let mut out = vec![0_u8; bytes.len()];
    let n = encode(&pc, table, &fields, &content, &mut out).unwrap();
    prop_assert_eq!(&out[..n], bytes);
    Ok(())
}

/// Lenient accepts whatever strict accepts, and more only with an anomaly flag; strict and
/// lenient reject with the same error otherwise (API-030, API-032, GEN-020).
fn lenient_superset_property(table: &AosTable, bytes: &[u8]) -> Result<(), TestCaseError> {
    let pc = table.physical_channel();
    let strict = AosFrame::decode(bytes, &pc, table, DecodeMode::Strict);
    let lenient = AosFrame::decode(bytes, &pc, table, DecodeMode::Lenient);
    match (strict, lenient) {
        (Ok(s), Ok(l)) => prop_assert_eq!(s, l),
        (Ok(_), Err(e)) => prop_assert!(false, "lenient rejected a strict frame: {e}"),
        (Err(s), Ok(l)) => {
            prop_assert!(matches!(
                s.kind(),
                ErrorKind::ReservedValue | ErrorKind::InvalidField
            ));
            prop_assert!(!l.anomalies().is_empty());
        }
        (Err(s), Err(l)) => prop_assert_eq!(s, l),
    }
    Ok(())
}

/// Every slice of a decoded view lies inside the input, in wire order, the fields cover the
/// frame exactly, and a pointer lies inside its zone (API-010, CMN-040, AOS-017).
fn slices_inside_property(table: &AosTable, bytes: &[u8]) -> Result<(), TestCaseError> {
    let pc = table.physical_channel();
    let Ok(f) = AosFrame::decode(bytes, &pc, table, DecodeMode::Lenient) else {
        return Ok(());
    };
    prop_assert_eq!(f.len(), bytes.len());
    prop_assert_eq!(f.layout().frame_len(), bytes.len());
    prop_assert!(!f.data_field().is_empty() && !f.data_unit().zone().is_empty());
    prop_assert_eq!(f.insert_zone().len(), pc.insert_zone_len());
    let mut at = 0;
    for range in f.layout().ranges() {
        prop_assert_eq!(range.start, at);
        at = range.end;
    }
    prop_assert_eq!(at, bytes.len());
    prop_assert!(f.data_field().ends_with(f.data_unit().zone()));
    if let DataUnit::Mpdu(m) = f.data_unit()
        && let aldis_aos::PacketPointer::StartsAt(n) = m.pointer()
    {
        prop_assert!(usize::from(n.get()) < m.packet_zone().len());
    }
    if let DataUnit::Bpdu(b) = f.data_unit()
        && let aldis_aos::BitstreamPointer::ValidBits(n) = b.pointer()
    {
        prop_assert!(usize::from(n.get()) < b.data_zone().len().saturating_mul(8));
    }
    prop_assert_eq!(AosFrame::peek_length(bytes, &pc), Ok(bytes.len()));
    Ok(())
}

proptest! {
    #![proptest_config(strategies::config(256, REGRESSIONS))]

    #[test]
    fn encode_then_decode_round_trips(f in aos_frame()) {
        // req: API-020, AOS-019, AOS-010, AOS-012, AOS-015, AOS-017, VER-023
        let pc = f.pc();
        let d = AosFrame::decode(&f.octets, &pc, &f.table, DecodeMode::Strict).unwrap();
        prop_assert_eq!((d.scid(), d.vcid()), (f.fields.scid, f.fields.vcid));
        prop_assert_eq!(d.vc_frame_count(), f.fields.vc_frame_count);
        prop_assert_eq!(d.replay_flag(), f.fields.replay_flag);
        prop_assert_eq!(d.vc_frame_count_cycle(), f.fields.cycle);
        prop_assert_eq!(pdu_of(d.data_unit()), f.fields.pdu);
        prop_assert_eq!(d.insert_zone(), f.insert_zone.as_slice());
        prop_assert_eq!(d.security_header(), f.security_header.as_slice());
        prop_assert_eq!(d.data_unit().zone(), f.zone.as_slice());
        prop_assert_eq!(d.security_trailer(), f.security_trailer.as_slice());
        prop_assert_eq!(d.ocf(), f.ocf);
        prop_assert_eq!(d.header().fhec().is_some(), pc.fhec());
        prop_assert_eq!(d.fecf().is_some(), pc.fecf());
        prop_assert_eq!(d.corrected_symbols(), CorrectedSymbols::Zero);
        prop_assert!(d.anomalies().is_empty());
        prop_assert_eq!(d.header().to_octets()[..d.header().len()].to_vec(), f.octets[..d.header().len()].to_vec());
        prop_assert_eq!(AosFrame::peek_length(&f.octets, &pc), Ok(f.octets.len()));
    }

    #[test]
    fn two_phase_writer_matches_encode(f in aos_frame()) {
        // req: API-020, API-023, AOS-019, VER-023
        let pc = f.pc();
        let mut out = vec![0_u8; f.octets.len()];
        let mut w = AosFrameWriter::begin(&mut out, &pc, &f.table, &f.fields).unwrap();
        w.insert_zone_mut().copy_from_slice(&f.insert_zone);
        w.security_header_mut().copy_from_slice(&f.security_header);
        w.zone_mut().copy_from_slice(&f.zone);
        w.security_trailer_mut().copy_from_slice(&f.security_trailer);
        if let Some(ocf) = f.ocf {
            w.set_ocf(ocf).unwrap();
        }
        let ctx = FrameCtx::Aos { scid: f.fields.scid, vcid: f.fields.vcid };
        prop_assert_eq!(w.protect(&mut NoSecurity, &ctx).unwrap().finalize().len(), f.octets.len());
        prop_assert_eq!(out, f.octets);
    }

    #[test]
    fn rejected_encode_leaves_the_buffer(f in aos_frame(), short in 1_usize..9) {
        // req: API-022, GEN-042, VER-023
        let len = f.octets.len().saturating_sub(short);
        let mut out = vec![0xEE_u8; len];
        let e = encode(&f.pc(), &f.table, &f.fields, &f.content(), &mut out).unwrap_err();
        prop_assert_eq!(e.kind(), ErrorKind::BufferTooSmall);
        prop_assert!(out.iter().all(|&b| b == 0xEE));
    }

    #[test]
    fn two_symbol_errors_are_corrected(
        f in aos_frame().prop_filter("a channel with the FHEC", |f| f.pc().fhec()),
        hits in strategies::proptest::collection::btree_map(0_usize..10, 1_u8..16, 1..=2),
    ) {
        // req: AOS-013, AOS-014, VER-023
        // The ten transmitted symbols are the nibbles of octets 0, 1, 5, 6, and 7.
        let mut bad = f.octets.clone();
        for (&symbol, &value) in &hits {
            let octet = [0, 1, 5, 6, 7][symbol / 2];
            bad[octet] ^= if symbol % 2 == 0 { value << 4 } else { value };
        }
        let pc = f.pc();
        let good = AosFrame::decode(&f.octets, &pc, &f.table, DecodeMode::Strict).unwrap();
        let d = AosFrame::decode(&bad, &pc, &f.table, DecodeMode::Strict).unwrap();
        prop_assert_eq!(d.corrected_symbols().get(), u8::try_from(hits.len()).unwrap());
        prop_assert_eq!(d.header().to_octets(), good.header().to_octets());
        prop_assert_eq!(d.data_unit(), good.data_unit());
        prop_assert_eq!(d.as_bytes(), bad.as_slice());
    }

    #[test]
    fn oid_frames_decode_as_idle_data(t in vc_table(), count in vc_frame_count(), skip in 0_usize..64) {
        // req: AOS-016, AOS-015, CMN-021, GEN-042
        let pc = t.physical_channel();
        let scid = t.vcs().next().unwrap().scid();
        prop_assert!(t.has_spacecraft(scid));
        let mut oid = OidGenerator::new();
        let mut reference = OidGenerator::new();
        let mut skipped = vec![0_u8; skip];
        oid.fill(&mut skipped);
        reference.fill(&mut skipped);
        let mut out = vec![0_u8; pc.frame_len()];
        let w = AosFrameWriter::begin_oid(&mut out, &pc, &t, &AosFrameFields::oid(scid, count), &mut oid).unwrap();
        prop_assert_eq!(w.unsecured().unwrap().finalize().len(), pc.frame_len());
        let d = AosFrame::decode(&out, &pc, &t, DecodeMode::Strict).unwrap();
        prop_assert!(d.is_oid());
        prop_assert_eq!(d.vc_frame_count(), count);
        let mut expected = vec![0_u8; d.data_field().len()];
        reference.fill(&mut expected);
        prop_assert_eq!(d.data_unit(), DataUnit::IdleData(&expected));
        prop_assert_eq!(oid, reference);
        prop_assert_eq!((d.ocf(), d.security_header().len()), (None, 0));
    }

    #[test]
    fn decoders_never_panic((table, bytes) in aos_frame_decoder_input()) {
        // req: GEN-020, API-010, API-030, API-032, AOS-017, VER-023
        strict_re_encode_property(&table, &bytes)?;
        lenient_superset_property(&table, &bytes)?;
        slices_inside_property(&table, &bytes)?;
    }

    #[test]
    fn arbitrary_octets_never_panic((table, _) in aos_frame_decoder_input(), bytes in octets(0..64)) {
        // req: GEN-020, VER-023
        let pc = table.physical_channel();
        for mode in [DecodeMode::Strict, DecodeMode::Lenient] {
            let _ = AosFrame::decode(&bytes, &pc, &table, mode);
            let _ = aldis_aos::Mpdu::decode(&bytes, pc.edition(), mode);
            let _ = aldis_aos::Bpdu::decode(&bytes, mode);
        }
        let _ = AosFrame::peek_length(&bytes, &pc);
        let _ = aldis_aos::AosPrimaryHeader::decode(&bytes, &pc);
    }
}

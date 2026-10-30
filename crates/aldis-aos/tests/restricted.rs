//! A test for every restricted type of `aldis-aos` (RS-020, VER-023; M2 plan WP-7): each public
//! type with a checked constructor accepts exactly the range 732.0-B-5 gives it, rejects every
//! other value with the documented error, and carries every accepted value through its wire
//! field and back. The bounds are written from the clauses cited, as field widths and reserved
//! values, and compared with the type's own constants. Inputs come from the shared strategies
//! of `aldis-testkit` (VER-054); each `// restricted:` marker names the type a test covers, and
//! `cargo xtask restricted` checks that every restricted type has one.
#![expect(
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test helpers on small bounded values fail loudly (RS-018)"
)]

use aldis_aos::field;
use aldis_aos::{
    AosEdition, AosFrame, AosFrameContent, AosFrameFields, AosPhysicalChannelConfig,
    AosVirtualChannelConfig, BitstreamPointer, Bpdu, DataFieldContent, FrameCountCycle, Mpdu,
    OID_VCID, PacketOffset, PacketPointer, PduHeader, ValidBitCount, VcFrameCount, VcTable, encode,
};
use aldis_core::ids::{SpacecraftId, VcId};
use aldis_core::{Error, ErrorDetail, ErrorKind, FieldId};
use aldis_testkit::strategies;
use aldis_testkit::strategies::aos::{
    EncodedAosFrame, aos_frame, content, cycle, physical_channel, vc_frame_count,
};
use aldis_testkit::strategies::common::spacecraft_id;
use aldis_testkit::strategies::proptest::prelude::*;

/// Failing cases are saved here and replayed first.
const REGRESSIONS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/proptest-regressions/restricted.txt"
);

/// Width of the Virtual Channel Frame Count: bits 16–39 (732.0 §4.1.2.4.1–2).
const VCF_COUNT_BITS: u32 = 39 - 16 + 1;
/// Width of the Virtual Channel Frame Count Cycle: bits 44–47 (732.0 §4.1.2.5.5.1).
const CYCLE_BITS: u32 = 47 - 44 + 1;
/// Width of the First Header Pointer of Issue 5: bits 0–15 of the `M_PDU` Header (732.0
/// §4.1.4.2.2.1).
const FHP_BITS: u32 = 15 + 1;
/// Width of the First Header Pointer of Issue 4: 11 bits after a 5-bit spare (732.0-B-4
/// §4.1.4.2.1.4; AOS-003).
const FHP_ISSUE4_BITS: u32 = 11;
/// Width of the Bitstream Data Pointer: bits 2–15 of the `B_PDU` Header (732.0
/// §4.1.4.3.3.1).
const BDP_BITS: u32 = 15 - 2 + 1;
/// The Primary Header, 6 octets, 8 with the FHEC (732.0 §4.1.1.1 a).
const HEADER_OCTETS: usize = 6;
/// The Frame Header Error Control, 2 octets (732.0 §4.1.2.6).
const FHEC_OCTETS: usize = 2;
/// The Frame Error Control Field, 2 octets (732.0 §4.1.1.1 e).
const FECF_OCTETS: usize = 2;
/// Fewest data field octets: the field is mandatory (732.0 §4.1.1.1 c).
const DATA_MIN: usize = 1;
/// The VCID of OID frames, all ones of the 6-bit field: bits 10–15 (732.0 §4.1.2.3.1,
/// §4.1.4.1.5.1).
const OID: u8 = (1 << (15 - 10 + 1)) - 1;

/// All ones of a field of `bits` bits.
const fn ones(bits: u32) -> u16 {
    u16::MAX >> (16 - bits)
}

/// An error of `kind` for `fld`, with the bound and the value (GEN-041).
const fn mismatch(kind: ErrorKind, fld: FieldId, expected: u64, found: u64) -> Error {
    Error::new(kind)
        .with_field(fld)
        .with_detail(ErrorDetail::Expected { expected, found })
}

/// A table of one virtual channel 5 of spacecraft 0x155 carrying `content` on `pc`.
fn table(pc: AosPhysicalChannelConfig, content: DataFieldContent) -> VcTable<1> {
    let vc = AosVirtualChannelConfig::new(
        SpacecraftId::new(0x155 & pc.edition().max_scid()).unwrap(),
        VcId::new(5).unwrap(),
        content,
    )
    .unwrap();
    VcTable::new(pc, &[vc]).unwrap()
}

/// Encodes a frame of the only VC of `t` with `count`, `cycle`, and `pdu`, a zero zone, and
/// decodes it strictly: the decoded count and cycle.
fn round_trip(
    t: &VcTable<1>,
    count: VcFrameCount,
    cycle: Option<FrameCountCycle>,
) -> (VcFrameCount, Option<FrameCountCycle>) {
    let pc = t.physical_channel();
    let vc = *t.vcs().next().unwrap();
    let layout = t.frame_layout(vc.scid(), vc.vcid()).unwrap();
    let zone = vec![0_u8; layout.data().len()];
    let iz = vec![0_u8; pc.insert_zone_len()];
    let fields = AosFrameFields {
        scid: vc.scid(),
        vcid: vc.vcid(),
        vc_frame_count: count,
        replay_flag: true,
        cycle,
        pdu: PduHeader::None,
    };
    let content = AosFrameContent {
        insert_zone: &iz,
        security_header: &[],
        zone: &zone,
        security_trailer: &[],
        ocf: None,
    };
    let mut out = vec![0_u8; pc.frame_len()];
    let n = encode(&pc, t, &fields, &content, &mut out).unwrap();
    let f = AosFrame::decode(&out[..n], &pc, t).unwrap();
    (f.vc_frame_count(), f.vc_frame_count_cycle())
}

/// A `VCA_SDU` channel of 32-octet frames with the FHEC and FECF.
fn vca_table() -> VcTable<1> {
    let pc = AosPhysicalChannelConfig::new(32, true, 0, true, AosEdition::Issue5).unwrap();
    table(pc, DataFieldContent::VcaSdu)
}

/// The Virtual Channel Frame Count is 24 bits: `new` and `TryFrom` accept exactly
/// `0..=2^24 − 1`, reject the rest with `OutOfRange` (`vcf_count`), and the edges travel
/// through a frame (732.0 §4.1.2.4). The values between are drawn by
/// [`vc_frame_count_round_trips`] and [`vc_frame_count_rejects_above_24_bits`].
#[test]
fn vc_frame_count_is_24_bits() {
    // restricted: VcFrameCount
    // req: AOS-019, API-001, GEN-041, VER-023
    let max = (1_u32 << VCF_COUNT_BITS) - 1;
    assert_eq!(VcFrameCount::MAX, max);
    let t = vca_table();
    for v in [0, 1, max - 1, max, max + 1, max + 2, u32::MAX - 1, u32::MAX] {
        match VcFrameCount::try_from(v) {
            Ok(count) => {
                assert!(v <= max);
                assert_eq!(VcFrameCount::new(v), Some(count));
                assert_eq!((count.get(), u32::from(count)), (v, v));
                assert_eq!(round_trip(&t, count, None).0, count);
            }
            Err(e) => {
                assert!(v > max);
                assert_eq!(VcFrameCount::new(v), None);
                assert_eq!(
                    e,
                    mismatch(
                        ErrorKind::OutOfRange,
                        field::VCF_COUNT,
                        u64::from(max),
                        u64::from(v)
                    )
                );
            }
        }
    }
}

/// The Virtual Channel Frame Count Cycle is 4 bits: `new` and `TryFrom` accept exactly
/// `0..=15`, reject the rest with `OutOfRange` (`vc_frame_count_cycle`), and every value
/// travels through a frame with the Use Flag set (732.0 §4.1.2.5.5).
#[test]
fn frame_count_cycle_is_four_bits() {
    // restricted: FrameCountCycle
    // req: AOS-012, AOS-019, API-001, GEN-041, VER-023
    assert_eq!(u32::from(FrameCountCycle::MAX), (1 << CYCLE_BITS) - 1);
    let t = vca_table();
    for v in 0..=u8::MAX {
        let in_range = u32::from(v) < 1 << CYCLE_BITS;
        assert_eq!(FrameCountCycle::new(v).is_some(), in_range, "{v}");
        match FrameCountCycle::try_from(v) {
            Ok(c) => {
                assert_eq!((c.get(), u8::from(c)), (v, v));
                let count = VcFrameCount::new(0x12_3456).unwrap();
                assert_eq!(round_trip(&t, count, Some(c)), (count, Some(c)));
            }
            Err(e) => assert_eq!(
                e,
                mismatch(
                    ErrorKind::OutOfRange,
                    field::VC_FRAME_COUNT_CYCLE,
                    u64::from(FrameCountCycle::MAX),
                    u64::from(v)
                )
            ),
        }
    }
}

/// An `M_PDU` field of `zone_len` zone octets, the header first.
fn mpdu_field(zone_len: usize) -> Vec<u8> {
    vec![0x5A_u8; 2 + zone_len]
}

/// A packet offset is any 16-bit pointer value of Issue 5 below its two reserved ones: `new`
/// and `TryFrom` accept exactly `0..=0xFFFD`, reject the rest with `OutOfRange`
/// (`first_header_pointer`), and every offset travels through the `M_PDU` Header of an `M_PDU`
/// whose Packet Zone holds it, in Issue 5, and in Issue 4 up to its 11-bit pointer's largest
/// offset (732.0 §4.1.4.2.2.3–5; 732.0-B-4 §4.1.4.2.1.4).
#[test]
fn packet_offset_is_below_the_reserved_pointers() {
    // restricted: PacketOffset
    // req: AOS-003, AOS-017, AOS-019, API-001, GEN-041, VER-023
    let max = ones(FHP_BITS) - 2;
    assert_eq!(PacketOffset::MAX, max);
    assert_eq!(PacketPointer::max_offset(AosEdition::Issue5), max);
    assert_eq!(
        PacketPointer::max_offset(AosEdition::Issue4Compatible),
        ones(FHP_ISSUE4_BITS) - 2
    );
    // A Packet Zone one octet past the largest offset.
    let mut field = mpdu_field(usize::from(max) + 1);
    for v in 0..=u16::MAX {
        let in_range = v <= max;
        assert_eq!(PacketOffset::new(v).is_some(), in_range, "{v}");
        match PacketOffset::try_from(v) {
            Ok(offset) => {
                assert_eq!((offset.get(), u16::from(offset)), (v, v));
                let p = PacketPointer::StartsAt(offset);
                for edition in [AosEdition::Issue5, AosEdition::Issue4Compatible] {
                    if v > PacketPointer::max_offset(edition) {
                        // No raw value in this edition: to_raw refuses it, nothing saturates.
                        assert_eq!(p.to_raw(edition), None);
                        assert_eq!(p.header_octets(edition), None);
                        continue;
                    }
                    field[..2].copy_from_slice(&p.header_octets(edition).unwrap());
                    let m = Mpdu::decode(&field, edition).unwrap();
                    assert_eq!(m.pointer(), p);
                }
            }
            Err(e) => assert_eq!(
                e,
                mismatch(
                    ErrorKind::OutOfRange,
                    field::FIRST_HEADER_POINTER,
                    u64::from(max),
                    u64::from(v)
                )
            ),
        }
    }
}

/// What `PacketPointer::classify` gives for `raw` in a zone of `zone_len` octets, from 732.0
/// §4.1.4.2.2.3–5: all ones is "no Packet starts", all ones minus one "only Idle Data", an
/// offset inside the zone and the `width`-bit pointer a Packet start, and any other value, one
/// wider than the pointer included, `Malformed` (`first_header_pointer`) with the zone's last
/// offset.
fn expected_packet_pointer(raw: u16, zone_len: usize, width: u32) -> Result<PacketPointer, Error> {
    let all_ones = ones(width);
    let max = all_ones - 2;
    if raw == all_ones {
        Ok(PacketPointer::NoPacketStart)
    } else if raw == all_ones - 1 {
        Ok(PacketPointer::OnlyIdleData)
    } else if raw <= max && usize::from(raw) < zone_len {
        Ok(PacketPointer::StartsAt(PacketOffset::new(raw).unwrap()))
    } else {
        Err(mismatch(
            ErrorKind::Malformed,
            field::FIRST_HEADER_POINTER,
            u64::try_from(zone_len - 1).unwrap(),
            u64::from(raw),
        ))
    }
}

/// Every 16-bit raw value, in Packet Zones at and around the pointer's edges, is classified
/// in each edition as 732.0 §4.1.4.2.2.3–5 (732.0-B-4 §4.1.4.2.3) says: by `classify` for
/// every value, those wider than the Issue 4 pointer rejected, and by `Mpdu::decode` on the
/// wire for every value of the edition's width; an accepted pointer writes back the same raw
/// value (AOS-003, AOS-017).
#[test]
fn packet_pointer_classifies_every_raw_value() {
    // restricted: PacketPointer
    // req: AOS-003, AOS-017, API-032, GEN-041, VER-023
    let zones = [
        1_usize, 2, 0x7FC, 0x7FD, 0x7FE, 0x7FF, 0x800, 0xFFFC, 0xFFFD, 0xFFFE, 0x1_0000,
    ];
    let mut field = mpdu_field(0x1_0000);
    for (edition, width) in [
        (AosEdition::Issue5, FHP_BITS),
        (AosEdition::Issue4Compatible, FHP_ISSUE4_BITS),
    ] {
        assert_eq!(PacketPointer::no_packet_start_raw(edition), ones(width));
        assert_eq!(PacketPointer::only_idle_data_raw(edition), ones(width) - 1);
        for zone_len in zones {
            let unit = &mut field[..2 + zone_len];
            for raw in 0..=u16::MAX {
                let want = expected_packet_pointer(raw, zone_len, width);
                let got = PacketPointer::classify(raw, zone_len, edition);
                assert_eq!(got, want, "{edition:?} raw {raw:#x} zone {zone_len}");
                if raw <= ones(width) {
                    unit[..2].copy_from_slice(&raw.to_be_bytes());
                    let decoded = Mpdu::decode(unit, edition).map(|m| m.pointer());
                    assert_eq!(decoded, want);
                }
                if let Ok(p) = got {
                    assert_eq!(p.to_raw(edition), Some(raw));
                    assert_eq!(p.header_octets(edition), Some(raw.to_be_bytes()));
                }
            }
        }
    }
}

/// A count of valid bits is the Bitstream Data Pointer plus one, the pointer any 14-bit value
/// below the two reserved ones: `new` and `TryFrom` accept exactly `1..=0x3FFE`, reject the
/// rest with `OutOfRange` (`bitstream_data_pointer`) and the bound crossed, and every count
/// travels through the `B_PDU` Header of a `B_PDU` whose zone holds it (732.0
/// §4.1.4.3.3.1–4).
#[test]
fn valid_bit_count_is_below_the_reserved_pointers() {
    // restricted: ValidBitCount
    // req: AOS-017, AOS-019, API-001, GEN-041, VER-023
    let (min, max) = (1, ones(BDP_BITS) - 1);
    assert_eq!((ValidBitCount::MIN, ValidBitCount::MAX), (min, max));
    // A zone of 2 048 octets, whose last bit index lies above every count's.
    let mut unit = vec![0xC3_u8; 2 + 2048];
    for v in 0..=u16::MAX {
        let in_range = (min..=max).contains(&v);
        assert_eq!(ValidBitCount::new(v).is_some(), in_range, "{v}");
        match ValidBitCount::try_from(v) {
            Ok(count) => {
                assert_eq!((count.get(), u16::from(count)), (v, v));
                assert_eq!(count.last_bit(), v - 1);
                let p = BitstreamPointer::ValidBits(count);
                unit[..2].copy_from_slice(&p.header_octets());
                let b = Bpdu::decode(&unit).unwrap();
                assert_eq!(b.pointer(), p);
            }
            Err(e) => {
                let bound = if v < min { min } else { max };
                assert_eq!(
                    e,
                    mismatch(
                        ErrorKind::OutOfRange,
                        field::BITSTREAM_DATA_POINTER,
                        u64::from(bound),
                        u64::from(v)
                    )
                );
            }
        }
    }
}

/// What `BitstreamPointer::classify` gives for `raw` in a zone of `zone_len` octets, from
/// 732.0 §4.1.4.3.3.2–4: all ones "all valid", all ones minus one "only idle", the index of a
/// bit before the zone's last one the count of valid bits up to it; the last bit's index is
/// `InvalidField`, since a zone without idle data has the pointer all ones (§4.1.4.3.3.3); any
/// other value, beyond the zone or wider than the 14-bit field, is `Malformed` with the zone's
/// last bit index.
fn expected_bitstream_pointer(raw: u16, zone_len: usize) -> Result<BitstreamPointer, Error> {
    let last_bit = zone_len * 8 - 1;
    let all_ones = ones(BDP_BITS);
    if raw == all_ones {
        Ok(BitstreamPointer::AllValid)
    } else if raw == all_ones - 1 {
        Ok(BitstreamPointer::OnlyIdle)
    } else if usize::from(raw) == last_bit {
        Err(mismatch(
            ErrorKind::InvalidField,
            field::BITSTREAM_DATA_POINTER,
            u64::from(all_ones),
            u64::from(raw),
        ))
    } else if usize::from(raw) < last_bit && raw < all_ones - 1 {
        Ok(BitstreamPointer::ValidBits(
            ValidBitCount::new(raw + 1).unwrap(),
        ))
    } else {
        Err(mismatch(
            ErrorKind::Malformed,
            field::BITSTREAM_DATA_POINTER,
            u64::try_from(last_bit).unwrap(),
            u64::from(raw),
        ))
    }
}

/// Every 16-bit raw value, in zones at and around the 14-bit pointer's edges, is classified as
/// 732.0 §4.1.4.3.3 says; the 14-bit values also come so from `Bpdu::decode` on the wire, and
/// an accepted pointer writes back the same raw value (AOS-017, AOS-018).
#[test]
fn bitstream_pointer_classifies_every_raw_value() {
    // restricted: BitstreamPointer
    // req: AOS-017, AOS-018, API-032, GEN-041, VER-023
    assert_eq!(
        (
            BitstreamPointer::MAX_RAW,
            BitstreamPointer::ALL_VALID_RAW,
            BitstreamPointer::ONLY_IDLE_RAW
        ),
        (ones(BDP_BITS), ones(BDP_BITS), ones(BDP_BITS) - 1)
    );
    let zones = [1_usize, 2, 1023, 2046, 2047, 2048, 4096];
    let mut field = vec![0_u8; 2 + 4096];
    for zone_len in zones {
        let unit = &mut field[..2 + zone_len];
        for raw in 0..=u16::MAX {
            let want = expected_bitstream_pointer(raw, zone_len);
            let got = BitstreamPointer::classify(raw, zone_len);
            assert_eq!(got, want, "raw {raw:#x} zone {zone_len}");
            if raw <= ones(BDP_BITS) {
                unit[..2].copy_from_slice(&raw.to_be_bytes());
                assert_eq!(Bpdu::decode(unit).map(|b| b.pointer()), want);
            }
            if let Ok(p) = got {
                assert_eq!(p.to_raw(), raw);
            }
        }
    }
}

/// The shortest frame of a physical channel holds the Primary Header (8 octets with the FHEC),
/// the Insert Zone, one data octet, and the FECF if present; 732.0 sets no upper bound (732.0
/// §4.1.1.1, §4.1.1.2). `new` accepts exactly the lengths from that minimum up, in both
/// editions, rejects the shorter ones with `InvalidConfig` (`frame_len`) and the minimum, and a
/// frame of every accepted length up to 64 octets encodes and decodes (AOS-001, AOS-015).
#[test]
fn physical_channel_frame_length_has_its_minimum() {
    // restricted: AosPhysicalChannelConfig
    // req: AOS-001, AOS-015, API-040, GEN-041, VER-023
    for edition in [AosEdition::Issue5, AosEdition::Issue4Compatible] {
        for fhec in [false, true] {
            for fecf in [false, true] {
                for iz in (0..=12).chain([usize::MAX]) {
                    let fixed = HEADER_OCTETS
                        + if fhec { FHEC_OCTETS } else { 0 }
                        + DATA_MIN
                        + if fecf { FECF_OCTETS } else { 0 };
                    // Exact, so that an Insert Zone of `usize::MAX` octets fits no frame.
                    let need = u128::try_from(fixed).unwrap() + u128::try_from(iz).unwrap();
                    let min = fixed.saturating_add(iz);
                    for len in (0..=64).chain([usize::MAX - 1, usize::MAX]) {
                        let fits = u128::try_from(len).unwrap() >= need;
                        match AosPhysicalChannelConfig::new(len, fhec, iz, fecf, edition) {
                            Ok(pc) => {
                                assert!(fits, "{len} {iz}");
                                assert_eq!(
                                    (
                                        pc.frame_len(),
                                        pc.fhec(),
                                        pc.insert_zone_len(),
                                        pc.fecf(),
                                        pc.edition()
                                    ),
                                    (len, fhec, iz, fecf, edition)
                                );
                                if len <= 64 {
                                    let t = table(pc, DataFieldContent::VcaSdu);
                                    let count = VcFrameCount::new(9).unwrap();
                                    assert_eq!(round_trip(&t, count, None).0, count);
                                }
                            }
                            Err(e) => {
                                assert!(!fits, "{len} {iz}");
                                assert_eq!(
                                    e,
                                    mismatch(
                                        ErrorKind::InvalidConfig,
                                        field::FRAME_LEN,
                                        u64::try_from(min).unwrap(),
                                        u64::try_from(len).unwrap()
                                    )
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

/// A data virtual channel has a VCID 0–62; 63 is reserved for OID frames (732.0 Tables 5-2 and
/// 5-3, §4.1.4.1.5.1). `new` accepts every other VCID with every content, rejects 63 with
/// `InvalidConfig` (`vc_id`) and the largest data VCID, and a frame of every accepted VC
/// encodes and decodes on its VCID (AOS-016, API-040).
#[test]
fn virtual_channel_vcid_is_not_the_oid_vcid() {
    // restricted: AosVirtualChannelConfig
    // req: AOS-016, API-040, GEN-041, VER-023
    assert_eq!(OID_VCID.get(), OID);
    let pc = AosPhysicalChannelConfig::new(40, false, 2, true, AosEdition::Issue5).unwrap();
    let scid = SpacecraftId::new(0x3FF).unwrap();
    let contents = [
        DataFieldContent::Mpdu,
        DataFieldContent::Bpdu,
        DataFieldContent::VcaSdu,
        DataFieldContent::IdleData,
    ];
    for v in 0..=VcId::<6>::MAX {
        let vcid = VcId::new(v).unwrap();
        for c in contents {
            match AosVirtualChannelConfig::new(scid, vcid, c) {
                Ok(vc) => {
                    assert_ne!(v, OID);
                    assert_eq!((vc.scid(), vc.vcid(), vc.content()), (scid, vcid, c));
                    let t = VcTable::<1>::new(pc, &[vc]).unwrap();
                    let layout = t.frame_layout(scid, vcid).unwrap();
                    let (zone, pdu) = match c {
                        DataFieldContent::Mpdu => (
                            layout.data().len() - 2,
                            PduHeader::Mpdu(PacketPointer::NoPacketStart),
                        ),
                        DataFieldContent::Bpdu => (
                            layout.data().len() - 2,
                            PduHeader::Bpdu(BitstreamPointer::AllValid),
                        ),
                        DataFieldContent::VcaSdu | DataFieldContent::IdleData => {
                            (layout.data().len(), PduHeader::None)
                        }
                    };
                    let zone = vec![0_u8; zone];
                    let fields = AosFrameFields {
                        scid,
                        vcid,
                        vc_frame_count: VcFrameCount::new(1).unwrap(),
                        replay_flag: false,
                        cycle: None,
                        pdu,
                    };
                    let content = AosFrameContent {
                        insert_zone: &[0, 0],
                        security_header: &[],
                        zone: &zone,
                        security_trailer: &[],
                        ocf: None,
                    };
                    let mut out = [0_u8; 40];
                    let n = encode(&pc, &t, &fields, &content, &mut out).unwrap();
                    let f = AosFrame::decode(&out[..n], &pc, &t).unwrap();
                    assert_eq!((f.vcid(), f.is_oid()), (vcid, false));
                }
                Err(e) => {
                    assert_eq!(v, OID);
                    assert_eq!(
                        e,
                        mismatch(
                            ErrorKind::InvalidConfig,
                            field::VC_ID,
                            u64::from(OID - 1),
                            u64::from(OID)
                        )
                    );
                }
            }
        }
    }
}

/// The frame `f` re-encoded with other count and cycle fields, then decoded strictly.
fn re_encoded(
    f: &EncodedAosFrame,
    count: VcFrameCount,
    cycle: Option<FrameCountCycle>,
) -> (VcFrameCount, Option<FrameCountCycle>) {
    let pc = f.pc();
    let fields = AosFrameFields {
        vc_frame_count: count,
        cycle,
        ..f.fields
    };
    let mut out = vec![0_u8; pc.frame_len()];
    let n = encode(&pc, &f.table, &fields, &f.content(), &mut out).unwrap();
    let d = AosFrame::decode(&out[..n], &pc, &f.table).unwrap();
    (d.vc_frame_count(), d.vc_frame_count_cycle())
}

proptest! {
    #![proptest_config(strategies::config(128, REGRESSIONS))]

    /// Any count and cycle of the shared strategies travel through any frame of the shared
    /// strategy (732.0 §4.1.2.4, §4.1.2.5.5).
    #[test]
    fn vc_frame_count_round_trips(f in aos_frame(), count in vc_frame_count(), c in cycle()) {
        // restricted: VcFrameCount, FrameCountCycle, AosPhysicalChannelConfig, AosVirtualChannelConfig
        // req: AOS-012, AOS-019, VER-023, VER-054
        prop_assert_eq!(re_encoded(&f, count, c), (count, c));
    }

    /// Every 32-bit value above 24 bits is rejected with `OutOfRange` (`vcf_count`) (732.0
    /// §4.1.2.4.2).
    #[test]
    fn vc_frame_count_rejects_above_24_bits(v in (VcFrameCount::MAX + 1)..=u32::MAX) {
        // restricted: VcFrameCount
        // req: API-001, GEN-041, VER-023
        prop_assert_eq!(VcFrameCount::new(v), None);
        prop_assert_eq!(
            VcFrameCount::try_from(v),
            Err(mismatch(ErrorKind::OutOfRange, field::VCF_COUNT, u64::from(VcFrameCount::MAX), u64::from(v)))
        );
    }

    /// Any physical channel of the shared strategy carries a frame of a virtual channel of any
    /// content, any SCID it can hold (732.0 Tables 5-1, 5-3).
    #[test]
    fn configurations_carry_frames(
        pc in physical_channel(),
        scid in spacecraft_id::<10>(),
        c in content(),
        vcid in 0_u8..OID,
    ) {
        // restricted: AosPhysicalChannelConfig, AosVirtualChannelConfig
        // req: AOS-001, AOS-015, API-040, VER-023, VER-054
        let vc = AosVirtualChannelConfig::new(scid, VcId::new(vcid).unwrap(), c).unwrap();
        // A table refuses an SCID too wide for Issue 4, or a VC with no room for its data.
        prop_assume!(VcTable::<1>::new(pc, &[vc]).is_ok());
        let t = VcTable::<1>::new(pc, &[vc]).unwrap();
        let layout = t.frame_layout(scid, vc.vcid()).unwrap();
        let header = if matches!(c, DataFieldContent::Mpdu | DataFieldContent::Bpdu) { 2 } else { 0 };
        prop_assume!(layout.data().len() > header);
        let pdu = match c {
            DataFieldContent::Mpdu => PduHeader::Mpdu(PacketPointer::OnlyIdleData),
            DataFieldContent::Bpdu => PduHeader::Bpdu(BitstreamPointer::OnlyIdle),
            DataFieldContent::VcaSdu | DataFieldContent::IdleData => PduHeader::None,
        };
        let zone = vec![0_u8; layout.data().len() - header];
        let iz = vec![0_u8; pc.insert_zone_len()];
        let fields = AosFrameFields {
            scid,
            vcid: vc.vcid(),
            vc_frame_count: VcFrameCount::new(3).unwrap(),
            replay_flag: false,
            cycle: None,
            pdu,
        };
        let content = AosFrameContent { insert_zone: &iz, security_header: &[], zone: &zone, security_trailer: &[], ocf: None };
        let mut out = vec![0_u8; pc.frame_len()];
        let n = encode(&pc, &t, &fields, &content, &mut out).unwrap();
        let f = AosFrame::decode(&out[..n], &pc, &t).unwrap();
        prop_assert_eq!((f.len(), f.scid(), f.vcid()), (pc.frame_len(), scid, vc.vcid()));
    }
}

/// `VcTable::new` accepts exactly the virtual channel sets 732.0 §5 allows within its capacity
/// `N`, and rejects each other with its documented error (GEN-011, API-040, AOS-018): a VC given
/// twice (`InvalidConfig`, `vc_id`); an SCID above the 8 bits of `Issue4Compatible`
/// (`spacecraft_id`, with that limit; AOS-002); a data field with no octet after the `M_PDU`
/// Header (`frame_data_field`, with the shortest; CMN-041); for a `B_PDU` VC, an `Issue5` frame
/// longer than the 14-bit Bitstream Data Pointer counts, 2 048 octets (`frame_len`), and a
/// Bitstream Data Zone longer than 2 047 octets (`frame_data_field`), each with its limit
/// (AOS-018); more than `N` VCs (`CapacityExceeded`, `vc_id`). An accepted table carries a frame
/// of each VC through `encode` and `decode`.
#[test]
fn vc_tables_accept_exactly_the_configurable_channels() {
    // restricted: VcTable
    // req: AOS-002, AOS-018, API-040, GEN-011, GEN-060, CMN-041, VER-023
    type Table = VcTable<2>;
    let vc = |scid: u16, vcid: u8, content| {
        let scid = SpacecraftId::new(scid).unwrap();
        AosVirtualChannelConfig::new(scid, VcId::new(vcid).unwrap(), content).unwrap()
    };
    let pc = |len, fhec, fecf, edition| {
        AosPhysicalChannelConfig::new(len, fhec, 0, fecf, edition).unwrap()
    };
    let failure = |pc, vcs: &[AosVirtualChannelConfig]| Table::new(pc, vcs).unwrap_err();
    let invalid = |fld, expected: usize, found: usize| {
        mismatch(
            ErrorKind::InvalidConfig,
            fld,
            u64::try_from(expected).unwrap(),
            u64::try_from(found).unwrap(),
        )
    };
    let five = AosEdition::Issue5;
    let four = AosEdition::Issue4Compatible;

    // Accepted, at the capacity: a frame of each VC round-trips.
    let wide = pc(1000, true, true, five);
    let vcs = [
        vc(0x2A5, 1, DataFieldContent::Mpdu),
        vc(0x2A5, 2, DataFieldContent::Bpdu),
    ];
    let t = Table::new(wide, &vcs).unwrap();
    assert_eq!(t.vcs().copied().collect::<Vec<_>>(), vcs);
    for (v, pdu) in [
        (1, PduHeader::Mpdu(PacketPointer::NoPacketStart)),
        (2, PduHeader::Bpdu(BitstreamPointer::AllValid)),
    ] {
        let layout = t
            .frame_layout(vcs[0].scid(), VcId::new(v).unwrap())
            .unwrap();
        let zone = vec![0xA5_u8; layout.data().len() - 2];
        let fields = AosFrameFields {
            scid: vcs[0].scid(),
            vcid: VcId::new(v).unwrap(),
            vc_frame_count: VcFrameCount::new(7).unwrap(),
            replay_flag: false,
            cycle: None,
            pdu,
        };
        let content = AosFrameContent {
            insert_zone: &[],
            security_header: &[],
            zone: &zone,
            security_trailer: &[],
            ocf: None,
        };
        let mut out = vec![0_u8; wide.frame_len()];
        let n = encode(&wide, &t, &fields, &content, &mut out).unwrap();
        let f = AosFrame::decode(&out[..n], &wide, &t).unwrap();
        assert_eq!((f.vcid().get(), f.data_unit().zone()), (v, zone.as_slice()));
    }

    // Duplicates, and capacity (GEN-011).
    let twice = [vcs[0], vcs[0]];
    let e = failure(wide, &twice);
    assert_eq!(
        (e.kind(), e.field()),
        (ErrorKind::InvalidConfig, Some(field::VC_ID))
    );
    let three = [vcs[0], vcs[1], vc(0x2A5, 3, DataFieldContent::VcaSdu)];
    let e = failure(wide, &three);
    assert_eq!(
        (e.kind(), e.field()),
        (ErrorKind::CapacityExceeded, Some(field::VC_ID))
    );

    // Issue 4 has 8 bits of SCID (AOS-002).
    let narrow = pc(64, false, true, four);
    let scid_max = usize::from(ones(8));
    assert!(Table::new(narrow, &[vc(0xFF, 1, DataFieldContent::Mpdu)]).is_ok());
    assert_eq!(
        failure(narrow, &[vc(0x100, 1, DataFieldContent::Mpdu)]),
        invalid(field::SPACECRAFT_ID, scid_max, 0x100)
    );

    // An M_PDU needs an octet after its 2-octet header: header 6, data 2, FECF 2 leave none.
    let tiny = pc(HEADER_OCTETS + 2 + FECF_OCTETS, false, true, five);
    assert!(Table::new(tiny, &[vc(1, 1, DataFieldContent::VcaSdu)]).is_ok());
    assert_eq!(
        failure(tiny, &[vc(1, 1, DataFieldContent::Mpdu)]),
        invalid(field::FRAME_DATA_FIELD, 3, 2)
    );

    // B_PDU limits (AOS-018): the 14-bit pointer counts bits of at most 2 048 octets, and the
    // longest zone it can name is 2 047 octets.
    let max_bitstream_frame = (1_usize << BDP_BITS) / 8;
    let max_zone = 2047;
    let bpdu = [vc(1, 1, DataFieldContent::Bpdu)];
    assert!(Table::new(pc(max_bitstream_frame, true, true, five), &bpdu).is_ok());
    assert_eq!(
        failure(pc(max_bitstream_frame + 1, true, true, five), &bpdu),
        invalid(
            field::FRAME_LEN,
            max_bitstream_frame,
            max_bitstream_frame + 1
        )
    );
    // In Issue 4, no FHEC or FECF: the frame is the header, the B_PDU Header, and the zone.
    let len = |zone: usize| HEADER_OCTETS + 2 + zone;
    assert!(Table::new(pc(len(max_zone), false, false, four), &bpdu).is_ok());
    assert_eq!(
        failure(pc(len(max_zone + 1), false, false, four), &bpdu),
        invalid(field::FRAME_DATA_FIELD, max_zone, max_zone + 1)
    );
}

proptest! {
    #![proptest_config(strategies::config(64, REGRESSIONS))]

    /// Every table of the shared strategies is rebuilt from its own virtual channels, and
    /// decodes the frames encoded on it (API-040, VER-054).
    #[test]
    fn vc_tables_from_the_strategies_rebuild(f in aos_frame()) {
        // req: API-040, VER-023, VER-054
        let vcs: Vec<AosVirtualChannelConfig> = f.table.vcs().copied().collect();
        let again = aldis_testkit::strategies::aos::AosTable::new(f.pc(), &vcs).unwrap();
        prop_assert_eq!(&again, &f.table);
        prop_assert!(AosFrame::decode(&f.octets, &f.pc(), &again).is_ok());
    }
}

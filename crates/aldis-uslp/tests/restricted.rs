//! A test for every restricted type of `aldis-uslp` (RS-020, VER-023; M2 plan WP-7): each public
//! type with a checked constructor accepts exactly the range 732.1-B-3 gives it, rejects every
//! other value with the documented error, and carries every accepted value through its wire
//! field and back. The bounds are written from the clauses cited, as field widths, length
//! limits, and the SANA registry snapshot, and compared with the type's own constants. Inputs
//! come from the shared strategies of `aldis-testkit` (VER-054); each `// restricted:` marker
//! names the type a test covers, and `cargo xtask restricted` checks that every restricted type
//! has one.
#![expect(
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    reason = "test helpers on small bounded values fail loudly (RS-018)"
)]

use std::collections::BTreeSet;

use aldis_core::security::SecurityLayout;
use aldis_core::{DecodeMode, Error, ErrorDetail, ErrorKind, FieldId};
use aldis_testkit::strategies;
use aldis_testkit::strategies::proptest::prelude::*;
use aldis_testkit::strategies::uslp::{
    frame_kind, tfdz_offset, upid, uslp_frame, vcf_count, vcf_count_len,
};
use aldis_uslp::field;
use aldis_uslp::{
    ChannelFields, FirstHeaderPointer, FrameContent, FrameFields, FrameKind, LastValidOctet,
    MAX_FRAME_LEN, MapConfig, OcfUse, PRIMARY_HEADER_LEN, PhysicalChannelConfig, Reason,
    SecurityScope, SourceOrDestination, TfdfHeader, TfdzOffset, TfdzRule, TransferFrameType,
    TruncatedFrameLen, UnassignedUpid, Upid, UslpFrame, UslpMapId, UslpSpacecraftId, UslpVcId,
    VcConfig, VcLookup, VcParams, VcTable, VcfCount, VcfCountLen, encode, encode_truncated,
};

/// Failing cases are saved here and replayed first.
const REGRESSIONS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/proptest-regressions/restricted.txt"
);

/// The SANA registry `uslp_protocol_id`, as committed (M2 plan D13, OI-06); `cargo xtask sana`
/// checks that the generated `Upid` table matches it.
const UPID_REGISTRY: &str = include_str!("../../../specs/sana/uslp_protocol_id.tsv");

/// Width of the Frame Length field, which holds the length minus one: bits 32–47 (732.1
/// §4.1.2.7.1–2).
const FRAME_LENGTH_BITS: u32 = 47 - 32 + 1;
/// Longest frame: what the Frame Length field counts (732.1 §4.1.2.7 note 4).
const LONGEST: usize = 1 << FRAME_LENGTH_BITS;
/// The Primary Header without its VCF Count: bits 0–55, the count starting in bit 56 (732.1
/// §4.1.2.12.2).
const HEADER_OCTETS: usize = 56 / 8;
/// The shortest TFDF Header, 1 octet, and 3 with the pointer (732.1 §4.1.4.2.1.2).
const TFDF_SHORT: usize = 1;
/// The TFDF Header with the First Header or Last Valid Octet Pointer (732.1 §4.1.4.2.1.2).
const TFDF_LONG: usize = 3;
/// Fewest TFDZ octets (CMN-041).
const TFDZ_MIN: usize = 1;
/// The Frame Error Control Field, 2 octets (732.1 §4.1.6).
const FECF_OCTETS: usize = 2;
/// Width of the VCF Count Length field: bits 53–55 (732.1 §4.1.2.11.1–2).
const VCF_COUNT_LEN_BITS: u32 = 55 - 53 + 1;
/// Width of the pointer field, 16 bits; all ones means "none" (732.1 §4.1.4.2.4.2, .4, .6).
const POINTER_ONES: u16 = u16::MAX;
/// Width of the UPID: bits 3–7 of the TFDF Header (732.1 §4.1.4.2.3.1).
const UPID_BITS: u32 = 7 - 3 + 1;
/// The Truncated Transfer Frame Length, 6 to 32 octets (732.1 Table 5-3).
const TRUNCATED_RANGE: core::ops::RangeInclusive<usize> = 6..=32;
/// The truncated Primary Header, 4 octets (732.1 Annex D1.2.2 a).
const TRUNCATED_HEADER_OCTETS: usize = 4;
/// The VCID of OID frames, all ones (732.1 §4.1.4.1.6).
const OID: u8 = 63;

/// The validation reasons and their stable codes, as `docs/standards/uslp-frame.md` §3.1 lists
/// them (M2 plan D14; USLP-014).
const REASONS: [(u8, &str); 18] = [
    (1, "UnexpectedVersion"),
    (2, "UnexpectedSpacecraft"),
    (3, "ReservedFlags"),
    (4, "ReservedSpare"),
    (5, "InvalidVcid"),
    (6, "InvalidMapId"),
    (7, "TruncatedFrameNotAllowed"),
    (8, "VcfCountLengthMismatch"),
    (9, "OcfFlagMismatch"),
    (10, "FixedLengthMismatch"),
    (11, "FrameTooLong"),
    (12, "FrameTooShort"),
    (13, "LengthMismatch"),
    (14, "FecfMismatch"),
    (15, "ConstructionRuleMismatch"),
    (16, "PointerBeyondZone"),
    (17, "OidHeaderMismatch"),
    (18, "NonTruncatedFrameNotAllowed"),
];

/// `InvalidConfig` naming `fld`.
const fn invalid_config(fld: FieldId) -> Error {
    Error::new(ErrorKind::InvalidConfig).with_field(fld)
}

/// The Spacecraft ID of the tables here.
const fn scid() -> UslpSpacecraftId {
    UslpSpacecraftId::new(0xBEEF).unwrap()
}

/// VC 1 and MAP 0, the channel of the tables here.
const fn ids() -> (UslpVcId, UslpMapId) {
    (UslpVcId::new(1).unwrap(), UslpMapId::new(0).unwrap())
}

/// A table of one VC 1 with `params`, and its MAP 0, on `pc`.
fn table(pc: PhysicalChannelConfig, params: VcParams) -> Result<VcTable<1, 1>, Error> {
    let (vc_id, map_id) = ids();
    let vc = VcConfig::new(scid(), vc_id, params)?;
    let map = MapConfig::new(scid(), vc_id, map_id, SecurityLayout::NONE);
    VcTable::new(pc, &[vc], &[map])
}

/// A variable-length channel of frames of at most `max` octets without FECF, with VC 1 of
/// `params`.
fn variable(max: usize, params: VcParams) -> VcTable<1, 1> {
    let pc = PhysicalChannelConfig::new(TransferFrameType::Variable, max, 0, false).unwrap();
    table(pc, params).unwrap()
}

/// The fields of a frame of `kind` with `count` on VC 1, MAP 0.
const fn fields(kind: FrameKind, count: u64) -> FrameFields {
    let (vc_id, map_id) = ids();
    FrameFields {
        spacecraft_id: scid(),
        source_or_destination: SourceOrDestination::Destination,
        vc_id,
        map_id,
        kind,
        vcf_count: count,
    }
}

/// Encodes a frame of `f` with `tfdf` and the TFDZ `zone` on `t`, or the writer's error.
fn frame(
    t: &VcTable<1, 1>,
    f: FrameFields,
    tfdf: TfdfHeader,
    zone: &[u8],
) -> Result<Vec<u8>, Error> {
    let iz = vec![0_u8; aldis_uslp::VcLookup::physical_channel(t).insert_zone_len()];
    let content = FrameContent {
        tfdf,
        insert_zone: &iz,
        ocf: None,
    };
    let mut out = vec![0_u8; LONGEST];
    let n = encode(t, f, content, zone, &mut out)?;
    out.truncate(n);
    Ok(out)
}

/// A TFDF Header of a variable-length TFDZ carrying `upid`.
const fn short_tfdf(upid: Upid) -> TfdfHeader {
    TfdfHeader {
        rule: TfdzRule::NoSegmentation,
        upid,
    }
}

/// The Bypass/Sequence Control and Protocol Control Command Flags select sequence-controlled
/// (`00`), expedited (`10`), or protocol control (`11`) frames; `01` is reserved (732.1
/// §4.1.2.8, Table 4-1). `from_flags` accepts exactly the three combinations, each comes back
/// from a frame, and the reserved one on the wire is rejected with `ReservedValue`
/// (`protocol_control_command_flag`) in both modes (USLP-001).
#[test]
fn frame_kind_has_three_flag_combinations() {
    // restricted: FrameKind
    // req: USLP-001, VER-023
    let t = variable(64, VcParams::new(TransferFrameType::Variable));
    for bypass in [false, true] {
        for control in [false, true] {
            if let Some(kind) = FrameKind::from_flags(bypass, control) {
                assert!(bypass || !control);
                assert_eq!(
                    (kind.bypass_flag(), kind.protocol_control_flag()),
                    (bypass, control)
                );
                let octets =
                    frame(&t, fields(kind, 0), short_tfdf(Upid::Packets), &[1, 2]).unwrap();
                let d = UslpFrame::decode(&octets, &t, DecodeMode::Strict).unwrap();
                assert_eq!(d.full().unwrap().kind(), kind);
            } else {
                assert!(!bypass && control);
                let mut octets = frame(
                    &t,
                    fields(FrameKind::SequenceControlled, 0),
                    short_tfdf(Upid::Packets),
                    &[1, 2],
                )
                .unwrap();
                // Bit 49, the Protocol Control Command Flag (§4.1.2.8.2.1).
                octets[6] |= 0x40;
                for mode in [DecodeMode::Strict, DecodeMode::Lenient] {
                    let e = UslpFrame::decode(&octets, &t, mode).unwrap_err();
                    assert_eq!(e.kind(), ErrorKind::ReservedValue);
                    assert_eq!(e.field(), Some(field::PROTOCOL_CONTROL_COMMAND_FLAG));
                }
            }
        }
    }
}

/// The VCF Count Length is a 3-bit field of 0 to 7 octets, and the longest count of `n` octets
/// is `2^(8n) − 1`, none without a field (732.1 §4.1.2.11, Table 4-2). `new` accepts exactly
/// `0..=7`, rejects the rest with `InvalidConfig` (`vcf_count_len`), and a frame of a VC with
/// each length carries the longest count of that length (USLP-004).
#[test]
fn vcf_count_len_is_three_bits() {
    // restricted: VcfCountLen
    // req: USLP-004, API-040, VER-023
    for v in 0..=u8::MAX {
        let in_range = u32::from(v) < 1 << VCF_COUNT_LEN_BITS;
        match VcfCountLen::new(v) {
            Ok(len) => {
                assert!(in_range, "{v}");
                assert_eq!((len.get(), len.octets()), (v, usize::from(v)));
                let longest = u64::try_from((1_u128 << (8 * u32::from(v))) - 1).unwrap();
                assert_eq!(len.max_count(), longest);
                let params = VcParams {
                    vcf_count_len_sequence: len,
                    ..VcParams::new(TransferFrameType::Variable)
                };
                let t = variable(64, params);
                let f = fields(FrameKind::SequenceControlled, longest);
                let octets = frame(&t, f, short_tfdf(Upid::Packets), &[9]).unwrap();
                let d = UslpFrame::decode(&octets, &t, DecodeMode::Strict).unwrap();
                assert_eq!(
                    d.full().unwrap().vcf_count(),
                    VcfCount::new(len, longest).unwrap()
                );
            }
            Err(e) => {
                assert!(!in_range, "{v}");
                assert_eq!(e, invalid_config(field::VCF_COUNT_LEN));
            }
        }
    }
    assert_eq!(VcfCountLen::NONE.get(), 0);
}

/// A VCF Count of `n` octets holds `0..=2^(8n) − 1` (732.1 §4.1.2.12.3, Table 4-2): `new`
/// accepts exactly those values for each length, at the edges here and for any value in
/// [`vcf_count_fits_its_length`].
#[test]
fn vcf_count_edges_fit_their_length() {
    // restricted: VcfCount
    // req: USLP-004, VER-023
    for n in 0..=7_u8 {
        let len = VcfCountLen::new(n).unwrap();
        let longest = (1_u128 << (8 * u32::from(n))) - 1;
        for v in [
            0,
            1,
            longest.saturating_sub(1),
            longest,
            longest + 1,
            u128::from(u64::MAX),
        ] {
            let Ok(value) = u64::try_from(v) else {
                continue;
            };
            let count = VcfCount::new(len, value);
            assert_eq!(count.is_some(), v <= longest, "{n} {v}");
            if let Some(c) = count {
                assert_eq!((c.len(), c.value()), (len, value));
            }
        }
    }
}

/// The Transfer Frame Length is at most 65 536 octets, what the Frame Length field counts, and
/// at least the 7-octet Primary Header, a 1-octet TFDF Header, and one TFDZ octet; an Insert
/// Zone is allowed only on fixed-length channels (732.1 §4.1.2.7, §4.1.2.12.2, §4.1.4.2.1.2,
/// Table 5-1 note 5). `new` accepts exactly that, rejects a length outside with
/// `InvalidConfig` (`frame_len` fixed, `max_frame_len` variable) and an Insert Zone on a
/// variable-length channel with `InvalidConfig` (`insert_zone_len`); a frame of every accepted
/// length up to 48 octets, and of 65 536, encodes and decodes (USLP-003, USLP-008).
#[test]
fn physical_channel_frame_length_is_the_spec_range() {
    // restricted: PhysicalChannelConfig
    // req: USLP-003, USLP-008, API-040, VER-023
    assert_eq!(
        (MAX_FRAME_LEN, PRIMARY_HEADER_LEN),
        (LONGEST, HEADER_OCTETS)
    );
    let shortest = HEADER_OCTETS + TFDF_SHORT + TFDZ_MIN;
    let lengths: Vec<usize> = (0..=48)
        .chain(LONGEST - 2..=LONGEST + 2)
        .chain([usize::MAX])
        .collect();
    for frame_type in [TransferFrameType::Fixed, TransferFrameType::Variable] {
        let fixed = frame_type == TransferFrameType::Fixed;
        for fecf in [false, true] {
            for iz in [0_usize, 1, 5] {
                for &len in &lengths {
                    let len_ok = (shortest..=LONGEST).contains(&len);
                    let want = if !len_ok {
                        Err(invalid_config(if fixed {
                            field::FRAME_LEN
                        } else {
                            field::MAX_FRAME_LEN
                        }))
                    } else if iz > 0 && !fixed {
                        Err(invalid_config(field::INSERT_ZONE_LEN))
                    } else {
                        Ok((frame_type, len, iz, fecf))
                    };
                    let got = PhysicalChannelConfig::new(frame_type, len, iz, fecf);
                    assert_eq!(
                        got.map(|pc| (
                            pc.frame_type(),
                            pc.frame_len(),
                            pc.insert_zone_len(),
                            pc.fecf()
                        )),
                        want,
                        "{frame_type:?} {len} {iz} {fecf}"
                    );
                    if let Ok(pc) = got
                        && (len <= 48 || len == LONGEST)
                    {
                        physical_channel_carries_a_frame(pc);
                    }
                }
            }
        }
    }
}

/// A frame filling the frame length of `pc`, on a VC of the channel's type, encodes and decodes
/// at that length; a fixed-length channel too short for the 3-octet TFDF Header of its VCs has
/// no VC (CMN-041).
fn physical_channel_carries_a_frame(pc: PhysicalChannelConfig) {
    let fixed = pc.frame_type() == TransferFrameType::Fixed;
    let (tfdf, header_len) = if fixed {
        (
            TfdfHeader {
                rule: TfdzRule::PacketsSpanningFrames(FirstHeaderPointer::NoPacketStartOrEnd),
                upid: Upid::Packets,
            },
            TFDF_LONG,
        )
    } else {
        (short_tfdf(Upid::Packets), TFDF_SHORT)
    };
    let overhead =
        HEADER_OCTETS + pc.insert_zone_len() + header_len + if pc.fecf() { FECF_OCTETS } else { 0 };
    let t = table(pc, VcParams::new(pc.frame_type()));
    let Some(tfdz_len) = pc
        .frame_len()
        .checked_sub(overhead)
        .filter(|&n| n >= TFDZ_MIN)
    else {
        assert!(t.is_err(), "{pc:?}");
        return;
    };
    let t = t.unwrap();
    let octets = frame(
        &t,
        fields(FrameKind::Expedited, 0),
        tfdf,
        &vec![0x77; tfdz_len],
    )
    .unwrap();
    let d = UslpFrame::decode(&octets, &t, DecodeMode::Strict).unwrap();
    assert_eq!(d.len(), pc.frame_len());
}

/// The Truncated Transfer Frame Length is 6 to 32 octets (732.1 Table 5-3): `new` accepts
/// exactly that, rejects the rest with `InvalidConfig` (`truncated_frame_len`), and a
/// truncated frame of every accepted length encodes and decodes at that length (USLP-002).
#[test]
fn truncated_frame_len_is_six_to_thirty_two() {
    // restricted: TruncatedFrameLen
    // req: USLP-002, API-040, VER-023
    for v in (0..=300).chain([usize::MAX]) {
        match TruncatedFrameLen::new(v) {
            Ok(len) => {
                assert!(TRUNCATED_RANGE.contains(&v), "{v}");
                assert_eq!(len.get(), v);
                let params = VcParams {
                    truncated_frame_len: Some(len),
                    ..VcParams::new(TransferFrameType::Variable)
                };
                let t = variable(64, params);
                let (vc_id, map_id) = ids();
                let channel = ChannelFields {
                    spacecraft_id: scid(),
                    source_or_destination: SourceOrDestination::Source,
                    vc_id,
                    map_id,
                };
                let zone = vec![0xE1_u8; v - TRUNCATED_HEADER_OCTETS - TFDF_SHORT];
                let mut out = [0_u8; 64];
                let n =
                    encode_truncated(&t, channel, short_tfdf(Upid::OctetStream), &zone, &mut out)
                        .unwrap();
                let d = UslpFrame::decode(&out[..n], &t, DecodeMode::Strict).unwrap();
                assert_eq!((d.len(), d.is_truncated()), (v, true));
            }
            Err(e) => {
                assert!(!TRUNCATED_RANGE.contains(&v), "{v}");
                assert_eq!(e, invalid_config(field::TRUNCATED_FRAME_LEN));
            }
        }
    }
}

/// What `VcConfig::new` gives, from 732.1: "Inclusion of OCF Allowed" only on a
/// variable-length VC and "Required" only on a fixed-length one (Table 5-3); truncated frames
/// only on a variable-length VC (Annex D1.2.1); VCID 63 only for fixed-length OID frames
/// (§4.1.4.1.6–7, Table 5-1 note 4; USLP-016), with no MAP channels of its own (§4.1.4.1.8).
const fn expected_vc(
    fixed: bool,
    ocf: OcfUse,
    truncated: bool,
    vcid: u8,
    per_map: bool,
) -> Result<(), Error> {
    let ocf_ok = match ocf {
        OcfUse::Absent => true,
        OcfUse::Allowed => !fixed,
        OcfUse::Required => fixed,
    };
    if !ocf_ok {
        Err(invalid_config(field::OCF_FLAG))
    } else if fixed && truncated {
        Err(invalid_config(field::TRUNCATED_FRAME_LEN))
    } else if vcid == OID && !fixed {
        Err(invalid_config(field::TRANSFER_FRAME_TYPE))
    } else if vcid == OID && per_map {
        Err(invalid_config(field::VC_ID))
    } else {
        Ok(())
    }
}

/// Every combination of VC Transfer Frame Type, OCF use, truncated frames, SDLS scope, and a
/// VCID of a data VC or 63 is accepted or rejected as 732.1 says, with the documented field,
/// whatever the VCF Count Lengths (USLP-002, USLP-009, USLP-016).
#[test]
fn vc_config_accepts_what_732_1_allows() {
    // restricted: VcConfig
    // req: USLP-002, USLP-009, USLP-016, API-040, VER-023
    let len = VcfCountLen::new(2).unwrap();
    for frame_type in [TransferFrameType::Fixed, TransferFrameType::Variable] {
        for ocf in [OcfUse::Absent, OcfUse::Allowed, OcfUse::Required] {
            for truncated in [None, Some(TruncatedFrameLen::new(28).unwrap())] {
                for vcid in [0_u8, 1, 62, OID] {
                    for security in [
                        SecurityScope::Vc(SecurityLayout::NONE),
                        SecurityScope::PerMap,
                    ] {
                        let params = VcParams {
                            frame_type,
                            vcf_count_len_sequence: len,
                            vcf_count_len_expedited: VcfCountLen::NONE,
                            ocf,
                            truncated_frame_len: truncated,
                            security,
                        };
                        let got = VcConfig::new(scid(), UslpVcId::new(vcid).unwrap(), params);
                        let want = expected_vc(
                            frame_type == TransferFrameType::Fixed,
                            ocf,
                            truncated.is_some(),
                            vcid,
                            security == SecurityScope::PerMap,
                        );
                        assert_eq!(got.map(|vc| vc.params()), want.map(|()| params));
                    }
                }
            }
        }
    }
}

/// A TFDZ offset is any 16-bit pointer value but all ones, which means "none" (732.1
/// §4.1.4.2.4.2–6): `new` and `TryFrom` accept exactly `0..=0xFFFE` and reject `0xFFFF` with
/// `OutOfRange` (`fhp_lvo_pointer`). The pointer must lie inside the TFDZ (USLP-006), so only
/// offsets below the longest TFDZ, 65 526 octets in a 65 536-octet frame, have a wire form;
/// the edges travel through such a frame here, and any offset of the shared strategy that fits
/// in [`tfdz_offsets_round_trip`].
#[test]
fn tfdz_offset_is_below_all_ones() {
    // restricted: TfdzOffset
    // req: USLP-005, USLP-006, API-001, GEN-041, VER-023
    assert_eq!(TfdzOffset::MAX, POINTER_ONES - 1);
    for v in 0..=u16::MAX {
        match TfdzOffset::try_from(v) {
            Ok(offset) => {
                assert!(v < POINTER_ONES);
                assert_eq!(TfdzOffset::new(v), Some(offset));
                assert_eq!((offset.get(), u16::from(offset)), (v, v));
            }
            Err(e) => {
                assert_eq!(v, POINTER_ONES);
                assert_eq!(TfdzOffset::new(v), None);
                assert_eq!(
                    e,
                    Error::new(ErrorKind::OutOfRange)
                        .with_field(field::FHP_LVO_POINTER)
                        .with_detail(ErrorDetail::Expected {
                            expected: u64::from(TfdzOffset::MAX),
                            found: u64::from(v)
                        })
                );
            }
        }
    }
    let longest_tfdz = LONGEST - HEADER_OCTETS - TFDF_LONG;
    let t = variable(LONGEST, VcParams::new(TransferFrameType::Fixed));
    for offset in [0, 1, longest_tfdz - 2, longest_tfdz - 1] {
        let offset = TfdzOffset::new(u16::try_from(offset).unwrap()).unwrap();
        assert_offset_round_trips(&t, offset, longest_tfdz);
    }
}

/// `offset` as a First Header Pointer and as a Last Valid Octet Pointer comes back from a
/// frame with a TFDZ of `tfdz_len` octets on `t` (USLP-005, USLP-006).
fn assert_offset_round_trips(t: &VcTable<1, 1>, offset: TfdzOffset, tfdz_len: usize) {
    for rule in [
        TfdzRule::PacketsSpanningFrames(FirstHeaderPointer::PacketStartsAt(offset)),
        TfdzRule::SduStart(LastValidOctet::At(offset)),
        TfdzRule::SduContinuation(LastValidOctet::At(offset)),
    ] {
        let tfdf = TfdfHeader {
            rule,
            upid: Upid::Packets,
        };
        let octets = frame(
            t,
            fields(FrameKind::Expedited, 0),
            tfdf,
            &vec![0_u8; tfdz_len],
        )
        .unwrap();
        let d = UslpFrame::decode(&octets, t, DecodeMode::Strict).unwrap();
        assert_eq!(d.tfdf_header(), tfdf);
    }
}

/// The UPID values the SANA snapshot lists as assigned.
fn assigned_upids() -> BTreeSet<u8> {
    UPID_REGISTRY
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| {
            let mut cols = l.split('\t');
            let status = cols.next()?;
            let bits = cols.next()?.strip_prefix("0b")?;
            (status == "Assigned").then(|| u8::from_str_radix(bits, 2).unwrap())
        })
        .collect()
}

/// An unassigned UPID is a 5-bit value the SANA registry does not assign (732.1
/// §4.1.4.2.3.1–3; OI-06): `new` accepts exactly those values of `0..=31` that the committed
/// registry snapshot does not list as assigned, and rejects every other value (USLP-007).
#[test]
fn unassigned_upid_is_what_sana_leaves() {
    // restricted: UnassignedUpid
    // req: USLP-007, VER-023
    let assigned = assigned_upids();
    assert!(!assigned.is_empty());
    for v in 0..=u8::MAX {
        let want = u32::from(v) < 1 << UPID_BITS && !assigned.contains(&v);
        let got = UnassignedUpid::new(v);
        assert_eq!(got.is_some(), want, "{v}");
        if let Some(u) = got {
            assert_eq!(u.get(), v);
            assert_eq!(Upid::from_bits(v), Some(Upid::Other(u)));
        }
    }
}

/// The UPID is 5 bits: `from_bits` accepts exactly `0..=31`, gives a named variant for each
/// value the SANA snapshot assigns and `Other` for the rest, gives back its bits, and every
/// value travels through the TFDF Header of a frame (732.1 §4.1.4.2.3; USLP-007).
#[test]
fn upid_is_five_bits() {
    // restricted: Upid
    // req: USLP-007, VER-023
    let assigned = assigned_upids();
    let t = variable(64, VcParams::new(TransferFrameType::Variable));
    for v in 0..=u8::MAX {
        let in_range = u32::from(v) < 1 << UPID_BITS;
        match Upid::from_bits(v) {
            Some(upid) => {
                assert!(in_range, "{v}");
                assert_eq!(upid.bits(), v);
                assert_eq!(matches!(upid, Upid::Other(_)), !assigned.contains(&v));
                let tfdf = short_tfdf(upid);
                let octets = frame(&t, fields(FrameKind::Expedited, 0), tfdf, &[3, 4]).unwrap();
                let d = UslpFrame::decode(&octets, &t, DecodeMode::Strict).unwrap();
                assert_eq!(d.tfdf_header(), tfdf);
            }
            None => assert!(!in_range, "{v}"),
        }
    }
}

/// The validation reasons are exactly those of `docs/standards/uslp-frame.md` §3.1, each with
/// its stable code: `from_code` accepts exactly those codes, names the reason the table names,
/// and gives back its code; `ALL` lists them in code order (USLP-014, M2 plan D14).
#[test]
fn reason_codes_are_the_documented_ones() {
    // restricted: Reason
    // req: USLP-014, VER-023
    for code in 0..=u8::MAX {
        let documented = REASONS.iter().find(|(c, _)| *c == code);
        match (Reason::from_code(code), documented) {
            (Some(r), Some((_, name))) => {
                assert_eq!(format!("{r:?}"), *name);
                assert_eq!(r.code(), code);
            }
            (None, None) => {}
            (got, want) => panic!("code {code}: {got:?} against {want:?}"),
        }
    }
    let codes: Vec<u8> = Reason::ALL.iter().map(|r| r.code()).collect();
    let documented: Vec<u8> = REASONS.iter().map(|(c, _)| *c).collect();
    assert_eq!(codes, documented);
}

proptest! {
    #![proptest_config(strategies::config(64, REGRESSIONS))]

    /// Any VCF Count of the shared strategy travels through a frame of a VC of its length, for
    /// either quality of service (732.1 §4.1.2.11–12; USLP-004).
    #[test]
    fn vcf_count_fits_its_length(count in vcf_count(), kind in frame_kind(), raw in any::<u64>(), other in vcf_count_len()) {
        // restricted: VcfCount, VcfCountLen, FrameKind
        // req: USLP-001, USLP-004, VER-023, VER-054
        let bypass = kind.bypass_flag();
        let params = VcParams {
            vcf_count_len_sequence: if bypass { other } else { count.len() },
            vcf_count_len_expedited: if bypass { count.len() } else { other },
            ..VcParams::new(TransferFrameType::Variable)
        };
        let t = variable(64, params);
        let octets = frame(&t, fields(kind, count.value()), short_tfdf(Upid::Packets), &[5]).unwrap();
        let d = UslpFrame::decode(&octets, &t, DecodeMode::Strict).unwrap();
        let full = d.full().unwrap();
        prop_assert_eq!((full.kind(), full.vcf_count()), (kind, count));
        // Any 64-bit value fits exactly when it is at most the length's longest count.
        prop_assert_eq!(VcfCount::new(count.len(), raw).is_some(), raw <= count.len().max_count());
    }

    /// Any TFDZ offset of the shared strategy that a frame can hold travels through the TFDF
    /// Header of a frame whose TFDZ holds it (732.1 §4.1.4.2.4; USLP-006).
    #[test]
    fn tfdz_offsets_round_trip(offset in tfdz_offset(), extra in 0_usize..4) {
        // restricted: TfdzOffset
        // req: USLP-005, USLP-006, VER-023, VER-054
        let longest_tfdz = LONGEST - HEADER_OCTETS - TFDF_LONG;
        let tfdz_len = usize::from(offset.get()) + 1 + extra;
        prop_assume!(tfdz_len <= longest_tfdz);
        let t = variable(LONGEST, VcParams::new(TransferFrameType::Fixed));
        assert_offset_round_trips(&t, offset, tfdz_len);
    }

    /// Any UPID of the shared strategy travels through the TFDF Header of a frame (USLP-007).
    #[test]
    fn upids_round_trip(u in upid()) {
        // restricted: Upid, UnassignedUpid
        // req: USLP-007, VER-023, VER-054
        let t = variable(64, VcParams::new(TransferFrameType::Variable));
        let octets = frame(&t, fields(FrameKind::SequenceControlled, 0), short_tfdf(u), &[1]).unwrap();
        prop_assert_eq!(UslpFrame::decode(&octets, &t, DecodeMode::Strict).map(|d| d.tfdf_header().upid), Ok(u));
        if let Upid::Other(o) = u {
            prop_assert_eq!(UnassignedUpid::new(o.get()), Some(o));
        }
    }

    /// Every frame of the shared strategy, built on channels whose physical channel, virtual
    /// channels, and truncated frame lengths their checked constructors accepted, decodes to
    /// its length (732.1 §5, Annex D; USLP-002, USLP-003).
    #[test]
    fn configured_channels_carry_frames(f in uslp_frame()) {
        // restricted: PhysicalChannelConfig, VcConfig, TruncatedFrameLen
        // req: USLP-002, USLP-003, VER-023, VER-054
        let d = UslpFrame::decode(&f.octets, &f.channels.table, DecodeMode::Strict).unwrap();
        prop_assert_eq!(d.len(), f.octets.len());
        if let Some(tr) = d.truncated() {
            let vc = aldis_uslp::VcLookup::vc(&f.channels.table, d.spacecraft_id(), d.vc_id()).unwrap();
            prop_assert_eq!(Some(tr.len()), vc.truncated_frame_len().map(TruncatedFrameLen::get));
        }
    }
}

/// A fixed-length 64-octet frame of MAP `map_id` of VC `vc_id` encodes and decodes on `t`.
fn map_frame_round_trips(
    t: &VcTable<2, 2>,
    scid: UslpSpacecraftId,
    vc_id: UslpVcId,
    map_id: UslpMapId,
) {
    let fields = FrameFields {
        spacecraft_id: scid,
        source_or_destination: SourceOrDestination::from_bit(false),
        vc_id,
        map_id,
        kind: FrameKind::Expedited,
        vcf_count: 0,
    };
    let content = FrameContent {
        tfdf: TfdfHeader {
            rule: TfdzRule::PacketsSpanningFrames(FirstHeaderPointer::from_raw(0)),
            upid: Upid::Packets,
        },
        insert_zone: &[],
        ocf: None,
    };
    // The 7-octet header, the 3-octet TFDF Header, and the FECF leave 52 octets.
    let tfdz = [0x5A_u8; 52];
    let mut out = [0_u8; 64];
    assert_eq!(encode(t, fields, content, &tfdz, &mut out), Ok(64));
    let f = UslpFrame::decode(&out, t, DecodeMode::Strict).unwrap();
    assert_eq!(f.tfdz(), tfdz.as_slice());
}

/// `VcTable::new` accepts exactly the channel sets 732.1 §5 allows within its capacities, and
/// rejects each other with its documented error (GEN-011, API-040, API-075). For VCs:
/// `InvalidConfig` with `vc_id` for a VC given twice or VCID 63 on a variable-length channel
/// (Table 5-1 note 4; USLP-009), `transfer_frame_type` for a variable-length VC on a
/// fixed-length channel (Table 5-3 note 2), `frame_data_field` for SDLS lengths that leave no
/// TFDZ octet (CMN-041), and `CapacityExceeded` (`vc_id`) above `V`. For MAPs: `InvalidConfig`
/// (`map_id`) for a MAP on a VC not given, on VCID 63, given twice, or with SDLS lengths on a VC
/// that sets them itself, `frame_data_field` for a MAP whose lengths leave no TFDZ octet, and
/// `CapacityExceeded` (`map_id`) above `M`. Then `truncated_frame_len` for a truncated frame
/// length below 28 with SDLS (Annex D1.2.4; USLP-012). An accepted table carries a frame through
/// `encode` and `decode`.
#[test]
fn vc_tables_imply_vcid_63_only_on_fixed_channels() {
    // restricted: VcTable
    // req: USLP-009, GEN-060
    // VCID 63 needs no entry on a fixed channel (Table 5-2 note 2), and has none on a variable
    // one (Table 5-1 note 4).
    type Table = VcTable<2, 2>;
    let scid = UslpSpacecraftId::new(0xA5A5).unwrap();
    let id = |v: u8| UslpVcId::new(v).unwrap();
    let pc = |t| PhysicalChannelConfig::new(t, 64, 0, true).unwrap();
    let only_vc1 = |t| {
        let vc = VcConfig::new(scid, id(1), VcParams::new(t)).unwrap();
        Table::new(pc(t), &[vc], &[]).unwrap()
    };
    let fixed = only_vc1(TransferFrameType::Fixed);
    assert_eq!(
        fixed.vc(scid, id(63)).map(|v| v.params()),
        Some(VcParams::new(TransferFrameType::Fixed))
    );
    assert_eq!(fixed.vcs().count(), 1);
    assert_eq!(only_vc1(TransferFrameType::Variable).vc(scid, id(63)), None);
}

#[test]
fn vc_tables_accept_exactly_the_configurable_channels() {
    // restricted: VcTable
    // req: API-040, API-075, GEN-011, GEN-060, CMN-041, USLP-009, USLP-012, VER-023
    type Table = VcTable<2, 2>;
    let scid = UslpSpacecraftId::new(0xA5A5).unwrap();
    let id = |v: u8| UslpVcId::new(v).unwrap();
    let mid = |m: u8| UslpMapId::new(m).unwrap();
    let sec = |h: usize| SecurityLayout::new(h, 0).unwrap();
    let vc = |v: u8, params: VcParams| VcConfig::new(scid, id(v), params).unwrap();
    let with = |frame_type, security| VcParams {
        security,
        ..VcParams::new(frame_type)
    };
    let map = |v: u8, m: u8, security| MapConfig::new(scid, id(v), mid(m), security);
    let fixed_pc = PhysicalChannelConfig::new(TransferFrameType::Fixed, 64, 0, true).unwrap();
    let variable_pc = PhysicalChannelConfig::new(TransferFrameType::Variable, 64, 0, true).unwrap();
    let fixed = |v: u8| vc(v, VcParams::new(TransferFrameType::Fixed));
    let variable = |v: u8| vc(v, VcParams::new(TransferFrameType::Variable));
    let none = SecurityLayout::NONE;
    let failure = |pc, vcs: &[VcConfig], maps: &[MapConfig]| {
        let e = Table::new(pc, vcs, maps).unwrap_err();
        (e.kind(), e.field())
    };
    let invalid = |fld| (ErrorKind::InvalidConfig, Some(fld));

    // Accepted, at both capacities: a frame of a MAP round-trips.
    let vcs = [fixed(1), fixed(2)];
    let maps = [map(1, 2, none), map(2, 3, none)];
    let t = Table::new(fixed_pc, &vcs, &maps).unwrap();
    assert_eq!(t.vcs().copied().collect::<Vec<_>>(), vcs);
    assert_eq!(t.maps().copied().collect::<Vec<_>>(), maps);
    map_frame_round_trips(&t, scid, id(1), mid(2));

    // VCs.
    assert_eq!(
        failure(fixed_pc, &[fixed(1), fixed(1)], &[]),
        invalid(field::VC_ID)
    );
    assert_eq!(
        failure(fixed_pc, &[variable(1)], &[]),
        invalid(field::TRANSFER_FRAME_TYPE)
    );
    assert_eq!(
        failure(variable_pc, &[fixed(63)], &[]),
        invalid(field::VC_ID)
    );
    assert!(Table::new(variable_pc, &[fixed(62)], &[]).is_ok());
    // A fixed 64-octet frame: the header, the TFDF Header, and the FECF leave 52 octets.
    let own = |h| vc(1, with(TransferFrameType::Fixed, SecurityScope::Vc(sec(h))));
    assert!(Table::new(fixed_pc, &[own(51)], &[]).is_ok());
    assert_eq!(
        failure(fixed_pc, &[own(52)], &[]),
        invalid(field::FRAME_DATA_FIELD)
    );
    let three = [fixed(1), fixed(2), fixed(3)];
    assert_eq!(
        failure(fixed_pc, &three, &[]),
        (ErrorKind::CapacityExceeded, Some(field::VC_ID))
    );

    // MAPs.
    assert_eq!(
        failure(fixed_pc, &vcs, &[map(5, 2, none)]),
        invalid(field::MAP_ID)
    );
    let oid = [fixed(1), fixed(63)];
    assert_eq!(
        failure(fixed_pc, &oid, &[map(63, 2, none)]),
        invalid(field::MAP_ID)
    );
    let twice = [map(1, 2, none), map(1, 2, none)];
    assert_eq!(failure(fixed_pc, &vcs, &twice), invalid(field::MAP_ID));
    let sets_its_own = [own(2)];
    assert_eq!(
        failure(fixed_pc, &sets_its_own, &[map(1, 2, sec(1))]),
        invalid(field::MAP_ID)
    );
    // Per MAP on a variable 64-octet frame: the header, a 1-octet TFDF Header, and the FECF
    // leave 54 octets.
    let per_map = [vc(
        1,
        with(TransferFrameType::Variable, SecurityScope::PerMap),
    )];
    assert!(Table::new(variable_pc, &per_map, &[map(1, 2, sec(53))]).is_ok());
    assert_eq!(
        failure(variable_pc, &per_map, &[map(1, 2, sec(54))]),
        invalid(field::FRAME_DATA_FIELD)
    );
    let maps3 = [map(1, 2, none), map(1, 3, none), map(2, 4, none)];
    assert_eq!(
        failure(fixed_pc, &vcs, &maps3),
        (ErrorKind::CapacityExceeded, Some(field::MAP_ID))
    );

    // Truncated frames with SDLS need 28 octets (Annex D1.2.4).
    let truncated = |len, security| {
        vc(
            1,
            VcParams {
                truncated_frame_len: Some(TruncatedFrameLen::new(len).unwrap()),
                security,
                ..VcParams::new(TransferFrameType::Variable)
            },
        )
    };
    let sdls = SecurityScope::Vc(SecurityLayout::USLP_TRUNCATED);
    assert!(Table::new(variable_pc, &[truncated(28, sdls)], &[]).is_ok());
    assert!(Table::new(variable_pc, &[truncated(27, SecurityScope::Vc(none))], &[]).is_ok());
    assert_eq!(
        failure(variable_pc, &[truncated(27, sdls)], &[]),
        invalid(field::TRUNCATED_FRAME_LEN)
    );
}

proptest! {
    #![proptest_config(strategies::config(64, REGRESSIONS))]

    /// Every table of the shared strategies is rebuilt from its own channels, and decodes the
    /// frames encoded on it (API-040, VER-054).
    #[test]
    fn vc_tables_from_the_strategies_rebuild(f in uslp_frame()) {
        // req: API-040, VER-023, VER-054
        let t = &f.channels.table;
        let vcs: Vec<VcConfig> = t.vcs().copied().collect();
        let maps: Vec<MapConfig> = t.maps().copied().collect();
        let again = aldis_testkit::strategies::uslp::Channels::new(t.physical(), &vcs, &maps);
        prop_assert_eq!(again.as_ref(), Ok(t));
        prop_assert!(UslpFrame::decode(&f.octets, t, DecodeMode::Strict).is_ok());
    }
}

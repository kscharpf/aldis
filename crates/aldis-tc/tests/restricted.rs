//! A test for every restricted type of `aldis-tc` (RS-020, VER-023; M2 plan WP-7): each public
//! type with a checked constructor accepts exactly the range 232.0-B-4 gives it, rejects every
//! other value with the documented error, and carries every accepted value through its wire
//! field and back. The bounds are written from the clauses cited, as field widths and length
//! limits, and compared with the type's own constants. Inputs come from the shared strategies
//! of `aldis-testkit` (VER-054); each `// restricted:` marker names the type a test covers, and
//! `cargo xtask restricted` checks that every restricted type has one.
#![expect(
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    reason = "test helpers on small bounded values fail loudly (RS-018)"
)]

use aldis_core::crc::write_fecf;
use aldis_core::ids::FrameSeqNum;
use aldis_core::security::SecurityLayout;
use aldis_core::{Error, ErrorDetail, ErrorKind};
use aldis_tc::field;
use aldis_tc::{
    Clcw, ClcwStatus, ControlCommand, CopInEffect, FarmBCounter, FrameData, FrameFields, FrameType,
    MAX_FRAME_LEN, MIN_FRAME_LEN, MapConfig, PRIMARY_HEADER_LEN, PhysicalChannelConfig, Reason,
    SegmentHeader, SegmentHeaderUse, SequenceFlags, TcFrame, TcMapId, TcSpacecraftId, TcVcId,
    VcConfig, VcTable, encode,
};
use aldis_testkit::strategies::proptest::prelude::*;
use aldis_testkit::strategies::tc::{Channels, clcw, frame_type, tc_frame};
use aldis_testkit::strategies::{self, octets};

/// Failing cases are saved here and replayed first.
const REGRESSIONS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/proptest-regressions/restricted.txt"
);

/// Width of the Status Field: bits 3–5 of the CLCW (232.0 §4.2.1.4).
const STATUS_BITS: u32 = 5 - 3 + 1;
/// Width of the COP in Effect: bits 6–7 of the CLCW (232.0 §4.2.1.5.1).
const COP_BITS: u32 = 7 - 6 + 1;
/// Width of the FARM-B Counter: bits 21–22 of the CLCW (232.0 §4.2.1.9.1).
const FARM_B_BITS: u32 = 22 - 21 + 1;
/// Width of the Frame Length field, which holds the length minus one: bits 22–31 (232.0
/// §4.1.2.7.1–2).
const FRAME_LENGTH_BITS: u32 = 31 - 22 + 1;
/// Longest frame: what the Frame Length field can count (232.0 §4.1.2.7 note 2).
const LONGEST: usize = 1 << FRAME_LENGTH_BITS;
/// The Transfer Frame Header, 5 octets (232.0 §4.1.1 a).
const HEADER_OCTETS: usize = 5;
/// Fewest data field octets: the field is mandatory (232.0 §4.1.1 b).
const DATA_MIN: usize = 1;
/// The Frame Error Control Field, 2 octets (232.0 §4.1.1 c).
const FECF_OCTETS: usize = 2;

/// The validation reasons and their stable codes, as `docs/standards/tc-frame.md` §4.1 lists
/// them (M2 plan D14; TC-021, TC-022).
const REASONS: [(u8, &str); 11] = [
    (1, "UnexpectedVersion"),
    (2, "UnexpectedSpacecraft"),
    (3, "ReservedFlags"),
    (4, "ReservedSpare"),
    (5, "InvalidVcid"),
    (6, "FrameTooLong"),
    (7, "FrameTooShort"),
    (8, "LengthMismatch"),
    (9, "FecfMismatch"),
    (10, "InvalidMapId"),
    (11, "ReservedControlCommand"),
];

/// An `OutOfRange` error for `fld` with the bound and the value (API-001, GEN-041).
fn out_of_range(fld: aldis_core::FieldId, bound: u8, found: u8) -> Error {
    Error::new(ErrorKind::OutOfRange)
        .with_field(fld)
        .with_detail(ErrorDetail::Expected {
            expected: u64::from(bound),
            found: u64::from(found),
        })
}

/// A CLCW with every other field fixed.
fn base_clcw() -> Clcw {
    Clcw {
        status: ClcwStatus::default(),
        cop_in_effect: CopInEffect::COP_1,
        vc_id: TcVcId::new(5).unwrap(),
        no_rf_available: false,
        no_bit_lock: true,
        lockout: false,
        wait: true,
        retransmit: false,
        farm_b_counter: FarmBCounter::default(),
        report_value: FrameSeqNum::new(0xA5),
    }
}

/// The CLCW decoded strictly from its own four octets (232.0 §4.2.1; TC-030).
fn clcw_round_trip(c: &Clcw) -> Clcw {
    Clcw::decode(c.to_octets()).unwrap()
}

/// The Status Field is 3 bits, passed through unchanged: `new` and `TryFrom` accept exactly
/// `0..=7`, reject the rest with `OutOfRange` (`clcw_status`), and every value travels through
/// a CLCW (232.0 §4.2.1.4).
#[test]
fn clcw_status_is_three_bits() {
    // restricted: ClcwStatus
    // req: TC-030, TC-031, API-001, GEN-041, VER-023
    assert_eq!(u32::from(ClcwStatus::MAX), (1 << STATUS_BITS) - 1);
    for v in 0..=u8::MAX {
        let in_range = u32::from(v) < 1 << STATUS_BITS;
        assert_eq!(ClcwStatus::new(v).is_some(), in_range, "{v}");
        match ClcwStatus::try_from(v) {
            Ok(status) => {
                assert_eq!((status.get(), u8::from(status)), (v, v));
                let c = Clcw {
                    status,
                    ..base_clcw()
                };
                assert_eq!(clcw_round_trip(&c), c);
            }
            Err(e) => assert_eq!(e, out_of_range(field::CLCW_STATUS, ClcwStatus::MAX, v)),
        }
    }
}

/// The COP in Effect is 2 bits, `01` for COP-1, and the decoder exposes every value: `new` and
/// `TryFrom` accept exactly `0..=3`, reject the rest with `OutOfRange` (`cop_in_effect`), and
/// every value travels through a CLCW (232.0 §4.2.1.5).
#[test]
fn cop_in_effect_is_two_bits() {
    // restricted: CopInEffect
    // req: TC-030, API-001, GEN-041, VER-023
    assert_eq!(u32::from(CopInEffect::MAX), (1 << COP_BITS) - 1);
    // For COP-1 the field is '01' (§4.2.1.5.2).
    assert_eq!(CopInEffect::COP_1.get(), 0b01);
    for v in 0..=u8::MAX {
        let in_range = u32::from(v) < 1 << COP_BITS;
        assert_eq!(CopInEffect::new(v).is_some(), in_range, "{v}");
        match CopInEffect::try_from(v) {
            Ok(cop_in_effect) => {
                assert_eq!(u8::from(cop_in_effect), v);
                let c = Clcw {
                    cop_in_effect,
                    ..base_clcw()
                };
                assert_eq!(clcw_round_trip(&c), c);
            }
            Err(e) => assert_eq!(e, out_of_range(field::COP_IN_EFFECT, CopInEffect::MAX, v)),
        }
    }
}

/// The FARM-B Counter is 2 bits: `new` and `TryFrom` accept exactly `0..=3`, reject the rest
/// with `OutOfRange` (`farm_b_counter`), and every value travels through a CLCW (232.0
/// §4.2.1.9).
#[test]
fn farm_b_counter_is_two_bits() {
    // restricted: FarmBCounter
    // req: TC-030, API-001, GEN-041, VER-023
    assert_eq!(u32::from(FarmBCounter::MAX), (1 << FARM_B_BITS) - 1);
    for v in 0..=u8::MAX {
        let in_range = u32::from(v) < 1 << FARM_B_BITS;
        assert_eq!(FarmBCounter::new(v).is_some(), in_range, "{v}");
        match FarmBCounter::try_from(v) {
            Ok(farm_b_counter) => {
                assert_eq!(farm_b_counter.get(), v);
                let c = Clcw {
                    farm_b_counter,
                    ..base_clcw()
                };
                assert_eq!(clcw_round_trip(&c), c);
            }
            Err(e) => assert_eq!(e, out_of_range(field::FARM_B_COUNTER, FarmBCounter::MAX, v)),
        }
    }
}

/// A table of one Virtual Channel 0 of spacecraft 0x123, without Segment Headers or SDLS, on a
/// Physical Channel of at most `pc_max` octets, the VC of at most `vc_max`.
fn table(pc_max: usize, vc_max: usize, fecf: bool) -> VcTable<1, 1> {
    let pc = PhysicalChannelConfig::new(pc_max, fecf).unwrap();
    let vc = VcConfig::new(
        TcSpacecraftId::new(0x123).unwrap(),
        TcVcId::new(0).unwrap(),
        vc_max,
        SegmentHeaderUse::Absent(SecurityLayout::NONE),
    )
    .unwrap();
    VcTable::new(pc, &[vc], &[]).unwrap()
}

/// Fields of a frame of `frame_type` on the Virtual Channel of [`table`].
const fn fields(frame_type: FrameType) -> FrameFields {
    FrameFields {
        frame_type,
        spacecraft_id: TcSpacecraftId::new(0x123).unwrap(),
        vc_id: TcVcId::new(0).unwrap(),
        sequence_number: FrameSeqNum::new(7),
    }
}

/// Encodes a Type-AD frame of `len` octets on `t`, or the writer's error.
fn frame_of_len(t: &VcTable<1, 1>, len: usize, fecf: bool) -> Result<Vec<u8>, Error> {
    let user = vec![0x3C_u8; len - HEADER_OCTETS - if fecf { FECF_OCTETS } else { 0 }];
    let mut out = vec![0_u8; LONGEST + 8];
    let data = FrameData::Fdu {
        segment_header: None,
        user_data: &user,
    };
    let n = encode(t, fields(FrameType::Ad), data, &mut out)?;
    out.truncate(n);
    Ok(out)
}

/// The Bypass and Control Command Flags select Type-AD (`00`), Type-BD (`10`), or Type-BC
/// (`11`); `01` is reserved (232.0 §4.1.2.3, Table 4-1). `from_flags` accepts exactly the three
/// combinations, each comes back from a frame, and the reserved one on the wire is rejected
/// with `ReservedValue` (`control_command_flag`) in both modes (TC-002).
#[test]
fn frame_type_has_three_flag_combinations() {
    // restricted: FrameType
    // req: TC-002, API-001, VER-023
    let t = table(64, 64, true);
    for bypass in [false, true] {
        for control in [false, true] {
            let reserved = !bypass && control;
            if let Some(ty) = FrameType::from_flags(bypass, control) {
                assert!(!reserved);
                assert_eq!(
                    (ty.bypass_flag(), ty.control_command_flag()),
                    (bypass, control)
                );
                let user = [1_u8, 2, 3];
                let data = if control {
                    FrameData::Control(ControlCommand::Unlock)
                } else {
                    FrameData::Fdu {
                        segment_header: None,
                        user_data: &user,
                    }
                };
                let mut out = [0_u8; 64];
                let n = encode(&t, fields(ty), data, &mut out).unwrap();
                let f = TcFrame::decode(&out[..n], &t).unwrap();
                assert_eq!(f.frame_type(), ty);
            } else {
                assert!(reserved);
                // A Type-AD frame with the Control Command Flag set, resealed.
                let mut frame = frame_of_len(&t, 12, true).unwrap();
                frame[0] |= 0x10;
                write_fecf(&mut frame).unwrap();
                for e in [
                    TcFrame::decode(&frame, &t).unwrap_err(),
                    TcFrame::decode_lenient(&frame, &t).unwrap_err(),
                ] {
                    assert_eq!(e.kind(), ErrorKind::ReservedValue);
                    assert_eq!(e.field(), Some(field::CONTROL_COMMAND_FLAG));
                }
            }
        }
    }
}

/// The Maximum Transfer Frame Length of a Physical Channel is at most 1024 octets, what the
/// Frame Length field counts, and at least the header and one data octet, two more with the
/// FECF (232.0 §4.1.1, §4.1.2.7, Table 5-1). `new` accepts exactly that, rejects the rest with
/// `InvalidConfig` (`max_frame_len`), and on every accepted channel a frame of the maximum
/// length encodes and decodes, and one octet more is refused (TC-007).
#[test]
fn physical_channel_max_frame_len_is_the_spec_range() {
    // restricted: PhysicalChannelConfig
    // req: TC-006, TC-007, API-040, VER-023
    assert_eq!(
        (MAX_FRAME_LEN, MIN_FRAME_LEN),
        (LONGEST, HEADER_OCTETS + DATA_MIN)
    );
    assert_eq!(PRIMARY_HEADER_LEN, HEADER_OCTETS);
    for fecf in [false, true] {
        let shortest = HEADER_OCTETS + DATA_MIN + if fecf { FECF_OCTETS } else { 0 };
        for len in (0..=LONGEST + 16).chain([usize::MAX]) {
            let in_range = (shortest..=LONGEST).contains(&len);
            match PhysicalChannelConfig::new(len, fecf) {
                Ok(pc) => {
                    assert!(in_range, "{len}");
                    assert_eq!((pc.max_frame_len(), pc.fecf()), (len, fecf));
                    let t = table(len, LONGEST, fecf);
                    let frame = frame_of_len(&t, len, fecf).unwrap();
                    let f = TcFrame::decode(&frame, &t).unwrap();
                    assert_eq!((f.len(), f.fecf().is_some()), (len, fecf));
                    if len < LONGEST {
                        let e = frame_of_len(&t, len + 1, fecf).unwrap_err();
                        assert_eq!(
                            (e.kind(), e.field()),
                            (ErrorKind::OutOfRange, Some(field::FRAME_LEN))
                        );
                    }
                }
                Err(e) => {
                    assert!(!in_range, "{len}");
                    assert_eq!(
                        e,
                        Error::new(ErrorKind::InvalidConfig).with_field(field::MAX_FRAME_LEN)
                    );
                }
            }
        }
    }
}

/// The Maximum Transfer Frame Length of a Virtual Channel is up to 1024 octets and at least
/// the header and one data octet (232.0 Table 5-3, §4.1.1). `new` accepts exactly that,
/// rejects the rest with `InvalidConfig` (`max_frame_len`), and on every accepted channel a
/// frame of the VC's maximum length encodes and decodes, and one octet more is refused
/// (TC-007).
#[test]
fn vc_config_max_frame_len_is_the_spec_range() {
    // restricted: VcConfig
    // req: TC-007, API-040, VER-023
    let shortest = HEADER_OCTETS + DATA_MIN;
    for len in (0..=LONGEST + 16).chain([usize::MAX]) {
        let in_range = (shortest..=LONGEST).contains(&len);
        let vc = VcConfig::new(
            TcSpacecraftId::new(0x3FF).unwrap(),
            TcVcId::new(63).unwrap(),
            len,
            SegmentHeaderUse::Present,
        );
        match vc {
            Ok(vc) => {
                assert!(in_range, "{len}");
                assert_eq!(vc.max_frame_len(), len);
                let t = table(LONGEST, len, false);
                let frame = frame_of_len(&t, len, false).unwrap();
                assert_eq!(TcFrame::decode(&frame, &t).map(|f| f.len()), Ok(len));
                if len < LONGEST {
                    let e = frame_of_len(&t, len + 1, false).unwrap_err();
                    assert_eq!(
                        (e.kind(), e.field()),
                        (ErrorKind::OutOfRange, Some(field::FRAME_LEN))
                    );
                }
            }
            Err(e) => {
                assert!(!in_range, "{len}");
                assert_eq!(
                    e,
                    Error::new(ErrorKind::InvalidConfig).with_field(field::MAX_FRAME_LEN)
                );
            }
        }
    }
}

/// The validation reasons are exactly those of `docs/standards/tc-frame.md` §4.1, each with its
/// stable code: `from_code` accepts exactly those codes, names the reason the table names, and
/// gives back its code; `ALL` lists them in code order (TC-021, M2 plan D14).
#[test]
fn reason_codes_are_the_documented_ones() {
    // restricted: Reason
    // req: TC-021, TC-022, VER-023
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
    #![proptest_config(strategies::config(128, REGRESSIONS))]

    /// Any CLCW of the shared strategy, with any value of each restricted field, round-trips
    /// through its four octets (232.0 §4.2.1; TC-030, TC-031).
    #[test]
    fn clcw_fields_round_trip(
        c in clcw(),
        (status, cop, farm_b) in (0..=ClcwStatus::MAX, 0..=CopInEffect::MAX, 0..=FarmBCounter::MAX),
    ) {
        // restricted: ClcwStatus, CopInEffect, FarmBCounter
        // req: TC-030, TC-031, VER-023, VER-054
        let c = Clcw {
            status: ClcwStatus::new(status).unwrap(),
            cop_in_effect: CopInEffect::new(cop).unwrap(),
            farm_b_counter: FarmBCounter::new(farm_b).unwrap(),
            ..c
        };
        let d = Clcw::decode(c.to_octets()).unwrap();
        prop_assert_eq!(d, c);
    }

    /// The frame type of any frame of the shared strategy comes back from its flags; the same
    /// frame with its type changed decodes as that type when the content allows it (232.0
    /// Table 4-1; TC-002).
    #[test]
    fn frame_types_round_trip(f in tc_frame(), ty in frame_type(), user in octets(1..=8)) {
        // restricted: FrameType, PhysicalChannelConfig, VcConfig
        // req: TC-001, TC-002, TC-007, VER-023, VER-054
        let t = &f.channels.table;
        let d = TcFrame::decode(&f.octets, t).unwrap();
        prop_assert_eq!(d.frame_type(), f.fields.frame_type);
        prop_assert!(d.len() <= t.physical().max_frame_len());
        let data = match ty {
            FrameType::Bc => FrameData::Control(ControlCommand::Unlock),
            FrameType::Ad | FrameType::Bd => FrameData::Fdu {
                segment_header: None,
                user_data: &user,
            },
        };
        let fields = FrameFields { frame_type: ty, ..f.channels.plain };
        let mut out = vec![0_u8; 64];
        let n = encode(t, fields, data, &mut out).unwrap();
        prop_assert_eq!(TcFrame::decode(&out[..n], t).map(|d| d.frame_type()), Ok(ty));
    }
}

/// `VcTable::new` accepts exactly the channel sets 232.0 §5 allows within its capacities, and
/// rejects each other with its documented error (GEN-011, API-040): more than `V` Virtual
/// Channels or `M` MAPs (`CapacityExceeded`, `vc_id` or `map_id`); a Virtual Channel given
/// twice (`InvalidConfig`, `vc_id`); a MAP given twice, on a Virtual Channel not given, or on
/// one without a Segment Header (`InvalidConfig`, `map_id`; 232.0 §4.1.3.2.2.1.2); a Virtual
/// Channel or MAP whose longest frame leaves no data octet (`InvalidConfig`, `frame_data_field`;
/// CMN-041). An accepted table carries a frame on each channel through `encode` and `decode`.
#[test]
fn vc_tables_accept_exactly_the_configurable_channels() {
    // restricted: VcTable
    // req: API-040, GEN-011, GEN-060, CMN-041, TC-021, TC-022, VER-023
    type Table = VcTable<2, 2>;
    let scid = TcSpacecraftId::new(0x2A5).unwrap();
    let vc = |v: u8| TcVcId::new(v).unwrap();
    let map = |m: u8| TcMapId::new(m).unwrap();
    let pc = PhysicalChannelConfig::new(MAX_FRAME_LEN, true).unwrap();
    let plain = |v: u8, len: usize, sec: SecurityLayout| {
        VcConfig::new(scid, vc(v), len, SegmentHeaderUse::Absent(sec)).unwrap()
    };
    let segmented =
        |v: u8, len: usize| VcConfig::new(scid, vc(v), len, SegmentHeaderUse::Present).unwrap();
    let on = |v: u8, m: u8, sec: SecurityLayout| MapConfig::new(scid, vc(v), map(m), sec);
    let none = SecurityLayout::NONE;
    let err = |kind, fld| (kind, Some(fld), ErrorDetail::None);
    let invalid = |fld| err(ErrorKind::InvalidConfig, fld);
    let failure = |vcs: &[VcConfig], maps: &[MapConfig]| {
        let e = Table::new(pc, vcs, maps).unwrap_err();
        (e.kind(), e.field(), e.detail())
    };

    // Accepted, at both capacities: a frame on each channel round-trips.
    let vcs = [plain(1, MAX_FRAME_LEN, none), segmented(2, MAX_FRAME_LEN)];
    let maps = [on(2, 3, none), on(2, 4, none)];
    let t = Table::new(pc, &vcs, &maps).unwrap();
    assert_eq!(t.vcs().copied().collect::<Vec<_>>(), vcs);
    assert_eq!(t.maps().copied().collect::<Vec<_>>(), maps);
    let user = [0x5A_u8; 9];
    for (v, segment_header) in [
        (1, None),
        (
            2,
            Some(SegmentHeader {
                sequence_flags: SequenceFlags::Unsegmented,
                map_id: map(3),
            }),
        ),
        (
            2,
            Some(SegmentHeader {
                sequence_flags: SequenceFlags::Unsegmented,
                map_id: map(4),
            }),
        ),
    ] {
        let fields = FrameFields {
            frame_type: FrameType::Ad,
            spacecraft_id: scid,
            vc_id: vc(v),
            sequence_number: FrameSeqNum::new(1),
        };
        let mut out = [0_u8; 64];
        let data = FrameData::Fdu {
            segment_header,
            user_data: &user,
        };
        let n = encode(&t, fields, data, &mut out).unwrap();
        let f = TcFrame::decode(&out[..n], &t).unwrap();
        assert_eq!(
            (f.header().vc_id(), f.segment_header()),
            (vc(v), segment_header)
        );
    }

    // Capacities (GEN-011).
    let three = [plain(1, 64, none), plain(5, 64, none), segmented(2, 64)];
    assert_eq!(
        failure(&three, &[]),
        err(ErrorKind::CapacityExceeded, field::VC_ID)
    );
    let maps3 = [on(2, 3, none), on(2, 4, none), on(2, 5, none)];
    assert_eq!(
        failure(&vcs, &maps3),
        err(ErrorKind::CapacityExceeded, field::MAP_ID)
    );
    // Duplicates and MAPs without a place.
    assert_eq!(
        failure(&[plain(1, 64, none), segmented(1, 64)], &[]),
        invalid(field::VC_ID)
    );
    assert_eq!(
        failure(&vcs, &[on(2, 3, none), on(2, 3, none)]),
        invalid(field::MAP_ID)
    );
    assert_eq!(failure(&vcs, &[on(6, 3, none)]), invalid(field::MAP_ID));
    assert_eq!(failure(&vcs, &[on(1, 3, none)]), invalid(field::MAP_ID));
    // No data octet: the Primary Header (5), the Segment Header (1), the security fields, and
    // the FECF (2) fill the longest frame.
    let full = SecurityLayout::new(3, 0).unwrap();
    assert_eq!(
        failure(&[plain(1, 10, full)], &[]),
        invalid(field::FRAME_DATA_FIELD)
    );
    assert!(Table::new(pc, &[plain(1, 11, full)], &[]).is_ok());
    let tight = [segmented(2, 11)];
    assert_eq!(
        failure(&tight, &[on(2, 3, full)]),
        invalid(field::FRAME_DATA_FIELD)
    );
    assert!(Table::new(pc, &tight, &[on(2, 3, SecurityLayout::new(2, 0).unwrap())]).is_ok());
}

proptest! {
    #![proptest_config(strategies::config(64, REGRESSIONS))]

    /// Every table of the shared strategies is rebuilt from its own channels, and decodes the
    /// frames encoded on it (API-040, VER-054).
    #[test]
    fn vc_tables_from_the_strategies_rebuild(f in tc_frame()) {
        // req: API-040, VER-023, VER-054
        let t = &f.channels.table;
        let vcs: Vec<VcConfig> = t.vcs().copied().collect();
        let maps: Vec<MapConfig> = t.maps().copied().collect();
        let again = Channels::new(t.physical(), &vcs, &maps).unwrap();
        prop_assert_eq!(&again, t);
        prop_assert!(TcFrame::decode(&f.octets, &again).is_ok());
    }
}

//! A test for every restricted type of `aldis-tm` (RS-020, VER-023; M2 plan WP-7): each public
//! type with a checked constructor accepts exactly the range 132.0-B-3 gives it, rejects every
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

use aldis_core::crc::write_fecf;
use aldis_core::ids::{SpacecraftId, VcId};
use aldis_core::{Error, ErrorDetail, ErrorKind};
use aldis_testkit::strategies::common::{spacecraft_id, vc_id};
use aldis_testkit::strategies::proptest::prelude::*;
use aldis_testkit::strategies::proptest::test_runner::TestCaseError;
use aldis_testkit::strategies::tm::{
    first_header_pointer, fsh_len, packet_offset, physical_channel, segment_length_id,
};
use aldis_testkit::strategies::{self, octets};
use aldis_tm::MAX_FRAME_LEN;
use aldis_tm::field;
use aldis_tm::header::PRIMARY_HEADER_LEN;
use aldis_tm::{
    DataFieldContent, DataFieldStatus, FirstHeaderPointer, FshLen, PacketOffset, PacketPointer,
    SegmentLengthId, TmFrame, TmFrameContent, TmFrameFields, TmMasterChannelConfig,
    TmPhysicalChannelConfig, TmVirtualChannelConfig, encode,
};

/// Failing cases are saved here and replayed first.
const REGRESSIONS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/proptest-regressions/restricted.txt"
);

/// Width of the Segment Length Identifier: bits 35–36 (132.0 §4.1.2.7.5.1).
const SLI_BITS: u32 = 36 - 35 + 1;
/// Width of the First Header Pointer: bits 37–47 (132.0 §4.1.2.7.6.1).
const FHP_BITS: u32 = 47 - 37 + 1;
/// All ones, "no Packet starts" (132.0 §4.1.2.7.6.4).
const FHP_NO_PACKET_START: u16 = (1 << FHP_BITS) - 1;
/// All ones but the last bit, "only Idle Data" (132.0 §4.1.2.7.6.5).
const FHP_ONLY_IDLE_DATA: u16 = FHP_NO_PACKET_START - 1;
/// The Secondary Header: a 1-octet Identification Field and 1 to 63 data octets (132.0
/// §4.1.3.1.3).
const FSH_ID_LEN: u8 = 1;
/// Fewest Secondary Header data octets (132.0 §4.1.3.1.3 b).
const FSH_DATA_MIN: u8 = 1;
/// Most Secondary Header data octets (132.0 §4.1.3.1.3 b).
const FSH_DATA_MAX: u8 = 63;
/// Length of the Frame Error Control Field (132.0 §4.1.1.1 e).
const FECF_OCTETS: usize = 2;
/// Fewest data field octets: the field is mandatory, a whole number of octets (132.0
/// §4.1.1.1 c).
const DATA_MIN: usize = 1;

/// An `OutOfRange` error for `field` with the bound and the value (API-001, GEN-041).
const fn out_of_range(fld: aldis_core::FieldId, bound: u64, found: u64) -> Error {
    Error::new(ErrorKind::OutOfRange)
        .with_field(fld)
        .with_detail(ErrorDetail::Expected {
            expected: bound,
            found,
        })
}

/// A master channel of `frame_len`-octet frames with one virtual channel 0 carrying `content`,
/// with a `VC_FSH` of `fsh` octets if given.
fn channel(
    frame_len: usize,
    fecf: bool,
    content: DataFieldContent,
    fsh: Option<FshLen>,
) -> TmMasterChannelConfig {
    let pc = TmPhysicalChannelConfig::new(frame_len, fecf).unwrap();
    let mut vc = TmVirtualChannelConfig::new(VcId::new(0).unwrap(), content);
    if let Some(len) = fsh {
        vc = vc.with_fsh(len);
    }
    TmMasterChannelConfig::new(pc, SpacecraftId::new(0x2A).unwrap(), None, false)
        .with_vc(vc)
        .unwrap()
}

/// Encodes a frame of VC 0 of `mc` with `status` and a zero data field, then decodes it
/// strictly: the decoded status and Secondary Header length.
fn round_trip(
    mc: &TmMasterChannelConfig,
    status: DataFieldStatus,
) -> Result<(DataFieldStatus, Option<FshLen>), Error> {
    let vcid = VcId::new(0).unwrap();
    let layout = mc.frame_layout(vcid).unwrap();
    let data = vec![0_u8; layout.data().len()];
    let fsh = vec![0x5A_u8; layout.secondary().len().saturating_sub(1)];
    let content = TmFrameContent {
        secondary_header_data: &fsh,
        ..TmFrameContent::new(&data)
    };
    let mut out = vec![0_u8; mc.pc().frame_len()];
    let n = encode(
        mc,
        &TmFrameFields::new(vcid, 1, 2, status),
        &content,
        &mut out,
    )?;
    let f = TmFrame::decode_with(&out[..n], mc.pc(), mc)?;
    Ok((f.data_field_status(), f.fsh_len()))
}

/// The status of a `VCA_SDU` frame with these raw fields.
const fn vca(sli: SegmentLengthId, fhp: FirstHeaderPointer) -> DataFieldStatus {
    DataFieldStatus::VcaSdu {
        packet_order_flag: false,
        segment_length_id: sli,
        first_header_pointer: fhp,
    }
}

/// The Segment Length Identifier is 2 bits: `new` and `TryFrom` accept exactly `0..=3`, reject
/// the rest with `OutOfRange` (`segment_length_id`), and every value travels through a
/// `VCA_SDU` frame, where it is carried raw (132.0 §4.1.2.7.5).
#[test]
fn segment_length_id_is_two_bits() {
    // restricted: SegmentLengthId
    // req: TM-007, API-001, GEN-041, VER-023
    assert_eq!(u32::from(SegmentLengthId::MAX), (1 << SLI_BITS) - 1);
    // '11' marks non-use of Source Packet Segments (§4.1.2.7.5.2).
    assert_eq!(SegmentLengthId::NO_SEGMENTS.get(), 0b11);
    let mc = channel(32, true, DataFieldContent::VcaSdu, None);
    for v in 0..=u8::MAX {
        let in_range = u32::from(v) < 1 << SLI_BITS;
        assert_eq!(SegmentLengthId::new(v).is_some(), in_range, "{v}");
        match SegmentLengthId::try_from(v) {
            Ok(sli) => {
                assert!(in_range);
                assert_eq!(sli.get(), v);
                let status = vca(sli, FirstHeaderPointer::NO_PACKET_START);
                assert_eq!(round_trip(&mc, status).unwrap().0, status);
            }
            Err(e) => assert_eq!(
                e,
                out_of_range(
                    field::SEGMENT_LENGTH_ID,
                    u64::from(SegmentLengthId::MAX),
                    u64::from(v)
                )
            ),
        }
    }
}

/// The First Header Pointer is 11 bits: `new` and `TryFrom` accept exactly `0..=0x7FF`,
/// reject the rest with `OutOfRange` (`first_header_pointer`), and every value travels raw
/// through a `VCA_SDU` frame (132.0 §4.1.2.7.6.1, §4.1.2.7.6.2 note).
#[test]
fn first_header_pointer_is_eleven_bits() {
    // restricted: FirstHeaderPointer
    // req: TM-008, TM-009, API-001, GEN-041, VER-023
    assert_eq!(FirstHeaderPointer::MAX, FHP_NO_PACKET_START);
    assert_eq!(
        FirstHeaderPointer::NO_PACKET_START.get(),
        FHP_NO_PACKET_START
    );
    assert_eq!(FirstHeaderPointer::ONLY_IDLE_DATA.get(), FHP_ONLY_IDLE_DATA);
    let mc = channel(32, false, DataFieldContent::VcaSdu, None);
    for v in 0..=u16::MAX {
        let in_range = v <= FHP_NO_PACKET_START;
        assert_eq!(FirstHeaderPointer::new(v).is_some(), in_range, "{v}");
        match FirstHeaderPointer::try_from(v) {
            Ok(fhp) => {
                assert_eq!(fhp.get(), v);
                let status = vca(SegmentLengthId::NO_SEGMENTS, fhp);
                assert_eq!(round_trip(&mc, status).unwrap().0, status);
            }
            Err(e) => assert_eq!(
                e,
                out_of_range(
                    field::FIRST_HEADER_POINTER,
                    u64::from(FirstHeaderPointer::MAX),
                    u64::from(v)
                )
            ),
        }
    }
}

/// The Secondary Header is 2 to 64 octets, the Identification Field and 1 to 63 data octets:
/// `new` and `TryFrom` accept exactly that, reject the rest with `OutOfRange` (`fsh_len`) and
/// the bound crossed, and every length travels through the Identification Field of a frame
/// (132.0 §4.1.3.1.3, §4.1.3.2.3.2).
#[test]
fn fsh_len_is_two_to_sixty_four() {
    // restricted: FshLen
    // req: TM-003, TM-009, API-040, GEN-041, VER-023
    let (min, max) = (FSH_ID_LEN + FSH_DATA_MIN, FSH_ID_LEN + FSH_DATA_MAX);
    assert_eq!((FshLen::MIN, FshLen::MAX), (min, max));
    for v in 0..=u8::MAX {
        let in_range = (min..=max).contains(&v);
        assert_eq!(FshLen::new(v).is_some(), in_range, "{v}");
        match FshLen::try_from(v) {
            Ok(len) => {
                assert_eq!((len.get(), len.octets()), (v, usize::from(v)));
                // The length field holds the total length minus one (§4.1.3.2.3.2).
                assert_eq!(len.id_octet(), v - 1);
                let mc = channel(80, true, DataFieldContent::Packets, Some(len));
                let status = DataFieldStatus::Packets(PacketPointer::NoPacketStart);
                assert_eq!(round_trip(&mc, status).unwrap(), (status, Some(len)));
            }
            Err(e) => {
                let bound = if v < min { min } else { max };
                assert_eq!(
                    e,
                    out_of_range(field::FSH_LEN, u64::from(bound), u64::from(v))
                );
            }
        }
    }
}

/// A packet offset is any 11-bit pointer value below the two reserved ones: `new` and
/// `TryFrom` accept exactly `0..=0x7FD`, reject the rest with `OutOfRange`
/// (`first_header_pointer`), and every offset travels through the First Header Pointer of a
/// frame whose data field is long enough to hold it (132.0 §4.1.2.7.6.3–5).
#[test]
fn packet_offset_is_below_the_reserved_pointers() {
    // restricted: PacketOffset
    // req: TM-008, TM-009, API-001, GEN-041, VER-023
    assert_eq!(PacketOffset::MAX, FHP_ONLY_IDLE_DATA - 1);
    // The longest data field an offset needs: one octet past the largest offset.
    let data_len = usize::from(FHP_ONLY_IDLE_DATA);
    let mc = channel(
        PRIMARY_HEADER_LEN + data_len + FECF_OCTETS,
        true,
        DataFieldContent::Packets,
        None,
    );
    for v in 0..=u16::MAX {
        let in_range = v < FHP_ONLY_IDLE_DATA;
        assert_eq!(PacketOffset::new(v).is_some(), in_range, "{v}");
        match PacketOffset::try_from(v) {
            Ok(offset) => {
                assert_eq!(offset.get(), v);
                let status = DataFieldStatus::Packets(PacketPointer::StartsAt(offset));
                assert_eq!(round_trip(&mc, status).unwrap().0, status);
            }
            Err(e) => assert_eq!(
                e,
                out_of_range(
                    field::FIRST_HEADER_POINTER,
                    u64::from(PacketOffset::MAX),
                    u64::from(v)
                )
            ),
        }
    }
}

/// The classification of every First Header Pointer of a Synchronization Flag `0` frame, for
/// data fields at and around the pointer's edges: all ones is "no Packet starts", all ones
/// minus one "only Idle Data", an offset inside the data field a Packet start, and any other
/// value `Malformed` (`first_header_pointer`) with the last offset of the field; an accepted
/// pointer gives back its raw value (132.0 §4.1.2.7.6.3–5).
#[test]
fn packet_pointer_classifies_every_raw_value() {
    // restricted: PacketPointer
    // req: TM-008, API-032, GEN-041, VER-023
    let edges = [1_usize, 2, 0x7FC, 0x7FD, 0x7FE, 0x7FF, 0x800, 4096];
    for data_len in edges {
        for raw in 0..=FHP_NO_PACKET_START {
            let fhp = FirstHeaderPointer::new(raw).unwrap();
            let expected = match raw {
                FHP_NO_PACKET_START => Ok(PacketPointer::NoPacketStart),
                FHP_ONLY_IDLE_DATA => Ok(PacketPointer::OnlyIdleData),
                n if usize::from(n) < data_len => {
                    Ok(PacketPointer::StartsAt(PacketOffset::new(n).unwrap()))
                }
                n => Err(Error::new(ErrorKind::Malformed)
                    .with_field(field::FIRST_HEADER_POINTER)
                    .with_detail(ErrorDetail::Expected {
                        expected: u64::try_from(data_len - 1).unwrap(),
                        found: u64::from(n),
                    })),
            };
            let got = PacketPointer::classify(fhp, data_len);
            assert_eq!(got, expected, "raw {raw:#x}, data field {data_len}");
            if let Ok(p) = got {
                assert_eq!(p.to_raw(), fhp);
                assert_eq!(FirstHeaderPointer::from(p), fhp);
            }
        }
    }
}

/// The shortest frame holds the 6-octet Primary Header, one data octet, and the FECF if
/// present; 132.0 sets no upper bound (132.0 §4.1.1.1, §4.1.1.2), and the project sets 4 095
/// octets (OI-51: above it the CRC-16 misses some double-bit errors). `new` accepts exactly the
/// lengths from that minimum to 4 095, rejects the shorter ones with `InvalidConfig`
/// (`frame_len`) and the minimum, the longer ones with `InvalidConfig` (`frame_len`) and 4 095, and a frame of every accepted length up to 300 octets encodes and decodes
/// at that length.
#[test]
fn physical_channel_frame_length_has_its_minimum() {
    // restricted: TmPhysicalChannelConfig
    // req: TM-001, TM-006, API-040, GEN-041, VER-023, VER-064
    for fecf in [false, true] {
        let min = PRIMARY_HEADER_LEN + DATA_MIN + if fecf { FECF_OCTETS } else { 0 };
        for len in (0..=300).chain([4094, 4095, 4096, 4097, usize::MAX - 1, usize::MAX]) {
            match TmPhysicalChannelConfig::new(len, fecf) {
                Ok(pc) => {
                    assert!((min..=MAX_FRAME_LEN).contains(&len), "{len}");
                    assert_eq!((pc.frame_len(), pc.fecf()), (len, fecf));
                    if len <= 300 {
                        let mc = channel(len, fecf, DataFieldContent::Packets, None);
                        let status = DataFieldStatus::Packets(PacketPointer::OnlyIdleData);
                        assert_eq!(round_trip(&mc, status).unwrap().0, status);
                    }
                }
                Err(e) => {
                    assert!(len < min || len > MAX_FRAME_LEN, "{len}");
                    let bound = if len < min { min } else { MAX_FRAME_LEN };
                    assert_eq!(
                        e,
                        Error::new(ErrorKind::InvalidConfig)
                            .with_field(field::FRAME_LEN)
                            .with_detail(ErrorDetail::Expected {
                                expected: u64::try_from(bound).unwrap(),
                                found: u64::try_from(len).unwrap(),
                            })
                    );
                }
            }
        }
    }
}

/// Rewrites the First Header Pointer of `frame` to `raw` and reseals the FECF if present.
fn with_pointer(frame: &mut [u8], raw: u16, fecf: bool) {
    let [hi, lo] = raw.to_be_bytes();
    frame[4] = (frame[4] & 0xF8) | hi;
    frame[5] = lo;
    if fecf {
        write_fecf(frame).unwrap();
    }
}

proptest! {
    #![proptest_config(strategies::config(128, REGRESSIONS))]

    /// The raw fields of a `VCA_SDU` frame drawn from the shared strategies come back from a
    /// frame of any physical channel and spacecraft (132.0 §4.1.2.7.4–6 notes).
    #[test]
    fn vca_sdu_raw_fields_round_trip(
        pc in physical_channel(),
        scid in spacecraft_id::<10>(),
        vcid in vc_id::<3>(),
        (sli, fhp, pof) in (segment_length_id(), first_header_pointer(), any::<bool>()),
        counts in any::<(u8, u8)>(),
    ) {
        // restricted: SegmentLengthId, FirstHeaderPointer
        // req: TM-007, TM-008, TM-009, VER-023, VER-054
        let mc = TmMasterChannelConfig::new(pc, scid, None, false)
            .with_vc(TmVirtualChannelConfig::new(vcid, DataFieldContent::VcaSdu))
            .unwrap();
        let status = DataFieldStatus::VcaSdu {
            packet_order_flag: pof,
            segment_length_id: sli,
            first_header_pointer: fhp,
        };
        let data = vec![0_u8; mc.frame_layout(vcid).unwrap().data().len()];
        let mut out = vec![0_u8; pc.frame_len()];
        let n = encode(&mc, &TmFrameFields::new(vcid, counts.0, counts.1, status), &TmFrameContent::new(&data), &mut out).unwrap();
        let f = TmFrame::decode_with(&out[..n], mc.pc(), &mc).unwrap();
        prop_assert_eq!(f.data_field_status(), status);
    }

    /// A Secondary Header length drawn from the shared strategy comes back from the
    /// Identification Field of a frame on any physical channel long enough to hold it (132.0
    /// §4.1.3.2.3).
    #[test]
    fn fsh_len_round_trips(pc in physical_channel(), len in fsh_len(), data in octets(63)) {
        // restricted: FshLen
        // req: TM-003, TM-009, VER-023, VER-054
        let vcid = VcId::new(1).unwrap();
        let vc = TmVirtualChannelConfig::new(vcid, DataFieldContent::Packets).with_fsh(len);
        // A channel that leaves no data octet with this Secondary Header is not drawn (CMN-041).
        let Ok(mc) = TmMasterChannelConfig::new(pc, SpacecraftId::new(7).unwrap(), None, false).with_vc(vc) else {
            return Err(TestCaseError::reject("no data octet"));
        };
        let fsh_data = &data[..usize::from(len.get() - 1)];
        let zone = vec![0_u8; mc.frame_layout(vcid).unwrap().data().len()];
        let content = TmFrameContent { secondary_header_data: fsh_data, ..TmFrameContent::new(&zone) };
        let status = DataFieldStatus::Packets(PacketPointer::NoPacketStart);
        let mut out = vec![0_u8; pc.frame_len()];
        let n = encode(&mc, &TmFrameFields::new(vcid, 0, 0, status), &content, &mut out).unwrap();
        let f = TmFrame::decode_with(&out[..n], mc.pc(), &mc).unwrap();
        prop_assert_eq!(f.fsh_len(), Some(len));
        prop_assert_eq!(f.secondary_header_data(), Some(fsh_data));
    }

    /// A packet offset drawn from the shared strategy, and any raw pointer, decode from a
    /// frame of any physical channel exactly as `PacketPointer::classify` gives them for its
    /// data field (132.0 §4.1.2.7.6; TM-008).
    #[test]
    fn decoded_pointers_are_classified(
        pc in physical_channel(),
        offset in packet_offset(),
        raw in first_header_pointer(),
        use_offset in any::<bool>(),
    ) {
        // restricted: PacketOffset, PacketPointer, TmPhysicalChannelConfig
        // req: TM-001, TM-008, TM-009, VER-023, VER-054
        let mc = TmMasterChannelConfig::new(pc, SpacecraftId::new(9).unwrap(), None, false)
            .with_vc(TmVirtualChannelConfig::new(VcId::new(0).unwrap(), DataFieldContent::Packets))
            .unwrap();
        let data_len = mc.frame_layout(VcId::new(0).unwrap()).unwrap().data().len();
        let raw = if use_offset { offset.get() } else { raw.get() };
        let fields = TmFrameFields::new(VcId::new(0).unwrap(), 0, 0, DataFieldStatus::Packets(PacketPointer::NoPacketStart));
        let data = vec![0_u8; data_len];
        let mut out = vec![0_u8; pc.frame_len()];
        let n = encode(&mc, &fields, &TmFrameContent::new(&data), &mut out).unwrap();
        prop_assert_eq!(n, pc.frame_len());
        with_pointer(&mut out, raw, pc.fecf());
        let expected = PacketPointer::classify(FirstHeaderPointer::new(raw).unwrap(), data_len);
        let got = TmFrame::decode_with(&out, mc.pc(), &mc).map(|f| f.data_field_status());
        prop_assert_eq!(got, expected.map(DataFieldStatus::Packets));
    }
}

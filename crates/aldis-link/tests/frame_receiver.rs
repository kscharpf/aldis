//! Bounds, refusals, reconfiguration, housekeeping, and the decisions of OI-78 for the
//! `FrameReceiver` (LNK-002, LNK-003, LNK-050…LNK-054, LNK-074, LNK-075, LNK-077, LNK-079,
//! LNK-080, LNK-081, API-053, API-056, API-057, API-058).
#![expect(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::as_conversions,
    unused_must_use,
    reason = "tests fail loudly by design"
)]

use aldis_core::ErrorKind;
use aldis_core::engine::OutRange;
use aldis_core::field;
use aldis_core::ids::{MapId, VcId};
use aldis_core::ocf::Ocf;
use aldis_link::protocol::{Aos, Tc, Tm, Uslp};
use aldis_link::receiver::{
    AosChannel, AosContent, AosVc, CallerDiscardReason, ChannelScope, CheckedFrame, DiscardReason,
    FRAME_RECEIVER_MAX_ACTIONS, FrameReceiver, FrameReceiverConfig, MapSet, ReceiveProtocol,
    ReceiverInput, ReceiverOutput, Service, TcChannel, TcVc, TmChannel, TmContent, TmVc,
    UslpChannel, UslpVc, VcEntry,
};
use aldis_testkit::strategies;
use aldis_testkit::strategies::proptest::prelude::*;
use aldis_testkit::strategies::proptest::test_runner::RngSeed;

/// Failing cases are saved here and replayed first.
const REGRESSIONS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/proptest-regressions/frame_receiver.txt"
);

const LEN: usize = 40;
const CLCW: [u8; 4] = [0x01, 0x04, 0x08, 0x05];
const REPORT2: [u8; 4] = [0x80, 0x11, 0x22, 0x33];

// ---- frame builders (no FECF: the receiver trusts the caller's check) ----

fn tm_frame(
    vc: u8,
    mc: u8,
    vcc: u8,
    fhp: u16,
    sync: bool,
    fsh: &[u8],
    ocf: Option<[u8; 4]>,
) -> Vec<u8> {
    let w0 = (0x123u16 << 4) | (u16::from(vc) << 1) | u16::from(ocf.is_some());
    let w2 = (u16::from(!fsh.is_empty()) << 15) | (u16::from(sync) << 14) | (3 << 11) | fhp;
    let mut f = w0.to_be_bytes().to_vec();
    f.extend([mc, vcc]);
    f.extend(w2.to_be_bytes());
    f.extend(fsh);
    f.resize(LEN - ocf.map_or(0, |_| 4), 0x55);
    f.extend(ocf.into_iter().flatten());
    f
}

fn aos_frame(
    vc: u8,
    count: u32,
    cycle: Option<u8>,
    replay: bool,
    ocf: Option<[u8; 4]>,
    data: &[u8],
) -> Vec<u8> {
    let mut f = vec![0x40, vc];
    f.extend(&count.to_be_bytes()[1..]);
    f.push((u8::from(replay) << 7) | (u8::from(cycle.is_some()) << 6) | cycle.unwrap_or(0));
    f.extend([0xA1, 0xA2]);
    f.extend(data);
    f.resize(LEN - ocf.map_or(0, |_| 4), 0x77);
    f.extend(ocf.into_iter().flatten());
    f
}

fn uslp_frame(
    vc: u8,
    map: u8,
    bypass: bool,
    control: bool,
    count: Option<(u8, u64)>,
    ocf: Option<[u8; 4]>,
) -> Vec<u8> {
    let top = (0b1100u32 << 28) | (0x1234 << 12) | (u32::from(vc) << 5) | (u32::from(map) << 1);
    let (len, value) = count.unwrap_or((0, 0));
    let flags =
        (u8::from(bypass) << 7) | (u8::from(control) << 6) | (u8::from(ocf.is_some()) << 3) | len;
    let mut f = top.to_be_bytes().to_vec();
    f.extend((LEN as u16 - 1).to_be_bytes());
    f.push(flags);
    f.extend(&value.to_be_bytes()[8 - usize::from(len)..]);
    f.extend([0xB1, 0xB2]);
    f.resize(LEN - ocf.map_or(0, |_| 4), 0x33);
    f.extend(ocf.into_iter().flatten());
    f
}

fn tc_frame(vc: u8) -> Vec<u8> {
    let w1 = (u16::from(vc) << 10) | (LEN as u16 - 1);
    let mut f = vec![0x00, 0x2A];
    f.extend(w1.to_be_bytes());
    f.push(0);
    f.resize(LEN, 0x11);
    f
}

// ---- receivers ----

type TmRx = FrameReceiver<Tm, 8>;
type AosRx = FrameReceiver<Aos, 8>;
type UslpRx = FrameReceiver<Uslp, 8>;
type TcRx = FrameReceiver<Tc, 8>;

fn vc<const B: u32>(n: u8) -> VcId<B> {
    VcId::<B>::new(n).unwrap()
}

fn tm_config(ocf: Service, fsh: Service) -> FrameReceiverConfig<Tm, 8> {
    FrameReceiverConfig::new(
        LEN,
        false,
        TmChannel::new(ocf, fsh),
        &[
            VcEntry::new(vc(0), TmVc::new(TmContent::Packets, false, None).unwrap()),
            VcEntry::new(vc(1), TmVc::new(TmContent::Vca, false, None).unwrap()),
            VcEntry::new(
                vc(5),
                TmVc::new(TmContent::Unserved, true, Some(2)).unwrap(),
            ),
        ],
    )
    .unwrap()
}

fn tm() -> TmRx {
    TmRx::new(tm_config(Service::Vc, Service::Vc))
}

fn aos_config(fhec: bool) -> FrameReceiverConfig<Aos, 8> {
    FrameReceiverConfig::new(
        LEN,
        false,
        AosChannel::new(2, fhec),
        &[
            VcEntry::new(vc(1), AosVc::new(AosContent::Packets, true)),
            VcEntry::new(vc(2), AosVc::new(AosContent::Vca, false)),
            VcEntry::new(vc(3), AosVc::new(AosContent::Bitstream, false)),
            VcEntry::new(vc(5), AosVc::new(AosContent::Unserved, false)),
        ],
    )
    .unwrap()
}

fn aos() -> AosRx {
    AosRx::new(aos_config(false))
}

fn uslp_config() -> FrameReceiverConfig<Uslp, 8> {
    let maps = |m: &[u8]| {
        MapSet::new(
            &m.iter()
                .map(|&n| MapId::<4>::new(n).unwrap())
                .collect::<Vec<_>>(),
        )
    };
    FrameReceiverConfig::new(
        LEN,
        false,
        UslpChannel::new(2, Service::Mc),
        &[
            VcEntry::new(vc(1), UslpVc::new(maps(&[0, 2]), false, true)),
            VcEntry::new(vc(2), UslpVc::new(maps(&[0]), true, true)),
            VcEntry::new(vc(3), UslpVc::new(maps(&[0]), false, false)),
        ],
    )
    .unwrap()
}

fn uslp() -> UslpRx {
    UslpRx::new(uslp_config())
}

fn tc() -> TcRx {
    TcRx::new(
        FrameReceiverConfig::new(
            LEN,
            false,
            TcChannel,
            &[
                VcEntry::new(vc(1), TcVc::SERVED),
                VcEntry::new(vc(2), TcVc::UNSERVED),
            ],
        )
        .unwrap(),
    )
}

type Out<P> = Vec<ReceiverOutput<P>>;

fn feed<P: ReceiveProtocol, const V: usize>(rx: &mut FrameReceiver<P, V>, frame: &[u8]) -> Out<P> {
    try_feed(rx, frame).unwrap()
}

fn try_feed<P: ReceiveProtocol, const V: usize>(
    rx: &mut FrameReceiver<P, V>,
    frame: &[u8],
) -> Result<Out<P>, aldis_core::Error> {
    let mut area = vec![0u8; rx.config().area_len()];
    rx.step(ReceiverInput::Frame(CheckedFrame::new(frame)), &mut area)
        .map(|a| a.iter().copied().collect())
}

fn refused<P: ReceiveProtocol, const V: usize>(
    rx: &mut FrameReceiver<P, V>,
    frame: &[u8],
) -> aldis_core::Error {
    try_feed(rx, frame).unwrap_err()
}

const fn is_gap<P: ReceiveProtocol>(o: &ReceiverOutput<P>, missing: u64) -> bool {
    matches!(o, ReceiverOutput::FrameGap { missing: m, .. } if *m == missing)
}

// ---- LNK-074: the worst cases ----

#[test]
fn the_worst_case_of_each_protocol_returns_exactly_the_bound() {
    // req: LNK-074, LNK-050, LNK-051, LNK-053, API-057
    assert_eq!(FRAME_RECEIVER_MAX_ACTIONS, 5);
    // TM: a frame for a configured VC that is not served, after a gap of the MC and of the VC:
    // MC gap, VC gap, FSH, OCF, FrameDiscarded.
    let mut rx = tm();
    feed(&mut rx, &tm_frame(5, 0, 0, 0, true, &[1, 9], Some(CLCW)));
    let out = feed(&mut rx, &tm_frame(5, 2, 2, 0, true, &[1, 9], Some(CLCW)));
    assert_eq!(out.len(), 5);
    assert!(out.iter().take(2).all(|o| is_gap(o, 1)));
    assert!(matches!(out[2], ReceiverOutput::FshSdu { .. }));
    assert!(matches!(out[3], ReceiverOutput::Ocf { .. }));
    assert!(matches!(
        out[4],
        ReceiverOutput::FrameDiscarded {
            reason: DiscardReason::NotServed,
            ..
        }
    ));
    // A served TM frame has five too: two gaps, FSH, OCF, field.
    let config = FrameReceiverConfig::<Tm, 8>::new(
        LEN,
        false,
        TmChannel::new(Service::Vc, Service::Vc),
        &[VcEntry::new(
            vc(0),
            TmVc::new(TmContent::Packets, true, Some(2)).unwrap(),
        )],
    )
    .unwrap();
    let mut rx = TmRx::new(config);
    feed(&mut rx, &tm_frame(0, 0, 0, 0, false, &[1, 9], Some(CLCW)));
    let out = feed(&mut rx, &tm_frame(0, 2, 2, 0, false, &[1, 9], Some(CLCW)));
    assert_eq!(out.len(), 5);
    assert!(matches!(
        out[4],
        ReceiverOutput::DataField { gap: true, .. }
    ));

    // AOS: a rejected frame has VC count, Insert SDU, FrameDiscarded; a served one has four.
    let mut rx = aos();
    feed(&mut rx, &aos_frame(5, 0, None, false, None, &[]));
    let out = feed(&mut rx, &aos_frame(5, 2, None, false, None, &[]));
    assert_eq!(out.len(), 3);
    assert!(is_gap(&out[0], 1) && matches!(out[1], ReceiverOutput::InsertSdu { .. }));
    feed(&mut rx, &aos_frame(1, 0, None, false, Some(CLCW), &[0, 0]));
    let out = feed(&mut rx, &aos_frame(1, 2, None, false, Some(CLCW), &[0, 0]));
    assert_eq!(out.len(), 4);
    assert!(matches!(
        out[3],
        ReceiverOutput::DataField { gap: true, .. }
    ));

    // USLP: a known VC that is not served, with a gap: VC count, Insert SDU, OCF, FrameDiscarded.
    let mut rx = uslp();
    feed(
        &mut rx,
        &uslp_frame(3, 0, false, false, Some((1, 4)), Some(CLCW)),
    );
    let out = feed(
        &mut rx,
        &uslp_frame(3, 0, false, false, Some((1, 6)), Some(CLCW)),
    );
    assert_eq!(out.len(), 4);
    assert!(is_gap(&out[0], 1));
    assert!(matches!(
        out[3],
        ReceiverOutput::FrameDiscarded {
            reason: DiscardReason::NotServed,
            ..
        }
    ));
    // ... and a served one: gap, Insert SDU, OCF, field.
    feed(
        &mut rx,
        &uslp_frame(1, 0, false, false, Some((1, 4)), Some(CLCW)),
    );
    let out = feed(
        &mut rx,
        &uslp_frame(1, 0, false, false, Some((1, 6)), Some(CLCW)),
    );
    assert_eq!(out.len(), 4);

    // TC: one frame, one action.
    let mut rx = tc();
    assert_eq!(feed(&mut rx, &tc_frame(1)).len(), 1);
    assert_eq!(feed(&mut rx, &tc_frame(2)).len(), 1);
    assert_eq!(feed(&mut rx, &tc_frame(9)).len(), 1);
}

#[test]
fn the_other_inputs_return_at_most_one_action() {
    // req: LNK-074, LNK-053, LNK-080, LNK-003
    let mut rx = tm();
    let mut area = vec![0u8; rx.config().area_len()];
    let loss = rx.step(ReceiverInput::FrameLoss, &mut area).unwrap();
    assert_eq!(
        (loss.len(), loss.get(0)),
        (1, Some(&ReceiverOutput::LossFlag))
    );
    let discard = rx
        .step(
            ReceiverInput::CallerDiscard(CallerDiscardReason::Security),
            &mut area,
        )
        .unwrap();
    assert_eq!(
        discard.get(0),
        Some(&ReceiverOutput::FrameDiscarded {
            reason: DiscardReason::Security,
            vc: None,
            map: None
        })
    );
    let config = *rx.config();
    let reconf = rx
        .step(ReceiverInput::Reconfigure(config), &mut area)
        .unwrap();
    assert!(reconf.is_empty());
}

// ---- LNK-075 and API-058: the area and the refusals ----

#[test]
fn an_area_shorter_than_the_bound_fails_before_any_change() {
    // req: LNK-075, API-058
    let mut rx = uslp();
    let before = rx.clone();
    let mut area = vec![0u8; rx.config().area_len() - 1];
    let frame = uslp_frame(1, 0, false, false, Some((1, 4)), None);
    let e = rx
        .step(ReceiverInput::Frame(CheckedFrame::new(&frame)), &mut area)
        .unwrap_err();
    assert_eq!(e.kind(), ErrorKind::BufferTooSmall);
    assert_eq!(rx.status().refused.get(), 1);
    // The engine is as it was but for the refusal counter.
    let mut area = vec![0u8; rx.config().area_len()];
    let mut twin = before;
    let input = || ReceiverInput::Frame(CheckedFrame::new(&frame));
    assert_eq!(
        rx.step(input(), &mut area).unwrap(),
        twin.step(input(), &mut area).unwrap()
    );
}

#[test]
fn the_area_of_a_protocol_control_frame_holds_the_frame_and_the_insert_zone() {
    // req: LNK-075, LNK-081
    let mut rx = uslp();
    let mut area = vec![0u8; LEN + 2];
    let frame = uslp_frame(1, 0, true, true, Some((1, 4)), None);
    let out = rx
        .step(ReceiverInput::Frame(CheckedFrame::new(&frame)), &mut area)
        .unwrap();
    assert_eq!(out.len(), 2);
    assert!(matches!(
        out.get(1),
        Some(ReceiverOutput::ProtocolControl { frame: r, .. }) if r.end() == LEN + 2
    ));
}

#[test]
fn a_frame_of_another_length_is_refused() {
    // req: LNK-050, LNK-079, API-058; OI-78
    let mut rx = tm();
    let mut frame = tm_frame(0, 0, 0, 0, false, &[], None);
    frame.pop();
    let e = refused(&mut rx, &frame);
    assert_eq!(
        (e.kind(), e.field()),
        (ErrorKind::LengthMismatch, Some(field::FRAME_LEN))
    );
    frame.extend([0, 0]);
    assert_eq!(refused(&mut rx, &frame).kind(), ErrorKind::LengthMismatch);
    assert_eq!(rx.status().frames_in.get(), 0);
    assert_eq!(rx.status().refused.get(), 2);
}

#[test]
fn the_reserved_flag_combination_of_uslp_is_refused() {
    // req: LNK-050, API-058; OI-78 (732.1 Table 4-1)
    let mut rx = uslp();
    let e = refused(&mut rx, &uslp_frame(1, 0, false, true, Some((1, 4)), None));
    assert_eq!(
        (e.kind(), e.field()),
        (
            ErrorKind::ReservedValue,
            Some(field::PROTOCOL_CONTROL_COMMAND_FLAG)
        )
    );
}

#[test]
fn a_tm_frame_that_contradicts_its_vc_is_refused() {
    // req: LNK-050, API-058; 132.0 §4.1.2.4.3, §4.1.2.7.2.3
    let mut rx = tm();
    let e = refused(&mut rx, &tm_frame(0, 0, 0, 0, false, &[], Some(CLCW)));
    assert_eq!(
        (e.kind(), e.field()),
        (ErrorKind::InvalidField, Some(field::OCF_FLAG))
    );
    let e = refused(&mut rx, &tm_frame(0, 0, 0, 0, false, &[1, 9], None));
    assert_eq!(
        (e.kind(), e.field()),
        (ErrorKind::InvalidField, Some(field::FSH_FLAG))
    );
    let e = refused(&mut rx, &tm_frame(5, 0, 0, 0, true, &[2, 9, 9], Some(CLCW)));
    assert_eq!(
        (e.kind(), e.field()),
        (ErrorKind::LengthMismatch, Some(field::FSH_LEN))
    );
    // A frame whose Secondary Header leaves no data octet is malformed.
    let e = refused(&mut rx, &{
        let mut f = tm_frame(2, 0, 0, 0, false, &[], None);
        f[4] |= 0x80;
        f[6] = 0x3F;
        f
    });
    assert_eq!(
        (e.kind(), e.field()),
        (ErrorKind::Malformed, Some(field::FRAME_DATA_FIELD))
    );
    // None of them counted.
    assert_eq!(rx.status().frames_in.get(), 0);
    assert_eq!(
        feed(&mut rx, &tm_frame(0, 9, 9, 0, false, &[], None)).len(),
        1
    );
}

#[test]
fn a_bitstream_pointer_beyond_the_zone_is_refused_without_a_change() {
    // req: LNK-077, API-058; OI-78
    let mut rx = aos();
    feed(&mut rx, &aos_frame(3, 0, None, false, None, &[0x3F, 0xFF]));
    // The zone is 40 - 6 - 2 - 2 = 30 octets, 240 bits; a pointer of 300 is beyond it.
    let e = refused(&mut rx, &aos_frame(3, 1, None, false, None, &[0x01, 0x2C]));
    assert_eq!(
        (e.kind(), e.field()),
        (ErrorKind::Malformed, Some(field::BITSTREAM_DATA_POINTER))
    );
    // The count 1 was not taken: the next frame, count 1, is in sequence.
    let out = feed(&mut rx, &aos_frame(3, 1, None, false, None, &[0x3F, 0xFF]));
    assert_eq!(out.len(), 2);
}

#[test]
fn the_valid_bits_of_a_bpdu_are_delivered_and_the_idle_bits_dropped() {
    // req: LNK-077
    let mut rx = aos();
    let mut area = vec![0u8; rx.config().area_len()];
    for (pointer, bits, last) in [
        ([0x00, 0x13], 20, 0x77),
        ([0x3F, 0xFF], 240, 0x77),
        ([0x00, 0x00], 1, 0x77),
    ] {
        let frame = aos_frame(3, 0, None, false, None, &pointer);
        let out = rx
            .step(ReceiverInput::Frame(CheckedFrame::new(&frame)), &mut area)
            .unwrap();
        let Some(ReceiverOutput::DataField {
            data, bit_count, ..
        }) = out.get(1)
        else {
            panic!("{out:?}");
        };
        assert_eq!(*bit_count, Some(bits));
        let octets = data.slice(&area).unwrap();
        assert_eq!(octets.len(), bits.div_ceil(8));
        let mask = if bits % 8 == 0 {
            0xFF
        } else {
            0xFFu8 << (8 - bits % 8)
        };
        assert_eq!(octets.last().copied(), Some(last & mask));
        assert_eq!(octets.last().unwrap() & !mask, 0);
        // Count 0 repeats from here on: restart the expectation by reconfiguring.
        let config = *rx.config();
        rx.step(ReceiverInput::Reconfigure(config), &mut area)
            .unwrap();
    }
    // Every count of valid bits from 1 to 17 keeps the bits it names and zeroes the rest.
    for n in 1..=17usize {
        let pointer = u16::try_from(n - 1).unwrap().to_be_bytes();
        let frame = aos_frame(3, 0, None, false, None, &pointer);
        let config = *rx.config();
        rx.step(ReceiverInput::Reconfigure(config), &mut area)
            .unwrap();
        let out = rx
            .step(ReceiverInput::Frame(CheckedFrame::new(&frame)), &mut area)
            .unwrap();
        let Some(ReceiverOutput::DataField {
            data, bit_count, ..
        }) = out.get(1)
        else {
            panic!()
        };
        assert_eq!(*bit_count, Some(n));
        let octets = data.slice(&area).unwrap();
        let ones: u32 = octets.iter().map(|b| b.count_ones()).sum();
        assert!(ones <= u32::try_from(n).unwrap());
        assert_eq!(octets.len(), n.div_ceil(8));
    }
    let config = *rx.config();
    rx.step(ReceiverInput::Reconfigure(config), &mut area)
        .unwrap();
    // Only idle data: the Insert Zone is delivered, the B_PDU is dropped.
    let out = feed(&mut rx, &aos_frame(3, 0, None, false, None, &[0x3F, 0xFE]));
    assert!(matches!(out[..], [ReceiverOutput::InsertSdu { .. }]));
}

// ---- OI-78: the other decisions ----

#[test]
fn a_change_of_the_cycle_use_flag_is_a_discontinuity() {
    // req: LNK-051; OI-78
    let mut rx = aos();
    feed(&mut rx, &aos_frame(2, 7, None, false, None, &[]));
    let out = feed(&mut rx, &aos_frame(2, 8, Some(1), false, None, &[]));
    assert!(matches!(out[0], ReceiverOutput::CountDiscontinuity { .. }));
    assert!(matches!(
        out.last(),
        Some(ReceiverOutput::DataField { gap: true, .. })
    ));
    // The expectation is the new count plus one in the new mode (cycle 1, count 9).
    let out = feed(&mut rx, &aos_frame(2, 9, Some(1), false, None, &[]));
    assert!(matches!(
        out.last(),
        Some(ReceiverOutput::DataField { gap: false, .. })
    ));
    assert_eq!(rx.status().count_discontinuities.get(), 1);
}

#[test]
fn frame_loss_changes_no_count() {
    // req: LNK-053; OI-68, OI-78
    let mut rx = aos();
    feed(&mut rx, &aos_frame(2, 7, None, false, None, &[]));
    let mut area = vec![0u8; rx.config().area_len()];
    rx.step(ReceiverInput::FrameLoss, &mut area).unwrap();
    let out = feed(&mut rx, &aos_frame(2, 8, None, false, None, &[]));
    assert!(matches!(
        out.last(),
        Some(ReceiverOutput::DataField { gap: false, .. })
    ));
    assert_eq!(rx.status().frame_loss.get(), 1);
    let out = feed(&mut rx, &aos_frame(2, 10, None, false, None, &[]));
    assert!(is_gap(&out[0], 1));
}

#[test]
fn a_raw_frame_gives_its_insert_zone_at_the_fixed_offset() {
    // req: LNK-079; OI-78
    let raw = |rx: &mut AosRx, f: &[u8]| {
        let mut area = vec![0u8; rx.config().area_len()];
        let out = rx.step(ReceiverInput::RawFrame(f), &mut area)?;
        Ok::<_, aldis_core::Error>((out, area))
    };
    // AOS: the replay flag of a header the caller rejected is not passed on.
    let mut rx = AosRx::new(aos_config(false).with_raw_frames(true).unwrap());
    let (out, area) = raw(&mut rx, &aos_frame(1, 0, None, true, None, &[])).unwrap();
    let Some(ReceiverOutput::InsertSdu { sdu, replay }) = out.get(0) else {
        panic!()
    };
    assert_eq!(
        (sdu.slice(&area).unwrap(), *replay),
        (&[0xA1, 0xA2][..], None)
    );
    assert_eq!(rx.status().raw_frames_in.get(), 1);
    // ... after the FHEC when the channel has one.
    let mut rx = AosRx::new(aos_config(true).with_raw_frames(true).unwrap());
    let mut frame = aos_frame(1, 0, None, false, None, &[]);
    frame.splice(6..6, [0xEE, 0xEE]);
    frame.truncate(LEN);
    let (out, area) = raw(&mut rx, &frame).unwrap();
    let Some(ReceiverOutput::InsertSdu { sdu, .. }) = out.get(0) else {
        panic!()
    };
    assert_eq!(sdu.slice(&area).unwrap(), &[0xA1, 0xA2]);
    // USLP: the offset follows the VCF Count Length of the frame, which the caller did not
    // validate.
    for len in [0u8, 3] {
        let mut rx = UslpRx::new(uslp_config().with_raw_frames(true).unwrap());
        let frame = uslp_frame(1, 0, false, false, Some((len, 5)), None);
        let mut area = vec![0u8; rx.config().area_len()];
        let out = rx.step(ReceiverInput::RawFrame(&frame), &mut area).unwrap();
        let Some(ReceiverOutput::InsertSdu { sdu, replay }) = out.get(0) else {
            panic!()
        };
        assert_eq!(
            (sdu.slice(&area).unwrap(), *replay),
            (&[0xB1, 0xB2][..], None)
        );
    }
}

#[test]
fn a_raw_frame_is_refused_when_the_option_is_off_or_the_length_is_wrong() {
    // req: LNK-079; OI-73
    let mut area = vec![0u8; LEN + 2];
    let mut rx = aos();
    let frame = aos_frame(1, 0, None, false, None, &[]);
    let e = rx
        .step(ReceiverInput::RawFrame(&frame), &mut area)
        .unwrap_err();
    assert_eq!(
        (e.kind(), e.field()),
        (ErrorKind::InvalidConfig, Some(field::RAW_FRAMES))
    );
    let mut rx = AosRx::new(aos_config(false).with_raw_frames(true).unwrap());
    let e = rx
        .step(ReceiverInput::RawFrame(&frame[1..]), &mut area)
        .unwrap_err();
    assert_eq!(
        (e.kind(), e.field()),
        (ErrorKind::LengthMismatch, Some(field::FRAME_LEN))
    );
    // TM and TC have no Insert Zone: the option stays off.
    let mut rx = tm();
    let e = rx
        .step(ReceiverInput::RawFrame(&frame), &mut area)
        .unwrap_err();
    assert_eq!(e.field(), Some(field::RAW_FRAMES));
    assert_eq!(rx.status().refused.get(), 1);
}

#[test]
fn a_channel_without_an_insert_zone_delivers_none() {
    // req: LNK-053
    let config = FrameReceiverConfig::<Aos, 2>::new(
        LEN,
        false,
        AosChannel::new(0, false),
        &[VcEntry::new(vc(1), AosVc::new(AosContent::Vca, false))],
    )
    .unwrap();
    let mut rx = FrameReceiver::<Aos, 2>::new(config);
    let out = feed(&mut rx, &aos_frame(1, 0, None, false, None, &[]));
    assert!(matches!(out[..], [ReceiverOutput::DataField { .. }]));
    assert!(config.with_raw_frames(true).is_err());
    // Only a protocol with an Insert Zone has an offset for a raw frame.
    assert_eq!(
        Tm::raw_insert_zone_offset(&TmChannel::new(Service::None, Service::None), &[]),
        None
    );
    assert_eq!(Tc::raw_insert_zone_offset(&TcChannel, &[]), None);
    assert_eq!(
        Tm::insert_zone_len(&TmChannel::new(Service::None, Service::None)),
        0
    );
    assert_eq!(Tc::insert_zone_len(&TcChannel), 0);
}

// ---- LNK-050…LNK-054 beyond the vectors ----

#[test]
fn the_replay_flag_is_passed_on_every_delivered_part() {
    // req: LNK-054
    let mut rx = aos();
    let out = feed(&mut rx, &aos_frame(1, 0, None, true, Some(CLCW), &[0, 0]));
    assert_eq!(out.len(), 3);
    for o in &out {
        let replay = match o {
            ReceiverOutput::InsertSdu { replay, .. }
            | ReceiverOutput::Ocf { replay, .. }
            | ReceiverOutput::DataField { replay, .. } => *replay,
            other => panic!("{other:?}"),
        };
        assert_eq!(replay, Some(true));
    }
}

#[test]
fn the_ocf_is_classified_and_delivered_for_its_scope() {
    // req: LNK-053
    let mut rx = TmRx::new(tm_config(Service::Mc, Service::None));
    let out = feed(&mut rx, &tm_frame(5, 0, 0, 0, true, &[1, 9], Some(REPORT2)));
    // The FSH is not delivered (no service); the OCF is a Type-2 Report of the master channel.
    assert!(matches!(
        out[..],
        [
            ReceiverOutput::Ocf {
                scope: ChannelScope::MasterChannel,
                ocf: Ocf::ProjectSpecific(_),
                ..
            },
            ReceiverOutput::FrameDiscarded { .. }
        ]
    ));
    let out = feed(&mut rx, &tm_frame(5, 1, 1, 0, true, &[1, 9], Some(CLCW)));
    assert!(matches!(
        out[0],
        ReceiverOutput::Ocf {
            ocf: Ocf::Clcw(_),
            ..
        }
    ));
}

#[test]
fn a_gap_flag_stays_until_a_data_field_is_delivered() {
    // req: LNK-051, LNK-061
    let mut rx = tm();
    feed(&mut rx, &tm_frame(1, 0, 0, 0, true, &[], None));
    let out = feed(&mut rx, &tm_frame(1, 1, 3, 0, true, &[], None));
    assert!(matches!(
        out[..],
        [
            ReceiverOutput::FrameGap { .. },
            ReceiverOutput::DataField { gap: true, .. }
        ]
    ));
    let out = feed(&mut rx, &tm_frame(1, 2, 4, 0, true, &[], None));
    assert!(matches!(
        out.last(),
        Some(ReceiverOutput::DataField { gap: false, .. })
    ));
    // A gap on a VC that is not served flags nothing.
    feed(&mut rx, &tm_frame(5, 3, 0, 0, true, &[1, 9], Some(CLCW)));
    feed(&mut rx, &tm_frame(5, 4, 5, 0, true, &[1, 9], Some(CLCW)));
    assert_eq!(rx.status().gaps.get(), 2);
}

#[test]
fn an_oid_frame_of_uslp_and_aos_is_counted_and_untracked() {
    // req: LNK-052
    let mut rx = uslp();
    let out = feed(
        &mut rx,
        &uslp_frame(63, 0, false, false, Some((1, 9)), Some(CLCW)),
    );
    assert!(matches!(
        out[..],
        [ReceiverOutput::InsertSdu { .. }, ReceiverOutput::Ocf { .. }]
    ));
    assert_eq!(
        feed(
            &mut rx,
            &uslp_frame(63, 0, false, false, Some((1, 2)), None)
        )
        .len(),
        1
    );
    assert_eq!(rx.status().oid_frames.get(), 2);
    assert_eq!(
        rx.status().count_discontinuities.get() + rx.status().gaps.get(),
        0
    );
    let mut rx = aos();
    feed(&mut rx, &aos_frame(63, 9, None, false, None, &[]));
    feed(&mut rx, &aos_frame(63, 2, None, false, None, &[]));
    assert_eq!(rx.status().oid_frames.get(), 2);
}

#[test]
fn a_tc_frame_is_delivered_whole() {
    // req: LNK-081
    let mut rx = tc();
    let frame = tc_frame(1);
    let mut area = vec![0u8; LEN];
    let out = rx
        .step(ReceiverInput::Frame(CheckedFrame::new(&frame)), &mut area)
        .unwrap();
    let Some(ReceiverOutput::TcFrame { vc: v, frame: r }) = out.get(0) else {
        panic!()
    };
    assert_eq!((u8::from(*v), r.slice(&area).unwrap()), (1, &frame[..]));
    assert_eq!(*r, OutRange::new(0, LEN));
}

// ---- LNK-002, LNK-003, API-056: housekeeping ----

#[test]
fn counters_count_and_persist_across_a_reconfiguration() {
    // req: LNK-002, LNK-003, LNK-050, LNK-080, API-056
    let mut rx = tm();
    feed(&mut rx, &tm_frame(0, 0, 0, 0, false, &[], None));
    feed(&mut rx, &tm_frame(4, 1, 0, 0, false, &[], None));
    feed(&mut rx, &tm_frame(5, 2, 0, 0, true, &[1, 9], Some(CLCW)));
    let mut area = vec![0u8; rx.config().area_len()];
    for reason in [
        CallerDiscardReason::CrcMismatch,
        CallerDiscardReason::HeaderUncorrectable,
        CallerDiscardReason::Security,
    ] {
        rx.step(ReceiverInput::CallerDiscard(reason), &mut area)
            .unwrap();
    }
    let s = *rx.status();
    assert_eq!(
        (
            s.frames_in.get(),
            s.unknown_channel.get(),
            s.not_served.get()
        ),
        (3, 1, 1)
    );
    assert_eq!(
        (
            s.crc_mismatch.get(),
            s.header_uncorrectable.get(),
            s.security.get()
        ),
        (1, 1, 1)
    );
    // The expected counts restart; the counters stay.
    let config = *rx.config();
    rx.step(ReceiverInput::Reconfigure(config), &mut area)
        .unwrap();
    let out = feed(&mut rx, &tm_frame(0, 200, 77, 0, false, &[], None));
    assert_eq!(out.len(), 1);
    assert_eq!(rx.status().frames_in.get(), 4);
    assert_eq!(rx.status().unknown_channel.get(), 1);
    // Reconfiguring to another channel serves it at once.
    let other = FrameReceiverConfig::<Tm, 8>::new(
        LEN,
        false,
        TmChannel::new(Service::None, Service::None),
        &[VcEntry::new(
            vc(4),
            TmVc::new(TmContent::Vca, false, None).unwrap(),
        )],
    )
    .unwrap();
    rx.step(ReceiverInput::Reconfigure(other), &mut area)
        .unwrap();
    assert_eq!(rx.vcs().map(u8::from).collect::<Vec<_>>(), [4]);
    assert!(matches!(
        feed(&mut rx, &tm_frame(4, 0, 0, 0, true, &[], None))[..],
        [ReceiverOutput::DataField { .. }]
    ));
}

// ---- LNK-078-style configuration errors ----

#[test]
fn a_configuration_is_checked_where_it_is_built() {
    // req: API-040, RS-020, LNK-079
    let tm_vc = TmVc::new(TmContent::Vca, false, None).unwrap();
    let channel = TmChannel::new(Service::None, Service::None);
    let one = [VcEntry::<Tm>::new(vc(1), tm_vc)];
    let e = FrameReceiverConfig::<Tm, 8>::new(7, true, channel, &one).unwrap_err();
    assert_eq!(
        (e.kind(), e.field()),
        (ErrorKind::InvalidConfig, Some(field::FRAME_LEN))
    );
    assert!(FrameReceiverConfig::<Tm, 8>::new(9, true, channel, &one).is_ok());
    let twice = [one[0], one[0]];
    let e = FrameReceiverConfig::<Tm, 8>::new(LEN, false, channel, &twice).unwrap_err();
    assert_eq!(
        (e.kind(), e.field()),
        (ErrorKind::InvalidConfig, Some(field::VC_ID))
    );
    let e = FrameReceiverConfig::<Tm, 1>::new(
        LEN,
        false,
        channel,
        &[one[0], VcEntry::new(vc(2), tm_vc)],
    )
    .unwrap_err();
    assert_eq!(
        (e.kind(), e.field()),
        (ErrorKind::CapacityExceeded, Some(field::VC_ID))
    );
    // A VC whose OCF and Secondary Header do not fit.
    let wide = TmVc::new(TmContent::Vca, true, Some(64)).unwrap();
    let e = FrameReceiverConfig::<Tm, 8>::new(60, false, channel, &[VcEntry::new(vc(1), wide)])
        .unwrap_err();
    assert_eq!(e.field(), Some(field::FRAME_LEN));
    for bad in [1, 65] {
        let e = TmVc::new(TmContent::Vca, false, Some(bad)).unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::InvalidConfig, Some(field::FSH_LEN))
        );
    }
    let v = TmVc::new(TmContent::Packets, true, Some(64)).unwrap();
    assert_eq!(
        (v.content(), v.ocf(), v.fsh_len()),
        (TmContent::Packets, true, Some(64))
    );
    // Raw frames need an Insert Zone.
    let e = tm_config(Service::Vc, Service::Vc)
        .with_raw_frames(true)
        .unwrap_err();
    assert_eq!(
        (e.kind(), e.field()),
        (ErrorKind::InvalidConfig, Some(field::RAW_FRAMES))
    );
    assert!(
        tm_config(Service::Vc, Service::Vc)
            .with_raw_frames(false)
            .is_ok()
    );
    // Accessors.
    let config = aos_config(true).with_raw_frames(true).unwrap();
    assert!(config.raw_frames() && !config.fecf());
    assert_eq!((config.frame_len(), config.area_len()), (LEN, LEN + 2));
    assert_eq!(
        (config.channel().insert_zone_len(), config.channel().fhec()),
        (2, true)
    );
    assert_eq!(config.vcs().count(), 4);
    let entry = config.vcs().next().unwrap();
    assert_eq!(u8::from(entry.vc()), 1);
    assert_eq!(
        (entry.params().content(), entry.params().ocf()),
        (AosContent::Packets, true)
    );
    let channel = TmChannel::new(Service::Mc, Service::Vc);
    assert_eq!((channel.ocf(), channel.fsh()), (Service::Mc, Service::Vc));
    let channel = UslpChannel::new(3, Service::Vc);
    assert_eq!((channel.insert_zone_len(), channel.ocf()), (3, Service::Vc));
    let p = UslpVc::new(MapSet::new(&[MapId::<4>::new(2).unwrap()]), true, false);
    assert!(p.cop() && !p.served() && p.maps().contains(MapId::<4>::new(2).unwrap()));
    assert!(!p.maps().contains(MapId::<4>::new(3).unwrap()));
    assert!(TcVc::SERVED.served() && !TcVc::UNSERVED.served());
    assert!(!MapSet::default().contains(MapId::<4>::new(0).unwrap()));
}

// ---- API-058, API-053, GEN-020: any input, no panic, no InternalError ----

fn check_out<P: ReceiveProtocol, const V: usize>(
    rx: &mut FrameReceiver<P, V>,
    frame: &[u8],
) -> Result<(), TestCaseError> {
    let before = rx.clone();
    let mut area = vec![0u8; rx.config().area_len()];
    match rx.step(ReceiverInput::Frame(CheckedFrame::new(frame)), &mut area) {
        Ok(actions) => {
            prop_assert!(actions.len() <= FRAME_RECEIVER_MAX_ACTIONS);
            for o in actions.iter() {
                let range = match o {
                    ReceiverOutput::DataField { data, .. } => Some(*data),
                    ReceiverOutput::FshSdu { sdu, .. } | ReceiverOutput::InsertSdu { sdu, .. } => {
                        Some(*sdu)
                    }
                    ReceiverOutput::TcFrame { frame, .. }
                    | ReceiverOutput::ProtocolControl { frame, .. } => Some(*frame),
                    _ => None,
                };
                if let Some(r) = range {
                    prop_assert!(r.end() <= area.len());
                }
            }
        }
        Err(e) => {
            prop_assert_ne!(e.kind(), ErrorKind::InternalError);
            // The refusal leaves the engine as it was but for its counter: both take the same
            // next frames to the same actions.
            let mut twin = before;
            let next = tm_like(frame);
            let a = twin.step(ReceiverInput::Frame(CheckedFrame::new(&next)), &mut area);
            let b = rx.step(ReceiverInput::Frame(CheckedFrame::new(&next)), &mut area);
            prop_assert_eq!(a.is_ok(), b.is_ok());
            if let (Ok(a), Ok(b)) = (a, b) {
                prop_assert_eq!(a, b);
            }
        }
    }
    Ok(())
}

/// A frame that is acceptable to every protocol's header reading, of the same length.
fn tm_like(frame: &[u8]) -> Vec<u8> {
    let mut f = frame.to_vec();
    if let Some(b) = f.get_mut(4) {
        *b &= 0x3F;
    }
    f
}

fn frames() -> impl Strategy<Value = Vec<Vec<u8>>> {
    // Frames of the channel length from random octets, biased toward the VCs and flags the
    // receivers are configured for.
    let frame = (
        prop::collection::vec(any::<u8>(), LEN),
        0u8..=7,
        any::<bool>(),
    )
        .prop_map(|(mut f, vc, bias)| {
            if bias {
                f[1] = (f[1] & 0xC0) | vc;
            }
            f
        });
    prop::collection::vec(frame, 1..12)
}

proptest! {
    #![proptest_config(ProptestConfig {
        // A fixed seed keeps the coverage of the library the same from run to run (VER-024).
        rng_seed: RngSeed::Fixed(0x00F2_A3E5),
        ..strategies::config(256, REGRESSIONS)
    })]

    // req: LNK-074, LNK-075, API-057, API-058, GEN-020
    #[test]
    fn no_frame_makes_a_receiver_fail_internally(fs in frames()) {
        let mut tmr = tm();
        let mut aosr = aos();
        let mut uslpr = uslp();
        let mut tcr = tc();
        for f in &fs {
            check_out(&mut tmr, f)?;
            check_out(&mut aosr, f)?;
            check_out(&mut uslpr, f)?;
            check_out(&mut tcr, f)?;
            // The same octets as a USLP frame with random flags.
            let mut g = f.clone();
            g[0] = 0xC0 | (g[0] & 0x0F);
            check_out(&mut uslpr, &g)?;
        }
    }
}

#[test]
fn a_gap_and_a_discontinuity_never_come_together_for_one_counter() {
    // req: LNK-051, LNK-074 (the bound argument: a counter yields one action)
    let mut rx = tm();
    feed(&mut rx, &tm_frame(0, 10, 10, 0, false, &[], None));
    let out = feed(&mut rx, &tm_frame(0, 10, 10, 0, false, &[], None));
    assert_eq!(
        out.iter()
            .filter(|o| matches!(o, ReceiverOutput::CountDiscontinuity { .. }))
            .count(),
        2
    );
    assert_eq!(out.len(), 3);
}

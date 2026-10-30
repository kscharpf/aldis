//! FOP-1 with the USLP frame target: Type-AD, Type-BD, and Type-BC (protocol control) frames of
//! a USLP Virtual Channel and MAP (COP-010, USLP-013), decoded with the USLP codec and the TC
//! control-command parser, and a closed loop with FARM-1 over such frames.
#![cfg(feature = "uslp")]
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test-only harness: a violated assumption must fail loudly (RS-018)"
)]

use aldis_cop1::{
    Directive, Farm1, Farm1Config, Farm1Frame, Farm1Input, Farm1Output, Farm1Window, Fop1,
    Fop1Channel, Fop1Config, Fop1Input, Fop1Output, Fop1State, Fop1Target, FrameKind, TimeoutType,
    TransferNotification, fop1_max_actions,
};
use aldis_core::ErrorKind;
use aldis_core::ids::FrameSeqNum;
use aldis_core::security::SecurityLayout;
use aldis_core::time::{Duration, Instant};
use aldis_tc::{Clcw, ClcwStatus, ControlCommand, CopInEffect, FarmBCounter, TcVcId};
use aldis_uslp::{
    FirstHeaderPointer, FrameKind as UslpKind, MapConfig, OcfUse, PhysicalChannelConfig,
    SecurityScope, TfdfHeader, TfdzRule, TransferFrameType, TruncatedFrameLen, Upid, UslpFrame,
    UslpMapId, UslpSpacecraftId, UslpVcId, VcConfig, VcParams, VcTable, VcfCountLen,
};
use core::num::NonZeroU8;

const Q: usize = 4;
const F: usize = 64;
const N: usize = fop1_max_actions(Q);
type Engine = Fop1<Q, F, N>;
const NOW: Instant = Instant::from_micros(0);

const fn scid() -> UslpSpacecraftId {
    UslpSpacecraftId::new(0x155).unwrap()
}

const fn vcid() -> UslpVcId {
    UslpVcId::new(5).unwrap()
}

const fn mapid() -> UslpMapId {
    UslpMapId::new(2).unwrap()
}

fn physical(fecf: bool) -> PhysicalChannelConfig {
    PhysicalChannelConfig::new(TransferFrameType::Variable, F, 0, fecf).unwrap()
}

/// The parameters a VC must have for the target: variable-length, a one-octet sequence-controlled
/// VCF Count, no expedited count, no OCF, no truncated frames, no SDLS.
fn params() -> VcParams {
    VcParams {
        vcf_count_len_sequence: VcfCountLen::new(1).unwrap(),
        ..VcParams::new(TransferFrameType::Variable)
    }
}

fn vc(params: VcParams) -> VcConfig {
    VcConfig::new(scid(), vcid(), params).unwrap()
}

const fn map() -> MapConfig {
    MapConfig::new(scid(), vcid(), mapid(), SecurityLayout::NONE)
}

const TFDF: TfdfHeader = TfdfHeader {
    rule: TfdzRule::NoSegmentation,
    upid: Upid::OnlyIdleData,
};

fn target(fecf: bool) -> Fop1Target {
    Fop1Target::uslp(physical(fecf), vc(params()), map(), TFDF).unwrap()
}

/// The channel the flight end decodes with: the one the target is built from.
fn channels(fecf: bool) -> VcTable<1, 1> {
    VcTable::new(physical(fecf), &[vc(params())], &[map()]).unwrap()
}

fn engine(fecf: bool) -> Engine {
    Engine::new(Fop1Config {
        target: target(fecf),
        t1_initial: Duration::from_micros(1_000),
        transmission_limit: NonZeroU8::new(3).unwrap(),
        timeout_type: TimeoutType::Tt0,
        sliding_window_k: 2,
        ad_repetitions: 2,
        bc_repetitions: 3,
    })
    .unwrap()
}

fn clcw(n_r: u8) -> Clcw {
    Clcw {
        status: ClcwStatus::new(0).unwrap(),
        cop_in_effect: CopInEffect::COP_1,
        vc_id: TcVcId::new(vcid().get()).unwrap(),
        no_rf_available: false,
        no_bit_lock: false,
        lockout: false,
        wait: false,
        retransmit: false,
        farm_b_counter: FarmBCounter::default(),
        report_value: FrameSeqNum::new(n_r),
    }
}

/// Steps `f` and returns the next engine, the frame it transmits (if any), and the actions.
fn go(f: &Engine, input: Fop1Input<'_>) -> (Engine, Option<Vec<u8>>, Vec<Fop1Output>) {
    let mut out = [0u8; F];
    let (next, actions) = f.step(input, NOW, &mut out).unwrap();
    let actions: Vec<Fop1Output> = actions.iter().copied().collect();
    let frame = actions.iter().find_map(|a| match a {
        Fop1Output::Transmit { frame, .. } => Some(frame.slice(&out).unwrap().to_vec()),
        _ => None,
    });
    (next, frame, actions)
}

const fn directive(d: Directive) -> Fop1Input<'static> {
    Fop1Input::Directive {
        handle: 1,
        directive: d,
    }
}

/// An engine in State S1 (Active), by Initiate AD without a CLCW Check.
fn active() -> Engine {
    let (f, frame, _) = go(
        &engine(true),
        directive(Directive::InitiateAdWithoutClcwCheck),
    );
    assert!(frame.is_none());
    assert_eq!(f.state(), Fop1State::Active);
    f
}

#[test]
fn the_target_names_its_channel_and_sizes_its_frames() {
    // req: COP-010, API-040
    let t = target(true);
    assert_eq!(
        t.channel(),
        Fop1Channel::Uslp {
            spacecraft_id: scid(),
            vc_id: vcid(),
            map_id: mapid()
        }
    );
    assert_eq!(t.max_frame_len(), F);
    // The Primary Header (7), the VCF Count (1), the TFDF Header (1), and the FECF (2).
    assert_eq!(t.max_fdu_len(), F - 7 - 1 - 1 - 2);
    assert_eq!(target(false).max_fdu_len(), F - 7 - 1 - 1);
    // The longest FDU fills the longest frame exactly; one more does not fit.
    let f = active();
    let fdu = vec![0xAB; t.max_fdu_len()];
    let (g, frame, _) = go(
        &f,
        Fop1Input::RequestAd {
            handle: 7,
            fdu: &fdu,
        },
    );
    assert_eq!(frame.unwrap().len(), F);
    assert_eq!(g.sent_queue_len(), 1);
    let long = vec![0xAB; t.max_fdu_len() + 1];
    let mut out = [0u8; F];
    let e = f
        .step(
            Fop1Input::RequestAd {
                handle: 7,
                fdu: &long,
            },
            NOW,
            &mut out,
        )
        .unwrap_err();
    assert_eq!(e.kind(), ErrorKind::OutOfRange);
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one straight list of the refusals of the constructor, in its order"
)]
fn the_target_refuses_what_fop1_cannot_fill() {
    // req: COP-010, API-040, RS-020
    use aldis_core::field;
    let try_target = |physical, vc, map, tfdf| {
        Fop1Target::uslp(physical, vc, map, tfdf)
            .map(|_| ())
            .map_err(|e| (e.kind(), e.field()))
    };
    let bad = |fld| Err((ErrorKind::InvalidConfig, Some(fld)));
    let fixed_physical = PhysicalChannelConfig::new(TransferFrameType::Fixed, F, 0, true).unwrap();
    assert_eq!(
        try_target(fixed_physical, vc(params()), map(), TFDF),
        bad(field::TRANSFER_FRAME_TYPE)
    );
    let fixed_channel = vc(VcParams {
        ocf: OcfUse::Absent,
        ..VcParams::new(TransferFrameType::Fixed)
    });
    assert_eq!(
        try_target(physical(true), fixed_channel, map(), TFDF),
        bad(field::TRANSFER_FRAME_TYPE)
    );
    // An Insert Zone exists only on a fixed-length channel, which is refused whole.
    let insert = PhysicalChannelConfig::new(TransferFrameType::Fixed, F, 2, true).unwrap();
    assert_eq!(
        try_target(insert, vc(params()), map(), TFDF),
        bad(field::TRANSFER_FRAME_TYPE)
    );
    let truncated = vc(VcParams {
        truncated_frame_len: Some(TruncatedFrameLen::new(12).unwrap()),
        ..params()
    });
    assert_eq!(
        try_target(physical(true), truncated, map(), TFDF),
        bad(field::TRUNCATED_FRAME_LEN)
    );
    for p in [
        VcParams {
            vcf_count_len_sequence: VcfCountLen::new(2).unwrap(),
            ..params()
        },
        VcParams {
            vcf_count_len_sequence: VcfCountLen::NONE,
            ..params()
        },
        VcParams {
            vcf_count_len_expedited: VcfCountLen::new(1).unwrap(),
            ..params()
        },
    ] {
        assert_eq!(
            try_target(physical(true), vc(p), map(), TFDF),
            bad(field::VCF_COUNT_LEN)
        );
    }
    let sdls = SecurityLayout::new(6, 0).unwrap();
    let vc_sdls = vc(VcParams {
        security: SecurityScope::Vc(sdls),
        ..params()
    });
    assert_eq!(
        try_target(physical(true), vc_sdls, map(), TFDF),
        bad(field::SECURITY_HEADER_LEN)
    );
    let per_map = vc(VcParams {
        security: SecurityScope::PerMap,
        ..params()
    });
    let map_sdls = MapConfig::new(scid(), vcid(), mapid(), sdls);
    assert_eq!(
        try_target(physical(true), per_map, map_sdls, TFDF),
        bad(field::SECURITY_HEADER_LEN)
    );
    // A VC with SDLS lengths per MAP is accepted when this MAP has none.
    assert_eq!(try_target(physical(true), per_map, map(), TFDF), Ok(()));
    for rule in [
        TfdzRule::PacketsSpanningFrames(FirstHeaderPointer::from_raw(0)),
        TfdzRule::SduStart(aldis_uslp::LastValidOctet::from_raw(0)),
        TfdzRule::SduContinuation(aldis_uslp::LastValidOctet::from_raw(0)),
    ] {
        assert_eq!(
            try_target(
                physical(true),
                vc(params()),
                map(),
                TfdfHeader { rule, ..TFDF }
            ),
            bad(field::TFDZ_CONSTRUCTION_RULE)
        );
    }
    let short = PhysicalChannelConfig::new(TransferFrameType::Variable, 13, 0, true).unwrap();
    assert_eq!(
        try_target(short, vc(params()), map(), TFDF),
        bad(field::MAX_FRAME_LEN)
    );
    // The shortest frame that holds Set V(R): 7 + 1 + 1 + 3 octets without an FECF.
    let exact = PhysicalChannelConfig::new(TransferFrameType::Variable, 12, 0, false).unwrap();
    assert_eq!(try_target(exact, vc(params()), map(), TFDF), Ok(()));
    // A MAP of another VC is refused by the channel table.
    let other = MapConfig::new(
        scid(),
        UslpVcId::new(6).unwrap(),
        mapid(),
        SecurityLayout::NONE,
    );
    assert_eq!(
        try_target(physical(true), vc(params()), other, TFDF),
        bad(field::MAP_ID)
    );
    for rule in [
        TfdzRule::OctetStream,
        TfdzRule::StartingSegment,
        TfdzRule::ContinuingSegment,
        TfdzRule::LastSegment,
        TfdzRule::NoSegmentation,
    ] {
        assert_eq!(
            try_target(
                physical(true),
                vc(params()),
                map(),
                TfdfHeader { rule, ..TFDF }
            ),
            Ok(())
        );
    }
}

#[test]
fn type_ad_and_type_bd_frames_carry_the_fdu_as_the_tfdz() {
    // req: COP-010, COP-011, COP-016
    let t = channels(true);
    let f = active();
    let fdu = [0xDE, 0xAD, 0xBE, 0xEF];
    let (f, frame, actions) = go(
        &f,
        Fop1Input::RequestAd {
            handle: 5,
            fdu: &fdu,
        },
    );
    let frame = frame.unwrap();
    let ad = UslpFrame::decode(&frame, &t).unwrap();
    let full = ad.full().unwrap();
    assert_eq!(full.kind(), UslpKind::SequenceControlled);
    assert_eq!(full.vcf_count().value(), 0);
    assert_eq!(
        (ad.vc_id(), ad.map_id(), ad.spacecraft_id()),
        (vcid(), mapid(), scid())
    );
    assert_eq!((ad.tfdz(), ad.tfdf_header()), (&fdu[..], TFDF));
    assert!(matches!(
        actions.last(),
        Some(Fop1Output::Transmit {
            kind: FrameKind::Ad,
            retransmission: false,
            repetitions: 2,
            ..
        })
    ));
    // The next Type-AD frame has N(S) = 1 in the VCF Count.
    let (f, frame, _) = go(
        &f,
        Fop1Input::TransmitDone {
            kind: FrameKind::Ad,
            accepted: true,
        },
    );
    assert!(frame.is_none());
    let (f, frame, _) = go(
        &f,
        Fop1Input::RequestAd {
            handle: 6,
            fdu: &fdu,
        },
    );
    let frame = frame.unwrap();
    let second = UslpFrame::decode(&frame, &t).unwrap();
    assert_eq!(second.full().unwrap().vcf_count().value(), 1);
    // The retransmission is the master copy, octet for octet.
    let (f, frame, _) = go(
        &f,
        Fop1Input::TransmitDone {
            kind: FrameKind::Ad,
            accepted: true,
        },
    );
    assert!(frame.is_none());
    let retransmit = Clcw {
        retransmit: true,
        ..clcw(0)
    };
    let (_, again, _) = go(&f, Fop1Input::Clcw(retransmit));
    assert_eq!(again.as_deref(), Some(&frame_of(&ad)[..]));
    // A Type-BD frame is expedited user data, with no VCF Count.
    let (_, frame, _) = go(
        &active(),
        Fop1Input::RequestBd {
            handle: 9,
            fdu: &fdu,
        },
    );
    let frame = frame.unwrap();
    let bd = UslpFrame::decode(&frame, &t).unwrap();
    assert_eq!(bd.full().unwrap().kind(), UslpKind::Expedited);
    assert_eq!(
        (bd.tfdz(), bd.full().unwrap().vcf_count().value()),
        (&fdu[..], 0)
    );
}

fn frame_of(f: &UslpFrame<'_>) -> Vec<u8> {
    f.octets().to_vec()
}

#[test]
fn type_bc_frames_carry_the_control_commands_as_protocol_control() {
    // req: COP-010, USLP-013, TC-005
    let t = channels(true);
    let (f, frame, _) = go(&engine(true), directive(Directive::InitiateAdWithUnlock));
    assert_eq!(f.state(), Fop1State::InitializingWithBcFrame);
    let frame = frame.unwrap();
    let bc = UslpFrame::decode(&frame, &t).unwrap();
    assert_eq!(bc.full().unwrap().kind(), UslpKind::ProtocolControl);
    assert_eq!(bc.full().unwrap().vcf_count().value(), 0);
    assert_eq!(bc.tfdf_header().upid, Upid::Cop1ControlCommands);
    assert_eq!(bc.tfdf_header().rule, TfdzRule::NoSegmentation);
    assert_eq!(
        ControlCommand::parse(bc.tfdz()),
        Some(ControlCommand::Unlock)
    );

    let set_vr = Fop1Input::Directive {
        handle: 2,
        directive: Directive::InitiateAdWithSetVr {
            v_r: FrameSeqNum::new(0x17),
        },
    };
    let (g, frame, _) = go(&engine(true), set_vr);
    assert_eq!(g.state(), Fop1State::InitializingWithBcFrame);
    let frame = frame.unwrap();
    let bc = UslpFrame::decode(&frame, &t).unwrap();
    assert_eq!(
        ControlCommand::parse(bc.tfdz()),
        Some(ControlCommand::SetVr(FrameSeqNum::new(0x17)))
    );
    assert_eq!(bc.full().unwrap().kind(), UslpKind::ProtocolControl);

    // The Type-BC frame is a master copy: it is retransmitted unchanged when the timer expires.
    let (g, _, _) = go(
        &g,
        Fop1Input::TransmitDone {
            kind: FrameKind::Bc,
            accepted: true,
        },
    );
    let mut out = [0u8; F];
    let late = Instant::from_micros(1_000);
    let (_, actions) = g.step(Fop1Input::Timeout, late, &mut out).unwrap();
    let again = actions
        .iter()
        .find_map(|a| match a {
            Fop1Output::Transmit { frame, .. } => frame.slice(&out),
            _ => None,
        })
        .unwrap();
    assert_eq!(again, &frame[..]);
}

#[test]
fn a_clcw_of_another_virtual_channel_is_refused() {
    // req: COP-010, COP-012
    let f = active();
    let mut out = [0u8; F];
    let other = Clcw {
        vc_id: TcVcId::new(vcid().get() + 1).unwrap(),
        ..clcw(0)
    };
    let e = f.step(Fop1Input::Clcw(other), NOW, &mut out).unwrap_err();
    assert_eq!(e.kind(), ErrorKind::UnknownChannel);
    assert!(f.step(Fop1Input::Clcw(clcw(0)), NOW, &mut out).is_ok());
}

/// The frame FARM-1 reads from a USLP frame: the TFDZ, and N(S) from the VCF Count.
fn farm_frame<'a>(frame: &UslpFrame<'a>) -> Farm1Frame<'a> {
    let full = frame.full().unwrap();
    match full.kind() {
        UslpKind::SequenceControlled => Farm1Frame::Ad {
            sequence_number: FrameSeqNum::new(u8::try_from(full.vcf_count().value()).unwrap()),
            data: frame.tfdz(),
        },
        UslpKind::Expedited => Farm1Frame::Bd { data: frame.tfdz() },
        UslpKind::ProtocolControl => Farm1Frame::Bc {
            command: ControlCommand::parse(frame.tfdz()),
        },
    }
}

/// Feeds `input` to the ground end, then the Transmit Responses its frames cause; the frames go to
/// `link` and the Positive Confirms to `confirmed`.
fn feed(
    ground: &mut Engine,
    link: &mut Vec<Vec<u8>>,
    confirmed: &mut Vec<u32>,
    input: Fop1Input<'_>,
) {
    let mut work = vec![input];
    // bound: each response transmits at most one frame, and K and Q bound the chain
    while let Some(input) = work.pop() {
        let mut area = [0u8; F];
        let (next, actions) = ground.step(input, NOW, &mut area).unwrap();
        *ground = next;
        for a in actions.iter() {
            match *a {
                Fop1Output::Transmit { kind, frame, .. } => {
                    link.push(frame.slice(&area).unwrap().to_vec());
                    work.push(Fop1Input::TransmitDone {
                        kind,
                        accepted: true,
                    });
                }
                Fop1Output::Transfer(n) if n.status == TransferNotification::PositiveConfirm => {
                    confirmed.push(n.handle);
                }
                Fop1Output::Alert(a) => panic!("Alert {a:?}"),
                _ => {}
            }
        }
    }
}

#[test]
fn fop1_and_farm1_deliver_over_uslp_frames_through_a_loss() {
    // req: COP-010, COP-040, USLP-013
    let t = channels(true);
    let mut farm = Farm1::new(
        Farm1Config {
            vc_id: TcVcId::new(vcid().get()).unwrap(),
            status: ClcwStatus::new(0).unwrap(),
            window: Farm1Window::retransmission_allowed(8).unwrap(),
        },
        FrameSeqNum::new(0),
    );
    let mut ground = engine(true);
    let mut delivered: Vec<Vec<u8>> = Vec::new();
    let mut confirmed = Vec::new();
    // A queue of the frames in flight, and the one the link loses.
    let mut link: Vec<Vec<u8>> = Vec::new();
    let mut dropped = false;
    let mut out = [0u8; F];
    feed(
        &mut ground,
        &mut link,
        &mut confirmed,
        directive(Directive::InitiateAdWithoutClcwCheck),
    );
    let fdus: Vec<[u8; 3]> = (0..5u8).map(|i| [i, i ^ 0xFF, 0x5A]).collect();
    let mut next = 0;
    // bound: at most 200 rounds; each round the ground end feeds the CLCW the flight end reports
    for round in 0..200 {
        if next < fdus.len() && !ground.wait_queue_occupied() {
            feed(
                &mut ground,
                &mut link,
                &mut confirmed,
                Fop1Input::RequestAd {
                    handle: u32::try_from(next).unwrap(),
                    fdu: &fdus[next],
                },
            );
            next += 1;
        }
        for frame in std::mem::take(&mut link) {
            // The link loses the second Type-AD frame, once.
            if !dropped
                && UslpFrame::decode(&frame, &t)
                    .unwrap()
                    .full()
                    .unwrap()
                    .vcf_count()
                    .value()
                    == 1
            {
                dropped = true;
                continue;
            }
            let decoded = UslpFrame::decode(&frame, &t).unwrap();
            let (after, actions) = farm
                .step(
                    Farm1Input::Frame {
                        frame: farm_frame(&decoded),
                        buffer_available: true,
                    },
                    NOW,
                    &mut out,
                )
                .unwrap();
            farm = after;
            for a in actions.iter() {
                if let Farm1Output::Deliver { data, .. } = a {
                    delivered.push(data.slice(&out).unwrap().to_vec());
                }
            }
        }
        feed(
            &mut ground,
            &mut link,
            &mut confirmed,
            Fop1Input::Clcw(farm.clcw()),
        );
        if confirmed.len() == fdus.len() {
            break;
        }
        assert!(round < 199, "no completion: {delivered:?} {confirmed:?}");
    }
    assert!(dropped);
    assert_eq!(
        delivered,
        fdus.iter().map(|f| f.to_vec()).collect::<Vec<_>>()
    );
    assert_eq!(confirmed, [0, 1, 2, 3, 4]);
}

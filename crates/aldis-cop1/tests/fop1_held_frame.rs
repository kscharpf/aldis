//! `Fop1::held_frame`: the frame the engine holds for the outstanding Transmit request of a kind
//! (232.1 §5.1.6, §5.1.7; COP-011, API-056). A caller that holds a request until a link slot reads
//! the master copy from the `Sent_Queue` instead of copying it from the output area (#505).
#![expect(
    clippy::unwrap_used,
    reason = "test-only harness: a violated assumption must fail loudly (RS-018)"
)]

use aldis_cop1::{
    Directive, Fop1, Fop1Config, Fop1Input, Fop1Output, Fop1State, Fop1Target, FrameKind, HeldKind,
    TimeoutType, fop1_max_actions,
};
use aldis_core::ids::FrameSeqNum;
use aldis_core::time::{Duration, Instant};
use aldis_tc::{
    Clcw, ClcwStatus, CopInEffect, FarmBCounter, PhysicalChannelConfig, TcSpacecraftId, TcVcId,
};
use core::num::NonZeroU8;

const Q: usize = 4;
const F: usize = 32;
const N: usize = fop1_max_actions(Q);
type Engine = Fop1<Q, F, N>;
const VCID: u8 = 5;

fn engine() -> Engine {
    let target = Fop1Target::tc(
        TcSpacecraftId::new(7).unwrap(),
        TcVcId::new(VCID).unwrap(),
        PhysicalChannelConfig::new(F, true).unwrap(),
    )
    .unwrap();
    Engine::new(Fop1Config {
        target,
        t1_initial: Duration::from_micros(1_000),
        transmission_limit: NonZeroU8::new(3).unwrap(),
        timeout_type: TimeoutType::Tt0,
        sliding_window_k: NonZeroU8::new(4).unwrap(),
        ad_repetitions: NonZeroU8::MIN,
        bc_repetitions: NonZeroU8::MIN,
    })
    .unwrap()
}

/// One transmit request of an input: its kind, N(S), whether it retransmits, and the frame
/// octets that the engine wrote to the output area.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Sent {
    kind: FrameKind,
    sequence_number: FrameSeqNum,
    retransmission: bool,
    frame: Vec<u8>,
}

/// Feeds `input` at `now` and commits it; returns every Transmit request it produced, with the
/// octets of its frame copied out of the output area.
fn feed(f: &mut Engine, input: Fop1Input<'_>, now: u64) -> Vec<Sent> {
    let mut area = [0u8; 4 * F];
    let actions = f
        .decide(input, Instant::from_micros(now), &mut area)
        .unwrap()
        .commit();
    actions
        .iter()
        .filter_map(|a| match a {
            Fop1Output::Transmit {
                kind,
                sequence_number,
                retransmission,
                frame,
                ..
            } => Some(Sent {
                kind: *kind,
                sequence_number: *sequence_number,
                retransmission: *retransmission,
                frame: frame.slice(&area).unwrap().to_vec(),
            }),
            Fop1Output::Abort
            | Fop1Output::Transfer(_)
            | Fop1Output::Directive(_)
            | Fop1Output::Alert(_)
            | Fop1Output::Suspend => None,
        })
        .collect()
}

const fn directive(d: Directive) -> Fop1Input<'static> {
    Fop1Input::Directive {
        handle: 1,
        directive: d,
    }
}

const fn done(kind: FrameKind) -> Fop1Input<'static> {
    Fop1Input::TransmitDone {
        kind,
        accepted: true,
    }
}

fn clcw(retransmit: bool, lockout: bool, n_r: u8) -> Fop1Input<'static> {
    Fop1Input::Clcw(Clcw {
        status: ClcwStatus::new(0).unwrap(),
        cop_in_effect: CopInEffect::new(1).unwrap(),
        vc_id: TcVcId::new(VCID).unwrap(),
        no_rf_available: false,
        no_bit_lock: false,
        lockout,
        wait: false,
        retransmit,
        farm_b_counter: FarmBCounter::default(),
        report_value: FrameSeqNum::new(n_r),
    })
}

/// An engine in S1 (Active), after Initiate AD without CLCW check.
fn active() -> Engine {
    let mut f = engine();
    assert_eq!(
        feed(&mut f, directive(Directive::InitiateAdWithoutClcwCheck), 0),
        vec![]
    );
    assert_eq!(f.state(), Fop1State::Active);
    f
}

/// Requests `fdu` as a Type-AD frame and returns the one Transmit request.
fn request_ad(f: &mut Engine, fdu: &[u8], now: u64) -> Sent {
    let mut sent = feed(f, Fop1Input::RequestAd { handle: 2, fdu }, now);
    assert_eq!(sent.len(), 1);
    sent.remove(0)
}

fn held(f: &Engine, kind: HeldKind) -> Option<Vec<u8>> {
    f.held_frame(kind).map(<[u8]>::to_vec)
}

#[test]
fn the_ad_frame_is_held_from_its_request_until_the_response() {
    // req: COP-011, API-056
    let mut f = active();
    assert_eq!(held(&f, HeldKind::Ad), None, "no request is outstanding");
    let request = request_ad(&mut f, &[1, 2, 3], 10);
    assert_eq!(request.kind, FrameKind::Ad);
    assert!(!f.out_flag_ready(FrameKind::Ad));
    assert_eq!(held(&f, HeldKind::Ad), Some(request.frame.clone()));
    assert_eq!(
        f.held_frame(HeldKind::Ad).map(<[u8]>::len),
        Some(request.frame.len())
    );
    // The other kind holds nothing.
    assert_eq!(held(&f, HeldKind::Bc), None);
    // The response releases the Out flag: no request is outstanding, the master copy stays for
    // retransmission and is no longer held for a request.
    assert_eq!(feed(&mut f, done(FrameKind::Ad), 20), vec![]);
    assert!(f.out_flag_ready(FrameKind::Ad));
    assert_eq!(f.sent_queue_len(), 1);
    assert_eq!(held(&f, HeldKind::Ad), None);
}

#[test]
fn the_held_ad_frame_is_the_one_in_flight_not_the_oldest_on_the_queue() {
    // req: COP-011, API-056
    let mut f = active();
    let first = request_ad(&mut f, &[1, 1, 1], 10);
    assert_eq!(feed(&mut f, done(FrameKind::Ad), 20), vec![]);
    let second = request_ad(&mut f, &[2, 2, 2, 2], 30);
    assert_ne!(first.frame, second.frame);
    assert_eq!(f.sent_queue_len(), 2);
    assert_eq!(held(&f, HeldKind::Ad), Some(second.frame.clone()));
    // An FDU requested while the flag is Not_Ready waits; the held frame does not change.
    let waiting = Fop1Input::RequestAd {
        handle: 3,
        fdu: &[3],
    };
    assert_eq!(feed(&mut f, waiting, 40), vec![]);
    assert_eq!(held(&f, HeldKind::Ad), Some(second.frame));
    // The response sends the waiting FDU: the held frame is the third, at the third position.
    let mut sent = feed(&mut f, done(FrameKind::Ad), 50);
    assert_eq!(sent.len(), 1);
    let third = sent.remove(0);
    assert_eq!(third.sequence_number, FrameSeqNum::new(2));
    assert_eq!(held(&f, HeldKind::Ad), Some(third.frame));
}

#[test]
fn the_held_frame_survives_a_removal_of_acknowledged_frames_ahead_of_it() {
    // req: COP-011, API-056
    let mut f = active();
    let _first = request_ad(&mut f, &[1], 10);
    assert_eq!(feed(&mut f, done(FrameKind::Ad), 20), vec![]);
    let second = request_ad(&mut f, &[2, 2], 30);
    // A CLCW acknowledges the first frame while the second is outstanding: the queue moves up.
    assert_eq!(feed(&mut f, clcw(false, false, 1), 40), vec![]);
    assert_eq!(f.sent_queue_len(), 1);
    assert_eq!(held(&f, HeldKind::Ad), Some(second.frame));
}

#[test]
fn the_bc_frame_is_held_while_its_request_is_outstanding() {
    // req: COP-011, API-056
    for (command, expected) in [
        (Directive::InitiateAdWithUnlock, "Unlock"),
        (
            Directive::InitiateAdWithSetVr {
                v_r: FrameSeqNum::new(9),
            },
            "Set V(R)",
        ),
    ] {
        let mut f = engine();
        let mut sent = feed(&mut f, directive(command), 0);
        assert_eq!(sent.len(), 1, "{expected}");
        let request = sent.remove(0);
        assert_eq!(request.kind, FrameKind::Bc, "{expected}");
        assert!(!f.out_flag_ready(FrameKind::Bc));
        assert_eq!(held(&f, HeldKind::Bc), Some(request.frame), "{expected}");
        assert_eq!(held(&f, HeldKind::Ad), None, "{expected}");
        assert_eq!(feed(&mut f, done(FrameKind::Bc), 10), vec![]);
        assert!(f.out_flag_ready(FrameKind::Bc));
        assert_eq!(held(&f, HeldKind::Bc), None, "{expected}");
    }
}

#[test]
fn a_type_bd_request_holds_no_frame_and_changes_no_held_frame() {
    // req: COP-011, API-056
    // A Type-BD frame is not retained and has no `HeldKind` to ask for (RS-020).
    let mut f = active();
    let mut sent = feed(
        &mut f,
        Fop1Input::RequestBd {
            handle: 4,
            fdu: &[7, 7],
        },
        10,
    );
    assert_eq!(sent.len(), 1);
    assert_eq!(sent.remove(0).kind, FrameKind::Bd);
    assert!(!f.out_flag_ready(FrameKind::Bd));
    assert_eq!(held(&f, HeldKind::Ad), None);
    assert_eq!(held(&f, HeldKind::Bc), None);
    // A Type-AD request outstanding at the same time is held; the Type-BD one changes nothing.
    let ad = request_ad(&mut f, &[1], 20);
    assert_eq!(held(&f, HeldKind::Ad), Some(ad.frame.clone()));
    assert_eq!(held(&f, HeldKind::Bc), None);
    assert_eq!(feed(&mut f, done(FrameKind::Bd), 30), vec![]);
    assert!(f.out_flag_ready(FrameKind::Bd));
    assert_eq!(held(&f, HeldKind::Ad), Some(ad.frame));
    assert_eq!(held(&f, HeldKind::Bc), None);
}

#[test]
fn a_held_kind_converts_to_and_from_a_frame_kind() {
    // req: COP-011, API-056, RS-020
    assert_eq!(HeldKind::from_frame_kind(FrameKind::Ad), Some(HeldKind::Ad));
    assert_eq!(HeldKind::from_frame_kind(FrameKind::Bc), Some(HeldKind::Bc));
    assert_eq!(HeldKind::from_frame_kind(FrameKind::Bd), None);
    for k in [HeldKind::Ad, HeldKind::Bc] {
        assert_eq!(HeldKind::from_frame_kind(FrameKind::from(k)), Some(k));
    }
}

/// An engine in S1 with two frames on the `Sent_Queue`, both answered, so that both Out flags are
/// Ready; returns it with the two requests.
fn two_sent() -> (Engine, Sent, Sent) {
    let mut f = active();
    let first = request_ad(&mut f, &[1, 1], 10);
    assert_eq!(feed(&mut f, done(FrameKind::Ad), 20), vec![]);
    let second = request_ad(&mut f, &[2, 2, 2], 30);
    assert_eq!(feed(&mut f, done(FrameKind::Ad), 40), vec![]);
    (f, first, second)
}

#[test]
fn a_retransmission_holds_the_retransmitted_master_copy_after_a_retransmit_clcw() {
    // req: COP-011, API-056
    let (mut f, first, second) = two_sent();
    // The Retransmit Flag with N(R) = NN(R) marks both frames; Look for FDU sends the first.
    let mut sent = feed(&mut f, clcw(true, false, 0), 50);
    assert_eq!(sent.len(), 1);
    let retransmit = sent.remove(0);
    assert!(retransmit.retransmission);
    assert_eq!(retransmit.sequence_number, first.sequence_number);
    assert_eq!(retransmit.frame, first.frame);
    assert_eq!(held(&f, HeldKind::Ad), Some(first.frame));
    // The response of the first sends the second, a retransmission as well.
    let mut sent = feed(&mut f, done(FrameKind::Ad), 60);
    assert_eq!(sent.len(), 1);
    let retransmit = sent.remove(0);
    assert!(retransmit.retransmission);
    assert_eq!(retransmit.frame, second.frame);
    assert_eq!(held(&f, HeldKind::Ad), Some(second.frame));
}

#[test]
fn a_retransmission_holds_the_retransmitted_master_copy_after_a_timeout() {
    // req: COP-011, API-056
    let (mut f, first, _) = two_sent();
    let deadline = f.next_deadline().unwrap().as_micros();
    let mut sent = feed(&mut f, Fop1Input::Timeout, deadline);
    assert_eq!(sent.len(), 1);
    let retransmit = sent.remove(0);
    assert!(retransmit.retransmission);
    assert_eq!(retransmit.frame, first.frame);
    assert_eq!(held(&f, HeldKind::Ad), Some(first.frame));
}

#[test]
fn a_bc_retransmission_holds_the_retransmitted_master_copy_after_a_timeout() {
    // req: COP-011, API-056
    let mut f = engine();
    let mut sent = feed(&mut f, directive(Directive::InitiateAdWithUnlock), 0);
    let first = sent.remove(0);
    assert_eq!(feed(&mut f, done(FrameKind::Bc), 10), vec![]);
    assert_eq!(held(&f, HeldKind::Bc), None);
    let deadline = f.next_deadline().unwrap().as_micros();
    let mut sent = feed(&mut f, Fop1Input::Timeout, deadline);
    assert_eq!(sent.len(), 1);
    let retransmit = sent.remove(0);
    assert_eq!(retransmit.kind, FrameKind::Bc);
    assert!(retransmit.retransmission);
    assert_eq!(retransmit.frame, first.frame);
    assert_eq!(held(&f, HeldKind::Bc), Some(first.frame));
}

#[test]
fn a_purge_of_the_sent_queue_leaves_no_held_frame() {
    // req: COP-011, API-056
    // Alert: a CLCW with the Lockout Flag purges the queue while the request is outstanding.
    let mut f = active();
    let _ = request_ad(&mut f, &[1, 2, 3], 10);
    assert!(held(&f, HeldKind::Ad).is_some());
    assert_eq!(feed(&mut f, clcw(false, true, 0), 20), vec![]);
    assert_eq!(f.state(), Fop1State::Initial);
    assert_eq!(f.sent_queue_len(), 0);
    // The Out flag stays Not_Ready until the caller answers; the engine retains no copy.
    assert!(!f.out_flag_ready(FrameKind::Ad));
    assert_eq!(held(&f, HeldKind::Ad), None);
    assert_eq!(feed(&mut f, done(FrameKind::Ad), 30), vec![]);
    assert!(f.out_flag_ready(FrameKind::Ad));
    assert_eq!(held(&f, HeldKind::Ad), None);

    // Terminate AD purges the Type-BC frame the same way.
    let mut f = engine();
    let _ = feed(&mut f, directive(Directive::InitiateAdWithUnlock), 0);
    assert!(held(&f, HeldKind::Bc).is_some());
    let _ = feed(&mut f, directive(Directive::TerminateAd), 10);
    assert_eq!(f.sent_queue_len(), 0);
    assert!(!f.out_flag_ready(FrameKind::Bc));
    assert_eq!(held(&f, HeldKind::Bc), None);
}

#[test]
fn a_frame_acknowledged_before_its_response_is_no_longer_held() {
    // req: COP-011, API-056
    // 232.1 §5.1.7 a): a CLCW that acknowledges the frame in flight removes its master copy;
    // the Out flag stays Not_Ready until the response (§5.1.6).
    let mut f = active();
    let _ = request_ad(&mut f, &[1, 2, 3], 10);
    assert_eq!(feed(&mut f, clcw(false, false, 1), 20), vec![]);
    assert_eq!(f.sent_queue_len(), 0);
    assert!(!f.out_flag_ready(FrameKind::Ad));
    assert_eq!(held(&f, HeldKind::Ad), None);

    // S5, E1: the CLCW confirms the Type-BC frame; Release BC copy.
    let mut f = engine();
    let _ = feed(&mut f, directive(Directive::InitiateAdWithUnlock), 0);
    assert_eq!(feed(&mut f, clcw(false, false, 0), 10), vec![]);
    assert_eq!(f.state(), Fop1State::Active);
    assert!(!f.out_flag_ready(FrameKind::Bc));
    assert_eq!(held(&f, HeldKind::Bc), None);
}

//! FOP-1 and FARM-1 across a link that loses, duplicates, reorders, and corrupts what it carries,
//! every choice taken from the input (COP-040, COP-041): the FOP-1 submits Sequence-Controlled
//! FDUs, the forward link carries its Type-AD frames to a FARM-1 behind `validate_frame`, and the
//! return link carries the CLCWs the FARM-1 reports. At every instant the FDUs the FARM-1
//! delivered are a gap-free, duplicate-free, in-order prefix of the stream submitted, with their
//! octets intact, and the FDUs the FOP-1 confirmed are an in-order part of what was delivered
//! (232.1 §1). Neither engine refuses an input the harness may make, and none panics (GEN-020).
#![no_main]

#[path = "../../crates/aldis-cop1/tests/common/mod.rs"]
mod common;

use common::Rig;

// req: COP-040, COP-041, COP-010, COP-030, COP-035, GEN-020, VER-022

use aldis_cop1::{
    Directive, Farm1, Farm1Config, Farm1Frame, Farm1Input, Farm1Output, Farm1Window, FduKind,
    Fop1Config, Fop1Input, Fop1Output, Fop1State, Fop1Target, TimeoutType, TransferNotification,
    fop1_max_actions,
};
use aldis_core::ids::FrameSeqNum;
use aldis_core::security::SecurityLayout;
use aldis_core::time::{Duration, Instant};
use aldis_tc::{
    Clcw, ClcwStatus, PhysicalChannelConfig, SegmentHeaderUse, TcSpacecraftId, TcVcId, VcConfig,
    VcTable, validate_frame,
};
use core::num::NonZeroU8;
use libfuzzer_sys::arbitrary::{Result, Unstructured};
use libfuzzer_sys::fuzz_target;

const Q: usize = 8;
const F: usize = 32;
const N: usize = fop1_max_actions(Q);
const SCID: u16 = 0x155;
const VCID: u8 = 3;
/// The octets of an FDU: its index, then a pattern derived from it.
const FDU_LEN: usize = 4;

const fn fdu(index: u8) -> [u8; FDU_LEN] {
    [index, !index, index ^ 0x5A, index.wrapping_mul(31)]
}

/// Feeds `input` to the FOP-1, then the Transmit Responses its frames cause (the lower layer
/// accepts every frame); the frames go to `forward`, the confirmations and the Suspend to the caller.
fn ground(
    f: &mut Rig<Q, F, N>,
    input: Fop1Input<'_>,
    now: u64,
    forward: &mut Vec<Vec<u8>>,
    confirmed: &mut Vec<u32>,
    suspended: &mut bool,
) {
    let mut pending = vec![input];
    // bound: each Transmit Response transmits at most one frame, so the chain ends within K + 2
    // responses; the loop ends at 100 passes in any case
    for _ in 0..100 {
        let Some(input) = pending.pop() else {
            return;
        };
        let mut area = [0_u8; F];
        let decision = f
            .decide(input, Instant::from_micros(now), &mut area)
            .unwrap();
        for a in f.commit(decision).iter() {
            match a {
                Fop1Output::Transmit { kind, frame, .. } => {
                    forward.push(frame.slice(&area).unwrap().to_vec());
                    pending.push(Fop1Input::TransmitDone {
                        kind: *kind,
                        accepted: true,
                    });
                }
                Fop1Output::Transfer(n) if n.status == TransferNotification::PositiveConfirm => {
                    confirmed.push(n.handle);
                }
                Fop1Output::Suspend => *suspended = true,
                _ => {}
            }
        }
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "one loop over the operations of the harness, each a short arm"
)]
fn run(u: &mut Unstructured<'_>) -> Result<()> {
    let scid = TcSpacecraftId::new(SCID).unwrap();
    let vc = TcVcId::new(VCID).unwrap();
    let mut g = Rig::<Q, F, N>::new(Fop1Config {
        target: Fop1Target::tc(scid, vc, F, true).unwrap(),
        t1_initial: Duration::from_micros(u.int_in_range(1..=2_000)?),
        transmission_limit: NonZeroU8::new(u.int_in_range(1..=5)?).unwrap(),
        timeout_type: if u.arbitrary()? {
            TimeoutType::Tt1
        } else {
            TimeoutType::Tt0
        },
        sliding_window_k: u.int_in_range(1..=4)?,
        ad_repetitions: 1,
        bc_repetitions: 1,
    })
    .unwrap();
    let mut flight = Farm1::new(
        Farm1Config {
            vc_id: vc,
            status: ClcwStatus::new(0).unwrap(),
            window: Farm1Window::retransmission_allowed(2 * u.int_in_range(1..=16)?).unwrap(),
        },
        FrameSeqNum::new(0),
    );
    let channels = VcTable::<1, 0>::new(
        PhysicalChannelConfig::new(F, true).unwrap(),
        &[VcConfig::new(scid, vc, F, SegmentHeaderUse::Absent(SecurityLayout::NONE)).unwrap()],
        &[],
    )
    .unwrap();
    let mut forward: Vec<Vec<u8>> = Vec::new();
    let mut ret: Vec<[u8; 4]> = Vec::new();
    let mut delivered: Vec<u8> = Vec::new();
    let mut confirmed: Vec<u32> = Vec::new();
    let (mut now, mut next, mut handle, mut suspended) = (0_u64, 0_u8, 1_000_u32, false);
    let mut buffer_free = true;
    ground(
        &mut g,
        Fop1Input::Directive {
            handle: 0,
            directive: Directive::InitiateAdWithoutClcwCheck,
        },
        now,
        &mut forward,
        &mut confirmed,
        &mut suspended,
    );
    // bound: each pass consumes at least one octet of the input, and `u` is finite
    while !u.is_empty() {
        now += u.int_in_range(0..=1_500)?;
        match u.int_in_range(0..=7)? {
            // Submit the next FDU.
            0 if g.state() < Fop1State::InitializingWithoutBcFrame && !g.wait_queue_occupied() => {
                let data = fdu(next);
                ground(
                    &mut g,
                    Fop1Input::RequestAd {
                        handle: u32::from(next),
                        fdu: &data,
                    },
                    now,
                    &mut forward,
                    &mut confirmed,
                    &mut suspended,
                );
                next = next.wrapping_add(1);
                if next == 0 {
                    // The index is one octet: stop before it repeats.
                    return Ok(());
                }
            }
            // A frame of the forward link arrives (any one: the link reorders), is lost,
            // duplicated, or corrupted.
            1 | 2 if !forward.is_empty() => {
                let i = u.int_in_range(0..=forward.len() - 1)?;
                let mut bytes = if u.ratio(1, 4)? {
                    forward[i].clone()
                } else {
                    forward.remove(i)
                };
                if u.ratio(1, 8)? {
                    continue;
                }
                if u.ratio(1, 8)? {
                    let bit = u.int_in_range(0..=bytes.len() * 8 - 1)?;
                    bytes[bit / 8] ^= 1 << (bit % 8);
                }
                let input = match validate_frame(&bytes, &channels) {
                    Ok(valid) => Farm1Input::Frame {
                        frame: Farm1Frame::from_tc(&valid),
                        buffer_available: buffer_free,
                    },
                    Err(_) => Farm1Input::InvalidFrame,
                };
                let mut area = [0_u8; F];
                let (next_flight, actions) = flight
                    .step(input, Instant::from_micros(now), &mut area)
                    .unwrap();
                flight = next_flight;
                for a in actions.iter() {
                    if let Farm1Output::Deliver {
                        kind: FduKind::Ad,
                        data,
                        ..
                    } = a
                    {
                        let octets = data.slice(&area).unwrap();
                        let index = *octets.first().unwrap();
                        assert_eq!(octets, fdu(index));
                        delivered.push(index);
                        if u.ratio(1, 6)? {
                            buffer_free = false;
                        }
                    }
                }
            }
            // The receiving application releases its buffer.
            3 if !buffer_free => {
                buffer_free = true;
                let mut area = [0_u8; F];
                flight = flight
                    .step(
                        Farm1Input::BufferReleased,
                        Instant::from_micros(now),
                        &mut area,
                    )
                    .unwrap()
                    .0;
            }
            // The FARM-1 samples its CLCW.
            4 => ret.push(flight.clcw().to_octets()),
            // A CLCW of the return link arrives, or is lost, or is duplicated.
            5 | 6 if !ret.is_empty() => {
                let i = u.int_in_range(0..=ret.len() - 1)?;
                let word = if u.ratio(1, 4)? {
                    ret[i]
                } else {
                    ret.remove(i)
                };
                if u.ratio(1, 8)? {
                    continue;
                }
                let input = match Clcw::decode(word) {
                    Ok(clcw) => Fop1Input::Clcw(clcw),
                    Err(_) => Fop1Input::InvalidClcw,
                };
                ground(
                    &mut g,
                    input,
                    now,
                    &mut forward,
                    &mut confirmed,
                    &mut suspended,
                );
            }
            // The clock passes the deadline of the timer, and a Suspend is answered by Resume.
            _ => {
                if g.next_deadline().is_some_and(|d| d.as_micros() <= now) {
                    ground(
                        &mut g,
                        Fop1Input::Timeout,
                        now,
                        &mut forward,
                        &mut confirmed,
                        &mut suspended,
                    );
                }
                if g.state() == Fop1State::Initial && !suspended {
                    // After an Alert: initiate again, with an Unlock when the FARM-1 is in
                    // Lockout, and resubmit from the first FDU not delivered. Initiate AD with
                    // Set V(R) is not used: a Type-BC frame the link duplicates can arrive after
                    // later deliveries and set V(R) back, which the harness cannot tell from a
                    // fault of the engines.
                    handle += 1;
                    let directive = if flight.lockout_flag() {
                        Directive::InitiateAdWithUnlock
                    } else {
                        Directive::InitiateAdWithoutClcwCheck
                    };
                    next = u8::try_from(delivered.len()).unwrap();
                    // What the links still carry belongs to the stream before the Alert.
                    forward.clear();
                    ret.clear();
                    ground(
                        &mut g,
                        Fop1Input::Directive { handle, directive },
                        now,
                        &mut forward,
                        &mut confirmed,
                        &mut suspended,
                    );
                }
                if suspended && g.state() == Fop1State::Initial {
                    suspended = false;
                    handle += 1;
                    ground(
                        &mut g,
                        Fop1Input::Directive {
                            handle,
                            directive: Directive::ResumeAd,
                        },
                        now,
                        &mut forward,
                        &mut confirmed,
                        &mut suspended,
                    );
                }
            }
        }
        // The service guarantee at every instant (232.1 §1).
        assert!(
            delivered
                .iter()
                .enumerate()
                .all(|(i, d)| usize::from(*d) == i)
        );
        // The FDUs confirmed are in order, and each was delivered (an Alert purges the FDUs it
        // leaves unconfirmed, so a delivered FDU may never be confirmed).
        assert!(confirmed.windows(2).all(|w| w[0] < w[1]));
        assert!(
            confirmed
                .last()
                .is_none_or(|h| usize::try_from(*h).is_ok_and(|h| h < delivered.len()))
        );
        assert!(delivered.len() <= usize::from(next));
    }
    Ok(())
}

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    let _ = run(&mut u);
});

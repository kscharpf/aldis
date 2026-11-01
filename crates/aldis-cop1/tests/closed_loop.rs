//! Closed-loop tests of FOP-1 and FARM-1 across a simulated bidirectional link (COP-040; M3
//! plan D7).
//!
//! The ground end is a [`Fop1`] that submits a stream of Sequence-Controlled FDUs; the flight
//! end is a [`Farm1`] that validates each arriving frame with `aldis-tc` and delivers the FDUs.
//! The forward link carries the Type-AD frames the FOP-1 transmits, and the return link the
//! CLCWs the FARM-1 reports at a fixed sampling period. Each link loses, duplicates, delays,
//! and (forward only) corrupts what it carries; a delay that varies from item to item reorders
//! the items. A corrupted frame fails the FECF, so the flight end feeds E9 (`InvalidFrame`). The
//! return link never corrupts: a corrupted CLCW would fail the check of the frame that carries
//! it and never reach the FOP-1, so this harness does not model it, and `Fop1Input::InvalidClcw`
//! is driven by `event_sequences.rs`, not here.
//!
//! Every scenario asserts the service guarantee of 232.1 §1: the delivered FDUs are a gap-free,
//! duplicate-free, in-order prefix of the submitted stream, and either all FDUs are delivered
//! and confirmed or the FOP-1 raised an Alert. Under Timeout Type 1 there is a third outcome,
//! "suspended" (COP-040): the driver answers each Suspend with a Resume directive (232.1
//! §5.1.9.3), but Resume only starts the Timer and clears `Suspend_State` (§5.2.17) and does not
//! reset the `Transmission_Count`, which only Initialize sets to 1 (§5.2.14 c). A frame lost
//! beyond the limit is therefore never retransmitted, and the transfer cycles through Suspend
//! and Resume without an Alert and without completing. Those runs are asserted as such.
//!
//! The simulation is a fixed-step clock with a seeded generator, so a failure reproduces from
//! its scenario and seed.
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "test-only harness: a violated assumption must fail loudly (RS-018)"
)]

#[path = "common/mod.rs"]
mod common;

use aldis_cop1::{
    Alert, Directive, Farm1, Farm1Config, Farm1Input, Farm1Output, Farm1Window, FduKind,
    Fop1Config, Fop1Input, Fop1Output, Fop1State, Fop1Target, FrameKind, TimeoutType,
    TransferNotification, fop1_max_actions,
};
use aldis_core::ids::FrameSeqNum;
use aldis_core::security::SecurityLayout;
use aldis_core::time::{Duration, Instant};
use aldis_tc::{
    Clcw, ClcwStatus, PhysicalChannelConfig, SegmentHeaderUse, TcSpacecraftId, TcVcId, VcConfig,
    VcTable, validate_frame,
};
use aldis_testkit::strategies::proptest::prelude::*;
use aldis_testkit::strategies::{self};
use core::num::NonZeroU8;

/// Failing cases are saved here and replayed first.
const REGRESSIONS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/proptest-regressions/closed_loop.txt"
);

/// A `Sent_Queue` of 16 frames of at most 64 octets.
use common::Rig;

const Q: usize = 16;
const F: usize = 64;
const N: usize = fop1_max_actions(Q);
type Ground = Rig<Q, F, N>;

const SCID: u16 = 0x155;
const VCID: u8 = 3;
/// The clock step of the simulation.
const TICK_US: u64 = 1_000;
/// The octets of an FDU: its index in two octets, then a pattern derived from it.
const FDU_LEN: usize = 8;

/// A splitmix64 generator: small, seeded, and the same on every platform.
struct Rng(u64);

impl Rng {
    const fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// True with probability `permille` in a thousand.
    fn chance(&mut self, permille: u32) -> bool {
        self.next() % 1000 < u64::from(permille)
    }

    /// A number in `0..=max`.
    const fn up_to(&mut self, max: u64) -> u64 {
        self.next() % (max + 1)
    }
}

/// What a link does to the items it carries (COP-040).
#[derive(Debug, Clone, Copy)]
struct Link {
    /// Per thousand items lost.
    loss: u32,
    /// Per thousand items delivered twice.
    duplicate: u32,
    /// Per thousand items with a flipped bit (forward link only).
    corrupt: u32,
    /// The least delay, in microseconds.
    delay_us: u64,
    /// The delay varies by up to this much per item.
    jitter_us: u64,
    /// Whether the link keeps the order of what it carries: the delay of an item is at least
    /// that of the one before.
    in_order: bool,
}

impl Link {
    const IDEAL: Link = Link {
        loss: 0,
        duplicate: 0,
        corrupt: 0,
        delay_us: 2_000,
        jitter_us: 0,
        in_order: true,
    };
}

/// One direction of the link: items with the time they arrive.
struct Channel {
    link: Link,
    items: Vec<(u64, u64, Vec<u8>)>,
    count: u64,
    latest: u64,
}

impl Channel {
    const fn new(link: Link) -> Channel {
        Channel {
            link,
            items: Vec::new(),
            count: 0,
            latest: 0,
        }
    }

    fn send(&mut self, now: u64, item: &[u8], rng: &mut Rng) {
        if rng.chance(self.link.loss) {
            return;
        }
        let copies = if rng.chance(self.link.duplicate) {
            2
        } else {
            1
        };
        for _ in 0..copies {
            let mut bytes = item.to_vec();
            if rng.chance(self.link.corrupt) {
                let bits = u64::try_from(bytes.len() * 8).unwrap();
                let bit = usize::try_from(rng.up_to(bits - 1)).unwrap();
                bytes[bit / 8] ^= 1 << (bit % 8);
            }
            let mut due = now + self.link.delay_us + rng.up_to(self.link.jitter_us);
            if self.link.in_order {
                due = due.max(self.latest);
                self.latest = due;
            }
            self.count += 1;
            self.items.push((due, self.count, bytes));
        }
    }

    /// The items that have arrived by `now`, in the order of arrival.
    fn arrived(&mut self, now: u64) -> Vec<Vec<u8>> {
        let (mut due, later): (Vec<_>, Vec<_>) = self.items.drain(..).partition(|i| i.0 <= now);
        self.items = later;
        due.sort_by_key(|i| (i.0, i.1));
        due.into_iter().map(|i| i.2).collect()
    }
}

/// A closed-loop scenario (COP-040).
#[derive(Debug, Clone, Copy)]
struct Scenario {
    name: &'static str,
    forward: Link,
    ret: Link,
    /// Microseconds between CLCW samples of the FARM-1.
    clcw_period_us: u64,
    /// FDUs to transfer.
    fdus: u16,
    /// The FOP Sliding Window Width K.
    k: u8,
    /// The FARM Sliding Window Width W (retransmission allowed, so PW = NW = W/2).
    w: u16,
    t1_us: u64,
    transmission_limit: u8,
    timeout_type: TimeoutType,
    /// Per thousand accepted FDUs after which the receiving application holds its buffer.
    stall: u32,
    /// Per thousand ticks at which a held buffer is released.
    release: u32,
    /// The simulated time after which the scenario stops.
    horizon_us: u64,
}

/// What a scenario produced.
#[derive(Debug, Default)]
struct Outcome {
    delivered: Vec<u16>,
    confirmed: Vec<u32>,
    negative: Vec<u32>,
    alerts: Vec<Alert>,
    suspensions: usize,
    expedited_delivered: Vec<u16>,
    retransmissions: usize,
    stalls: usize,
    discarded: usize,
    /// Expiries of T1 while FOP-1 is in S3 (E16, E104; OI-67).
    s3_expiries: usize,
}

const fn fdu(index: u16) -> [u8; FDU_LEN] {
    let [hi, lo] = index.to_be_bytes();
    let seed = index.wrapping_mul(0x9E37);
    let [a, b] = seed.to_be_bytes();
    [hi, lo, a, b, a ^ 0xFF, b ^ 0xFF, hi ^ lo, 0x5A]
}

/// The index an FDU carries, after checking the rest of its octets.
fn index_of(octets: &[u8]) -> u16 {
    let index = u16::from_be_bytes([octets[0], octets[1]]);
    assert_eq!(
        octets,
        fdu(index),
        "delivered octets differ from those sent"
    );
    index
}

struct World {
    s: Scenario,
    rng: Rng,
    now: u64,
    ground: Ground,
    flight: Farm1,
    channels: VcTable<1, 0>,
    forward: Channel,
    ret: Channel,
    out: [u8; F],
    buffer_free: bool,
    next_fdu: u16,
    next_handle: u32,
    resume_due: bool,
    outcome: Outcome,
}

impl World {
    fn new(s: Scenario, seed: u64) -> World {
        let scid = TcSpacecraftId::new(SCID).unwrap();
        let vc = TcVcId::new(VCID).unwrap();
        let target = Fop1Target::tc(scid, vc, F, true).unwrap();
        let ground = Ground::new(Fop1Config {
            target,
            t1_initial: Duration::from_micros(s.t1_us),
            transmission_limit: NonZeroU8::new(s.transmission_limit).unwrap(),
            timeout_type: s.timeout_type,
            sliding_window_k: s.k,
            ad_repetitions: 1,
            bc_repetitions: 1,
        })
        .unwrap();
        let flight = Farm1::new(
            Farm1Config {
                vc_id: vc,
                status: ClcwStatus::new(0).unwrap(),
                window: Farm1Window::retransmission_allowed(s.w).unwrap(),
            },
            FrameSeqNum::new(0),
        );
        let physical = PhysicalChannelConfig::new(F, true).unwrap();
        let channel =
            VcConfig::new(scid, vc, F, SegmentHeaderUse::Absent(SecurityLayout::NONE)).unwrap();
        World {
            s,
            rng: Rng(seed),
            now: 0,
            ground,
            flight,
            channels: VcTable::new(physical, &[channel], &[]).unwrap(),
            forward: Channel::new(s.forward),
            ret: Channel::new(s.ret),
            out: [0; F],
            buffer_free: true,
            next_fdu: 0,
            next_handle: 1_000_000,
            resume_due: false,
            outcome: Outcome::default(),
        }
    }

    const fn instant(&self) -> Instant {
        Instant::from_micros(self.now)
    }

    /// Feeds `input` to the FOP-1, then the Transmit Responses its frames cause: the lower
    /// layer accepts every frame at once.
    fn ground_input(&mut self, input: Fop1Input<'_>) {
        let mut responses = Vec::new();
        self.ground_once(input, &mut responses);
        let mut steps = 0;
        // bound: each response transmits at most one frame, so the chain ends within K + 2
        // responses; `steps` fails the test beyond 1000
        while let Some(kind) = responses.pop() {
            steps += 1;
            assert!(steps < 1_000, "Transmit Responses do not end");
            self.ground_once(
                Fop1Input::TransmitDone {
                    kind,
                    accepted: true,
                },
                &mut responses,
            );
        }
    }

    fn ground_once(&mut self, input: Fop1Input<'_>, responses: &mut Vec<FrameKind>) {
        let now = self.instant();
        let decision = self
            .ground
            .decide(input, now, &mut self.out)
            .unwrap_or_else(|e| panic!("{}: FOP-1 refused {input:?}: {e:?}", self.s.name));
        let actions = self.ground.commit(decision);
        for action in actions.iter() {
            match *action {
                Fop1Output::Transmit {
                    kind,
                    retransmission,
                    frame,
                    ..
                } => {
                    let bytes = frame.slice(&self.out).unwrap().to_vec();
                    self.forward.send(self.now, &bytes, &mut self.rng);
                    self.outcome.retransmissions += usize::from(retransmission);
                    responses.push(kind);
                }
                Fop1Output::Transfer(n) => match n.status {
                    TransferNotification::PositiveConfirm => self.outcome.confirmed.push(n.handle),
                    TransferNotification::NegativeConfirm => self.outcome.negative.push(n.handle),
                    TransferNotification::Accept | TransferNotification::Reject => {}
                },
                Fop1Output::Alert(a) => self.outcome.alerts.push(a),
                Fop1Output::Suspend => {
                    self.resume_due = true;
                    self.outcome.suspensions += 1;
                }
                Fop1Output::Abort | Fop1Output::Directive(_) => {}
            }
        }
    }

    fn flight_input(&mut self, input: Farm1Input<'_>) {
        let (next, actions) = self
            .flight
            .step(input, self.instant(), &mut self.out)
            .unwrap_or_else(|e| panic!("{}: FARM-1 refused {input:?}: {e:?}", self.s.name));
        self.flight = next;
        for action in actions.iter() {
            match *action {
                Farm1Output::Deliver { kind, data, .. } => {
                    let index = index_of(data.slice(&self.out).unwrap());
                    match kind {
                        FduKind::Ad => {
                            self.outcome.delivered.push(index);
                            if self.rng.chance(self.s.stall) {
                                self.buffer_free = false;
                                self.outcome.stalls += 1;
                            }
                        }
                        FduKind::Bd => self.outcome.expedited_delivered.push(index),
                    }
                }
                Farm1Output::Notify(_) => {}
            }
        }
    }

    /// A frame arrives at the flight end.
    fn frame_arrives(&mut self, bytes: &[u8]) {
        if let Ok(valid) = validate_frame(bytes, &self.channels) {
            let input = Farm1Input::from_tc(&valid, self.buffer_free);
            self.flight_input(input);
        } else {
            self.outcome.discarded += 1;
            self.flight_input(Farm1Input::InvalidFrame);
        }
    }

    const fn handle(&mut self) -> u32 {
        self.next_handle += 1;
        self.next_handle
    }

    /// One step of the clock.
    fn tick(&mut self) {
        self.now += TICK_US;
        for bytes in self.forward.arrived(self.now) {
            self.frame_arrives(&bytes);
        }
        if !self.buffer_free && self.rng.chance(self.s.release) {
            self.buffer_free = true;
            self.flight_input(Farm1Input::BufferReleased);
        }
        for octets in self.ret.arrived(self.now) {
            let input = match Clcw::decode(octets.try_into().unwrap()) {
                Ok(clcw) => Fop1Input::Clcw(clcw),
                Err(_) => Fop1Input::InvalidClcw,
            };
            self.ground_input(input);
        }
        if self.now.is_multiple_of(self.s.clcw_period_us) {
            let word = self.flight.clcw().to_octets();
            self.ret.send(self.now, &word, &mut self.rng);
        }
        if self
            .ground
            .next_deadline()
            .is_some_and(|d| d.as_micros() <= self.now)
        {
            if self.ground.state() == Fop1State::RetransmitWithWait {
                self.outcome.s3_expiries += 1;
            }
            self.ground_input(Fop1Input::Timeout);
        }
        if self.resume_due && self.ground.state() == Fop1State::Initial {
            self.resume_due = false;
            let handle = self.handle();
            self.ground_input(Fop1Input::Directive {
                handle,
                directive: Directive::ResumeAd,
            });
        }
        self.submit();
    }

    /// Submits the next FDUs while the FOP-1 accepts them and the service is initiated.
    fn submit(&mut self) {
        if self.ground.state() == Fop1State::Initial {
            return;
        }
        // bound: at most `fdus` submissions in the scenario; the loop also ends when the
        // FOP-1 stops accepting (its Wait_Queue holds one FDU)
        while self.next_fdu < self.s.fdus && !self.ground.wait_queue_occupied() {
            let index = self.next_fdu;
            let data = fdu(index);
            self.ground_input(Fop1Input::RequestAd {
                handle: u32::from(index),
                fdu: &data,
            });
            if self.outcome.alerts.is_empty() {
                self.next_fdu += 1;
            } else {
                return;
            }
        }
    }

    fn run(mut self) -> Outcome {
        let handle = self.handle();
        self.ground_input(Fop1Input::Directive {
            handle,
            directive: Directive::InitiateAdWithoutClcwCheck,
        });
        // bound: the clock ends at the horizon, or sooner when the transfer is complete or the
        // FOP-1 has raised an Alert
        while self.now < self.s.horizon_us {
            self.tick();
            let complete = self.outcome.confirmed.len() == usize::from(self.s.fdus);
            if complete || !self.outcome.alerts.is_empty() {
                break;
            }
        }
        self.outcome
    }
}

/// Asserts the part of the service guarantee that holds at any instant: what was delivered is
/// an in-order, gap-free, duplicate-free prefix of the stream, and what was confirmed was
/// delivered (232.1 §1; COP-040).
fn assert_prefix(s: &Scenario, seed: u64, o: &Outcome) {
    let at = format!("{} seed {seed}: {o:?}", s.name);
    let delivered: Vec<u16> = (0..u16::try_from(o.delivered.len()).unwrap()).collect();
    assert_eq!(
        o.delivered, delivered,
        "not an in-order, gap-free, duplicate-free prefix: {at}"
    );
    assert!(
        o.confirmed.len() <= o.delivered.len()
            && o.confirmed
                .iter()
                .enumerate()
                .all(|(i, h)| *h == u32::try_from(i).unwrap()),
        "a confirmed FDU is not an in-order delivered one: {at}"
    );
}

/// Asserts the service guarantee of 232.1 §1 on `o`: the prefix, and then either all FDUs
/// delivered and confirmed or an Alert (COP-040).
fn assert_guarantee(s: &Scenario, seed: u64, o: &Outcome) {
    assert_prefix(s, seed, o);
    let at = format!("{} seed {seed}: {o:?}", s.name);
    if o.alerts.is_empty() {
        assert_eq!(o.delivered.len(), usize::from(s.fdus), "incomplete: {at}");
        assert_eq!(o.confirmed.len(), usize::from(s.fdus), "unconfirmed: {at}");
    }
}

/// Asserts the outcome COP-040 allows (232.1 §1, §5.2.14 c, §5.2.17): the prefix, and then all
/// FDUs delivered and confirmed, or an Alert, or, under Timeout Type 1 only, a transfer that has
/// been suspended and was still unfinished and without Alert when the run ended.
fn assert_outcome(s: &Scenario, seed: u64, o: &Outcome) {
    assert_prefix(s, seed, o);
    let complete = o.confirmed.len() == usize::from(s.fdus);
    if !complete && o.alerts.is_empty() {
        assert!(
            s.timeout_type == TimeoutType::Tt1 && o.suspensions > 0,
            "neither complete, nor an Alert, nor suspended under Timeout Type 1: {} seed {seed}: {o:?}",
            s.name
        );
    }
}

const BASE: Scenario = Scenario {
    name: "base",
    forward: Link::IDEAL,
    ret: Link::IDEAL,
    clcw_period_us: 5_000,
    fdus: 60,
    k: 4,
    w: 16,
    t1_us: 100_000,
    transmission_limit: 8,
    timeout_type: TimeoutType::Tt0,
    stall: 0,
    release: 0,
    horizon_us: 20_000_000,
};

/// Runs `s` for each of `seeds` and returns the outcomes.
fn run_seeds(s: Scenario, seeds: u64) -> Vec<Outcome> {
    (0..seeds)
        .map(|seed| {
            let o = World::new(s, seed).run();
            assert_guarantee(&s, seed, &o);
            o
        })
        .collect()
}

/// Asserts that no scenario of `outcomes` raised an Alert.
fn assert_no_alert(s: &Scenario, outcomes: &[Outcome]) {
    for (seed, o) in outcomes.iter().enumerate() {
        assert!(
            o.alerts.is_empty(),
            "{} seed {seed}: {:?}",
            s.name,
            o.alerts
        );
    }
}

#[test]
fn an_ideal_link_transfers_everything_without_retransmission() {
    // req: COP-040, COP-010, COP-011, COP-030, COP-035
    let outcomes = run_seeds(BASE, 4);
    assert_no_alert(&BASE, &outcomes);
    for o in &outcomes {
        assert_eq!(o.retransmissions, 0);
        assert_eq!(o.discarded, 0);
    }
}

#[test]
fn forward_loss_is_repaired_by_retransmission() {
    // req: COP-040, COP-011, COP-013, COP-030, COP-034
    let s = Scenario {
        name: "forward loss",
        forward: Link {
            loss: 150,
            ..Link::IDEAL
        },
        transmission_limit: 12,
        ..BASE
    };
    let outcomes = run_seeds(s, 24);
    assert_no_alert(&s, &outcomes);
    assert!(outcomes.iter().any(|o| o.retransmissions > 0));
}

#[test]
fn return_loss_is_repaired_by_the_next_report_or_the_timer() {
    // req: COP-040, COP-013, COP-035
    let s = Scenario {
        name: "return loss",
        ret: Link {
            loss: 300,
            ..Link::IDEAL
        },
        transmission_limit: 12,
        ..BASE
    };
    let outcomes = run_seeds(s, 24);
    assert_no_alert(&s, &outcomes);
}

#[test]
fn loss_on_both_links_with_delay() {
    // req: COP-040, COP-011, COP-013, COP-035
    let lossy = Link {
        loss: 100,
        delay_us: 30_000,
        jitter_us: 0,
        ..Link::IDEAL
    };
    let s = Scenario {
        name: "loss on both links",
        forward: lossy,
        ret: lossy,
        t1_us: 300_000,
        transmission_limit: 12,
        ..BASE
    };
    let outcomes = run_seeds(s, 24);
    assert_no_alert(&s, &outcomes);
}

#[test]
fn corruption_is_discarded_by_the_fecf_and_repaired() {
    // req: COP-040, COP-011, COP-031
    let s = Scenario {
        name: "forward corruption",
        forward: Link {
            corrupt: 150,
            ..Link::IDEAL
        },
        transmission_limit: 12,
        ..BASE
    };
    let outcomes = run_seeds(s, 24);
    assert_no_alert(&s, &outcomes);
    assert!(outcomes.iter().any(|o| o.discarded > 0));
}

#[test]
fn duplicated_frames_and_reports_deliver_once() {
    // req: COP-040, COP-030, COP-035
    let dup = Link {
        duplicate: 300,
        ..Link::IDEAL
    };
    let s = Scenario {
        name: "duplication",
        forward: dup,
        ret: dup,
        ..BASE
    };
    let outcomes = run_seeds(s, 24);
    assert_no_alert(&s, &outcomes);
}

#[test]
fn forward_reordering_is_repaired() {
    // req: COP-040, COP-011, COP-030
    let s = Scenario {
        name: "forward reordering",
        forward: Link {
            jitter_us: 12_000,
            in_order: false,
            ..Link::IDEAL
        },
        transmission_limit: 12,
        ..BASE
    };
    let outcomes = run_seeds(s, 24);
    assert_no_alert(&s, &outcomes);
}

#[test]
fn a_slow_clcw_sampling_rate_still_completes() {
    // req: COP-040, COP-013, COP-035
    let s = Scenario {
        name: "slow sampling",
        clcw_period_us: 60_000,
        t1_us: 400_000,
        transmission_limit: 12,
        ..BASE
    };
    let outcomes = run_seeds(s, 8);
    assert_no_alert(&s, &outcomes);
}

#[test]
fn a_receiver_that_holds_its_buffer_waits_and_resumes() {
    // req: COP-040, COP-033, COP-035
    let s = Scenario {
        name: "buffer stalls",
        stall: 200,
        release: 100,
        transmission_limit: 12,
        ..BASE
    };
    let outcomes = run_seeds(s, 16);
    assert_no_alert(&s, &outcomes);
    assert!(outcomes.iter().any(|o| o.stalls > 0));
}

#[test]
fn every_impairment_at_once() {
    // req: COP-040, COP-011, COP-013, COP-030, COP-033, COP-035
    let s = Scenario {
        name: "all impairments",
        forward: Link {
            loss: 80,
            duplicate: 80,
            corrupt: 80,
            delay_us: 10_000,
            jitter_us: 8_000,
            in_order: false,
        },
        ret: Link {
            loss: 80,
            duplicate: 80,
            corrupt: 0,
            delay_us: 10_000,
            jitter_us: 8_000,
            in_order: true,
        },
        stall: 100,
        release: 150,
        t1_us: 200_000,
        transmission_limit: 20,
        ..BASE
    };
    let outcomes = run_seeds(s, 24);
    assert_no_alert(&s, &outcomes);
}

#[test]
fn a_dead_forward_link_ends_in_an_alert_at_the_transmission_limit() {
    // req: COP-040, COP-013, COP-004
    let s = Scenario {
        name: "dead link",
        forward: Link {
            loss: 1000,
            ..Link::IDEAL
        },
        transmission_limit: 3,
        horizon_us: 5_000_000,
        ..BASE
    };
    for o in run_seeds(s, 4) {
        assert!(!o.alerts.is_empty(), "the Alert of the transmission limit");
        assert_eq!(o.delivered, Vec::<u16>::new());
    }
}

#[test]
fn timeout_type_one_suspends_resumes_and_completes() {
    // req: COP-040, COP-013, COP-014
    // The use 232.1 §5.1.9.3 names: the round trip is longer than T1_Initial times the
    // Transmission_Limit, so the FOP-1 retransmits, reaches the limit, and suspends before the
    // first CLCW arrives. The driver resumes it (§5.2.17 restarts the timer), the CLCWs in
    // flight acknowledge the frames, and the transfer completes without an Alert.
    let slow = Link {
        delay_us: 60_000,
        ..Link::IDEAL
    };
    let s = Scenario {
        name: "suspend and resume",
        forward: slow,
        ret: slow,
        timeout_type: TimeoutType::Tt1,
        transmission_limit: 2,
        t1_us: 40_000,
        horizon_us: 30_000_000,
        ..BASE
    };
    let outcomes = run_seeds(s, 8);
    assert_no_alert(&s, &outcomes);
    assert!(outcomes.iter().all(|o| o.suspensions > 0));
}

#[test]
fn a_loss_after_a_resume_leaves_the_transfer_suspended_not_alerted() {
    // req: COP-040, COP-013, COP-014
    // The scenario above, with a lossy forward link: after a Resume a frame can still be lost,
    // and Resume does not reset the `Transmission_Count` (232.1 §5.2.17, §5.2.14 c), so the
    // frame is not retransmitted and the transfer cycles through Suspend and Resume. The
    // outcome is "complete" or "suspended" and never an Alert; some seeds end suspended.
    let slow = Link {
        delay_us: 60_000,
        ..Link::IDEAL
    };
    let s = Scenario {
        name: "resume then loss",
        forward: Link { loss: 200, ..slow },
        ret: slow,
        timeout_type: TimeoutType::Tt1,
        transmission_limit: 2,
        t1_us: 40_000,
        horizon_us: 30_000_000,
        ..BASE
    };
    let outcomes: Vec<Outcome> = (0..16)
        .map(|seed| {
            let o = World::new(s, seed).run();
            assert_outcome(&s, seed, &o);
            o
        })
        .collect();
    assert_no_alert(&s, &outcomes);
    assert!(
        outcomes
            .iter()
            .any(|o| o.confirmed.len() < usize::from(s.fdus) && o.suspensions > 0),
        "some seed ends suspended"
    );
}

#[test]
fn reordered_reports_complete_or_alert() {
    // req: COP-040, COP-004, COP-012
    // A report that arrives after a newer one carries an N(R) behind NN(R): E13, the Alert
    // `NnR`. The service guarantee still holds up to the Alert.
    let s = Scenario {
        name: "reordered reports",
        ret: Link {
            delay_us: 5_000,
            jitter_us: 20_000,
            in_order: false,
            ..Link::IDEAL
        },
        transmission_limit: 12,
        ..BASE
    };
    let outcomes = run_seeds(s, 32);
    assert!(
        outcomes.iter().any(|o| !o.alerts.is_empty()),
        "a stale report raises the Alert for an invalid N(R)"
    );
}

proptest! {
    #![proptest_config(strategies::config(64, REGRESSIONS))]

    #[test]
    fn any_link_gives_the_service_guarantee_or_an_alert(
        seed in any::<u64>(),
        forward in (0u32..400, 0u32..400, 0u32..300, 0u64..40_000, 0u64..30_000, any::<bool>()),
        ret in (0u32..400, 0u32..400, 0u64..40_000, 0u64..30_000, any::<bool>()),
        k in 1u8..=8,
        w_half in 5u16..=60,
        period in 1u64..=50,
        stall in 0u32..300,
        tl in 1u8..=10,
        tt1 in any::<bool>(),
    ) {
        // req: COP-040, VER-023
        let s = Scenario {
            name: "random link",
            forward: Link {
                loss: forward.0,
                duplicate: forward.1,
                corrupt: forward.2,
                delay_us: forward.3,
                jitter_us: forward.4,
                in_order: forward.5,
            },
            ret: Link {
                loss: ret.0,
                duplicate: ret.1,
                corrupt: 0,
                delay_us: ret.2,
                jitter_us: ret.3,
                in_order: ret.4,
            },
            clcw_period_us: period * 1_000,
            fdus: 24,
            k,
            w: 2 * w_half,
            t1_us: 120_000,
            transmission_limit: tl,
            timeout_type: if tt1 { TimeoutType::Tt1 } else { TimeoutType::Tt0 },
            stall,
            release: 120,
            horizon_us: 60_000_000,
        };
        let o = World::new(s, seed).run();
        assert_outcome(&s, seed, &o);
    }
}

/// The case behind the seed `5edf89f7…` of `proptest-regressions/closed_loop.txt`, run directly.
///
/// A stale CLCW moves FOP-1 from S2 to S3 while T1 runs, T1 expires in S3 (E16), and a later
/// event takes FOP-1 to S1. With the literal "Ignore" cell, which also disarmed the timer, S1
/// then had frames on the `Sent_Queue` and no timer, and the transfer neither completed nor
/// raised an Alert (OI-67; 232.1 §5.1.9.1).
#[test]
fn a_timer_that_expires_in_s3_is_rearmed_so_that_the_transfer_ends() {
    // req: COP-013, COP-040
    let s = Scenario {
        name: "t1 expires in s3",
        forward: Link {
            loss: 240,
            duplicate: 355,
            corrupt: 98,
            delay_us: 7105,
            jitter_us: 5908,
            in_order: true,
        },
        ret: Link {
            loss: 197,
            duplicate: 122,
            corrupt: 0,
            delay_us: 17524,
            jitter_us: 25200,
            in_order: false,
        },
        clcw_period_us: 40 * 1_000,
        fdus: 24,
        k: 8,
        w: 16,
        t1_us: 120_000,
        transmission_limit: 5,
        timeout_type: TimeoutType::Tt0,
        stall: 184,
        release: 120,
        horizon_us: 60_000_000,
    };
    let seed = 5_298_483_145_364_017_037;
    let o = World::new(s, seed).run();
    assert!(
        o.s3_expiries > 0,
        "the seed no longer reaches an expiry of T1 in S3; find another: {o:?}"
    );
    assert_outcome(&s, seed, &o);
}

//! Calls every public `aldis-cop1` FARM-1 API (VER-010; M3 WP-3). Inputs pass through
//! `black_box`.

use core::hint::black_box as bb;

use aldis_cop1::{
    Actions, FARM1_MAX_ACTIONS, FARM1_TABLE, Farm1, Farm1Config, Farm1Event, Farm1Frame,
    Farm1Input, Farm1Notify, Farm1Output, Farm1State, Farm1Window, FduKind, OutRange,
};
use aldis_core::ids::FrameSeqNum;
use aldis_core::security::SecurityLayout;
use aldis_core::time::Instant;
use aldis_core::{DecodeMode, ErrorKind, field};
use aldis_tc::{
    ClcwStatus, ControlCommand, CopInEffect, FrameData, FrameFields, FrameType,
    PhysicalChannelConfig, SegmentHeaderUse, TcFrame, TcSpacecraftId, TcVcId, VcConfig, VcTable,
    encode,
};

use crate::exercise::Tally;
use crate::vectors::{Case, Fail, Run};

const NOW: Instant = Instant::from_micros(0);

/// The successful result of a step: the next state and its actions.
type Step = (Farm1, Actions<Farm1Output, FARM1_MAX_ACTIONS>);

/// The result of a step that succeeded, or `None`.
fn step(f: &Farm1, input: Farm1Input<'_>, out: &mut [u8]) -> Option<Step> {
    f.step(bb(input), NOW, out).ok()
}

/// A Type-AD, Type-BD, and Type-BC TC frame mapped to FARM-1 frames.
fn tc_group(t: &mut Tally) {
    let (Some(scid), Some(vc)) = (TcSpacecraftId::new(bb(0x155)), TcVcId::new(bb(1))) else {
        t.check(false);
        return;
    };
    let plain = SegmentHeaderUse::Absent(SecurityLayout::NONE);
    let (Ok(vcc), Ok(pc)) = (
        VcConfig::new(scid, vc, 1024, plain),
        PhysicalChannelConfig::new(1024, true),
    ) else {
        t.check(false);
        return;
    };
    let Ok(table) = VcTable::<1, 1>::new(pc, &[vcc], &[]) else {
        t.check(false);
        return;
    };
    let mut buf = [0_u8; 32];
    let set = ControlCommand::SetVr(FrameSeqNum::new(bb(9)));
    let fdu = |user_data| FrameData::Fdu {
        segment_header: None,
        user_data,
    };
    for (frame_type, data) in [
        (FrameType::Ad, fdu(bb(&[1, 2, 3]))),
        (FrameType::Bd, fdu(bb(&[4, 5]))),
        (FrameType::Bc, FrameData::Control(set)),
    ] {
        let fields = FrameFields {
            frame_type,
            spacecraft_id: scid,
            vc_id: vc,
            sequence_number: FrameSeqNum::new(bb(7)),
        };
        let Ok(n) = encode(&table, fields, data, &mut buf) else {
            t.check(false);
            continue;
        };
        let octets = buf.get(..n).unwrap_or_default();
        let Ok(frame) = TcFrame::decode(bb(octets), &table, DecodeMode::Strict) else {
            t.check(false);
            continue;
        };
        t.check(match (frame_type, Farm1Frame::from_tc(&frame)) {
            (
                FrameType::Ad,
                Farm1Frame::Ad {
                    sequence_number,
                    data,
                },
            ) => sequence_number.get() == 7 && data == [1, 2, 3],
            (FrameType::Bd, Farm1Frame::Bd { data }) => data == [4, 5],
            (FrameType::Bc, Farm1Frame::Bc { command }) => command == Some(set),
            _ => false,
        });
    }
}

/// The window constructors and accessors.
fn window_group(t: &mut Tally) {
    let allowed = Farm1Window::retransmission_allowed(bb(16));
    t.check(allowed.is_ok_and(|w| w.width() == 16 && w.positive() == 8 && w.negative() == 8));
    t.check(Farm1Window::retransmission_allowed(bb(3)).is_err_and(|e| {
        e.kind() == ErrorKind::InvalidConfig && e.field() == Some(field::FARM_SLIDING_WINDOW_W)
    }));
    let not_allowed = Farm1Window::retransmission_not_allowed(bb(10), bb(7));
    t.check(not_allowed.is_ok_and(|w| w.positive() == 7 && w.negative() == 3));
    t.check(Farm1Window::retransmission_not_allowed(bb(257), bb(1)).is_err());
    t.check(
        Farm1Window::retransmission_not_allowed(bb(10), bb(0))
            .is_err_and(|e| e.field() == Some(field::FARM_POSITIVE_WINDOW_PW)),
    );
    t.check(
        Farm1Window::retransmission_not_allowed(bb(256), bb(128))
            .is_err_and(|e| e.field() == Some(field::FARM_NEGATIVE_WINDOW_NW)),
    );
}

/// The accepted FDU: E1.
fn accept_group(t: &mut Tally, f: &Farm1) {
    let data = bb([0xA1_u8, 0xB2, 0xC3]);
    let mut out = [0_u8; 8];
    let input = Farm1Input::Frame {
        frame: Farm1Frame::Ad {
            sequence_number: FrameSeqNum::new(bb(10)),
            data: &data,
        },
        buffer_available: bb(true),
    };
    let Some((next, acts)) = step(f, input, &mut out) else {
        t.check(false);
        return;
    };
    t.check(next.v_r().get() == 11 && acts.len() == 1 && !acts.is_empty());
    t.check(acts.iter().count() == 1);
    t.check(matches!(
        acts.get(0),
        Some(Farm1Output::Deliver { kind: FduKind::Ad, sequence_number, data: range })
            if sequence_number.get() == 10 && *range == OutRange::new(0, 3)
                && range.slice(&out) == Some(&data[..]) && range.start() == 0 && range.len() == 3
                && range.end() == 3 && !range.is_empty()
    ));
    // The area is too short for the FDU.
    let short = out.get_mut(..2).unwrap_or_default();
    t.check(
        f.step(bb(input), NOW, short)
            .is_err_and(|e| e.kind() == ErrorKind::BufferTooSmall),
    );
}

/// E2 to E5 from S1, and the cell Table 6-1 marks "Not applicable".
fn window_events(t: &mut Tally, f: &Farm1) {
    let data = bb([1_u8]);
    let mut out = [0_u8; 8];
    let ad = |n: u8, buffer_available: bool| Farm1Input::Frame {
        frame: Farm1Frame::Ad {
            sequence_number: FrameSeqNum::new(n),
            data: &data,
        },
        buffer_available,
    };
    let Some((wait, _)) = step(f, ad(bb(10), false), &mut out) else {
        t.check(false);
        return;
    };
    t.check(wait.state() == Farm1State::Wait && wait.wait_flag() && wait.retransmit_flag());
    t.check(
        wait.step(bb(ad(10, true)), NOW, &mut out)
            .is_err_and(|e| e.kind() == ErrorKind::InvalidState),
    );
    let buffer = step(&wait, Farm1Input::BufferReleased, &mut out);
    t.check(buffer.is_some_and(|(g, _)| !g.wait_flag() && g.state() == Farm1State::Open));
    t.check(step(f, ad(bb(13), true), &mut out).is_some_and(|(g, _)| g.retransmit_flag()));
    t.check(step(f, ad(bb(8), true), &mut out).is_some_and(|(g, _)| !g.retransmit_flag()));
    let Some((lock, a)) = step(f, ad(bb(110), true), &mut out) else {
        t.check(false);
        return;
    };
    t.check(lock.state() == Farm1State::Lockout && lock.lockout_flag() && lock.clcw().lockout);
    t.check(matches!(
        a.get(0),
        Some(Farm1Output::Notify(Farm1Notify::FrameDiscarded {
            event: Farm1Event::E5,
            state: Farm1State::Open
        }))
    ));
    t.check(matches!(
        a.get(1),
        Some(Farm1Output::Notify(Farm1Notify::StateChanged {
            event: Farm1Event::E5,
            from: Farm1State::Open,
            to: Farm1State::Lockout
        }))
    ));
    let unlock = Farm1Input::Frame {
        frame: Farm1Frame::Bc {
            command: Some(ControlCommand::Unlock),
        },
        buffer_available: true,
    };
    t.check(step(&lock, unlock, &mut out).is_some_and(|(g, a)| {
        g.state() == Farm1State::Open
            && a.iter()
                .any(|x| *x == Farm1Output::Notify(Farm1Notify::Unlocked))
    }));
}

/// E6, E8, E9, and the physical flags.
fn other_events(t: &mut Tally, f: &Farm1) {
    let data = bb([1_u8]);
    let mut out = [0_u8; 8];
    let bd = Farm1Input::Frame {
        frame: Farm1Frame::Bd { data: &data },
        buffer_available: true,
    };
    t.check(step(f, bd, &mut out).is_some_and(|(g, _)| g.farm_b_counter().get() == 1));
    let set = Farm1Input::Frame {
        frame: Farm1Frame::Bc {
            command: Some(ControlCommand::SetVr(FrameSeqNum::new(bb(77)))),
        },
        buffer_available: true,
    };
    t.check(step(f, set, &mut out).is_some_and(|(g, _)| g.v_r().get() == 77));
    let unknown = Farm1Input::Frame {
        frame: Farm1Frame::Bc { command: None },
        buffer_available: true,
    };
    t.check(step(f, unknown, &mut out).is_some_and(|(g, a)| g == *f && a.len() == 1));
    t.check(step(f, Farm1Input::InvalidFrame, &mut out).is_some_and(|(_, a)| a.len() == 1));
    let flags = Farm1Input::SetPhysicalFlags {
        no_rf_available: bb(true),
        no_bit_lock: bb(false),
    };
    t.check(
        step(f, flags, &mut out).is_some_and(|(g, a)| g.clcw().no_rf_available && a.is_empty()),
    );
}

/// Every input kind, the queries, and the table.
pub fn run(t: &mut Tally) {
    window_group(t);
    tc_group(t);
    let (Some(vc_id), Some(status), Ok(window)) = (
        TcVcId::new(bb(5)),
        ClcwStatus::new(bb(3)),
        Farm1Window::retransmission_allowed(bb(16)),
    ) else {
        t.check(false);
        return;
    };
    let config = Farm1Config {
        vc_id,
        status,
        window,
    };
    let f = Farm1::new(config, FrameSeqNum::new(bb(10)));
    t.check(f.config() == config && f.state() == Farm1State::Open);
    t.check(f.v_r().get() == 10 && !f.lockout_flag() && !f.wait_flag() && !f.retransmit_flag());
    t.check(f.farm_b_counter().get() == 0);
    let c = f.clcw();
    t.check(
        c.cop_in_effect == CopInEffect::COP_1 && c.vc_id == vc_id && c.report_value.get() == 10,
    );
    accept_group(t, &f);
    window_events(t, &f);
    other_events(t, &f);
    t.check(FARM1_TABLE.len() == 3 && FARM1_MAX_ACTIONS == 4);
}

/// The `cop1/` vector group (CCSDS 232.1 engine scenarios, M3 WP-2). The directory holds no
/// scenario yet, so no case is generated; a case that did arrive fails until WP-2 gives the
/// group its runner (the engine-scenario schema is not a codec case).
pub const fn vector(_case: &Case) -> Run {
    Err(Fail::Vector)
}

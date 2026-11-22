//! Calls every public `aldis-cop1` FARM-1 API (VER-010; M3 WP-3). Inputs pass through
//! `black_box`.

use core::hint::black_box as bb;

use aldis_cop1::{
    Actions, FARM1_MAX_ACTIONS, FARM1_TABLE, Farm1, Farm1Config, Farm1Event, Farm1Frame,
    Farm1Input, Farm1Notify, Farm1Output, Farm1State, Farm1Window, FduKind, OutRange,
};
use aldis_core::DecodeMode;
use aldis_core::ids::FrameSeqNum;
use aldis_core::security::SecurityLayout;
use aldis_core::time::Instant;
use aldis_core::{ErrorKind, field};
use aldis_tc::{
    ClcwStatus, ControlCommand, CopInEffect, FrameData, FrameFields, FrameType,
    PhysicalChannelConfig, SegmentHeaderUse, TcSpacecraftId, TcVcId, VcConfig, VcTable,
    check_frame, encode,
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
        let Ok(frame) = check_frame(bb(octets), &table, DecodeMode::Strict) else {
            t.check(false);
            continue;
        };
        let mapped = Farm1Frame::from_tc(&frame);
        t.check(match frame_type {
            FrameType::Ad => {
                mapped.kind() == FrameType::Ad
                    && mapped.sequence_number() == Some(FrameSeqNum::new(7))
                    && mapped.data() == Some(&[1, 2, 3][..])
            }
            FrameType::Bd => mapped.kind() == FrameType::Bd && mapped.data() == Some(&[4, 5][..]),
            FrameType::Bc => {
                mapped.kind() == FrameType::Bc && mapped.control_command() == Some(set)
            }
        });
    }
}

/// A Type-AD, Type-BD, and Type-BC USLP frame mapped to FARM-1 inputs (`from_uslp`; #718).
fn uslp_group(t: &mut Tally) {
    use aldis_uslp as u;
    let (Some(scid), Some(vc), Some(map), Ok(count_len)) = (
        u::UslpSpacecraftId::new(bb(0x155)),
        u::UslpVcId::new(bb(5)),
        u::UslpMapId::new(bb(2)),
        u::VcfCountLen::new(bb(1)),
    ) else {
        t.check(false);
        return;
    };
    let params = u::VcParams {
        vcf_count_len_sequence: count_len,
        ..u::VcParams::new(u::TransferFrameType::Variable)
    };
    let (Ok(pc), Ok(vcc)) = (
        u::PhysicalChannelConfig::new(u::TransferFrameType::Variable, 64, 0, true),
        u::VcConfig::new(scid, vc, params),
    ) else {
        t.check(false);
        return;
    };
    let mc = u::MapConfig::new(scid, vc, map, SecurityLayout::NONE);
    let Ok(table) = u::VcTable::<1, 1>::new(pc, &[vcc], &[mc]) else {
        t.check(false);
        return;
    };
    let set = ControlCommand::SetVr(FrameSeqNum::new(bb(3)));
    let set_octets = set.to_octets();
    let cop1 = u::TfdfHeader {
        rule: u::TfdzRule::NoSegmentation,
        upid: u::Upid::Cop1ControlCommands,
    };
    let stream = u::TfdfHeader {
        rule: u::TfdzRule::NoSegmentation,
        upid: u::Upid::OctetStream,
    };
    let mut buf = [0u8; 64];
    for (kind, header, tfdz) in [
        (u::FrameKind::SequenceControlled, stream, &[1u8, 2, 3][..]),
        (u::FrameKind::Expedited, stream, &[4, 5][..]),
        (u::FrameKind::ProtocolControl, cop1, set_octets.as_slice()),
        (u::FrameKind::ProtocolControl, stream, set_octets.as_slice()),
    ] {
        let fields = u::FrameFields {
            spacecraft_id: scid,
            source_or_destination: u::SourceOrDestination::Destination,
            vc_id: vc,
            map_id: map,
            kind,
            vcf_count: if kind == u::FrameKind::SequenceControlled {
                bb(7)
            } else {
                bb(0)
            },
        };
        let content = u::FrameContent {
            tfdf: header,
            insert_zone: &[],
            ocf: None,
        };
        let Ok(n) = u::encode(&table, fields, content, bb(tfdz), &mut buf) else {
            t.check(false);
            continue;
        };
        let octets = buf.get(..n).unwrap_or_default();
        let Ok(checked) = u::check_frame(bb(octets), &table, DecodeMode::Strict) else {
            t.check(false);
            continue;
        };
        let checked = checked.into_view();
        check_uslp_farm1(t, kind, header.upid, set, &checked);
    }
}

/// Checks `Farm1Frame::from_uslp` and `Farm1Input::from_uslp` on one checked USLP frame.
fn check_uslp_farm1(
    t: &mut Tally,
    kind: aldis_uslp::FrameKind,
    upid: aldis_uslp::Upid,
    set: ControlCommand,
    checked: &aldis_core::Checked<aldis_uslp::UslpFrame<'_>>,
) {
    use aldis_uslp as u;
    let is_bad_bc = kind == u::FrameKind::ProtocolControl && upid != u::Upid::Cop1ControlCommands;
    let is_reserved_cmd = |e: aldis_core::Error| {
        e.kind() == ErrorKind::ReservedValue && e.field() == Some(field::CONTROL_COMMAND)
    };
    let frame = Farm1Frame::from_uslp(checked);
    t.check(match (kind, is_bad_bc, frame) {
        (u::FrameKind::SequenceControlled, _, Ok(f)) => {
            f.kind() == FrameType::Ad && f.sequence_number() == Some(FrameSeqNum::new(7))
        }
        (u::FrameKind::Expedited, _, Ok(f)) => f.kind() == FrameType::Bd,
        (u::FrameKind::ProtocolControl, false, Ok(f)) => {
            f.kind() == FrameType::Bc && f.control_command() == Some(set)
        }
        (u::FrameKind::ProtocolControl, true, Err(e)) => is_reserved_cmd(e),
        _ => false,
    });
    let input = Farm1Input::from_uslp(checked, bb(true));
    t.check(match (is_bad_bc, input) {
        (true, Err(e)) => is_reserved_cmd(e),
        (false, Ok(Farm1Input::Frame(f))) => f.vc_id().get() == 5 && f.buffer_available(),
        _ => false,
    });
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

/// Steps `f` with a checked TC frame built from raw fields (no FECF, lenient decode).
fn step_tc(
    f: &Farm1,
    frame_type: FrameType,
    vc_id: TcVcId,
    n_s: u8,
    payload: &[u8],
    buffer_available: bool,
    out: &mut [u8],
) -> aldis_core::Result<Step> {
    let mut wire = [0_u8; 32];
    let total = 5usize.saturating_add(payload.len());
    let flags = match frame_type {
        FrameType::Ad => 0x00,
        FrameType::Bd => 0x20,
        FrameType::Bc => 0x30,
    };
    let len_minus_1 = u16::try_from(total.saturating_sub(1))
        .map_err(|_| aldis_core::Error::new(ErrorKind::InternalError))?;
    let vc_len = (u16::from(vc_id.get()) << 10) | len_minus_1;
    let [hi, lo] = vc_len.to_be_bytes();
    if let Some(hdr) = wire.get_mut(..5) {
        hdr.copy_from_slice(&[flags, 0, hi, lo, n_s]);
    }
    if let Some(dst) = wire.get_mut(5..total) {
        dst.copy_from_slice(payload);
    }
    let scid = TcSpacecraftId::new(0).ok_or(aldis_core::Error::new(ErrorKind::InternalError))?;
    let pc = PhysicalChannelConfig::new(1024, false)?;
    let vcc = VcConfig::new(
        scid,
        vc_id,
        1024,
        SegmentHeaderUse::Absent(SecurityLayout::NONE),
    )?;
    let table = VcTable::<1, 0>::new(pc, &[vcc], &[])?;
    let slice = wire
        .get(..total)
        .ok_or(aldis_core::Error::new(ErrorKind::InternalError))?;
    let checked = check_frame(bb(slice), &table, DecodeMode::Lenient).map_err(|r| r.error())?;
    f.step(
        bb(Farm1Input::from_tc(&checked, buffer_available)),
        NOW,
        out,
    )
}

/// The accepted FDU: E1.
fn accept_group(t: &mut Tally, f: &Farm1) {
    let data = bb([0xA1_u8, 0xB2, 0xC3]);
    let mut out = [0_u8; 8];
    let Ok((next, acts)) = step_tc(
        f,
        FrameType::Ad,
        f.config().vc_id,
        bb(10),
        &data,
        bb(true),
        &mut out,
    ) else {
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
        step_tc(
            f,
            FrameType::Ad,
            f.config().vc_id,
            bb(10),
            &data,
            bb(true),
            short,
        )
        .is_err_and(|e| e.kind() == ErrorKind::BufferTooSmall),
    );
}

/// E2 to E5 from S1, and the cell Table 6-1 marks "Not applicable".
fn window_events(t: &mut Tally, f: &Farm1) {
    let data = bb([1_u8]);
    let mut out = [0_u8; 8];
    let ad = |farm: &Farm1, n: u8, buffer_available: bool, out: &mut [u8]| {
        step_tc(
            farm,
            FrameType::Ad,
            f.config().vc_id,
            n,
            &data,
            buffer_available,
            out,
        )
    };
    let Ok((wait, _)) = ad(f, bb(10), false, &mut out) else {
        t.check(false);
        return;
    };
    t.check(wait.state() == Farm1State::Wait && wait.wait_flag() && wait.retransmit_flag());
    t.check(ad(&wait, 10, true, &mut out).is_err_and(|e| e.kind() == ErrorKind::InvalidState));
    let buffer = step(&wait, Farm1Input::BufferReleased, &mut out);
    t.check(buffer.is_some_and(|(g, _)| !g.wait_flag() && g.state() == Farm1State::Open));
    t.check(ad(f, bb(13), true, &mut out).is_ok_and(|(g, _)| g.retransmit_flag()));
    t.check(ad(f, bb(8), true, &mut out).is_ok_and(|(g, _)| !g.retransmit_flag()));
    let Ok((lock, a)) = ad(f, bb(110), true, &mut out) else {
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
    let unlock = ControlCommand::Unlock.to_octets();
    t.check(
        step_tc(
            &lock,
            FrameType::Bc,
            f.config().vc_id,
            0,
            unlock.as_slice(),
            true,
            &mut out,
        )
        .is_ok_and(|(g, a)| {
            g.state() == Farm1State::Open
                && a.iter()
                    .any(|x| *x == Farm1Output::Notify(Farm1Notify::Unlocked))
        }),
    );
}

/// E6, E8, E9, and the physical flags.
fn other_events(t: &mut Tally, f: &Farm1) {
    let data = bb([1_u8]);
    let mut out = [0_u8; 8];
    t.check(
        step_tc(f, FrameType::Bd, f.config().vc_id, 0, &data, true, &mut out)
            .is_ok_and(|(g, _)| g.farm_b_counter().get() == 1),
    );
    let set = ControlCommand::SetVr(FrameSeqNum::new(bb(77))).to_octets();
    t.check(
        step_tc(
            f,
            FrameType::Bc,
            f.config().vc_id,
            0,
            set.as_slice(),
            true,
            &mut out,
        )
        .is_ok_and(|(g, _)| g.v_r().get() == 77),
    );
    t.check(
        step_tc(
            f,
            FrameType::Bc,
            f.config().vc_id,
            0,
            &[0xFF],
            true,
            &mut out,
        )
        .is_ok_and(|(g, a)| g == *f && a.len() == 1),
    );
    t.check(step(f, Farm1Input::InvalidFrame, &mut out).is_some_and(|(_, a)| a.len() == 1));
    // A frame of another Virtual Channel is refused (COP-037).
    let other = TcVcId::new(bb(6));
    t.check(other.is_some_and(|vc_id| {
        step_tc(f, FrameType::Bc, vc_id, 0, &[0xFF], true, &mut out)
            .is_err_and(|e| e.kind() == ErrorKind::UnknownChannel)
    }));
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
    uslp_group(t);
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
    t.check(FARM1_TABLE.len() == 3 && FARM1_MAX_ACTIONS == 3);
}

/// The `cop1/` vector group (CCSDS 232.1 engine scenarios, M3 WP-2). The directory holds no
/// scenario yet, so no case is generated; a case that did arrive fails until WP-2 gives the
/// group its runner (the engine-scenario schema is not a codec case).
pub const fn vector(_case: &Case) -> Run {
    Err(Fail::Vector)
}

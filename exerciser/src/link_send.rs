//! Calls every public `aldis-link` API of the sending engines of M4 WP-4 (VER-010): the
//! `ChannelMux` for TM, AOS, and USLP with its policies, slots, OID frames, and refusals, the
//! `AccessGenerator` for TM, AOS, and USLP, and the `BitstreamGenerator`. Inputs pass through
//! `black_box`. The `link/` vector files are engine scenarios the host suites run.

use core::hint::black_box as bb;

use aldis_aos::AosEdition;
use aldis_core::ErrorKind;
use aldis_core::ids::{MapId, SpacecraftId, VcId};
use aldis_core::security::SecurityLayout;
use aldis_link::access::{
    ACCESS_GENERATOR_MAX_ACTIONS, AccessGenerator, AccessGeneratorConfig, AccessInput, AccessOutput,
};
use aldis_link::bitstream::{
    BITSTREAM_GENERATOR_MAX_ACTIONS, BitstreamGenerator, BitstreamGeneratorConfig, BitstreamInput,
    BitstreamOutput,
};
use aldis_link::common::Qos;
use aldis_link::mux::{
    AosChannel, AosVc, CHANNEL_MUX_MAX_ACTIONS, ChannelMux, ChannelMuxConfig, Cop, FrameClass, Fsh,
    MAX_FSH_DATA, MuxInput, MuxOutput, Offer, Scope, TmChannel, TmRole, TmVc, UslpChannel, UslpVc,
    VcConfig,
};
use aldis_link::policy::{MuxPolicy, Selector, Weight};
use aldis_link::protocol::{Aos, Route, Tm, Uslp, UslpParams};
use aldis_uslp::{SourceOrDestination, Upid, VcfCountLen};

use crate::exercise::Tally;

/// The frame and the field of the engines.
const F: usize = 48;

const fn policy_of(i: usize) -> MuxPolicy {
    match i {
        0 => MuxPolicy::FixedPriority,
        1 => MuxPolicy::RoundRobin,
        _ => MuxPolicy::WeightedRoundRobin,
    }
}

/// A TM mux under each policy: data, OID, slots, refusals, and `Reconfigure`.
#[expect(
    clippy::too_many_lines,
    reason = "one straight-line call of every public API of the TM mux"
)]
fn tm(t: &mut Tally, policy: MuxPolicy) {
    let (Some(scid), Some(oid)) = (SpacecraftId::<10>::new(bb(165)), VcId::<3>::new(bb(7))) else {
        t.check(false);
        return;
    };
    let (Some(v1), Some(v3), Some(weight)) = (
        VcId::<3>::new(bb(1)),
        VcId::<3>::new(bb(3)),
        Weight::new(bb(2)).ok(),
    ) else {
        t.check(false);
        return;
    };
    let Ok(security) = SecurityLayout::new(bb(2), bb(1)) else {
        t.check(false);
        return;
    };
    let Ok(fsh) = Fsh::new(&[bb(1), 2]) else {
        t.check(false);
        return;
    };
    let mut channel = TmChannel::new(scid, oid);
    channel.mc_ocf = Some([0; 4]);
    let mut v1_params = TmVc::new(TmRole::Packets);
    v1_params.fsh = Some(fsh);
    let vcs = [
        VcConfig::<Tm>::new(v1, v1_params)
            .with_priority(bb(2))
            .with_weight(weight)
            .with_security(security),
        VcConfig::<Tm>::new(v3, TmVc::new(TmRole::Vca)).with_priority(bb(1)),
        VcConfig::<Tm>::new(oid, TmVc::new(TmRole::Packets)),
    ];
    let Ok(config) = ChannelMuxConfig::<Tm, 4>::new(channel, bb(F), true, policy, &vcs) else {
        t.check(false);
        return;
    };
    t.check(config.frame_len() == F && config.fecf() && config.policy() == policy);
    t.check(config.vc_count() == 3 && config.insert_zone_len() == 0);
    t.check(config.channel().scid == scid);
    t.check(config.field_len(1, Qos::SequenceControlled) == Some(F - 6 - 3 - 2 - 1 - 4 - 2));
    t.check(config.field_len(bb(5), Qos::Expedited).is_none());
    t.check(fsh.len() == 2 && !fsh.is_empty() && fsh.octets().count() == 2 && MAX_FSH_DATA == 63);
    let vc = vcs.first();
    t.check(vc.is_some_and(|v| {
        v.vc() == v1
            && v.priority() == 2
            && v.weight() == weight
            && v.security() == security
            && v.params().role == TmRole::Packets
    }));
    let Ok(mut mux) = ChannelMux::<Tm, 4, F>::new(config) else {
        t.check(false);
        return;
    };
    let mut out = [0u8; F];
    t.check(mux.config() == &config);
    let field = [bb(9u8); F];
    let offer = |vc: u8, len: usize, pointer: Option<u16>| Offer {
        vc,
        field: field.get(..len).unwrap_or_default(),
        pointer,
        route: None,
        cop: None,
    };
    let kinds = |mux: &mut ChannelMux<Tm, 4, F>, input, out: &mut [u8; F]| {
        mux.step(input, out).err().map(|e| e.kind())
    };
    let fsh_new = [bb(5u8), 6];
    t.check(kinds(&mut mux, MuxInput::Offer(offer(1, 30, Some(0))), &mut out).is_none());
    t.check(
        kinds(&mut mux, MuxInput::Offer(offer(1, 30, Some(0))), &mut out)
            == Some(ErrorKind::WouldBlock),
    );
    t.check(
        kinds(&mut mux, MuxInput::Offer(offer(1, 3, Some(0))), &mut out)
            == Some(ErrorKind::LengthMismatch),
    );
    t.check(
        kinds(&mut mux, MuxInput::Offer(offer(6, 3, Some(0))), &mut out)
            == Some(ErrorKind::UnknownChannel),
    );
    t.check(kinds(&mut mux, MuxInput::Offer(offer(3, 36, None)), &mut out).is_none());
    t.check(
        kinds(
            &mut mux,
            MuxInput::SetOcf {
                scope: Scope::Mc,
                ocf: [bb(1); 4],
            },
            &mut out,
        )
        .is_none(),
    );
    t.check(
        kinds(
            &mut mux,
            MuxInput::SetOcf {
                scope: Scope::Vc(bb(1)),
                ocf: [1; 4],
            },
            &mut out,
        )
        .is_some(),
    );
    t.check(
        kinds(
            &mut mux,
            MuxInput::SetFsh {
                scope: Scope::Vc(bb(1)),
                data: &fsh_new,
            },
            &mut out,
        )
        .is_none(),
    );
    t.check(
        kinds(
            &mut mux,
            MuxInput::SetFsh {
                scope: Scope::Mc,
                data: &[1],
            },
            &mut out,
        )
        .is_some(),
    );
    t.check(kinds(&mut mux, MuxInput::SetInsert(&[1]), &mut out).is_some());
    for _ in 0..3 {
        let Ok(actions) = mux.step(MuxInput::Opportunity, &mut out) else {
            t.check(false);
            continue;
        };
        t.check(!actions.is_empty() && actions.len() <= CHANNEL_MUX_MAX_ACTIONS);
        for a in actions.iter() {
            if let MuxOutput::Transmit(x) = a {
                let in_frame = x.frame.slice(&out).is_some()
                    && x.security_header.end() <= x.data.start()
                    && x.data.end() <= x.security_trailer.start();
                t.check(in_frame && matches!(x.class, FrameClass::Data | FrameClass::Oid));
            }
        }
    }
    let mut short = [0u8; 4];
    t.check(kinds_short(&mut mux, &mut short) == Some(ErrorKind::BufferTooSmall));
    let s = mux.status();
    t.check(s.frames_out.get() == 3 && s.oid_frames.get() >= 1 && s.offers.get() == 2);
    t.check(s.refused.get() == 2 && s.would_block.get() == 1 && s.insert_repeated.get() == 0);
    t.check(s.slots.high_water() >= s.slots.current() && s.discarded_octets.get() == 0);
    t.check(
        mux.step(MuxInput::Offer(offer(3, 36, None)), &mut out)
            .is_ok_and(|a| a.is_empty()),
    );
    t.check(
        mux.step(MuxInput::Reconfigure(config), &mut out)
            .is_ok_and(|a| a.get(0) == Some(&MuxOutput::Discarded { octets: 36 })),
    );
    t.check(CHANNEL_MUX_MAX_ACTIONS == 2);
}

fn kinds_short(mux: &mut ChannelMux<Tm, 4, F>, short: &mut [u8; 4]) -> Option<ErrorKind> {
    mux.step(MuxInput::Opportunity, short)
        .err()
        .map(|e| e.kind())
}

/// An AOS mux with an Insert Zone, an FHEC, and the cycle, and its OID frames.
fn aos(t: &mut Tally) {
    let (Some(scid), Some(v1)) = (SpacecraftId::<10>::new(bb(421)), VcId::<6>::new(bb(1))) else {
        t.check(false);
        return;
    };
    let Ok(channel) = AosChannel::new(scid, AosEdition::Issue5, bb(true), bb(true)) else {
        t.check(false);
        return;
    };
    t.check(channel.scid() == scid && channel.edition() == AosEdition::Issue5);
    t.check(channel.fhec() && channel.cycle_in_use());
    t.check(SpacecraftId::<10>::new(bb(256)).is_some_and(|big| {
        AosChannel::new(big, AosEdition::Issue4Compatible, false, false).is_err()
    }));
    let vcs = [VcConfig::<Aos>::new(
        v1,
        AosVc {
            ocf: Some([bb(1); 4]),
        },
    )];
    let config = ChannelMuxConfig::<Aos, 2>::new(channel, F, true, MuxPolicy::RoundRobin, &vcs)
        .and_then(|c| c.with_insert_zone(bb(3), bb(0xA5)));
    let Ok(config) = config else {
        t.check(false);
        return;
    };
    t.check(config.insert_zone_len() == 3);
    let Ok(mut mux) = ChannelMux::<Aos, 2, F>::new(config) else {
        t.check(false);
        return;
    };
    let mut out = [0u8; F];
    let data = [bb(4u8); F];
    let len = F - 8 - 2 - 3 - 4;
    let offer = Offer {
        vc: 1,
        field: data.get(..len).unwrap_or_default(),
        pointer: None,
        route: None,
        cop: None,
    };
    t.check(mux.step(MuxInput::Offer(offer), &mut out).is_ok());
    t.check(
        mux.step(MuxInput::SetInsert(&[bb(1), 2, 3]), &mut out)
            .is_ok(),
    );
    t.check(mux.step(MuxInput::SetInsert(&[1]), &mut out).is_err());
    t.check(
        mux.step(
            MuxInput::SetOcf {
                scope: Scope::Vc(1),
                ocf: [bb(2); 4],
            },
            &mut out,
        )
        .is_ok(),
    );
    // The frame with a fresh IN_SDU, then an OID frame whose Insert Zone repeats.
    t.check(
        mux.step(MuxInput::Opportunity, &mut out)
            .is_ok_and(|a| a.len() == 1),
    );
    t.check(
        mux.step(MuxInput::Opportunity, &mut out)
            .is_ok_and(|a| a.len() == 2 && a.get(1) == Some(&MuxOutput::InsertSduRepeated)),
    );
    t.check(mux.status().insert_repeated.get() == 1);
    t.check(mux.config().channel().cycle_in_use());
}

/// A USLP mux with COP-1 on a VC: the `cop` block, routes, and the OID frame with the MC's OCF.
fn uslp(t: &mut Tally) {
    let (Some(scid), Some(v1), Some(map)) = (
        SpacecraftId::<16>::new(bb(0x1234)),
        VcId::<6>::new(bb(1)),
        MapId::<4>::new(bb(3)),
    ) else {
        t.check(false);
        return;
    };
    let (Ok(sc), Ok(exp)) = (VcfCountLen::new(bb(2)), VcfCountLen::new(bb(1))) else {
        t.check(false);
        return;
    };
    let channel = UslpChannel {
        scid,
        source_or_destination: SourceOrDestination::Source,
        oid_vcf_len: VcfCountLen::NONE,
        mc_ocf: Some([bb(7); 4]),
    };
    let params = UslpVc {
        map,
        qos: Qos::SequenceControlled,
        vcf_len_sc: sc,
        vcf_len_exp: exp,
        cop: bb(true),
    };
    let vcs = [VcConfig::<Uslp>::new(v1, params)];
    let Ok(config) =
        ChannelMuxConfig::<Uslp, 2>::new(channel, F, true, MuxPolicy::FixedPriority, &vcs)
    else {
        t.check(false);
        return;
    };
    let Ok(mut mux) = ChannelMux::<Uslp, 2, F>::new(config) else {
        t.check(false);
        return;
    };
    let mut out = [0u8; F];
    let data = [bb(5u8); F];
    let len = config.field_len(1, Qos::Expedited).unwrap_or(0);
    let offer = Offer {
        vc: 1,
        field: data.get(..len).unwrap_or_default(),
        pointer: None,
        route: Some(Route {
            map,
            qos: Qos::Expedited,
        }),
        cop: Cop::new(bb(0), true, bb(true)).ok(),
    };
    t.check(Cop::new(bb(0), false, bb(true)).is_err());
    t.check(
        Cop::TypeAd { vcf_count: bb(4) }.vcf_count() == Some(4) && !Cop::TypeBd.protocol_control(),
    );
    t.check(mux.step(MuxInput::Offer(offer), &mut out).is_ok());
    let bad_map = MapId::<4>::new(bb(2)).map(|m| Route {
        map: m,
        qos: Qos::Expedited,
    });
    let wrong = Offer {
        route: bad_map,
        ..offer
    };
    t.check(
        mux.step(MuxInput::Offer(wrong), &mut out)
            .err()
            .map(|e| e.kind())
            == Some(ErrorKind::UnknownChannel),
    );
    t.check(
        mux.step(MuxInput::Opportunity, &mut out)
            .is_ok_and(|a| a.len() == 1),
    );
    t.check(
        mux.step(MuxInput::Opportunity, &mut out)
            .is_ok_and(|a| a.len() == 1),
    );
    t.check(mux.status().oid_frames.get() == 1);
}

/// The policies on their own.
fn policies(t: &mut Tally) {
    for i in 0..3 {
        let mut s = Selector::new();
        let policy = policy_of(i);
        let two = Weight::new(bb(2)).unwrap_or(Weight::ONE);
        let weight = move |_: usize| two;
        let first = s.select(
            policy,
            3,
            |p| p != 0,
            |p| u8::try_from(p).unwrap_or(0),
            weight,
        );
        t.check(first == Some(2) || first == Some(1));
        t.check(s.select(policy, 3, |_| false, |_| 0, weight).is_none());
        s.forfeit();
    }
    t.check(Weight::new(bb(0)).is_err() && Weight::ONE.get() == 1);
    t.check(MuxPolicy::default() == MuxPolicy::FixedPriority);
    t.check(Selector::default() == Selector::new());
}

/// The Access generator for each protocol.
fn access(t: &mut Tally) {
    let (Some(v3), Some(v6), Some(map)) = (
        VcId::<3>::new(bb(1)),
        VcId::<6>::new(bb(2)),
        MapId::<4>::new(bb(1)),
    ) else {
        t.check(false);
        return;
    };
    let mut out = [0u8; 16];
    if let Ok(c) = AccessGeneratorConfig::<Tm>::new(v3, (), bb(8)) {
        let c = c.with_idle_pattern(bb(0xA5));
        t.check(c.vc() == v3 && c.field_len() == 8 && c.idle_pattern() == 0xA5);
        t.check(c.max_sdu_len() == 8 && c.with_max_sdu_len(bb(7)).is_err());
        if let Ok(mut g) = AccessGenerator::<Tm, 32>::new(c) {
            t.check(g.config() == &c);
            let sdu = [bb(3u8); 8];
            t.check(g.step(AccessInput::SubmitSdu(&sdu), &mut out).is_ok());
            t.check(g.step(AccessInput::SubmitSdu(&sdu[..4]), &mut out).is_err());
            t.check(g.step(AccessInput::Release, &mut out).is_ok_and(|a| {
                matches!(a.get(0), Some(AccessOutput::DataField(f)) if f.pointer.is_none() && f.route.is_none())
            }));
            t.check(g.step(AccessInput::Reconfigure(c), &mut out).is_ok());
            t.check(g.status().sdus_in.get() == 1 && g.status().fields_out.get() == 1);
        } else {
            t.check(false);
        }
    } else {
        t.check(false);
    }
    if let Ok(c) = AccessGeneratorConfig::<Aos>::new(VcId::<6>::new(bb(1)).unwrap_or(v6), (), bb(8))
    {
        t.check(AccessGenerator::<Aos, 4>::new(c).is_err());
        t.check(AccessGenerator::<Aos, 16>::new(c).is_ok());
    } else {
        t.check(false);
    }
    let Ok(params) = UslpParams::new(Upid::Packets, map, Qos::Expedited) else {
        t.check(false);
        return;
    };
    let Ok(c) = AccessGeneratorConfig::<Uslp>::new(v6, params, bb(8))
        .and_then(|c| c.with_max_sdu_len(bb(12)))
    else {
        t.check(false);
        return;
    };
    let Ok(mut g) = AccessGenerator::<Uslp, 32>::new(c) else {
        t.check(false);
        return;
    };
    let sdu = [bb(6u8); 12];
    t.check(g.step(AccessInput::SubmitSdu(&sdu), &mut out).is_ok());
    t.check(g.step(AccessInput::Release, &mut out).is_ok_and(|a| {
        matches!(a.get(0), Some(AccessOutput::DataField(f)) if f.pointer == Some(0xFFFF) && f.route.is_some())
    }));
    t.check(g.step(AccessInput::Release, &mut out).is_ok_and(
        |a| matches!(a.get(0), Some(AccessOutput::DataField(f)) if f.pointer == Some(0xFFFF)),
    ));
    t.check(g.step(AccessInput::Release, &mut out).is_ok_and(
        |a| matches!(a.get(0), Some(AccessOutput::DataField(f)) if f.pointer == Some(1)),
    ));
    t.check(g.step(AccessInput::SubmitSdu(&[]), &mut out).is_err());
    t.check(g.step(AccessInput::SubmitSdu(&sdu), &mut out).is_ok());
    t.check(
        g.step(AccessInput::Reconfigure(c), &mut out)
            .is_ok_and(|a| a.len() == 1),
    );
    t.check(g.status().discarded_octets.get() == 12 && g.status().would_block.get() == 0);
    t.check(ACCESS_GENERATOR_MAX_ACTIONS == 1);
}

/// The Bitstream generator.
fn bitstream(t: &mut Tally) {
    let Some(vc) = VcId::<6>::new(bb(3)) else {
        t.check(false);
        return;
    };
    let Ok(config) = BitstreamGeneratorConfig::<Aos>::new(vc, bb(6), bb(0xFF)) else {
        t.check(false);
        return;
    };
    t.check(config.vc() == vc && config.field_len() == 6 && config.idle_pattern() == 0xFF);
    t.check(BitstreamGeneratorConfig::<Aos>::new(vc, bb(2), 0).is_err());
    let mut g = BitstreamGenerator::<Aos, 8>::new(config);
    let mut out = [0u8; 6];
    let bits = [bb(0xA0u8)];
    t.check(
        g.step(
            BitstreamInput::SubmitBits {
                bits: &bits,
                bit_len: bb(3),
            },
            &mut out,
        )
        .is_ok(),
    );
    t.check(
        g.step(
            BitstreamInput::SubmitBits {
                bits: &bits,
                bit_len: bb(9),
            },
            &mut out,
        )
        .is_err(),
    );
    t.check(
        g.step(BitstreamInput::Release, &mut out).is_ok_and(
            |a| matches!(a.get(0), Some(BitstreamOutput::DataField(f)) if f.pointer == 2),
        ),
    );
    t.check(g.step(BitstreamInput::Release, &mut out).is_ok_and(
        |a| matches!(a.get(0), Some(BitstreamOutput::DataField(f)) if f.pointer == 0x3FFE),
    ));
    t.check(
        g.step(
            BitstreamInput::SubmitBits {
                bits: &[bb(1); 4],
                bit_len: 32,
            },
            &mut out,
        )
        .is_ok(),
    );
    t.check(g.step(BitstreamInput::Release, &mut out).is_ok_and(
        |a| matches!(a.get(0), Some(BitstreamOutput::DataField(f)) if f.pointer == 0x3FFF),
    ));
    t.check(
        g.step(
            BitstreamInput::SubmitBits {
                bits: &bits,
                bit_len: 8,
            },
            &mut out,
        )
        .is_ok(),
    );
    t.check(
        g.step(BitstreamInput::Reconfigure(config), &mut out)
            .is_ok_and(|a| a.get(0) == Some(&BitstreamOutput::Discarded { octets: 1 })),
    );
    let s = g.status();
    t.check(s.fields_out.get() == 3 && s.idle_fields.get() == 1 && s.refused.get() == 1);
    t.check(g.config() == &config && BITSTREAM_GENERATOR_MAX_ACTIONS == 1);
}

/// Runs the sending engines.
pub fn run(t: &mut Tally) {
    for i in 0..3 {
        tm(t, policy_of(i));
    }
    aos(t);
    uslp(t);
    policies(t);
    access(t);
    bitstream(t);
}

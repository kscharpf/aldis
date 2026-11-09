//! Configuration, refusals, reconfiguration, bounds, and housekeeping of the `ChannelMux` on the
//! shared link types, and a property test of its frames (LNK-002, LNK-003, LNK-030…LNK-036,
//! LNK-074…LNK-078, LNK-082, API-052, API-056, API-058, OI-78).
#![expect(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::too_many_lines,
    unused_must_use,
    reason = "tests fail loudly by design, and many steps are run for their effect on the state"
)]

use aldis_aos::AosEdition;
use aldis_core::ids::{MapId, SpacecraftId, VcId};
use aldis_core::security::SecurityLayout;
use aldis_core::{ErrorKind, field};
use aldis_link::common::Qos;
use aldis_link::mux::{
    AosChannel, AosVc, CHANNEL_MUX_MAX_ACTIONS, ChannelMux, ChannelMuxConfig, Cop, FrameClass, Fsh,
    MuxInput, MuxOutput, Offer, Scope, TmChannel, TmRole, TmVc, UslpChannel, UslpVc, VcConfig,
};
use aldis_link::policy::{MuxPolicy, Weight};
use aldis_link::protocol::{Aos, Route, Tm, Uslp};
use aldis_testkit::strategies;
use aldis_testkit::strategies::proptest::prelude::*;
use aldis_uslp::{SourceOrDestination, VcfCountLen};

/// Failing cases are saved here and replayed first.
const REGRESSIONS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/proptest-regressions/channel_mux_engine.txt"
);

const V: usize = 8;
const F: usize = 64;

type TmMux = ChannelMux<Tm, V, F>;
type AosMux = ChannelMux<Aos, V, F>;
type UslpMux = ChannelMux<Uslp, V, F>;

const fn tm_vc(id: u8, role: TmRole, priority: u8) -> VcConfig<Tm> {
    VcConfig::<Tm>::new(VcId::<3>::new(id).unwrap(), TmVc::new(role)).with_priority(priority)
}

fn tm_vcs() -> Vec<VcConfig<Tm>> {
    vec![
        tm_vc(1, TmRole::Packets, 1),
        tm_vc(2, TmRole::Packets, 3),
        tm_vc(3, TmRole::Vca, 2),
        tm_vc(7, TmRole::Packets, 0),
    ]
}

const fn tm_channel() -> TmChannel {
    TmChannel::new(
        SpacecraftId::<10>::new(165).unwrap(),
        VcId::<3>::new(7).unwrap(),
    )
}

fn tm_config(policy: MuxPolicy) -> ChannelMuxConfig<Tm, V> {
    ChannelMuxConfig::new(tm_channel(), 24, true, policy, &tm_vcs()).unwrap()
}

fn aos_channel() -> AosChannel {
    AosChannel::new(
        SpacecraftId::<10>::new(421).unwrap(),
        AosEdition::Issue5,
        false,
        false,
    )
    .unwrap()
}

fn aos_vc(id: u8, priority: u8) -> VcConfig<Aos> {
    VcConfig::<Aos>::new(VcId::<6>::new(id).unwrap(), AosVc::default()).with_priority(priority)
}

fn aos_config() -> ChannelMuxConfig<Aos, V> {
    ChannelMuxConfig::new(
        aos_channel(),
        28,
        true,
        MuxPolicy::FixedPriority,
        &[aos_vc(1, 1), aos_vc(2, 2)],
    )
    .unwrap()
}

fn uslp_vc(id: u8, cop: bool) -> VcConfig<Uslp> {
    VcConfig::<Uslp>::new(
        VcId::<6>::new(id).unwrap(),
        UslpVc::new(
            MapId::<4>::new(3).unwrap(),
            Qos::SequenceControlled,
            VcfCountLen::new(2).unwrap(),
            VcfCountLen::new(1).unwrap(),
            cop,
        )
        .unwrap(),
    )
    .with_priority(id)
}

const fn uslp_channel() -> UslpChannel {
    UslpChannel {
        scid: SpacecraftId::<16>::new(0x1234).unwrap(),
        source_or_destination: SourceOrDestination::Source,
        oid_vcf_len: VcfCountLen::NONE,
        mc_ocf: None,
    }
}

fn uslp_config(cop: bool) -> ChannelMuxConfig<Uslp, V> {
    ChannelMuxConfig::new(
        uslp_channel(),
        32,
        true,
        MuxPolicy::FixedPriority,
        &[uslp_vc(1, cop), uslp_vc(2, cop)],
    )
    .unwrap()
}

/// The kind and the field name of a failed result.
fn failure<T: std::fmt::Debug>(
    r: Result<T, aldis_core::Error>,
) -> (ErrorKind, Option<&'static str>) {
    let e = r.unwrap_err();
    (e.kind(), e.field().map(aldis_core::FieldId::name))
}

fn kind_of<T: std::fmt::Debug>(r: Result<T, aldis_core::Error>) -> ErrorKind {
    r.unwrap_err().kind()
}

const fn named(k: ErrorKind, f: aldis_core::FieldId) -> (ErrorKind, Option<&'static str>) {
    (k, Some(f.name()))
}

const fn offer(vc: u8, field: &[u8], pointer: Option<u16>) -> Offer<'_> {
    Offer {
        vc,
        field,
        pointer,
        route: None,
        cop: None,
    }
}

#[test]
fn a_configuration_the_blue_books_forbid_is_refused() {
    // req: LNK-078, LNK-031, API-040, API-071
    let invalid = |f| named(ErrorKind::InvalidConfig, f);
    let tm = |vcs: &[VcConfig<Tm>], channel: TmChannel, frame_len, policy| {
        ChannelMuxConfig::<Tm, V>::new(channel, frame_len, true, policy, vcs)
    };
    let fixed = MuxPolicy::FixedPriority;
    // More VCs than the capacity.
    let many: Vec<_> = (0..8).map(|i| tm_vc(i, TmRole::Packets, i)).collect();
    assert!(tm(&many, tm_channel(), 24, fixed).is_ok());
    assert_eq!(
        failure(ChannelMuxConfig::<Tm, 3>::new(
            tm_channel(),
            24,
            true,
            fixed,
            &tm_vcs()
        )),
        named(ErrorKind::CapacityExceeded, field::CAPACITY)
    );
    // A VCID twice.
    let mut twice = tm_vcs();
    twice.push(tm_vc(2, TmRole::Packets, 9));
    assert_eq!(
        failure(tm(&twice, tm_channel(), 24, fixed)),
        invalid(field::VC_ID)
    );
    // Equal priorities under fixed priority, and under no other policy.
    let mut equal = tm_vcs();
    equal.push(tm_vc(4, TmRole::Packets, 3));
    assert_eq!(
        failure(tm(&equal, tm_channel(), 24, fixed)),
        invalid(field::VC_PRIORITY)
    );
    assert!(tm(&equal, tm_channel(), 24, MuxPolicy::RoundRobin).is_ok());
    assert!(tm(&equal, tm_channel(), 24, MuxPolicy::WeightedRoundRobin).is_ok());
    // The VC dedicated to OID frames: configured, Packets, no SDLS (132.0 §4.1.4.6.3, §6.4.6.2).
    let without: Vec<_> = tm_vcs().into_iter().filter(|v| v.vc().get() != 7).collect();
    assert_eq!(
        failure(tm(&without, tm_channel(), 24, fixed)),
        invalid(field::VC_ID)
    );
    let mut vca_oid = tm_vcs();
    vca_oid[3] = tm_vc(7, TmRole::Vca, 0);
    assert_eq!(
        failure(tm(&vca_oid, tm_channel(), 24, fixed)),
        invalid(field::IDLE_POLICY)
    );
    let mut sdls_oid = tm_vcs();
    sdls_oid[3] = tm_vc(7, TmRole::Packets, 0).with_security(SecurityLayout::new(2, 0).unwrap());
    assert_eq!(
        failure(tm(&sdls_oid, tm_channel(), 24, fixed)),
        invalid(field::IDLE_POLICY)
    );
    // An MC and a VC that both have an OCF or a Secondary Header (132.0 §2.2.4 b, c).
    let mut both_ocf = tm_channel();
    both_ocf.mc_ocf = Some([0; 4]);
    let mut vcs = tm_vcs();
    vcs[0] = VcConfig::<Tm>::new(
        VcId::<3>::new(1).unwrap(),
        TmVc {
            ocf: Some([1; 4]),
            ..TmVc::new(TmRole::Packets)
        },
    )
    .with_priority(1);
    assert_eq!(
        failure(tm(&vcs, both_ocf, 24, fixed)),
        invalid(field::OCF_FLAG)
    );
    let mut both_fsh = tm_channel();
    both_fsh.mc_fsh = Some(Fsh::new(&[1]).unwrap());
    vcs[0] = VcConfig::<Tm>::new(
        VcId::<3>::new(1).unwrap(),
        TmVc {
            fsh: Some(Fsh::new(&[2, 3]).unwrap()),
            ..TmVc::new(TmRole::Packets)
        },
    )
    .with_priority(1);
    assert_eq!(
        failure(tm(&vcs, both_fsh, 24, fixed)),
        invalid(field::FSH_LEN)
    );
    // A frame that leaves no data field to some VC: 6 header + 2 FECF + 4 + 2 security.
    let mut secured = tm_vcs();
    secured[0] = tm_vc(1, TmRole::Packets, 1).with_security(SecurityLayout::new(4, 2).unwrap());
    assert!(tm(&secured, tm_channel(), 15, fixed).is_ok());
    assert_eq!(
        failure(tm(&secured, tm_channel(), 14, fixed)),
        invalid(field::FRAME_LEN)
    );
    // A header or trailer above the limits of API-071 is refused when the layout is built.
    assert_eq!(
        failure(SecurityLayout::new(65, 0)),
        invalid(field::SECURITY_HEADER_LEN)
    );
    assert_eq!(
        failure(SecurityLayout::new(0, 0x1_0000)),
        invalid(field::SECURITY_TRAILER_LEN)
    );
    // Secondary Header data of 1 to 63 octets.
    assert_eq!(failure(Fsh::new(&[])), invalid(field::FSH_LEN));
    assert_eq!(failure(Fsh::new(&[0; 64])), invalid(field::FSH_LEN));
    let fsh = Fsh::new(&[7; 63]).unwrap();
    assert_eq!(
        (fsh.len(), fsh.is_empty(), fsh.octets().count()),
        (63, false, 63)
    );
    // No Insert Zone on TM, none of 0 octets, and one that takes the whole data field.
    assert_eq!(
        failure(tm_config(fixed).with_insert_zone(2, 0)),
        invalid(field::INSERT_ZONE_LEN)
    );
    assert_eq!(
        failure(aos_config().with_insert_zone(0, 0)),
        invalid(field::INSERT_ZONE_LEN)
    );
    assert_eq!(
        failure(aos_config().with_insert_zone(20, 0)),
        invalid(field::FRAME_LEN)
    );
    assert!(aos_config().with_insert_zone(19, 0).is_ok());
    // AOS and USLP: VCID 63 is the OID VC; Issue 4 has an 8-bit SCID; USLP frames are at most
    // 65 536 octets.
    let aos = |vcs: &[VcConfig<Aos>], channel| {
        ChannelMuxConfig::<Aos, V>::new(channel, 28, true, fixed, vcs)
    };
    assert_eq!(
        failure(aos(&[aos_vc(63, 1)], aos_channel())),
        invalid(field::VC_ID)
    );
    let issue4 = |scid| {
        AosChannel::new(
            SpacecraftId::<10>::new(scid).unwrap(),
            AosEdition::Issue4Compatible,
            false,
            false,
        )
    };
    assert_eq!(failure(issue4(256)), invalid(field::SPACECRAFT_ID));
    assert!(aos(&[aos_vc(1, 1)], issue4(255).unwrap()).is_ok());
    // A TM frame is at most 4 095 octets (TM-001, OI-51, HZ-TM-04).
    let long = |len| ChannelMuxConfig::<Tm, V>::new(tm_channel(), len, true, fixed, &tm_vcs());
    assert!(long(4095).is_ok());
    assert_eq!(failure(long(4096)), invalid(field::FRAME_LEN));
    let uslp = |vcs: &[VcConfig<Uslp>], frame_len| {
        ChannelMuxConfig::<Uslp, V>::new(uslp_channel(), frame_len, true, fixed, vcs)
    };
    assert_eq!(
        failure(uslp(&[uslp_vc(63, false)], 32)),
        invalid(field::VC_ID)
    );
    assert!(uslp(&[uslp_vc(1, false)], 65_536).is_ok());
    assert_eq!(
        failure(uslp(&[uslp_vc(1, false)], 65_537)),
        invalid(field::FRAME_LEN)
    );
    // An OID frame needs a data field longer than the TFDF Header.
    assert_eq!(failure(uslp(&[], 8)), invalid(field::FRAME_LEN));
    assert_eq!(failure(uslp(&[], 12)), invalid(field::FRAME_LEN));
    assert!(uslp(&[], 13).is_ok());
}

#[test]
fn the_configuration_reports_what_it_holds() {
    // req: LNK-078, LNK-030, LNK-031
    let config = tm_config(MuxPolicy::RoundRobin);
    assert_eq!(config.frame_len(), 24);
    assert!(config.fecf());
    assert_eq!(config.policy(), MuxPolicy::RoundRobin);
    assert_eq!(config.vc_count(), 4);
    assert_eq!(config.insert_zone_len(), 0);
    assert_eq!(config.channel().scid.get(), 165);
    // The data field of each VC: the frame without header, FECF, and the other fields.
    assert_eq!(config.field_len(1, Qos::SequenceControlled), Some(16));
    assert_eq!(config.field_len(9, Qos::SequenceControlled), None);
    let uslp = uslp_config(false);
    assert_eq!(uslp.field_len(1, Qos::SequenceControlled), Some(21));
    assert_eq!(uslp.field_len(1, Qos::Expedited), Some(22));
    let vc = tm_vcs()[1];
    assert_eq!(vc.vc().get(), 2);
    assert_eq!((vc.priority(), vc.weight()), (3, Weight::ONE));
    assert_eq!(vc.security(), SecurityLayout::NONE);
    assert_eq!(vc.params().role, TmRole::Packets);
    assert_eq!(MuxPolicy::default(), MuxPolicy::FixedPriority);
    // The VCs are kept in ascending VCID order whatever the order given.
    let mut shuffled = tm_vcs();
    shuffled.reverse();
    let again =
        ChannelMuxConfig::<Tm, V>::new(tm_channel(), 24, true, MuxPolicy::FixedPriority, &shuffled)
            .unwrap();
    assert_eq!(again, tm_config(MuxPolicy::FixedPriority));
    let channel = aos_channel();
    assert_eq!(channel.scid().get(), 421);
    assert_eq!(channel.edition(), AosEdition::Issue5);
    assert!(!channel.fhec() && !channel.cycle_in_use());
    let flagged = AosChannel::new(
        SpacecraftId::<10>::new(7).unwrap(),
        AosEdition::Issue5,
        true,
        true,
    )
    .unwrap();
    assert!(flagged.fhec() && flagged.cycle_in_use());
    let no_fecf = ChannelMuxConfig::<Tm, V>::new(
        tm_channel(),
        24,
        false,
        MuxPolicy::FixedPriority,
        &tm_vcs(),
    )
    .unwrap();
    assert!(!no_fecf.fecf());
    assert!(uslp_vc(1, true).params().cop() && !uslp_vc(1, false).params().cop());
    let aos = aos_config().with_insert_zone(3, 0xA5).unwrap();
    assert_eq!(aos.insert_zone_len(), 3);
}

#[test]
fn an_engine_refuses_a_frame_longer_than_its_capacity() {
    // req: GEN-011, LNK-075
    let long = ChannelMuxConfig::<Tm, V>::new(
        tm_channel(),
        F + 1,
        true,
        MuxPolicy::FixedPriority,
        &tm_vcs(),
    )
    .unwrap();
    assert_eq!(
        failure(TmMux::new(long)),
        named(ErrorKind::CapacityExceeded, field::FRAME_LEN)
    );
    // The in-place head makes the same check (#240).
    assert_eq!(
        failure(TmMux::new_head(long)),
        named(ErrorKind::CapacityExceeded, field::FRAME_LEN)
    );
    assert!(TmMux::new_head(tm_config(MuxPolicy::FixedPriority)).is_ok());
    const { assert!(TmMux::HEAD_OFFSET < TmMux::BUFFERS_OFFSET) };
    let mut mux = TmMux::new(tm_config(MuxPolicy::FixedPriority)).unwrap();
    let mut out = [0; F];
    assert_eq!(
        failure(mux.step(MuxInput::Reconfigure(long), &mut out)),
        named(ErrorKind::CapacityExceeded, field::FRAME_LEN)
    );
    assert_eq!(mux.config().frame_len(), 24);
}

#[test]
fn an_offer_the_vc_cannot_have_is_refused_and_changes_nothing() {
    // req: LNK-076, LNK-082, LNK-002, API-052, API-058, OI-78
    let mut mux = TmMux::new(tm_config(MuxPolicy::FixedPriority)).unwrap();
    let mut out = [0; F];
    let field16 = [0x11; 16];
    let invalid = |f| named(ErrorKind::InvalidField, f);
    let mut step = |mux: &mut TmMux, o: Offer<'_>| failure(mux.step(MuxInput::Offer(o), &mut out));
    // A VC not configured, a field of another length.
    assert_eq!(
        step(&mut mux, offer(5, &field16, Some(0))).0,
        ErrorKind::UnknownChannel
    );
    assert_eq!(
        step(&mut mux, offer(1, &field16[..15], Some(0))),
        named(ErrorKind::LengthMismatch, field::FIELD_LEN)
    );
    // The First Header Pointer: required for Packets, refused for `VCA_SDUs`, at most 0x7FF.
    assert_eq!(
        step(&mut mux, offer(1, &field16, None)),
        invalid(field::FIRST_HEADER_POINTER)
    );
    assert_eq!(
        step(&mut mux, offer(3, &field16, Some(0))),
        invalid(field::FIRST_HEADER_POINTER)
    );
    assert_eq!(
        step(&mut mux, offer(1, &field16, Some(0x800))),
        named(ErrorKind::OutOfRange, field::FIRST_HEADER_POINTER)
    );
    // A route and a COP-1 block belong to USLP (OI-78).
    let route = Route {
        map: MapId::<4>::new(0).unwrap(),
        qos: Qos::SequenceControlled,
    };
    assert_eq!(
        step(
            &mut mux,
            Offer {
                route: Some(route),
                ..offer(1, &field16, Some(0))
            }
        ),
        invalid(field::MAP_ID)
    );
    let cop = Cop::TypeAd { vcf_count: 0 };
    assert_eq!(
        step(
            &mut mux,
            Offer {
                cop: Some(cop),
                ..offer(1, &field16, Some(0))
            }
        ),
        invalid(field::COP_IN_EFFECT)
    );
    let s = mux.status();
    assert_eq!(
        (s.offers.get(), s.refused.get(), s.would_block.get()),
        (0, 7, 0)
    );
    // The refusals changed nothing: the first valid offers are the first frames.
    assert_eq!(step_ok(&mut mux, offer(1, &field16, Some(5))), 0);
    assert_eq!(
        step(&mut mux, offer(1, &field16, Some(5))),
        (ErrorKind::WouldBlock, None)
    );
    let s = mux.status();
    assert_eq!(
        (s.offers.get(), s.refused.get(), s.would_block.get()),
        (1, 7, 1)
    );
    assert_eq!((s.slots.current(), s.slots.high_water()), (16, 16));
    let mut frame = [0; F];
    let a = mux.step(MuxInput::Opportunity, &mut frame).unwrap();
    assert_eq!(a.len(), 1);
    assert_eq!(&frame[2..4], [0, 0]);
    let s = mux.status();
    assert_eq!(
        (s.frames_out.get(), s.oid_frames.get(), s.slots.current()),
        (1, 0, 0)
    );
}

fn step_ok(mux: &mut TmMux, o: Offer<'_>) -> usize {
    let mut out = [0; F];
    mux.step(MuxInput::Offer(o), &mut out).unwrap().len()
}

#[test]
fn a_uslp_offer_is_checked_against_the_vc_and_its_cop() {
    // req: LNK-076, LNK-032, OI-78
    let field21 = [0x22; 21];
    let field22 = [0x22; 22];
    let mut out = [0; F];
    let invalid = |f| named(ErrorKind::InvalidField, f);
    let cop = |vcf_count, bypass, protocol_control| {
        Cop::new(vcf_count, bypass, protocol_control).unwrap()
    };
    let route = |map, qos| Route {
        map: MapId::<4>::new(map).unwrap(),
        qos,
    };
    // Without COP-1 in effect the VC takes no cop block.
    let mut mux = UslpMux::new(uslp_config(false)).unwrap();
    let with = Offer {
        cop: Some(cop(0, false, false)),
        ..offer(1, &field21, None)
    };
    assert_eq!(
        failure(mux.step(MuxInput::Offer(with), &mut out)),
        invalid(field::COP_IN_EFFECT)
    );
    // A MAP the VC does not have (OI-78): UnknownChannel naming the MAP, state unchanged.
    let wrong_map = Offer {
        route: Some(route(2, Qos::SequenceControlled)),
        ..offer(1, &field21, None)
    };
    assert_eq!(
        failure(mux.step(MuxInput::Offer(wrong_map), &mut out)),
        named(ErrorKind::UnknownChannel, field::MAP_ID)
    );
    // With COP-1 in effect.
    let mut mux = UslpMux::new(uslp_config(true)).unwrap();
    let mut refused = |o: Offer<'_>| failure(mux.step(MuxInput::Offer(o), &mut out));
    // The reserved flag combination of Table 4-1 (Bypass 0, Protocol Control Command 1) cannot
    // be built, and the others are the variants.
    assert_eq!(
        failure(Cop::new(0, false, true)),
        invalid(field::PROTOCOL_CONTROL_COMMAND_FLAG)
    );
    let (ad, bd, bc) = (
        cop(5, false, false),
        cop(5, true, false),
        cop(5, true, true),
    );
    assert_eq!(
        (ad, bd, bc),
        (Cop::TypeAd { vcf_count: 5 }, Cop::TypeBd, Cop::TypeBc)
    );
    assert_eq!((ad.bypass(), bd.bypass(), bc.bypass()), (false, true, true));
    assert_eq!(
        (
            ad.protocol_control(),
            bd.protocol_control(),
            bc.protocol_control()
        ),
        (false, false, true)
    );
    assert_eq!(
        (ad.vcf_count(), bd.vcf_count(), bc.vcf_count()),
        (Some(5), None, None)
    );
    // The route's QoS and the block's Bypass Flag must agree.
    assert_eq!(
        refused(Offer {
            route: Some(route(3, Qos::Expedited)),
            cop: Some(cop(0, false, false)),
            ..offer(1, &field21, None)
        }),
        invalid(field::QOS)
    );
    // A VCF Count that the count field of a sequence-controlled frame cannot hold (2 octets).
    assert_eq!(
        refused(Offer {
            cop: Some(cop(0x1_0000, false, false)),
            ..offer(1, &field21, None)
        }),
        named(ErrorKind::OutOfRange, field::VCF_COUNT)
    );
    // The field length follows the Bypass Flag of the block, not the VC's default QoS.
    assert_eq!(
        refused(Offer {
            cop: Some(cop(0, true, false)),
            ..offer(1, &field21, None)
        }),
        named(ErrorKind::LengthMismatch, field::FIELD_LEN)
    );
    let ok = Offer {
        cop: Some(cop(0xFFFF, false, false)),
        ..offer(1, &field21, None)
    };
    assert!(mux.step(MuxInput::Offer(ok), &mut out).unwrap().is_empty());
    let ok = Offer {
        route: Some(route(3, Qos::Expedited)),
        cop: Some(cop(99, true, true)),
        ..offer(2, &field22, None)
    };
    assert!(mux.step(MuxInput::Offer(ok), &mut out).unwrap().is_empty());
    // Type-BC (VC 2, priority 2) first, with the expedited count; then the given count.
    let a = mux.step(MuxInput::Opportunity, &mut out).unwrap();
    assert_eq!(a.len(), 1);
    assert_eq!(&out[..7], [0xC1, 0x23, 0x40, 0x46, 0x00, 0x1F, 0xC1]);
    assert_eq!(out[7], 0);
    mux.step(MuxInput::Opportunity, &mut out).unwrap();
    assert_eq!(&out[7..9], [0xFF, 0xFF]);
}

#[test]
fn a_slot_update_the_configuration_cannot_take_is_refused() {
    // req: LNK-033, LNK-034, LNK-076, API-058
    let mut out = [0; F];
    let mut tm = TmMux::new(tm_config(MuxPolicy::FixedPriority)).unwrap();
    let invalid = |f| named(ErrorKind::InvalidField, f);
    // No MC_OCF, no VC_OCF, no Secondary Header, no Insert Zone configured.
    for scope in [Scope::Mc, Scope::Vc(1)] {
        assert_eq!(
            failure(tm.step(MuxInput::SetOcf { scope, ocf: [0; 4] }, &mut out)),
            invalid(field::OCF_FLAG)
        );
        assert_eq!(
            failure(tm.step(MuxInput::SetFsh { scope, data: &[1] }, &mut out)),
            invalid(field::FSH_FLAG)
        );
    }
    assert_eq!(
        failure(tm.step(MuxInput::SetInsert(&[1]), &mut out)),
        invalid(field::INSERT_ZONE_LEN)
    );
    assert_eq!(
        kind_of(tm.step(
            MuxInput::SetOcf {
                scope: Scope::Vc(5),
                ocf: [0; 4]
            },
            &mut out
        )),
        ErrorKind::UnknownChannel
    );
    assert_eq!(
        kind_of(tm.step(
            MuxInput::SetFsh {
                scope: Scope::Vc(5),
                data: &[1]
            },
            &mut out
        )),
        ErrorKind::UnknownChannel
    );
    // A Secondary Header of another length than the configured one.
    let mut channel = tm_channel();
    channel.mc_fsh = Some(Fsh::new(&[1, 2]).unwrap());
    let config =
        ChannelMuxConfig::<Tm, V>::new(channel, 24, true, MuxPolicy::FixedPriority, &tm_vcs())
            .unwrap();
    let mut tm = TmMux::new(config).unwrap();
    assert_eq!(
        failure(tm.step(
            MuxInput::SetFsh {
                scope: Scope::Mc,
                data: &[1]
            },
            &mut out
        )),
        named(ErrorKind::LengthMismatch, field::FSH_LEN)
    );
    assert!(
        tm.step(
            MuxInput::SetFsh {
                scope: Scope::Mc,
                data: &[8, 9]
            },
            &mut out
        )
        .unwrap()
        .is_empty()
    );
    tm.step(MuxInput::Opportunity, &mut out).unwrap();
    assert_eq!(&out[6..9], [2, 8, 9]);
    // The Insert Zone of another length.
    let mut aos = AosMux::new(aos_config().with_insert_zone(2, 0xEE).unwrap()).unwrap();
    assert_eq!(
        failure(aos.step(MuxInput::SetInsert(&[1]), &mut out)),
        named(ErrorKind::LengthMismatch, field::INSERT_ZONE_LEN)
    );
    // A VC without an OCF in AOS.
    assert_eq!(
        failure(aos.step(
            MuxInput::SetOcf {
                scope: Scope::Vc(1),
                ocf: [0; 4]
            },
            &mut out
        )),
        invalid(field::OCF_FLAG)
    );
    assert_eq!(
        failure(aos.step(
            MuxInput::SetOcf {
                scope: Scope::Mc,
                ocf: [0; 4]
            },
            &mut out
        )),
        invalid(field::OCF_FLAG)
    );
}

#[test]
fn an_opportunity_needs_the_whole_frame_and_changes_nothing_without() {
    // req: LNK-075, LNK-030, API-058, API-054
    let mut mux = TmMux::new(tm_config(MuxPolicy::FixedPriority)).unwrap();
    let mut out = [0; F];
    mux.step(MuxInput::Offer(offer(1, &[1; 16], Some(0))), &mut out)
        .unwrap();
    let mut short = [0; 23];
    let e = mux.step(MuxInput::Opportunity, &mut short).unwrap_err();
    assert_eq!(e.kind(), ErrorKind::BufferTooSmall);
    assert_eq!(mux.status().frames_out.get(), 0);
    // The offer is still there and the first frame is the first of the counts; an area of
    // exactly the frame is enough.
    let mut exact = [0; 24];
    let a = mux.step(MuxInput::Opportunity, &mut exact).unwrap();
    let Some(MuxOutput::Transmit(t)) = a.get(0) else {
        panic!("no frame")
    };
    assert_eq!((t.class, t.vc, t.frame.len()), (FrameClass::Data, 1, 24));
    assert_eq!(&exact[2..4], [0, 0]);
}

#[test]
fn the_worst_case_of_an_input_is_the_bound() {
    // req: LNK-074, LNK-075, API-057, LNK-034
    assert_eq!(CHANNEL_MUX_MAX_ACTIONS, 2);
    // An Opportunity with an Insert Zone that repeats returns the frame and the notification.
    let mut aos = AosMux::new(aos_config().with_insert_zone(3, 0).unwrap()).unwrap();
    let mut out = [0; F];
    let a = aos.step(MuxInput::Opportunity, &mut out).unwrap();
    assert_eq!(a.len(), CHANNEL_MUX_MAX_ACTIONS);
    assert!(matches!(a.get(0), Some(MuxOutput::Transmit(_))));
    assert_eq!(a.get(1), Some(&MuxOutput::InsertSduRepeated));
    // The same frame with a fresh IN_SDU: one action.
    aos.step(MuxInput::SetInsert(&[1, 2, 3]), &mut out).unwrap();
    assert_eq!(aos.step(MuxInput::Opportunity, &mut out).unwrap().len(), 1);
    assert_eq!(aos.status().insert_repeated.get(), 1);
    // Every other input returns at most one action: Reconfigure with fields held.
    aos.step(MuxInput::Offer(offer(1, &[0; 17], None)), &mut out)
        .unwrap();
    aos.step(MuxInput::Offer(offer(2, &[0; 17], None)), &mut out)
        .unwrap();
    let a = aos
        .step(
            MuxInput::Reconfigure(aos_config().with_insert_zone(3, 0).unwrap()),
            &mut out,
        )
        .unwrap();
    assert_eq!(
        a.iter().copied().collect::<Vec<_>>(),
        [MuxOutput::Discarded { octets: 34 }]
    );
    assert_eq!(aos.status().discarded_octets.get(), 34);
    // Nothing held: no Discarded.
    let a = aos
        .step(MuxInput::Reconfigure(aos_config()), &mut out)
        .unwrap();
    assert!(a.is_empty());
    assert_eq!(aos.config().insert_zone_len(), 0);
}

#[test]
fn a_reconfiguration_keeps_the_counts_and_the_oid_generator_and_starts_the_walk_afresh() {
    // req: LNK-003, LNK-031, LNK-032, LNK-036
    let weighted = |w: u16| {
        let mut vcs = tm_vcs();
        vcs[0] = vcs[0].with_weight(Weight::new(w).unwrap());
        ChannelMuxConfig::<Tm, V>::new(tm_channel(), 24, true, MuxPolicy::WeightedRoundRobin, &vcs)
            .unwrap()
    };
    let mut mux = TmMux::new(weighted(3)).unwrap();
    let mut out = [0; F];
    // VC 1 is served once of its turn of 3.
    mux.step(MuxInput::Offer(offer(1, &[1; 16], Some(0))), &mut out)
        .unwrap();
    mux.step(MuxInput::Opportunity, &mut out).unwrap();
    // A reconfiguration starts the walk afresh: VC 1 again is the first of a new turn, so with
    // VC 1 and VC 2 offered VC 1 goes first, though VC 2 would follow it in a continued walk.
    mux.step(MuxInput::Reconfigure(weighted(3)), &mut out)
        .unwrap();
    mux.step(MuxInput::Offer(offer(1, &[2; 16], Some(0))), &mut out)
        .unwrap();
    mux.step(MuxInput::Offer(offer(2, &[3; 16], Some(0))), &mut out)
        .unwrap();
    mux.step(MuxInput::Opportunity, &mut out).unwrap();
    assert_eq!((out[1] >> 1) & 7, 1);
    // The MC count and the VC count continued (one frame before, now the second frame, and the second of VC 1).
    assert_eq!((out[2], out[3]), (1, 1));
    // OID frames continue the generator across a reconfiguration.
    mux.step(MuxInput::Opportunity, &mut out).unwrap();
    mux.step(MuxInput::Opportunity, &mut out).unwrap();
    assert_eq!(&out[6..10], [0xFF, 0xFF, 0xFF, 0xFF]);
    mux.step(MuxInput::Reconfigure(weighted(3)), &mut out)
        .unwrap();
    mux.step(MuxInput::Opportunity, &mut out).unwrap();
    assert_eq!(&out[6..10], [0xBE, 0x7E, 0x00, 0xB1]);
}

#[test]
fn the_aos_vc_frame_count_counts_up_by_one_per_frame() {
    // req: LNK-032
    let mut mux = AosMux::new(aos_config()).unwrap();
    let mut out = [0; F];
    for n in 0..300_u32 {
        mux.step(MuxInput::Offer(offer(1, &[0; 20], None)), &mut out)
            .unwrap();
        mux.step(MuxInput::Opportunity, &mut out).unwrap();
        assert_eq!(u32::from_be_bytes([0, out[2], out[3], out[4]]), n);
    }
}

#[test]
fn the_status_counts_what_the_engine_did() {
    // req: LNK-002, API-056
    let mut mux = AosMux::new(aos_config()).unwrap();
    let mut out = [0; F];
    mux.step(MuxInput::Offer(offer(1, &[0; 20], None)), &mut out)
        .unwrap();
    mux.step(MuxInput::Offer(offer(2, &[0; 20], None)), &mut out)
        .unwrap();
    for _ in 0..3 {
        mux.step(MuxInput::Opportunity, &mut out).unwrap();
    }
    let s = mux.status();
    assert_eq!(
        (s.frames_out.get(), s.oid_frames.get(), s.offers.get()),
        (3, 1, 2)
    );
    assert_eq!((s.slots.current(), s.slots.high_water()), (0, 40));
}

/// One step of a random walk over the mux.
#[derive(Debug, Clone, Copy)]
enum Op {
    Offer(u8, bool),
    Opportunity,
    SetOcf(u8),
    SetInsert,
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        (0_u8..9, any::<bool>()).prop_map(|(vc, right)| Op::Offer(vc, right)),
        Just(Op::Opportunity),
        Just(Op::Opportunity),
        (0_u8..3).prop_map(Op::SetOcf),
        Just(Op::SetInsert),
    ]
}

proptest! {
    #![proptest_config(strategies::config(128, REGRESSIONS))]

    #[test]
    fn no_sequence_of_inputs_breaks_the_engine(
        ops in prop::collection::vec(op(), 0..60),
        policy in 0_u8..3,
    ) {
        // req: LNK-074, LNK-075, LNK-030, LNK-032, API-057
        let policy = [MuxPolicy::FixedPriority, MuxPolicy::RoundRobin, MuxPolicy::WeightedRoundRobin]
            [usize::from(policy)];
        let mut mux = TmMux::new(tm_config(policy)).unwrap();
        let mut out = [0_u8; F];
        let mut frames = 0_u32;
        let mut vc_frames = [0_u32; 8];
        for op in ops {
            let input = match op {
                Op::Offer(vc, right) => {
                    let field = [vc; 16];
                    let len = if right { 16 } else { 15 };
                    let pointer = if vc == 3 { None } else { Some(0) };
                    match mux.step(MuxInput::Offer(Offer { vc, field: &field[..len], pointer, route: None, cop: None }), &mut out) {
                        Ok(a) => prop_assert!(a.is_empty()),
                        Err(e) => prop_assert_ne!(e.kind(), ErrorKind::InternalError),
                    }
                    continue;
                }
                Op::Opportunity => MuxInput::Opportunity,
                Op::SetOcf(vc) => MuxInput::SetOcf { scope: Scope::Vc(vc), ocf: [vc; 4] },
                Op::SetInsert => MuxInput::SetInsert(&[1]),
            };
            match mux.step(input, &mut out) {
                Ok(a) => {
                    prop_assert!(a.len() <= CHANNEL_MUX_MAX_ACTIONS);
                    if let Some(MuxOutput::Transmit(t)) = a.get(0) {
                        prop_assert_eq!(t.frame.len(), 24);
                        prop_assert!(t.security_header.end() <= t.data.start());
                        prop_assert!(t.data.end() <= t.security_trailer.start());
                        // The MC count is one per frame; the VC count one per frame of its VC.
                        prop_assert_eq!(out[2], u8::try_from(frames % 256).unwrap());
                        let vc = usize::from(t.vc);
                        prop_assert_eq!(out[3], u8::try_from(vc_frames[vc] % 256).unwrap());
                        frames += 1;
                        vc_frames[vc] += 1;
                    }
                }
                Err(e) => prop_assert_ne!(e.kind(), ErrorKind::InternalError),
            }
        }
    }
}

#[test]
fn the_protocols_without_cop_have_no_vcf_count_to_take() {
    // req: LNK-076, LNK-001
    use aldis_link::mux::MuxProtocol;
    let tm = TmVc::new(TmRole::Packets);
    let aos = AosVc::default();
    assert!(!Tm::vc_cop(&tm) && !Aos::vc_cop(&aos));
    assert_eq!((Tm::vcf_max(&tm, true), Aos::vcf_max(&aos, false)), (0, 0));
    assert!(Tm::vc_route(&tm).is_none() && Aos::vc_route(&aos).is_none());
    assert!(Tm::vc_fsh(&tm).is_none() && Aos::vc_fsh(&aos).is_none());
    assert_eq!(Aos::check_pointer(&aos, Some(5)).unwrap(), 0);
    assert_eq!(
        Uslp::check_pointer(uslp_vc(1, false).params(), Some(5)).unwrap(),
        0
    );
}

#[test]
fn a_frame_of_the_capacity_is_accepted() {
    // req: GEN-011, LNK-075
    let tm =
        ChannelMuxConfig::<Tm, V>::new(tm_channel(), F, true, MuxPolicy::FixedPriority, &tm_vcs())
            .unwrap();
    assert!(TmMux::new(tm).is_ok() && TmMux::new_head(tm).is_ok());
    let aos = ChannelMuxConfig::<Aos, V>::new(
        aos_channel(),
        F,
        true,
        MuxPolicy::FixedPriority,
        &[aos_vc(1, 1)],
    )
    .unwrap();
    assert!(AosMux::new(aos).is_ok());
    let uslp = ChannelMuxConfig::<Uslp, V>::new(
        uslp_channel(),
        F,
        true,
        MuxPolicy::FixedPriority,
        &[uslp_vc(1, false)],
    )
    .unwrap();
    assert!(UslpMux::new(uslp).is_ok());
}

#[test]
fn a_tm_frame_carries_all_ten_bits_of_the_scid() {
    // req: LNK-032
    let channel = TmChannel::new(
        SpacecraftId::<10>::new(0x2A5).unwrap(),
        VcId::<3>::new(7).unwrap(),
    );
    let config =
        ChannelMuxConfig::<Tm, V>::new(channel, 24, true, MuxPolicy::FixedPriority, &tm_vcs())
            .unwrap();
    let mut mux = TmMux::new(config).unwrap();
    let mut out = [0; F];
    step_ok(&mut mux, offer(1, &[1; 16], Some(0)));
    mux.step(MuxInput::Opportunity, &mut out).unwrap();
    // Version 00, then the SCID: bits 9-4 in octet 0, bits 3-0 in the top of octet 1.
    assert_eq!((out[0], out[1] >> 4), (0x2A, 0x5));
}

#[test]
fn a_uslp_frame_names_the_destination_when_the_channel_is_one() {
    // req: LNK-032
    let channel = UslpChannel {
        source_or_destination: SourceOrDestination::Destination,
        ..uslp_channel()
    };
    let config = ChannelMuxConfig::<Uslp, V>::new(
        channel,
        32,
        true,
        MuxPolicy::FixedPriority,
        &[uslp_vc(1, false)],
    )
    .unwrap();
    let mut mux = UslpMux::new(config).unwrap();
    let mut out = [0; F];
    mux.step(MuxInput::Offer(offer(1, &[0x22; 21], None)), &mut out)
        .unwrap();
    mux.step(MuxInput::Opportunity, &mut out).unwrap();
    // The SCID 0x1234 ends in the top four bits of octet 2; the next bit is the flag.
    assert_eq!(out[2] >> 4, 0x4);
    assert_eq!(out[2] >> 3 & 1, 1);
}

#[test]
fn an_issue_4_aos_frame_has_an_8_bit_scid_and_zero_spare_bits() {
    // req: LNK-032, AOS-002
    let channel = AosChannel::new(
        SpacecraftId::<10>::new(255).unwrap(),
        AosEdition::Issue4Compatible,
        false,
        false,
    )
    .unwrap();
    let config = ChannelMuxConfig::<Aos, V>::new(
        channel,
        28,
        true,
        MuxPolicy::FixedPriority,
        &[aos_vc(1, 1)],
    )
    .unwrap();
    let mut mux = AosMux::new(config).unwrap();
    let mut out = [0; F];
    mux.step(MuxInput::Offer(offer(1, &[0; 20], None)), &mut out)
        .unwrap();
    mux.step(MuxInput::Opportunity, &mut out).unwrap();
    assert_eq!(&out[..6], [0x7F, 0xC1, 0, 0, 0, 0]);
}

#[test]
fn a_uslp_vc_with_cop_needs_a_count_field_for_the_frame_sequence_number() {
    // req: LNK-078, LNK-076, RS-020, OI-78
    let make = |sc, cop| {
        UslpVc::new(
            MapId::<4>::new(3).unwrap(),
            Qos::Expedited,
            VcfCountLen::new(sc).unwrap(),
            VcfCountLen::new(0).unwrap(),
            cop,
        )
    };
    assert_eq!(
        failure(make(0, true)),
        named(ErrorKind::InvalidConfig, field::VCF_COUNT_LEN)
    );
    assert!(make(0, false).is_ok());
    let vc = make(1, true).unwrap();
    assert_eq!(
        (vc.map().get(), vc.qos(), vc.cop()),
        (3, Qos::Expedited, true)
    );
    assert_eq!((vc.vcf_len_sc().get(), vc.vcf_len_exp().get()), (1, 0));
}

#[test]
fn a_reconfiguration_puts_the_slots_back_to_the_configured_values() {
    // req: LNK-003, LNK-033
    let config = || {
        let mut channel = tm_channel();
        channel.mc_ocf = Some([0xA1; 4]);
        ChannelMuxConfig::<Tm, V>::new(channel, 24, true, MuxPolicy::FixedPriority, &tm_vcs())
            .unwrap()
    };
    let mut mux = TmMux::new(config()).unwrap();
    let mut out = [0; F];
    mux.step(
        MuxInput::SetOcf {
            scope: Scope::Mc,
            ocf: [0xB2; 4],
        },
        &mut out,
    )
    .unwrap();
    mux.step(MuxInput::Opportunity, &mut out).unwrap();
    assert_eq!(&out[18..22], [0xB2; 4]);
    // The new configuration's value is in the next frame, until the caller sets the OCF again.
    mux.step(MuxInput::Reconfigure(config()), &mut out).unwrap();
    mux.step(MuxInput::Opportunity, &mut out).unwrap();
    assert_eq!(&out[18..22], [0xA1; 4]);
    mux.step(
        MuxInput::SetOcf {
            scope: Scope::Mc,
            ocf: [0xC3; 4],
        },
        &mut out,
    )
    .unwrap();
    mux.step(MuxInput::Opportunity, &mut out).unwrap();
    assert_eq!(&out[18..22], [0xC3; 4]);
}

#[test]
fn an_aos_and_a_uslp_mux_reconfigure_and_report_what_they_drop() {
    // req: LNK-003, LNK-076
    let mut out = [0; F];
    let mut aos = AosMux::new(aos_config()).unwrap();
    let field = [7_u8; 20];
    aos.step(MuxInput::Offer(offer(1, &field, None)), &mut out)
        .unwrap();
    let actions = aos
        .step(MuxInput::Reconfigure(aos_config()), &mut out)
        .unwrap();
    assert_eq!(
        actions.iter().copied().collect::<Vec<_>>(),
        [MuxOutput::Discarded { octets: 20 }]
    );
    let mut uslp = UslpMux::new(uslp_config(false)).unwrap();
    let field = [7_u8; 21];
    uslp.step(MuxInput::Offer(offer(1, &field, None)), &mut out)
        .unwrap();
    let actions = uslp
        .step(MuxInput::Reconfigure(uslp_config(false)), &mut out)
        .unwrap();
    assert_eq!(
        actions.iter().copied().collect::<Vec<_>>(),
        [MuxOutput::Discarded { octets: 21 }]
    );
    // An oversize configuration is refused whatever the protocol, and changes nothing.
    let oversize = ChannelMuxConfig::<Aos, V>::new(
        aos_channel(),
        F + 1,
        true,
        MuxPolicy::FixedPriority,
        &[aos_vc(1, 1)],
    )
    .unwrap();
    assert_eq!(
        failure(aos.step(MuxInput::Reconfigure(oversize), &mut out)),
        named(ErrorKind::CapacityExceeded, field::FRAME_LEN)
    );
    assert_eq!(
        failure(AosMux::new(oversize)),
        named(ErrorKind::CapacityExceeded, field::FRAME_LEN)
    );
    let oversize = ChannelMuxConfig::<Uslp, V>::new(
        uslp_channel(),
        F + 1,
        true,
        MuxPolicy::FixedPriority,
        &[uslp_vc(1, false)],
    )
    .unwrap();
    assert_eq!(
        failure(uslp.step(MuxInput::Reconfigure(oversize), &mut out)),
        named(ErrorKind::CapacityExceeded, field::FRAME_LEN)
    );
    assert_eq!(
        failure(UslpMux::new(oversize)),
        named(ErrorKind::CapacityExceeded, field::FRAME_LEN)
    );
}

#[test]
fn a_uslp_frame_length_beyond_the_header_field_is_an_error_not_a_clamped_length() {
    // req: LNK-032, LNK-082
    use aldis_link::mux::{FrameFields, MuxProtocol};
    let mut fields = FrameFields {
        vc: 1,
        oid: false,
        frame_len: aldis_uslp::MAX_FRAME_LEN,
        mc_count: 0,
        vc_count: 0,
        ocf: false,
        secondary: false,
        pointer: 0,
        bypass: false,
        protocol_control: false,
    };
    let vc = uslp_vc(1, false);
    let mut header = [0_u8; 16];
    // The largest legal frame fills the field: 65 536 octets is the value 0xFFFF.
    Uslp::write_header(&uslp_channel(), Some(vc.params()), &fields, &mut header).unwrap();
    assert_eq!(header[4..6], [0xFF, 0xFF]);
    fields.frame_len = aldis_uslp::MAX_FRAME_LEN + 1;
    let e =
        Uslp::write_header(&uslp_channel(), Some(vc.params()), &fields, &mut header).unwrap_err();
    assert_eq!(e.kind(), ErrorKind::InternalError);
    assert_eq!(e.field(), Some(field::FRAME_LEN));
}

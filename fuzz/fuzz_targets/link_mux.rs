//! Arbitrary inputs to the `ChannelMux` of TM, AOS, and USLP (VER-022, LNK-030…LNK-036, LNK-074,
//! LNK-075, LNK-076, API-057, API-058, GEN-020): whatever the offers (any VC, field length, and
//! pointer), OCF and Insert Zone slots, opportunities, and reconfigurations, no step panics or
//! reports an internal error; a frame has the
//! configured length with its Security Header, data, and Security Trailer in order inside it.
#![no_main]

// req: VER-022, LNK-030, LNK-032, LNK-074, LNK-075, LNK-076, API-057, API-058, GEN-020

use aldis_aos::{AosEdition, AosPhysicalChannelConfig};
use aldis_core::ErrorKind;
use aldis_core::ids::{MapId, SpacecraftId, VcId};
use aldis_link::common::Qos;
use aldis_link::mux::{
    AosChannel, AosVc, CHANNEL_MUX_MAX_ACTIONS, ChannelMux, ChannelMuxConfig, MuxInput, MuxOutput,
    MuxProtocol, Offer, Scope, TmChannel, TmOffer, TmRole, TmVc, UslpChannel, UslpOffer, UslpVc,
    VcConfig,
};
use aldis_link::policy::MuxPolicy;
use aldis_link::protocol::{Aos, Tm, Uslp};
use aldis_tm::TmPhysicalChannelConfig;
use aldis_uslp::{
    PhysicalChannelConfig as UslpPhysicalChannelConfig, SourceOrDestination, TransferFrameType,
    VcfCountLen,
};
use libfuzzer_sys::arbitrary::{Result, Unstructured};
use libfuzzer_sys::fuzz_target;

const V: usize = 8;
const F: usize = 64;

fn policy(u: &mut Unstructured<'_>) -> Result<MuxPolicy> {
    Ok(match u.int_in_range(0..=2)? {
        0 => MuxPolicy::FixedPriority,
        1 => MuxPolicy::RoundRobin,
        _ => MuxPolicy::WeightedRoundRobin,
    })
}

fn tm_config(policy: MuxPolicy, frame_len: usize) -> ChannelMuxConfig<Tm, V> {
    let vc = |id: u8, role, priority| {
        VcConfig::<Tm>::new(VcId::<3>::new(id).unwrap(), TmVc::new(role)).with_priority(priority)
    };
    ChannelMuxConfig::new(
        TmChannel::new(
            TmPhysicalChannelConfig::new(frame_len, true).unwrap(),
            SpacecraftId::<10>::new(165).unwrap(),
            VcId::<3>::new(7).unwrap(),
        ),
        policy,
        &[
            vc(1, TmRole::Packets, 1),
            vc(2, TmRole::Packets, 3),
            vc(3, TmRole::Vca, 2),
            vc(7, TmRole::Packets, 0),
        ],
    )
    .unwrap()
}

fn aos_config(policy: MuxPolicy, frame_len: usize) -> ChannelMuxConfig<Aos, V> {
    let vc = |id: u8, priority| {
        VcConfig::<Aos>::new(VcId::<6>::new(id).unwrap(), AosVc::default()).with_priority(priority)
    };
    ChannelMuxConfig::new(
        AosChannel::new(
            AosPhysicalChannelConfig::new(frame_len, false, 0, true, AosEdition::Issue5).unwrap(),
            SpacecraftId::<10>::new(421).unwrap(),
            false,
        )
        .unwrap(),
        policy,
        &[vc(1, 1), vc(2, 2)],
    )
    .unwrap()
}

fn uslp_config(policy: MuxPolicy, frame_len: usize, cop: bool) -> ChannelMuxConfig<Uslp, V> {
    let vc = |id: u8| {
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
    };
    ChannelMuxConfig::new(
        UslpChannel::new(
            UslpPhysicalChannelConfig::new(TransferFrameType::Fixed, frame_len, 0, true).unwrap(),
            SpacecraftId::<16>::new(0x1234).unwrap(),
            SourceOrDestination::Source,
            VcfCountLen::NONE,
        )
        .unwrap(),
        policy,
        &[vc(1), vc(2)],
    )
    .unwrap()
}

/// The metadata of an offer from the First Header Pointer of a TM offer, the member the others lack.
trait FromPointer {
    fn from_pointer(pointer: Option<u16>) -> Self;
}

impl FromPointer for TmOffer {
    fn from_pointer(pointer: Option<u16>) -> Self {
        TmOffer { pointer }
    }
}

impl FromPointer for () {
    fn from_pointer(_: Option<u16>) -> Self {}
}

impl FromPointer for UslpOffer {
    fn from_pointer(_: Option<u16>) -> Self {
        UslpOffer::default()
    }
}

/// Runs a sequence of inputs; `fresh` builds the configuration for a policy and frame length.
fn walk<P: MuxProtocol>(
    u: &mut Unstructured<'_>,
    fresh: &dyn Fn(MuxPolicy, usize) -> ChannelMuxConfig<P, V>,
) -> Result<()>
where
    P::OfferMeta: FromPointer,
{
    let frame_len = u.int_in_range(20..=F)?;
    let mut mux = ChannelMux::<P, V, F>::new(fresh(policy(u)?, frame_len)).unwrap();
    let mut out = [0_u8; F];
    for _ in 0..u.int_in_range(1..=80)? {
        let field_octets = {
            let n = u.int_in_range(0..=F)?.min(u.len());
            u.bytes(n)?.to_vec()
        };
        let ocf = [u.arbitrary()?; 4];
        let input = match u.int_in_range(0..=9)? {
            0..=3 => MuxInput::Offer(Offer {
                vc: u.int_in_range(0..=9)?,
                field: &field_octets,
                meta: P::OfferMeta::from_pointer(if u.arbitrary()? {
                    Some(u.int_in_range(0..=0x7FF)?)
                } else {
                    None
                }),
            }),
            4 | 5 => MuxInput::Opportunity,
            6 => MuxInput::SetOcf {
                scope: if u.arbitrary()? {
                    Scope::Mc
                } else {
                    Scope::Vc(u.int_in_range(0..=9)?)
                },
                ocf,
            },
            7 => MuxInput::SetInsert(&field_octets[..field_octets.len().min(4)]),
            8 => MuxInput::SetFsh {
                scope: Scope::Vc(u.int_in_range(0..=9)?),
                data: &field_octets[..field_octets.len().min(4)],
            },
            _ => {
                let len = u.int_in_range(20..=F + 8)?;
                MuxInput::Reconfigure(fresh(policy(u)?, len))
            }
        };
        match mux.step(input, &mut out) {
            Ok(actions) => {
                assert!(actions.len() <= CHANNEL_MUX_MAX_ACTIONS);
                for a in actions.iter() {
                    if let MuxOutput::Transmit(t) = a {
                        assert_eq!(t.frame.len(), mux.config().frame_len());
                        assert!(t.frame.end() <= F);
                        assert!(t.security_header.end() <= t.data.start());
                        assert!(t.data.end() <= t.security_trailer.start());
                    }
                }
            }
            Err(e) => {
                assert_ne!(e.kind(), ErrorKind::InternalError);
            }
        }
    }
    Ok(())
}

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    // An input too short for a scenario ends it.
    let _ = match u.int_in_range(0..=2) {
        Ok(0) => walk::<Tm>(&mut u, &tm_config),
        Ok(1) => walk::<Aos>(&mut u, &aos_config),
        Ok(_) => walk::<Uslp>(&mut u, &|p, l| uslp_config(p, l, false)),
        Err(e) => Err(e),
    };
});

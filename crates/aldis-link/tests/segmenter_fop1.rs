//! The FDUs and TFDFs of the `MapSegmenter` are the requests of FOP-1 (LNK-043; 232.1 §4.4,
//! §5.1.4): `NextFdu` names the request and the service, the caller hands the FDU to
//! `RequestAd` (sequence-controlled) or `RequestBd` (expedited), and FOP-1 builds the frame whose
//! data field is the FDU. The longest FDU of the segmenter is the longest FDU of the FOP-1 target.
//!
//! A TC VC without a Segment Header takes the FDU as it is (`Fop1Target::tc`). A USLP VC takes the
//! TFDZ, and FOP-1 writes the TFDF Header of its target, which for rule `111` is the header the
//! segmenter wrote for a whole SDU. Portions of an SDU (rules `100`, `101`, `110`) and a TC VC with
//! a Segment Header need FOP-1 to take the header of the FDU, which `Fop1Target` does not do yet
//! (OI-71).
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    unused_must_use,
    reason = "tests fail loudly by design, and many steps are run for their effect on the state"
)]

use core::num::NonZeroU8;

use aldis_cop1::{
    Directive, Fop1, Fop1Config, Fop1Input, Fop1Output, Fop1Store, Fop1Target, FrameKind,
    TimeoutType, fop1_max_actions,
};
use aldis_core::ids::{MapId, VcId};
use aldis_core::time::{Duration, Instant};
use aldis_link::common::Qos;
use aldis_link::protocol::{Tc, Uslp};
use aldis_link::segmenter::{
    Framing, MapEntry, MapSegmenter, MapSegmenterConfig, SegmentedFdu, SegmenterInput,
    SegmenterOutput, SegmenterService, TcSegmenterParams, UslpSegmenterParams,
};
use aldis_tc::{TcSpacecraftId, TcVcId};
use aldis_uslp::{
    MapConfig, PhysicalChannelConfig, TfdfHeader, TfdzRule, TransferFrameType, Upid, UslpMapId,
    UslpSpacecraftId, UslpVcId, VcConfig, VcParams, VcfCountLen,
};

const Q: usize = 4;
const F: usize = 64;
const N: usize = fop1_max_actions(Q);
const NOW: Instant = Instant::from_micros(0);

/// FOP-1 in State S1 (Active) over `target`, with its store.
struct Link {
    fop: Fop1<Q, F, N>,
    store: Fop1Store<Q, F>,
}

impl Link {
    fn active(target: Fop1Target) -> Link {
        let mut link = Link {
            fop: Fop1::new(Fop1Config {
                target,
                t1_initial: Duration::from_micros(1_000),
                transmission_limit: NonZeroU8::new(3).unwrap(),
                timeout_type: TimeoutType::Tt0,
                sliding_window_k: 2,
                ad_repetitions: 1,
                bc_repetitions: 1,
            })
            .unwrap(),
            store: Fop1Store::new(),
        };
        let directive = Fop1Input::Directive {
            handle: 0,
            directive: Directive::InitiateAdWithoutClcwCheck,
        };
        link.step(directive);
        link
    }

    /// Steps FOP-1 and returns the frame it transmits, if any.
    fn step(&mut self, input: Fop1Input<'_>) -> Option<(FrameKind, Vec<u8>)> {
        let mut out = [0_u8; F];
        let decision = self.fop.decide(&self.store, input, NOW, &mut out).unwrap();
        let actions = self.fop.commit(&mut self.store, decision);
        actions.iter().find_map(|a| {
            if let Fop1Output::Transmit { kind, frame, .. } = a {
                Some((*kind, frame.slice(&out).unwrap().to_vec()))
            } else {
                None
            }
        })
    }

    /// Hands the FDU of the segmenter to FOP-1 as the request of its service.
    fn request(&mut self, f: &SegmentedFdu, fdu: &[u8]) -> (FrameKind, Vec<u8>) {
        let input = match f.qos {
            Qos::SequenceControlled => Fop1Input::RequestAd {
                handle: f.handle,
                fdu,
            },
            Qos::Expedited => Fop1Input::RequestBd {
                handle: f.handle,
                fdu,
            },
        };
        let sent = self.step(input).unwrap();
        // The lower procedures accept the frame, so that the next request of the kind can go out
        // (232.1 §5.1.6).
        self.step(Fop1Input::TransmitDone {
            kind: sent.0,
            accepted: true,
        });
        sent
    }
}

fn next<P: aldis_link::segmenter::SegmenterProtocol, const M: usize, const L: usize>(
    s: &mut MapSegmenter<P, M, L>,
    handle: u32,
    qos: Qos,
) -> (SegmentedFdu, Vec<u8>) {
    let mut out = [0_u8; F];
    let a = s
        .step(SegmenterInput::NextFdu { handle, qos }, NOW, &mut out)
        .unwrap();
    let Some(SegmenterOutput::Fdu(f)) = a.get(0).copied() else {
        panic!("no FDU for request {handle}");
    };
    (f, f.fdu.slice(&out).unwrap().to_vec())
}

fn spp(total: usize, fill: u8) -> Vec<u8> {
    let mut p = vec![0x00, 0x01, 0xC0, 0x00];
    p.extend_from_slice(&u16::try_from(total - 7).unwrap().to_be_bytes());
    p.resize(total, fill);
    p
}

#[test]
fn tc_fdus_are_the_requests_of_fop1() {
    // req: LNK-043, LNK-040, COP-010
    let target = Fop1Target::tc(
        TcSpacecraftId::new(0x155).unwrap(),
        TcVcId::new(2).unwrap(),
        40,
        true,
    )
    .unwrap();
    // The segmenter's longest FDU is the target's: the frame length less the Primary Header and
    // the FECF.
    assert_eq!(target.max_fdu_len(), 33);
    let config = MapSegmenterConfig::<Tc, 1>::new(
        VcId::<6>::new(2).unwrap(),
        TcSegmenterParams::without_segment_header(true, true),
        target.max_fdu_len(),
        &[],
    )
    .unwrap();
    let mut seg = MapSegmenter::<Tc, 1, 128>::new(config).unwrap();
    let map = MapId::<6>::new(0).unwrap();
    let mut out = [0_u8; F];
    // Two packets share an FDU where blocking is permitted; a VCA_SDU is an FDU of its own.
    let (a, b, vca) = (spp(10, 1), spp(12, 2), vec![9_u8; 20]);
    for packet in [&a, &b] {
        seg.step(SegmenterInput::SubmitPacket { map, packet }, NOW, &mut out)
            .unwrap();
    }
    seg.step(SegmenterInput::SubmitSdu { map, sdu: &vca }, NOW, &mut out)
        .unwrap();
    let mut link = Link::active(target);
    let (f, fdu) = next(&mut seg, 11, Qos::SequenceControlled);
    assert_eq!((f.handle, f.framing, fdu.len()), (11, Framing::Whole, 22));
    let (kind, frame) = link.request(&f, &fdu);
    // The Primary Header is 5 octets and the FECF 2; the data field is the FDU.
    assert_eq!(kind, FrameKind::Ad);
    assert_eq!(&frame[5..frame.len() - 2], &fdu[..]);
    let (f, fdu) = next(&mut seg, 12, Qos::Expedited);
    assert_eq!((f.qos, &fdu), (Qos::Expedited, &vca));
    let (kind, frame) = link.request(&f, &fdu);
    assert_eq!(kind, FrameKind::Bd);
    assert_eq!(&frame[5..frame.len() - 2], &vca[..]);
    // The longest FDU fills the longest frame; the segmenter never makes a longer one.
    seg.step(
        SegmenterInput::SubmitSdu { map, sdu: &[7; 33] },
        NOW,
        &mut out,
    )
    .unwrap();
    let (f, fdu) = next(&mut seg, 13, Qos::SequenceControlled);
    let (_, frame) = link.request(&f, &fdu);
    assert_eq!(frame.len(), 40);
    let r = seg.step(
        SegmenterInput::SubmitSdu { map, sdu: &[7; 34] },
        NOW,
        &mut out,
    );
    assert!(r.is_err());
}

#[test]
fn uslp_tfdfs_of_whole_sdus_are_the_requests_of_fop1() {
    // req: LNK-043, LNK-042, COP-010, USLP-013
    let scid = UslpSpacecraftId::new(0x155).unwrap();
    let vcid = UslpVcId::new(5).unwrap();
    let mapid = UslpMapId::new(2).unwrap();
    let physical = PhysicalChannelConfig::new(TransferFrameType::Variable, F, 0, true).unwrap();
    // The VC the target needs: a one-octet sequence-controlled VCF Count, none for the
    // expedited service.
    let params = VcParams {
        vcf_count_len_sequence: VcfCountLen::new(1).unwrap(),
        ..VcParams::new(TransferFrameType::Variable)
    };
    let vc = VcConfig::new(scid, vcid, params).unwrap();
    let map = MapConfig::new(
        scid,
        vcid,
        mapid,
        aldis_core::security::SecurityLayout::NONE,
    );
    let header = TfdfHeader {
        rule: TfdzRule::NoSegmentation,
        upid: Upid::OctetStream,
    };
    let target = Fop1Target::uslp(physical, vc, map, header).unwrap();
    // The segmenter's TFDF is the target's FDU, its header, and the VCF Count lengths the same.
    let config = MapSegmenterConfig::<Uslp, 2>::new(
        VcId::<6>::new(5).unwrap(),
        UslpSegmenterParams::new(VcfCountLen::new(1).unwrap(), VcfCountLen::NONE),
        target.max_fdu_len() + 1,
        &[MapEntry::<Uslp>::new(
            MapId::<4>::new(2).unwrap(),
            SegmenterService::Sdu,
            Upid::OctetStream,
        )],
    )
    .unwrap();
    let mut seg = MapSegmenter::<Uslp, 2, 128>::new(config).unwrap();
    let map = MapId::<4>::new(2).unwrap();
    let mut out = [0_u8; F];
    let mut link = Link::active(target);
    for (handle, qos, kind) in [
        (21, Qos::SequenceControlled, FrameKind::Ad),
        (22, Qos::Expedited, FrameKind::Bd),
    ] {
        let sdu = vec![u8::try_from(handle).unwrap(); 17];
        seg.step(SegmenterInput::SubmitSdu { map, sdu: &sdu }, NOW, &mut out)
            .unwrap();
        let (f, tfdf) = next(&mut seg, handle, qos);
        assert_eq!((f.framing, f.map), (Framing::Whole, Some(2)));
        // Rule 111 and the UPID of the MAP: the header FOP-1 writes for its target.
        assert_eq!(tfdf[0], 0b111 << 5 | Upid::OctetStream.bits());
        let (sent, frame) = link.request(&f, &tfdf[1..]);
        assert_eq!(sent, kind);
        // The TFDF, the segmenter's header and zone, is the end of the frame before the FECF.
        assert_eq!(
            &frame[frame.len() - 2 - tfdf.len()..frame.len() - 2],
            &tfdf[..]
        );
    }
}

//! Arbitrary frames into the `FrameReceiver` of TM, AOS, USLP, and TC (VER-022, LNK-050…LNK-054,
//! LNK-074, LNK-075, API-057, API-058, GEN-020): a frame the codec accepts reaches the receiver as
//! a `Plain` frame (RS-023); whatever its octets, no step panics or reports an internal error,
//! none returns more actions than `FRAME_RECEIVER_MAX_ACTIONS`, every range an action names lies
//! inside the output area, and a refused frame leaves the receiver as an area of no octet leaves
//! it (API-058).
#![no_main]

// req: VER-022, LNK-050, LNK-051, LNK-052, LNK-053, LNK-054, LNK-074, LNK-075, API-057, API-058, GEN-020

#[path = "../../crates/aldis-link/tests/support/mod.rs"]
mod support;

use aldis_core::ErrorKind;
use aldis_core::ids::{MapId, VcId};
use aldis_link::protocol::{Aos, Tc, Tm, Uslp};
use aldis_link::receiver::{
    AosChannel, AosContent, AosVc, DeliveryService, FRAME_RECEIVER_MAX_ACTIONS, FrameReceiver,
    FrameReceiverConfig, MapSet, ReceiverInput, ReceiverOutput, TcChannel, TcVc, TmChannel,
    TmContent, TmVc, UslpChannel, UslpVc, VcEntry,
};
use libfuzzer_sys::fuzz_target;
use support::Plained;

const LEN: usize = 40;

fn vc<const B: u32>(n: u8) -> VcId<B> {
    VcId::<B>::new(n).unwrap()
}

fn tm() -> FrameReceiver<Tm, 8> {
    FrameReceiver::new(
        FrameReceiverConfig::new(
            LEN,
            false,
            TmChannel::new(DeliveryService::Vc, DeliveryService::Vc),
            &[
                VcEntry::new(vc(0), TmVc::new(TmContent::Packets, false, None).unwrap()),
                VcEntry::new(vc(1), TmVc::new(TmContent::Vca, false, None).unwrap()),
                VcEntry::new(
                    vc(5),
                    TmVc::new(TmContent::Unserved, true, Some(2)).unwrap(),
                ),
            ],
        )
        .unwrap(),
    )
}

fn aos() -> FrameReceiver<Aos, 8> {
    FrameReceiver::new(
        FrameReceiverConfig::new(
            LEN,
            false,
            AosChannel::new(2, false),
            &[
                VcEntry::new(vc(1), AosVc::new(AosContent::Packets, true)),
                VcEntry::new(vc(2), AosVc::new(AosContent::Vca, false)),
                VcEntry::new(vc(3), AosVc::new(AosContent::Bitstream, false)),
                VcEntry::new(vc(5), AosVc::new(AosContent::Unserved, false)),
            ],
        )
        .unwrap(),
    )
}

fn uslp() -> FrameReceiver<Uslp, 8> {
    let maps = |m: &[u8]| {
        MapSet::new(
            &m.iter()
                .map(|&n| MapId::<4>::new(n).unwrap())
                .collect::<Vec<_>>(),
        )
    };
    FrameReceiver::new(
        FrameReceiverConfig::new(
            LEN,
            false,
            UslpChannel::new(2, DeliveryService::Mc),
            &[
                VcEntry::new(vc(1), UslpVc::new(maps(&[0, 2]), false, true)),
                VcEntry::new(vc(2), UslpVc::new(maps(&[0]), true, true)),
                VcEntry::new(vc(3), UslpVc::new(maps(&[0]), false, false)),
            ],
        )
        .unwrap(),
    )
}

fn tc() -> FrameReceiver<Tc, 8> {
    FrameReceiver::new(
        FrameReceiverConfig::new(
            LEN,
            false,
            TcChannel,
            &[
                VcEntry::new(vc(1), TcVc::SERVED),
                VcEntry::new(vc(2), TcVc::UNSERVED),
            ],
        )
        .unwrap(),
    )
}

fn check<P: Plained, const V: usize>(rx: &mut FrameReceiver<P, V>, frame: &[u8]) {
    // A frame the codec refuses never reaches the engine (RS-023).
    let Ok(plain) = P::plain(rx.config(), frame) else {
        return;
    };
    let before = rx.clone();
    let mut area = vec![0u8; rx.config().area_len()];
    match rx.step(ReceiverInput::Frame(plain), &mut area) {
        Ok(actions) => {
            assert!(actions.len() <= FRAME_RECEIVER_MAX_ACTIONS);
            for o in actions.iter() {
                let range = match o {
                    ReceiverOutput::DataField { data, .. } => Some(*data),
                    ReceiverOutput::FshSdu { sdu, .. } | ReceiverOutput::InsertSdu { sdu, .. } => {
                        Some(*sdu)
                    }
                    ReceiverOutput::TcFrame { frame, .. }
                    | ReceiverOutput::ProtocolControl { frame, .. } => Some(*frame),
                    _ => None,
                };
                if let Some(r) = range {
                    assert!(r.end() <= area.len());
                }
            }
        }
        Err(e) => {
            assert_ne!(e.kind(), ErrorKind::InternalError);
            // A twin that refused the frame for an output area of no octet equals it.
            let mut twin = before;
            let again = ReceiverInput::Frame(P::plain(twin.config(), frame).unwrap());
            assert!(twin.step(again, &mut []).is_err());
            assert_eq!(twin, *rx);
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let (mut tmr, mut aosr, mut uslpr, mut tcr) = (tm(), aos(), uslp(), tc());
    // Frames of the channel length, one after another, so that the counters and the gaps carry
    // from frame to frame; the first octets steer the VC of each.
    for chunk in data.chunks(LEN).filter(|c| c.len() == LEN) {
        let mut f = chunk.to_vec();
        check(&mut tmr, &f);
        check(&mut aosr, &f);
        check(&mut uslpr, &f);
        check(&mut tcr, &f);
        // The same octets as a USLP frame with the version bits of the Primary Header set.
        f[0] = 0xC0 | (f[0] & 0x0F);
        check(&mut uslpr, &f);
    }
});

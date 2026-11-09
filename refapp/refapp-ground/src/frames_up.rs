//! The uplink at frame level: the `MapSegmenter`, FOP-1 for Type-BD, and the TC frame
//! (A2 WP-4b).
//!
//! Specification: `docs/reference-application.md` §2.2 (order within an instant), §4.2, §4.4.4
//! (uplink frames); APP-040…APP-043, APP-071, APP-090, APP-133; LNK-040…LNK-043; COP-010,
//! TC-008.
//!
//! The ops-script packets are submitted to the segmenter when they are released: commands on
//! MAP 0, the table upload on MAP 1. In a step with `tx_slot`, [`Uplink::slot`] asks the
//! segmenter for one FDU with the expedited quality of service, gives it to FOP-1 as a
//! `RequestBd`, and returns the Type-BD frame (N(S) = 0, the FECF) that FOP-1 builds. Nothing
//! else of COP-1 is used (D2 of the A2 plan).

use core::num::NonZeroU8;

use aldis::cop1::{
    Fop1, Fop1Config, Fop1Input, Fop1Output, Fop1Store, Fop1Target, FrameKind, TimeoutType,
    fop1_max_actions,
};
use aldis::core::Error;
use aldis::core::ErrorKind;
use aldis::core::ids::{MapId, SpacecraftId, VcId};
use aldis::core::time::{Duration, Instant};
use aldis::link::common::Qos;
use aldis::link::protocol::Tc;
use aldis::link::segmenter::{
    MapEntry, MapSegmenter, MapSegmenterConfig, MapService, SegmenterInput, SegmenterOutput,
    TcSegmenterParams,
};
use refapp_scenario::Apid;
use refapp_scenario::generated::{apid, cop1, tc};

/// Capacity of the segmenter's queue, octets: the longest packet (512) with its entry, and the
/// packets of one instant besides.
const QUEUE: usize = 1024;
/// Entries of the `Sent_Queue` of FOP-1: K. Type-BD frames never enter it, so K bounds nothing
/// here (D2).
const SENT: usize = 4;
/// Octets of a frame, the capacity of FOP-1's master copies and of the output areas.
const FRAME: usize = 128;
/// Octets of an FDU (Segment Header included).
const FDU: usize = 121;
/// Actions of one FOP-1 input (COP-006).
const ACTIONS: usize = fop1_max_actions(SENT);

/// The sending end of the uplink at frame level.
#[derive(Debug, Clone)]
pub(crate) struct Uplink {
    segmenter: MapSegmenter<Tc, 2, QUEUE>,
    fop: Fop1<SENT, FRAME, ACTIONS>,
    store: Fop1Store<SENT, FRAME>,
    /// The handle of the next request (API-054).
    next_handle: u32,
    /// Inputs the engines refused; zero in a run whose script fits the capacities.
    refused: u32,
}

/// The MAP of the packets of `apid` (APP-041, §4.2).
const fn map_of(apid: Apid) -> Option<MapId<6>> {
    match apid.get() {
        apid::COMMANDS => Some(MapId::<6>::from_const::<{ tc::MAP_COMMANDS }>()),
        apid::TABLE_UPLOAD => Some(MapId::<6>::from_const::<{ tc::MAP_TABLE_UPLOAD }>()),
        _ => None,
    }
}

/// A MAP of the TC VC as the scenario configures it (232.0 Table 5-4; APP-136).
fn map_entry(id: u8, blocking: bool, segmentation: bool) -> Result<MapEntry<Tc>, Error> {
    let bad = Error::new(ErrorKind::InvalidConfig);
    let id = MapId::<6>::new(id).ok_or(bad)?;
    let entry = MapEntry::<Tc>::new(id, MapService::Any, ());
    let entry = if blocking {
        entry.with_blocking()
    } else {
        entry
    };
    if segmentation {
        entry.with_segmentation(usize::from(tc::MAX_PACKET_LENGTH))
    } else {
        Ok(entry)
    }
}

impl Uplink {
    /// The segmenter and FOP-1 of the uplink, from the scenario's values.
    ///
    /// # Errors
    /// The library's `InvalidConfig` if a scenario value is out of range for the engines.
    pub(crate) fn new() -> Result<Uplink, Error> {
        let bad = Error::new(ErrorKind::InvalidConfig);
        let spacecraft =
            SpacecraftId::<10>::new(refapp_scenario::generated::SPACECRAFT_ID).ok_or(bad)?;
        let vc = VcId::<6>::new(tc::VCID).ok_or(bad)?;
        let target = Fop1Target::tc(spacecraft, vc, usize::from(tc::MAX_FRAME_LENGTH), tc::FECF)?;
        // The capacities are constants of this module; the scenario's values must agree.
        let max_fdu = usize::from(tc::MAX_FDU_LENGTH);
        if usize::from(cop1::K) != SENT
            || usize::from(tc::MAX_FRAME_LENGTH) != FRAME
            || max_fdu != FDU
        {
            return Err(bad);
        }
        // The frame data field is the FDU, Segment Header included (OI-71): the target's
        // longest FDU is the scenario's.
        if target.max_fdu_len() != max_fdu || !tc::SEGMENT_HEADER {
            return Err(bad);
        }
        let config = MapSegmenterConfig::<Tc, 2>::new(
            vc,
            TcSegmenterParams::with_segment_header(tc::FECF),
            max_fdu,
            &[
                map_entry(
                    tc::MAP_COMMANDS,
                    tc::MAP_COMMANDS_BLOCKING,
                    tc::MAP_COMMANDS_SEGMENTATION,
                )?,
                map_entry(
                    tc::MAP_TABLE_UPLOAD,
                    tc::MAP_TABLE_UPLOAD_BLOCKING,
                    tc::MAP_TABLE_UPLOAD_SEGMENTATION,
                )?,
            ],
        )?;
        let timeout_type = match cop1::TIMEOUT_TYPE {
            0 => TimeoutType::Tt0,
            _ => return Err(bad),
        };
        let fop = Fop1::new(Fop1Config {
            target,
            t1_initial: Duration::from_micros(cop1::T1_INITIAL_US),
            transmission_limit: NonZeroU8::new(cop1::TRANSMISSION_LIMIT).ok_or(bad)?,
            timeout_type,
            sliding_window_k: cop1::K,
            ad_repetitions: cop1::FOP_REPETITIONS_TYPE_A,
            bc_repetitions: cop1::FOP_REPETITIONS_CONTROL,
        })?;
        Ok(Uplink {
            segmenter: MapSegmenter::new(config)?,
            fop,
            store: Fop1Store::new(),
            next_handle: 0,
            refused: 0,
        })
    }

    /// Inputs the engines refused so far.
    pub(crate) const fn refused(&self) -> u32 {
        self.refused
    }

    /// Hands a packet of the script to the segmenter (APP-042). A packet of another APID, or one
    /// the queue cannot take whole, is counted in [`refused`](Self::refused).
    pub(crate) fn submit(&mut self, apid: Apid, packet: &[u8], now: Instant) {
        let Some(map) = map_of(apid) else {
            self.refused = self.refused.saturating_add(1);
            return;
        };
        let mut unused = [0_u8; 0];
        let input = SegmenterInput::SubmitPacket { map, packet };
        if self.segmenter.step(input, now, &mut unused).is_err() {
            self.refused = self.refused.saturating_add(1);
        }
    }

    /// One uplink slot: the next FDU as a Type-BD frame, or `None` if the segmenter holds none
    /// (APP-071, APP-133).
    pub(crate) fn slot(&mut self, now: Instant) -> Option<Vec<u8>> {
        let handle = self.next_handle;
        self.next_handle = self.next_handle.wrapping_add(1);
        let mut area = [0_u8; FDU];
        let request = SegmenterInput::NextFdu {
            handle,
            qos: Qos::Expedited,
        };
        let Ok(actions) = self.segmenter.step(request, now, &mut area) else {
            self.refused = self.refused.saturating_add(1);
            return None;
        };
        let SegmenterOutput::Fdu(fdu) = actions.iter().next().copied()? else {
            return None;
        };
        let Some(octets) = fdu.fdu.slice(&area) else {
            self.refused = self.refused.saturating_add(1);
            return None;
        };
        self.request_bd(handle, octets, now)
    }

    /// FOP-1 `RequestBd` for `fdu` and the response of the lower procedures that frees the
    /// Bypass Out flag (232.1 §5.1.6): returns the frame FOP-1 transmits.
    fn request_bd(&mut self, handle: u32, fdu: &[u8], now: Instant) -> Option<Vec<u8>> {
        let frame = self.fop_step(Fop1Input::RequestBd { handle, fdu }, now)?;
        let done = Fop1Input::TransmitDone {
            kind: FrameKind::Bd,
            accepted: true,
        };
        self.fop_step(done, now);
        Some(frame)
    }

    /// One FOP-1 input; returns the frame its Transmit action holds, if any.
    fn fop_step(&mut self, input: Fop1Input<'_>, now: Instant) -> Option<Vec<u8>> {
        let mut out = [0_u8; FRAME];
        let Ok(decision) = self.fop.decide(&self.store, input, now, &mut out) else {
            self.refused = self.refused.saturating_add(1);
            return None;
        };
        let actions = self.fop.commit(&mut self.store, decision);
        actions.iter().find_map(|action| match action {
            Fop1Output::Transmit { frame, .. } => frame.slice(&out).map(<[u8]>::to_vec),
            Fop1Output::Abort
            | Fop1Output::Transfer(_)
            | Fop1Output::Directive(_)
            | Fop1Output::Alert(_)
            | Fop1Output::Suspend => None,
        })
    }
}

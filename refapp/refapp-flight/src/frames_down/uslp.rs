//! The USLP arm of the flight downlink (A4 WP-3b), as the application's [`DownProtocol`].
//!
//! Specification: docs/reference-application.md §4.3.2: fixed-length USLP frames of 223 octets with
//! the FECF and no Insert Zone (APP-175); VCs 0 and 1 on MAP 0 with the rule `000`, expedited,
//! with a one-octet VCF Count (APP-176); the packets of the TFDZ and the Encapsulation Idle Packet
//! that completes a released field (APP-177); OID frames on VCID 63 whenever neither VC has a
//! released field (APP-178); the CLCW in the MC OCF of every frame (APP-179).
//!
//! Requirements: APP-175…APP-179.
//!
//! **The field.** The generator builds the TFDF: the header of 3 octets (rule `000`, the UPID, the
//! First Header Pointer) and the TFDZ of 206, 209 octets in all. The frame is the primary header
//! (7), the VCF Count (1), the TFDF (209), the MC OCF (4), and the FECF (2).
//!
//! **Idle.** A protocol that fixes the idle packet kind, as USLP does (732.1 §4.1.4.3.4), has the
//! generator complete a released field with one Encapsulation Idle Packet (LNK-013). With no
//! released field the multiplexer sends an OID frame on VCID 63 (LNK-030), expedited, without a
//! VCF Count, and with the OCF of the MC (LNK-033, OI-76).
//!
//! **The frame.** The mux leaves the FECF zero (LNK-035); it is the only field finished after it.

use aldis::core::crc::write_fecf;
use aldis::core::ids::{MapId, SpacecraftId, VcId};
use aldis::core::{Error, ErrorKind, Result};
use aldis::link::common::Qos;
use aldis::link::mux::{
    ChannelMux, ChannelMuxConfig, MuxInput, Scope, UslpChannel, UslpOffer, UslpVc, VcConfig,
};
use aldis::link::policy::MuxPolicy;
use aldis::link::protocol::{Uslp, UslpParams};
use aldis::uslp::{
    FirstHeaderPointer, OcfUse, PhysicalChannelConfig, SourceOrDestination, TransferFrameType,
    Upid, VcConfig as CodecVcConfig, VcParams, VcfCountLen,
};
use refapp_scenario::generated::{SPACECRAFT_ID, tm, uslp_down as down};

use super::{DownProtocol, Mux, mux_priority, none};

/// Octets of the field handed to the generator: the TFDF, its header of 3 and the TFDZ of 206.
/// The frame is the primary header (7), the VCF Count (1), the TFDF (209), the OCF (4), and the
/// FECF (2) (APP-175, APP-176).
const TFDF: usize = 209;

const _: () = assert!(7 + 1 + TFDF + 4 + 2 == super::FRAME);

/// The geometry above has the FECF, the MC OCF, and the one-octet expedited VCF Count; the node
/// builds only with the scenario's values (APP-175, APP-176, D5).
const _: () = {
    assert!(down::FECF && down::OCF_REQUIRED);
    assert!(down::VCF_COUNT_LENGTH_EXPEDITED == 1 && down::VCF_COUNT_LENGTH_SEQUENCE == 0);
    assert!(down::MAP_ID == 0);
};

/// The VCs that carry data are the VCs of TM, so that `vc_of` serves both (§4.1, §4.3.2).
const _: () = {
    assert!(down::VCID_REALTIME == tm::VCID_REALTIME);
    assert!(down::VCID_PAYLOAD == tm::VCID_PAYLOAD);
};

/// The VC `id` with the MC OCF required, the expedited count of one octet and none for the
/// sequence-controlled service, and MAP 0 (APP-175, APP-176; OI-99).
fn vc(scid: SpacecraftId<16>, id: u8, rank: u8) -> Result<VcConfig<Uslp>> {
    let mut codec = VcParams::new(TransferFrameType::Fixed);
    codec.vcf_count_len_sequence = VcfCountLen::new(down::VCF_COUNT_LENGTH_SEQUENCE)?;
    codec.vcf_count_len_expedited = VcfCountLen::new(down::VCF_COUNT_LENGTH_EXPEDITED)?;
    codec.ocf = OcfUse::Required;
    let config = CodecVcConfig::new(scid, VcId::<6>::try_from(id)?, codec)?;
    let params = UslpVc::new(
        config,
        MapId::<4>::try_from(down::MAP_ID)?,
        Qos::Expedited,
        false,
    )?;
    Ok(VcConfig::<Uslp>::new(params).with_priority(mux_priority(rank)))
}

impl DownProtocol for Uslp {
    const FIELD_LEN: usize = TFDF;
    const DATA_VCS: [u8; 2] = [down::VCID_REALTIME, down::VCID_PAYLOAD];
    const DEADLINES_US: [u64; 2] = [
        down::RELEASE_DEADLINE_REALTIME_US,
        down::RELEASE_DEADLINE_PAYLOAD_US,
    ];
    const IDLE_VC: Option<u8> = None;
    const MAX_PACKET_LEN: u16 = down::MAX_PACKET_LENGTH;
    const IDLE_PATTERN: u8 = down::IDLE_PATTERN;

    /// MAP 0, expedited, and the UPID of Space Packets and Encapsulation packets, by its name in
    /// the SANA table (OI-06; APP-176).
    fn generator_params() -> Result<UslpParams> {
        UslpParams::new(
            Upid::Packets,
            MapId::<4>::try_from(down::MAP_ID)?,
            Qos::Expedited,
        )
    }

    /// The multiplexer: one master channel with the SCID of §3.4 as the Source, frames of 223
    /// octets with the FECF and no Insert Zone, the MC OCF in every frame, OID frames without a
    /// VCF Count, and VCs 0 and 1 with VC 0 first (APP-175, APP-176, APP-178).
    fn mux() -> Result<Mux<Self>> {
        let scid =
            SpacecraftId::<16>::new(SPACECRAFT_ID).ok_or(Error::new(ErrorKind::InvalidConfig))?;
        let physical = PhysicalChannelConfig::new(
            TransferFrameType::Fixed,
            usize::from(down::FRAME_LENGTH),
            0,
            down::FECF,
        )?;
        let channel = UslpChannel::new(
            physical,
            scid,
            SourceOrDestination::Source,
            VcfCountLen::NONE,
            Some(tm::OCF_INITIAL.to_be_bytes()),
        )?;
        let vcs = [
            vc(scid, down::VCID_REALTIME, down::PRIORITY_REALTIME)?,
            vc(scid, down::VCID_PAYLOAD, down::PRIORITY_PAYLOAD)?,
        ];
        let config = ChannelMuxConfig::<Self, 3>::new(channel, MuxPolicy::FixedPriority, &vcs)?;
        ChannelMux::new(config)
    }

    /// The route of a USLP field is the VC's own (MAP 0, expedited).
    fn offer_meta(_: FirstHeaderPointer) -> UslpOffer {
        UslpOffer::default()
    }

    /// The one OCF source of the master channel (APP-179).
    fn set_ocf(mux: &mut Mux<Self>, ocf: [u8; 4]) -> Result<()> {
        none(mux.step(
            MuxInput::SetOcf {
                scope: Scope::Mc,
                ocf,
            },
            &mut [],
        )?)
    }

    /// The FECF, the only field the mux leaves to the caller (LNK-035).
    fn finish(frame: &mut [u8]) -> Result<()> {
        write_fecf(frame)
    }
}

/// The frames of the USLP downlink, driven through the module's own `submit` and `slot`.
#[cfg(test)]
mod tests {
    extern crate std;

    use std::vec::Vec;

    use aldis::core::crc::crc16;
    use aldis::uslp::Upid;
    use refapp_scenario::Instant;
    use refapp_scenario::generated::{SPACECRAFT_ID, tm};

    use super::super::{FRAME, FrameDownlink};
    use super::Uslp;
    use crate::tests::{At, Out, Rec};

    /// A space packet of `len` octets (PVN 0, APID 1, unsegmented), at least 7.
    fn packet(len: usize) -> Vec<u8> {
        let mut p = std::vec![0_u8; len];
        let length = u16::try_from(len.saturating_sub(7)).unwrap();
        p[..6].copy_from_slice(&[0x00, 0x01, 0xC0, 0x00, 0, 0]);
        p[4..6].copy_from_slice(&length.to_be_bytes());
        p
    }

    /// The frames of one slot at `micros`.
    fn slot(down: &mut FrameDownlink<Uslp>, micros: u64) -> Vec<u8> {
        let mut rec = Rec::default();
        down.slot(Instant::from_micros(micros), &mut At(0, &mut rec))
            .unwrap();
        match rec.0.as_slice() {
            [(_, Out::Unit(frame))] => frame.clone(),
            other => panic!("one frame expected, got {other:?}"),
        }
    }

    /// The VCID of a frame: 6 bits over the octets 2 and 3.
    fn vcid(frame: &[u8]) -> u8 {
        (frame[2] & 0x07) << 3 | frame[3] >> 5
    }

    /// The TFDZ of a frame with a VCF Count of one octet: the TFDF header is 3 octets.
    fn zone(frame: &[u8]) -> &[u8] {
        &frame[11..frame.len().saturating_sub(6)]
    }

    /// A submitted packet on VC 0 at 0 s and the frame of the slot after its deadline (250 ms).
    fn data_frame(down: &mut FrameDownlink<Uslp>, len: usize) -> Vec<u8> {
        down.submit(0, &packet(len), Instant::from_micros(0))
            .unwrap();
        slot(down, 1_000_000)
    }

    #[test]
    fn every_frame_is_a_fixed_length_uslp_frame_with_the_fecf_and_the_mc_ocf() {
        // req: APP-175
        let mut down = FrameDownlink::<Uslp>::new().unwrap();
        let data = data_frame(&mut down, 20);
        let oid = slot(&mut down, 2_000_000);
        for frame in [&data, &oid] {
            assert_eq!(frame.len(), FRAME);
            assert_eq!(frame.len(), 223);
            // TFVN `1100` and the SCID of §3.4 over 4 + 16 bits.
            assert_eq!(frame[0] >> 4, 0b1100);
            let scid = (u16::from(frame[0] & 0x0F) << 12)
                | (u16::from(frame[1]) << 4)
                | u16::from(frame[2] >> 4);
            assert_eq!(scid, SPACECRAFT_ID);
            // The FECF is the CRC of the whole frame: the CRC over it, FECF included, is 0.
            assert_eq!(crc16(frame).get(), 0);
            // The OCF flag, and the OCF ahead of the FECF: the initial CLCW.
            assert_eq!(frame[6] & 0x08, 0x08);
            assert_eq!(frame[FRAME - 6..FRAME - 2], tm::OCF_INITIAL.to_be_bytes());
        }
        // Frame Length is the length less 1, in the octets 4 and 5 (222).
        assert_eq!(u16::from_be_bytes([data[4], data[5]]), 222);
        // The VCs 0 and 1 are served, VC 0 first; VCID 63 is not among them.
        down.submit(1, &packet(20), Instant::from_micros(2_000_000))
            .unwrap();
        down.submit(0, &packet(20), Instant::from_micros(2_000_000))
            .unwrap();
        assert_eq!(vcid(&slot(&mut down, 4_000_000)), 0);
        assert_eq!(vcid(&slot(&mut down, 5_000_000)), 1);
    }

    #[test]
    fn a_data_frame_has_the_header_of_the_expedited_unsegmented_downlink() {
        // req: APP-176
        let mut down = FrameDownlink::<Uslp>::new().unwrap();
        let frame = data_frame(&mut down, 20);
        // Source-or-Destination Identifier 0 (the Source), VCID 0, MAP ID 0, EOFPH 0.
        assert_eq!(frame[2] & 0x08, 0);
        assert_eq!(vcid(&frame), 0);
        assert_eq!(frame[3] & 0x1F, 0);
        // Bypass/Sequence Control 1 and Protocol Control Command 0, spares `00`, OCF flag 1,
        // VCF Count Length `001`.
        assert_eq!(frame[6], 0b1000_1001);
        // The VCF Count, 0 for the first frame of the VC.
        assert_eq!(frame[7], 0);
        // The TFDF header: rule `000`, the UPID of Packets, and the First Header Pointer 0.
        assert_eq!(frame[8], Upid::Packets.bits());
        assert_eq!(frame[8] >> 5, 0);
        assert_eq!(frame[9..11], [0, 0]);
        // A TFDZ of 206 octets.
        assert_eq!(zone(&frame).len(), 206);
    }

    #[test]
    fn the_vcf_count_is_kept_per_vc_and_wraps_modulo_256() {
        // req: APP-176
        let mut down = FrameDownlink::<Uslp>::new().unwrap();
        let mut counts = Vec::new();
        for i in 0..258_u64 {
            let at = i * 2_000_000;
            down.submit(0, &packet(20), Instant::from_micros(at))
                .unwrap();
            if i == 1 {
                // A frame of VC 1 between two of VC 0 does not move the count of VC 0.
                down.submit(1, &packet(20), Instant::from_micros(at))
                    .unwrap();
                let frame = slot(&mut down, at + 1_500_000);
                assert_eq!((vcid(&frame), frame[7]), (0, 1));
                let frame = slot(&mut down, at + 1_600_000);
                assert_eq!((vcid(&frame), frame[7]), (1, 0));
                continue;
            }
            counts.push(slot(&mut down, at + 1_500_000)[7]);
        }
        assert_eq!(counts[..3], [0, 2, 3]);
        assert_eq!(counts[254..], [255, 0, 1]);
    }

    #[test]
    fn a_released_partial_field_is_completed_with_one_encapsulation_idle_packet() {
        // req: APP-177, LNK-013
        let mut down = FrameDownlink::<Uslp>::new().unwrap();
        let frame = data_frame(&mut down, 20);
        let zone = zone(&frame);
        assert_eq!(zone[..20], packet(20));
        // The Idle Packet of the remainder (186 octets): a 2-octet header, the Encapsulation PVN
        // `111` in the first 3 bits, and the idle data octets (133.1 Table 4-2).
        assert_eq!(zone[20] >> 5, 0b111);
        assert_eq!(zone[20] & 0x03, 0b01);
        assert_eq!(zone[21], 186);
        assert!(zone[22..].iter().all(|&b| b == 0));
        // The first packet starts at 0.
        assert_eq!(frame[9..11], [0, 0]);
    }

    #[test]
    fn the_first_header_pointer_names_the_first_start_and_is_all_ones_for_none() {
        // req: APP-177, OI-69
        let mut down = FrameDownlink::<Uslp>::new().unwrap();
        // A packet of 300 octets: the TFDZ of 206 holds its first 206 octets, and the next field
        // its last 94, which a packet does not start, and then the Idle Packet, which does.
        down.submit(0, &packet(300), Instant::from_micros(0))
            .unwrap();
        let first = slot(&mut down, 1_000_000);
        assert_eq!(first[9..11], [0, 0]);
        let second = slot(&mut down, 2_000_000);
        assert_eq!(second[9..11], 94_u16.to_be_bytes());
        assert_eq!(zone(&second)[94] >> 5, 0b111);
        assert_eq!(zone(&second)[95], 112);
        // A packet of 412 octets fills two TFDZs: no packet starts in the second, which the
        // packet ends in (OI-69), so the pointer is all ones.
        down.submit(0, &packet(412), Instant::from_micros(3_000_000))
            .unwrap();
        let first = slot(&mut down, 4_000_000);
        assert_eq!(first[9..11], [0, 0]);
        let second = slot(&mut down, 5_000_000);
        assert_eq!(second[9..11], [0xFF, 0xFF]);
    }

    #[test]
    fn a_slot_without_a_released_field_sends_an_oid_frame_on_vcid_63() {
        // req: APP-178
        let mut down = FrameDownlink::<Uslp>::new().unwrap();
        let oid = slot(&mut down, 0);
        assert_eq!(vcid(&oid), 63);
        // MAP ID 0, Bypass/Sequence Control 1, Protocol Control Command 0, OCF flag 1, and the
        // VCF Count Length `000`: no count follows the header.
        assert_eq!(oid[3] & 0x1F, 0);
        assert_eq!(oid[6], 0b1000_1000);
        // The TFDF header at the octet 7: rule `001`, the UPID of only idle data, the Last Valid
        // Octet Pointer at the last octet of the TFDZ (206), and a TFDZ of 207 octets.
        assert_eq!(oid[7] >> 5, 0b001);
        assert_eq!(oid[7] & 0x1F, Upid::OnlyIdleData.bits());
        assert_eq!(oid[8..10], 206_u16.to_be_bytes());
        assert_eq!(oid.len() - 10 - 6, 207);
        // A data field released at its slot goes before the OID frame, and VCID 63 carries no
        // data frame.
        down.submit(1, &packet(20), Instant::from_micros(0))
            .unwrap();
        let data = slot(&mut down, 1_000_000);
        assert_eq!(vcid(&data), 1);
        assert_eq!(vcid(&slot(&mut down, 2_000_000)), 63);
        // The OID generator is never restarted (LNK-036): the next frame's zone continues it.
        let next = slot(&mut down, 3_000_000);
        assert_ne!(oid[10..10 + 207], next[10..10 + 207]);
    }

    #[test]
    fn the_ocf_of_every_frame_is_the_clcw_of_the_slot() {
        // req: APP-179
        let mut down = FrameDownlink::<Uslp>::new().unwrap();
        // A Type-1 report: the Type Flag, the first bit of the OCF, is 0.
        let clcw = [0x01, 0x02, 0x03, 0x04];
        down.set_ocf(clcw);
        let oid = slot(&mut down, 0);
        let data = data_frame(&mut down, 20);
        for frame in [&oid, &data] {
            assert_eq!(frame[FRAME - 6..FRAME - 2], clcw);
            assert_eq!(frame[FRAME - 6] & 0x80, 0);
            assert_eq!(crc16(frame).get(), 0);
        }
        let later = [0x01, 0x02, 0x03, 0x05];
        down.set_ocf(later);
        assert_eq!(slot(&mut down, 3_000_000)[FRAME - 6..FRAME - 2], later);
    }
}

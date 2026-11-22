//! FARM-1 over USLP: `Farm1Frame::from_uslp` and `Farm1Input::from_uslp` build the frame input
//! from a checked USLP frame, as `from_tc` does for TC (COP-031, COP-037, USLP-013, USLP-014,
//! API-016). One case per frame kind, the Type-BC frames that are not one Control Command (E9),
//! a foreign VCID, the refusals of the conversion, and equivalence with the input a flight node
//! built by hand before the constructor existed (issue #718).
#![cfg(feature = "uslp")]
#![expect(
    clippy::unwrap_used,
    reason = "test-only harness: a violated assumption must fail loudly (RS-018)"
)]

use aldis_cop1::{
    Farm1, Farm1Config, Farm1Event, Farm1Frame, Farm1Input, Farm1Notify, Farm1Output, Farm1State,
    Farm1Window,
};
use aldis_core::ids::FrameSeqNum;
use aldis_core::security::SecurityLayout;
use aldis_core::time::Instant;
use aldis_core::{Checked, DecodeMode, ErrorKind, field};
use aldis_tc::{ClcwStatus, ControlCommand, TcVcId};
use aldis_uslp::{
    FrameContent, FrameFields, FrameKind, MapConfig, OcfUse, PhysicalChannelConfig,
    SourceOrDestination, TfdfHeader, TfdzRule, TransferFrameType, TruncatedFrameLen, Upid,
    UslpFrame, UslpMapId, UslpSpacecraftId, UslpVcId, VcConfig, VcParams, VcTable, VcfCountLen,
    check_frame, encode, encode_truncated,
};

const NOW: Instant = Instant::from_micros(0);
const VC: u8 = 5;

const fn scid() -> UslpSpacecraftId {
    UslpSpacecraftId::new(0x155).unwrap()
}

const fn vcid() -> UslpVcId {
    UslpVcId::new(VC).unwrap()
}

const fn mapid() -> UslpMapId {
    UslpMapId::new(2).unwrap()
}

/// A variable-length channel with the FECF, a VC with the given count length for the
/// Sequence-Controlled service, and a truncated frame length of 12.
fn channels(seq_len: u8) -> VcTable<1, 1> {
    let physical = PhysicalChannelConfig::new(TransferFrameType::Variable, 64, 0, true).unwrap();
    let params = VcParams {
        vcf_count_len_sequence: VcfCountLen::new(seq_len).unwrap(),
        vcf_count_len_expedited: VcfCountLen::new(0).unwrap(),
        ocf: OcfUse::Absent,
        ..VcParams::new(TransferFrameType::Variable)
    };
    let vc = VcConfig::new(scid(), vcid(), params).unwrap();
    let map = MapConfig::new(scid(), vcid(), mapid(), SecurityLayout::NONE);
    VcTable::new(physical, &[vc], &[map]).unwrap()
}

const fn fields(kind: FrameKind, vcf_count: u64) -> FrameFields {
    FrameFields {
        spacecraft_id: scid(),
        source_or_destination: SourceOrDestination::Destination,
        vc_id: vcid(),
        map_id: mapid(),
        kind,
        vcf_count,
    }
}

const fn tfdf(rule: TfdzRule, upid: Upid) -> TfdfHeader {
    TfdfHeader { rule, upid }
}

const OCTET_STREAM: Upid = Upid::OctetStream;

fn encoded(
    table: &VcTable<1, 1>,
    f: FrameFields,
    header: TfdfHeader,
    tfdz: &[u8],
) -> std::vec::Vec<u8> {
    let content = FrameContent {
        tfdf: header,
        insert_zone: &[],
        ocf: None,
    };
    let mut out = [0_u8; 64];
    let n = encode(table, f, content, tfdz, &mut out).unwrap();
    out.get(..n).unwrap().to_vec()
}

fn check<'a>(table: &VcTable<1, 1>, octets: &'a [u8]) -> Checked<UslpFrame<'a>> {
    check_frame(octets, table, DecodeMode::Strict)
        .unwrap()
        .into_view()
}

/// What the flight node of the reference application built by hand before `from_uslp`
/// (`refapp-flight`, `frames_up/uslp.rs`, issue #718): the kind decides the variant, N(S) is the
/// VCF Count, the data is the TFDF, and a Type-BC frame that is not one Control Command under UPID
/// "COP-1 Control Commands" and rule `111` is refused (`None`, matching `ReservedValue`).
fn by_hand<'a>(checked: &Checked<UslpFrame<'a>>) -> Option<Farm1Frame<'a>> {
    let frame = checked.as_frame();
    let full = frame.full()?;
    let tfdf = full.octets().get(full.layout().data())?;
    Some(match full.kind() {
        FrameKind::SequenceControlled => Farm1Frame::Ad {
            sequence_number: FrameSeqNum::new(u8::try_from(full.vcf_count().value()).ok()?),
            data: tfdf,
        },
        FrameKind::Expedited => Farm1Frame::Bd { data: tfdf },
        FrameKind::ProtocolControl => {
            let command = if full.tfdf_header().upid == Upid::Cop1ControlCommands
                && full.tfdf_header().rule == TfdzRule::NoSegmentation
            {
                ControlCommand::parse(full.tfdz())
            } else {
                None
            }?;
            Farm1Frame::Bc {
                command: Some(command),
            }
        }
    })
}

fn bc_frame(header: TfdfHeader, tfdz: &[u8]) -> std::vec::Vec<u8> {
    encoded(
        &channels(1),
        fields(FrameKind::ProtocolControl, 0),
        header,
        tfdz,
    )
}

const fn cop1() -> TfdfHeader {
    tfdf(TfdzRule::NoSegmentation, Upid::Cop1ControlCommands)
}

fn farm() -> Farm1 {
    let config = Farm1Config {
        vc_id: TcVcId::new(VC).unwrap(),
        status: ClcwStatus::new(0).unwrap(),
        window: Farm1Window::retransmission_allowed(16).unwrap(),
    };
    Farm1::new(config, FrameSeqNum::new(7))
}

#[test]
fn a_sequence_controlled_frame_is_a_type_ad_frame_with_the_vcf_count_as_n_s() {
    // req: COP-031, COP-037, USLP-014, API-016
    let table = channels(1);
    let octets = encoded(
        &table,
        fields(FrameKind::SequenceControlled, 7),
        tfdf(TfdzRule::NoSegmentation, OCTET_STREAM),
        &[1, 2, 3],
    );
    let checked = check(&table, &octets);
    let tfdf_octets: &[u8] = octets.get(7 + 1..octets.len() - 2).unwrap();
    let want = Farm1Frame::Ad {
        sequence_number: FrameSeqNum::new(7),
        data: tfdf_octets,
    };
    assert_eq!(Farm1Frame::from_uslp(&checked).unwrap(), want);
    assert_eq!(tfdf_octets.len(), 1 + 3, "the TFDF Header and the TFDZ");
    assert_eq!(
        Farm1Input::from_uslp(&checked, true).unwrap(),
        Farm1Input::Frame {
            vc_id: TcVcId::new(VC).unwrap(),
            frame: want,
            buffer_available: true,
        }
    );
    // The frame is accepted at N(S) = V(R), E1, and the input of E2 differs by the flag only.
    let mut out = [0_u8; 16];
    let (next, actions) = farm()
        .step(
            Farm1Input::from_uslp(&checked, true).unwrap(),
            NOW,
            &mut out,
        )
        .unwrap();
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, Farm1Output::Deliver { .. }))
    );
    assert_eq!(next.state(), Farm1State::Open);
    assert!(matches!(
        Farm1Input::from_uslp(&checked, false).unwrap(),
        Farm1Input::Frame {
            buffer_available: false,
            ..
        }
    ));
}

#[test]
fn an_expedited_frame_is_a_type_bd_frame_with_the_tfdf() {
    // req: COP-031, COP-037, USLP-014, API-016
    let table = channels(1);
    let octets = encoded(
        &table,
        fields(FrameKind::Expedited, 0),
        tfdf(TfdzRule::NoSegmentation, OCTET_STREAM),
        &[9, 8],
    );
    let checked = check(&table, &octets);
    let tfdf_octets: &[u8] = octets.get(7..octets.len() - 2).unwrap();
    assert_eq!(
        Farm1Frame::from_uslp(&checked).unwrap(),
        Farm1Frame::Bd { data: tfdf_octets }
    );
    let mut out = [0_u8; 16];
    let (_, actions) = farm()
        .step(
            Farm1Input::from_uslp(&checked, true).unwrap(),
            NOW,
            &mut out,
        )
        .unwrap();
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, Farm1Output::Deliver { .. }))
    );
}

#[test]
fn a_protocol_control_frame_with_one_command_is_a_type_bc_frame() {
    // req: COP-031, USLP-013, USLP-014, API-016
    let table = channels(1);
    for command in [
        ControlCommand::Unlock,
        ControlCommand::SetVr(FrameSeqNum::new(0x17)),
    ] {
        let octets = bc_frame(cop1(), command.to_octets().as_slice());
        let checked = check(&table, &octets);
        assert_eq!(
            Farm1Frame::from_uslp(&checked).unwrap(),
            Farm1Frame::Bc {
                command: Some(command)
            }
        );
    }
}

#[test]
fn a_type_bc_frame_that_is_not_one_command_is_refused_and_gives_event_e9() {
    // req: COP-031, USLP-013, USLP-014
    let table = channels(1);
    let unlock = ControlCommand::Unlock.to_octets();
    let unlock = unlock.as_slice();
    let two = [unlock, unlock].concat();
    let cases = [
        // A TFDZ that is not a command.
        (cop1(), std::vec![0xFF, 0xFF]),
        // Another UPID, with the octets of a command.
        (
            tfdf(TfdzRule::NoSegmentation, OCTET_STREAM),
            unlock.to_vec(),
        ),
        // Two commands.
        (cop1(), two),
        // The UPID of Control Commands under another rule.
        (
            tfdf(TfdzRule::OctetStream, Upid::Cop1ControlCommands),
            unlock.to_vec(),
        ),
    ];
    for (header, tfdz) in cases {
        let octets = bc_frame(header, &tfdz);
        let checked = check(&table, &octets);
        for err in [
            Farm1Frame::from_uslp(&checked).unwrap_err(),
            Farm1Input::from_uslp(&checked, true).unwrap_err(),
        ] {
            assert_eq!(
                (err.kind(), err.field()),
                (ErrorKind::ReservedValue, Some(field::CONTROL_COMMAND))
            );
        }
        assert_eq!(by_hand(&checked), None);
        // Rejecting the frame feeds FARM-1 the invalid-frame input (E9; OI-100 (a)).
        let mut out = [0_u8; 16];
        let (next, actions) = farm()
            .step(Farm1Input::InvalidFrame, NOW, &mut out)
            .unwrap();
        assert!(actions.iter().any(|a| matches!(
            a,
            Farm1Output::Notify(Farm1Notify::FrameDiscarded {
                event: Farm1Event::E9,
                ..
            })
        )));
        assert!(
            !actions
                .iter()
                .any(|a| matches!(a, Farm1Output::Deliver { .. }))
        );
        assert_eq!(
            next.clcw(),
            farm().clcw(),
            "E9 changes nothing the CLCW reports"
        );
    }
}

#[test]
fn a_foreign_vcid_is_refused_by_farm1() {
    // req: COP-037, API-016
    let table = channels(1);
    let octets = encoded(
        &table,
        fields(FrameKind::Expedited, 0),
        tfdf(TfdzRule::NoSegmentation, OCTET_STREAM),
        &[1],
    );
    let checked = check(&table, &octets);
    let config = Farm1Config {
        vc_id: TcVcId::new(VC + 1).unwrap(),
        status: ClcwStatus::new(0).unwrap(),
        window: Farm1Window::retransmission_allowed(16).unwrap(),
    };
    let other = Farm1::new(config, FrameSeqNum::new(0));
    let input = Farm1Input::from_uslp(&checked, true).unwrap();
    assert!(matches!(input, Farm1Input::Frame { vc_id, .. } if vc_id == TcVcId::new(VC).unwrap()));
    let mut out = [0_u8; 16];
    let err = other.step(input, NOW, &mut out).unwrap_err();
    assert_eq!(
        (err.kind(), err.field()),
        (ErrorKind::UnknownChannel, Some(field::VC_ID))
    );
}

#[test]
fn a_truncated_frame_is_refused() {
    // req: COP-031, USLP-002, USLP-014
    let physical = PhysicalChannelConfig::new(TransferFrameType::Variable, 64, 0, false).unwrap();
    let params = VcParams {
        truncated_frame_len: Some(TruncatedFrameLen::new(12).unwrap()),
        ..VcParams::new(TransferFrameType::Variable)
    };
    let vc = VcConfig::new(scid(), vcid(), params).unwrap();
    let map = MapConfig::new(scid(), vcid(), mapid(), SecurityLayout::NONE);
    let table = VcTable::<1, 1>::new(physical, &[vc], &[map]).unwrap();
    let channel = aldis_uslp::ChannelFields {
        spacecraft_id: scid(),
        source_or_destination: SourceOrDestination::Destination,
        vc_id: vcid(),
        map_id: mapid(),
    };
    let mut out = [0_u8; 16];
    let n = encode_truncated(
        &table,
        channel,
        tfdf(TfdzRule::NoSegmentation, OCTET_STREAM),
        &[1, 2, 3, 4, 5, 6, 7],
        &mut out,
    )
    .unwrap();
    let checked = check(&table, &out[..n]);
    assert!(checked.is_truncated());
    for err in [
        Farm1Frame::from_uslp(&checked).unwrap_err(),
        Farm1Input::from_uslp(&checked, true).unwrap_err(),
    ] {
        assert_eq!(
            (err.kind(), err.field()),
            (
                ErrorKind::InvalidField,
                Some(field::END_OF_FRAME_PRIMARY_HEADER_FLAG)
            )
        );
    }
}

#[test]
fn a_type_ad_frame_whose_vcf_count_is_not_one_octet_is_refused() {
    // req: COP-031, USLP-014
    for len in [0_u8, 2, 3] {
        let table = channels(len);
        let octets = encoded(
            &table,
            fields(FrameKind::SequenceControlled, 0),
            tfdf(TfdzRule::NoSegmentation, OCTET_STREAM),
            &[1],
        );
        let checked = check(&table, &octets);
        let err = Farm1Input::from_uslp(&checked, true).unwrap_err();
        assert_eq!(
            (err.kind(), err.field()),
            (ErrorKind::InvalidField, Some(field::VCF_COUNT_LEN)),
            "length {len}"
        );
    }
}

#[test]
fn the_constructor_equals_the_input_built_by_hand() {
    // req: COP-031, COP-037, USLP-013, USLP-014, API-016
    let table = channels(1);
    let unlock = ControlCommand::Unlock.to_octets();
    let frames = [
        encoded(
            &table,
            fields(FrameKind::SequenceControlled, 255),
            tfdf(TfdzRule::NoSegmentation, OCTET_STREAM),
            &[1, 2, 3],
        ),
        encoded(
            &table,
            fields(FrameKind::Expedited, 0),
            tfdf(TfdzRule::NoSegmentation, OCTET_STREAM),
            &[4],
        ),
        bc_frame(cop1(), unlock.as_slice()),
        bc_frame(cop1(), &[0, 0]),
    ];
    for octets in &frames {
        let checked = check(&table, octets);
        let hand = by_hand(&checked);
        assert_eq!(Farm1Frame::from_uslp(&checked).ok(), hand);
        assert_eq!(
            Farm1Input::from_uslp(&checked, false).ok(),
            hand.map(|frame| Farm1Input::Frame {
                vc_id: TcVcId::new(checked.as_frame().vc_id().get()).unwrap(),
                frame,
                buffer_available: false,
            })
        );
    }
}

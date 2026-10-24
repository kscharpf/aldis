//! USLP-013: the TFDZ of a USLP frame that carries COP-1 control commands (Protocol Control
//! Command Flag 1, Table 4-1) decodes with the TC control-command decoder of `aldis-tc`
//! (TC-005). `aldis-tc` is a dev-dependency only, so the library keeps GEN-002 (M2 plan D8,
//! OI-34); `aldis-cop1` checks it again in M3.
#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test-only harness: a failure must fail loudly (RS-018)"
)]

use aldis_core::DecodeMode;
use aldis_core::ids::FrameSeqNum;
use aldis_core::security::SecurityLayout;
use aldis_tc::ControlCommand;
use aldis_testkit::{hex, vectors_dir};
use aldis_uslp::{
    FrameContent, FrameFields, FrameKind, PhysicalChannelConfig, SecurityScope,
    SourceOrDestination, TfdfHeader, TfdzRule, TransferFrameType, Upid, UslpFrame, UslpMapId,
    UslpSpacecraftId, UslpVcId, VcConfig, VcParams, VcTable, encode,
};

/// A variable-length channel whose VC 4 has SDLS, which protocol control frames do not use
/// (732.1 §6.3.1).
fn channels() -> VcTable<1, 1> {
    let pc = PhysicalChannelConfig::new(TransferFrameType::Variable, 128, 0, true).unwrap();
    let scid = UslpSpacecraftId::new(0x155).unwrap();
    let vcid = UslpVcId::new(4).unwrap();
    let params = VcParams {
        security: SecurityScope::Vc(SecurityLayout::new(6, 4).unwrap()),
        ..VcParams::new(TransferFrameType::Variable)
    };
    let map =
        aldis_uslp::MapConfig::new(scid, vcid, UslpMapId::new(0).unwrap(), SecurityLayout::NONE);
    VcTable::new(pc, &[VcConfig::new(scid, vcid, params).unwrap()], &[map]).unwrap()
}

#[test]
fn cop1_commands_round_trip_through_the_tc_decoder() {
    // req: USLP-013, TC-005
    let t = channels();
    let fields = FrameFields {
        spacecraft_id: UslpSpacecraftId::new(0x155).unwrap(),
        source_or_destination: SourceOrDestination::Destination,
        vc_id: UslpVcId::new(4).unwrap(),
        map_id: UslpMapId::new(0).unwrap(),
        kind: FrameKind::ProtocolControl,
        vcf_count: 0,
    };
    let content = FrameContent {
        tfdf: TfdfHeader {
            rule: TfdzRule::NoSegmentation,
            upid: Upid::Cop1ControlCommands,
        },
        insert_zone: &[],
        ocf: None,
    };
    for command in [
        ControlCommand::Unlock,
        ControlCommand::SetVr(FrameSeqNum::new(0x17)),
    ] {
        let octets = command.to_octets();
        let mut out = [0_u8; 32];
        let n = encode(&t, fields, content, octets.as_slice(), &mut out).unwrap();
        let f = UslpFrame::decode(&out[..n], &t, DecodeMode::Strict).unwrap();
        assert!(f.security_header().is_empty() && f.security_trailer().is_empty());
        assert_eq!(f.full().unwrap().kind(), FrameKind::ProtocolControl);
        assert_eq!(ControlCommand::parse(f.tfdz()), Some(command));
    }
}

#[test]
fn the_cop1_vectors_carry_tc_control_commands() {
    // req: USLP-013, TC-005, VER-021
    let path = vectors_dir().join("uslp/protocol_control_cop1.json");
    let file: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let mut seen = 0;
    for case in file["cases"].as_array().unwrap() {
        let expect = &case["expect"];
        let cop1 = expect["upid"].as_u64() == Some(u64::from(Upid::Cop1ControlCommands.bits()));
        if case["operation"] == "decode" && expect["ok"] == true && cop1 {
            let tfdz = hex::decode(expect["tfdz_hex"].as_str().unwrap()).unwrap();
            assert!(ControlCommand::parse(&tfdz).is_some(), "{}", case["id"]);
            seen += 1;
        }
    }
    assert!(seen > 0);
}

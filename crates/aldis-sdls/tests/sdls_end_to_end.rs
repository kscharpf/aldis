//! End-to-end tests for `aldis-sdls` across TM, TC, AOS, and USLP Transfer Frames (355.0-B-2).
#![expect(
    clippy::indexing_slicing,
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "integration test suite"
)]

use aldis_aos::{
    AosEdition, AosFrame, AosFrameFields, AosPhysicalChannelConfig, AosPlan,
    AosVirtualChannelConfig, DataFieldContent as AosContent, DataUnit,
    PacketOffset as AosPacketOffset, PacketPointer as AosPacketPointer, PduHeader,
    VcFrameCount as AosVcFrameCount, VcTable as AosVcTable,
};
use aldis_core::error::{Error, ErrorKind, Result};
use aldis_core::ids::{FrameSeqNum, SpacecraftId, VcId};
use aldis_core::plain::Plain;
use aldis_core::security::{FrameCtx, NoSecurity, SecuredFields, SecurityHook, SecurityLayout};
use aldis_sdls::{
    AeadProvider, AntiReplayWindow, HeaderFormat, IvLen, KeyId, MAX_AAD_LEN, MacLen, NoOpProvider,
    PlLen, SaConfig, SdlsEngine, SecurityHeaderView, SequenceNumber, ServiceType, SnLen, Spi,
    VerificationStatus, build_aad, encode_header,
};
use aldis_tc::{
    FrameFields as TcFrameFields, FrameType, MapConfig as TcMapConfig,
    PhysicalChannelConfig as TcPc, SegmentHeader, SegmentHeaderUse, SequenceFlags, TcFrame,
    TcMapId, TcSpacecraftId, TcVcId, VcConfig as TcVcConfig, VcTable as TcVcTable, writer::TcPlan,
};
use aldis_testkit::{Outcome, hex, run_codec_file};
use aldis_tm::{
    DataFieldContent as TmContent, DataFieldStatus, McFrameCount, PacketOffset as TmPacketOffset,
    PacketPointer as TmPacketPointer, TmFrame, TmFrameFields, TmMasterChannelConfig,
    TmPhysicalChannelConfig, TmPlan, TmVirtualChannelConfig, VcFrameCount as TmVcFrameCount,
};
use aldis_uslp::{
    FrameContent as UslpFrameContent, FrameFields as UslpFrameFields, FrameKind,
    MapConfig as UslpMapConfig, PhysicalChannelConfig as UslpPc, SecurityScope,
    SourceOrDestination, TfdfHeader, TfdzRule, TransferFrameType, Upid, UslpFrame, UslpMapId,
    UslpSpacecraftId, UslpVcId, VcConfig as UslpVcConfig, VcParams, VcTable as UslpVcTable,
    writer::UslpPlan,
};
use serde_json::json;

/// Deterministic invertible test AEAD provider (NOT cryptographic; tests protocol plumbing,
/// AAD binding, tag verification, and in-place encryption/decryption).
struct XorMacProvider {
    key_id: KeyId,
    secret: u8,
}

impl XorMacProvider {
    const fn new(key_id: KeyId, secret: u8) -> Self {
        Self { key_id, secret }
    }

    fn compute_tag(&self, nonce: &[u8], aad: &[u8], ciphertext: &[u8], tag_out: &mut [u8]) {
        let mut acc = [self.secret; 16];
        for (i, &b) in nonce
            .iter()
            .chain(aad.iter())
            .chain(ciphertext.iter())
            .enumerate()
        {
            acc[i % 16] = acc[i % 16].wrapping_add(b).rotate_left(1) ^ (i as u8);
        }
        for (slot, &b) in tag_out.iter_mut().zip(acc.iter().cycle()) {
            *slot = b;
        }
    }

    fn keystream_xor(&self, nonce: &[u8], data: &mut [u8]) {
        for ((i, byte), &n) in data.iter_mut().enumerate().zip(nonce.iter().cycle()) {
            *byte ^= self.secret ^ n ^ (i as u8);
        }
    }
}

impl AeadProvider for XorMacProvider {
    fn seal_in_place(
        &self,
        key: KeyId,
        nonce: &[u8],
        aad: &[u8],
        data: &mut [u8],
        tag_out: &mut [u8],
    ) -> Result<()> {
        if key != self.key_id || nonce.is_empty() {
            return Err(Error::new(ErrorKind::Security));
        }
        self.keystream_xor(nonce, data);
        self.compute_tag(nonce, aad, data, tag_out);
        Ok(())
    }

    fn open_in_place(
        &self,
        key: KeyId,
        nonce: &[u8],
        aad: &[u8],
        data: &mut [u8],
        tag: &[u8],
    ) -> Result<()> {
        if key != self.key_id || nonce.is_empty() || tag.len() > 64 {
            return Err(Error::new(ErrorKind::Security));
        }
        let mut expected = [0_u8; 64];
        self.compute_tag(nonce, aad, data, &mut expected[..tag.len()]);
        if expected[..tag.len()] != *tag {
            return Err(Error::new(ErrorKind::Security));
        }
        self.keystream_xor(nonce, data);
        Ok(())
    }
}

#[test]
fn tm_sdls_round_trip_and_mcfc_exclusion() {
    // req: API-070, API-071, API-073, API-074
    let scid = SpacecraftId::<10>::new(341).unwrap();
    let vcid = VcId::<3>::new(1).unwrap();
    let ctx = FrameCtx::Tm { scid, vcid };
    let sec = SecurityLayout::new(14, 16).unwrap();

    let pc = TmPhysicalChannelConfig::new(64, true).unwrap();
    let mc = TmMasterChannelConfig::new(pc, scid, None, false)
        .with_vc(TmVirtualChannelConfig::new(vcid, TmContent::Packets).with_security(sec))
        .unwrap();

    let spi = Spi::new(7).unwrap();
    let key = KeyId::new(1);
    let win = AntiReplayWindow::new(4).unwrap();
    let cfg = SaConfig::baseline_gcm(spi, ctx, key, win);

    let mut tx = SdlsEngine::<_, 4>::new(XorMacProvider::new(key, 0x5A));
    let mut rx = SdlsEngine::<_, 4>::new(XorMacProvider::new(key, 0x5A));
    tx.add_sa(cfg).unwrap();
    rx.add_sa(cfg).unwrap();

    let plaintext = b"hello CCSDS TM SDLS!";
    let fields = TmFrameFields::new(
        vcid,
        McFrameCount::new(10),
        TmVcFrameCount::new(20),
        DataFieldStatus::Packets(TmPacketPointer::StartsAt(
            TmPacketOffset::try_from(0).unwrap(),
        )),
    );
    let mut buf = [0_u8; 64];
    let mut w = TmPlan::begin(&mut buf, &mc, &fields).unwrap();
    w.data_mut()[..plaintext.len()].copy_from_slice(plaintext);
    let len = w.protect(&mut tx, &ctx).unwrap().finalize().len();
    assert_eq!(len, 64);

    // Ciphertext must differ from plaintext on the wire.
    let decoded = TmFrame::decode_with(&buf[..len], mc.pc(), &mc).unwrap();
    assert_ne!(&decoded.data_field()[..plaintext.len()], plaintext);
    assert_eq!(decoded.security_header().len(), 14);
    assert_eq!(decoded.security_trailer().len(), 16);
    let sec_fields = SecuredFields::adjacent(
        6,
        decoded.security_header().len(),
        decoded.data_field().len(),
        decoded.security_trailer().len(),
    );

    // NoSecurity must be rejected on a channel with SDLS fields.
    assert!(
        Plain::verify_with(
            &mut NoSecurity,
            &ctx,
            &mut buf[..len],
            &sec_fields,
            true,
            |b| { TmFrame::decode_with(b, mc.pc(), &mc) }
        )
        .is_err()
    );

    // 355.0-B-2 §4.2.2.6.2 d: changing TM MCFC (octet 2) does NOT invalidate AAD!
    let mut copy_mcfc = buf;
    copy_mcfc[2] ^= 0x01;
    let verified_range = rx.verify(&ctx, &mut copy_mcfc[..len], &sec_fields).unwrap();
    assert_eq!(rx.last_status(), VerificationStatus::NoFailure);
    assert_eq!(&copy_mcfc[verified_range][..plaintext.len()], plaintext);

    // Replaying the same sequence number must fail with AntiReplayFailure and scrub plaintext!
    let err = rx.verify(&ctx, &mut buf[..len], &sec_fields).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Security);
    assert_eq!(rx.last_status(), VerificationStatus::AntiReplayFailure);
    assert!(buf[sec_fields.data()].iter().all(|&b| b == 0));

    // Fresh frame #2 through Plain::verify_with produces Plain<TmFrame>.
    let mut buf2 = [0_u8; 64];
    let mut w2 = TmPlan::begin(&mut buf2, &mc, &fields).unwrap();
    w2.data_mut()[..plaintext.len()].copy_from_slice(plaintext);
    let len2 = w2.protect(&mut tx, &ctx).unwrap().finalize().len();
    let plain = Plain::verify_with(&mut rx, &ctx, &mut buf2[..len2], &sec_fields, true, |b| {
        TmFrame::decode_with(b, mc.pc(), &mc)
    })
    .unwrap();
    assert_eq!(&plain.data_field()[..plaintext.len()], plaintext);
    assert_eq!(rx.sequence_number(spi), Some(SequenceNumber::new(2)));
}

#[test]
fn tc_sdls_round_trip_and_aad_tamper() {
    // req: API-070, API-073, API-074
    let spi = Spi::new(42).unwrap();
    let key = KeyId::new(9);
    let win = AntiReplayWindow::new(16).unwrap();
    let sec = SecurityLayout::new(14, 16).unwrap();
    let payload = b"CCSDS-SDLS-DATA";

    // TC with Segment Header (per-MAP SDLS)
    let scid = TcSpacecraftId::new(341).unwrap();
    let vc_id = TcVcId::new(0).unwrap();
    let map_id = TcMapId::new(1).unwrap();
    let tc_ctx = FrameCtx::Tc {
        scid,
        vcid: vc_id,
        map_id: Some(map_id),
    };
    let tc_ch = TcVcTable::<1, 1>::new(
        TcPc::new(128, true).unwrap(),
        &[TcVcConfig::new(scid, vc_id, 128, SegmentHeaderUse::Present).unwrap()],
        &[TcMapConfig::new(scid, vc_id, map_id, sec)],
    )
    .unwrap();

    let mut tc_tx = SdlsEngine::<_, 2>::new(XorMacProvider::new(key, 0x33));
    let mut tc_rx = SdlsEngine::<_, 2>::new(XorMacProvider::new(key, 0x33));
    let tc_sa = SaConfig::baseline_gcm(spi, tc_ctx, key, win);
    tc_tx.add_sa(tc_sa).unwrap();
    tc_rx.add_sa(tc_sa).unwrap();

    let mut tc_buf = [0_u8; 128];
    let tc_fields = TcFrameFields {
        frame_type: FrameType::Ad,
        spacecraft_id: scid,
        vc_id,
        sequence_number: FrameSeqNum::new(0),
    };
    let seg = SegmentHeader {
        sequence_flags: SequenceFlags::Unsegmented,
        map_id,
    };
    let mut w = TcPlan::begin(&mut tc_buf, &tc_ch, tc_fields, Some(seg), payload.len()).unwrap();
    w.data_mut()[..payload.len()].copy_from_slice(payload);
    let tc_len = w
        .protect(payload.len(), &mut tc_tx, &tc_ctx)
        .unwrap()
        .finalize()
        .len();
    let tc_dec = TcFrame::decode(&tc_buf[..tc_len], &tc_ch).unwrap();
    let tc_sec_fields = SecuredFields::adjacent(
        6,
        tc_dec.security_header().len(),
        tc_dec.data_field().len(),
        tc_dec.security_trailer().len(),
    );

    // Tampering TC Segment Header (octet 5) must fail MAC verification (§4.2.2.6.2 f).
    let mut tampered = tc_buf;
    tampered[5] ^= 0x01;
    assert_eq!(
        tc_rx
            .verify(&tc_ctx, &mut tampered[..tc_len], &tc_sec_fields)
            .unwrap_err()
            .kind(),
        ErrorKind::Security
    );
    assert_eq!(tc_rx.last_status(), VerificationStatus::MacFailure);
    // Sequence number must NOT advance on MAC failure (§4.2.4.4 j).
    assert_eq!(tc_rx.sequence_number(spi), Some(SequenceNumber::ZERO));

    let tc_plain = Plain::verify_with(
        &mut tc_rx,
        &tc_ctx,
        &mut tc_buf[..tc_len],
        &tc_sec_fields,
        true,
        |b| TcFrame::decode(b, &tc_ch),
    )
    .unwrap();
    assert_eq!(tc_plain.data_field(), payload);
}

#[test]
fn aos_sdls_round_trip() {
    // req: API-070, API-073, API-074
    let spi = Spi::new(42).unwrap();
    let key = KeyId::new(9);
    let win = AntiReplayWindow::new(16).unwrap();
    let sec = SecurityLayout::new(14, 16).unwrap();
    let payload = b"CCSDS-SDLS-DATA";

    let scid = SpacecraftId::<10>::new(341).unwrap();
    let vc_id = VcId::<6>::new(3).unwrap();
    let aos_ctx = FrameCtx::Aos { scid, vcid: vc_id };
    let phys = AosPhysicalChannelConfig::new(64, true, 0, true, AosEdition::Issue5).unwrap();
    let aos_vc = AosVirtualChannelConfig::new(scid, vc_id, AosContent::Mpdu)
        .unwrap()
        .with_security(sec);
    let aos_ch = AosVcTable::<2>::new(phys, &[aos_vc]).unwrap();

    let mut aos_tx = SdlsEngine::<_, 2>::new(XorMacProvider::new(key, 0x77));
    let mut aos_rx = SdlsEngine::<_, 2>::new(XorMacProvider::new(key, 0x77));
    let aos_sa = SaConfig::baseline_gcm(spi, aos_ctx, key, win);
    aos_tx.add_sa(aos_sa).unwrap();
    aos_rx.add_sa(aos_sa).unwrap();

    let mut aos_buf = [0_u8; 64];
    let aos_fields = AosFrameFields::new(
        scid,
        vc_id,
        AosVcFrameCount::try_from(1).unwrap(),
        PduHeader::Mpdu(AosPacketPointer::StartsAt(
            AosPacketOffset::try_from(0).unwrap(),
        )),
    );
    let mut aw = AosPlan::begin(&mut aos_buf, &phys, &aos_ch, &aos_fields).unwrap();
    aw.data_mut()[..payload.len()].copy_from_slice(payload);
    let aos_len = aw.protect(&mut aos_tx, &aos_ctx).unwrap().finalize().len();
    // In AOS, the M_PDU Header is inside the encrypted Data Field, so verify_with decrypts
    // in place before AosFrame::decode parses the M_PDU Header.
    let aos_sec_fields = SecuredFields::adjacent(8, 14, aos_len - 8 - 14 - 16 - 2, 16);
    let aos_plain = Plain::verify_with(
        &mut aos_rx,
        &aos_ctx,
        &mut aos_buf[..aos_len],
        &aos_sec_fields,
        true,
        |b| AosFrame::decode(b, &phys, &aos_ch),
    )
    .unwrap();
    assert!(
        matches!(aos_plain.data_unit(), DataUnit::Mpdu(m) if &m.packet_zone()[..payload.len()] == payload)
    );
}

#[test]
fn uslp_sdls_round_trip() {
    // req: API-070, API-073, API-074
    let spi = Spi::new(42).unwrap();
    let key = KeyId::new(9);
    let win = AntiReplayWindow::new(16).unwrap();
    let sec = SecurityLayout::new(14, 16).unwrap();
    let payload = b"CCSDS-SDLS-DATA";

    let scid = UslpSpacecraftId::new(341).unwrap();
    let vc_id = UslpVcId::new(2).unwrap();
    let map_id = UslpMapId::new(5).unwrap();
    let uslp_ctx = FrameCtx::Uslp {
        scid,
        vcid: vc_id,
        map_id: Some(map_id),
    };
    let phys = UslpPc::new(TransferFrameType::Variable, 128, 0, true).unwrap();
    let mut vc_params = VcParams::new(TransferFrameType::Variable);
    vc_params.security = SecurityScope::PerMap;
    let uslp_ch = UslpVcTable::<2, 1>::new(
        phys,
        &[UslpVcConfig::new(scid, vc_id, vc_params).unwrap()],
        &[UslpMapConfig::new(scid, vc_id, map_id, sec)],
    )
    .unwrap();

    let mut uslp_tx = SdlsEngine::<_, 2>::new(XorMacProvider::new(key, 0x99));
    let mut uslp_rx = SdlsEngine::<_, 2>::new(XorMacProvider::new(key, 0x99));
    let uslp_sa = SaConfig::baseline_gcm(spi, uslp_ctx, key, win);
    uslp_tx.add_sa(uslp_sa).unwrap();
    uslp_rx.add_sa(uslp_sa).unwrap();

    let mut uslp_buf = [0_u8; 128];
    let uslp_fields = UslpFrameFields {
        spacecraft_id: scid,
        source_or_destination: SourceOrDestination::Destination,
        vc_id,
        map_id,
        kind: FrameKind::Expedited,
        vcf_count: 0,
    };
    let content = UslpFrameContent {
        tfdf: TfdfHeader {
            rule: TfdzRule::OctetStream,
            upid: Upid::OctetStream,
        },
        insert_zone: &[],
        ocf: None,
    };
    let mut uw =
        UslpPlan::begin(&mut uslp_buf, &uslp_ch, uslp_fields, content, payload.len()).unwrap();
    uw.data_mut()[..payload.len()].copy_from_slice(payload);
    let uslp_len = uw
        .protect(payload.len(), &mut uslp_tx, &uslp_ctx)
        .unwrap()
        .finalize()
        .len();
    // In USLP, the TFDF Header is inside the encrypted Data Field; Primary Header is 7 octets
    // (vcf_count_len = 0), Insert Zone = 0, OCF = 0, FECF = 2.
    let uslp_sec_fields = SecuredFields::adjacent(7, 14, uslp_len - 7 - 14 - 16 - 2, 16);
    let uslp_plain = Plain::verify_with(
        &mut uslp_rx,
        &uslp_ctx,
        &mut uslp_buf[..uslp_len],
        &uslp_sec_fields,
        true,
        |b| UslpFrame::decode(b, &uslp_ch),
    )
    .unwrap();
    assert_eq!(uslp_plain.tfdz(), payload);
}

#[test]
#[expect(clippy::too_many_lines, reason = "vector runner and edge-case matrix")]
fn sdls_header_vectors_and_edge_cases() {
    // req: API-070, API-071
    run_codec_file("sdls/header.json", |case| match case.operation.as_str() {
        "decode_header" => {
            let cfg = case.config.as_ref().unwrap();
            let iv_len =
                IvLen::new(u8::try_from(cfg["iv_len"].as_u64().unwrap()).unwrap()).unwrap();
            let sn_len =
                SnLen::new(u8::try_from(cfg["sn_len"].as_u64().unwrap()).unwrap()).unwrap();
            let pl_len =
                PlLen::new(u8::try_from(cfg["pl_len"].as_u64().unwrap()).unwrap()).unwrap();
            let fmt = HeaderFormat::new(iv_len, sn_len, pl_len);
            let input = hex::decode(case.input_hex.as_deref().unwrap()).unwrap();
            match SecurityHeaderView::decode(&input, fmt) {
                Ok(v) => Outcome::Ok(
                    json!({
                        "spi": v.spi().get(),
                        "iv_hex": hex::encode(v.iv()),
                        "sn_hex": hex::encode(v.sn()),
                        "pad_len_hex": hex::encode(v.pad_len()),
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
                Err(e) => Outcome::Err {
                    kind: format!("{:?}", e.kind()),
                    field: e.field().map(aldis_core::error::FieldId::code),
                },
            }
        }
        _ => Outcome::Unsupported,
    })
    .assert_passed();

    // Sequence number rollover must fail instead of wrapping to 0 (prevents GCM nonce reuse).
    let max_2 = SequenceNumber::new(0xFFFF);
    assert_eq!(
        max_2.checked_next(2).unwrap_err().kind(),
        ErrorKind::Security
    );
    assert_eq!(SequenceNumber::new(0xFFFE).checked_next(2), Ok(max_2));

    // Anti-replay window boundary checks.
    let win = AntiReplayWindow::new(4).unwrap();
    let sn = SequenceNumber::new(10);
    assert!(!sn.accepts(SequenceNumber::new(9), win));
    assert!(!sn.accepts(SequenceNumber::new(10), win));
    assert!(sn.accepts(SequenceNumber::new(11), win));
    assert!(sn.accepts(SequenceNumber::new(14), win));
    assert!(!sn.accepts(SequenceNumber::new(15), win));

    // Header encode/decode round-trip with IV + SN + Pad Length.
    let fmt = HeaderFormat::new(
        IvLen::new(4).unwrap(),
        SnLen::new(4).unwrap(),
        PlLen::new(1).unwrap(),
    );
    let spi = Spi::new(0x0102).unwrap();
    let mut out = [0_u8; 11];
    encode_header(
        &mut out,
        fmt,
        spi,
        &[0xAA, 0xBB, 0xCC, 0xDD],
        &[0, 0, 0, 7],
        &[3],
    )
    .unwrap();
    let view = SecurityHeaderView::decode(&out, fmt).unwrap();
    assert_eq!(view.spi(), spi);
    assert_eq!(view.iv(), &[0xAA, 0xBB, 0xCC, 0xDD]);
    assert_eq!(view.sn(), &[0, 0, 0, 7]);
    assert_eq!(view.pad_len(), &[3]);

    // ClearMode SA + NoOpProvider + duplicate SA rejection + capacity exceeded.
    let ctx = FrameCtx::Tm {
        scid: SpacecraftId::<10>::new(1).unwrap(),
        vcid: VcId::<3>::new(0).unwrap(),
    };
    let mut cfg = SaConfig::baseline_gcm(spi, ctx, KeyId::new(0), win);
    cfg.service = ServiceType::ClearMode;
    let mut eng: SdlsEngine<NoOpProvider, 1> = SdlsEngine::new(NoOpProvider);
    eng.add_sa(cfg).unwrap();
    assert_eq!(
        eng.add_sa(cfg).unwrap_err().kind(),
        ErrorKind::InvalidConfig
    );
    let ctx2 = FrameCtx::Tm {
        scid: SpacecraftId::<10>::new(1).unwrap(),
        vcid: VcId::<3>::new(1).unwrap(),
    };
    let mut cfg2 = SaConfig::baseline_gcm(Spi::new(2).unwrap(), ctx2, KeyId::new(0), win);
    cfg2.service = ServiceType::ClearMode;
    assert_eq!(
        eng.add_sa(cfg2).unwrap_err().kind(),
        ErrorKind::CapacityExceeded
    );

    // Invalid SA configs.
    let mut bad = SaConfig::baseline_gcm(spi, ctx, KeyId::new(1), win);
    bad.mac_len = MacLen::NONE;
    assert_eq!(bad.validate().unwrap_err().kind(), ErrorKind::InvalidConfig);

    // AAD buffer check.
    let mut aad = [0_u8; MAX_AAD_LEN];
    let frame = [0_u8; 40];
    let fields = SecuredFields::adjacent(6, 14, 4, 16);
    assert_eq!(
        build_aad(
            &ctx,
            &frame,
            &fields,
            HeaderFormat::BASELINE_GCM.iv_range(),
            &mut aad
        ),
        Ok(20)
    );
}

#[test]
#[expect(clippy::too_many_lines, reason = "exhaustive error and branch matrix")]
fn sdls_exhaustive_error_and_branch_coverage() {
    // req: API-070, API-071, API-073, API-074
    use aldis_sdls::header::decode_spi;

    // KeyId, IvLen, SnLen, PlLen, MacLen, AntiReplayWindow bounds
    assert_eq!(KeyId::new(7).get(), 7);
    assert_eq!(IvLen::new(33).unwrap_err().kind(), ErrorKind::InvalidConfig);
    assert_eq!(SnLen::new(1).unwrap_err().kind(), ErrorKind::InvalidConfig);
    assert_eq!(SnLen::new(9).unwrap_err().kind(), ErrorKind::InvalidConfig);
    assert_eq!(PlLen::new(3).unwrap_err().kind(), ErrorKind::InvalidConfig);
    assert_eq!(MacLen::new(1).unwrap_err().kind(), ErrorKind::InvalidConfig);
    assert_eq!(
        MacLen::new(65).unwrap_err().kind(),
        ErrorKind::InvalidConfig
    );
    assert_eq!(MacLen::new(0).unwrap().get(), 0);
    assert_eq!(MacLen::new(8).unwrap().get(), 8);
    assert_eq!(
        AntiReplayWindow::new(0).unwrap_err().kind(),
        ErrorKind::InvalidConfig
    );

    // SequenceNumber 1..=16 octets + errors
    assert_eq!(SequenceNumber::new(42).get(), 42);
    for octets in 1..=16_usize {
        let mut buf = [0_u8; 16];
        let slice = &mut buf[..octets];
        let sn = SequenceNumber::new(1);
        sn.encode_be(slice).unwrap();
        assert_eq!(SequenceNumber::decode_be(slice).unwrap(), sn);
        assert_eq!(sn.checked_next(octets).unwrap(), SequenceNumber::new(2));
    }
    assert_eq!(
        SequenceNumber::ZERO.checked_next(0).unwrap_err().kind(),
        ErrorKind::Security
    );
    assert_eq!(
        SequenceNumber::ZERO.checked_next(17).unwrap_err().kind(),
        ErrorKind::Security
    );
    assert_eq!(
        SequenceNumber::new(u128::MAX)
            .checked_next(16)
            .unwrap_err()
            .kind(),
        ErrorKind::Security
    );
    let mut small = [0_u8; 1];
    assert_eq!(
        SequenceNumber::new(0x100)
            .encode_be(&mut small)
            .unwrap_err()
            .kind(),
        ErrorKind::Security
    );
    assert_eq!(
        SequenceNumber::ZERO.encode_be(&mut []).unwrap_err().kind(),
        ErrorKind::Security
    );
    assert_eq!(
        SequenceNumber::decode_be(&[]).unwrap_err().kind(),
        ErrorKind::Security
    );
    assert_eq!(
        SequenceNumber::decode_be(&[0_u8; 17]).unwrap_err().kind(),
        ErrorKind::Security
    );

    // HeaderFormat + decode_spi / encode_header errors
    assert_eq!(
        decode_spi(&[0]).unwrap_err().kind(),
        ErrorKind::LengthMismatch
    );
    let fmt = HeaderFormat::new(
        IvLen::new(4).unwrap(),
        SnLen::new(4).unwrap(),
        PlLen::new(1).unwrap(),
    );
    let spi = Spi::new(5).unwrap();
    let mut sh = [0_u8; 11];
    assert_eq!(
        encode_header(&mut sh[..10], fmt, spi, &[0; 4], &[0; 4], &[0])
            .unwrap_err()
            .kind(),
        ErrorKind::LengthMismatch
    );
    assert_eq!(
        encode_header(&mut sh, fmt, spi, &[0; 3], &[0; 4], &[0])
            .unwrap_err()
            .kind(),
        ErrorKind::LengthMismatch
    );
    assert_eq!(
        encode_header(&mut sh, fmt, spi, &[0; 4], &[0; 3], &[0])
            .unwrap_err()
            .kind(),
        ErrorKind::LengthMismatch
    );
    assert_eq!(
        encode_header(&mut sh, fmt, spi, &[0; 4], &[0; 4], &[])
            .unwrap_err()
            .kind(),
        ErrorKind::LengthMismatch
    );

    // NoOpProvider direct seal_in_place / open_in_place
    let mut data = [1_u8, 2, 3];
    let mut tag = [0xFF_u8; 16];
    NoOpProvider
        .seal_in_place(KeyId::new(1), &[], &[], &mut data, &mut tag)
        .unwrap();
    assert_eq!(tag, [0_u8; 16]);
    NoOpProvider
        .open_in_place(KeyId::new(1), &[], &[], &mut data, &tag)
        .unwrap();

    // AAD error paths: too large, short frame, short headers for all 4 protocols, bad iv_range
    let mut aad = [0_u8; MAX_AAD_LEN];
    let tm_ctx = FrameCtx::Tm {
        scid: SpacecraftId::<10>::new(1).unwrap(),
        vcid: VcId::<3>::new(0).unwrap(),
    };
    let tc_ctx = FrameCtx::Tc {
        scid: TcSpacecraftId::new(1).unwrap(),
        vcid: TcVcId::new(0).unwrap(),
        map_id: None,
    };
    let aos_ctx = FrameCtx::Aos {
        scid: SpacecraftId::<10>::new(1).unwrap(),
        vcid: VcId::<6>::new(0).unwrap(),
    };
    let uslp_ctx = FrameCtx::Uslp {
        scid: UslpSpacecraftId::new(1).unwrap(),
        vcid: UslpVcId::new(0).unwrap(),
        map_id: None,
    };
    let huge = SecuredFields::adjacent(250, 14, 4, 16);
    assert_eq!(
        build_aad(&tm_ctx, &[0; 300], &huge, 2..14, &mut aad)
            .unwrap_err()
            .kind(),
        ErrorKind::BufferTooSmall
    );
    let ok_f = SecuredFields::adjacent(6, 14, 4, 16);
    assert_eq!(
        build_aad(&tm_ctx, &[0; 10], &ok_f, 2..14, &mut aad)
            .unwrap_err()
            .kind(),
        ErrorKind::InvalidField
    );
    assert_eq!(
        build_aad(
            &tm_ctx,
            &[0; 40],
            &SecuredFields::adjacent(5, 14, 4, 16),
            2..14,
            &mut aad
        )
        .unwrap_err()
        .kind(),
        ErrorKind::InvalidField
    );
    assert_eq!(
        build_aad(
            &tc_ctx,
            &[0; 40],
            &SecuredFields::adjacent(4, 14, 4, 16),
            2..14,
            &mut aad
        )
        .unwrap_err()
        .kind(),
        ErrorKind::InvalidField
    );
    // TC without Segment Header (hdr_start == 5)
    assert_eq!(
        build_aad(
            &tc_ctx,
            &[0; 40],
            &SecuredFields::adjacent(5, 14, 4, 16),
            2..14,
            &mut aad
        ),
        Ok(19)
    );
    assert_eq!(
        build_aad(
            &aos_ctx,
            &[0; 40],
            &SecuredFields::adjacent(5, 14, 4, 16),
            2..14,
            &mut aad
        )
        .unwrap_err()
        .kind(),
        ErrorKind::InvalidField
    );
    assert_eq!(
        build_aad(
            &uslp_ctx,
            &[0; 40],
            &SecuredFields::adjacent(3, 14, 4, 16),
            2..14,
            &mut aad
        )
        .unwrap_err()
        .kind(),
        ErrorKind::InvalidField
    );
    assert_eq!(
        build_aad(&tm_ctx, &[0; 40], &ok_f, 2..20, &mut aad)
            .unwrap_err()
            .kind(),
        ErrorKind::InvalidField
    );

    // SaConfig::validate error cases
    let win = AntiReplayWindow::new(4).unwrap();
    let mut cfg_no_iv = SaConfig::baseline_gcm(spi, tm_ctx, KeyId::new(1), win);
    cfg_no_iv.header = HeaderFormat::new(IvLen::NONE, SnLen::new(4).unwrap(), PlLen::NONE);
    assert_eq!(
        cfg_no_iv.validate().unwrap_err().kind(),
        ErrorKind::InvalidConfig
    );

    let mut cfg_pl = SaConfig::baseline_gcm(spi, tm_ctx, KeyId::new(1), win);
    cfg_pl.header = HeaderFormat::new(IvLen::BASELINE_GCM, SnLen::NONE, PlLen::new(1).unwrap());
    assert_eq!(
        cfg_pl.validate().unwrap_err().kind(),
        ErrorKind::InvalidConfig
    );

    let mut cfg_big_iv = SaConfig::baseline_gcm(spi, tm_ctx, KeyId::new(1), win);
    cfg_big_iv.header = HeaderFormat::new(IvLen::new(17).unwrap(), SnLen::NONE, PlLen::NONE);
    assert_eq!(
        cfg_big_iv.validate().unwrap_err().kind(),
        ErrorKind::InvalidConfig
    );

    // AuthenticatedEncryption with separate SN and >16-byte IV is valid.
    let mut cfg_sn_iv = SaConfig::baseline_gcm(spi, tm_ctx, KeyId::new(1), win);
    cfg_sn_iv.header =
        HeaderFormat::new(IvLen::new(17).unwrap(), SnLen::new(4).unwrap(), PlLen::NONE);
    assert!(cfg_sn_iv.validate().is_ok());

    // SdlsEngine edge cases: unknown ctx, unknown SPI, wrong ctx, layout mismatch, ClearMode
    // with separate SN and with neither IV nor SN, provider(), sequence_number(unknown)
    let mut eng: SdlsEngine<NoOpProvider, 3> = SdlsEngine::new(NoOpProvider);
    let _ = eng.provider();
    assert_eq!(eng.sequence_number(spi), None);
    let mut f = [0_u8; 40];
    assert_eq!(
        eng.protect(&tm_ctx, &mut f, &ok_f).unwrap_err().kind(),
        ErrorKind::Security
    );
    assert_eq!(
        eng.verify(&tm_ctx, &mut f, &ok_f).unwrap_err().kind(),
        ErrorKind::Security
    );
    assert_eq!(eng.last_status(), VerificationStatus::InvalidSpi);

    // Short buffer for header/trailer in verify & protect
    let mut cm_sn = SaConfig {
        spi,
        ctx: tm_ctx,
        service: ServiceType::ClearMode,
        key: KeyId::new(1),
        header: HeaderFormat::new(IvLen::NONE, SnLen::new(4).unwrap(), PlLen::NONE),
        mac_len: MacLen::NONE,
        window: win,
        initial_sn: SequenceNumber::ZERO,
    };
    eng.add_sa(cm_sn).unwrap();
    // Duplicate ctx with different SPI is rejected.
    let mut dup_ctx = cm_sn;
    dup_ctx.spi = Spi::new(99).unwrap();
    assert_eq!(
        eng.add_sa(dup_ctx).unwrap_err().kind(),
        ErrorKind::InvalidConfig
    );
    let sn_fields = SecuredFields::adjacent(6, 6, 4, 0);
    // Layout mismatch on protect (header mismatch AND trailer mismatch)
    assert_eq!(
        eng.protect(&tm_ctx, &mut f, &ok_f).unwrap_err().kind(),
        ErrorKind::InvalidField
    );
    assert_eq!(
        eng.protect(&tm_ctx, &mut f, &SecuredFields::adjacent(6, 6, 4, 16))
            .unwrap_err()
            .kind(),
        ErrorKind::InvalidField
    );
    // Buffer too short for header/trailer on protect
    assert_eq!(
        eng.protect(&tm_ctx, &mut f[..8], &sn_fields)
            .unwrap_err()
            .kind(),
        ErrorKind::InvalidField
    );
    assert_eq!(
        eng.protect(&tm_ctx, &mut f[..14], &sn_fields)
            .unwrap_err()
            .kind(),
        ErrorKind::InvalidField
    );
    // Valid ClearMode with separate SN protect + verify
    eng.protect(&tm_ctx, &mut f, &sn_fields).unwrap();
    assert_eq!(eng.sequence_number(spi), Some(SequenceNumber::new(1)));
    let mut rx_eng: SdlsEngine<NoOpProvider, 3> = SdlsEngine::new(NoOpProvider);
    rx_eng.add_sa(cm_sn).unwrap();
    // Header out of bounds on verify
    assert_eq!(
        rx_eng
            .verify(&tm_ctx, &mut f[..8], &sn_fields)
            .unwrap_err()
            .kind(),
        ErrorKind::Security
    );
    // Unknown SPI on verify
    let mut f_bad_spi = f;
    f_bad_spi[6] = 0;
    f_bad_spi[7] = 99;
    assert_eq!(
        rx_eng
            .verify(&tm_ctx, &mut f_bad_spi, &sn_fields)
            .unwrap_err()
            .kind(),
        ErrorKind::Security
    );
    assert_eq!(rx_eng.last_status(), VerificationStatus::InvalidSpi);
    // Wrong ctx for SPI on verify
    assert_eq!(
        rx_eng
            .verify(&tc_ctx, &mut f, &sn_fields)
            .unwrap_err()
            .kind(),
        ErrorKind::Security
    );
    assert_eq!(rx_eng.last_status(), VerificationStatus::InvalidSpi);
    // Wrong layout on verify (header mismatch AND trailer mismatch)
    assert_eq!(
        rx_eng
            .verify(&tm_ctx, &mut f, &SecuredFields::adjacent(6, 14, 4, 0))
            .unwrap_err()
            .kind(),
        ErrorKind::InvalidField
    );
    assert_eq!(
        rx_eng
            .verify(&tm_ctx, &mut f, &SecuredFields::adjacent(6, 6, 4, 16))
            .unwrap_err()
            .kind(),
        ErrorKind::InvalidField
    );
    // Buffer too short for trailer on verify
    assert_eq!(
        rx_eng
            .verify(&tm_ctx, &mut f[..14], &sn_fields)
            .unwrap_err()
            .kind(),
        ErrorKind::InvalidField
    );
    // Valid verify
    assert_eq!(rx_eng.verify(&tm_ctx, &mut f, &sn_fields), Ok(12..16));

    // ClearMode with neither IV nor SN (0-byte counter)
    cm_sn.spi = Spi::new(6).unwrap();
    cm_sn.ctx = aos_ctx;
    cm_sn.header = HeaderFormat::new(IvLen::NONE, SnLen::NONE, PlLen::NONE);
    cm_sn.mac_len = MacLen::BASELINE_GCM;
    eng.add_sa(cm_sn).unwrap();
    rx_eng.add_sa(cm_sn).unwrap();
    assert_eq!(
        eng.sequence_number(Spi::new(6).unwrap()),
        Some(SequenceNumber::ZERO)
    );
    assert_eq!(eng.sequence_number(Spi::new(77).unwrap()), None);
    let bare_fields = SecuredFields::adjacent(6, 2, 4, 16);
    let mut f2 = [0_u8; 32];
    eng.protect(&aos_ctx, &mut f2, &bare_fields).unwrap();
    // Buffer too short for trailer on protect & verify
    assert_eq!(
        eng.protect(&aos_ctx, &mut f2[..20], &bare_fields)
            .unwrap_err()
            .kind(),
        ErrorKind::InvalidField
    );
    assert_eq!(
        rx_eng
            .verify(&aos_ctx, &mut f2[..20], &bare_fields)
            .unwrap_err()
            .kind(),
        ErrorKind::InvalidField
    );
    assert_eq!(rx_eng.verify(&aos_ctx, &mut f2, &bare_fields), Ok(8..12));

    // Verify with 2nd SA in rx_eng covers the non-matching entry branch on sn update,
    // plus protect rollover failure and provider seal failure.
    let mut rx2 = SdlsEngine::<_, 2>::new(XorMacProvider::new(KeyId::new(9), 0x11));
    let mut tx2 = SdlsEngine::<_, 2>::new(XorMacProvider::new(KeyId::new(9), 0x11));
    let mut sa_a = SaConfig::baseline_gcm(Spi::new(10).unwrap(), tm_ctx, KeyId::new(9), win);
    sa_a.initial_sn = SequenceNumber::new(0xFFFF_FFFF_FFFF_FFFF_FFFF_FFFF);
    tx2.add_sa(sa_a).unwrap();
    // Rollover during protect fails with Security.
    let mut f3 = [0_u8; 40];
    assert_eq!(
        tx2.protect(&tm_ctx, &mut f3, &ok_f).unwrap_err().kind(),
        ErrorKind::Security
    );
    // Provider key mismatch during protect fails with Security.
    let sa_bad_key = SaConfig::baseline_gcm(Spi::new(11).unwrap(), aos_ctx, KeyId::new(999), win);
    tx2.add_sa(sa_bad_key).unwrap();
    assert_eq!(
        tx2.protect(&aos_ctx, &mut f3, &ok_f).unwrap_err().kind(),
        ErrorKind::Security
    );

    // 2 SAs in rx2: verify frame for 2nd SA so the sn-update loop skips the 1st SA.
    let sa1 = SaConfig::baseline_gcm(Spi::new(20).unwrap(), tm_ctx, KeyId::new(9), win);
    let sa2 = SaConfig::baseline_gcm(Spi::new(21).unwrap(), aos_ctx, KeyId::new(9), win);
    let mut tx3 = SdlsEngine::<_, 2>::new(XorMacProvider::new(KeyId::new(9), 0x11));
    tx3.add_sa(sa1).unwrap();
    tx3.add_sa(sa2).unwrap();
    rx2.add_sa(sa1).unwrap();
    rx2.add_sa(sa2).unwrap();
    tx3.protect(&aos_ctx, &mut f3, &ok_f).unwrap();
    assert_eq!(rx2.verify(&aos_ctx, &mut f3, &ok_f), Ok(20..24));
}

//! End-to-end AES-256-GCM SDLS tests across TM, TC, AOS, and USLP Transfer Frames
//! (CCSDS 352.0-B-2, 355.0-B-2 Annex E; OI-84, OI-85).
#![cfg(feature = "aes-gcm")]

use aldis_aos::{
    AosEdition, AosFrame, AosFrameFields, AosPhysicalChannelConfig, AosPlan,
    AosVirtualChannelConfig, DataFieldContent as AosContent, DataUnit,
    PacketOffset as AosPacketOffset, PacketPointer as AosPacketPointer, PduHeader,
    VcFrameCount as AosVcFrameCount, VcTable as AosVcTable,
};
use aldis_core::error::ErrorKind;
use aldis_core::ids::{FrameSeqNum, SpacecraftId, VcId};
use aldis_core::plain::Plain;
use aldis_core::security::{FrameCtx, SecuredFields, SecurityHook, SecurityLayout};
use aldis_sdls::{
    AeadProvider, AntiReplayWindow, HeaderFormat, IvLen, KeyId, MacLen, PlLen, SaConfig,
    SdlsEngine, SequenceNumber, ServiceType, SnLen, Spi, VerificationStatus,
};
use aldis_sdls_crypto::{
    AES_256_KEY_LEN, AES_GCM_NONCE_LEN, AES_GCM_TAG_LEN, Aes256GcmStore, Aes256Key,
};
use aldis_tc::{
    FrameFields as TcFrameFields, FrameType, MapConfig as TcMapConfig,
    PhysicalChannelConfig as TcPc, SegmentHeader, SegmentHeaderUse, SequenceFlags, TcFrame,
    TcMapId, TcSpacecraftId, TcVcId, VcConfig as TcVcConfig, VcTable as TcVcTable, writer::TcPlan,
};
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

const TEST_KEY: [u8; AES_256_KEY_LEN] = [
    0xfe, 0xff, 0xe9, 0x92, 0x86, 0x65, 0x73, 0x1c, 0x6d, 0x6a, 0x8f, 0x94, 0x67, 0x30, 0x83, 0x08,
    0xfe, 0xff, 0xe9, 0x92, 0x86, 0x65, 0x73, 0x1c, 0x6d, 0x6a, 0x8f, 0x94, 0x67, 0x30, 0x83, 0x08,
];

#[expect(clippy::unwrap_used, reason = "test helper")]
fn make_store(key_id: KeyId, key_bytes: [u8; AES_256_KEY_LEN]) -> Aes256GcmStore<4> {
    let mut store = Aes256GcmStore::<4>::new();
    store.insert(key_id, Aes256Key::new(key_bytes)).unwrap();
    store
}

#[test]
fn key_store_management_and_redaction() {
    // req: GEN-004, GEN-010, API-070
    let mut store = Aes256GcmStore::<2>::default();
    let k1 = KeyId::new(1);
    let k2 = KeyId::new(2);
    let k3 = KeyId::new(3);
    let key = Aes256Key::new(TEST_KEY);
    // Debug must redact key material.
    assert_eq!(format!("{key:?}"), "Aes256Key([REDACTED])");
    assert!(format!("{store:?}").contains("Aes256GcmStore"));

    assert!(!store.contains(k1));
    store.insert(k1, key).unwrap();
    assert!(store.contains(k1));
    // Duplicate KeyId rejected.
    assert_eq!(
        store
            .insert(k1, Aes256Key::new(TEST_KEY))
            .unwrap_err()
            .kind(),
        ErrorKind::InvalidConfig
    );
    store
        .insert(k2, Aes256Key::new([0x42; AES_256_KEY_LEN]))
        .unwrap();
    // Capacity exceeded rejected.
    assert_eq!(
        store
            .insert(k3, Aes256Key::new(TEST_KEY))
            .unwrap_err()
            .kind(),
        ErrorKind::CapacityExceeded
    );
    assert!(store.remove(k1));
    assert!(!store.contains(k1));
    assert!(!store.remove(k1));
    // Freed slot can be reused.
    store.insert(k3, Aes256Key::new(TEST_KEY)).unwrap();
    assert!(store.contains(k3));

    // open_in_place error checks: unknown key, bad nonce len, bad tag len, bad tag.
    let mut data = *b"hello";
    assert_eq!(
        store
            .open_in_place(
                k1,
                &[0; AES_GCM_NONCE_LEN],
                &[],
                &mut data,
                &[0; AES_GCM_TAG_LEN]
            )
            .unwrap_err()
            .kind(),
        ErrorKind::Security
    );
    assert_eq!(
        store
            .open_in_place(k2, &[0; 8], &[], &mut data, &[0; AES_GCM_TAG_LEN])
            .unwrap_err()
            .kind(),
        ErrorKind::Security
    );
    assert_eq!(
        store
            .open_in_place(k2, &[0; AES_GCM_NONCE_LEN], &[], &mut data, &[0; 8])
            .unwrap_err()
            .kind(),
        ErrorKind::Security
    );
    assert_eq!(
        store
            .open_in_place(
                k2,
                &[0; AES_GCM_NONCE_LEN],
                &[],
                &mut data,
                &[0; AES_GCM_TAG_LEN]
            )
            .unwrap_err()
            .kind(),
        ErrorKind::Security
    );
    // Unauthenticated ciphertext must not be modified on tag failure (355.0 §3.3.3.3 Note 2).
    assert_eq!(&data, b"hello");
}

#[test]
fn tm_aes256_gcm_round_trip_mcfc_exclusion_and_replay_scrub() {
    // req: API-070, API-073, API-074
    let scid = SpacecraftId::<10>::new(0x155).unwrap();
    let vcid = VcId::<3>::new(2).unwrap();
    let ctx = FrameCtx::Tm { scid, vcid };
    let spi = Spi::new(1).unwrap();
    let key = KeyId::new(10);
    let win = AntiReplayWindow::new(8).unwrap();
    let sec = SecurityLayout::new(
        HeaderFormat::BASELINE_GCM.total_len(),
        MacLen::BASELINE_GCM.get(),
    )
    .unwrap();

    let pc = TmPhysicalChannelConfig::new(64, true).unwrap();
    let mc = TmMasterChannelConfig::new(pc, scid, None, false)
        .with_vc(TmVirtualChannelConfig::new(vcid, TmContent::Packets).with_security(sec))
        .unwrap();

    let mut tx = SdlsEngine::<_, 2>::new(make_store(key, TEST_KEY));
    let mut rx = SdlsEngine::<_, 2>::new(make_store(key, TEST_KEY));
    let sa = SaConfig::baseline_gcm(spi, ctx, key, win);
    tx.add_sa(sa).unwrap();
    rx.add_sa(sa).unwrap();

    let payload = b"AES-256-GCM-TM-TELEMETRY!";
    let mut wire = [0_u8; 64];
    let fields = TmFrameFields::new(
        vcid,
        McFrameCount::new(5),
        TmVcFrameCount::new(1),
        DataFieldStatus::Packets(TmPacketPointer::StartsAt(
            TmPacketOffset::try_from(0).unwrap(),
        )),
    );
    let mut w = TmPlan::begin(&mut wire, &mc, &fields).unwrap();
    w.data_mut()[..payload.len()].copy_from_slice(payload);
    let _ = w.protect(&mut tx, &ctx).unwrap().finalize();

    // Independent OpenSSL AES-256-GCM known-answer verification for this exact TM frame
    // (6-octet PH + 14-octet SH + 26-octet Data + 16-octet MAC + 2-octet FECF = 64 octets).
    let expected_ct: [u8; 26] = [
        0x4e, 0x23, 0xf3, 0x6e, 0xe3, 0x4c, 0xc8, 0x78, 0x2b, 0xe1, 0x24, 0xf1, 0x3a, 0x40, 0x83,
        0x42, 0x32, 0x54, 0x9d, 0xec, 0xf9, 0x38, 0x19, 0x89, 0xad, 0xfa,
    ];
    let expected_tag: [u8; 16] = [
        0xef, 0x63, 0xeb, 0x3f, 0x38, 0xe2, 0xe2, 0x03, 0x44, 0xe3, 0xca, 0x7d, 0xe9, 0xf6, 0xd1,
        0x31,
    ];
    let wire_dec = TmFrame::decode_with(&wire, mc.pc(), &mc).unwrap();
    assert_eq!(wire_dec.data_field(), &expected_ct);
    assert_eq!(wire_dec.security_trailer(), &expected_tag);
    assert_ne!(&wire_dec.data_field()[..payload.len()], payload);
    assert_eq!(
        wire_dec.security_header().len(),
        HeaderFormat::BASELINE_GCM.total_len()
    );
    assert_eq!(wire_dec.security_trailer().len(), AES_GCM_TAG_LEN);

    let sec_fields = SecuredFields::adjacent(
        6,
        wire_dec.security_header().len(),
        wire_dec.data_field().len(),
        wire_dec.security_trailer().len(),
    );

    // Mutating TM MCFC (octet 2, masked to 0x00 per 355.0 §4.2.2.6.2 c) must still verify!
    let mut mcfc_changed = wire;
    mcfc_changed[2] = 0x99;
    let _ = aldis_core::crc::write_fecf(&mut mcfc_changed);
    let mut replay_copy = mcfc_changed;

    let plain = Plain::verify_with(&mut rx, &ctx, &mut mcfc_changed, &sec_fields, true, |b| {
        TmFrame::decode_with(b, mc.pc(), &mc)
    })
    .unwrap();
    assert_eq!(&plain.data_field()[..payload.len()], payload);
    assert_eq!(rx.last_status(), VerificationStatus::NoFailure);

    // Replaying the same frame must fail AntiReplayFailure and scrub plaintext.
    assert_eq!(
        Plain::verify_with(&mut rx, &ctx, &mut replay_copy, &sec_fields, true, |b| {
            TmFrame::decode_with(b, mc.pc(), &mc)
        })
        .unwrap_err()
        .kind(),
        ErrorKind::Security
    );
    assert_eq!(rx.last_status(), VerificationStatus::AntiReplayFailure);
    assert!(replay_copy[sec_fields.data()].iter().all(|&b| b == 0));

    // Tampering a ciphertext byte must fail MAC verification before replay check.
    let mut tampered = wire;
    tampered[20] ^= 0x01;
    let _ = aldis_core::crc::write_fecf(&mut tampered);
    assert_eq!(
        Plain::verify_with(&mut rx, &ctx, &mut tampered, &sec_fields, true, |b| {
            TmFrame::decode_with(b, mc.pc(), &mc)
        })
        .unwrap_err()
        .kind(),
        ErrorKind::Security
    );
    assert_eq!(rx.last_status(), VerificationStatus::MacFailure);
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "end-to-end TC, AOS, and USLP round trips"
)]
fn tc_aos_uslp_aes256_gcm_round_trips() {
    // req: API-070, API-073, API-074
    let spi = Spi::new(7).unwrap();
    let key = KeyId::new(22);
    let win = AntiReplayWindow::new(16).unwrap();
    let sec = SecurityLayout::new(
        HeaderFormat::BASELINE_GCM.total_len(),
        MacLen::BASELINE_GCM.get(),
    )
    .unwrap();
    let payload = b"CCSDS-352-AES256-GCM";

    // --- TC ---
    let tc_scid = TcSpacecraftId::new(341).unwrap();
    let tc_vc_id = TcVcId::new(0).unwrap();
    let tc_map = TcMapId::new(1).unwrap();
    let tc_ctx = FrameCtx::Tc {
        scid: tc_scid,
        vcid: tc_vc_id,
        map_id: Some(tc_map),
    };
    let tc_ch = TcVcTable::<1, 1>::new(
        TcPc::new(128, true).unwrap(),
        &[TcVcConfig::new(tc_scid, tc_vc_id, 128, SegmentHeaderUse::Present).unwrap()],
        &[TcMapConfig::new(tc_scid, tc_vc_id, tc_map, sec)],
    )
    .unwrap();
    let mut tc_tx = SdlsEngine::<_, 2>::new(make_store(key, TEST_KEY));
    let mut tc_rx = SdlsEngine::<_, 2>::new(make_store(key, TEST_KEY));
    let tc_sa = SaConfig::baseline_gcm(spi, tc_ctx, key, win);
    tc_tx.add_sa(tc_sa).unwrap();
    tc_rx.add_sa(tc_sa).unwrap();

    let mut tc_buf = [0_u8; 128];
    let tc_fields = TcFrameFields {
        frame_type: FrameType::Ad,
        spacecraft_id: tc_scid,
        vc_id: tc_vc_id,
        sequence_number: FrameSeqNum::new(0),
    };
    let seg = SegmentHeader {
        sequence_flags: SequenceFlags::Unsegmented,
        map_id: tc_map,
    };
    let mut tw = TcPlan::begin(&mut tc_buf, &tc_ch, tc_fields, Some(seg), payload.len()).unwrap();
    tw.data_mut()[..payload.len()].copy_from_slice(payload);
    let tc_len = tw
        .protect(payload.len(), &mut tc_tx, &tc_ctx)
        .unwrap()
        .finalize()
        .len();
    let tc_dec = TcFrame::decode(&tc_buf[..tc_len], &tc_ch).unwrap();
    assert_ne!(tc_dec.data_field(), payload);
    let tc_sec_fields = SecuredFields::adjacent(
        6,
        tc_dec.security_header().len(),
        tc_dec.data_field().len(),
        tc_dec.security_trailer().len(),
    );
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

    // --- AOS ---
    let aos_scid = SpacecraftId::<10>::new(341).unwrap();
    let aos_vc_id = VcId::<6>::new(3).unwrap();
    let aos_ctx = FrameCtx::Aos {
        scid: aos_scid,
        vcid: aos_vc_id,
    };
    let aos_phys = AosPhysicalChannelConfig::new(64, true, 0, true, AosEdition::Issue5).unwrap();
    let aos_vc = AosVirtualChannelConfig::new(aos_scid, aos_vc_id, AosContent::Mpdu)
        .unwrap()
        .with_security(sec);
    let aos_ch = AosVcTable::<2>::new(aos_phys, &[aos_vc]).unwrap();
    let mut aos_tx = SdlsEngine::<_, 2>::new(make_store(key, TEST_KEY));
    let mut aos_rx = SdlsEngine::<_, 2>::new(make_store(key, TEST_KEY));
    let aos_sa = SaConfig::baseline_gcm(spi, aos_ctx, key, win);
    aos_tx.add_sa(aos_sa).unwrap();
    aos_rx.add_sa(aos_sa).unwrap();

    let mut aos_buf = [0_u8; 64];
    let aos_fields = AosFrameFields::new(
        aos_scid,
        aos_vc_id,
        AosVcFrameCount::try_from(1).unwrap(),
        PduHeader::Mpdu(AosPacketPointer::StartsAt(
            AosPacketOffset::try_from(0).unwrap(),
        )),
    );
    let mut aw = AosPlan::begin(&mut aos_buf, &aos_phys, &aos_ch, &aos_fields).unwrap();
    aw.data_mut()[..payload.len()].copy_from_slice(payload);
    let aos_len = aw.protect(&mut aos_tx, &aos_ctx).unwrap().finalize().len();
    let aos_sec = SecuredFields::adjacent(8, 14, aos_len - 8 - 14 - 16 - 2, 16);
    let aos_plain = Plain::verify_with(
        &mut aos_rx,
        &aos_ctx,
        &mut aos_buf[..aos_len],
        &aos_sec,
        true,
        |b| AosFrame::decode(b, &aos_phys, &aos_ch),
    )
    .unwrap();
    assert!(
        matches!(aos_plain.data_unit(), DataUnit::Mpdu(m) if &m.packet_zone()[..payload.len()] == payload)
    );

    // --- USLP ---
    let uslp_scid = UslpSpacecraftId::new(341).unwrap();
    let uslp_vc_id = UslpVcId::new(2).unwrap();
    let uslp_map = UslpMapId::new(5).unwrap();
    let uslp_ctx = FrameCtx::Uslp {
        scid: uslp_scid,
        vcid: uslp_vc_id,
        map_id: Some(uslp_map),
    };
    let uslp_phys = UslpPc::new(TransferFrameType::Variable, 128, 0, true).unwrap();
    let mut vc_params = VcParams::new(TransferFrameType::Variable);
    vc_params.security = SecurityScope::PerMap;
    let uslp_ch = UslpVcTable::<2, 1>::new(
        uslp_phys,
        &[UslpVcConfig::new(uslp_scid, uslp_vc_id, vc_params).unwrap()],
        &[UslpMapConfig::new(uslp_scid, uslp_vc_id, uslp_map, sec)],
    )
    .unwrap();
    let mut uslp_tx = SdlsEngine::<_, 2>::new(make_store(key, TEST_KEY));
    let mut uslp_rx = SdlsEngine::<_, 2>::new(make_store(key, TEST_KEY));
    let uslp_sa = SaConfig::baseline_gcm(spi, uslp_ctx, key, win);
    uslp_tx.add_sa(uslp_sa).unwrap();
    uslp_rx.add_sa(uslp_sa).unwrap();

    let mut uslp_buf = [0_u8; 128];
    let uslp_fields = UslpFrameFields {
        spacecraft_id: uslp_scid,
        source_or_destination: SourceOrDestination::Destination,
        vc_id: uslp_vc_id,
        map_id: uslp_map,
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
    let uslp_sec = SecuredFields::adjacent(7, 14, uslp_len - 7 - 14 - 16 - 2, 16);
    let uslp_plain = Plain::verify_with(
        &mut uslp_rx,
        &uslp_ctx,
        &mut uslp_buf[..uslp_len],
        &uslp_sec,
        true,
        |b| UslpFrame::decode(b, &uslp_ch),
    )
    .unwrap();
    assert_eq!(uslp_plain.tfdz(), payload);
}

#[test]
fn unsupported_nonce_or_tag_length_rejected_by_provider() {
    // req: API-070, API-073
    let ctx = FrameCtx::Tm {
        scid: SpacecraftId::<10>::new(1).unwrap(),
        vcid: VcId::<3>::new(0).unwrap(),
    };
    let spi = Spi::new(1).unwrap();
    let key = KeyId::new(1);
    let win = AntiReplayWindow::new(4).unwrap();

    // Non-12-byte IV rejected by AES-256-GCM provider.
    let bad_iv_sa = SaConfig::new(
        spi,
        ctx,
        ServiceType::AuthenticatedEncryption,
        key,
        HeaderFormat::new(IvLen::new(8).unwrap(), SnLen::NONE, PlLen::NONE),
        MacLen::BASELINE_GCM,
        win,
        SequenceNumber::ZERO,
    )
    .unwrap();
    let mut tx = SdlsEngine::<_, 2>::new(make_store(key, TEST_KEY));
    tx.add_sa(bad_iv_sa).unwrap();
    let mut frame = [0_u8; 40];
    let fields = SecuredFields::adjacent(6, 10, 4, 16);
    assert_eq!(
        tx.protect(&ctx, &mut frame, &fields).unwrap_err().kind(),
        ErrorKind::Security
    );

    // Non-16-byte MAC rejected by AES-256-GCM provider.
    let bad_mac_sa = SaConfig::new(
        Spi::new(2).unwrap(),
        FrameCtx::Tm {
            scid: SpacecraftId::<10>::new(1).unwrap(),
            vcid: VcId::<3>::new(1).unwrap(),
        },
        ServiceType::AuthenticatedEncryption,
        key,
        HeaderFormat::BASELINE_GCM,
        MacLen::new(8).unwrap(),
        win,
        SequenceNumber::ZERO,
    )
    .unwrap();
    tx.add_sa(bad_mac_sa).unwrap();
    let fields2 = SecuredFields::adjacent(6, 14, 4, 8);
    assert_eq!(
        tx.protect(
            &FrameCtx::Tm {
                scid: SpacecraftId::<10>::new(1).unwrap(),
                vcid: VcId::<3>::new(1).unwrap(),
            },
            &mut frame,
            &fields2
        )
        .unwrap_err()
        .kind(),
        ErrorKind::Security
    );
}

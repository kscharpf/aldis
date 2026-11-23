//! Calls every public `aldis-sdls` API (VER-010) and runs every `test-vectors/sdls/` case.

use core::hint::black_box as bb;

use aldis_core::ids::{SpacecraftId, VcId};
use aldis_core::security::{FrameCtx, SecuredFields, SecurityHook};
use aldis_sdls::{
    AntiReplayWindow, HeaderFormat, IvLen, KeyId, MAX_AAD_LEN, MacLen, NoOpProvider, PlLen,
    SaConfig, SdlsEngine, SecurityHeaderView, SequenceNumber, ServiceType, SnLen, Spi,
    VerificationStatus, build_aad, encode_header,
};

use crate::exercise::Tally;
use crate::vectors::{Case, Fail, Out, Params as _, Run};

/// Runs the `aldis-sdls` API exercise on bare metal (VER-010).
pub fn run(t: &mut Tally) {
    let Ok(spi) = Spi::new(bb(1)) else {
        t.check(false);
        return;
    };
    t.check(spi.get() == 1 && Spi::new(bb(0)).is_err() && Spi::new(bb(u16::MAX)).is_err());

    let Ok(iv_len) = IvLen::new(bb(12)) else {
        t.check(false);
        return;
    };
    let Ok(sn_len) = SnLen::new(bb(0)) else {
        t.check(false);
        return;
    };
    let Ok(pl_len) = PlLen::new(bb(0)) else {
        t.check(false);
        return;
    };
    let Ok(mac_len) = MacLen::new(bb(16)) else {
        t.check(false);
        return;
    };
    let Ok(win) = AntiReplayWindow::new(bb(8)) else {
        t.check(false);
        return;
    };
    t.check(AntiReplayWindow::new(bb(0)).is_err());
    t.check(IvLen::new(bb(33)).is_err() && SnLen::new(bb(1)).is_err());
    t.check(PlLen::new(bb(3)).is_err() && MacLen::new(bb(7)).is_err());

    let fmt = HeaderFormat::new(iv_len, sn_len, pl_len);
    t.check(fmt == HeaderFormat::BASELINE_GCM && fmt.total_len() == 14);
    t.check(fmt.security_layout(mac_len).is_ok());

    let mut sh = [0_u8; 14];
    let iv = [0_u8; 12];
    t.check(encode_header(&mut sh, fmt, spi, &iv, &[], &[]).is_ok());
    t.check(SecurityHeaderView::decode(&sh, fmt).is_ok_and(|v| v.spi() == spi));

    let Some(scid) = SpacecraftId::<10>::new(bb(0x155)) else {
        t.check(false);
        return;
    };
    let Some(vcid) = VcId::<3>::new(bb(2)) else {
        t.check(false);
        return;
    };
    let ctx = FrameCtx::Tm { scid, vcid };
    let Ok(cfg) = SaConfig::clear_mode(spi, ctx, fmt, mac_len, win) else {
        t.check(false);
        return;
    };
    let gcm = SaConfig::baseline_gcm(spi, ctx, KeyId::new(bb(1)), win);
    t.check(
        gcm.spi() == spi
            && gcm.ctx() == ctx
            && gcm.service() == ServiceType::AuthenticatedEncryption
            && gcm.key() == KeyId::new(1)
            && gcm.header() == fmt
            && gcm.mac_len() == mac_len
            && gcm.window() == win
            && gcm.initial_sn() == SequenceNumber::ZERO,
    );

    let mut tx: SdlsEngine<NoOpProvider, 2> = SdlsEngine::new(NoOpProvider);
    let mut rx: SdlsEngine<NoOpProvider, 2> = SdlsEngine::new(NoOpProvider);
    t.check(tx.add_sa(cfg).is_ok() && rx.add_sa(cfg).is_ok());

    let mut frame = bb([0_u8; 40]);
    let fields = SecuredFields::adjacent(6, 14, 4, 16);
    t.check(tx.protect(&ctx, &mut frame, &fields).is_ok());
    t.check(tx.sequence_number(spi) == Some(SequenceNumber::new(1)));
    let mut aad = [0_u8; MAX_AAD_LEN];
    t.check(build_aad(&ctx, &frame, &fields, fmt.iv_range(), &mut aad) == Ok(20));
    t.check(rx.verify(&ctx, &mut frame, &fields) == Ok(20..24));
    t.check(rx.last_status() == VerificationStatus::NoFailure);
    // Replay must be rejected.
    t.check(rx.verify(&ctx, &mut frame, &fields).is_err());
    t.check(rx.last_status() == VerificationStatus::AntiReplayFailure);
}

/// Runs one `test-vectors/sdls/` codec case.
pub fn vector(case: &Case, out: &mut Out) -> Run {
    if case.op != "decode_header" {
        return Err(Fail::NotRepresentable);
    }
    let iv_len = IvLen::new(case.config.num("iv_len")?)?;
    let sn_len = SnLen::new(case.config.num("sn_len")?)?;
    let pl_len = PlLen::new(case.config.num("pl_len")?)?;
    let fmt = HeaderFormat::new(iv_len, sn_len, pl_len);
    let view = SecurityHeaderView::decode(bb(case.input), fmt)?;
    out.num("spi", u64::from(view.spi().get()));
    out.bytes("iv_hex", view.iv());
    out.bytes("sn_hex", view.sn());
    out.bytes("pad_len_hex", view.pad_len());
    Ok(())
}

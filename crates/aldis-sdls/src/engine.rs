//! SDLS Security Association database and `SecurityHook` engine (355.0-B-2 §4.2).

use core::ops::Range;

use aldis_core::error::{Error, ErrorKind, Result};
use aldis_core::field;
use aldis_core::security::{FrameCtx, HookLayout, SecuredFields, SecurityHook, SecurityLayout};

use crate::aad::{MAX_AAD_LEN, build_aad};
use crate::header::{HeaderFormat, SecurityHeaderView, decode_spi, encode_header};
use crate::provider::AeadProvider;
use crate::types::{
    AntiReplayWindow, KeyId, MacLen, SequenceNumber, ServiceType, Spi, VerificationStatus,
};

/// Static and initial dynamic parameters of one Security Association (355.0-B-2 §4.2.2, Table 6-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaConfig {
    /// Unique Security Parameter Index (1..=65534, §4.2.2.3).
    pub spi: Spi,
    /// Global Virtual Channel or MAP bound to this SA (§4.2.2.2).
    pub ctx: FrameCtx,
    /// Cryptographic service type (§4.2.2.4).
    pub service: ServiceType,
    /// Opaque cryptographic key handle passed to the [`AeadProvider`] (OI-84).
    pub key: KeyId,
    /// Security Header field lengths (§4.1.1).
    pub header: HeaderFormat,
    /// Security Trailer (MAC) length (§4.1.2).
    pub mac_len: MacLen,
    /// Anti-replay sequence number window width (`> 0`, §4.2.4.4 i).
    pub window: AntiReplayWindow,
    /// Initial managed sequence number / IV counter (§4.2.3.4, §4.2.4.4).
    pub initial_sn: SequenceNumber,
}

impl SaConfig {
    /// Baseline AES-GCM Authenticated Encryption SA (355.0-B-2 Annex E1/E3/E4):
    /// 14-octet Security Header (2 SPI + 12 IV), 16-octet Security Trailer (128-bit MAC).
    #[must_use]
    pub const fn baseline_gcm(
        spi: Spi,
        ctx: FrameCtx,
        key: KeyId,
        window: AntiReplayWindow,
    ) -> Self {
        Self {
            spi,
            ctx,
            service: ServiceType::AuthenticatedEncryption,
            key,
            header: HeaderFormat::BASELINE_GCM,
            mac_len: MacLen::BASELINE_GCM,
            window,
            initial_sn: SequenceNumber::ZERO,
        }
    }

    /// Validates that the SA configuration is internally consistent (355.0-B-2 §4.2.2.5–8).
    ///
    /// # Errors
    /// `InvalidConfig` if an `AuthenticatedEncryption` SA has `mac_len == 0`, has `iv_len == 0`
    /// (AEAD requires a nonce), has `iv_len > 16` when `sn_len == 0` (counter-mode IV exceeds
    /// 128-bit counter width), or has `pl_len > 0` (AEAD uses no padding).
    pub const fn validate(&self) -> Result<SecurityLayout> {
        let Ok(layout) = self.header.security_layout(self.mac_len) else {
            return Err(Error::new(ErrorKind::InvalidConfig));
        };
        if matches!(self.service, ServiceType::AuthenticatedEncryption) {
            if self.mac_len.get() == 0 {
                return Err(
                    Error::new(ErrorKind::InvalidConfig).with_field(field::SECURITY_TRAILER_LEN)
                );
            }
            if self.header.iv_len().get() == 0 {
                return Err(
                    Error::new(ErrorKind::InvalidConfig).with_field(field::SECURITY_HEADER_LEN)
                );
            }
            if self.header.pl_len().get() > 0 {
                return Err(
                    Error::new(ErrorKind::InvalidConfig).with_field(field::SECURITY_HEADER_LEN)
                );
            }
            if self.header.sn_len().get() == 0 && self.header.iv_len().get() > 16 {
                return Err(
                    Error::new(ErrorKind::InvalidConfig).with_field(field::SECURITY_HEADER_LEN)
                );
            }
        }
        Ok(layout)
    }

    /// Length in octets of the anti-replay counter field (SN if present, else IV; §4.1.1.4 Note).
    const fn counter_len(&self) -> usize {
        if self.header.sn_len().get() > 0 {
            self.header.sn_len().get()
        } else {
            self.header.iv_len().get()
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct SaSlot {
    config: SaConfig,
    layout: SecurityLayout,
    sn: SequenceNumber,
}

/// Fixed-capacity SDLS engine implementing [`SecurityHook`] (355.0-B-2 §4.2.3, §4.2.4).
///
/// Holds up to `N` Security Associations with zero heap allocation (`GEN-010`).
#[derive(Debug)]
pub struct SdlsEngine<P, const N: usize> {
    provider: P,
    sas: [Option<SaSlot>; N],
    last_status: VerificationStatus,
}

impl<P, const N: usize> SdlsEngine<P, N> {
    /// Creates an empty SDLS engine backed by `provider`.
    #[must_use]
    pub const fn new(provider: P) -> Self {
        Self {
            provider,
            sas: [None; N],
            last_status: VerificationStatus::NoFailure,
        }
    }

    /// Inserts a Security Association into the engine's table (355.0-B-2 §4.2.2.1–3).
    ///
    /// # Errors
    /// `InvalidConfig` if `config` fails validation, its SPI is already registered, or another SA
    /// is already active for `config.ctx` (§4.2.2.2.2); `CapacityExceeded` if all `N` slots are
    /// full.
    pub fn add_sa(&mut self, config: SaConfig) -> Result<()> {
        add_sa_slot(&mut self.sas, config)
    }

    /// Returns the current managed sequence number of `spi` (for caller persistence, OI-85).
    #[must_use]
    pub fn sequence_number(&self, spi: Spi) -> Option<SequenceNumber> {
        sequence_number_in(&self.sas, spi)
    }

    /// Returns the `VerificationStatus` from the most recent `verify` call (355.0-B-2 §3.3.3.2).
    #[must_use]
    pub const fn last_status(&self) -> VerificationStatus {
        self.last_status
    }

    /// Shared reference to the underlying cryptographic provider.
    #[must_use]
    pub const fn provider(&self) -> &P {
        &self.provider
    }
}

impl<P: AeadProvider, const N: usize> SecurityHook for SdlsEngine<P, N> {
    fn layout(&self) -> HookLayout {
        HookLayout::Any
    }

    /// `ApplySecurity` (355.0-B-2 §4.2.3).
    fn protect(&mut self, ctx: &FrameCtx, frame: &mut [u8], fields: &SecuredFields) -> Result<()> {
        protect_in(&self.provider, &mut self.sas, *ctx, frame, fields)
    }

    /// `ProcessSecurity` (355.0-B-2 §4.2.4).
    fn verify(
        &mut self,
        ctx: &FrameCtx,
        frame: &mut [u8],
        fields: &SecuredFields,
    ) -> Result<Range<usize>> {
        verify_in(
            &self.provider,
            &mut self.sas,
            &mut self.last_status,
            *ctx,
            frame,
            fields,
        )
    }
}

fn add_sa_slot(sas: &mut [Option<SaSlot>], config: SaConfig) -> Result<()> {
    let layout = config.validate()?;
    // bound: `sas.len() == N`.
    for entry in sas.iter().flatten() {
        if entry.config.spi == config.spi || entry.config.ctx == config.ctx {
            return Err(Error::new(ErrorKind::InvalidConfig));
        }
    }
    // bound: `sas.len() == N`.
    for slot in sas.iter_mut() {
        if slot.is_none() {
            *slot = Some(SaSlot {
                config,
                layout,
                sn: config.initial_sn,
            });
            return Ok(());
        }
    }
    Err(Error::new(ErrorKind::CapacityExceeded))
}

fn sequence_number_in(sas: &[Option<SaSlot>], spi: Spi) -> Option<SequenceNumber> {
    // bound: `sas.len() == N`.
    for entry in sas.iter().flatten() {
        if entry.config.spi == spi {
            return Some(entry.sn);
        }
    }
    None
}

fn protect_in(
    provider: &dyn AeadProvider,
    sas: &mut [Option<SaSlot>],
    ctx: FrameCtx,
    frame: &mut [u8],
    fields: &SecuredFields,
) -> Result<()> {
    let Some(sa) = sas
        .iter_mut()
        .flatten()
        .find(|entry| entry.config.ctx == ctx)
    else {
        return Err(Error::new(ErrorKind::Security));
    };
    let config = sa.config;
    let layout = sa.layout;
    if fields.header().len() != layout.header_len()
        || fields.trailer().len() != layout.trailer_len()
    {
        return Err(Error::new(ErrorKind::InvalidField).with_field(field::SECURED_FIELDS));
    }

    let counter_len = config.counter_len();
    let next_sn = if counter_len > 0 {
        sa.sn.checked_next(counter_len)?
    } else {
        sa.sn
    };

    let mut iv_buf = [0_u8; 32];
    let mut sn_buf = [0_u8; 8];
    let iv_len = config.header.iv_len().get();
    let sn_len = config.header.sn_len().get();
    let iv_slice = iv_buf.get_mut(..iv_len).unwrap_or_default();
    let sn_slice = sn_buf.get_mut(..sn_len).unwrap_or_default();
    if sn_len > 0 {
        next_sn.encode_be(sn_slice)?;
    } else if iv_len > 0 {
        next_sn.encode_be(iv_slice)?;
    } else {
        // ClearMode SA with neither IV nor SN.
    }

    // Write the Security Header into `frame` before computing AAD (§4.2.3.4 c–d).
    {
        let Some(sh_dst) = frame.get_mut(fields.header()) else {
            return Err(Error::new(ErrorKind::InvalidField).with_field(field::SECURED_FIELDS));
        };
        encode_header(sh_dst, config.header, config.spi, iv_slice, sn_slice, &[])?;
    }

    let mut aad_buf = [0_u8; MAX_AAD_LEN];
    let aad_len = build_aad(&ctx, frame, fields, config.header.iv_range(), &mut aad_buf)?;
    let aad = aad_buf.get(..aad_len).unwrap_or_default();

    let data_range = fields.data();
    let trailer_range = fields.trailer();
    let (before_trailer, trailer_and_after) = frame
        .split_at_mut_checked(trailer_range.start)
        .ok_or_else(|| Error::new(ErrorKind::InvalidField).with_field(field::SECURED_FIELDS))?;
    let data = before_trailer.get_mut(data_range).unwrap_or_default();
    let Some(tag_out) = trailer_and_after.get_mut(..trailer_range.len()) else {
        return Err(Error::new(ErrorKind::InvalidField).with_field(field::SECURED_FIELDS));
    };

    match config.service {
        ServiceType::AuthenticatedEncryption => {
            provider.seal_in_place(config.key, iv_slice, aad, data, tag_out)?;
        }
        ServiceType::ClearMode => {
            tag_out.fill(0);
        }
    }

    // Commit the incremented sequence number only after sealing succeeds.
    // bound: `sas.len() == N`.
    for entry in sas.iter_mut().flatten() {
        if entry.config.spi == config.spi {
            entry.sn = next_sn;
        }
    }
    Ok(())
}

fn verify_in(
    provider: &dyn AeadProvider,
    sas: &mut [Option<SaSlot>],
    last_status: &mut VerificationStatus,
    ctx: FrameCtx,
    frame: &mut [u8],
    fields: &SecuredFields,
) -> Result<Range<usize>> {
    *last_status = VerificationStatus::NoFailure;

    let Some(sh_bytes) = frame.get(fields.header()) else {
        *last_status = VerificationStatus::InvalidSpi;
        return Err(Error::new(ErrorKind::Security));
    };
    let Ok(spi) = decode_spi(sh_bytes) else {
        *last_status = VerificationStatus::InvalidSpi;
        return Err(Error::new(ErrorKind::Security));
    };

    // §4.2.4.3: verify SPI maps to an SA bound to this GVCID/GMAP_ID.
    let Some(sa) = sas
        .iter_mut()
        .flatten()
        .find(|entry| entry.config.spi == spi)
    else {
        *last_status = VerificationStatus::InvalidSpi;
        return Err(Error::new(ErrorKind::Security));
    };
    if sa.config.ctx != ctx {
        *last_status = VerificationStatus::InvalidSpi;
        return Err(Error::new(ErrorKind::Security));
    }
    let config = sa.config;
    let layout = sa.layout;
    let managed_sn = sa.sn;

    if fields.header().len() != layout.header_len()
        || fields.trailer().len() != layout.trailer_len()
    {
        *last_status = VerificationStatus::InvalidSpi;
        return Err(Error::new(ErrorKind::InvalidField).with_field(field::SECURED_FIELDS));
    }

    let hdr = SecurityHeaderView::decode(sh_bytes, config.header)?;

    let mut iv_buf = [0_u8; 32];
    let iv_slice = iv_buf.get_mut(..hdr.iv().len()).unwrap_or_default();
    for (d, &s) in iv_slice.iter_mut().zip(hdr.iv().iter()) {
        *d = s;
    }

    let received_sn = if !hdr.sn().is_empty() {
        Some(SequenceNumber::decode_be(hdr.sn())?)
    } else if !hdr.iv().is_empty() {
        Some(SequenceNumber::decode_be(hdr.iv())?)
    } else {
        None
    };

    let mut aad_buf = [0_u8; MAX_AAD_LEN];
    let aad_len = build_aad(&ctx, frame, fields, config.header.iv_range(), &mut aad_buf)?;
    let aad = aad_buf.get(..aad_len).unwrap_or_default();

    let data_range = fields.data();
    let trailer_range = fields.trailer();
    let (before_trailer, trailer_and_after) = frame
        .split_at_mut_checked(trailer_range.start)
        .ok_or_else(|| Error::new(ErrorKind::InvalidField).with_field(field::SECURED_FIELDS))?;
    let data = before_trailer
        .get_mut(data_range.clone())
        .unwrap_or_default();
    let Some(tag) = trailer_and_after.get(..trailer_range.len()) else {
        return Err(Error::new(ErrorKind::InvalidField).with_field(field::SECURED_FIELDS));
    };

    // §4.2.4.4 a–e: verify MAC / AEAD tag before checking or advancing anti-replay.
    match config.service {
        ServiceType::AuthenticatedEncryption => {
            if provider
                .open_in_place(config.key, iv_slice, aad, data, tag)
                .is_err()
            {
                *last_status = VerificationStatus::MacFailure;
                return Err(Error::new(ErrorKind::Security));
            }
        }
        ServiceType::ClearMode => {}
    }

    // §4.2.4.4 f–i: anti-replay sequence number window check.
    if let Some(rx_sn) = received_sn {
        if !managed_sn.accepts(rx_sn, config.window) {
            // Scrub decrypted plaintext before rejecting the replayed frame.
            data.fill(0);
            *last_status = VerificationStatus::AntiReplayFailure;
            return Err(Error::new(ErrorKind::Security));
        }
        // §4.2.4.4 j: update managed sequence number only after both MAC and window pass.
        // bound: `sas.len() == N`.
        for entry in sas.iter_mut().flatten() {
            if entry.config.spi == spi {
                entry.sn = rx_sn;
            }
        }
    }

    Ok(data_range)
}

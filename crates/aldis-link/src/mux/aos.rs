//! The AOS implementation of the `ChannelMux` (732.0 §4.1; LNK-030…LNK-036, LNK-076, LNK-082).

use aldis_aos::{
    AosEdition, AosPhysicalChannelConfig, AosPrimaryHeader, AosVirtualChannelConfig, VcFrameCount,
};
use aldis_core::ids::{SpacecraftId, VcId};
use aldis_core::layout::OCF_LEN;
use aldis_core::security::SecurityLayout;
use aldis_core::{CopFrameKind, Error, ErrorKind, Result, field};

use super::{Admitted, FrameFields, Fsh, MuxProtocol, VcConfig};
use crate::protocol::Aos;

/// The parameters of the physical channel of an AOS mux (732.0 §5.2 Table 5-1; LNK-030,
/// LNK-032, LNK-035).
///
/// Built only by [`new`](Self::new), which relates the edition to the SCID (RS-020).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AosChannel {
    physical: AosPhysicalChannelConfig,
    scid: SpacecraftId<10>,
    cycle_in_use: bool,
}

impl AosChannel {
    /// A channel of the Spacecraft Identifier `scid` on the physical channel `physical`,
    /// whose managed parameters give the edition of the standard and whether frames have a
    /// Frame Header Error Control field (which the caller computes, LNK-035), with the VC
    /// Frame Count Cycle in use if `cycle_in_use`: the Use Flag is set and the
    /// cycle counts the wraps of the VC Frame Count (LNK-032; 732.0 §4.1.2.5.5).
    ///
    /// # Errors
    /// `InvalidConfig` (`spacecraft_id`) for a SCID above 255 in the Issue 4 edition, whose
    /// SCID has 8 bits (732.0 §4.1.2.2.3 and its note 1; AOS-002).
    pub fn new(
        physical: AosPhysicalChannelConfig,
        scid: SpacecraftId<10>,
        cycle_in_use: bool,
    ) -> Result<Self> {
        if physical.edition() == AosEdition::Issue4Compatible && scid.get() > 0xFF {
            Err(Error::new(ErrorKind::InvalidConfig).with_field(field::SPACECRAFT_ID))
        } else {
            Ok(Self {
                physical,
                scid,
                cycle_in_use,
            })
        }
    }

    /// The managed parameters of the physical channel.
    #[must_use]
    pub const fn physical(&self) -> &AosPhysicalChannelConfig {
        &self.physical
    }

    /// The Spacecraft Identifier, 10 bits (8 bits in the Issue 4 edition).
    ///
    /// Requirements: LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
    #[must_use]
    pub const fn scid(&self) -> SpacecraftId<10> {
        self.scid
    }

    /// The edition of the standard, which sets the SCID width (AOS-002).
    #[must_use]
    pub const fn edition(&self) -> AosEdition {
        self.physical.edition()
    }

    /// Whether frames have a Frame Header Error Control field.
    ///
    /// Requirements: LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
    #[must_use]
    pub const fn fhec(&self) -> bool {
        self.physical.fhec()
    }

    /// Whether the VC Frame Count Cycle is in use.
    ///
    /// Requirements: LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
    #[must_use]
    pub const fn cycle_in_use(&self) -> bool {
        self.cycle_in_use
    }
}

/// The parameters of an AOS VC (732.0 §5.4 Table 5-3; LNK-033).
///
/// The codec's configuration of the VC says whether it has a `VC_OCF` and its SDLS lengths
/// (M6 plan WP-2c); the engine adds the initial value of the `VC_OCF`.
///
/// Built only by [`new`](Self::new) and [`with_initial_ocf`](Self::with_initial_ocf), which
/// relate the initial value to the configuration (RS-020).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AosVc {
    config: AosVirtualChannelConfig,
    ocf: [u8; OCF_LEN],
}

impl AosVc {
    /// The VC of the configuration `config`, whose `VC_OCF` (if it has one) starts as 0.
    ///
    /// Requirements: LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
    #[must_use]
    pub const fn new(config: AosVirtualChannelConfig) -> Self {
        Self {
            config,
            ocf: [0; OCF_LEN],
        }
    }

    /// The same VC with the initial `VC_OCF` `ocf`.
    ///
    /// # Errors
    /// `InvalidConfig` (`ocf_flag`) if the configuration has no `VC_OCF`.
    pub const fn with_initial_ocf(self, ocf: [u8; OCF_LEN]) -> Result<Self> {
        if self.config.ocf() {
            Ok(Self { ocf, ..self })
        } else {
            Err(Error::new(ErrorKind::InvalidConfig).with_field(field::OCF_FLAG))
        }
    }

    /// The codec's configuration of the VC. Its data field content means nothing to the mux,
    /// which treats the field of an offer as opaque.
    ///
    /// Requirements: LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
    #[must_use]
    pub const fn config(&self) -> &AosVirtualChannelConfig {
        &self.config
    }
}

impl MuxProtocol for Aos {
    type Channel = AosChannel;
    type VcParams = AosVc;
    type OfferMeta = ();
    type Pointer = ();
    const INSERT_ZONE: bool = true;
    const MC_COUNT: bool = false;
    const OID_BYPASS: bool = false;

    fn physical(channel: &AosChannel) -> &AosPhysicalChannelConfig {
        &channel.physical
    }

    fn oid_vc(_: &AosChannel) -> aldis_core::ids::VcId<6> {
        aldis_aos::OID_VCID
    }

    fn mc_ocf(_: &AosChannel) -> Option<[u8; OCF_LEN]> {
        None
    }

    fn mc_fsh(_: &AosChannel) -> Option<Fsh> {
        None
    }

    fn vc_id(vc: &AosVc) -> VcId<6> {
        vc.config.vcid()
    }

    fn security(vc: &AosVc) -> SecurityLayout {
        vc.config.security()
    }

    fn vc_ocf(vc: &AosVc) -> Option<[u8; OCF_LEN]> {
        vc.config.ocf().then_some(vc.ocf)
    }

    fn vc_fsh(_: &AosVc) -> Option<Fsh> {
        None
    }

    fn admit(_: &AosVc, (): &()) -> Result<Admitted<()>> {
        Ok(Admitted {
            kind: CopFrameKind::Ad,
            given_count: None,
            pointer: (),
        })
    }

    // VCID 63 is no VC of the codec's configuration (`AosVirtualChannelConfig::new`), so the
    // OID VC cannot be listed; each VC belongs to the master channel of the physical channel.
    fn check_channel(channel: &AosChannel, vcs: &[Option<VcConfig<Self>>]) -> Result<()> {
        if vcs
            .iter()
            .flatten()
            .any(|v| v.params.config.scid() != channel.scid)
        {
            Err(Error::new(ErrorKind::InvalidConfig).with_field(field::SPACECRAFT_ID))
        } else {
            Ok(())
        }
    }

    fn header_extension(channel: &AosChannel, _: Option<&AosVc>, _: bool) -> bool {
        channel.physical.fhec()
    }

    fn primary_header(
        channel: &AosChannel,
        _: Option<&AosVc>,
        f: &FrameFields<Self>,
    ) -> Result<AosPrimaryHeader> {
        // The count and, where the cycle is in use, the cycle of the running count (732.0
        // §4.1.2.5.5; LNK-032).
        let (count, cycle) = VcFrameCount::wrapping(f.vc_count);
        Ok(AosPrimaryHeader::unsealed(
            channel.physical.edition(),
            channel.scid,
            f.vc,
            count,
            channel.cycle_in_use.then_some(cycle),
            channel.physical.fhec(),
        ))
    }
}

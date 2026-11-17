//! The USLP implementation of the `ChannelMux` (732.1; LNK-030…LNK-036, LNK-076, LNK-082).

use aldis_core::PhysicalChannel;
use aldis_core::ids::{MapId, SpacecraftId};
use aldis_core::layout::OCF_LEN;
use aldis_core::security::SecurityLayout;
use aldis_core::{CopFrameKind, Error, ErrorKind, Result, field};
use aldis_uslp::PhysicalChannelConfig as UslpPhysicalChannelConfig;
use aldis_uslp::VcConfig as UslpVcConfig;
use aldis_uslp::{
    ChannelFields, FrameKind, OcfUse, SecurityScope, SourceOrDestination, TransferFrameType,
    UslpPrimaryHeader, UslpVcId, VcfCount, VcfCountLen,
};

use super::{Admitted, Cop, FrameFields, Fsh, MuxProtocol, VcConfig};
use crate::common::Qos;
use crate::protocol::{Route, Uslp};

/// The parameters of the physical channel and master channel of a USLP mux with
/// fixed-length frames (732.1 §5.2, §5.3; LNK-030, LNK-032, LNK-033).
///
/// Built only by [`new`](Self::new), which takes the managed parameters of a fixed-length
/// physical channel (RS-020).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UslpChannel {
    physical: UslpPhysicalChannelConfig,
    /// The Spacecraft Identifier, 16 bits.
    pub scid: SpacecraftId<16>,
    /// Whether the SCID is the source or the destination of the frames.
    pub source_or_destination: SourceOrDestination,
    /// The VCF Count Length of VCID 63 (none by default; LNK-032).
    pub oid_vcf_len: VcfCountLen,
    mc_ocf: Option<[u8; OCF_LEN]>,
}

impl UslpChannel {
    /// A channel of the Spacecraft Identifier `scid` (`source_or_destination` says what it
    /// names) on the fixed-length physical channel `physical`, with VCID 63 frames that have
    /// a VCF Count of `oid_vcf_len`, and the initial `MC_OCF` `mc_ocf` if given: "Inclusion
    /// of OCF Required" is then true for the master channel, and every frame has an OCF, OID
    /// frames included (LNK-033, OI-76; 732.1 Table 5-2, §4.1.5.3). The VCs of the mux must
    /// say the same ([`OcfUse::Required`], Table 5-3), because the mux has no `VC_OCF` slot
    /// for USLP; `ChannelMuxConfig::new` checks it.
    ///
    /// # Errors
    /// `InvalidConfig` (`transfer_frame_type`) for a physical channel of variable-length
    /// frames, which the mux does not build.
    pub fn new(
        physical: UslpPhysicalChannelConfig,
        scid: SpacecraftId<16>,
        source_or_destination: SourceOrDestination,
        oid_vcf_len: VcfCountLen,
        mc_ocf: Option<[u8; OCF_LEN]>,
    ) -> Result<UslpChannel> {
        if physical.variable_length() {
            return Err(Error::new(ErrorKind::InvalidConfig).with_field(field::TRANSFER_FRAME_TYPE));
        }
        Ok(UslpChannel {
            physical,
            scid,
            source_or_destination,
            oid_vcf_len,
            mc_ocf,
        })
    }

    /// The managed parameters of the physical channel.
    #[must_use]
    pub const fn physical(&self) -> &UslpPhysicalChannelConfig {
        &self.physical
    }

    /// The initial `MC_OCF`, if the master channel has one.
    ///
    /// Requirements: LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
    #[must_use]
    pub const fn mc_ocf(&self) -> Option<[u8; OCF_LEN]> {
        self.mc_ocf
    }
}

/// The parameters of a USLP VC (732.1 §5.4 Table 5-3; LNK-032, LNK-076).
///
/// The codec's configuration of the VC gives its VCF Count Lengths, its OCF, and its SDLS
/// lengths (M6 plan WP-2c); the engine adds the MAP and `QoS` of its frames and whether COP-1
/// is in effect.
///
/// Built only by [`new`](Self::new), which relates the extras to the configuration (RS-020).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UslpVc {
    config: UslpVcConfig,
    security: SecurityLayout,
    map: MapId<4>,
    qos: Qos,
    cop: bool,
}

impl UslpVc {
    /// A VC of the configuration `config` whose frames have the MAP ID `map`, and `qos` for
    /// an offered field that names none, with COP-1 in effect if `cop`, so that an offer may
    /// carry the `cop` block (LNK-076).
    ///
    /// # Errors
    /// `InvalidConfig`, checked in this order:
    /// - (`vcf_count_len`) for COP-1 in effect with a sequence-controlled VCF Count Length of
    ///   0: FOP-1 puts the Frame Sequence Number of a Type-AD frame in the VCF Count field,
    ///   which a length of `000` leaves out (732.1 §4.1.2.12.1, §4.2.7.4 note 3; WP-4's
    ///   reading, OI-78; LNK-078);
    /// - (`transfer_frame_type`) for a VC of variable-length frames, which the mux does not
    ///   build;
    /// - (`security_header_len`) for SDLS lengths set per MAP, which the mux has no MAP
    ///   configuration for.
    pub const fn new(config: UslpVcConfig, map: MapId<4>, qos: Qos, cop: bool) -> Result<UslpVc> {
        if let Err(e) = UslpVc::check_count_len(cop, config.vcf_count_len(false)) {
            return Err(e);
        }
        if !matches!(config.frame_type(), TransferFrameType::Fixed) {
            return Err(Error::new(ErrorKind::InvalidConfig).with_field(field::TRANSFER_FRAME_TYPE));
        }
        let SecurityScope::Vc(security) = config.security() else {
            return Err(Error::new(ErrorKind::InvalidConfig).with_field(field::SECURITY_HEADER_LEN));
        };
        Ok(UslpVc {
            config,
            security,
            map,
            qos,
            cop,
        })
    }

    /// The first check of [`new`](Self::new), for a caller that reads the members of a VC
    /// before it can build the codec's configuration of it: COP-1 in effect needs a VCF
    /// Count Length `vcf_len_sc` above 0 (LNK-078).
    ///
    /// # Errors
    /// `InvalidConfig` (`vcf_count_len`).
    pub const fn check_count_len(cop: bool, vcf_len_sc: VcfCountLen) -> Result<()> {
        if cop && vcf_len_sc.get() == 0 {
            Err(Error::new(ErrorKind::InvalidConfig).with_field(field::VCF_COUNT_LEN))
        } else {
            Ok(())
        }
    }

    /// The codec's configuration of the VC.
    ///
    /// Requirements: LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
    #[must_use]
    pub const fn config(&self) -> &UslpVcConfig {
        &self.config
    }

    /// The MAP ID of the VC's frames.
    ///
    /// Requirements: LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
    #[must_use]
    pub const fn map(&self) -> MapId<4> {
        self.map
    }

    /// The quality of service of an offered field that names none.
    ///
    /// Requirements: LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
    #[must_use]
    pub const fn qos(&self) -> Qos {
        self.qos
    }

    /// The VCF Count Length for sequence-controlled frames.
    ///
    /// Requirements: LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
    #[must_use]
    pub const fn vcf_len_sc(&self) -> VcfCountLen {
        self.config.vcf_count_len(false)
    }

    /// The VCF Count Length for expedited frames.
    ///
    /// Requirements: LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
    #[must_use]
    pub const fn vcf_len_exp(&self) -> VcfCountLen {
        self.config.vcf_count_len(true)
    }

    /// Whether COP-1 is in effect on the VC.
    ///
    /// Requirements: LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
    #[must_use]
    pub const fn cop(&self) -> bool {
        self.cop
    }
}

impl UslpVc {
    const fn count_len(self, bypass: bool) -> VcfCountLen {
        self.config.vcf_count_len(bypass)
    }
}

/// What a USLP offer carries besides its field: the MAP and `QoS` it is sent on, and the
/// COP-1 block (LNK-076).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UslpOffer {
    /// The MAP and `QoS` of the field: the MAP must be the VC's; without a route the VC's
    /// `QoS` applies.
    pub route: Option<Route>,
    /// The COP-1 block, for a VC with COP-1 in effect.
    pub cop: Option<Cop>,
}

impl MuxProtocol for Uslp {
    type Channel = UslpChannel;
    type VcParams = UslpVc;
    type OfferMeta = UslpOffer;
    type Pointer = ();
    const INSERT_ZONE: bool = true;
    const MC_COUNT: bool = false;
    const OID_BYPASS: bool = true;

    fn physical(channel: &UslpChannel) -> &UslpPhysicalChannelConfig {
        &channel.physical
    }

    fn oid_vc(_: &UslpChannel) -> UslpVcId {
        aldis_uslp::OID_VC
    }

    fn mc_ocf(channel: &UslpChannel) -> Option<[u8; OCF_LEN]> {
        channel.mc_ocf
    }

    fn mc_fsh(_: &UslpChannel) -> Option<Fsh> {
        None
    }

    fn vc_id(vc: &UslpVc) -> UslpVcId {
        vc.config.vc_id()
    }

    fn security(vc: &UslpVc) -> SecurityLayout {
        vc.security
    }

    fn vc_ocf(_: &UslpVc) -> Option<[u8; OCF_LEN]> {
        None
    }

    fn vc_fsh(_: &UslpVc) -> Option<Fsh> {
        None
    }

    fn admit(vc: &UslpVc, meta: &UslpOffer) -> Result<Admitted<()>> {
        let invalid = |f| Error::new(ErrorKind::InvalidField).with_field(f);
        let route_qos = match meta.route {
            Some(r) if r.map == vc.map => r.qos,
            Some(_) => {
                return Err(Error::new(ErrorKind::UnknownChannel).with_field(field::MAP_ID));
            }
            None => vc.qos,
        };
        let (kind, given_count) = match meta.cop {
            None => (
                if route_qos == Qos::Expedited {
                    CopFrameKind::Bd
                } else {
                    CopFrameKind::Ad
                },
                None,
            ),
            Some(_) if !vc.cop => return Err(invalid(field::COP_IN_EFFECT)),
            Some(c)
                if meta.route.is_some()
                    && (route_qos == Qos::Expedited) != c.kind().bypass_flag() =>
            {
                return Err(invalid(field::QOS));
            }
            Some(Cop::TypeAd { vcf_count }) if vcf_count > vc.count_len(false).max_count() => {
                return Err(Error::out_of_range(
                    field::VCF_COUNT,
                    vc.count_len(false).max_count(),
                    vcf_count,
                ));
            }
            Some(c) => (c.kind(), c.vcf_count()),
        };
        Ok(Admitted {
            kind,
            given_count,
            pointer: (),
        })
    }

    fn check_channel(channel: &UslpChannel, vcs: &[Option<VcConfig<Uslp>>]) -> Result<()> {
        // The frame length is at most `MAX_FRAME_LEN`, which the physical channel's
        // constructor checks.
        let bad = |f| Error::new(ErrorKind::InvalidConfig).with_field(f);
        if vcs.iter().flatten().any(|v| v.id() == aldis_uslp::OID_VCID) {
            return Err(bad(field::VC_ID));
        }
        let spacecraft = |v: &VcConfig<Uslp>| v.params.config.spacecraft_id() != channel.scid;
        if vcs.iter().flatten().any(spacecraft) {
            return Err(bad(field::SPACECRAFT_ID));
        }
        // 732.1 has two "Inclusion of OCF Required" parameters, one of the master channel
        // (Table 5-2, §4.1.5.3) and one of each VC (Table 5-3). The mux fills an OCF only
        // from `MC_OCF` (`vc_ocf` is `None` for USLP), so a VC that requires one is refused
        // without an `MC_OCF`, and a VC that does not is refused beside one, so that the
        // codec configuration describes the frames the mux builds.
        let ocf = |v: &VcConfig<Uslp>| {
            (v.params.config.ocf() == OcfUse::Required) != channel.mc_ocf.is_some()
        };
        if vcs.iter().flatten().any(ocf) {
            return Err(bad(field::OCF_FLAG));
        }
        Ok(())
    }

    fn header_extension(channel: &UslpChannel, vc: Option<&UslpVc>, bypass: bool) -> VcfCountLen {
        vc.map_or(channel.oid_vcf_len, |v| v.count_len(bypass))
    }

    fn primary_header(
        channel: &UslpChannel,
        vc: Option<&UslpVc>,
        f: &FrameFields<Uslp>,
    ) -> Result<UslpPrimaryHeader> {
        let bypass = f.kind.bypass_flag();
        let count_len = vc.map_or(channel.oid_vcf_len, |v| v.count_len(bypass));
        // The OID frame has MAP ID 0 (732.1 §4.1.4.1.9).
        let map_id = vc.map_or(MapId::<4>::from_const::<0>(), |v| v.map);
        UslpPrimaryHeader::for_writer(
            ChannelFields {
                spacecraft_id: channel.scid,
                source_or_destination: channel.source_or_destination,
                vc_id: f.vc,
                map_id,
            },
            f.frame_len,
            FrameKind::from(f.kind),
            f.ocf,
            VcfCount::wrapping(count_len, f.vc_count),
        )
    }
}

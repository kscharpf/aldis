//! The TM implementation of the `ChannelMux` (132.0 §4.1; LNK-030…LNK-036, LNK-076, LNK-082).

use aldis_core::ids::{SpacecraftId, VcId};
use aldis_core::layout::OCF_LEN;
use aldis_core::security::SecurityLayout;
use aldis_core::{CopFrameKind, Error, ErrorKind, Result, field};
use aldis_tm::TmPhysicalChannelConfig;
use aldis_tm::{
    DataFieldContent, FirstHeaderPointer, FshLen, SegmentLengthId, TmPrimaryHeader,
    TmVirtualChannelConfig,
};

use super::{Admitted, FrameFields, Fsh, MuxProtocol, VcConfig};
use crate::protocol::Tm;

/// The parameters of the physical channel and master channel of a TM mux (132.0 §5.1,
/// §5.2; LNK-030, LNK-033).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TmChannel {
    /// The managed parameters of the physical channel: the frame length and the FECF
    /// (132.0 Table 5-1; TM-001).
    pub physical: TmPhysicalChannelConfig,
    /// The Spacecraft Identifier, 10 bits (132.0 §4.1.2.2.3).
    pub scid: SpacecraftId<10>,
    /// The VC dedicated to OID frames, which is one of the VCs and carries Packets without
    /// SDLS (LNK-014, LNK-078; 132.0 §4.1.4.6.3).
    pub oid_vc: VcId<3>,
    /// The initial `MC_OCF`: every frame of the master channel has an OCF (LNK-033).
    pub mc_ocf: Option<[u8; OCF_LEN]>,
    /// The initial `MC_FSH`: every frame of the master channel has a Secondary Header
    /// (LNK-033).
    pub mc_fsh: Option<Fsh>,
}

impl TmChannel {
    /// A channel of the physical channel `physical` with no `MC_OCF` and no `MC_FSH`.
    ///
    /// Requirements: LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
    #[must_use]
    pub const fn new(
        physical: TmPhysicalChannelConfig,
        scid: SpacecraftId<10>,
        oid_vc: VcId<3>,
    ) -> Self {
        Self {
            physical,
            scid,
            oid_vc,
            mc_ocf: None,
            mc_fsh: None,
        }
    }
}

/// The parameters of a TM VC (132.0 §5.4 Table 5-3; LNK-033, LNK-082).
///
/// The codec's configuration of the VC says what it carries (the Synchronization Flag of its
/// frames, LNK-082), whether and how long its Secondary Header is, whether it has a `VC_OCF`,
/// and its SDLS lengths (M6 plan WP-2c); the engine adds the initial value of the slots.
///
/// Built only by [`new`](Self::new) and the `with_initial_` methods, which relate the initial
/// values to the configuration (RS-020).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TmVc {
    config: TmVirtualChannelConfig,
    ocf: [u8; OCF_LEN],
    fsh: Option<Fsh>,
}

impl TmVc {
    /// The VC of the configuration `config`, whose `VC_OCF` (if it has one) and `VC_FSH` data
    /// (of the length it gives) start as 0.
    ///
    /// Requirements: LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
    #[must_use]
    pub const fn new(config: TmVirtualChannelConfig) -> Self {
        Self {
            config,
            ocf: [0; OCF_LEN],
            fsh: match config.fsh() {
                Some(len) => Some(Fsh::zeroed(len)),
                None => None,
            },
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

    /// The same VC with the initial `VC_FSH` `fsh`.
    ///
    /// # Errors
    /// `InvalidConfig` (`fsh_len`) unless the configuration has a `VC_FSH` of the length of
    /// `fsh`.
    pub fn with_initial_fsh(self, fsh: Fsh) -> Result<Self> {
        if self.config.fsh().map(FshLen::octets) == Some(fsh.total_len()) {
            Ok(Self {
                fsh: Some(fsh),
                ..self
            })
        } else {
            Err(Error::new(ErrorKind::InvalidConfig).with_field(field::FSH_LEN))
        }
    }

    /// The codec's configuration of the VC.
    ///
    /// Requirements: LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
    #[must_use]
    pub const fn config(&self) -> &TmVirtualChannelConfig {
        &self.config
    }
}

/// What a TM offer carries besides its field: the raw First Header Pointer of the frame
/// header (LNK-076, LNK-082).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct TmOffer {
    /// The raw First Header Pointer: required on a TM VC of Packets and refused on a TM VC
    /// of `VCA_SDUs`.
    pub pointer: Option<u16>,
}

impl MuxProtocol for Tm {
    type Channel = TmChannel;
    type VcParams = TmVc;
    type OfferMeta = TmOffer;
    type Pointer = FirstHeaderPointer;
    const INSERT_ZONE: bool = false;
    const MC_COUNT: bool = true;
    const OID_BYPASS: bool = false;

    fn physical(channel: &TmChannel) -> &TmPhysicalChannelConfig {
        &channel.physical
    }

    fn oid_vc(channel: &TmChannel) -> VcId<3> {
        channel.oid_vc
    }

    fn mc_ocf(channel: &TmChannel) -> Option<[u8; OCF_LEN]> {
        channel.mc_ocf
    }

    fn mc_fsh(channel: &TmChannel) -> Option<Fsh> {
        channel.mc_fsh
    }

    fn vc_id(vc: &TmVc) -> VcId<3> {
        vc.config.vcid()
    }

    fn security(vc: &TmVc) -> SecurityLayout {
        vc.config.security()
    }

    fn vc_ocf(vc: &TmVc) -> Option<[u8; OCF_LEN]> {
        vc.config.ocf().then_some(vc.ocf)
    }

    fn vc_fsh(vc: &TmVc) -> Option<Fsh> {
        vc.fsh
    }

    fn admit(vc: &TmVc, meta: &TmOffer) -> Result<Admitted<FirstHeaderPointer>> {
        let pointer = match (vc.config.content(), meta.pointer) {
            (DataFieldContent::Packets, Some(raw)) => {
                FirstHeaderPointer::new(raw).ok_or(Error::out_of_range(
                    field::FIRST_HEADER_POINTER,
                    u64::from(FirstHeaderPointer::MAX),
                    u64::from(raw),
                ))?
            }
            (DataFieldContent::VcaSdu, None) => FirstHeaderPointer::ZERO,
            (DataFieldContent::Packets, None) | (DataFieldContent::VcaSdu, Some(_)) => {
                return Err(
                    Error::new(ErrorKind::InvalidField).with_field(field::FIRST_HEADER_POINTER)
                );
            }
        };
        Ok(Admitted {
            kind: CopFrameKind::Ad,
            given_count: None,
            pointer,
        })
    }

    fn check_channel(channel: &TmChannel, vcs: &[Option<VcConfig<Self>>]) -> Result<()> {
        let bad = |f| Error::new(ErrorKind::InvalidConfig).with_field(f);
        let oid = vcs
            .iter()
            .flatten()
            .find(|v| v.id() == channel.oid_vc.get())
            .ok_or(bad(field::VC_ID))?;
        if oid.params.config.content() == DataFieldContent::Packets && oid.security().is_none() {
            Ok(())
        } else {
            Err(bad(field::IDLE_POLICY))
        }
    }

    fn header_extension(_: &TmChannel, _: Option<&TmVc>, _: bool) {}

    fn primary_header(
        channel: &TmChannel,
        vc: Option<&TmVc>,
        f: &FrameFields<Self>,
    ) -> Result<TmPrimaryHeader> {
        let [.., mc] = f.mc_count.to_be_bytes();
        let [.., count] = f.vc_count.to_be_bytes();
        Ok(TmPrimaryHeader {
            scid: channel.scid,
            vcid: f.vc,
            ocf_flag: f.ocf,
            mc_frame_count: mc,
            vc_frame_count: count,
            fsh_flag: f.secondary,
            // Synchronization Flag 1 for a `VCA_SDU`; Packet Order Flag 0 and Segment Length
            // Identifier 11 (132.0 §4.1.2.7).
            sync_flag: !f.oid && vc.is_some_and(|v| v.config.content().sync_flag()),
            packet_order_flag: false,
            segment_length_id: SegmentLengthId::NO_SEGMENTS,
            first_header_pointer: f.pointer.unwrap_or(FirstHeaderPointer::ONLY_IDLE_DATA),
        })
    }
}

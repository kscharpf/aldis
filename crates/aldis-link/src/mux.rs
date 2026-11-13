//! The `ChannelMux` of fixed-length frames (LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md
//! §2.4, §4).
//!
//! It chooses the VC of each frame, counts frames, and builds data frames and OID frames, with
//! LNK-002, LNK-003, LNK-074, and LNK-075 on the shared types.
//!
//! The engine serves one physical channel with one master channel of one protocol (TM, AOS, or
//! USLP with fixed-length frames, LNK-001, OI-75). A generator does not push a data field into
//! it: the framework feeds each field returned by `Release` as an [`Offer`] (LNK-076), and the
//! mux keeps at most one offered field for each VC. `Opportunity` returns exactly one frame
//! (LNK-030): the next field of the VC the policy chooses (LNK-031), or an OID frame when no VC
//! has a field (LNK-014, D2 of the M4 plan).
//!
//! **Frames.** The mux writes the whole frame into the caller's output area and gives the ranges
//! of the Security Header, the data, and the Security Trailer, which are zero octets in the
//! frame; the caller applies the SDLS hook over them, then the FHEC (AOS) and the FECF are
//! computed by the codec writer (LNK-035, API-074). The FHEC and the FECF are zero octets here.
//! The count of a frame is assigned when the frame is selected (LNK-032); the OCF, the TM
//! Secondary Header, and the Insert Zone come from slots that the caller updates (LNK-033,
//! LNK-034).
//!
//! **Bounds (LNK-074, LNK-075, API-062).** `Opportunity` returns at most [`CHANNEL_MUX_MAX_ACTIONS`]
//! actions and writes the frame length of the configuration to the output area; every other
//! input returns at most one action and writes nothing.

use core::fmt::Debug;
use core::ops::Range;

use aldis_core::PhysicalChannel;
use aldis_core::engine::{Actions, OutRange};
use aldis_core::layout::{FrameLayout, LayoutSpec, OCF_LEN};
use aldis_core::oid::OidGenerator;
use aldis_core::security::SecurityLayout;
use aldis_core::writer::{Fixed, FrameWriter, Unsealed};
use aldis_core::{CopFrameKind, Error, ErrorKind, Result, field};
#[cfg(feature = "tm")]
use aldis_tm::FshLen;

use crate::common::{Counter, Occupancy, Qos};
use crate::policy::{MuxPolicy, Selector, Weight};
use crate::protocol::Protocol;

/// The most actions one input of the `ChannelMux` returns (LNK-074).
///
/// The worst case is `Opportunity`: it returns exactly one `Transmit`, and a `Notify` of
/// `InsertSduRepeated` when the Insert Zone repeats the slot of the frame before (LNK-034): two
/// actions, whatever the frame length, the number of VCs, or the policy, because the frame is
/// one fixed-length frame and the Insert Zone of a physical channel is one `IN_SDU` (LNK-034).
/// `Reconfigure` returns at most one `Discarded` (LNK-003); `Offer`, `SetOcf`, `SetFsh`, and
/// `SetInsert` return none (LNK-076). Docs/04-engines.md §4 gives the same bound.
pub const CHANNEL_MUX_MAX_ACTIONS: usize = 2;

/// The most data octets of a Transfer Frame Secondary Header: 63, after the Identification Field
/// in a header of at most 64 octets (132.0 §4.1.3.2.3, Table 5-2).
pub const MAX_FSH_DATA: usize = 63;

/// The VCIDs of every protocol fit 6 bits.
const VCID_SPACE: usize = 64;

/// The Data Field of a Transfer Frame Secondary Header: 1 to 63 octets after the Identification
/// Field, which the mux adds (132.0 §4.1.3.2.3; LNK-033, TM-003).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fsh {
    len: u8,
    data: [u8; MAX_FSH_DATA],
}

impl Fsh {
    /// The Secondary Header data `data`.
    ///
    /// # Errors
    /// `InvalidConfig` (`fsh_len`) unless `data` holds 1 to 63 octets.
    ///
    /// Requirements: LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
    pub fn new(data: &[u8]) -> Result<Fsh> {
        let len = u8::try_from(data.len())
            .ok()
            .filter(|&n| n >= 1 && usize::from(n) <= MAX_FSH_DATA)
            .ok_or(Error::new(ErrorKind::InvalidConfig).with_field(field::FSH_LEN))?;
        let mut buf = [0_u8; MAX_FSH_DATA];
        for (d, s) in buf.iter_mut().zip(data) {
            *d = *s;
        }
        Ok(Fsh { len, data: buf })
    }

    /// The Secondary Header of the length `len` (the Identification Field and the data) with
    /// every data octet 0: the initial value of a slot whose caller gave none.
    #[cfg(feature = "tm")]
    const fn zeroed(len: FshLen) -> Fsh {
        Fsh {
            len: len.get().saturating_sub(1),
            data: [0; MAX_FSH_DATA],
        }
    }

    /// Replaces the data with `data` of the same length.
    fn overwrite(&mut self, data: &[u8]) {
        for (d, s) in self.data.iter_mut().zip(data) {
            *d = *s;
        }
    }

    /// The number of data octets, 1 to 63.
    ///
    /// Requirements: LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
    #[must_use]
    pub fn len(&self) -> usize {
        usize::from(self.len)
    }

    /// The length of the whole Secondary Header as the codec's configuration states it: the
    /// Identification Field and the data, 2 to 64 octets (132.0 §4.1.3.1.3).
    ///
    /// Requirements: LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
    #[cfg(feature = "tm")]
    #[must_use]
    pub fn fsh_len(&self) -> Option<FshLen> {
        u8::try_from(self.total_len()).ok().and_then(FshLen::new)
    }

    /// Always `false`: the data holds at least one octet.
    ///
    /// Requirements: LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        false
    }

    /// The data octets.
    ///
    /// Requirements: LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
    pub fn octets(&self) -> impl Iterator<Item = u8> + '_ {
        self.data.iter().copied().take(self.len())
    }

    /// The length of the whole Secondary Header: the Identification Field and the data.
    fn total_len(&self) -> usize {
        self.len().saturating_add(1)
    }

    /// The Identification Field: version `00` and the length minus one (132.0 §4.1.3.2.2–3).
    const fn id_octet(&self) -> u8 {
        self.len
    }
}

/// The master channel or a VC, as the owner of a slot (LNK-033, LNK-076).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scope {
    /// The master channel (`MC_OCF`, `MC_FSH`).
    Mc,
    /// The Virtual Channel with this VCID (`VC_OCF`, `VC_FSH`).
    Vc(u8),
}

/// The block that COP-1 gives with a field (LNK-032, LNK-076; 732.1 §4.1.2.12.5, §4.2.7.4
/// notes 2–3).
///
/// One variant for each frame kind of 732.1 Table 4-1 that COP-1 sends, so the reserved flag
/// combination (Bypass 0, Protocol Control Command 1) cannot be represented (RS-020).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Cop {
    /// A Type-AD frame (Bypass 0, Protocol Control Command 0): `vcf_count` is the Frame
    /// Sequence Number, which the mux takes as given and counts nothing for.
    TypeAd {
        /// The VCF Count to put in the frame.
        vcf_count: u64,
    },
    /// A Type-BD frame (Bypass 1, Protocol Control Command 0): the mux uses and increments its
    /// own expedited VCF Count.
    TypeBd,
    /// A Type-BC frame (Bypass 1, Protocol Control Command 1): as Type-BD.
    TypeBc,
}

impl Cop {
    /// The block of the two flags and the count FOP-1 gives. With `bypass` the count is dropped:
    /// the mux uses its own expedited count (LNK-032).
    ///
    /// # Errors
    /// `InvalidField` (`protocol_control_command_flag`) for Bypass 0 with Protocol Control
    /// Command 1, the reserved combination of 732.1 Table 4-1.
    pub const fn new(vcf_count: u64, bypass: bool, protocol_control: bool) -> Result<Cop> {
        match (bypass, protocol_control) {
            (false, false) => Ok(Cop::TypeAd { vcf_count }),
            (true, false) => Ok(Cop::TypeBd),
            (true, true) => Ok(Cop::TypeBc),
            (false, true) => Err(Error::new(ErrorKind::InvalidField)
                .with_field(field::PROTOCOL_CONTROL_COMMAND_FLAG)),
        }
    }

    /// The kind of the frame, shared with the codecs and FOP-1 (OI-96).
    ///
    /// Requirements: LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
    #[must_use]
    pub const fn kind(self) -> CopFrameKind {
        match self {
            Cop::TypeAd { .. } => CopFrameKind::Ad,
            Cop::TypeBd => CopFrameKind::Bd,
            Cop::TypeBc => CopFrameKind::Bc,
        }
    }

    /// The Bypass/Sequence Control Flag.
    ///
    /// Requirements: LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
    #[must_use]
    pub const fn bypass(self) -> bool {
        self.kind().bypass_flag()
    }

    /// The Protocol Control Command Flag.
    ///
    /// Requirements: LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
    #[must_use]
    pub const fn protocol_control(self) -> bool {
        self.kind().control_command_flag()
    }

    /// The VCF Count the mux takes as given: the Frame Sequence Number of a Type-AD frame.
    ///
    /// Requirements: LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
    #[must_use]
    pub const fn vcf_count(self) -> Option<u64> {
        match self {
            Cop::TypeAd { vcf_count } => Some(vcf_count),
            Cop::TypeBd | Cop::TypeBc => None,
        }
    }
}

/// A data field offered for a VC (LNK-076): what a generator returns with its octets, and the
/// metadata of the protocol `P` (M6 plan WP-2b).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Offer<'a, P: MuxProtocol> {
    /// The VCID of the field.
    pub vc: u8,
    /// The field: the TM data field, the AOS `M_PDU`, `B_PDU`, or `VCA_SDU`, the USLP TFDF.
    pub field: &'a [u8],
    /// What the protocol's frames need besides the field: the pointer of TM, the route and the
    /// COP-1 block of USLP, nothing of AOS (`()`).
    pub meta: P::OfferMeta,
}

/// An input of the `ChannelMux` (docs/04-engines.md §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MuxInput<'a, P: MuxProtocol, const V: usize> {
    /// A data field for a VC (LNK-076).
    Offer(Offer<'a, P>),
    /// Replace the OCF slot of the MC or a VC (LNK-033).
    SetOcf {
        /// Whose slot.
        scope: Scope,
        /// The four octets of the OCF.
        ocf: [u8; OCF_LEN],
    },
    /// Replace the Secondary Header slot of the MC or a VC (TM, LNK-033).
    SetFsh {
        /// Whose slot.
        scope: Scope,
        /// The data octets, of the configured length.
        data: &'a [u8],
    },
    /// Replace the `IN_SDU` of the Insert Zone slot (AOS, USLP; LNK-034).
    SetInsert(&'a [u8]),
    /// Build the next frame (LNK-030).
    Opportunity,
    /// Replace the configuration between data units (LNK-003, GEN-061).
    Reconfigure(ChannelMuxConfig<P, V>),
}

/// Whether a frame carries data of a VC or only idle data (LNK-030).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FrameClass {
    /// A frame of the field a VC offered.
    Data,
    /// An OID frame: the data field is the next octets of the OID generator (CMN-020).
    Oid,
}

/// A frame the mux built, in the output area (LNK-030, LNK-035).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Transmit {
    /// Data or OID.
    pub class: FrameClass,
    /// The VCID of the frame.
    pub vc: u8,
    /// The whole frame; the Security Header and Trailer, the FHEC, and the FECF are zero
    /// octets (LNK-035).
    pub frame: OutRange,
    /// The Security Header, for the SDLS hook (an empty range at the right place if the VC has
    /// none).
    pub security_header: OutRange,
    /// The data field, which the hook protects.
    pub data: OutRange,
    /// The Security Trailer.
    pub security_trailer: OutRange,
}

/// An action of the `ChannelMux` (API-057).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MuxOutput {
    /// The next frame (LNK-030).
    Transmit(Transmit),
    /// The Insert Zone holds the value of the frame before: no `IN_SDU` arrived since (LNK-034,
    /// OI-68).
    InsertSduRepeated,
    /// `Reconfigure` discarded the offered fields, with this many octets (LNK-003).
    Discarded {
        /// The octets of the fields.
        octets: usize,
    },
}

/// The actions of one input of the `ChannelMux` (LNK-074).
pub type MuxActions = Actions<MuxOutput, CHANNEL_MUX_MAX_ACTIONS>;

/// The read-only status of a `ChannelMux`, for housekeeping telemetry (API-056, LNK-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MuxStatus {
    /// Frames built, OID frames included.
    pub frames_out: Counter,
    /// OID frames built.
    pub oid_frames: Counter,
    /// Fields accepted.
    pub offers: Counter,
    /// Fields refused as invalid.
    pub refused: Counter,
    /// Fields refused with `WouldBlock` (LNK-076).
    pub would_block: Counter,
    /// Frames whose Insert Zone repeated the slot (LNK-034).
    pub insert_repeated: Counter,
    /// Octets discarded by `Reconfigure` (LNK-003).
    pub discarded_octets: Counter,
    /// The offered fields (API-060).
    pub slots: Occupancy,
}

/// The per-frame values the protocol writes into the primary header (LNK-032, LNK-082).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameFields<P: MuxProtocol> {
    /// The VC of the frame: the OID VC of the protocol for an OID frame.
    pub vc: P::Vc,
    /// Whether the frame is an OID frame.
    pub oid: bool,
    /// The frame length in octets.
    pub frame_len: usize,
    /// The count of the master channel (TM), before it is masked to the field.
    pub mc_count: u64,
    /// The count of the VC, before it is masked to the field.
    pub vc_count: u64,
    /// Whether the frame has an OCF.
    pub ocf: bool,
    /// Whether the frame has a TM Secondary Header.
    pub secondary: bool,
    /// What the offered field gave the header (TM: the First Header Pointer); none of an OID
    /// frame.
    pub pointer: Option<P::Pointer>,
    /// The kind of the frame: the Bypass/Sequence Control and Protocol Control Command Flags of
    /// USLP, which also select the count; Type-AD for TM and AOS.
    pub kind: CopFrameKind,
}

/// What a protocol makes of the metadata of an offer: the kind of the frame, the VCF Count the
/// offer gave, and the pointer of the header (LNK-076, LNK-082).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Admitted<T> {
    /// The kind of the frame (Type-AD unless COP-1 or the VC's quality of service says
    /// otherwise).
    pub kind: CopFrameKind,
    /// The VCF Count the offer gives for a Type-AD frame, which the mux counts nothing for.
    pub given_count: Option<u64>,
    /// The protocol's own value for the header.
    pub pointer: T,
}

/// A frame protocol the `ChannelMux` serves: TM, AOS, or USLP with fixed-length frames
/// (LNK-001, D3 of the M4 plan).
///
/// The differences between the protocols are the parameters the associated types hold and
/// the functions below; the engine has no runtime check of the protocol. The octets of the
/// primary header and of the data field header of an OID frame come from the codec's
/// [`FrameFormat`](aldis_core::FrameFormat) (M6 plan WP-2b).
pub trait MuxProtocol: Protocol {
    /// The parameters of the physical channel and its master channel.
    type Channel: Copy + Debug + Eq;
    /// The parameters of a VC.
    type VcParams: Copy + Debug + Eq;
    /// What an [`Offer`] carries besides its field (LNK-076).
    type OfferMeta: Copy + Debug + Eq + Default;
    /// The value an offer gives the header of its frame: the First Header Pointer of TM, `()`
    /// where the header has none.
    type Pointer: Copy + Debug + Eq;

    /// Whether the physical channel has an Insert Zone (AOS, USLP; LNK-034).
    const INSERT_ZONE: bool;
    /// Whether the master channel has its own frame count (TM; LNK-032).
    const MC_COUNT: bool;
    /// Whether OID frames are expedited (USLP; LNK-030).
    const OID_BYPASS: bool;

    /// The managed parameters of the physical channel the channel parameters embed (M6 plan
    /// WP-2c).
    fn physical(channel: &Self::Channel) -> &Self::Physical;
    /// The VC of OID frames: the dedicated VC (TM) or 63 (AOS, USLP; LNK-030).
    fn oid_vc(channel: &Self::Channel) -> Self::Vc;
    /// The initial `MC_OCF`, if the master channel has one (LNK-033).
    fn mc_ocf(channel: &Self::Channel) -> Option<[u8; OCF_LEN]>;
    /// The initial `MC_FSH`, if the master channel has one (TM).
    fn mc_fsh(channel: &Self::Channel) -> Option<Fsh>;
    /// The VC the parameters are of: the identifier of the codec's VC configuration (M6 plan
    /// WP-2c).
    fn vc_id(vc: &Self::VcParams) -> Self::Vc;
    /// The lengths of the Security Header and Trailer of the VC, which the codec's VC
    /// configuration sets (LNK-035, API-070).
    fn security(vc: &Self::VcParams) -> SecurityLayout;
    /// The initial `VC_OCF`, if the VC has one (LNK-033).
    fn vc_ocf(vc: &Self::VcParams) -> Option<[u8; OCF_LEN]>;
    /// The initial `VC_FSH`, if the VC has one (TM).
    fn vc_fsh(vc: &Self::VcParams) -> Option<Fsh>;
    /// Checks the metadata of an offer against the VC and says what the frame is (LNK-076,
    /// LNK-082).
    ///
    /// # Errors
    /// `InvalidField` (`first_header_pointer`, `map_id`, `cop_in_effect`, `qos`) or
    /// `OutOfRange` (`first_header_pointer`, `vcf_count`) for a pointer, route, or COP block the
    /// VC cannot have; `UnknownChannel` (`map_id`) for a MAP the VC does not have.
    fn admit(vc: &Self::VcParams, meta: &Self::OfferMeta) -> Result<Admitted<Self::Pointer>>;
    /// The relations between the channel and its VCs (LNK-078): called with the VCs in ascending
    /// VCID order.
    ///
    /// # Errors
    /// `InvalidConfig` naming the parameter.
    fn check_channel(channel: &Self::Channel, vcs: &[Option<VcConfig<Self>>]) -> Result<()>;
    /// The managed parameter that lengthens the primary header of a frame of the VC (`None`: the
    /// OID frame of a protocol whose OID VC is not configured): the FHEC of AOS, the VCF Count
    /// Length of USLP. The header holds
    /// [`primary_header_len`](aldis_core::FrameFormat::primary_header_len) of it octets.
    fn header_extension(
        channel: &Self::Channel,
        vc: Option<&Self::VcParams>,
        bypass: bool,
    ) -> Self::HeaderExtension;
    /// The primary header of a frame, as the codec's writer takes it (LNK-032, LNK-082): the
    /// counts are masked to their fields, and an error-control field is left zero (LNK-035).
    ///
    /// # Errors
    /// `OutOfRange` (`frame_len`) for a USLP frame length above the 16-bit field, a length
    /// `check_channel` rules out: it is an error, not a clamped length.
    fn primary_header(
        channel: &Self::Channel,
        vc: Option<&Self::VcParams>,
        fields: &FrameFields<Self>,
    ) -> Result<Self::PrimaryHeader>;
}

/// The parameters of one VC of a `ChannelMux` (LNK-031, LNK-035, LNK-078): the codec's VC
/// configuration with the engine's own extras, the priority and the weight of the multiplexing
/// policy (M6 plan WP-2c).
///
/// The VC and the Security Header and Trailer lengths are those of the codec's configuration in
/// [`params`](Self::params); the mux restates neither (RS-020).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VcConfig<P: MuxProtocol> {
    priority: u8,
    weight: Weight,
    params: P::VcParams,
}

impl<P: MuxProtocol> VcConfig<P> {
    /// A VC with the parameters `params`, priority 0, and weight 1.
    ///
    /// Requirements: LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
    #[must_use]
    pub const fn new(params: P::VcParams) -> VcConfig<P> {
        VcConfig {
            priority: 0,
            weight: Weight::ONE,
            params,
        }
    }

    /// The same VC with the priority of fixed-priority multiplexing (LNK-031). A higher value is
    /// served first: [`MuxPolicy::FixedPriority`] picks the offered VC with the highest priority,
    /// the opposite of a rank in which 1 is served first (as in the "Mux priority" row of
    /// `docs/reference-application.md` §4.1). The ordering algorithm is not specified by CCSDS
    /// and is left to the project (732.0 §4.2.5.3, §4.2.6.3; 732.1 §4.2.5.3, §4.2.8.3;
    /// 232.0 §4.3.3.3, §4.3.6.3, §4.3.7.3), so this direction is Aldis's own choice; a caller that
    /// holds a rank passes it inverted (for example 255 less the rank).
    #[must_use]
    pub const fn with_priority(self, priority: u8) -> VcConfig<P> {
        VcConfig { priority, ..self }
    }

    /// The same VC with the weight of weighted round robin (LNK-031).
    #[must_use]
    pub const fn with_weight(self, weight: Weight) -> VcConfig<P> {
        VcConfig { weight, ..self }
    }

    /// The VC.
    ///
    /// Requirements: LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
    #[must_use]
    pub fn vc(&self) -> P::Vc {
        P::vc_id(&self.params)
    }

    /// The priority (LNK-031).
    #[must_use]
    pub const fn priority(&self) -> u8 {
        self.priority
    }

    /// The weight (LNK-031).
    #[must_use]
    pub const fn weight(&self) -> Weight {
        self.weight
    }

    /// The Security Header and Trailer lengths (LNK-035).
    #[must_use]
    pub fn security(&self) -> SecurityLayout {
        P::security(&self.params)
    }

    /// The protocol's parameters of the VC.
    ///
    /// Requirements: LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
    #[must_use]
    pub const fn params(&self) -> &P::VcParams {
        &self.params
    }

    fn id(&self) -> u8 {
        self.vc().into()
    }
}

/// The configuration of a `ChannelMux`: the managed parameters of one physical channel with one
/// master channel and `V` VCs at most (LNK-030…LNK-035, LNK-078).
///
/// Built only by [`new`](Self::new), which checks that every frame of every VC has a data
/// field (API-040, RS-020).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelMuxConfig<P: MuxProtocol, const V: usize> {
    channel: P::Channel,
    policy: MuxPolicy,
    idle_pattern: u8,
    vcs: [Option<VcConfig<P>>; V],
    count: usize,
}

impl<P: MuxProtocol, const V: usize> ChannelMuxConfig<P, V> {
    /// The configuration of the physical channel `channel`, whose managed parameters
    /// (the frame length, the FECF, the Insert Zone) are those of its
    /// [`Physical`](aldis_core::FrameFormat::Physical) set, with the VCs `vcs` (in any order,
    /// each VCID once). The Insert Zone, if the set has one, starts filled with the octet 0.
    ///
    /// # Errors
    /// - `CapacityExceeded` (`capacity`) for more than `V` VCs;
    /// - `InvalidConfig` (`vc_id`) for a VCID twice or the OID VCID of the protocol where it is
    ///   not dedicated; (`vc_priority`) for equal priorities under fixed priority; (`frame_len`)
    ///   for a frame that leaves no data field to some VC; (`ocf_flag`, `fsh_len`) for an MC
    ///   and a VC that both have an OCF or a Secondary Header (132.0 §2.2.4 b, c); and the
    ///   protocol's own checks (LNK-078).
    pub fn new(
        channel: P::Channel,
        policy: MuxPolicy,
        vcs: &[VcConfig<P>],
    ) -> Result<ChannelMuxConfig<P, V>> {
        if vcs.len() > V {
            return Err(Error::new(ErrorKind::CapacityExceeded).with_field(field::CAPACITY));
        }
        let mut sorted = [None; V];
        for v in vcs {
            let rank = vcs.iter().filter(|o| o.id() < v.id()).count();
            match sorted.get_mut(rank) {
                Some(slot @ None) => *slot = Some(*v),
                _ => return Err(Error::new(ErrorKind::InvalidConfig).with_field(field::VC_ID)),
            }
        }
        ChannelMuxConfig {
            channel,
            policy,
            idle_pattern: 0,
            vcs: sorted,
            count: vcs.len(),
        }
        .checked()
    }

    /// The same configuration with the Insert Zone (one `IN_SDU` in every frame, of the length
    /// the physical channel's managed parameters give) initially filled with `idle_pattern`
    /// (LNK-034).
    ///
    /// # Errors
    /// `InvalidConfig` (`insert_zone_len`) for a physical channel without an Insert Zone.
    pub fn with_insert_zone_fill(self, idle_pattern: u8) -> Result<ChannelMuxConfig<P, V>> {
        if self.insert_zone_len() == 0 {
            return Err(Error::new(ErrorKind::InvalidConfig).with_field(field::INSERT_ZONE_LEN));
        }
        Ok(ChannelMuxConfig {
            idle_pattern,
            ..self
        })
    }

    /// Checks the relations between the parameters (LNK-078).
    fn checked(self) -> Result<ChannelMuxConfig<P, V>> {
        let bad = |f| Error::new(ErrorKind::InvalidConfig).with_field(f);
        P::check_channel(&self.channel, &self.vcs)?;
        if self.policy == MuxPolicy::FixedPriority {
            for v in self.vcs() {
                if self.vcs().filter(|o| o.priority == v.priority).count() > 1 {
                    return Err(bad(field::VC_PRIORITY));
                }
            }
        }
        let mc_ocf = P::mc_ocf(&self.channel).is_some();
        let mc_fsh = P::mc_fsh(&self.channel).is_some();
        if self.vcs().any(|v| mc_ocf && P::vc_ocf(&v.params).is_some()) {
            return Err(bad(field::OCF_FLAG));
        }
        if self.vcs().any(|v| mc_fsh && P::vc_fsh(&v.params).is_some()) {
            return Err(bad(field::FSH_LEN));
        }
        for bypass in [false, true] {
            if self.frame_len() <= self.overhead(None, bypass)
                || self
                    .vcs()
                    .any(|v| self.frame_len() <= self.overhead(Some(v), bypass))
            {
                return Err(bad(field::FRAME_LEN));
            }
        }
        if self.data_len(self.oid_vc().map(|(_, v)| v), P::OID_BYPASS) <= P::SDU_HEADER_LEN {
            return Err(bad(field::FRAME_LEN));
        }
        Ok(self)
    }

    /// The VCs in ascending VCID order.
    fn vcs(&self) -> impl Iterator<Item = &VcConfig<P>> {
        self.vcs.iter().flatten()
    }

    /// The VC with the VCID `id` and its position.
    fn find(&self, id: u8) -> Option<(usize, &VcConfig<P>)> {
        self.vcs().enumerate().find(|(_, v)| v.id() == id)
    }

    /// The frame an OID frame is built as: the dedicated VC (TM) or none (AOS, USLP).
    fn oid_vc(&self) -> Option<(usize, &VcConfig<P>)> {
        self.find(P::oid_vc(&self.channel).into())
    }

    /// The Transfer Frame Secondary Header of a frame of `vc`: its own, else the MC's (TM).
    fn fsh(&self, vc: Option<&VcConfig<P>>) -> Option<Fsh> {
        vc.and_then(|v| P::vc_fsh(&v.params))
            .or_else(|| P::mc_fsh(&self.channel))
    }

    /// Whether a frame of `vc` has a Transfer Frame Secondary Header.
    fn has_fsh(&self, vc: Option<&VcConfig<P>>) -> bool {
        self.fsh(vc).is_some()
    }

    /// The octets of the Transfer Frame Secondary Header or Insert Zone of a frame of `vc`.
    fn secondary_len(&self, vc: Option<&VcConfig<P>>) -> usize {
        self.fsh(vc)
            .map_or(self.insert_zone_len(), |f| f.total_len())
    }

    /// Whether a frame of `vc` has an OCF (LNK-033).
    fn has_ocf(&self, vc: Option<&VcConfig<P>>) -> bool {
        vc.is_some_and(|v| P::vc_ocf(&v.params).is_some()) || P::mc_ocf(&self.channel).is_some()
    }

    /// The layout of the fields of a frame of `vc`, which leaves the data field open.
    fn spec(&self, vc: Option<&VcConfig<P>>, bypass: bool) -> LayoutSpec {
        LayoutSpec {
            frame_len: self.frame_len(),
            primary_header_len: P::primary_header_len(&P::header_extension(
                &self.channel,
                vc.map(|v| &v.params),
                bypass,
            )),
            secondary_len: self.secondary_len(vc),
            security: vc.map_or(SecurityLayout::NONE, VcConfig::security),
            ocf: self.has_ocf(vc),
            fecf: self.fecf(),
        }
    }

    /// The octets of a frame of `vc` other than its data field.
    fn overhead(&self, vc: Option<&VcConfig<P>>, bypass: bool) -> usize {
        self.spec(vc, bypass).overhead()
    }

    /// The octets of the data field of a frame of `vc` for the flag `bypass`.
    fn data_len(&self, vc: Option<&VcConfig<P>>, bypass: bool) -> usize {
        self.frame_len().saturating_sub(self.overhead(vc, bypass))
    }

    /// The octets of the data field a VC offers for the quality of service `qos`: the length
    /// of the field of every [`Offer`] for it (LNK-076), or `None` for a VCID not configured.
    #[must_use]
    pub fn field_len(&self, vc: u8, qos: Qos) -> Option<usize> {
        let (_, v) = self.find(vc)?;
        Some(self.data_len(Some(v), qos == Qos::Expedited))
    }

    /// The physical channel's parameters.
    ///
    /// Requirements: LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
    #[must_use]
    pub const fn channel(&self) -> &P::Channel {
        &self.channel
    }

    /// The frame length in octets.
    ///
    /// Requirements: LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
    #[must_use]
    pub fn frame_len(&self) -> usize {
        P::physical(&self.channel).frame_len()
    }

    /// Whether frames have a FECF.
    ///
    /// Requirements: LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
    #[must_use]
    pub fn fecf(&self) -> bool {
        P::physical(&self.channel).fecf()
    }

    /// The multiplexing policy (LNK-031).
    #[must_use]
    pub const fn policy(&self) -> MuxPolicy {
        self.policy
    }

    /// The octets of the Insert Zone, 0 if none (LNK-034).
    #[must_use]
    pub fn insert_zone_len(&self) -> usize {
        P::physical(&self.channel).insert_zone_len()
    }

    /// The number of VCs.
    ///
    /// Requirements: LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
    #[must_use]
    pub const fn vc_count(&self) -> usize {
        self.count
    }
}

/// The counts of one VC: one for each `QoS` (LNK-032).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct VcCounts {
    sequence_controlled: u64,
    expedited: u64,
}

impl VcCounts {
    const fn get(&self, bypass: bool) -> u64 {
        if bypass {
            self.expedited
        } else {
            self.sequence_controlled
        }
    }

    const fn bump(&mut self, bypass: bool) {
        let slot = if bypass {
            &mut self.expedited
        } else {
            &mut self.sequence_controlled
        };
        *slot = slot.wrapping_add(1);
    }
}

/// What the mux keeps of an offered field besides its octets (LNK-076).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Held<T> {
    len: usize,
    kind: CopFrameKind,
    pointer: T,
    given_count: Option<u64>,
}

/// The `ChannelMux` of one physical channel (LNK-030…LNK-036).
///
/// `V` is the capacity of VCs and `F` the capacity of a frame in octets (GEN-011); a
/// configuration that needs more is refused with `CapacityExceeded`. The field slots hold
/// `V × F` octets.
///
/// The two fields are public so that a caller can build the engine in place in its own storage,
/// without an engine on the stack: all-zero buffers are the empty state (CABI-040, GEN-012).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelMux<P: MuxProtocol, const V: usize, const F: usize> {
    /// Everything but the octet buffers.
    pub head: ChannelMuxHead<P, V>,
    /// The octet buffers, valid when all zero.
    pub bufs: ChannelMuxBuffers<V, F>,
}

/// Everything of a [`ChannelMux`] but its octet buffers.
///
/// Requirements: CABI-040 (in-place construction, M4 plan WP-7), LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelMuxHead<P: MuxProtocol, const V: usize> {
    config: ChannelMuxConfig<P, V>,
    held: [Option<Held<P::Pointer>>; V],
    vc_ocf: [Option<[u8; OCF_LEN]>; V],
    vc_fsh: [Option<Fsh>; V],
    mc_ocf: Option<[u8; OCF_LEN]>,
    mc_fsh: Option<Fsh>,
    insert_fresh: bool,
    mc_count: u64,
    selector: Selector,
    oid: OidGenerator,
    status: MuxStatus,
}

/// The buffers of a [`ChannelMux`]: the field slots, the Insert Zone slot, and the frame counts.
///
/// All zero is the empty state of the slots and the counts. The Insert Zone slot starts at the
/// configured idle pattern (LNK-034): [`with_idle`](Self::with_idle) builds the buffers that
/// way. An in-place build zeroes the buffers and then calls [`ChannelMux::init_slots`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelMuxBuffers<const V: usize, const F: usize> {
    slots: [[u8; F]; V],
    insert: [u8; F],
    /// The frame counts of the VCs, 0 at the start: they are plain integers, and all zero is
    /// their empty state, so they live here and keep the head small (CABI-040, #240).
    counts: [VcCounts; VCID_SPACE],
}

impl<const V: usize, const F: usize> ChannelMuxBuffers<V, F> {
    /// Empty slots, and the Insert Zone slot filled with the idle pattern `idle` (LNK-034).
    #[must_use]
    pub const fn with_idle(idle: u8) -> Self {
        Self {
            slots: [[0; F]; V],
            insert: [idle; F],
            counts: [VcCounts {
                sequence_controlled: 0,
                expedited: 0,
            }; VCID_SPACE],
        }
    }
}

impl<P: MuxProtocol, const V: usize> ChannelMuxHead<P, V> {
    /// Sets the OCF and Secondary Header slots to their configured values.
    fn init_slots(&mut self) {
        self.mc_ocf = P::mc_ocf(&self.config.channel);
        self.mc_fsh = P::mc_fsh(&self.config.channel);
        for ((v, ocf), fsh) in self
            .config
            .vcs
            .iter()
            .zip(self.vc_ocf.iter_mut())
            .zip(self.vc_fsh.iter_mut())
        {
            let params = v.as_ref().map(|v| v.params);
            *ocf = params.and_then(|p| P::vc_ocf(&p));
            *fsh = params.and_then(|p| P::vc_fsh(&p));
        }
        self.insert_fresh = false;
    }
}

impl<P: MuxProtocol, const V: usize, const F: usize> ChannelMux<P, V, F> {
    /// A mux with no field offered, the counts at 0, and the OID generator at its start
    /// (LNK-032, LNK-036).
    ///
    /// # Errors
    /// `CapacityExceeded` (`frame_len`) if the frame is longer than `F` octets.
    pub fn new(config: ChannelMuxConfig<P, V>) -> Result<ChannelMux<P, V, F>> {
        let idle = config.idle_pattern;
        let head = Self::new_head(config)?;
        Ok(ChannelMux {
            head,
            bufs: ChannelMuxBuffers::with_idle(idle),
        })
    }

    /// The head of a mux with no field offered: the part of [`Self::new`] that is built from
    /// the configuration.
    ///
    /// # Errors
    /// As [`Self::new`].
    ///
    /// Requirements: CABI-040 (in-place construction, M4 plan WP-7), LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
    pub fn new_head(config: ChannelMuxConfig<P, V>) -> Result<ChannelMuxHead<P, V>> {
        ChannelMux::<P, V, F>::check_capacity(&config)?;
        let mut head = ChannelMuxHead {
            config,
            held: [None; V],
            vc_ocf: [None; V],
            vc_fsh: [None; V],
            mc_ocf: None,
            mc_fsh: None,
            insert_fresh: false,
            mc_count: 0,
            selector: Selector::new(),
            oid: OidGenerator::new(),
            status: MuxStatus::default(),
        };
        head.init_slots();
        Ok(head)
    }

    /// Puts every slot at its configured value (LNK-033, LNK-034): the head's OCF and Secondary
    /// Header slots, and the Insert Zone slot at the idle pattern. An in-place build calls it on the
    /// engine it has zeroed and given its head (CABI-040).
    ///
    /// Requirements: LNK-033, LNK-034, CABI-040; docs/04-engines.md §2.4.
    pub fn init_slots(&mut self) {
        self.head.init_slots();
        self.bufs.insert.fill(self.head.config.idle_pattern);
    }

    fn check_capacity(config: &ChannelMuxConfig<P, V>) -> Result<()> {
        if config.frame_len() > F {
            Err(Error::new(ErrorKind::CapacityExceeded).with_field(field::FRAME_LEN))
        } else {
            Ok(())
        }
    }

    /// The configuration in force.
    ///
    /// Requirements: LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
    #[must_use]
    pub const fn config(&self) -> &ChannelMuxConfig<P, V> {
        &self.head.config
    }

    /// The status snapshot: counters and occupancy (API-056, LNK-002).
    #[must_use]
    pub const fn status(&self) -> &MuxStatus {
        &self.head.status
    }

    /// Counts a refused offer that an adapter could not form as an [`Offer`] of this protocol
    /// (the C ABI's flat input carries a route that a TM offer has no member for) and returns
    /// `error`, so that `refused` counts what the engine would have refused (LNK-002, LNK-076).
    #[must_use]
    pub const fn refuse(&mut self, error: Error) -> Error {
        self.head.status.refused.incr();
        error
    }

    /// Runs one input (API-057).
    ///
    /// An input that fails leaves the slots, the counts, the policy, and the OID generator as
    /// they were and writes no action (API-058); the housekeeping counters of the status count
    /// a refusal.
    ///
    /// # Errors
    /// - `Offer`: `UnknownChannel` for a VCID not configured, (`map_id`) for a MAP
    ///   the VC does not have; `InvalidField` (`first_header_pointer`, `map_id`,
    ///   `cop_in_effect`, `qos`) for a pointer, route, or COP
    ///   block the VC cannot have; `OutOfRange` (`vcf_count`); `LengthMismatch` (`field_len`);
    ///   `WouldBlock` when the VC's slot is occupied (LNK-076).
    /// - `SetOcf`, `SetFsh`: `UnknownChannel`; `InvalidField` (`ocf_flag`,
    ///   `fsh_flag`) for a slot the configuration lacks; `LengthMismatch` (`fsh_len`).
    /// - `SetInsert`: `InvalidField` (`insert_zone_len`) without an Insert Zone, `LengthMismatch`
    ///   (`insert_zone_len`).
    /// - `Opportunity`: `BufferTooSmall` if `out` is shorter than the frame (LNK-075).
    /// - `Reconfigure`: `CapacityExceeded` as for [`new`](Self::new).
    pub fn step(&mut self, input: MuxInput<'_, P, V>, out: &mut [u8]) -> Result<MuxActions> {
        let mut actions = MuxActions::new();
        match input {
            MuxInput::Offer(offer) => self.offer(&offer)?,
            MuxInput::SetOcf { scope, ocf } => self.set_ocf(scope, ocf)?,
            MuxInput::SetFsh { scope, data } => self.set_fsh(scope, data)?,
            MuxInput::SetInsert(data) => self.set_insert(data)?,
            MuxInput::Opportunity => self.opportunity(out, &mut actions)?,
            MuxInput::Reconfigure(config) => {
                ChannelMux::<P, V, F>::check_capacity(&config)?;
                let octets = self
                    .head
                    .held
                    .iter()
                    .flatten()
                    .fold(0_usize, |n, h| n.saturating_add(h.len));
                self.head.config = config;
                self.head.held = [None; V];
                self.head.selector = Selector::new();
                self.init_slots();
                self.head.status.discarded_octets.add(octets);
                self.head.status.slots.set(0);
                if octets > 0 {
                    actions.push(MuxOutput::Discarded { octets })?;
                }
            }
        }
        Ok(actions)
    }

    /// `Offer` (LNK-076, LNK-082).
    fn offer(&mut self, offer: &Offer<'_, P>) -> Result<()> {
        match self.try_offer(offer) {
            Ok(()) => {
                self.head.status.offers.incr();
                Ok(())
            }
            Err(e) => {
                if e.kind() == ErrorKind::WouldBlock {
                    self.head.status.would_block.incr();
                } else {
                    self.head.status.refused.incr();
                }
                Err(e)
            }
        }
    }

    fn try_offer(&mut self, offer: &Offer<'_, P>) -> Result<()> {
        let (pos, vc) = self
            .head
            .config
            .find(offer.vc)
            .ok_or(Error::new(ErrorKind::UnknownChannel))?;
        let admitted = P::admit(&vc.params, &offer.meta)?;
        if offer.field.len()
            != self
                .head
                .config
                .data_len(Some(vc), admitted.kind.bypass_flag())
        {
            return Err(Error::new(ErrorKind::LengthMismatch).with_field(field::FIELD_LEN));
        }
        let Some((slot, held)) = self
            .bufs
            .slots
            .iter_mut()
            .zip(self.head.held.iter_mut())
            .nth(pos)
        else {
            return Err(Error::new(ErrorKind::InternalError));
        };
        if held.is_some() {
            return Err(Error::new(ErrorKind::WouldBlock));
        }
        for (d, s) in slot.iter_mut().zip(offer.field) {
            *d = *s;
        }
        *held = Some(Held {
            len: offer.field.len(),
            kind: admitted.kind,
            pointer: admitted.pointer,
            given_count: admitted.given_count,
        });
        self.refresh_occupancy();
        Ok(())
    }

    fn refresh_occupancy(&mut self) {
        let octets = self
            .head
            .held
            .iter()
            .flatten()
            .fold(0_usize, |n, h| n.saturating_add(h.len));
        self.head.status.slots.set(octets);
    }

    /// The position of the VC of `scope` (`None` for the MC).
    fn scope_pos(&self, scope: Scope) -> Result<Option<usize>> {
        match scope {
            Scope::Mc => Ok(None),
            Scope::Vc(id) => self
                .head
                .config
                .find(id)
                .map(|(pos, _)| Some(pos))
                .ok_or(Error::new(ErrorKind::UnknownChannel)),
        }
    }

    /// `SetOcf` (LNK-033).
    fn set_ocf(&mut self, scope: Scope, ocf: [u8; OCF_LEN]) -> Result<()> {
        let slot = match self.scope_pos(scope)? {
            None => Some(&mut self.head.mc_ocf),
            Some(pos) => self.head.vc_ocf.get_mut(pos),
        };
        match slot {
            Some(slot @ Some(_)) => {
                *slot = Some(ocf);
                Ok(())
            }
            _ => Err(Error::new(ErrorKind::InvalidField).with_field(field::OCF_FLAG)),
        }
    }

    /// `SetFsh` (LNK-033).
    fn set_fsh(&mut self, scope: Scope, data: &[u8]) -> Result<()> {
        let slot = match self.scope_pos(scope)? {
            None => Some(&mut self.head.mc_fsh),
            Some(pos) => self.head.vc_fsh.get_mut(pos),
        };
        match slot {
            Some(Some(old)) if old.len() == data.len() => {
                old.overwrite(data);
                Ok(())
            }
            Some(Some(_)) => Err(Error::new(ErrorKind::LengthMismatch).with_field(field::FSH_LEN)),
            _ => Err(Error::new(ErrorKind::InvalidField).with_field(field::FSH_FLAG)),
        }
    }

    /// `SetInsert` (LNK-034).
    fn set_insert(&mut self, data: &[u8]) -> Result<()> {
        let len = self.head.config.insert_zone_len();
        if len == 0 {
            return Err(Error::new(ErrorKind::InvalidField).with_field(field::INSERT_ZONE_LEN));
        }
        if data.len() != len {
            return Err(Error::new(ErrorKind::LengthMismatch).with_field(field::INSERT_ZONE_LEN));
        }
        for (d, s) in self.bufs.insert.iter_mut().zip(data) {
            *d = *s;
        }
        self.head.insert_fresh = true;
        Ok(())
    }

    /// `Opportunity` (LNK-030…LNK-036).
    fn opportunity(&mut self, out: &mut [u8], actions: &mut MuxActions) -> Result<()> {
        let frame_len = self.head.config.frame_len();
        if out.len() < frame_len {
            return Err(Error::buffer_too_small(frame_len));
        }
        let (selector, pick) = self.pick()?;
        let fields = self.fields(&pick);
        let layout = FrameLayout::compute(
            &self
                .head
                .config
                .spec(pick.vc.as_ref(), fields.kind.bypass_flag()),
        )?;
        // The frame is zero but for the fields the mux writes: the Security Header and Trailer, the
        // FHEC, and the FECF stay zero for the caller (LNK-035).
        let mut writer = FrameWriter::reserve_fixed(out, layout.clone(), Unsealed)?;
        self.write_frame(&mut writer, &pick, &fields)?;
        let written = writer.leave_unsealed().len();
        // The frame is built: commit the state (API-058).
        self.commit(selector, &pick, &fields);
        actions.push(MuxOutput::Transmit(Transmit {
            class: if fields.oid {
                FrameClass::Oid
            } else {
                FrameClass::Data
            },
            vc: fields.vc.into(),
            frame: OutRange::new(0, written),
            security_header: out_range(&layout.security_header()),
            data: out_range(&layout.data()),
            security_trailer: out_range(&layout.security_trailer()),
        }))?;
        if self.head.config.insert_zone_len() > 0 {
            if !self.head.insert_fresh {
                self.head.status.insert_repeated.incr();
                actions.push(MuxOutput::InsertSduRepeated)?;
            }
            self.head.insert_fresh = false;
        }
        Ok(())
    }

    /// Chooses the VC of the next frame by the policy, on a copy of the policy state (LNK-031).
    fn pick(&self) -> Result<(Selector, Pick<P>)> {
        let cfg = &self.head.config;
        let held = &self.head.held;
        let mut selector = self.head.selector;
        let selected = selector.select(
            cfg.policy,
            cfg.count,
            |i| held.get(i).is_some_and(Option::is_some),
            |i| cfg.vcs().nth(i).map_or(0, VcConfig::priority),
            |i| cfg.vcs().nth(i).map_or(Weight::ONE, VcConfig::weight),
        );
        let pick = if let Some(p) = selected {
            let (Some(vc), Some(h)) = (cfg.vcs().nth(p), held.get(p).copied().flatten()) else {
                return Err(Error::new(ErrorKind::InternalError));
            };
            Pick {
                selected,
                vc_pos: selected,
                vc: Some(*vc),
                held: Some(h),
            }
        } else {
            selector.forfeit();
            let oid = cfg.oid_vc();
            Pick {
                selected,
                vc_pos: oid.map(|(p, _)| p),
                vc: oid.map(|(_, v)| *v),
                held: None,
            }
        };
        Ok((selector, pick))
    }

    /// The values of the primary header: the counts are those of the frame selected (LNK-032).
    fn fields(&self, pick: &Pick<P>) -> FrameFields<P> {
        let cfg = &self.head.config;
        let vc = pick
            .vc
            .as_ref()
            .map_or_else(|| P::oid_vc(&cfg.channel), VcConfig::vc);
        let kind = pick.held.map_or(
            if P::OID_BYPASS {
                CopFrameKind::Bd
            } else {
                CopFrameKind::Ad
            },
            |h| h.kind,
        );
        let counted = self
            .bufs
            .counts
            .get(usize::from(vc.into()))
            .map_or(0, |c| c.get(kind.bypass_flag()));
        FrameFields {
            vc,
            oid: pick.held.is_none(),
            frame_len: cfg.frame_len(),
            mc_count: self.head.mc_count,
            vc_count: pick.held.and_then(|h| h.given_count).unwrap_or(counted),
            ocf: cfg.has_ocf(pick.vc.as_ref()),
            secondary: cfg.has_fsh(pick.vc.as_ref()),
            pointer: pick.held.map(|h| h.pointer),
            kind,
        }
    }

    /// Takes the offered field, counts the frame, and keeps the policy state (LNK-032).
    fn commit(&mut self, selector: Selector, pick: &Pick<P>, fields: &FrameFields<P>) {
        self.head.selector = selector;
        if let Some(slot) = pick.selected.and_then(|p| self.head.held.get_mut(p)) {
            *slot = None;
        }
        let given = pick.held.is_some_and(|h| h.given_count.is_some());
        if !given && let Some(c) = self.bufs.counts.get_mut(usize::from(fields.vc.into())) {
            c.bump(fields.kind.bypass_flag());
        }
        if P::MC_COUNT {
            self.head.mc_count = self.head.mc_count.wrapping_add(1);
        }
        self.refresh_occupancy();
        self.head.status.frames_out.incr();
        if fields.oid {
            self.head.status.oid_frames.incr();
        }
    }

    /// Writes the fields of a frame through the core frame writer, all but the Security Header and
    /// Trailer, the FHEC, and the FECF, which stay zero (LNK-035).
    fn write_frame(
        &mut self,
        writer: &mut FrameWriter<'_, Fixed, Unsealed>,
        pick: &Pick<P>,
        fields: &FrameFields<P>,
    ) -> Result<()> {
        P::write_primary_header(
            &P::primary_header(
                &self.head.config.channel,
                pick.vc.as_ref().map(|v| &v.params),
                fields,
            )?,
            writer.primary_header_mut(),
        )?;
        let fsh = pick
            .vc_pos
            .and_then(|p| self.head.vc_fsh.get(p).copied().flatten())
            .or(self.head.mc_fsh);
        let secondary = writer.secondary_mut();
        if let Some(fsh) = fsh {
            let (id, rest) = secondary
                .split_first_mut()
                .ok_or(Error::new(ErrorKind::InternalError))?;
            *id = fsh.id_octet();
            for (d, s) in rest.iter_mut().zip(fsh.octets()) {
                *d = s;
            }
        } else {
            for (d, s) in secondary.iter_mut().zip(self.bufs.insert.iter()) {
                *d = *s;
            }
        }
        let data_len = writer.layout().data().len();
        let data = writer.data_field_mut();
        if fields.oid {
            let (head, zone) = data
                .split_at_mut_checked(P::SDU_HEADER_LEN)
                .ok_or(Error::new(ErrorKind::InternalError))?;
            P::write_oid_data_header(data_len, head);
            self.head.oid.fill(zone);
        } else {
            let slot = pick
                .vc_pos
                .and_then(|p| self.bufs.slots.get(p))
                .ok_or(Error::new(ErrorKind::InternalError))?;
            for (d, s) in data.iter_mut().zip(slot) {
                *d = *s;
            }
        }
        let ocf = pick
            .vc_pos
            .and_then(|p| self.head.vc_ocf.get(p).copied().flatten())
            .or(self.head.mc_ocf);
        if let Some(ocf) = ocf {
            for (d, s) in writer.ocf_mut().iter_mut().zip(ocf) {
                *d = s;
            }
        }
        Ok(())
    }
}

/// The frame an `Opportunity` builds: the VC whose field it carries, or the OID frame.
#[derive(Debug, Clone, Copy)]
struct Pick<P: MuxProtocol> {
    /// The position of the VC whose field is sent; none for an OID frame.
    selected: Option<usize>,
    /// The position of the VC whose parameters the frame has (TM: the dedicated VC of an OID
    /// frame).
    vc_pos: Option<usize>,
    vc: Option<VcConfig<P>>,
    held: Option<Held<P::Pointer>>,
}

/// A range of the frame as an action range.
fn out_range(r: &Range<usize>) -> OutRange {
    OutRange::new(r.start, r.len())
}

#[cfg(feature = "tm")]
mod tm_impl {
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
        ) -> TmChannel {
            TmChannel {
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
        pub const fn new(config: TmVirtualChannelConfig) -> TmVc {
            TmVc {
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
        pub const fn with_initial_ocf(self, ocf: [u8; OCF_LEN]) -> Result<TmVc> {
            if self.config.ocf() {
                Ok(TmVc { ocf, ..self })
            } else {
                Err(Error::new(ErrorKind::InvalidConfig).with_field(field::OCF_FLAG))
            }
        }

        /// The same VC with the initial `VC_FSH` `fsh`.
        ///
        /// # Errors
        /// `InvalidConfig` (`fsh_len`) unless the configuration has a `VC_FSH` of the length of
        /// `fsh`.
        pub fn with_initial_fsh(self, fsh: Fsh) -> Result<TmVc> {
            if self.config.fsh().map(FshLen::octets) == Some(fsh.total_len()) {
                Ok(TmVc {
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

        fn check_channel(channel: &TmChannel, vcs: &[Option<VcConfig<Tm>>]) -> Result<()> {
            let bad = |f| Error::new(ErrorKind::InvalidConfig).with_field(f);
            let oid = vcs
                .iter()
                .flatten()
                .find(|v| v.id() == channel.oid_vc.get())
                .ok_or(bad(field::VC_ID))?;
            if oid.params.config.content() == DataFieldContent::Packets && oid.security().is_none()
            {
                Ok(())
            } else {
                Err(bad(field::IDLE_POLICY))
            }
        }

        fn header_extension(_: &TmChannel, _: Option<&TmVc>, _: bool) {}

        fn primary_header(
            channel: &TmChannel,
            vc: Option<&TmVc>,
            f: &FrameFields<Tm>,
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
}

#[cfg(feature = "tm")]
pub use tm_impl::{TmChannel, TmOffer, TmVc};

#[cfg(feature = "aos")]
mod aos_impl {
    use aldis_aos::{
        AosEdition, AosPhysicalChannelConfig, AosPrimaryHeader, AosVirtualChannelConfig,
        VcFrameCount,
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
        ) -> Result<AosChannel> {
            if physical.edition() == AosEdition::Issue4Compatible && scid.get() > 0xFF {
                Err(Error::new(ErrorKind::InvalidConfig).with_field(field::SPACECRAFT_ID))
            } else {
                Ok(AosChannel {
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
        pub const fn new(config: AosVirtualChannelConfig) -> AosVc {
            AosVc {
                config,
                ocf: [0; OCF_LEN],
            }
        }

        /// The same VC with the initial `VC_OCF` `ocf`.
        ///
        /// # Errors
        /// `InvalidConfig` (`ocf_flag`) if the configuration has no `VC_OCF`.
        pub const fn with_initial_ocf(self, ocf: [u8; OCF_LEN]) -> Result<AosVc> {
            if self.config.ocf() {
                Ok(AosVc { ocf, ..self })
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
        fn check_channel(channel: &AosChannel, vcs: &[Option<VcConfig<Aos>>]) -> Result<()> {
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
            f: &FrameFields<Aos>,
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
}

#[cfg(feature = "aos")]
pub use aos_impl::{AosChannel, AosVc};

#[cfg(feature = "uslp")]
mod uslp_impl {
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
                return Err(
                    Error::new(ErrorKind::InvalidConfig).with_field(field::TRANSFER_FRAME_TYPE)
                );
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
        pub const fn new(
            config: UslpVcConfig,
            map: MapId<4>,
            qos: Qos,
            cop: bool,
        ) -> Result<UslpVc> {
            if let Err(e) = UslpVc::check_count_len(cop, config.vcf_count_len(false)) {
                return Err(e);
            }
            if !matches!(config.frame_type(), TransferFrameType::Fixed) {
                return Err(
                    Error::new(ErrorKind::InvalidConfig).with_field(field::TRANSFER_FRAME_TYPE)
                );
            }
            let SecurityScope::Vc(security) = config.security() else {
                return Err(
                    Error::new(ErrorKind::InvalidConfig).with_field(field::SECURITY_HEADER_LEN)
                );
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

        fn header_extension(
            channel: &UslpChannel,
            vc: Option<&UslpVc>,
            bypass: bool,
        ) -> VcfCountLen {
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
}

#[cfg(feature = "uslp")]
pub use uslp_impl::{UslpChannel, UslpOffer, UslpVc};

#[cfg(all(test, feature = "aos"))]
#[cfg_attr(coverage_nightly, coverage(off))]
#[expect(
    clippy::unwrap_used,
    reason = "test helpers fail loudly by design (RS-018)"
)]
mod tests {
    use aldis_aos::{
        AosEdition, AosPhysicalChannelConfig, AosVirtualChannelConfig, DataFieldContent,
    };
    use aldis_core::ids::{SpacecraftId, VcId};

    use super::{
        AosChannel, AosVc, ChannelMux, ChannelMuxConfig, MuxInput, Offer, VcConfig, VcCounts,
    };
    use crate::policy::MuxPolicy;
    use crate::protocol::Aos;

    fn mux(cycle_in_use: bool) -> ChannelMux<Aos, 2, 32> {
        let physical =
            AosPhysicalChannelConfig::new(28, false, 0, true, AosEdition::Issue5).unwrap();
        let channel = AosChannel::new(
            physical,
            SpacecraftId::<10>::new(421).unwrap(),
            cycle_in_use,
        )
        .unwrap();
        let config = AosVirtualChannelConfig::new(
            SpacecraftId::<10>::new(421).unwrap(),
            VcId::<6>::new(1).unwrap(),
            DataFieldContent::VcaSdu,
        )
        .unwrap();
        let vc = VcConfig::<Aos>::new(AosVc::new(config));
        ChannelMux::new(ChannelMuxConfig::new(channel, MuxPolicy::RoundRobin, &[vc]).unwrap())
            .unwrap()
    }

    /// The VC Frame Count and the signaling octet of the next frame of VC 1.
    fn next(mux: &mut ChannelMux<Aos, 2, 32>) -> (u32, u8) {
        let mut out = [0_u8; 32];
        let field = [0_u8; 20];
        let _ = mux
            .step(
                MuxInput::Offer(Offer {
                    vc: 1,
                    field: &field,
                    meta: (),
                }),
                &mut out,
            )
            .unwrap();
        let _ = mux.step(MuxInput::Opportunity, &mut out).unwrap();
        (u32::from_be_bytes([0, out[2], out[3], out[4]]), out[5])
    }

    #[test]
    fn the_aos_vc_frame_count_wraps_at_24_bits() {
        // req: LNK-032, OI-74
        // The mux has no start count (OI-74), so the test sets the counter next to the wrap.
        let mut m = mux(false);
        m.bufs.counts[1] = VcCounts {
            sequence_controlled: 0x00FF_FFFE,
            expedited: 0,
        };
        let got = [(); 4].map(|()| next(&mut m));
        // The count wraps to 0 with the cycle unused: Use Flag 0 and cycle 0.
        assert_eq!(
            got,
            [
                (0x00FF_FFFE, 0x10),
                (0x00FF_FFFF, 0x10),
                (0, 0x10),
                (1, 0x10)
            ]
        );
    }

    #[test]
    fn the_aos_vc_frame_count_cycle_counts_the_wraps() {
        // req: LNK-032, OI-74
        let mut m = mux(true);
        m.bufs.counts[1] = VcCounts {
            sequence_controlled: 0x00FF_FFFF,
            expedited: 0,
        };
        let got = [(); 3].map(|()| next(&mut m));
        // The Use Flag is set (0x40), the spare bits hold the SCID's top bits (0x10), and the
        // cycle counts the wrap.
        assert_eq!(got, [(0x00FF_FFFF, 0x50), (0, 0x51), (1, 0x51)]);
        // The cycle is 4 bits: it wraps after 16 cycles.
        m.bufs.counts[1] = VcCounts {
            sequence_controlled: (0xF_u64 << 24) | 0x00FF_FFFF,
            expedited: 0,
        };
        let got = [(); 2].map(|()| next(&mut m));
        assert_eq!(got, [(0x00FF_FFFF, 0x5F), (0, 0x50)]);
    }

    /// Whatever the count a VC has reached, the next frames carry its low 24 bits, and with the
    /// cycle in use its next 4 bits, which wrap after 16 cycles (732.0 §4.1.2.4, §4.1.2.5.5).
    /// The sweep takes every start next to each wrap of the 24 bits and of the cycle, and runs
    /// frames across it, in both modes (LNK-032).
    #[test]
    #[expect(
        clippy::excessive_nesting,
        reason = "an exhaustive sweep over independent parameters: each loop is one dimension"
    )]
    fn the_vc_frame_count_and_cycle_follow_the_number_of_frames() {
        // req: LNK-032
        for cycle_in_use in [false, true] {
            for k in 1_u64..=16 {
                for off in 0_u64..6 {
                    for frames in 1_u64..6 {
                        let start = (k << 24) - off;
                        let mut m = mux(cycle_in_use);
                        m.bufs.counts[1] = VcCounts {
                            sequence_controlled: start,
                            expedited: 0,
                        };
                        for i in 0..frames {
                            let total = start + i;
                            let (count, signaling) = next(&mut m);
                            assert_eq!(u64::from(count), total & 0x00FF_FFFF);
                            let cycle = if cycle_in_use {
                                0x40 | u8::try_from((total >> 24) & 0xF).unwrap()
                            } else {
                                0
                            };
                            assert_eq!(signaling, 0x10 | cycle);
                        }
                    }
                }
            }
        }
    }
}

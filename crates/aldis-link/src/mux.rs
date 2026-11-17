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
    pub fn new(data: &[u8]) -> Result<Self> {
        let len = u8::try_from(data.len())
            .ok()
            .filter(|&n| n >= 1 && usize::from(n) <= MAX_FSH_DATA)
            .ok_or(Error::new(ErrorKind::InvalidConfig).with_field(field::FSH_LEN))?;
        let mut buf = [0_u8; MAX_FSH_DATA];
        for (d, s) in buf.iter_mut().zip(data) {
            *d = *s;
        }
        Ok(Self { len, data: buf })
    }

    /// The Secondary Header of the length `len` (the Identification Field and the data) with
    /// every data octet 0: the initial value of a slot whose caller gave none.
    #[cfg(feature = "tm")]
    const fn zeroed(len: FshLen) -> Self {
        Self {
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
    pub const fn new(vcf_count: u64, bypass: bool, protocol_control: bool) -> Result<Self> {
        match (bypass, protocol_control) {
            (false, false) => Ok(Self::TypeAd { vcf_count }),
            (true, false) => Ok(Self::TypeBd),
            (true, true) => Ok(Self::TypeBc),
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
            Self::TypeAd { .. } => CopFrameKind::Ad,
            Self::TypeBd => CopFrameKind::Bd,
            Self::TypeBc => CopFrameKind::Bc,
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
            Self::TypeAd { vcf_count } => Some(vcf_count),
            Self::TypeBd | Self::TypeBc => None,
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
    pub const fn new(params: P::VcParams) -> Self {
        Self {
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
    pub const fn with_priority(self, priority: u8) -> Self {
        Self { priority, ..self }
    }

    /// The same VC with the weight of weighted round robin (LNK-031).
    #[must_use]
    pub const fn with_weight(self, weight: Weight) -> Self {
        Self { weight, ..self }
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
    pub fn new(channel: P::Channel, policy: MuxPolicy, vcs: &[VcConfig<P>]) -> Result<Self> {
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
        Self {
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
    pub fn with_insert_zone_fill(self, idle_pattern: u8) -> Result<Self> {
        if self.insert_zone_len() == 0 {
            return Err(Error::new(ErrorKind::InvalidConfig).with_field(field::INSERT_ZONE_LEN));
        }
        Ok(Self {
            idle_pattern,
            ..self
        })
    }

    /// Checks the relations between the parameters (LNK-078).
    fn checked(self) -> Result<Self> {
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
/// A caller builds the engine in place in its own storage, without an engine on the stack, from
/// [`new_head`](Self::new_head) and all-zero buffers, the empty state (CABI-040, GEN-012).
/// [`HEAD_OFFSET`](Self::HEAD_OFFSET) and [`BUFFERS_OFFSET`](Self::BUFFERS_OFFSET) are the offsets
/// of the two fields.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelMux<P: MuxProtocol, const V: usize, const F: usize> {
    head: ChannelMuxHead<P, V>,
    bufs: ChannelMuxBuffers<V, F>,
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
/// way. An in-place build zeroes the buffers and then calls [`ChannelMux::place_slots`].
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
    pub fn new(config: ChannelMuxConfig<P, V>) -> Result<Self> {
        let idle = config.idle_pattern;
        let head = Self::new_head(config)?;
        Ok(Self {
            head,
            bufs: ChannelMuxBuffers::with_idle(idle),
        })
    }

    /// The offset of the head field, for the in-place build of the C ABI (CABI-040). Not part of
    /// the API.
    #[doc(hidden)]
    pub const HEAD_OFFSET: usize = core::mem::offset_of!(Self, head);
    /// The offset of the buffers field, for the in-place build of the C ABI (CABI-040). Not part
    /// of the API.
    #[doc(hidden)]
    pub const BUFFERS_OFFSET: usize = core::mem::offset_of!(Self, bufs);

    /// The head of a mux with no field offered: the part of [`Self::new`] that is built from
    /// the configuration.
    ///
    /// # Errors
    /// As [`Self::new`].
    ///
    /// Requirements: CABI-040 (in-place construction, M4 plan WP-7), LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
    pub fn new_head(config: ChannelMuxConfig<P, V>) -> Result<ChannelMuxHead<P, V>> {
        Self::check_capacity(&config)?;
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
    /// Header slots, and the Insert Zone slot at the idle pattern.
    fn init_slots(&mut self) {
        self.head.init_slots();
        self.bufs.insert.fill(self.head.config.idle_pattern);
    }

    /// [`init_slots`](Self::init_slots) for in-place construction only (CABI-040): `aldis-ffi`
    /// calls it on the engine it has zeroed and given its head, whose buffers are private.
    /// Calling it on a running mux resets the caller's OCF slots and the Insert Zone to their
    /// configured values.
    ///
    /// Requirements: LNK-033, LNK-034, CABI-040; docs/04-engines.md §2.4.
    #[doc(hidden)]
    pub fn place_slots(&mut self) {
        self.init_slots();
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
                Self::check_capacity(&config)?;
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
mod tm;

#[cfg(feature = "tm")]
pub use tm::{TmChannel, TmOffer, TmVc};

#[cfg(feature = "aos")]
mod aos;

#[cfg(feature = "aos")]
pub use aos::{AosChannel, AosVc};

#[cfg(feature = "uslp")]
mod uslp;

#[cfg(feature = "uslp")]
pub use uslp::{UslpChannel, UslpOffer, UslpVc};

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

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

use aldis_core::engine::{Actions, OutRange};
use aldis_core::layout::{FrameLayout, LayoutSpec, OCF_LEN};
use aldis_core::oid::OidGenerator;
use aldis_core::security::SecurityLayout;
use aldis_core::{Error, ErrorKind, Result, field};

use crate::common::{Counter, Occupancy, Qos};
use crate::policy::{MuxPolicy, Selector, Weight};
use crate::protocol::{Protocol, Route};

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

    /// The Bypass/Sequence Control Flag.
    ///
    /// Requirements: LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
    #[must_use]
    pub const fn bypass(self) -> bool {
        !matches!(self, Cop::TypeAd { .. })
    }

    /// The Protocol Control Command Flag.
    ///
    /// Requirements: LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
    #[must_use]
    pub const fn protocol_control(self) -> bool {
        matches!(self, Cop::TypeBc)
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

/// A data field offered for a VC (LNK-076): what a generator returns with its octets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Offer<'a> {
    /// The VCID of the field.
    pub vc: u8,
    /// The field: the TM data field, the AOS `M_PDU`, `B_PDU`, or `VCA_SDU`, the USLP TFDF.
    pub field: &'a [u8],
    /// The raw First Header Pointer that TM carries in the frame header: required on a TM VC
    /// of Packets, refused on a TM VC of `VCA_SDUs`, and ignored by AOS and USLP, which carry
    /// the pointer in the field.
    pub pointer: Option<u16>,
    /// The MAP and `QoS` of the field (USLP): the MAP must be the VC's; without a route the VC's
    /// `QoS` applies. TM and AOS have none.
    pub route: Option<Route>,
    /// The COP-1 block, for a USLP VC with COP-1 in effect.
    pub cop: Option<Cop>,
}

/// An input of the `ChannelMux` (docs/04-engines.md §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MuxInput<'a, P: MuxProtocol, const V: usize> {
    /// A data field for a VC (LNK-076).
    Offer(Offer<'a>),
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
#[expect(
    clippy::struct_excessive_bools,
    reason = "plain values for the header writers: each is one flag of a header, 132.0 §4.1.2.4, §4.1.2.7; 732.1 §4.1.2.8"
)]
pub struct FrameFields {
    /// The VCID of the frame.
    pub vc: u8,
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
    /// The First Header Pointer of a TM frame of Packets or `VCA_SDUs`; unused by an OID frame
    /// and by the other protocols.
    pub pointer: u16,
    /// The Bypass/Sequence Control Flag (USLP); also the `QoS` that selects the count.
    pub bypass: bool,
    /// The Protocol Control Command Flag (USLP).
    pub protocol_control: bool,
}

/// A frame protocol the `ChannelMux` serves: TM, AOS, or USLP with fixed-length frames
/// (LNK-001, D3 of the M4 plan).
///
/// The differences between the protocols are the parameters the two associated types hold and
/// the functions below; the engine has no runtime check of the protocol.
pub trait MuxProtocol: Protocol {
    /// The parameters of the physical channel and its master channel.
    type Channel: Copy + Debug + Eq;
    /// The parameters of a VC.
    type VcParams: Copy + Debug + Eq;

    /// Whether the physical channel has an Insert Zone (AOS, USLP; LNK-034).
    const INSERT_ZONE: bool;
    /// Whether the master channel has its own frame count (TM; LNK-032).
    const MC_COUNT: bool;
    /// Whether OID frames are expedited (USLP; LNK-030).
    const OID_BYPASS: bool;

    /// The VCID of OID frames: the dedicated VC (TM) or 63 (AOS, USLP; LNK-030).
    fn oid_vcid(channel: &Self::Channel) -> u8;
    /// The initial `MC_OCF`, if the master channel has one (LNK-033).
    fn mc_ocf(channel: &Self::Channel) -> Option<[u8; OCF_LEN]>;
    /// The initial `MC_FSH`, if the master channel has one (TM).
    fn mc_fsh(channel: &Self::Channel) -> Option<Fsh>;
    /// The initial `VC_OCF`, if the VC has one (LNK-033).
    fn vc_ocf(vc: &Self::VcParams) -> Option<[u8; OCF_LEN]>;
    /// The initial `VC_FSH`, if the VC has one (TM).
    fn vc_fsh(vc: &Self::VcParams) -> Option<Fsh>;
    /// The MAP and `QoS` of a VC (USLP); none on the others.
    fn vc_route(vc: &Self::VcParams) -> Option<Route>;
    /// Whether COP-1 is in effect on the VC (USLP, LNK-076).
    fn vc_cop(vc: &Self::VcParams) -> bool;
    /// The largest VCF Count the VC's count field holds for the flag `bypass` (USLP).
    fn vcf_max(vc: &Self::VcParams, bypass: bool) -> u64;
    /// The First Header Pointer to put in the header for the offered `pointer` (LNK-076,
    /// LNK-082); 0 where the header has none.
    ///
    /// # Errors
    /// `InvalidField` or `OutOfRange` (`first_header_pointer`) for a pointer the VC cannot
    /// have.
    fn check_pointer(vc: &Self::VcParams, pointer: Option<u16>) -> Result<u16>;
    /// The relations between the channel and its VCs (LNK-078): called with the VCs in ascending
    /// VCID order.
    ///
    /// # Errors
    /// `InvalidConfig` naming the parameter.
    fn check_channel(
        channel: &Self::Channel,
        frame_len: usize,
        vcs: &[Option<VcConfig<Self>>],
    ) -> Result<()>;
    /// The octets of the primary header of a frame of the VC (`None`: the OID frame of a
    /// protocol whose OID VC is not configured), the FHEC and the VCF Count included.
    fn header_len(channel: &Self::Channel, vc: Option<&Self::VcParams>, bypass: bool) -> usize;
    /// Writes the primary header into `header`, which holds [`header_len`](Self::header_len)
    /// octets (LNK-032, LNK-082).
    ///
    /// # Errors
    /// `InternalError` for a frame length the header cannot hold; `check_channel` rules it out.
    fn write_header(
        channel: &Self::Channel,
        vc: Option<&Self::VcParams>,
        fields: &FrameFields,
        header: &mut [u8],
    ) -> Result<()>;
    /// The octets of an OID frame's data field that the protocol sets before the OID generator
    /// output: the TFDF Header of USLP (LNK-030).
    const OID_DATA_HEADER: usize;
    /// Writes the data header of an OID frame into `header`, `OID_DATA_HEADER` octets, for a
    /// data field of `data_len` octets (LNK-030).
    fn write_oid_data_header(data_len: usize, header: &mut [u8]);
}

/// The parameters of one VC of a `ChannelMux` (LNK-031, LNK-035, LNK-078).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VcConfig<P: MuxProtocol> {
    vc: P::Vc,
    priority: u8,
    weight: Weight,
    security: SecurityLayout,
    params: P::VcParams,
}

impl<P: MuxProtocol> VcConfig<P> {
    /// A VC with priority 0, weight 1, and no Security Header or Trailer.
    ///
    /// Requirements: LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
    #[must_use]
    pub const fn new(vc: P::Vc, params: P::VcParams) -> VcConfig<P> {
        VcConfig {
            vc,
            priority: 0,
            weight: Weight::ONE,
            security: SecurityLayout::NONE,
            params,
        }
    }

    /// The same VC with the priority of fixed-priority multiplexing (LNK-031).
    #[must_use]
    pub const fn with_priority(self, priority: u8) -> VcConfig<P> {
        VcConfig { priority, ..self }
    }

    /// The same VC with the weight of weighted round robin (LNK-031).
    #[must_use]
    pub const fn with_weight(self, weight: Weight) -> VcConfig<P> {
        VcConfig { weight, ..self }
    }

    /// The same VC with the lengths of its Security Header and Trailer (LNK-035, API-071).
    #[must_use]
    pub const fn with_security(self, security: SecurityLayout) -> VcConfig<P> {
        VcConfig { security, ..self }
    }

    /// The VC.
    ///
    /// Requirements: LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
    #[must_use]
    pub const fn vc(&self) -> P::Vc {
        self.vc
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
    pub const fn security(&self) -> SecurityLayout {
        self.security
    }

    /// The protocol's parameters of the VC.
    ///
    /// Requirements: LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
    #[must_use]
    pub const fn params(&self) -> &P::VcParams {
        &self.params
    }

    fn id(&self) -> u8 {
        self.vc.into()
    }
}

/// The configuration of a `ChannelMux`: the managed parameters of one physical channel with one
/// master channel and `V` VCs at most (LNK-030…LNK-035, LNK-078).
///
/// Built only by [`new`](Self::new) and [`with_insert_zone`](Self::with_insert_zone), which
/// check that every frame of every VC has a data field (API-040, RS-020).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelMuxConfig<P: MuxProtocol, const V: usize> {
    channel: P::Channel,
    frame_len: usize,
    fecf: bool,
    policy: MuxPolicy,
    insert_len: usize,
    idle_pattern: u8,
    vcs: [Option<VcConfig<P>>; V],
    count: usize,
}

impl<P: MuxProtocol, const V: usize> ChannelMuxConfig<P, V> {
    /// The configuration of a physical channel of frames of `frame_len` octets, with a FECF if
    /// `fecf`, the VCs `vcs` (in any order, each VCID once), and no Insert Zone.
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
        frame_len: usize,
        fecf: bool,
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
            frame_len,
            fecf,
            policy,
            insert_len: 0,
            idle_pattern: 0,
            vcs: sorted,
            count: vcs.len(),
        }
        .checked()
    }

    /// The same configuration with an Insert Zone of one `IN_SDU` of `len` octets in every frame,
    /// initially filled with `idle_pattern` (LNK-034).
    ///
    /// # Errors
    /// `InvalidConfig` (`insert_zone_len`) for 0 or a protocol without an Insert Zone, and
    /// (`frame_len`) when some frame has no data field left.
    pub fn with_insert_zone(self, len: usize, idle_pattern: u8) -> Result<ChannelMuxConfig<P, V>> {
        if len == 0 || !P::INSERT_ZONE {
            return Err(Error::new(ErrorKind::InvalidConfig).with_field(field::INSERT_ZONE_LEN));
        }
        ChannelMuxConfig {
            insert_len: len,
            idle_pattern,
            ..self
        }
        .checked()
    }

    /// Checks the relations between the parameters (LNK-078).
    fn checked(self) -> Result<ChannelMuxConfig<P, V>> {
        let bad = |f| Error::new(ErrorKind::InvalidConfig).with_field(f);
        P::check_channel(&self.channel, self.frame_len, &self.vcs)?;
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
            if self.frame_len <= self.overhead(None, bypass)
                || self
                    .vcs()
                    .any(|v| self.frame_len <= self.overhead(Some(v), bypass))
            {
                return Err(bad(field::FRAME_LEN));
            }
        }
        if self.data_len(self.oid_vc().map(|(_, v)| v), P::OID_BYPASS) <= P::OID_DATA_HEADER {
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
        self.find(P::oid_vcid(&self.channel))
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
        self.fsh(vc).map_or(self.insert_len, |f| f.total_len())
    }

    /// Whether a frame of `vc` has an OCF (LNK-033).
    fn has_ocf(&self, vc: Option<&VcConfig<P>>) -> bool {
        vc.is_some_and(|v| P::vc_ocf(&v.params).is_some()) || P::mc_ocf(&self.channel).is_some()
    }

    /// The layout of the fields of a frame of `vc`, which leaves the data field open.
    fn spec(&self, vc: Option<&VcConfig<P>>, bypass: bool) -> LayoutSpec {
        LayoutSpec {
            frame_len: self.frame_len,
            primary_header_len: P::header_len(&self.channel, vc.map(|v| &v.params), bypass),
            secondary_len: self.secondary_len(vc),
            security: vc.map_or(SecurityLayout::NONE, |v| v.security),
            ocf: self.has_ocf(vc),
            fecf: self.fecf,
        }
    }

    /// The octets of a frame of `vc` other than its data field.
    fn overhead(&self, vc: Option<&VcConfig<P>>, bypass: bool) -> usize {
        self.spec(vc, bypass).overhead()
    }

    /// The octets of the data field of a frame of `vc` for the flag `bypass`.
    fn data_len(&self, vc: Option<&VcConfig<P>>, bypass: bool) -> usize {
        self.frame_len.saturating_sub(self.overhead(vc, bypass))
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
    pub const fn frame_len(&self) -> usize {
        self.frame_len
    }

    /// Whether frames have a FECF.
    ///
    /// Requirements: LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
    #[must_use]
    pub const fn fecf(&self) -> bool {
        self.fecf
    }

    /// The multiplexing policy (LNK-031).
    #[must_use]
    pub const fn policy(&self) -> MuxPolicy {
        self.policy
    }

    /// The octets of the Insert Zone, 0 if none (LNK-034).
    #[must_use]
    pub const fn insert_zone_len(&self) -> usize {
        self.insert_len
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
struct Held {
    len: usize,
    bypass: bool,
    protocol_control: bool,
    pointer: u16,
    given_count: Option<u64>,
}

/// The `ChannelMux` of one physical channel (LNK-030…LNK-036).
///
/// `V` is the capacity of VCs and `F` the capacity of a frame in octets (GEN-011); a
/// configuration that needs more is refused with `CapacityExceeded`. The field slots hold
/// `V × F` octets.
///
/// The engine is its [`ChannelMuxHead`] followed by its [`ChannelMuxBuffers`] (`repr(C)`), so
/// that a caller can build it in place (see [`crate::inplace`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[repr(C)]
pub struct ChannelMux<P: MuxProtocol, const V: usize, const F: usize> {
    head: ChannelMuxHead<P, V>,
    bufs: ChannelMuxBuffers<V, F>,
}

/// Everything of a [`ChannelMux`] but its octet buffers (see [`crate::inplace`]).
///
/// Requirements: CABI-040 (in-place construction, M4 plan WP-7), LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelMuxHead<P: MuxProtocol, const V: usize> {
    config: ChannelMuxConfig<P, V>,
    held: [Option<Held>; V],
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
/// way, and an in-place build writes that pattern at [`INSERT_OFFSET`](Self::INSERT_OFFSET)
/// (see [`crate::inplace`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[repr(C)]
pub struct ChannelMuxBuffers<const V: usize, const F: usize> {
    slots: [[u8; F]; V],
    insert: [u8; F],
    /// The frame counts of the VCs, 0 at the start: they are plain integers, and all zero is
    /// their empty state, so they live here and keep the head small (CABI-040, #240).
    counts: [VcCounts; VCID_SPACE],
}

impl<const V: usize, const F: usize> ChannelMuxBuffers<V, F> {
    /// The offset of the Insert Zone slot in the buffers, for an in-place build (see
    /// [`crate::inplace`]).
    ///
    /// Requirements: CABI-040 (in-place construction, M4 plan WP-7), LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
    pub const INSERT_OFFSET: usize = core::mem::offset_of!(Self, insert);

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

    /// The offset of the head in the engine, for an in-place build (see [`crate::inplace`]).
    ///
    /// Requirements: CABI-040 (in-place construction, M4 plan WP-7), LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
    pub const HEAD_OFFSET: usize = core::mem::offset_of!(Self, head);
    /// The offset of the buffers in the engine, for an in-place build (see [`crate::inplace`]).
    ///
    /// Requirements: CABI-040 (in-place construction, M4 plan WP-7), LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
    pub const BUFFERS_OFFSET: usize = core::mem::offset_of!(Self, bufs);

    /// The head of a mux with no field offered: the part of [`Self::new`] that is built from
    /// the configuration (see [`crate::inplace`]).
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
    /// Header slots, and the Insert Zone slot at the idle pattern.
    fn init_slots(&mut self) {
        self.head.init_slots();
        self.bufs.insert.fill(self.head.config.idle_pattern);
    }

    const fn check_capacity(config: &ChannelMuxConfig<P, V>) -> Result<()> {
        if config.frame_len > F {
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
    fn offer(&mut self, offer: &Offer<'_>) -> Result<()> {
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

    fn try_offer(&mut self, offer: &Offer<'_>) -> Result<()> {
        let invalid = |f| Error::new(ErrorKind::InvalidField).with_field(f);
        let (pos, vc) = self
            .head
            .config
            .find(offer.vc)
            .ok_or(Error::new(ErrorKind::UnknownChannel))?;
        let default = P::vc_route(&vc.params);
        let route_qos = match (offer.route, default) {
            (Some(r), Some(d)) if r.map == d.map => Some(r.qos),
            (Some(_), Some(_)) => {
                return Err(Error::new(ErrorKind::UnknownChannel).with_field(field::MAP_ID));
            }
            (Some(_), None) => return Err(invalid(field::MAP_ID)),
            (None, d) => d.map(|d| d.qos),
        };
        let (bypass, protocol_control, given_count) = match offer.cop {
            None => (route_qos == Some(Qos::Expedited), false, None),
            Some(_) if !P::vc_cop(&vc.params) => return Err(invalid(field::COP_IN_EFFECT)),
            Some(c)
                if offer.route.is_some() && (route_qos == Some(Qos::Expedited)) != c.bypass() =>
            {
                return Err(invalid(field::QOS));
            }
            Some(Cop::TypeAd { vcf_count }) if vcf_count > P::vcf_max(&vc.params, false) => {
                return Err(Error::out_of_range(
                    field::VCF_COUNT,
                    P::vcf_max(&vc.params, false),
                    vcf_count,
                ));
            }
            Some(c) => (c.bypass(), c.protocol_control(), c.vcf_count()),
        };
        let pointer = P::check_pointer(&vc.params, offer.pointer)?;
        if offer.field.len() != self.head.config.data_len(Some(vc), bypass) {
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
            bypass,
            protocol_control,
            pointer,
            given_count,
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
        let len = self.head.config.insert_len;
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
        let frame_len = self.head.config.frame_len;
        if out.len() < frame_len {
            return Err(Error::buffer_too_small(frame_len));
        }
        let (selector, pick) = self.pick()?;
        let fields = self.fields(&pick);
        let layout = FrameLayout::compute(&self.head.config.spec(pick.vc.as_ref(), fields.bypass))?;
        let frame = out
            .get_mut(..frame_len)
            .ok_or(Error::new(ErrorKind::InternalError))?;
        frame.fill(0);
        self.write_frame(frame, &layout, &pick, &fields)?;
        // The frame is built: commit the state (API-058).
        self.commit(selector, &pick, &fields);
        actions.push(MuxOutput::Transmit(Transmit {
            class: if fields.oid {
                FrameClass::Oid
            } else {
                FrameClass::Data
            },
            vc: fields.vc,
            frame: OutRange::new(0, frame_len),
            security_header: out_range(&layout.security_header()),
            data: out_range(&layout.data()),
            security_trailer: out_range(&layout.security_trailer()),
        }))?;
        if self.head.config.insert_len > 0 {
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
    fn fields(&self, pick: &Pick<P>) -> FrameFields {
        let cfg = &self.head.config;
        let id = pick
            .vc
            .as_ref()
            .map_or_else(|| P::oid_vcid(&cfg.channel), VcConfig::id);
        let bypass = pick.held.map_or(P::OID_BYPASS, |h| h.bypass);
        let counted = self
            .bufs
            .counts
            .get(usize::from(id))
            .map_or(0, |c| c.get(bypass));
        FrameFields {
            vc: id,
            oid: pick.held.is_none(),
            frame_len: cfg.frame_len,
            mc_count: self.head.mc_count,
            vc_count: pick.held.and_then(|h| h.given_count).unwrap_or(counted),
            ocf: cfg.has_ocf(pick.vc.as_ref()),
            secondary: cfg.has_fsh(pick.vc.as_ref()),
            pointer: pick.held.map_or(0, |h| h.pointer),
            bypass,
            protocol_control: pick.held.is_some_and(|h| h.protocol_control),
        }
    }

    /// Takes the offered field, counts the frame, and keeps the policy state (LNK-032).
    fn commit(&mut self, selector: Selector, pick: &Pick<P>, fields: &FrameFields) {
        self.head.selector = selector;
        if let Some(slot) = pick.selected.and_then(|p| self.head.held.get_mut(p)) {
            *slot = None;
        }
        let given = pick.held.is_some_and(|h| h.given_count.is_some());
        if !given && let Some(c) = self.bufs.counts.get_mut(usize::from(fields.vc)) {
            c.bump(fields.bypass);
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

    /// Writes the fields of a frame, all but the Security Header and Trailer, the FHEC, and the
    /// FECF (LNK-035).
    fn write_frame(
        &mut self,
        frame: &mut [u8],
        layout: &FrameLayout,
        pick: &Pick<P>,
        fields: &FrameFields,
    ) -> Result<()> {
        let header = frame
            .get_mut(layout.primary_header())
            .ok_or(Error::new(ErrorKind::InternalError))?;
        P::write_header(
            &self.head.config.channel,
            pick.vc.as_ref().map(|v| &v.params),
            fields,
            header,
        )?;
        let secondary = frame
            .get_mut(layout.secondary())
            .ok_or(Error::new(ErrorKind::InternalError))?;
        let fsh = pick
            .vc_pos
            .and_then(|p| self.head.vc_fsh.get(p).copied().flatten())
            .or(self.head.mc_fsh);
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
        let data = frame
            .get_mut(layout.data())
            .ok_or(Error::new(ErrorKind::InternalError))?;
        if fields.oid {
            let (head, zone) = data
                .split_at_mut_checked(P::OID_DATA_HEADER)
                .ok_or(Error::new(ErrorKind::InternalError))?;
            P::write_oid_data_header(layout.data().len(), head);
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
            let dst = frame
                .get_mut(layout.ocf())
                .ok_or(Error::new(ErrorKind::InternalError))?;
            for (d, s) in dst.iter_mut().zip(ocf) {
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
    held: Option<Held>,
}

/// A range of the frame as an action range.
fn out_range(r: &Range<usize>) -> OutRange {
    OutRange::new(r.start, r.len())
}

#[cfg(feature = "tm")]
mod tm_impl {
    use aldis_core::ids::{SpacecraftId, VcId};
    use aldis_core::layout::OCF_LEN;
    use aldis_core::{Error, ErrorKind, Result, field};
    use aldis_tm::FirstHeaderPointer;
    use aldis_tm::config::TmPhysicalChannelConfig;

    use super::{FrameFields, Fsh, MuxProtocol, VcConfig};
    use crate::protocol::{Route, Tm};

    /// What a TM VC carries, which sets the data field status of its frames (LNK-082).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub enum TmRole {
        /// Packets: Synchronization Flag 0, Segment Length Identifier `11`, and the First Header
        /// Pointer of the field (132.0 §4.1.2.7).
        Packets,
        /// `VCA_SDUs`: Synchronization Flag 1, Packet Order Flag 0, Segment Length Identifier
        /// `11`, First Header Pointer 0 (LNK-082).
        Vca,
    }

    /// The parameters of the physical channel and master channel of a TM mux (132.0 §5.1,
    /// §5.2; LNK-030, LNK-033).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct TmChannel {
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
        /// A channel with no `MC_OCF` and no `MC_FSH`.
        ///
        /// Requirements: LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
        #[must_use]
        pub const fn new(scid: SpacecraftId<10>, oid_vc: VcId<3>) -> TmChannel {
            TmChannel {
                scid,
                oid_vc,
                mc_ocf: None,
                mc_fsh: None,
            }
        }
    }

    /// The parameters of a TM VC (132.0 §5.4 Table 5-3; LNK-033, LNK-082).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct TmVc {
        /// What the VC carries.
        pub role: TmRole,
        /// The initial `VC_OCF`: every frame of the VC has an OCF.
        pub ocf: Option<[u8; OCF_LEN]>,
        /// The initial `VC_FSH`: every frame of the VC has a Secondary Header.
        pub fsh: Option<Fsh>,
    }

    impl TmVc {
        /// A VC with no `VC_OCF` and no `VC_FSH`.
        ///
        /// Requirements: LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
        #[must_use]
        pub const fn new(role: TmRole) -> TmVc {
            TmVc {
                role,
                ocf: None,
                fsh: None,
            }
        }
    }

    /// `1 << bit` if `set`, else 0.
    const fn flag(set: bool, bit: u32) -> u8 {
        if set { 1 << bit } else { 0 }
    }

    impl MuxProtocol for Tm {
        type Channel = TmChannel;
        type VcParams = TmVc;
        const INSERT_ZONE: bool = false;
        const MC_COUNT: bool = true;
        const OID_BYPASS: bool = false;
        const OID_DATA_HEADER: usize = 0;

        fn oid_vcid(channel: &TmChannel) -> u8 {
            channel.oid_vc.get()
        }

        fn mc_ocf(channel: &TmChannel) -> Option<[u8; OCF_LEN]> {
            channel.mc_ocf
        }

        fn mc_fsh(channel: &TmChannel) -> Option<Fsh> {
            channel.mc_fsh
        }

        fn vc_ocf(vc: &TmVc) -> Option<[u8; OCF_LEN]> {
            vc.ocf
        }

        fn vc_fsh(vc: &TmVc) -> Option<Fsh> {
            vc.fsh
        }

        fn vc_route(_: &TmVc) -> Option<Route> {
            None
        }

        fn vc_cop(_: &TmVc) -> bool {
            false
        }

        fn vcf_max(_: &TmVc, _: bool) -> u64 {
            0
        }

        fn check_pointer(vc: &TmVc, pointer: Option<u16>) -> Result<u16> {
            match (vc.role, pointer) {
                (TmRole::Packets, Some(raw)) => FirstHeaderPointer::new(raw)
                    .map(FirstHeaderPointer::get)
                    .ok_or(Error::out_of_range(
                        field::FIRST_HEADER_POINTER,
                        u64::from(FirstHeaderPointer::MAX),
                        u64::from(raw),
                    )),
                (TmRole::Vca, None) => Ok(0),
                (TmRole::Packets, None) | (TmRole::Vca, Some(_)) => {
                    Err(Error::new(ErrorKind::InvalidField).with_field(field::FIRST_HEADER_POINTER))
                }
            }
        }

        fn check_channel(
            channel: &TmChannel,
            frame_len: usize,
            vcs: &[Option<VcConfig<Tm>>],
        ) -> Result<()> {
            let bad = |f| Error::new(ErrorKind::InvalidConfig).with_field(f);
            // The physical channel checks of the codec: the longest frame for which the CRC-16
            // detects every double-bit error (TM-001, OI-51, HZ-TM-04).
            TmPhysicalChannelConfig::new(frame_len, false)?;
            let oid = vcs
                .iter()
                .flatten()
                .find(|v| v.id() == channel.oid_vc.get())
                .ok_or(bad(field::VC_ID))?;
            if oid.params.role == TmRole::Packets && oid.security.is_none() {
                Ok(())
            } else {
                Err(bad(field::IDLE_POLICY))
            }
        }

        fn header_len(_: &TmChannel, _: Option<&TmVc>, _: bool) -> usize {
            aldis_tm::header::PRIMARY_HEADER_LEN
        }

        fn write_header(
            channel: &TmChannel,
            vc: Option<&TmVc>,
            f: &FrameFields,
            header: &mut [u8],
        ) -> Result<()> {
            let [s_hi, s_lo] = channel.scid.get().to_be_bytes();
            let vca = !f.oid && vc.is_some_and(|v| v.role == TmRole::Vca);
            let pointer = if f.oid {
                FirstHeaderPointer::ONLY_IDLE_DATA.get()
            } else {
                f.pointer
            };
            let [p_hi, p_lo] = pointer.to_be_bytes();
            let [.., mc] = f.mc_count.to_be_bytes();
            let [.., count] = f.vc_count.to_be_bytes();
            let octets = [
                ((s_hi & 0b11) << 4) | (s_lo >> 4),
                (s_lo << 4) | ((f.vc & 0b111) << 1) | flag(f.ocf, 0),
                mc,
                count,
                // Packet Order Flag 0, Segment Length Identifier 11 (132.0 §4.1.2.7).
                flag(f.secondary, 7) | flag(vca, 6) | (0b11 << 3) | (p_hi & 0b111),
                p_lo,
            ];
            for (d, s) in header.iter_mut().zip(octets) {
                *d = s;
            }
            Ok(())
        }

        fn write_oid_data_header(_: usize, _: &mut [u8]) {}
    }
}

#[cfg(feature = "tm")]
pub use tm_impl::{TmChannel, TmRole, TmVc};

#[cfg(feature = "aos")]
mod aos_impl {
    use aldis_aos::AosEdition;
    use aldis_core::ids::SpacecraftId;
    use aldis_core::layout::OCF_LEN;
    use aldis_core::{Error, ErrorKind, Result, field};

    use super::{FrameFields, Fsh, MuxProtocol, VcConfig};
    use crate::protocol::{Aos, Route};

    /// The parameters of the physical channel of an AOS mux (732.0 §5.2 Table 5-1; LNK-030,
    /// LNK-032, LNK-035).
    ///
    /// Built only by [`new`](Self::new), which relates the edition to the SCID (RS-020).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct AosChannel {
        scid: SpacecraftId<10>,
        edition: AosEdition,
        fhec: bool,
        cycle_in_use: bool,
    }

    impl AosChannel {
        /// A channel of the Spacecraft Identifier `scid` in the `edition` of the standard, with
        /// a Frame Header Error Control field if `fhec` (which the caller computes, LNK-035),
        /// and the VC Frame Count Cycle in use if `cycle_in_use`: the Use Flag is set and the
        /// cycle counts the wraps of the VC Frame Count (LNK-032; 732.0 §4.1.2.5.5).
        ///
        /// # Errors
        /// `InvalidConfig` (`spacecraft_id`) for a SCID above 255 in the Issue 4 edition, whose
        /// SCID has 8 bits (732.0 §4.1.2.2.3 and its note 1; AOS-002).
        pub fn new(
            scid: SpacecraftId<10>,
            edition: AosEdition,
            fhec: bool,
            cycle_in_use: bool,
        ) -> Result<AosChannel> {
            if edition == AosEdition::Issue4Compatible && scid.get() > 0xFF {
                Err(Error::new(ErrorKind::InvalidConfig).with_field(field::SPACECRAFT_ID))
            } else {
                Ok(AosChannel {
                    scid,
                    edition,
                    fhec,
                    cycle_in_use,
                })
            }
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
            self.edition
        }

        /// Whether frames have a Frame Header Error Control field.
        ///
        /// Requirements: LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
        #[must_use]
        pub const fn fhec(&self) -> bool {
            self.fhec
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
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct AosVc {
        /// The initial `VC_OCF`: every frame of the VC has an OCF (the OID frame has none).
        pub ocf: Option<[u8; OCF_LEN]>,
    }

    /// `1 << bit` if `set`, else 0.
    const fn flag(set: bool, bit: u32) -> u8 {
        if set { 1 << bit } else { 0 }
    }

    impl MuxProtocol for Aos {
        type Channel = AosChannel;
        type VcParams = AosVc;
        const INSERT_ZONE: bool = true;
        const MC_COUNT: bool = false;
        const OID_BYPASS: bool = false;
        const OID_DATA_HEADER: usize = 0;

        fn oid_vcid(_: &AosChannel) -> u8 {
            aldis_aos::OID_VCID.get()
        }

        fn mc_ocf(_: &AosChannel) -> Option<[u8; OCF_LEN]> {
            None
        }

        fn mc_fsh(_: &AosChannel) -> Option<Fsh> {
            None
        }

        fn vc_ocf(vc: &AosVc) -> Option<[u8; OCF_LEN]> {
            vc.ocf
        }

        fn vc_fsh(_: &AosVc) -> Option<Fsh> {
            None
        }

        fn vc_route(_: &AosVc) -> Option<Route> {
            None
        }

        fn vc_cop(_: &AosVc) -> bool {
            false
        }

        fn vcf_max(_: &AosVc, _: bool) -> u64 {
            0
        }

        fn check_pointer(_: &AosVc, _: Option<u16>) -> Result<u16> {
            Ok(0)
        }

        fn check_channel(_: &AosChannel, _: usize, vcs: &[Option<VcConfig<Aos>>]) -> Result<()> {
            if vcs
                .iter()
                .flatten()
                .any(|v| v.id() == aldis_aos::OID_VCID.get())
            {
                Err(Error::new(ErrorKind::InvalidConfig).with_field(field::VC_ID))
            } else {
                Ok(())
            }
        }

        fn header_len(channel: &AosChannel, _: Option<&AosVc>, _: bool) -> usize {
            if channel.fhec {
                aldis_aos::header::PRIMARY_HEADER_LEN.saturating_add(aldis_aos::header::FHEC_LEN)
            } else {
                aldis_aos::header::PRIMARY_HEADER_LEN
            }
        }

        fn write_header(
            channel: &AosChannel,
            _: Option<&AosVc>,
            f: &FrameFields,
            header: &mut [u8],
        ) -> Result<()> {
            // The SCID: its low 8 bits in the first two octets, the two most significant of the
            // 10 in bits 42–43 of Issue 5 (732.0 §4.1.2.2.3, §4.1.2.5.4; AOS-002).
            let [s_hi, scid8] = channel.scid.get().to_be_bytes();
            let spare = match channel.edition {
                AosEdition::Issue5 => s_hi & 0b11,
                AosEdition::Issue4Compatible => 0,
            };
            let [.., c2, c3, c4] = f.vc_count.to_be_bytes();
            let [.., cycle] = (f.vc_count >> 24).to_be_bytes();
            let cycle = if channel.cycle_in_use {
                cycle & 0x0F
            } else {
                0
            };
            let octets = [
                (aldis_aos::header::VERSION << 6) | (scid8 >> 2),
                ((scid8 & 0b11) << 6) | (f.vc & 0x3F),
                c2,
                c3,
                c4,
                // Replay Flag 0.
                flag(channel.cycle_in_use, 6) | (spare << 4) | cycle,
            ];
            for (d, s) in header.iter_mut().zip(octets) {
                *d = s;
            }
            Ok(())
        }

        fn write_oid_data_header(_: usize, _: &mut [u8]) {}
    }
}

#[cfg(feature = "aos")]
pub use aos_impl::{AosChannel, AosVc};

#[cfg(feature = "uslp")]
mod uslp_impl {
    use aldis_core::ids::{MapId, SpacecraftId};
    use aldis_core::layout::OCF_LEN;
    use aldis_core::{Error, ErrorKind, Result, field};
    use aldis_uslp::{
        LastValidOctet, SourceOrDestination, TfdfHeader, TfdzRule, Upid, VcfCountLen,
    };

    use super::{FrameFields, Fsh, MuxProtocol, VcConfig};
    use crate::common::Qos;
    use crate::protocol::{Route, Uslp};

    /// The parameters of the physical channel and master channel of a USLP mux with
    /// fixed-length frames (732.1 §5.2, §5.3; LNK-030, LNK-032, LNK-033).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct UslpChannel {
        /// The Spacecraft Identifier, 16 bits.
        pub scid: SpacecraftId<16>,
        /// Whether the SCID is the source or the destination of the frames.
        pub source_or_destination: SourceOrDestination,
        /// The VCF Count Length of VCID 63 (none by default; LNK-032).
        pub oid_vcf_len: VcfCountLen,
        /// The initial `MC_OCF`: "Inclusion of OCF Required" is true for the master channel, and
        /// every frame has an OCF, OID frames included (LNK-033, OI-76).
        pub mc_ocf: Option<[u8; OCF_LEN]>,
    }

    /// The parameters of a USLP VC (732.1 §5.4 Table 5-3; LNK-032, LNK-076).
    ///
    /// Built only by [`new`](Self::new), which relates COP-1 to the VCF Count Length (RS-020).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct UslpVc {
        map: MapId<4>,
        qos: Qos,
        vcf_len_sc: VcfCountLen,
        vcf_len_exp: VcfCountLen,
        cop: bool,
    }

    impl UslpVc {
        /// A VC whose frames have the MAP ID `map`, and `qos` for an offered field that names
        /// none, the VCF Count Lengths `vcf_len_sc` for sequence-controlled and `vcf_len_exp`
        /// for expedited frames, and COP-1 in effect if `cop`, so that an offer may carry the
        /// `cop` block (LNK-076).
        ///
        /// # Errors
        /// `InvalidConfig` (`vcf_count_len`) for COP-1 in effect with a sequence-controlled
        /// VCF Count Length of 0: FOP-1 puts the Frame Sequence Number of a Type-AD frame in
        /// the VCF Count field, which a length of `000` leaves out (732.1 §4.1.2.12.1,
        /// §4.2.7.4 note 3; WP-4's reading, OI-78).
        pub const fn new(
            map: MapId<4>,
            qos: Qos,
            vcf_len_sc: VcfCountLen,
            vcf_len_exp: VcfCountLen,
            cop: bool,
        ) -> Result<UslpVc> {
            if cop && vcf_len_sc.get() == 0 {
                Err(Error::new(ErrorKind::InvalidConfig).with_field(field::VCF_COUNT_LEN))
            } else {
                Ok(UslpVc {
                    map,
                    qos,
                    vcf_len_sc,
                    vcf_len_exp,
                    cop,
                })
            }
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
            self.vcf_len_sc
        }

        /// The VCF Count Length for expedited frames.
        ///
        /// Requirements: LNK-030…LNK-036, LNK-076, LNK-082; docs/04-engines.md §2.4.
        #[must_use]
        pub const fn vcf_len_exp(&self) -> VcfCountLen {
            self.vcf_len_exp
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
            if bypass {
                self.vcf_len_exp
            } else {
                self.vcf_len_sc
            }
        }
    }

    impl MuxProtocol for Uslp {
        type Channel = UslpChannel;
        type VcParams = UslpVc;
        const INSERT_ZONE: bool = true;
        const MC_COUNT: bool = false;
        const OID_BYPASS: bool = true;
        const OID_DATA_HEADER: usize = 3;

        fn oid_vcid(_: &UslpChannel) -> u8 {
            aldis_uslp::OID_VCID
        }

        fn mc_ocf(channel: &UslpChannel) -> Option<[u8; OCF_LEN]> {
            channel.mc_ocf
        }

        fn mc_fsh(_: &UslpChannel) -> Option<Fsh> {
            None
        }

        fn vc_ocf(_: &UslpVc) -> Option<[u8; OCF_LEN]> {
            None
        }

        fn vc_fsh(_: &UslpVc) -> Option<Fsh> {
            None
        }

        fn vc_route(vc: &UslpVc) -> Option<Route> {
            Some(Route {
                map: vc.map,
                qos: vc.qos,
            })
        }

        fn vc_cop(vc: &UslpVc) -> bool {
            vc.cop
        }

        fn vcf_max(vc: &UslpVc, bypass: bool) -> u64 {
            vc.count_len(bypass).max_count()
        }

        fn check_pointer(_: &UslpVc, _: Option<u16>) -> Result<u16> {
            Ok(0)
        }

        fn check_channel(
            _: &UslpChannel,
            frame_len: usize,
            vcs: &[Option<VcConfig<Uslp>>],
        ) -> Result<()> {
            let bad = |f| Error::new(ErrorKind::InvalidConfig).with_field(f);
            if vcs.iter().flatten().any(|v| v.id() == aldis_uslp::OID_VCID) {
                Err(bad(field::VC_ID))
            } else if frame_len > aldis_uslp::MAX_FRAME_LEN {
                Err(bad(field::FRAME_LEN))
            } else {
                Ok(())
            }
        }

        fn header_len(channel: &UslpChannel, vc: Option<&UslpVc>, bypass: bool) -> usize {
            let count = vc.map_or(channel.oid_vcf_len, |v| v.count_len(bypass));
            aldis_uslp::PRIMARY_HEADER_LEN.saturating_add(count.octets())
        }

        fn write_header(
            channel: &UslpChannel,
            vc: Option<&UslpVc>,
            f: &FrameFields,
            header: &mut [u8],
        ) -> Result<()> {
            let count_len = vc.map_or(channel.oid_vcf_len, |v| v.count_len(f.bypass));
            let map = vc.map_or(0, |v| v.map.get());
            // TFVN `1100`, SCID, Source-or-Destination, VCID, MAP ID, End of Primary Header
            // Flag 0 (732.1 §4.1.2.2–6).
            let word = (0b1100_u32 << 28)
                | (u32::from(channel.scid.get()) << 12)
                | (u32::from(channel.source_or_destination.bit()) << 11)
                | (u32::from(f.vc & 0x3F) << 5)
                | (u32::from(map & 0x0F) << 1);
            // The Frame Length field holds the length less one (732.1 §4.1.2.7). `check_channel` holds
            // the frame length to `MAX_FRAME_LEN`, so the conversion cannot fail; if it does, it is
            // an error, not a clamped length.
            let [l0, l1] = u16::try_from(f.frame_len.saturating_sub(1))
                .map_err(|_| Error::new(ErrorKind::InternalError).with_field(field::FRAME_LEN))?
                .to_be_bytes();
            let flags = (u8::from(f.bypass) << 7)
                | (u8::from(f.protocol_control) << 6)
                | (u8::from(f.ocf) << 3)
                | (count_len.get() & 0b111);
            let count = (f.vc_count & count_len.max_count()).to_be_bytes();
            let skip = 8_usize.saturating_sub(count_len.octets());
            let [w0, w1, w2, w3] = word.to_be_bytes();
            let head = [w0, w1, w2, w3, l0, l1, flags];
            for (d, s) in header
                .iter_mut()
                .zip(head.iter().chain(count.iter().skip(skip)))
            {
                *d = *s;
            }
            Ok(())
        }

        fn write_oid_data_header(data_len: usize, header: &mut [u8]) {
            // Rule `001`, UPID "Only Idle Data", and the Last Valid Octet Pointer at the last
            // octet of the TFDZ (732.1 §4.1.4.1.9–11; LNK-030).
            let [.., a, b] = data_len
                .saturating_sub(Uslp::OID_DATA_HEADER.saturating_add(1))
                .to_be_bytes();
            let tfdf = TfdfHeader {
                rule: TfdzRule::SduStart(LastValidOctet::from_raw(u16::from_be_bytes([a, b]))),
                upid: Upid::OnlyIdleData,
            };
            for (d, s) in header.iter_mut().zip(tfdf.octets()) {
                *d = s;
            }
        }
    }
}

#[cfg(feature = "uslp")]
pub use uslp_impl::{UslpChannel, UslpVc};

#[cfg(all(test, feature = "aos"))]
#[cfg_attr(coverage_nightly, coverage(off))]
#[expect(
    clippy::unwrap_used,
    reason = "test helpers fail loudly by design (RS-018)"
)]
mod tests {
    use aldis_aos::AosEdition;
    use aldis_core::ids::{SpacecraftId, VcId};

    use super::{
        AosChannel, AosVc, ChannelMux, ChannelMuxConfig, MuxInput, Offer, VcConfig, VcCounts,
    };
    use crate::policy::MuxPolicy;
    use crate::protocol::Aos;

    fn mux(cycle_in_use: bool) -> ChannelMux<Aos, 2, 32> {
        let channel = AosChannel::new(
            SpacecraftId::<10>::new(421).unwrap(),
            AosEdition::Issue5,
            false,
            cycle_in_use,
        )
        .unwrap();
        let vc = VcConfig::<Aos>::new(VcId::<6>::new(1).unwrap(), AosVc::default());
        ChannelMux::new(
            ChannelMuxConfig::new(channel, 28, true, MuxPolicy::RoundRobin, &[vc]).unwrap(),
        )
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
                    pointer: None,
                    route: None,
                    cop: None,
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

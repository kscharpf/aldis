//! Scenario values generated from `refapp/scenario/scenario.json` (APP-107).
//!
//! Do not edit: run `cargo run -p refapp-harness -- gen-scenario` after changing the
//! scenario file. Section numbers refer to `docs/reference-application.md`.

/// Scenario identifier in every golden-trace header (§7.2).
pub const SCENARIO: &str = "aldis-refapp";

/// Scenario version (§7.2, APP-002).
pub const SCENARIO_VERSION: u16 = 1;

/// Spacecraft ID of every frame (§3.4, APP-026); fictional (OI-26).
pub const SPACECRAFT_ID: u16 = 341;

/// Time model (§5, APP-060).
pub mod time {
    /// CUC coarse time at the start of the run, s since 1958-01-01 TAI.
    pub const T0_COARSE_S: u32 = 2_100_000_000;

    /// CUC coarse octets.
    pub const CUC_COARSE_OCTETS: u8 = 4;

    /// CUC fractional octets.
    pub const CUC_FINE_OCTETS: u8 = 2;

    /// Explicit P-field octet (301.0 §3.2.2).
    pub const CUC_EXPLICIT_PFIELD: u8 = 30;
}

/// Link model (§6.1).
pub mod link {
    /// One-way delay, both directions, µs (APP-072).
    pub const ONE_WAY_DELAY_US: u64 = 10_000;

    /// Downlink frame slot period, µs (APP-071).
    pub const FRAME_SLOT_US: u64 = 125_000;

    /// Uplink slot period, µs (APP-071).
    pub const UPLINK_SLOT_US: u64 = 250_000;
}

/// APIDs (§3.1, APP-020).
pub mod apid {
    /// Housekeeping.
    pub const HOUSEKEEPING: u16 = 16;

    /// Event reports.
    pub const EVENTS: u16 = 17;

    /// Payload science.
    pub const PAYLOAD: u16 = 32;

    /// Memory dump.
    pub const DUMP: u16 = 33;

    /// Commands.
    pub const COMMANDS: u16 = 128;

    /// Table upload.
    pub const TABLE_UPLOAD: u16 = 129;
}

/// TM downlink (§4.1).
pub mod tm {
    /// Transfer frame length, octets (APP-030).
    pub const FRAME_LENGTH: u16 = 223;

    /// FECF present (APP-030).
    pub const FECF: bool = true;

    /// VC of housekeeping and events (APP-032).
    pub const VCID_REALTIME: u8 = 0;

    /// VC of payload and dump (APP-032).
    pub const VCID_PAYLOAD: u8 = 1;

    /// VC of OID frames (APP-034).
    pub const VCID_IDLE: u8 = 7;

    /// Release deadline of the real-time VC, µs (APP-033).
    pub const RELEASE_DEADLINE_REALTIME_US: u64 = 250_000;

    /// Release deadline of the payload VC, µs (APP-033).
    pub const RELEASE_DEADLINE_PAYLOAD_US: u64 = 1_000_000;

    /// Maximum packet length, octets (APP-032).
    pub const MAX_PACKET_LENGTH: u16 = 512;

    /// Idle Packet data octet (APP-033).
    pub const IDLE_PATTERN: u8 = 0;

    /// Valid Packet Version Number (APP-136).
    pub const PACKET_VERSION: u8 = 0;

    /// Multiplexing priority of the real-time VC (APP-136).
    pub const PRIORITY_REALTIME: u8 = 1;

    /// Multiplexing priority of the payload VC (APP-136).
    pub const PRIORITY_PAYLOAD: u8 = 2;

    /// Initial value of the OCF slot (APP-136).
    pub const OCF_INITIAL: u32 = 0x0100_0000;

    /// The extractor delivers incomplete packets (APP-136).
    pub const DELIVER_INCOMPLETE_PACKETS: bool = false;

    /// The extractor delivers Idle Packets (APP-136).
    pub const DELIVER_IDLE_PACKETS: bool = false;
}

/// TC uplink (§4.2).
pub mod tc {
    /// Maximum transfer frame length, octets (APP-040).
    pub const MAX_FRAME_LENGTH: u16 = 128;

    /// FECF present (APP-040).
    pub const FECF: bool = true;

    /// The uplink VC (APP-041).
    pub const VCID: u8 = 0;

    /// MAP of real-time commands (APP-041).
    pub const MAP_COMMANDS: u8 = 0;

    /// MAP of the table upload (APP-041).
    pub const MAP_TABLE_UPLOAD: u8 = 1;

    /// Maximum frame data unit length, octets (APP-041).
    pub const MAX_FDU_LENGTH: u16 = 121;

    /// Maximum packet length, octets (APP-041).
    pub const MAX_PACKET_LENGTH: u16 = 512;

    /// Valid Packet Version Number (APP-136).
    pub const PACKET_VERSION: u8 = 0;

    /// The uplink VC carries a Segment Header (APP-136).
    pub const SEGMENT_HEADER: bool = true;

    /// MAP 0 permits blocking (APP-136).
    pub const MAP_COMMANDS_BLOCKING: bool = true;

    /// MAP 0 permits segmentation (APP-136).
    pub const MAP_COMMANDS_SEGMENTATION: bool = false;

    /// MAP 1 permits blocking (APP-136).
    pub const MAP_TABLE_UPLOAD_BLOCKING: bool = false;

    /// MAP 1 permits segmentation (APP-136).
    pub const MAP_TABLE_UPLOAD_SEGMENTATION: bool = true;
}

/// COP-1 parameters (§4.2, APP-044).
pub mod cop1 {
    /// FOP sliding window width K.
    pub const K: u8 = 4;

    /// FARM sliding window width W.
    pub const W: u8 = 8;

    /// `T1_Initial`, µs.
    pub const T1_INITIAL_US: u64 = 1_000_000;

    /// `Transmission_Limit`.
    pub const TRANSMISSION_LIMIT: u8 = 3;

    /// `Timeout_Type`.
    pub const TIMEOUT_TYPE: u8 = 0;

    /// FARM-1 internal buffer depth (COP-032).
    pub const FARM_BUFFER_DEPTH: u8 = 2;

    /// Status Field of the CLCW (APP-136).
    pub const FARM_STATUS_FIELD: u8 = 0;

    /// Initial V(R) of FARM-1 (APP-136).
    pub const FARM_INITIAL_VR: u8 = 0;

    /// Repetitions of a Type-A frame (APP-136).
    pub const FOP_REPETITIONS_TYPE_A: u8 = 1;

    /// Repetitions of a control command (APP-136).
    pub const FOP_REPETITIONS_CONTROL: u8 = 1;
}

/// AOS downlink of the variant `aos` (§4.3.1, APP-189).
pub mod aos {
    /// Transfer Frame Length, octets (APP-169).
    pub const FRAME_LENGTH: u16 = 223;

    /// Frame Header Error Control present (APP-169).
    pub const FHEC: bool = true;

    /// Frame Error Control Field present (APP-169).
    pub const FECF: bool = true;

    /// VC of housekeeping and events (APP-170).
    pub const VCID_REALTIME: u8 = 0;

    /// VC of payload and dump (APP-170).
    pub const VCID_PAYLOAD: u8 = 1;

    /// VC of the idle `M_PDU`s (APP-170).
    pub const VCID_IDLE: u8 = 7;

    /// `VC_OCF` present on each VC (APP-173).
    pub const VC_OCF: bool = true;

    /// Release deadline of the real-time VC, µs (APP-172).
    pub const RELEASE_DEADLINE_REALTIME_US: u64 = 250_000;

    /// Release deadline of the payload VC, µs (APP-172).
    pub const RELEASE_DEADLINE_PAYLOAD_US: u64 = 1_000_000;

    /// Maximum Packet Length, octets (APP-172).
    pub const MAX_PACKET_LENGTH: u16 = 512;

    /// Idle data octet (APP-172).
    pub const IDLE_PATTERN: u8 = 0;

    /// Valid Packet Version Number (APP-189).
    pub const PACKET_VERSION: u8 = 0;

    /// Multiplexing priority of the real-time VC (APP-170).
    pub const PRIORITY_REALTIME: u8 = 1;

    /// Multiplexing priority of the payload VC (APP-170).
    pub const PRIORITY_PAYLOAD: u8 = 2;

    /// Multiplexing priority of the idle VC (APP-170).
    pub const PRIORITY_IDLE: u8 = 3;

    /// The extractor delivers incomplete packets (APP-189).
    pub const DELIVER_INCOMPLETE_PACKETS: bool = false;
}

/// USLP downlink of the variant `uslp` (§4.3.2, APP-189).
pub mod uslp_down {
    /// Transfer Frame Length, octets (APP-175).
    pub const FRAME_LENGTH: u16 = 223;

    /// Frame Error Control Field present (APP-175).
    pub const FECF: bool = true;

    /// VC of housekeeping and events (APP-175).
    pub const VCID_REALTIME: u8 = 0;

    /// VC of payload and dump (APP-175).
    pub const VCID_PAYLOAD: u8 = 1;

    /// Inclusion of OCF Required of the master channel (APP-175).
    pub const OCF_REQUIRED: bool = true;

    /// VC Count Length for the Sequence-Controlled service, octets (APP-176).
    pub const VCF_COUNT_LENGTH_SEQUENCE: u8 = 0;

    /// VC Count Length for the Expedited service, octets (APP-176).
    pub const VCF_COUNT_LENGTH_EXPEDITED: u8 = 1;

    /// MAP ID of MAP 0 (APP-176).
    pub const MAP_ID: u8 = 0;

    /// Maximum delay for a TFDF to be completed on the real-time VC, µs (APP-177).
    pub const RELEASE_DEADLINE_REALTIME_US: u64 = 250_000;

    /// Maximum delay for a TFDF to be completed on the payload VC, µs (APP-177).
    pub const RELEASE_DEADLINE_PAYLOAD_US: u64 = 1_000_000;

    /// Maximum Packet Length, octets (APP-177).
    pub const MAX_PACKET_LENGTH: u16 = 512;

    /// Idle data octet (APP-177).
    pub const IDLE_PATTERN: u8 = 0;

    /// Valid Packet Version Number of the Space Packets (APP-189).
    pub const PACKET_VERSION: u8 = 0;

    /// Valid Packet Version Number of the Encapsulation Packets (APP-189).
    pub const ENCAPSULATION_PACKET_VERSION: u8 = 7;

    /// Multiplexing priority of the real-time VC (APP-175).
    pub const PRIORITY_REALTIME: u8 = 1;

    /// Multiplexing priority of the payload VC (APP-175).
    pub const PRIORITY_PAYLOAD: u8 = 2;

    /// The extractor delivers incomplete packets (APP-189).
    pub const DELIVER_INCOMPLETE_PACKETS: bool = false;
}

/// USLP uplink of the variant `uslp` (§4.3.3, APP-189).
pub mod uslp_up {
    /// Maximum Transfer Frame Length, octets (APP-182).
    pub const MAX_FRAME_LENGTH: u16 = 512;

    /// Frame Error Control Field present (APP-181).
    pub const FECF: bool = true;

    /// The uplink VC (APP-181).
    pub const VCID: u8 = 0;

    /// MAP ID of MAP 0 (APP-181).
    pub const MAP_ID: u8 = 0;

    /// VC Count Length for the Sequence-Controlled service, octets (APP-181).
    pub const VCF_COUNT_LENGTH_SEQUENCE: u8 = 1;

    /// VC Count Length for the Expedited service, octets (APP-181).
    pub const VCF_COUNT_LENGTH_EXPEDITED: u8 = 0;

    /// Inclusion of OCF Allowed (APP-181).
    pub const OCF_ALLOWED: bool = false;

    /// Maximum Packet Length of MAP 0, octets (APP-182).
    pub const MAX_PACKET_LENGTH: u16 = 501;

    /// MAP 0 permits blocking (APP-181).
    pub const MAP_BLOCKING: bool = true;

    /// MAP 0 permits segmentation (APP-181).
    pub const MAP_SEGMENTATION: bool = false;
}

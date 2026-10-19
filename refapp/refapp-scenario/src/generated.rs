//! Scenario values generated from `refapp/scenario/scenario.json` (APP-107).
//!
//! Do not edit: run `cargo run -p refapp-harness -- gen-scenario` after changing the
//! scenario file. Section numbers refer to `docs/reference-application.md`.

/// Scenario identifier in every golden-trace header (§7.2).
pub const SCENARIO: &str = "aldis-refapp";

/// Scenario version (§7.2, APP-002).
pub const SCENARIO_VERSION: u16 = 1;

/// Spacecraft ID of every frame (§3.4, APP-026); provisional and fictional until OI-26.
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
}

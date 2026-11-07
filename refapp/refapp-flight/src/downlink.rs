//! Downlink packet construction: on-board time, per-APID sequence counts, secondary header
//! forms (docs/reference-application.md §3.1, §3.2, §5; APP-021, APP-022, APP-060).

use aldis::core::ids::Apid;
use aldis::core::{Error, ErrorKind, Result};
use aldis::spp::{
    PacketFields, PacketType, SequenceCounter, SequenceFlags, SpacePacketWriter, SppConfig,
};
use aldis::time::{Cuc, CucEpoch, CucFormat, TimeCode};
use refapp_scenario::generated::{apid, time};
use refapp_scenario::{Instant, Output};

/// Octets of the largest packet buffer: the maximum packet length of the downlink (§4.1,
/// APP-032).
const PACKET_BUFFER: usize = 512;

/// A downlink packet stream: one APID with its own sequence count and the form of its
/// secondary header (§3.1, APP-020, APP-022).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    /// Housekeeping, `0x010`, CUC with implicit P-field.
    Housekeeping,
    /// Event reports, `0x011`, CUC with explicit P-field.
    Events,
    /// Payload science, `0x020`, CUC with implicit P-field.
    Payload,
    /// Memory dump, `0x021`, CUC with implicit P-field.
    Dump,
}

impl Stream {
    /// The raw APID of the stream (§3.1).
    #[must_use]
    pub const fn apid_value(self) -> u16 {
        match self {
            Stream::Housekeeping => apid::HOUSEKEEPING,
            Stream::Events => apid::EVENTS,
            Stream::Payload => apid::PAYLOAD,
            Stream::Dump => apid::DUMP,
        }
    }

    /// Whether the secondary header's Time Code Field starts with its P-field (§3.2).
    #[must_use]
    pub const fn explicit_pfield(self) -> bool {
        match self {
            Stream::Events => true,
            Stream::Housekeeping | Stream::Payload | Stream::Dump => false,
        }
    }
}

/// On-board time for `now`: CUC Level 1, 4 coarse and 2 fractional octets, equal to
/// T0 + `now` (§5, APP-060).
///
/// The conversion truncates (TIME-011); every time stamp of the
/// scenario is a multiple of 125 000 µs, which two fractional octets hold exactly (APP-061).
///
/// # Errors
/// `OutOfRange` if the coarse time does not fit four octets (TIME-012).
pub fn onboard_time(now: Instant) -> Result<TimeCode> {
    // The split of `now` into seconds and nanoseconds is glue that the library lacks: #305.
    let micros = now.as_micros();
    let seconds = (micros / 1_000_000)
        .checked_add(u64::from(time::T0_COARSE_S))
        .ok_or(Error::new(ErrorKind::OutOfRange))?;
    // Below 10⁶ µs, so below 10⁹ ns.
    let nanos = u32::try_from((micros % 1_000_000).saturating_mul(1_000))
        .map_err(|_| Error::new(ErrorKind::OutOfRange))?;
    let format = CucFormat::new(
        CucEpoch::Tai1958,
        time::CUC_COARSE_OCTETS,
        time::CUC_FINE_OCTETS,
    )?;
    Ok(TimeCode::Cuc(Cuc::from_seconds_nanos(
        format, seconds, nanos,
    )?))
}

/// Copies `src` to the start of `dst`.
fn copy_into(dst: &mut [u8], src: &[u8]) -> Result<()> {
    let to = dst
        .get_mut(..src.len())
        .ok_or(Error::buffer_too_small(src.len()))?;
    for (d, s) in to.iter_mut().zip(src) {
        *d = *s;
    }
    Ok(())
}

/// The downlink side of the flight node: one sequence counter per APID, all starting at 0 when
/// the run starts (§3.1, APP-021).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Downlink {
    housekeeping: SequenceCounter,
    events: SequenceCounter,
    payload: SequenceCounter,
    dump: SequenceCounter,
}

impl Downlink {
    /// A downlink with every count at 0.
    #[must_use]
    pub fn new() -> Downlink {
        Downlink::default()
    }

    const fn counter(&mut self, stream: Stream) -> &mut SequenceCounter {
        match stream {
            Stream::Housekeeping => &mut self.housekeeping,
            Stream::Events => &mut self.events,
            Stream::Payload => &mut self.payload,
            Stream::Dump => &mut self.dump,
        }
    }

    /// Builds a telemetry packet of `stream` with the time stamp of `now` and `user_data`,
    /// and reports it as `packet_out` and then as a link unit (§7.3). The stream's count
    /// advances only when the packet was built.
    ///
    /// # Errors
    /// The error of the time conversion or of the packet writer; nothing is output and the
    /// count does not advance.
    pub fn send(
        &mut self,
        stream: Stream,
        flags: SequenceFlags,
        now: Instant,
        user_data: &[u8],
        out: &mut dyn Output,
    ) -> Result<()> {
        // `Apid::new` of a known-good constant still has an error path: #304.
        let apid = Apid::new(stream.apid_value()).ok_or(Error::new(ErrorKind::InvalidConfig))?;
        let count = self.counter(stream).peek();
        let fields = PacketFields {
            packet_type: PacketType::Telemetry,
            apid,
            sequence_flags: flags,
            sequence_count: count,
        };
        // The size of the scratch buffer is a guess of the caller: #306.
        let mut stamp_buffer = [0_u8; 8];
        let stamp_len = onboard_time(now)?.encode(stream.explicit_pfield(), &mut stamp_buffer)?;
        let stamp = stamp_buffer
            .get(..stamp_len)
            .ok_or(Error::buffer_too_small(stamp_len))?;
        let mut buffer = [0_u8; PACKET_BUFFER];
        let mut writer = SpacePacketWriter::begin(
            &mut buffer,
            &SppConfig::DEFAULT,
            fields,
            stamp_len,
            user_data.len(),
        )?;
        copy_into(writer.secondary_header_mut(), stamp)?;
        copy_into(writer.user_data_mut(), user_data)?;
        let total = writer.finish(user_data.len())?;
        let packet = buffer.get(..total).ok_or(Error::buffer_too_small(total))?;
        self.counter(stream).advance();
        out.packet_out(apid, packet);
        out.unit(packet);
        Ok(())
    }
}

//! Downlink packet construction: on-board time, per-APID sequence counts, secondary header
//! forms (docs/reference-application.md §3.1, §3.2, §5; APP-021, APP-022, APP-060).

use aldis::core::ids::Apid;
use aldis::core::{Error, ErrorKind, Result};
use aldis::spp::{
    PacketFields, PacketType, Segmenter, SegmenterState, SequenceCounter, SequenceFlags,
    SpacePacketWriter, SppConfig,
};
use aldis::time::{Cuc, CucEpoch, CucFormat, TimeCode, TimeFormat};
use refapp_scenario::generated::{apid, time};
use refapp_scenario::{Instant, Output};

/// Octets of the packet buffer: the Maximum Packet Length of the downlink, 512 octets
/// (§4.1 packet-transfer parameters; APP-032; `generated::tm::MAX_PACKET_LENGTH`).
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
    /// The APID of the stream (§3.1).
    #[must_use]
    pub const fn apid(self) -> Apid {
        match self {
            Stream::Housekeeping => Apid::from_const::<{ apid::HOUSEKEEPING }>(),
            Stream::Events => Apid::from_const::<{ apid::EVENTS }>(),
            Stream::Payload => Apid::from_const::<{ apid::PAYLOAD }>(),
            Stream::Dump => Apid::from_const::<{ apid::DUMP }>(),
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

/// The CUC format of every on-board time code: Level 1, 4 coarse and 2 fractional octets (§5).
const fn cuc_format() -> Result<CucFormat> {
    CucFormat::new(
        CucEpoch::Tai1958,
        time::CUC_COARSE_OCTETS,
        time::CUC_FINE_OCTETS,
    )
}

/// Octets of the stamp buffer: a time code of [`cuc_format`] with its P-field, the longest form
/// of §3.2. A format that does not exist gives 0, and the encoding then fails.
const STAMP_BUFFER: usize = match cuc_format() {
    Ok(format) => TimeFormat::Cuc(format).encoded_len(true),
    Err(_) => 0,
};

const _: () = assert!(STAMP_BUFFER > 0, "the CUC format of §5 must exist");

/// On-board time for `now`: CUC Level 1, 4 coarse and 2 fractional octets, equal to
/// T0 + `now` (§5, APP-060).
///
/// The conversion truncates (TIME-011); every time stamp of the
/// scenario is a multiple of 125 000 µs, which two fractional octets hold exactly (APP-061).
///
/// # Errors
/// `OutOfRange` if the coarse time does not fit four octets (TIME-012).
pub fn onboard_time(now: Instant) -> Result<TimeCode> {
    let epoch_offset = u64::from(time::T0_COARSE_S);
    Ok(TimeCode::Cuc(Cuc::from_instant(
        cuc_format()?,
        epoch_offset,
        now,
    )?))
}

/// The T-field of the secondary header for `now`, with the P-field if the stream has one
/// (§3.2), in `buffer`.
fn time_stamp(stream: Stream, now: Instant, buffer: &mut [u8; STAMP_BUFFER]) -> Result<&[u8]> {
    let len = TimeFormat::Cuc(cuc_format()?).encoded_len(stream.explicit_pfield());
    let slot = buffer.get_mut(..len).ok_or(Error::buffer_too_small(len))?;
    onboard_time(now)?.encode(stream.explicit_pfield(), slot)?;
    Ok(slot)
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
        let apid = stream.apid();
        let count = self.counter(stream).peek();
        let fields = PacketFields {
            packet_type: PacketType::Telemetry,
            apid,
            sequence_flags: flags,
            sequence_count: count,
        };
        let mut stamp_buffer = [0_u8; STAMP_BUFFER];
        let stamp = time_stamp(stream, now, &mut stamp_buffer)?;
        let stamp_len = stamp.len();
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

    /// The progress of a segmenter over `sdu` for the dump APID, at most `max_user_len` user-data
    /// octets per packet (SPP-013, APP-016).
    ///
    /// # Errors
    /// The segmenter's error.
    pub fn segmenter(sdu: &[u8], max_user_len: usize) -> Result<SegmenterState> {
        let apid = Stream::Dump.apid();
        Ok(Segmenter::new(
            &SppConfig::DEFAULT,
            PacketType::Telemetry,
            apid,
            sdu,
            max_user_len,
        )?
        .suspend())
    }

    /// Sends the next dump packet of `sdu` with the time stamp of `now`, as `packet_out` and then
    /// as a link unit (§7.3), and returns the progress after it. The Segmenter sets the Sequence
    /// Flags (`01`, `00`, `10`, or `11` for a single packet, SPP-013) and takes the next count of
    /// the dump APID (APP-021).
    ///
    /// # Errors
    /// The error of the segmenter, the time conversion, or `InvalidState` if `progress` is
    /// already done; nothing is output and the count does not advance.
    pub fn send_segment(
        &mut self,
        progress: SegmenterState,
        sdu: &[u8],
        now: Instant,
        out: &mut dyn Output,
    ) -> Result<SegmenterState> {
        let apid = Stream::Dump.apid();
        let mut stamp_buffer = [0_u8; STAMP_BUFFER];
        let stamp = time_stamp(Stream::Dump, now, &mut stamp_buffer)?;
        let mut segmenter = progress.resume(sdu)?;
        let mut buffer = [0_u8; PACKET_BUFFER];
        let total = segmenter
            .next_packet(&mut self.dump, stamp, &mut buffer)?
            .ok_or(Error::new(ErrorKind::InvalidState))?;
        let packet = buffer.get(..total).ok_or(Error::buffer_too_small(total))?;
        out.packet_out(apid, packet);
        out.unit(packet);
        Ok(segmenter.suspend())
    }
}

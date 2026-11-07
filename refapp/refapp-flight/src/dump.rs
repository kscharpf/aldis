//! The memory dump (docs/reference-application.md §3.3.4; APP-016).

use aldis::core::Error;
use aldis::spp::SegmenterState;
use refapp_scenario::generated::link;
use refapp_scenario::{Instant, Output};

use crate::command::DumpRegion;
use crate::downlink::Downlink;

/// Octets before the memory in the dump SDU: address (u32) and length (u16) (§3.3.4).
const SDU_HEADER_LEN: usize = 6;

/// Longest dump SDU: the header and 1 024 memory octets (`DUMP_MAX_LENGTH`).
const SDU_MAX_LEN: usize = SDU_HEADER_LEN + 1_024;

/// User-data octets of a dump packet at most: the SDU is cut into chunks of 128 (§3.3.4,
/// APP-016).
pub const SEGMENT_OCTETS: usize = 128;

/// The simulated memory: the octet at address `a` is (a XOR (a >> 8)) mod 256 (§3.3.4).
#[must_use]
pub const fn memory_octet(address: u32) -> u8 {
    let [_, _, _, low] = (address ^ (address >> 8)).to_be_bytes();
    low
}

/// A packet of the dump that was sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment {
    /// The slot instant of the packet, which is its time stamp.
    pub at: Instant,
    /// Whether it was the last packet of the dump.
    pub last: bool,
}

/// A dump in progress: its SDU, how far the segmenter has come, and when the next packet is due
/// (APP-016).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dump {
    region: DumpRegion,
    sdu: [u8; SDU_MAX_LEN],
    sdu_len: usize,
    progress: SegmenterState,
    /// Slot instant of the next packet, µs.
    next_us: u64,
}

impl Dump {
    /// A dump of `region` whose first packet is due at `first_us` (APP-016).
    ///
    /// # Errors
    /// The segmenter's or the packet APID's error, which the scenario's values rule out.
    pub fn start(region: DumpRegion, first_us: u64) -> Result<Dump, Error> {
        let mut sdu = [0_u8; SDU_MAX_LEN];
        let [a3, a2, a1, a0] = region.address().to_be_bytes();
        let [l1, l0] = region.length().to_be_bytes();
        // Every address of the region is below 2³² (`DumpRegion::new`).
        let memory =
            (0..u32::from(region.length())).map(|i| memory_octet(region.address().wrapping_add(i)));
        let header = [a3, a2, a1, a0, l1, l0];
        for (d, s) in sdu.iter_mut().zip(header.into_iter().chain(memory)) {
            *d = s;
        }
        let sdu_len = SDU_HEADER_LEN.saturating_add(usize::from(region.length()));
        let whole = sdu.get(..sdu_len).ok_or(Error::buffer_too_small(sdu_len))?;
        let progress = Downlink::segmenter(whole, SEGMENT_OCTETS)?;
        Ok(Dump {
            region,
            sdu,
            sdu_len,
            progress,
            next_us: first_us,
        })
    }

    /// The memory this dump reads.
    #[must_use]
    pub const fn region(&self) -> DumpRegion {
        self.region
    }

    /// The slot instant of the next packet, µs.
    #[must_use]
    pub const fn due_us(&self) -> u64 {
        self.next_us
    }

    /// Sends the next packet, stamped with its slot instant, and moves the next slot one frame
    /// slot (125 000 µs) on (APP-016, APP-061).
    ///
    /// # Errors
    /// The error of the segmenter or of the packet writer; nothing is output and the dump stays
    /// where it was.
    pub fn send(
        &mut self,
        downlink: &mut Downlink,
        out: &mut dyn Output,
    ) -> Result<Segment, Error> {
        let at = Instant::from_micros(self.next_us);
        let sdu = self
            .sdu
            .get(..self.sdu_len)
            .ok_or(Error::buffer_too_small(self.sdu_len))?;
        self.progress = downlink.send_segment(self.progress, sdu, at, out)?;
        self.next_us = self.next_us.saturating_add(link::FRAME_SLOT_US);
        Ok(Segment {
            at,
            last: self.progress.is_done(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{SDU_HEADER_LEN, SDU_MAX_LEN, memory_octet};
    use crate::command::DUMP_MAX_LENGTH;

    #[test]
    fn simulated_memory_follows_the_formula() {
        // req: APP-023
        // a = 0x00010000: low octet of a ^ (a >> 8) = 0x00 ^ 0x00 = 0x00; a + 1: 0x01 ^ 0x00.
        assert_eq!(memory_octet(0x0001_0000), 0x00);
        assert_eq!(memory_octet(0x0001_0001), 0x01);
        // a = 0x1234: 0x34 ^ 0x12 = 0x26.
        assert_eq!(memory_octet(0x1234), 0x26);
        // The shifted octet 1 enters the low octet: 0xFFFF gives 0xFF ^ 0xFF = 0.
        assert_eq!(memory_octet(0xFFFF), 0x00);
        assert_eq!(memory_octet(0xFFFF_FFFF), 0x00);
    }

    #[test]
    fn the_sdu_buffer_holds_the_longest_dump() {
        // req: APP-016
        assert_eq!(SDU_MAX_LEN, 1_030);
        // The buffer is sized from the limit that `DumpRegion` enforces.
        assert_eq!(
            usize::from(DUMP_MAX_LENGTH).checked_add(SDU_HEADER_LEN),
            Some(SDU_MAX_LEN)
        );
    }
}

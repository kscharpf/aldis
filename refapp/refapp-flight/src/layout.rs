//! Fixed-size user-data layouts (docs/reference-application.md §3.3): all integers are
//! unsigned and big-endian (GEN-050).

/// Concatenates `parts` into an array of `N` octets. A short concatenation leaves the tail
/// zero and a long one is cut; the layouts below fill their arrays exactly, which their tests
/// check.
fn assemble<const N: usize>(parts: &[&[u8]]) -> [u8; N] {
    let mut out = [0_u8; N];
    for (d, s) in out.iter_mut().zip(parts.iter().flat_map(|p| p.iter())) {
        *d = *s;
    }
    out
}

/// The payload mode (§2.1, §3.3.1 octet 9); *on* at t = 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PayloadMode {
    /// 0: no payload packets.
    Off,
    /// 1: one packet per payload slot.
    #[default]
    On,
}

impl PayloadMode {
    /// The octet of §3.3.1 (0 off, 1 on); the `PAYLOAD_MODE` event carries the same value.
    #[must_use]
    pub const fn octet(self) -> u8 {
        match self {
            PayloadMode::Off => 0,
            PayloadMode::On => 1,
        }
    }
}

/// The status the housekeeping packet reports (§3.3.1). The command handler and the table
/// store of A1 WP-3b update it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Status {
    /// Commands executed, wrapping modulo 2¹⁶ (octets 4–5).
    pub commands_executed: u16,
    /// Commands rejected, wrapping modulo 2¹⁶ (octets 6–7).
    pub commands_rejected: u16,
    /// Time-tag queue occupancy (octet 8).
    pub queue_occupancy: u8,
    /// Last activated table ID, 0 for none (octet 10).
    pub table_id: u8,
    /// CRC-16 of the last activated table, 0 for none (octets 12–13).
    pub table_crc: u16,
    /// Dumps completed (octets 14–15).
    pub dumps_completed: u16,
    /// The payload mode (octet 9, §2.1).
    pub payload_mode: PayloadMode,
}

/// Length of the housekeeping user data (§3.3.1, APP-023).
pub const HK_LEN: usize = 32;
/// Length of an event report's user data (§3.3.2).
pub const EVENT_LEN: usize = 8;
/// Length of the payload user data (§3.3.3).
pub const PAYLOAD_LEN: usize = 400;

/// The housekeeping user data of packet number `k` (§3.3.1, APP-023).
#[must_use]
pub fn housekeeping(status: &Status, k: u32) -> [u8; HK_LEN] {
    let mut sensors = [[0_u8; 2]; 8];
    let mut base = 1_000_u32;
    for s in &mut sensors {
        // (1000 · (j + 1) + 3 · k) mod 2¹⁶ is the low two octets of the sum.
        let [_, _, hi, lo] = base.wrapping_add(k.wrapping_mul(3)).to_be_bytes();
        *s = [hi, lo];
        base = base.wrapping_add(1_000);
    }
    let [s0, s1, s2, s3, s4, s5, s6, s7] = sensors;
    assemble(&[
        &k.to_be_bytes(),
        &status.commands_executed.to_be_bytes(),
        &status.commands_rejected.to_be_bytes(),
        &[
            status.queue_occupancy,
            status.payload_mode.octet(),
            status.table_id,
            0,
        ],
        &status.table_crc.to_be_bytes(),
        &status.dumps_completed.to_be_bytes(),
        &s0,
        &s1,
        &s2,
        &s3,
        &s4,
        &s5,
        &s6,
        &s7,
    ])
}

/// The user data of payload packet number `n` (§3.3.3, APP-023): the number, then octet
/// i = (n + i) mod 256 for i = 4…399.
#[must_use]
pub fn payload(n: u32) -> [u8; PAYLOAD_LEN] {
    let [_, _, _, low] = n.to_be_bytes();
    let mut v = low.wrapping_add(4);
    let mut tail = [0_u8; PAYLOAD_LEN - 4];
    for d in &mut tail {
        *d = v;
        v = v.wrapping_add(1);
    }
    assemble(&[&n.to_be_bytes(), &tail])
}

#[cfg(test)]
mod tests {
    use super::{PayloadMode, Status, housekeeping, payload};

    #[test]
    fn housekeeping_layout() {
        // req: APP-023
        let status = Status {
            commands_executed: 0x0102,
            commands_rejected: 0x0304,
            queue_occupancy: 5,
            table_id: 1,
            table_crc: 0x0C4F,
            dumps_completed: 2,
            payload_mode: PayloadMode::Off,
        };
        let hk = housekeeping(&status, 7);
        assert_eq!(hk[..4], [0, 0, 0, 7]);
        assert_eq!(hk[4..8], [1, 2, 3, 4]);
        assert_eq!(hk[8..12], [5, 0, 1, 0]);
        assert_eq!(hk[12..16], [0x0C, 0x4F, 0, 2]);
        // Sensor j = 0: 1000 + 3 · 7 = 1021 = 0x03FD; j = 7: 8000 + 21 = 8021 = 0x1F55.
        assert_eq!(hk[16..18], [0x03, 0xFD]);
        assert_eq!(hk[30..32], [0x1F, 0x55]);
    }

    #[test]
    fn housekeeping_defaults_and_wrap() {
        // req: APP-023
        let hk = housekeeping(&Status::default(), 0);
        assert_eq!(hk[8..16], [0, 1, 0, 0, 0, 0, 0, 0]);
        assert_eq!(hk[16..18], 1000_u16.to_be_bytes());
        // Sensor values wrap modulo 2¹⁶: k = 21 846 gives 3k = 65 538.
        let hk = housekeeping(&Status::default(), 21_846);
        assert_eq!(hk[16..18], 1002_u16.to_be_bytes());
    }

    #[test]
    fn payload_layout() {
        // req: APP-023
        let p = payload(0x0102_03FE);
        assert_eq!(p[..4], [1, 2, 3, 0xFE]);
        // Octet 4 = (n + 4) mod 256 = 0x02, then it wraps.
        assert_eq!(p[4..8], [0x02, 0x03, 0x04, 0x05]);
        assert_eq!(p[399], 0xFE_u8.wrapping_add(399_u16.to_be_bytes()[1]));
    }
}

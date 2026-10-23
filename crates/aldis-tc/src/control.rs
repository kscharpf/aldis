//! Control Commands Unlock and Set V(R) (232.0 §4.1.3.3; TC-005).

use aldis_core::ids::FrameSeqNum;

/// First octet of the Set V(R) Control Command, `10000010` (232.0 §4.1.3.3.3).
const SET_VR: u8 = 0b1000_0010;

/// A Control Command of a Type-C frame (232.0 §4.1.3.3; TC-005).
///
/// 232.0 §4.1.3.3.4 reserves every other bit combination, so these two are all a writer may
/// send. A decoder in lenient mode reports any other pattern with
/// [`TcAnomaly::UnknownControlCommand`](crate::anomaly::TcAnomaly::UnknownControlCommand)
/// and keeps its octets in the view's data field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ControlCommand {
    /// Unlock: one octet of zeros (232.0 §4.1.3.3.2).
    Unlock,
    /// Set V(R): `82 00` followed by the new V(R) (232.0 §4.1.3.3.3).
    SetVr(FrameSeqNum),
}

impl ControlCommand {
    /// The Control Command that `octets` holds exactly, or `None` for a reserved pattern,
    /// including a known pattern with octets missing or added (232.0 §4.1.3.3; TC-005).
    #[must_use]
    pub const fn parse(octets: &[u8]) -> Option<ControlCommand> {
        match octets {
            [0] => Some(ControlCommand::Unlock),
            [SET_VR, 0, vr] => Some(ControlCommand::SetVr(FrameSeqNum::new(*vr))),
            _ => None,
        }
    }

    /// The octets of the command, as a Type-C frame's data field carries them (232.0
    /// §4.1.3.3; API-020).
    #[must_use]
    pub const fn to_octets(self) -> ControlOctets {
        match self {
            ControlCommand::Unlock => ControlOctets {
                octets: [0; 3],
                len: 1,
            },
            ControlCommand::SetVr(vr) => ControlOctets {
                octets: [SET_VR, 0, vr.get()],
                len: 3,
            },
        }
    }
}

/// The one or three octets of a [`ControlCommand`] (232.0 §4.1.3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ControlOctets {
    octets: [u8; 3],
    len: usize,
}

impl ControlOctets {
    /// The octets (232.0 §4.1.3.3; API-020).
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        self.octets.get(..self.len).unwrap_or_default()
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::ControlCommand;
    use aldis_core::ids::FrameSeqNum;

    #[test]
    fn unlock_and_set_vr() {
        // req: TC-005, API-020
        assert_eq!(ControlCommand::parse(&[0]), Some(ControlCommand::Unlock));
        assert_eq!(
            ControlCommand::parse(&[0x82, 0, 0xFE]),
            Some(ControlCommand::SetVr(FrameSeqNum::new(0xFE)))
        );
        for reserved in [
            &[][..],
            &[1],
            &[0, 0],
            &[0x82, 0],
            &[0x82, 1, 0],
            &[0x83, 0, 0],
            &[0x82, 0, 0, 0],
        ] {
            assert_eq!(ControlCommand::parse(reserved), None, "{reserved:02X?}");
        }
        assert_eq!(ControlCommand::Unlock.to_octets().as_slice(), [0]);
        let set = ControlCommand::SetVr(FrameSeqNum::new(7));
        assert_eq!(set.to_octets().as_slice(), [0x82, 0, 7]);
        assert_eq!(ControlCommand::parse(set.to_octets().as_slice()), Some(set));
    }
}

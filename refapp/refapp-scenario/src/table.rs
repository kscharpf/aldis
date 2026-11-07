//! Table identifiers and lengths (docs/reference-application.md §3.3.5, §3.3.6; APP-017).
//!
//! Each type holds a value of its range only (RS-020): the ops-script checks of the harness, the
//! ground node's table upload, and the flight node's table store share them.

use aldis::core::crc::crc16;
use aldis::core::{Error, ErrorKind};

/// A CRC-16 (CMN-001): every `u16` is one, so the type only keeps a CRC from being taken for a
/// length or a sequence count (api-feedback #318: aldis-core's CRC functions return a bare `u16`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Crc16(u16);

impl Crc16 {
    /// The CRC-16 of `data` (CMN-001).
    #[must_use]
    pub fn of(data: &[u8]) -> Crc16 {
        Crc16(crc16(data))
    }

    /// The CRC-16 carried on the wire, such as the argument of `ACTIVATE_TABLE` (§3.3.5).
    #[must_use]
    pub const fn from_wire(value: u16) -> Crc16 {
        Crc16(value)
    }

    /// The value, for encoding.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

/// A table ID: 1 to 255 (§3.3.6, APP-017).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableId(u8);

impl TableId {
    /// The table ID `id`.
    ///
    /// # Errors
    /// `InvalidField` for 0.
    pub const fn new(id: u8) -> Result<TableId, Error> {
        if id == 0 {
            Err(Error::new(ErrorKind::InvalidField))
        } else {
            Ok(TableId(id))
        }
    }

    /// The value, 1 to 255.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

/// A table length in octets: 1 to [`TableLength::MAX`] (§3.3.6, APP-017).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableLength(u16);

impl TableLength {
    /// The longest table, octets (APP-017): 503, so that the upload packet, 9 octets longer
    /// (§3.3.6), fits the 512-octet Maximum Packet Length of the uplink.
    pub const MAX: u16 = 503;

    /// The table length `length`.
    ///
    /// # Errors
    /// `InvalidField` for 0, `OutOfRange` above [`TableLength::MAX`].
    pub const fn new(length: u16) -> Result<TableLength, Error> {
        if length == 0 {
            Err(Error::new(ErrorKind::InvalidField))
        } else if length > TableLength::MAX {
            Err(Error::new(ErrorKind::OutOfRange))
        } else {
            Ok(TableLength(length))
        }
    }

    /// The value, 1 to 503.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use aldis::core::ErrorKind;

    use super::{Crc16, TableId, TableLength};

    #[test]
    fn table_lengths_are_one_to_503() {
        // req: APP-017
        let kind = |n| TableLength::new(n).unwrap_err().kind();
        assert_eq!(kind(0), ErrorKind::InvalidField);
        assert_eq!(TableLength::new(1).unwrap().get(), 1);
        assert_eq!(TableLength::new(503).unwrap().get(), 503);
        assert_eq!(TableLength::MAX, 503);
        assert_eq!(kind(504), ErrorKind::OutOfRange);
        assert_eq!(kind(u16::MAX), ErrorKind::OutOfRange);
    }

    #[test]
    fn crc16_of_the_check_string_and_the_wire_value() {
        // req: APP-017
        assert_eq!(Crc16::of(b"123456789").get(), 0x29B1);
        assert_eq!(Crc16::from_wire(0x0C4F).get(), 0x0C4F);
        assert_ne!(Crc16::from_wire(1), Crc16::from_wire(2));
    }

    #[test]
    fn table_ids_are_one_to_255() {
        // req: APP-017
        assert_eq!(TableId::new(0).unwrap_err().kind(), ErrorKind::InvalidField);
        assert_eq!(TableId::new(1).unwrap().get(), 1);
        assert_eq!(TableId::new(255).unwrap().get(), 255);
    }
}

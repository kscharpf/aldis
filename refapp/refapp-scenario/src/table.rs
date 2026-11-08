//! Table identifiers and lengths (docs/reference-application.md §3.3.5, §3.3.6; APP-017).
//!
//! Each type holds a value of its range only (RS-020): the harness's scenario types take their
//! bounds from the constants of these types, and the ground node's table upload and the flight
//! node's table store use the types.

use aldis::core::{Error, ErrorKind};

/// A table ID: 1 to 255 (§3.3.6, APP-017).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableId(u8);

impl TableId {
    /// The lowest table ID (§3.3.6, APP-017).
    pub const MIN: u8 = 1;
    /// The highest table ID (§3.3.6, APP-017): `u8::MAX`, so that [`TableId::new`] needs no upper
    /// check.
    pub const MAX: u8 = u8::MAX;

    /// The table ID `id`.
    ///
    /// # Errors
    /// `InvalidField` below [`TableId::MIN`].
    pub const fn new(id: u8) -> Result<TableId, Error> {
        if id < TableId::MIN {
            Err(Error::new(ErrorKind::InvalidField))
        } else {
            Ok(TableId(id))
        }
    }

    /// The value, from [`TableId::MIN`] to [`TableId::MAX`].
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

/// A table length in octets: 1 to [`TableLength::MAX`] (§3.3.6, APP-017).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableLength(u16);

impl TableLength {
    /// The shortest table, octets (§3.3.6, APP-017).
    pub const MIN: u16 = 1;
    /// The longest table, octets (APP-017): 503, so that the upload packet, 9 octets longer
    /// (§3.3.6), fits the 512-octet Maximum Packet Length of the uplink.
    pub const MAX: u16 = 503;

    /// The table length `length`.
    ///
    /// # Errors
    /// `InvalidField` below [`TableLength::MIN`], `OutOfRange` above [`TableLength::MAX`].
    pub const fn new(length: u16) -> Result<TableLength, Error> {
        if length < TableLength::MIN {
            Err(Error::new(ErrorKind::InvalidField))
        } else if length > TableLength::MAX {
            Err(Error::new(ErrorKind::OutOfRange))
        } else {
            Ok(TableLength(length))
        }
    }

    /// The value, from [`TableLength::MIN`] to [`TableLength::MAX`].
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use aldis::core::ErrorKind;

    use super::{TableId, TableLength};

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
    fn table_ids_are_one_to_255() {
        // req: APP-017
        assert_eq!(TableId::new(0).unwrap_err().kind(), ErrorKind::InvalidField);
        assert_eq!(TableId::new(1).unwrap().get(), 1);
        assert_eq!(TableId::new(255).unwrap().get(), 255);
    }
}

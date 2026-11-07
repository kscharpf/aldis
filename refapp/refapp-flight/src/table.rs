//! The table store (docs/reference-application.md §2.1, §3.3.6; APP-017).

use aldis::core::{Error, ErrorKind};
use refapp_scenario::{Crc16, TableId, TableLength};

/// The outcome of `ACTIVATE_TABLE` (APP-017).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activation {
    /// No table is stored, or its ID is another one: `TABLE_REJECTED` reason 1.
    NoTable,
    /// The CRC-16 of the stored table is not the command's: `TABLE_REJECTED` reason 2.
    CrcMismatch,
    /// The table is active: `TABLE_ACTIVATED` with its length and CRC-16.
    Activated {
        /// Table length, octets.
        length: TableLength,
        /// CRC-16 of the table (CMN-001).
        crc: Crc16,
    },
}

/// The table most recently uploaded. The scenario gives a table no effect, so the node never
/// reads its octets after the upload; the store keeps what activation compares and reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Table {
    id: TableId,
    length: TableLength,
    /// CRC-16 (CMN-001) over the table octets, computed when the table is stored.
    crc: Crc16,
}

/// Holds one table, the most recent upload (§2.1, APP-017).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TableStore {
    stored: Option<Table>,
}

impl TableStore {
    /// Stores the table of an upload packet's user data (§3.3.6): table ID (1…255), length L
    /// (1…503), and exactly L octets. The table replaces the one stored; a rejected upload leaves
    /// it. Returns the table ID and the length.
    ///
    /// # Errors
    /// `Incomplete` for user data shorter than the 3 octets before the table, `InvalidField` for
    /// table ID 0 or length 0, `OutOfRange` for a length above [`TABLE_MAX_LENGTH`],
    /// `LengthMismatch` if the octets after the length are not L.
    pub fn store(&mut self, data: &[u8]) -> Result<(TableId, TableLength), Error> {
        let &[id, l1, l0, ref octets @ ..] = data else {
            return Err(Error::incomplete(3));
        };
        let id = TableId::new(id)?;
        let length = TableLength::new(u16::from_be_bytes([l1, l0]))?;
        if octets.len() != usize::from(length.get()) {
            return Err(Error::new(ErrorKind::LengthMismatch));
        }
        self.stored = Some(Table {
            id,
            length,
            crc: Crc16::of(octets),
        });
        Ok((id, length))
    }

    /// `ACTIVATE_TABLE` for `table_id` with the CRC-16 the command claims (APP-017).
    #[must_use]
    pub fn activate(&self, table_id: u8, claimed_crc: Crc16) -> Activation {
        match self.stored {
            Some(table) if table.id.get() == table_id => {
                if table.crc == claimed_crc {
                    Activation::Activated {
                        length: table.length,
                        crc: table.crc,
                    }
                } else {
                    Activation::CrcMismatch
                }
            }
            Some(_) | None => Activation::NoTable,
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use aldis::core::ErrorKind;
    use refapp_scenario::{Crc16, TableId, TableLength};

    use super::{Activation, TableStore};

    /// Table octets of the scenario: octet i = (3 · i + 1) mod 256 (§3.3.6).
    fn pattern(length: usize) -> std::vec::Vec<u8> {
        (0..length)
            .map(|i| u8::try_from(i.wrapping_mul(3).wrapping_add(1) % 256).unwrap())
            .collect()
    }

    /// The user data of an upload of table `id` with `length` octets and `declared` as its
    /// length field.
    fn upload(id: u8, declared: u16, length: usize) -> std::vec::Vec<u8> {
        let mut v = std::vec![id];
        v.extend_from_slice(&declared.to_be_bytes());
        v.extend(pattern(length));
        v
    }

    #[test]
    fn the_nominal_table_activates_with_its_crc() {
        // req: APP-017
        let mut store = TableStore::default();
        assert_eq!(
            store.store(&upload(1, 300, 300)),
            Ok((TableId::new(1).unwrap(), TableLength::new(300).unwrap()))
        );
        assert_eq!(
            store.activate(1, Crc16::from_wire(0x0C4F)),
            Activation::Activated {
                length: TableLength::new(300).unwrap(),
                crc: Crc16::from_wire(0x0C4F)
            }
        );
        assert_eq!(
            store.activate(1, Crc16::from_wire(0x0C4E)),
            Activation::CrcMismatch
        );
    }

    #[test]
    fn no_table_or_another_id_is_reason_one() {
        // req: APP-017
        let mut store = TableStore::default();
        assert_eq!(
            store.activate(1, Crc16::from_wire(0x0C4F)),
            Activation::NoTable
        );
        store.store(&upload(1, 300, 300)).unwrap();
        assert_eq!(
            store.activate(2, Crc16::from_wire(0x0C4F)),
            Activation::NoTable
        );
        assert_eq!(
            store.activate(0, Crc16::from_wire(0x0C4F)),
            Activation::NoTable
        );
    }

    #[test]
    fn only_the_latest_upload_is_kept() {
        // req: APP-017
        let mut store = TableStore::default();
        store.store(&upload(1, 300, 300)).unwrap();
        store.store(&upload(2, 4, 4)).unwrap();
        assert_eq!(
            store.activate(1, Crc16::from_wire(0x0C4F)),
            Activation::NoTable
        );
        assert_eq!(
            store.activate(2, Crc16::from_wire(0)),
            Activation::CrcMismatch
        );
        // The same ID again replaces the table of that ID.
        store.store(&upload(2, 300, 300)).unwrap();
        assert_eq!(
            store.activate(2, Crc16::from_wire(0x0C4F)),
            Activation::Activated {
                length: TableLength::new(300).unwrap(),
                crc: Crc16::from_wire(0x0C4F)
            }
        );
    }

    #[test]
    fn a_table_of_503_octets_fits_and_504_does_not() {
        // req: APP-017
        assert_eq!(TableLength::MAX, 503);
        let mut store = TableStore::default();
        assert_eq!(
            store.store(&upload(1, 503, 503)),
            Ok((TableId::new(1).unwrap(), TableLength::new(503).unwrap()))
        );
        let error = store.store(&upload(2, 504, 504)).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::OutOfRange);
        // The rejected upload left the stored table.
        assert_eq!(store.activate(2, Crc16::from_wire(0)), Activation::NoTable);
        assert_eq!(
            store.activate(1, Crc16::from_wire(0)),
            Activation::CrcMismatch
        );
    }

    #[test]
    fn malformed_uploads_are_refused_with_their_kind() {
        // req: APP-017, APP-024
        let kind = |data: &[u8]| TableStore::default().store(data).unwrap_err().kind();
        assert_eq!(kind(&[]), ErrorKind::Incomplete);
        assert_eq!(kind(&[1, 0]), ErrorKind::Incomplete);
        assert_eq!(kind(&upload(0, 4, 4)), ErrorKind::InvalidField);
        assert_eq!(kind(&upload(1, 0, 0)), ErrorKind::InvalidField);
        assert_eq!(kind(&upload(1, 5, 4)), ErrorKind::LengthMismatch);
        assert_eq!(kind(&upload(1, 4, 5)), ErrorKind::LengthMismatch);
    }
}

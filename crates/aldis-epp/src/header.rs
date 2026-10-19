//! Encapsulation Packet header: identifiers, header lengths, and the owned header type
//! (133.1 §4.1.2; EPP-001…EPP-008, API-014, API-021).

use aldis_core::{Error, ErrorDetail, ErrorKind, FieldId, Result};

/// Packet Version Number of the Encapsulation Packet, `111` (133.1 §4.1.2.2.2).
pub(crate) const VERSION: u8 = 0b111;

/// Defines a range-checked identifier of `$bits` bits (API-001).
macro_rules! small_id {
    ($(#[$doc:meta])* $name:ident, $field:literal, $max:literal) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
        pub struct $name(u8);

        impl $name {
            /// Largest valid value (API-001).
            pub const MAX: u8 = $max;

            /// The value, or `None` if it does not fit the field (API-001).
            #[must_use]
            pub const fn new(value: u8) -> Option<$name> {
                if value <= $name::MAX { Some($name(value)) } else { None }
            }

            /// The numeric value (API-001).
            #[must_use]
            pub const fn get(self) -> u8 {
                self.0
            }
        }

        impl TryFrom<u8> for $name {
            type Error = Error;

            fn try_from(value: u8) -> Result<$name> {
                $name::new(value).ok_or_else(|| {
                    Error::out_of_range($field, u64::from($name::MAX), u64::from(value))
                })
            }
        }

        impl From<$name> for u8 {
            fn from(id: $name) -> u8 {
                id.0
            }
        }
    };
}

small_id!(
    /// Encapsulation Protocol ID, 3 bits (133.1 §4.1.2.3). Values other than the three
    /// constants are assigned by SANA and are checked against a caller-supplied set
    /// ([`EppConfig`](crate::config::EppConfig), EPP-010).
    Epi, "epi", 0b111
);
small_id!(
    /// Encapsulation Protocol ID Extension, 4 bits; present in 4- and 8-octet headers
    /// (133.1 §4.1.2.6).
    EpiExtension, "epi_extension", 0xF
);
small_id!(
    /// User Defined field, 4 bits of mission-specific header data; present in 4- and 8-octet
    /// headers (133.1 §4.1.2.5).
    UserDefined, "user_defined", 0xF
);

impl Epi {
    /// `000`: Encapsulation Idle Packet (133.1 §4.1.2.3 note 1).
    pub const IDLE: Epi = Epi(0b000);
    /// `110`: the Protocol ID Extension identifies the protocol (133.1 §4.1.2.3 note 2).
    pub const EXTENDED: Epi = Epi(0b110);
    /// `111`: mission-specific, privately defined data (133.1 §4.1.2.3 note 3).
    pub const MISSION_SPECIFIC: Epi = Epi(0b111);

    /// Whether this is the idle EPI `000` (133.1 §4.1.2.3 note 1).
    #[must_use]
    pub const fn is_idle(self) -> bool {
        self.0 == Epi::IDLE.0
    }
}

/// Length of the Packet Header, set by the Length of Length field (133.1 Table 4-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HeaderLen {
    /// 1 octet, Length of Length `00`: Idle Packet of total length 1 (§4.1.2.4.4, EPP-004).
    One,
    /// 2 octets, Length of Length `01`: 1-octet Packet Length, total 2–255 (Table 4-2).
    Two,
    /// 4 octets, Length of Length `10`: User Defined, Extension, 2-octet Packet Length
    /// (Figure 4-2, Table 4-2).
    Four,
    /// 8 octets, Length of Length `11`: adds the CCSDS Defined Field and a 4-octet length
    /// (Figure 4-2, Table 4-2).
    Eight,
}

impl HeaderLen {
    /// The header length for a Length of Length value; only the two low bits are used
    /// (133.1 Table 4-1, EPP-002).
    #[must_use]
    pub const fn from_length_of_length(lol: u8) -> HeaderLen {
        match lol & 0b11 {
            0b00 => HeaderLen::One,
            0b01 => HeaderLen::Two,
            0b10 => HeaderLen::Four,
            _ => HeaderLen::Eight,
        }
    }

    /// The Length of Length field value (133.1 §4.1.2.4, API-021).
    #[must_use]
    pub const fn length_of_length(self) -> u8 {
        match self {
            HeaderLen::One => 0b00,
            HeaderLen::Two => 0b01,
            HeaderLen::Four => 0b10,
            HeaderLen::Eight => 0b11,
        }
    }

    /// Header length in octets (133.1 §4.1.2.1.1).
    #[must_use]
    pub const fn octets(self) -> usize {
        match self {
            HeaderLen::One => 1,
            HeaderLen::Two => 2,
            HeaderLen::Four => 4,
            HeaderLen::Eight => 8,
        }
    }

    /// Largest total packet length this header can describe (133.1 Table 4-2).
    #[must_use]
    pub const fn max_packet_len(self) -> u32 {
        match self {
            HeaderLen::One => 1,
            HeaderLen::Two => 0xFF,
            HeaderLen::Four => 0xFFFF,
            HeaderLen::Eight => u32::MAX,
        }
    }

    /// Whether the header has the User Defined and Protocol ID Extension fields (133.1
    /// §4.1.2.1.1 d–e).
    #[must_use]
    pub const fn has_extension(self) -> bool {
        matches!(self, HeaderLen::Four | HeaderLen::Eight)
    }
}

/// Header size policy of the encoder (EPP-008).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum HeaderSize {
    /// The smallest header that can carry the fields and the total length. The 1-octet header
    /// is chosen only for an Idle Packet without data (133.1 §4.1.2.1.2 note).
    #[default]
    Auto,
    /// Exactly this header length; `OutOfRange` if the total length does not fit, and
    /// `InvalidField` if a field the caller set does not exist in this header.
    Fixed(HeaderLen),
}

/// The header fields a caller chooses when encoding (EPP-008).
///
/// Derived fields (Length of Length, Packet Length) are computed by the encoder, and the CCSDS
/// Defined Field is always written as zero (API-021, GEN-052). The fields are private, so the
/// Protocol ID Extension is non-zero only with EPI `110` (133.1 §4.1.2.6.3, RS-020).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct EncapsulationFields {
    epi: Epi,
    extension: EpiExtension,
    user_defined: UserDefined,
}

impl EncapsulationFields {
    /// Fields with `epi`, a zero extension, and zero user-defined bits; valid for every EPI,
    /// including `110` with extended EPI 0 (EPP-008).
    #[must_use]
    pub const fn new(epi: Epi) -> EncapsulationFields {
        EncapsulationFields {
            epi,
            extension: EpiExtension(0),
            user_defined: UserDefined(0),
        }
    }

    /// Fields with EPI `110`, whose Protocol ID Extension `extension` identifies the protocol
    /// (133.1 §4.1.2.6.2, EPP-008).
    #[must_use]
    pub const fn extended(extension: EpiExtension) -> EncapsulationFields {
        EncapsulationFields {
            epi: Epi::EXTENDED,
            extension,
            user_defined: UserDefined(0),
        }
    }

    /// The same fields with User Defined bits `user_defined`; a non-zero value needs a 4- or
    /// 8-octet header (133.1 §4.1.2.5, EPP-008).
    #[must_use]
    pub const fn with_user_defined(self, user_defined: UserDefined) -> EncapsulationFields {
        EncapsulationFields {
            user_defined,
            ..self
        }
    }

    /// Fields from separately supplied values, for callers that hold them raw.
    ///
    /// # Errors
    /// `InvalidField` (`epi_extension`) for a non-zero extension when `epi` is not `110`
    /// (133.1 §4.1.2.6.3, EPP-008).
    pub const fn from_parts(
        epi: Epi,
        extension: EpiExtension,
        user_defined: UserDefined,
    ) -> Result<EncapsulationFields> {
        if extension.0 != 0 && epi.0 != Epi::EXTENDED.0 {
            return Err(invalid("epi_extension"));
        }
        Ok(EncapsulationFields {
            epi,
            extension,
            user_defined,
        })
    }

    /// Encapsulation Protocol ID (133.1 §4.1.2.3).
    #[must_use]
    pub const fn epi(&self) -> Epi {
        self.epi
    }

    /// Protocol ID Extension; zero unless the EPI is `110` (133.1 §4.1.2.6).
    #[must_use]
    pub const fn extension(&self) -> EpiExtension {
        self.extension
    }

    /// User Defined field (133.1 §4.1.2.5).
    #[must_use]
    pub const fn user_defined(&self) -> UserDefined {
        self.user_defined
    }

    /// Whether the fields need a header with the User Defined and Extension fields: EPI
    /// `110`, whose protocol the extension identifies, or non-zero user-defined bits (133.1
    /// §4.1.2.1.1 d–e, §4.1.2.6.2). A non-zero extension implies EPI `110`.
    const fn needs_extension(self) -> bool {
        self.epi.0 == Epi::EXTENDED.0 || self.user_defined.0 != 0
    }
}

/// An Encapsulation Packet header as plain values (API-014). Fields absent from the header
/// length read as `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EncapsulationHeader {
    epi: Epi,
    header_len: HeaderLen,
    extension: EpiExtension,
    user_defined: UserDefined,
    ccsds_defined: u16,
    packet_len: u32,
}

/// Converts a length to `u64` for error details, saturating.
pub(crate) fn len_u64(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

/// `InvalidField` naming `field`.
pub(crate) const fn invalid(field: &'static str) -> Error {
    Error::new(ErrorKind::InvalidField).with_field(FieldId::new(field))
}

impl EncapsulationHeader {
    /// The header for a packet with `fields` and `data_len` data octets under `size`
    /// (EPP-005, EPP-008, API-021). Crate-internal: callers size a user packet's header through
    /// [`EppConfig::header`](crate::config::EppConfig::header), which also applies the managed
    /// parameters (EPP-010).
    ///
    /// # Errors
    /// - `InvalidField` (`epi`) for an empty data field when the EPI is not idle (EPP-005), or
    ///   for `Fixed(One)` with a non-idle EPI.
    /// - `InvalidField` (`header_size`) for `Fixed(One | Two)` when the fields need a 4- or
    ///   8-octet header.
    /// - `OutOfRange` (`packet_len`) when the total length does not fit the chosen header,
    ///   or no header under `Auto`.
    pub(crate) fn for_data(
        fields: EncapsulationFields,
        size: HeaderSize,
        data_len: usize,
    ) -> Result<EncapsulationHeader> {
        if data_len == 0 && !fields.epi.is_idle() {
            return Err(invalid("epi"));
        }
        let header_len = match size {
            HeaderSize::Auto => Self::auto_len(fields, data_len)?,
            HeaderSize::Fixed(len) => {
                if len == HeaderLen::One && !fields.epi.is_idle() {
                    return Err(invalid("epi"));
                }
                if !len.has_extension() && fields.needs_extension() {
                    return Err(invalid("header_size"));
                }
                len
            }
        };
        let total = data_len.checked_add(header_len.octets());
        let max = header_len.max_packet_len();
        match total.and_then(|t| u32::try_from(t).ok()) {
            Some(packet_len) if packet_len <= max => Ok(EncapsulationHeader {
                epi: fields.epi,
                header_len,
                extension: fields.extension,
                user_defined: fields.user_defined,
                ccsds_defined: 0,
                packet_len,
            }),
            _ => Err(Error::out_of_range(
                "packet_len",
                u64::from(max),
                total.map_or(u64::MAX, len_u64),
            )),
        }
    }

    /// The smallest header for `fields` and `data_len` (EPP-008 `Auto`).
    fn auto_len(fields: EncapsulationFields, data_len: usize) -> Result<HeaderLen> {
        if fields.epi.is_idle() && data_len == 0 && !fields.needs_extension() {
            return Ok(HeaderLen::One);
        }
        let candidates: &[HeaderLen] = if fields.needs_extension() {
            &[HeaderLen::Four, HeaderLen::Eight]
        } else {
            &[HeaderLen::Two, HeaderLen::Four, HeaderLen::Eight]
        };
        candidates
            .iter()
            .copied()
            .find(|len| {
                data_len
                    .checked_add(len.octets())
                    .and_then(|t| u32::try_from(t).ok())
                    .is_some_and(|t| t <= len.max_packet_len())
            })
            .ok_or_else(|| {
                Error::out_of_range(
                    "packet_len",
                    u64::from(u32::MAX),
                    len_u64(data_len).saturating_add(8),
                )
            })
    }

    /// The header of an Idle Packet of `total_len` octets, with the smallest header that fits
    /// (EPP-009).
    ///
    /// # Errors
    /// `OutOfRange` (`packet_len`) if `total_len` is 0 (limit 1) or above `u32::MAX` (limit
    /// `u32::MAX`), GEN-041.
    pub fn idle(total_len: usize) -> Result<EncapsulationHeader> {
        let found = len_u64(total_len);
        if total_len == 0 {
            return Err(Error::out_of_range("packet_len", 1, 0));
        }
        let packet_len = u32::try_from(total_len)
            .map_err(|_| Error::out_of_range("packet_len", u64::from(u32::MAX), found))?;
        let header_len = match packet_len {
            1 => HeaderLen::One,
            2..=0xFF => HeaderLen::Two,
            0x100..=0xFFFF => HeaderLen::Four,
            _ => HeaderLen::Eight,
        };
        Ok(EncapsulationHeader {
            epi: Epi::IDLE,
            header_len,
            extension: EpiExtension(0),
            user_defined: UserDefined(0),
            ccsds_defined: 0,
            packet_len,
        })
    }

    /// Parses the header at the start of `bytes` (133.1 §4.1.2). Checks the version
    /// (EPP-001), the EPI of a 1-octet header (EPP-004), EPI `110` in a 2-octet header
    /// (EPP-012), Packet Length ≥ header length (EPP-003), and the idle EPI when the data field
    /// is absent (EPP-005). Does not check the extension or the CCSDS Defined Field, which are
    /// mode-dependent, or that the data field is present in `bytes`.
    pub(crate) fn parse(bytes: &[u8]) -> Result<EncapsulationHeader> {
        let Some(&octet0) = bytes.first() else {
            return Err(Error::incomplete(1));
        };
        let version = octet0 >> 5;
        if version != VERSION {
            return Err(Error::new(ErrorKind::UnsupportedVersion)
                .with_field(FieldId::new("version"))
                .with_detail(ErrorDetail::Expected {
                    expected: u64::from(VERSION),
                    found: u64::from(version),
                }));
        }
        let epi = Epi((octet0 >> 2) & 0b111);
        let header_len = HeaderLen::from_length_of_length(octet0);
        let (octet1, ccsds_defined, packet_len) = match (header_len, bytes) {
            (HeaderLen::One, _) => (0, 0, 1),
            (HeaderLen::Two, [_, len, ..]) => (0, 0, u32::from(*len)),
            (HeaderLen::Four, [_, o1, l0, l1, ..]) => {
                (*o1, 0, u32::from(u16::from_be_bytes([*l0, *l1])))
            }
            (HeaderLen::Eight, [_, o1, c0, c1, l0, l1, l2, l3, ..]) => (
                *o1,
                u16::from_be_bytes([*c0, *c1]),
                u32::from_be_bytes([*l0, *l1, *l2, *l3]),
            ),
            _ => return Err(Error::incomplete(header_len.octets())),
        };
        if header_len == HeaderLen::One && !epi.is_idle() {
            return Err(invalid("epi"));
        }
        // EPI 110 needs the Protocol ID Extension to identify the protocol (EPP-012).
        if header_len == HeaderLen::Two && epi == Epi::EXTENDED {
            return Err(invalid("epi"));
        }
        let header_octets = len_u64(header_len.octets());
        if header_octets > u64::from(packet_len) {
            return Err(Error::new(ErrorKind::LengthMismatch)
                .with_field(FieldId::new("packet_len"))
                .with_detail(ErrorDetail::Expected {
                    expected: header_octets,
                    found: u64::from(packet_len),
                }));
        }
        // An absent data field requires the idle EPI, in both modes (EPP-005, OI-16).
        if header_octets == u64::from(packet_len) && !epi.is_idle() {
            return Err(invalid("epi"));
        }
        Ok(EncapsulationHeader {
            epi,
            header_len,
            extension: EpiExtension(octet1 & 0xF),
            user_defined: UserDefined(octet1 >> 4),
            ccsds_defined,
            packet_len,
        })
    }

    /// The same header with Packet Length `packet_len` (no checks; for the two-phase writer).
    pub(crate) const fn with_packet_len(self, packet_len: u32) -> EncapsulationHeader {
        EncapsulationHeader { packet_len, ..self }
    }

    /// The header octets as an encoder writes them: the first [`HeaderLen::octets`] are used.
    /// The CCSDS Defined Field is written as zero, and the Protocol ID Extension as zero unless
    /// the EPI is `110`, so a header taken from a lenient decode re-encodes conformant (GEN-052,
    /// 133.1 §4.1.2.6.3, §4.1.2.7.2).
    fn octets(&self) -> [u8; 8] {
        let octet0 = (VERSION << 5) | (self.epi.0 << 2) | self.header_len.length_of_length();
        let extension = if self.epi == Epi::EXTENDED {
            self.extension.0
        } else {
            0
        };
        let octet1 = (self.user_defined.0 << 4) | extension;
        let [l0, l1, l2, l3] = self.packet_len.to_be_bytes();
        match self.header_len {
            HeaderLen::One => [octet0, 0, 0, 0, 0, 0, 0, 0],
            HeaderLen::Two => [octet0, l3, 0, 0, 0, 0, 0, 0],
            HeaderLen::Four => [octet0, octet1, l2, l3, 0, 0, 0, 0],
            HeaderLen::Eight => [octet0, octet1, 0, 0, l0, l1, l2, l3],
        }
    }

    /// Copies the header octets into the start of `out`, which the caller has sized; the single
    /// header writer behind [`encode`](Self::encode) and the [`writer`](crate::writer) module.
    pub(crate) fn write_to(&self, out: &mut [u8]) {
        let n = self.header_len.octets();
        for (d, s) in out.iter_mut().zip(self.octets()).take(n) {
            *d = s;
        }
    }

    /// Writes the header octets to the start of `out`; returns the header length. Reserved
    /// fields are written as zero (GEN-052, see `octets`).
    ///
    /// # Errors
    /// `BufferTooSmall` if `out` is shorter than the header; `out` is then unchanged.
    pub fn encode(&self, out: &mut [u8]) -> Result<usize> {
        let n = self.header_len.octets();
        let dst = out.get_mut(..n).ok_or(Error::buffer_too_small(n))?;
        self.write_to(dst);
        Ok(n)
    }

    /// Encapsulation Protocol ID (133.1 §4.1.2.3, API-014).
    #[must_use]
    pub const fn epi(&self) -> Epi {
        self.epi
    }

    /// Header length (133.1 §4.1.2.4, API-014).
    #[must_use]
    pub const fn header_len(&self) -> HeaderLen {
        self.header_len
    }

    /// Protocol ID Extension, if the header has one (133.1 §4.1.2.6, API-014).
    #[must_use]
    pub const fn extension(&self) -> Option<EpiExtension> {
        if self.header_len.has_extension() {
            Some(self.extension)
        } else {
            None
        }
    }

    /// User Defined field, if the header has one (133.1 §4.1.2.5, API-014).
    #[must_use]
    pub const fn user_defined(&self) -> Option<UserDefined> {
        if self.header_len.has_extension() {
            Some(self.user_defined)
        } else {
            None
        }
    }

    /// CCSDS Defined Field, if the header has one (8-octet header; EPP-007).
    #[must_use]
    pub const fn ccsds_defined(&self) -> Option<u16> {
        match self.header_len {
            HeaderLen::Eight => Some(self.ccsds_defined),
            _ => None,
        }
    }

    /// Total packet length in octets, header included (133.1 §4.1.2.8.2).
    #[must_use]
    pub const fn packet_len(&self) -> u32 {
        self.packet_len
    }

    /// Length of the Encapsulated Data Field in octets (133.1 Table 4-2, API-014).
    #[must_use]
    pub fn data_len(&self) -> usize {
        usize::try_from(self.packet_len)
            .unwrap_or(usize::MAX)
            .saturating_sub(self.header_len.octets())
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{
        EncapsulationFields, EncapsulationHeader, Epi, EpiExtension, HeaderLen, HeaderSize,
        UserDefined,
    };
    use aldis_core::{ErrorDetail, ErrorKind};

    #[test]
    fn identifiers_are_range_checked() {
        // req: API-001
        assert_eq!(Epi::new(7).map(Epi::get), Some(7));
        assert_eq!(Epi::new(8), None);
        assert_eq!(Epi::try_from(8).unwrap_err().kind(), ErrorKind::OutOfRange);
        assert_eq!(u8::from(Epi::try_from(6).unwrap()), 6);
        assert_eq!(EpiExtension::new(15).map(u8::from), Some(15));
        assert_eq!(
            EpiExtension::try_from(16).unwrap_err().kind(),
            ErrorKind::OutOfRange
        );
        assert_eq!(UserDefined::new(16), None);
        assert_eq!(UserDefined::try_from(9).map(UserDefined::get), Ok(9));
        assert!(Epi::IDLE.is_idle() && !Epi::MISSION_SPECIFIC.is_idle());
        assert_eq!(
            (
                Epi::IDLE.get(),
                Epi::EXTENDED.get(),
                Epi::MISSION_SPECIFIC.get()
            ),
            (0, 6, 7)
        );
        assert_eq!(Epi::default(), Epi::IDLE);
    }

    #[test]
    fn header_len_table() {
        // req: EPP-002
        for (lol, len, octets, max, ext) in [
            (0, HeaderLen::One, 1, 1, false),
            (1, HeaderLen::Two, 2, 255, false),
            (2, HeaderLen::Four, 4, 65_535, true),
            (3, HeaderLen::Eight, 8, u32::MAX, true),
        ] {
            assert_eq!(HeaderLen::from_length_of_length(lol), len);
            assert_eq!(HeaderLen::from_length_of_length(lol | 0xFC), len);
            assert_eq!(len.length_of_length(), lol);
            assert_eq!(len.octets(), octets);
            assert_eq!(len.max_packet_len(), max);
            assert_eq!(len.has_extension(), ext);
        }
        assert_eq!(HeaderSize::default(), HeaderSize::Auto);
    }

    #[test]
    fn accessors_follow_header_length() {
        // req: EPP-011, API-014
        let fields = EncapsulationFields {
            epi: Epi::EXTENDED,
            extension: EpiExtension::new(3).unwrap(),
            user_defined: UserDefined::new(5).unwrap(),
        };
        let h =
            EncapsulationHeader::for_data(fields, HeaderSize::Fixed(HeaderLen::Eight), 2).unwrap();
        assert_eq!(h.epi(), Epi::EXTENDED);
        assert_eq!(h.header_len(), HeaderLen::Eight);
        assert_eq!(h.extension().map(EpiExtension::get), Some(3));
        assert_eq!(h.user_defined().map(UserDefined::get), Some(5));
        assert_eq!(h.ccsds_defined(), Some(0));
        assert_eq!((h.packet_len(), h.data_len()), (10, 2));
        let four = EncapsulationHeader::for_data(fields, HeaderSize::Auto, 2).unwrap();
        assert_eq!(four.header_len(), HeaderLen::Four);
        assert_eq!(four.ccsds_defined(), None);
        let two = EncapsulationHeader::for_data(
            EncapsulationFields::new(Epi::MISSION_SPECIFIC),
            HeaderSize::Auto,
            1,
        )
        .unwrap();
        assert_eq!(two.header_len(), HeaderLen::Two);
        assert_eq!(
            (two.extension(), two.user_defined(), two.ccsds_defined()),
            (None, None, None)
        );
        assert_eq!(
            EncapsulationFields::default(),
            EncapsulationFields::new(Epi::IDLE)
        );
    }

    #[test]
    fn fields_cannot_hold_an_extension_without_epi_110() {
        // req: EPP-006, EPP-008, RS-020
        let ext = EpiExtension::new(9).unwrap();
        let user = UserDefined::new(3).unwrap();
        let x = EncapsulationFields::extended(ext).with_user_defined(user);
        assert_eq!(
            (x.epi(), x.extension(), x.user_defined()),
            (Epi::EXTENDED, ext, user)
        );
        let m = EncapsulationFields::new(Epi::MISSION_SPECIFIC);
        assert_eq!(
            (m.epi(), m.extension().get(), m.user_defined().get()),
            (Epi::MISSION_SPECIFIC, 0, 0)
        );
        assert_eq!(
            EncapsulationFields::from_parts(Epi::EXTENDED, ext, user),
            Ok(x)
        );
        assert_eq!(
            EncapsulationFields::from_parts(
                Epi::MISSION_SPECIFIC,
                EpiExtension::default(),
                UserDefined::default()
            ),
            Ok(m)
        );
        let e = EncapsulationFields::from_parts(Epi::MISSION_SPECIFIC, ext, user).unwrap_err();
        assert_eq!(
            (e.kind(), e.field().map(aldis_core::FieldId::name)),
            (ErrorKind::InvalidField, Some("epi_extension"))
        );
    }

    #[test]
    fn for_data_rejections() {
        // req: EPP-005, EPP-006, EPP-008, API-021
        let user1 = UserDefined::new(1).unwrap();
        let mission = EncapsulationFields::new(Epi::MISSION_SPECIFIC);
        let cases = [
            (mission, HeaderSize::Auto, 0, "epi"),
            (mission, HeaderSize::Fixed(HeaderLen::One), 1, "epi"),
            (
                EncapsulationFields {
                    user_defined: user1,
                    ..mission
                },
                HeaderSize::Fixed(HeaderLen::Two),
                1,
                "header_size",
            ),
            (
                EncapsulationFields::new(Epi::EXTENDED),
                HeaderSize::Fixed(HeaderLen::One),
                1,
                "epi",
            ),
        ];
        for (fields, size, len, field) in cases {
            let e = EncapsulationHeader::for_data(fields, size, len).unwrap_err();
            assert_eq!(e.kind(), ErrorKind::InvalidField, "{fields:?} {size:?}");
            assert_eq!(e.field().map(aldis_core::FieldId::name), Some(field));
        }
        let idle = EncapsulationFields::new(Epi::IDLE);
        let h = EncapsulationHeader::for_data(idle, HeaderSize::Auto, 0).unwrap();
        assert_eq!(h.header_len(), HeaderLen::One);
        let h = EncapsulationHeader::for_data(idle, HeaderSize::Fixed(HeaderLen::One), 0).unwrap();
        assert_eq!(h.packet_len(), 1);
        let e =
            EncapsulationHeader::for_data(idle, HeaderSize::Fixed(HeaderLen::One), 1).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::OutOfRange);
    }

    #[test]
    fn auto_with_only_user_defined_on_idle() {
        // req: EPP-008
        let fields = EncapsulationFields {
            user_defined: UserDefined::new(1).unwrap(),
            ..EncapsulationFields::new(Epi::IDLE)
        };
        let h = EncapsulationHeader::for_data(fields, HeaderSize::Auto, 0).unwrap();
        assert_eq!((h.header_len(), h.packet_len()), (HeaderLen::Four, 4));
    }

    #[test]
    fn oversized_lengths_are_out_of_range() {
        // req: EPP-008, GEN-020
        let f = EncapsulationFields::new(Epi::MISSION_SPECIFIC);
        for size in [HeaderSize::Auto, HeaderSize::Fixed(HeaderLen::Eight)] {
            let e = EncapsulationHeader::for_data(f, size, usize::MAX).unwrap_err();
            assert_eq!(e.kind(), ErrorKind::OutOfRange);
        }
        let e =
            EncapsulationHeader::for_data(f, HeaderSize::Fixed(HeaderLen::Two), 254).unwrap_err();
        assert_eq!(
            e.detail(),
            ErrorDetail::Expected {
                expected: 255,
                found: 256
            }
        );
        let e = EncapsulationHeader::for_data(f, HeaderSize::Fixed(HeaderLen::Eight), usize::MAX)
            .unwrap_err();
        assert_eq!(
            e.detail(),
            ErrorDetail::Expected {
                expected: u64::from(u32::MAX),
                found: u64::MAX
            }
        );
        let e = EncapsulationHeader::idle(0).unwrap_err();
        assert_eq!(
            (e.kind(), e.detail()),
            (
                ErrorKind::OutOfRange,
                ErrorDetail::Expected {
                    expected: 1,
                    found: 0
                }
            )
        );
        assert_eq!(
            EncapsulationHeader::idle(usize::MAX).unwrap_err().kind(),
            ErrorKind::OutOfRange
        );
    }

    #[test]
    fn idle_header_boundaries() {
        // req: EPP-009
        for (total, len) in [
            (1, HeaderLen::One),
            (2, HeaderLen::Two),
            (255, HeaderLen::Two),
            (256, HeaderLen::Four),
            (65_535, HeaderLen::Four),
            (65_536, HeaderLen::Eight),
        ] {
            let h = EncapsulationHeader::idle(total).unwrap();
            assert_eq!((h.header_len(), h.epi()), (len, Epi::IDLE), "total {total}");
            assert_eq!(usize::try_from(h.packet_len()).unwrap(), total);
        }
    }

    #[test]
    fn encode_needs_room_for_the_header() {
        // req: API-022
        let h = EncapsulationHeader::idle(300).unwrap();
        let mut out = [0xAA_u8; 3];
        let e = h.encode(&mut out).unwrap_err();
        assert_eq!(
            (e.kind(), e.detail()),
            (ErrorKind::BufferTooSmall, ErrorDetail::Needed(4))
        );
        assert_eq!(out, [0xAA; 3]);
        let mut out = [0_u8; 4];
        assert_eq!(h.encode(&mut out), Ok(4));
        assert_eq!(out, [0xE2, 0x00, 0x01, 0x2C]);
    }

    #[test]
    fn encode_zeroes_reserved_fields_of_a_lenient_header() {
        // req: GEN-052, EPP-006, EPP-007
        let bytes = [0xE7, 0x01, 0x00, 0x01, 0, 0, 0, 9, 0xAB];
        let h = EncapsulationHeader::parse(&bytes).unwrap();
        assert_eq!(
            (h.extension().map(EpiExtension::get), h.ccsds_defined()),
            (Some(1), Some(1))
        );
        let mut out = [0_u8; 8];
        assert_eq!(h.encode(&mut out), Ok(8));
        assert_eq!(out, [0xE7, 0x00, 0x00, 0x00, 0, 0, 0, 9]);
        // EPI 110 keeps its extension; the CCSDS Defined Field is still zeroed.
        let h = EncapsulationHeader::parse(&[0xFB, 0x51, 0xBE, 0xEF, 0, 0, 0, 9, 0xAB]).unwrap();
        assert_eq!(h.encode(&mut out), Ok(8));
        assert_eq!(out, [0xFB, 0x51, 0x00, 0x00, 0, 0, 0, 9]);
    }

    #[test]
    fn parse_rejects_an_empty_non_idle_packet() {
        // req: EPP-005
        for bytes in [
            &[0xE5, 0x02][..],
            &[0xE6, 0x00, 0x00, 0x04],
            &[0xE7, 0, 0, 0, 0, 0, 0, 8],
        ] {
            let e = EncapsulationHeader::parse(bytes).unwrap_err();
            assert_eq!(
                (e.kind(), e.field().map(aldis_core::FieldId::name)),
                (ErrorKind::InvalidField, Some("epi"))
            );
        }
        assert_eq!(
            EncapsulationHeader::parse(&[0xE1, 0x02]).map(|h| h.packet_len()),
            Ok(2)
        );
    }

    #[test]
    fn parse_reports_the_found_version() {
        // req: EPP-001, GEN-041
        let e = EncapsulationHeader::parse(&[0x20]).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::UnsupportedVersion);
        assert_eq!(e.field().map(aldis_core::FieldId::name), Some("version"));
        assert_eq!(
            e.detail(),
            ErrorDetail::Expected {
                expected: 7,
                found: 1
            }
        );
        let e = EncapsulationHeader::parse(&[0xE1, 0x01]).unwrap_err();
        assert_eq!(
            e.detail(),
            ErrorDetail::Expected {
                expected: 2,
                found: 1
            }
        );
        assert_eq!(e.field().map(aldis_core::FieldId::name), Some("packet_len"));
    }
}

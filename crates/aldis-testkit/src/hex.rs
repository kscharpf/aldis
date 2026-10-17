//! Hex encoding used by test vectors: upper- or lower-case digits, no separators.

/// Decodes a hex string into octets.
///
/// # Errors
/// Returns a message for an odd number of digits or a non-hex character.
pub fn decode(text: &str) -> Result<Vec<u8>, String> {
    if !text.len().is_multiple_of(2) {
        return Err(format!("odd number of hex digits ({})", text.len()));
    }
    let (pairs, _) = text.as_bytes().as_chunks::<2>();
    pairs
        .iter()
        .enumerate()
        .map(|(i, &[hi, lo])| {
            let digit = |c: u8| match c {
                b'0'..=b'9' => Ok(c - b'0'),
                b'a'..=b'f' => Ok(c - b'a' + 10),
                b'A'..=b'F' => Ok(c - b'A' + 10),
                _ => Err(format!(
                    "invalid hex character {:?} at offset {}",
                    char::from(c),
                    2 * i
                )),
            };
            Ok(digit(hi)? << 4 | digit(lo)?)
        })
        .collect()
}

/// Encodes octets as upper-case hex, the canonical form in vector files.
#[must_use]
pub fn encode(octets: &[u8]) -> String {
    use std::fmt::Write as _;
    octets
        .iter()
        .fold(String::with_capacity(octets.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02X}");
            s
        })
}

#[cfg(test)]
mod tests {
    use super::{decode, encode};

    #[test]
    fn round_trip() {
        // req: TV-001
        assert_eq!(decode("00FFa5").unwrap(), [0x00, 0xFF, 0xA5]);
        assert_eq!(encode(&[0x00, 0xFF, 0xA5]), "00FFA5");
        assert_eq!(decode("").unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn rejects_malformed() {
        // req: TV-001
        assert!(decode("ABC").is_err());
        assert!(decode("0G").is_err());
        assert!(decode("0 1").is_err());
    }
}

//! The probe: derives the bit layout of a header from the encoders and decoders of the codec
//! crates (VER-072).
//!
//! A header is a list of fields. A field is either *encoded*, a function from a value of the
//! field to the header octets the crate's public encoder writes, or *rejected*, a fixed field
//! (a version number, a reserved spare) that the encoder never changes and whose bits the
//! decoder names when it rejects them. The bits of an encoded field are the bits that change
//! when its value changes. The bits of a rejected field are the bits left over for which the
//! decoder of the baseline header, with that bit flipped, fails naming the field. The result is
//! an error unless every bit belongs to exactly one field.
//!
//! Bits are numbered as the Blue Books number them: bit 0 is the most significant bit of the
//! first octet.

use std::collections::{BTreeMap, BTreeSet};

use aldis_core::FieldId;

/// What names a bit of a header that a decoder rejects: the field the error names, or the
/// reason a decoder gave when the bit changed how the header is read.
pub type Reject = Box<dyn Fn(&[u8]) -> Option<FieldId>>;

/// Encodes the header with one field set to the value; `None` when the encoder refuses it.
pub type Setter = Box<dyn Fn(u64) -> Option<Vec<u8>>>;

/// How the bits of a field are found.
pub enum How {
    /// From the encoder: the bits that change between the header with the field at `base` and
    /// the header with the field at another value that `set` accepts.
    Encode {
        /// A value of the field that `set` accepts, the reference for the changes.
        base: u64,
        /// The header with the field set to a value.
        set: Setter,
    },
    /// From the decoder: the bits of the baseline header that no encoded field owns and whose
    /// change the decoder rejects naming this field.
    Rejected,
    /// Computed by the encoder from the other fields, such as a check sequence: the bits from
    /// `from_bit` to the end of the header. Any encoded field may change them; at least one
    /// must change each.
    Derived {
        /// The first bit of the computed field.
        from_bit: usize,
    },
}

/// One field of a header.
pub struct Field {
    /// The name shown in the table.
    pub name: &'static str,
    /// The field of the catalogue (`aldis_core::field`), if it has a row there.
    pub id: Option<FieldId>,
    /// The clause the rustdoc of the field cites (RS-015).
    pub clause: &'static str,
    /// How the bits are found.
    pub how: How,
}

/// A header to probe.
pub struct Header {
    /// The title of the section.
    pub title: &'static str,
    /// One sentence on what is probed and how it is encoded.
    pub note: &'static str,
    /// The length of the header in bits.
    pub bits: usize,
    /// A header every decoder accepts, the reference for the rejected fields.
    pub baseline: Vec<u8>,
    /// The fields, in the order of the table.
    pub fields: Vec<Field>,
    /// The field a decoder names when it rejects or reads differently the header octets given.
    pub reject: Reject,
}

/// A field with the bits found for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    /// The name shown in the table.
    pub name: &'static str,
    /// The catalogue code and name, if the field has a row.
    pub id: Option<(u16, &'static str)>,
    /// The clause cited.
    pub clause: &'static str,
    /// The bits, in ascending order.
    pub bits: Vec<usize>,
}

/// The layout of one header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// The title of the section.
    pub title: &'static str,
    /// One sentence on what is probed.
    pub note: &'static str,
    /// The length in bits.
    pub bits: usize,
    /// The fields, in the order of the table.
    pub fields: Vec<Placed>,
}

/// The bits that differ between `a` and `b`, MSB first: bit 0 is the most significant bit of
/// the first octet. The octets beyond the shorter slice count as unequal.
pub fn changed_bits(a: &[u8], b: &[u8]) -> Vec<usize> {
    let mut out = Vec::new();
    for i in 0..a.len().max(b.len()) {
        let diff = a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0);
        let missing = a.get(i).is_none() || b.get(i).is_none();
        for j in 0..8_usize {
            if missing || diff & (0x80 >> j) != 0 {
                out.push(i * 8 + j);
            }
        }
    }
    out
}

/// `bytes` with bit `bit` flipped, numbered MSB first; unchanged if `bit` is out of range.
pub fn flip(bytes: &[u8], bit: usize) -> Vec<u8> {
    let mut out = bytes.to_vec();
    if let Some(octet) = out.get_mut(bit / 8) {
        *octet ^= 0x80 >> (bit % 8);
    }
    out
}

/// The runs of consecutive numbers in the ascending `bits`, as `(first, last)`.
pub fn ranges(bits: &[usize]) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    for &b in bits {
        match out.last_mut() {
            Some((_, last)) if *last + 1 == b => *last = b,
            _ => out.push((b, b)),
        }
    }
    out
}

/// Candidate values for a field with reference `base`: for each bit position, `base` with the
/// bit flipped, set, and cleared, `base` plus and minus the bit, and the bit alone.
fn candidates(base: u64) -> Vec<u64> {
    let mut out = BTreeSet::new();
    for k in 0..64 {
        let bit = 1_u64 << k;
        out.insert(base ^ bit);
        out.insert(base | bit);
        out.insert(base & !bit);
        out.insert(base.wrapping_add(bit));
        out.insert(base.wrapping_sub(bit));
        out.insert(bit);
    }
    out.remove(&base);
    out.into_iter().collect()
}

/// The bits an encoded field owns: the union of the changes against its reference header.
fn encoded_bits(
    header: &Header,
    field: &Field,
    base: u64,
    set: &Setter,
) -> Result<BTreeSet<usize>, String> {
    let at = format!("{}: {}", header.title, field.name);
    let reference = set(base).ok_or(format!("{at}: the encoder refuses the base value {base}"))?;
    if reference.len() != header.bits.div_ceil(8) {
        return Err(format!(
            "{at}: the encoder wrote {} octets, the header has {} bits",
            reference.len(),
            header.bits
        ));
    }
    let mut bits = BTreeSet::new();
    for v in candidates(base) {
        if let Some(octets) = set(v) {
            if octets.len() != reference.len() {
                return Err(format!(
                    "{at}: the length of the header changes with the value {v}"
                ));
            }
            bits.extend(changed_bits(&reference, &octets));
        }
    }
    if bits.is_empty() {
        return Err(format!("{at}: no accepted value changes a bit"));
    }
    Ok(bits)
}

/// Assigns the claims of the fields to the `total` bits: an error naming a bit that two fields
/// claim, a bit that none claims, a bit outside the header, and a field that claims no bit.
/// Each claim is a field name and its bits.
pub fn assign(
    total: usize,
    claims: &[(&'static str, BTreeSet<usize>)],
) -> Result<Vec<Vec<usize>>, String> {
    let mut owner: BTreeMap<usize, &'static str> = BTreeMap::new();
    for (name, bits) in claims {
        if bits.is_empty() {
            return Err(format!("field {name}: no bit"));
        }
        for &bit in bits {
            if bit >= total {
                return Err(format!(
                    "field {name}: bit {bit} is outside the {total}-bit header"
                ));
            }
            if let Some(first) = owner.insert(bit, name) {
                return Err(format!("bit {bit} belongs to both {first} and {name}"));
            }
        }
    }
    let missing: Vec<usize> = (0..total).filter(|b| !owner.contains_key(b)).collect();
    if !missing.is_empty() {
        let list: Vec<String> = ranges(&missing)
            .iter()
            .map(|(a, b)| {
                if a == b {
                    a.to_string()
                } else {
                    format!("{a}-{b}")
                }
            })
            .collect();
        return Err(format!("bit(s) {} belong to no field", list.join(", ")));
    }
    Ok(claims
        .iter()
        .map(|(_, bits)| bits.iter().copied().collect())
        .collect())
}

/// Probes `header`; the layout, or what is wrong with the header or the codec.
pub fn probe(header: &Header) -> Result<Layout, String> {
    let title = header.title;
    if let Some(id) = (header.reject)(&header.baseline) {
        return Err(format!(
            "{title}: the decoder rejects the baseline header, naming {}",
            id.name()
        ));
    }
    if let Some(f) = header
        .fields
        .iter()
        .find(|f| matches!(f.how, How::Rejected) && f.id.is_none())
    {
        return Err(format!(
            "{title}: field {}: a rejected field needs the field code the decoder names",
            f.name
        ));
    }
    let derived_from = header.fields.iter().find_map(|f| match f.how {
        How::Derived { from_bit } => Some(from_bit),
        How::Encode { .. } | How::Rejected => None,
    });
    let region: BTreeSet<usize> =
        derived_from.map_or_else(BTreeSet::new, |from| (from..header.bits).collect());
    // The encoded fields first; the bits of the derived field are not theirs.
    let mut claims: Vec<(&'static str, BTreeSet<usize>)> = Vec::new();
    let mut reached: BTreeSet<usize> = BTreeSet::new();
    let mut owned: BTreeSet<usize> = BTreeSet::new();
    for field in &header.fields {
        if let How::Encode { base, set } = &field.how {
            let raw = encoded_bits(header, field, *base, set)?;
            reached.extend(raw.intersection(&region));
            let own: BTreeSet<usize> = raw.difference(&region).copied().collect();
            owned.extend(&own);
            claims.push((field.name, own));
        }
    }
    if reached != region {
        return Err(format!(
            "{title}: a computed field has bits that no encoded field changes"
        ));
    }
    let mut all: Vec<(&'static str, BTreeSet<usize>)> = Vec::new();
    let mut next_encoded = claims.into_iter();
    for field in &header.fields {
        match &field.how {
            How::Encode { .. } => all.extend(next_encoded.next()),
            How::Rejected => {
                let mut bits = BTreeSet::new();
                for bit in (0..header.bits).filter(|b| !owned.contains(b) && !region.contains(b)) {
                    let flipped = flip(&header.baseline, bit);
                    if (header.reject)(&flipped).is_some_and(|id| Some(id) == field.id) {
                        bits.insert(bit);
                    }
                }
                all.push((field.name, bits));
            }
            How::Derived { .. } => {
                all.push((field.name, region.clone()));
            }
        }
    }
    let placed = assign(header.bits, &all).map_err(|e| format!("{title}: {e}"))?;
    Ok(Layout {
        title,
        note: header.note,
        bits: header.bits,
        fields: header
            .fields
            .iter()
            .zip(placed)
            .map(|(f, bits)| Placed {
                name: f.name,
                id: f.id.map(|id| (id.code().get(), id.name())),
                clause: f.clause,
                bits,
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::{Field, Header, How, assign, changed_bits, flip, probe, ranges};
    use std::collections::BTreeSet;

    fn set(bits: &[usize]) -> BTreeSet<usize> {
        bits.iter().copied().collect()
    }

    #[test]
    fn bits_are_numbered_from_the_msb_of_the_first_octet() {
        // req: VER-072
        assert_eq!(changed_bits(&[0x80, 0x00], &[0x00, 0x00]), vec![0]);
        assert_eq!(changed_bits(&[0x01, 0x00], &[0x00, 0x00]), vec![7]);
        assert_eq!(changed_bits(&[0x00, 0x80], &[0x00, 0x00]), vec![8]);
        assert_eq!(changed_bits(&[0x00, 0x01], &[0x00, 0x00]), vec![15]);
        assert_eq!(flip(&[0x00, 0x00], 0), vec![0x80, 0x00]);
        assert_eq!(flip(&[0x00, 0x00], 15), vec![0x00, 0x01]);
        assert_eq!(flip(&[0x00], 8), vec![0x00]);
        assert_eq!(
            ranges(&[2, 3, 4, 9, 11, 12]),
            vec![(2, 4), (9, 9), (11, 12)]
        );
    }

    #[test]
    fn two_fields_on_one_bit_fail() {
        // req: VER-072
        let claims = [("a", set(&[0, 1, 2])), ("b", set(&[2, 3]))];
        let e = assign(4, &claims).unwrap_err();
        assert!(e.contains("bit 2 belongs to both a and b"), "{e}");
    }

    #[test]
    fn a_bit_of_no_field_fails() {
        // req: VER-072
        let claims = [("a", set(&[0, 1])), ("b", set(&[4]))];
        let e = assign(5, &claims).unwrap_err();
        assert!(e.contains("bit(s) 2-3 belong to no field"), "{e}");
    }

    #[test]
    fn a_field_of_no_bit_or_a_bit_beyond_the_header_fails() {
        // req: VER-072
        let e = assign(2, &[("a", set(&[0, 1])), ("b", set(&[]))]).unwrap_err();
        assert!(e.contains("field b: no bit"), "{e}");
        let e = assign(2, &[("a", set(&[0, 1, 2]))]).unwrap_err();
        assert!(e.contains("outside"), "{e}");
    }

    #[test]
    fn a_full_assignment_lists_the_bits_of_each_field() {
        // req: VER-072
        let claims = [("a", set(&[0, 1, 5])), ("b", set(&[2, 3, 4]))];
        let placed = assign(6, &claims).unwrap();
        assert_eq!(placed, vec![vec![0, 1, 5], vec![2, 3, 4]]);
    }

    /// A one-octet header: a 3-bit field in bits 0-2 and a flag in bit 3; bits 4-7 are the
    /// caller's.
    fn toy(fields: Vec<Field>) -> Header {
        Header {
            title: "Toy",
            note: "",
            bits: 8,
            baseline: vec![0x20],
            fields,
            reject: Box::new(|_| None),
        }
    }

    fn x_field() -> Field {
        Field {
            name: "x",
            id: None,
            clause: "-",
            how: How::Encode {
                base: 1,
                set: Box::new(|v| {
                    u8::try_from(v)
                        .ok()
                        .filter(|v| *v < 8)
                        .map(|v| vec![v << 5])
                }),
            },
        }
    }

    fn flag_field() -> Field {
        Field {
            name: "flag",
            id: None,
            clause: "-",
            how: How::Encode {
                base: 0,
                set: Box::new(|v| (v < 2).then(|| vec![0x10 * u8::try_from(v).unwrap_or(0)])),
            },
        }
    }

    #[test]
    fn a_probed_header_with_a_gap_fails_loudly() {
        // req: VER-072
        let e = probe(&toy(vec![x_field(), flag_field()])).unwrap_err();
        assert!(e.contains("bit(s) 4-7 belong to no field"), "{e}");
    }

    #[test]
    fn a_probed_header_finds_each_field_where_the_encoder_puts_it() {
        // req: VER-072
        let mut h = toy(vec![x_field(), flag_field()]);
        h.bits = 4;
        let layout = probe(&h).unwrap();
        let bits: Vec<&[usize]> = layout.fields.iter().map(|f| f.bits.as_slice()).collect();
        assert_eq!(bits, [&[0, 1, 2][..], &[3][..]]);
    }

    #[test]
    fn overlapping_encoded_fields_fail() {
        // req: VER-072
        let mut again = x_field();
        again.name = "y";
        let mut h = toy(vec![x_field(), again]);
        h.bits = 3;
        let e = probe(&h).unwrap_err();
        assert!(e.contains("bit 0 belongs to both x and y"), "{e}");
    }

    #[test]
    fn a_field_that_accepts_no_value_fails() {
        // req: VER-072
        let dead = Field {
            name: "dead",
            id: None,
            clause: "-",
            how: How::Encode {
                base: 0,
                set: Box::new(|v| (v == 0).then(|| vec![0])),
            },
        };
        let e = probe(&toy(vec![dead])).unwrap_err();
        assert!(e.contains("no accepted value changes a bit"), "{e}");
    }

    #[test]
    fn a_rejected_field_without_a_field_code_fails() {
        // req: VER-072
        let mut h = checked(|v| v);
        for f in &mut h.fields {
            if matches!(f.how, How::Rejected) {
                f.id = None;
            }
        }
        let e = probe(&h).unwrap_err();
        assert!(e.contains("needs the field code"), "{e}");
    }

    /// A two-octet header: a 3-bit field `x` in bits 0-2 that sets the second octet as a check
    /// value of `check`, and bits 3-7 that the decoder rejects when they are not zero.
    fn checked(check: fn(u8) -> u8) -> Header {
        let rejected = Field {
            name: "fixed",
            id: Some(aldis_core::field::FRAME_VERSION),
            clause: "-",
            how: How::Rejected,
        };
        let derived = Field {
            name: "check",
            id: None,
            clause: "-",
            how: How::Derived { from_bit: 8 },
        };
        let x = Field {
            name: "x",
            id: None,
            clause: "-",
            how: How::Encode {
                base: 1,
                set: Box::new(move |v| {
                    u8::try_from(v)
                        .ok()
                        .filter(|v| *v < 8)
                        .map(|v| vec![v << 5, check(v)])
                }),
            },
        };
        Header {
            title: "Checked",
            note: "",
            bits: 16,
            baseline: vec![0x20, check(1)],
            fields: vec![x, rejected, derived],
            reject: Box::new(|h| {
                (h.first().copied().unwrap_or(0) & 0x1F != 0)
                    .then_some(aldis_core::field::FRAME_VERSION)
            }),
        }
    }

    #[test]
    fn a_fixed_field_takes_the_bits_the_decoder_rejects() {
        // req: VER-072
        let layout = probe(&checked(|v| v.wrapping_mul(37).wrapping_add(1))).unwrap();
        let bits: Vec<&[usize]> = layout.fields.iter().map(|f| f.bits.as_slice()).collect();
        assert_eq!(bits[0], [0, 1, 2]);
        assert_eq!(bits[1], [3, 4, 5, 6, 7]);
        assert_eq!(bits[2], (8..16).collect::<Vec<_>>());
    }

    #[test]
    fn a_computed_field_that_no_field_changes_fails() {
        // req: VER-072
        let e = probe(&checked(|_| 0)).unwrap_err();
        assert!(
            e.contains("computed field has bits that no encoded field changes"),
            "{e}"
        );
    }

    #[test]
    fn a_decoder_that_rejects_the_baseline_fails() {
        // req: VER-072
        let mut h = checked(|v| v);
        h.reject = Box::new(|_| Some(aldis_core::field::FRAME_VERSION));
        let e = probe(&h).unwrap_err();
        assert!(e.contains("rejects the baseline header"), "{e}");
    }
}

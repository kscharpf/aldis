//! Mirror enumerations: the one place where an enumeration value from C becomes a Rust value
//! (OI-57 convention 2; CABI-010, CABI-022).
//!
//! A value the caller passes in is a plain `uint8_t` or `uint32_t`; a C `enum` never reaches
//! Rust, because an out-of-range discriminant would be undefined behaviour. For each
//! enumeration this crate declares a fieldless `#[repr(u8)]` or `#[repr(u32)]` *mirror enum*
//! whose variants are named for the `ALDIS_*` constants they define, and lists them in
//! [`mirror!`]. cbindgen emits the variants as enumerators of an `enum` that no field of the
//! header uses, so a field stays a fixed-width integer; the enumerators are the constants of
//! the header. [`parse`] is the single parser: it finds the variant whose value equals the
//! argument, and fails with `InvalidField` when there is none (CABI-022). The caller then
//! converts the mirror enum to the core type with an exhaustive `match`, which the compiler
//! checks, so that a new variant of either enum cannot be forgotten. The same `match` in the
//! other direction gives the value of a field the library writes.

use aldis::core::Result;

use crate::abi::invalid_argument;

/// A mirror enum: a fieldless enum whose discriminants are the values the C side passes
/// (declared by [`mirror!`]).
pub(crate) trait Mirror: Copy + 'static {
    /// The integer type of the C field, `u8` or `u32`: the `repr` of the enum.
    type Repr: Copy + PartialEq;
    /// Every variant, in declaration order.
    const ALL: &'static [Self];
    /// The discriminant: the value of the `ALDIS_*` constant.
    fn code(self) -> Self::Repr;
}

/// The variant of `E` with the discriminant `value`; `InvalidField` naming no field when there
/// is none (CABI-022). The argument stays an integer until here (RS-020).
pub(crate) fn parse<E: Mirror>(value: E::Repr) -> Result<E> {
    E::ALL
        .iter()
        .copied()
        .find(|variant| variant.code() == value)
        .ok_or_else(invalid_argument)
}

/// Declares what a mirror enum needs, after the enum itself (cbindgen reads the enum, not the
/// macro): its [`Mirror`] implementation, and the Rust constant `ALDIS_*: repr` of each variant,
/// named as the variant with the visibility `$vis` of the enum. The `match` that names every variant makes the compiler reject a
/// variant missing from the list, and the unreachable-pattern lint one listed twice.
macro_rules! mirror {
    ($vis:vis $ty:ident: $repr:ident { $($name:ident),+ $(,)? }) => {
        $(
            #[doc = concat!("The value of [`", stringify!($ty), "::", stringify!($name), "`].")]
            $vis const $name: $repr = $ty::$name as $repr;
        )+

        impl $crate::mirror::Mirror for $ty {
            type Repr = $repr;
            const ALL: &'static [Self] = &[$($ty::$name),+];

            #[expect(
                clippy::as_conversions,
                reason = "the discriminant of a fieldless enum with a fixed `repr`"
            )]
            fn code(self) -> $repr {
                self as $repr
            }
        }

        const _: fn($ty) = |value| match value {
            $($ty::$name => ()),+
        };
    };
}

pub(crate) use mirror;

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::abi::aldis_mode;
    use crate::aos::{aldis_aos_content, aldis_aos_edition, aldis_aos_pointer};
    use crate::ocf::aldis_ocf_report;
    use crate::oid::aldis_oid_form;
    use crate::spp::{aldis_spp_packet_type, aldis_spp_secondary, aldis_spp_sequence_flags};
    use crate::spp_state::{aldis_spp_discard, aldis_spp_reassembly, aldis_spp_track};
    use crate::tc::{
        aldis_tc_control, aldis_tc_discard, aldis_tc_frame, aldis_tc_sequence_flags,
        aldis_tc_stream,
    };
    use crate::time::{
        aldis_time_ccs_variation, aldis_time_cds_day, aldis_time_cds_submillis, aldis_time_code,
    };
    use crate::tm::{aldis_tm_content, aldis_tm_pointer};
    use crate::uslp::{
        aldis_uslp_direction, aldis_uslp_discard, aldis_uslp_frame, aldis_uslp_kind,
        aldis_uslp_ocf, aldis_uslp_rule, aldis_uslp_security, aldis_uslp_stream,
    };

    /// Two values, `repr(u8)`.
    #[repr(u8)]
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[expect(non_camel_case_types, reason = "the variants are the C constants")]
    enum Pair {
        TEST_ONE = 3,
        TEST_TWO = 9,
    }
    mirror!(Pair: u8 { TEST_ONE, TEST_TWO });

    // req: CABI-010, CABI-022
    #[test]
    fn parse_finds_exactly_the_declared_values() {
        assert_eq!(TEST_ONE, 3);
        assert_eq!(TEST_TWO, 9);
        assert_eq!(parse::<Pair>(3), Ok(Pair::TEST_ONE));
        assert_eq!(parse::<Pair>(9), Ok(Pair::TEST_TWO));
        for value in 0..=u8::MAX {
            assert_eq!(
                parse::<Pair>(value).is_ok(),
                value == 3 || value == 9,
                "value {value}"
            );
        }
    }

    // req: CABI-022
    #[test]
    fn an_unknown_value_is_invalid_field() {
        use aldis::core::ErrorKind;
        assert_eq!(
            parse::<Pair>(4).unwrap_err().kind(),
            ErrorKind::InvalidField
        );
    }

    // req: CABI-010
    #[test]
    fn all_lists_the_variants_in_order() {
        assert_eq!(Pair::ALL, &[Pair::TEST_ONE, Pair::TEST_TWO]);
        assert_eq!(Pair::TEST_TWO.code(), 9);
    }
    // req: CABI-010, CABI-002
    #[test]
    fn every_mirror_enum_is_in_the_header_with_the_same_values() {
        // Each variant of a mirror enum, by `Debug` name and value, is an enumerator of the `enum` of
        // the same name in the generated header, and the header has no other enumerator.
        fn check<E: Mirror + core::fmt::Debug>(name: &str, header: &str)
        where
            E::Repr: Into<u64>,
        {
            let (_, after) = header
                .split_once(&format!("enum {name}\n"))
                .unwrap_or_else(|| panic!("`enum {name}` is not in the header"));
            let (body, _) = after.split_once("\n};").unwrap();
            let mut listed = Vec::new();
            for line in body.lines() {
                let line = line.trim();
                if line.starts_with("ALDIS_") {
                    let (id, value) = line.trim_end_matches(',').split_once(" = ").unwrap();
                    listed.push((id.to_owned(), value.parse::<u64>().unwrap()));
                }
            }
            let declared: Vec<(String, u64)> = E::ALL
                .iter()
                .map(|v| (format!("{v:?}"), v.code().into()))
                .collect();
            assert_eq!(declared, listed, "enum {name}");
        }

        let header = include_str!("../../../include/aldis/aldis.h");
        check::<aldis_aos_content>("aldis_aos_content", header);
        check::<aldis_aos_edition>("aldis_aos_edition", header);
        check::<aldis_aos_pointer>("aldis_aos_pointer", header);
        check::<aldis_mode>("aldis_mode", header);
        check::<aldis_ocf_report>("aldis_ocf_report", header);
        check::<aldis_oid_form>("aldis_oid_form", header);
        check::<aldis_spp_discard>("aldis_spp_discard", header);
        check::<aldis_spp_packet_type>("aldis_spp_packet_type", header);
        check::<aldis_spp_reassembly>("aldis_spp_reassembly", header);
        check::<aldis_spp_secondary>("aldis_spp_secondary", header);
        check::<aldis_spp_sequence_flags>("aldis_spp_sequence_flags", header);
        check::<aldis_spp_track>("aldis_spp_track", header);
        check::<aldis_tc_control>("aldis_tc_control", header);
        check::<aldis_tc_discard>("aldis_tc_discard", header);
        check::<aldis_tc_frame>("aldis_tc_frame", header);
        check::<aldis_tc_sequence_flags>("aldis_tc_sequence_flags", header);
        check::<aldis_tc_stream>("aldis_tc_stream", header);
        check::<aldis_time_ccs_variation>("aldis_time_ccs_variation", header);
        check::<aldis_time_cds_day>("aldis_time_cds_day", header);
        check::<aldis_time_cds_submillis>("aldis_time_cds_submillis", header);
        check::<aldis_time_code>("aldis_time_code", header);
        check::<aldis_tm_content>("aldis_tm_content", header);
        check::<aldis_tm_pointer>("aldis_tm_pointer", header);
        check::<aldis_uslp_direction>("aldis_uslp_direction", header);
        check::<aldis_uslp_discard>("aldis_uslp_discard", header);
        check::<aldis_uslp_frame>("aldis_uslp_frame", header);
        check::<aldis_uslp_kind>("aldis_uslp_kind", header);
        check::<aldis_uslp_ocf>("aldis_uslp_ocf", header);
        check::<aldis_uslp_rule>("aldis_uslp_rule", header);
        check::<aldis_uslp_security>("aldis_uslp_security", header);
        check::<aldis_uslp_stream>("aldis_uslp_stream", header);
    }
}

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
    use crate::error::aldis_status;
    use crate::farm1::{aldis_farm1_action, aldis_farm1_fdu, aldis_farm1_input};
    use crate::fop1::{
        aldis_fop1_action, aldis_fop1_alert, aldis_fop1_directive, aldis_fop1_frame,
        aldis_fop1_input, aldis_fop1_notice, aldis_fop1_timeout,
    };
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
        fn check<E: Mirror + core::fmt::Debug>(name: &str, header: &str, seen: &mut Vec<String>)
        where
            E::Repr: Into<u64>,
        {
            seen.push(name.to_owned());
            let (_, after) = header
                .split_once(&format!("\nenum {name}\n"))
                .unwrap_or_else(|| panic!("`enum {name}` is not in the header"));
            let mut listed = Vec::new();
            // The enumerators are the lines that start with `ALDIS_` up to the closing brace; the
            // brace style and the preprocessor lines of the fixed underlying type do not matter.
            for line in after.lines().skip_while(|l| !l.contains('{')) {
                let line = line.trim();
                if line.starts_with('}') {
                    break;
                }
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
        let mut seen = Vec::new();
        check::<aldis_aos_content>("aldis_aos_content", header, &mut seen);
        check::<aldis_aos_edition>("aldis_aos_edition", header, &mut seen);
        check::<aldis_aos_pointer>("aldis_aos_pointer", header, &mut seen);
        check::<aldis_farm1_action>("aldis_farm1_action", header, &mut seen);
        check::<aldis_farm1_fdu>("aldis_farm1_fdu", header, &mut seen);
        check::<aldis_farm1_input>("aldis_farm1_input", header, &mut seen);
        check::<aldis_fop1_action>("aldis_fop1_action", header, &mut seen);
        check::<aldis_fop1_alert>("aldis_fop1_alert", header, &mut seen);
        check::<aldis_fop1_directive>("aldis_fop1_directive", header, &mut seen);
        check::<aldis_fop1_frame>("aldis_fop1_frame", header, &mut seen);
        check::<aldis_fop1_input>("aldis_fop1_input", header, &mut seen);
        check::<aldis_fop1_notice>("aldis_fop1_notice", header, &mut seen);
        check::<aldis_fop1_timeout>("aldis_fop1_timeout", header, &mut seen);
        check::<aldis_mode>("aldis_mode", header, &mut seen);
        check::<aldis_ocf_report>("aldis_ocf_report", header, &mut seen);
        check::<aldis_oid_form>("aldis_oid_form", header, &mut seen);
        check::<aldis_spp_discard>("aldis_spp_discard", header, &mut seen);
        check::<aldis_spp_packet_type>("aldis_spp_packet_type", header, &mut seen);
        check::<aldis_spp_reassembly>("aldis_spp_reassembly", header, &mut seen);
        check::<aldis_spp_secondary>("aldis_spp_secondary", header, &mut seen);
        check::<aldis_spp_sequence_flags>("aldis_spp_sequence_flags", header, &mut seen);
        check::<aldis_spp_track>("aldis_spp_track", header, &mut seen);
        check::<aldis_status>("aldis_status", header, &mut seen);
        check::<aldis_tc_control>("aldis_tc_control", header, &mut seen);
        check::<aldis_tc_discard>("aldis_tc_discard", header, &mut seen);
        check::<aldis_tc_frame>("aldis_tc_frame", header, &mut seen);
        check::<aldis_tc_sequence_flags>("aldis_tc_sequence_flags", header, &mut seen);
        check::<aldis_tc_stream>("aldis_tc_stream", header, &mut seen);
        check::<aldis_time_ccs_variation>("aldis_time_ccs_variation", header, &mut seen);
        check::<aldis_time_cds_day>("aldis_time_cds_day", header, &mut seen);
        check::<aldis_time_cds_submillis>("aldis_time_cds_submillis", header, &mut seen);
        check::<aldis_time_code>("aldis_time_code", header, &mut seen);
        check::<aldis_tm_content>("aldis_tm_content", header, &mut seen);
        check::<aldis_tm_pointer>("aldis_tm_pointer", header, &mut seen);
        check::<aldis_uslp_direction>("aldis_uslp_direction", header, &mut seen);
        check::<aldis_uslp_discard>("aldis_uslp_discard", header, &mut seen);
        check::<aldis_uslp_frame>("aldis_uslp_frame", header, &mut seen);
        check::<aldis_uslp_kind>("aldis_uslp_kind", header, &mut seen);
        check::<aldis_uslp_ocf>("aldis_uslp_ocf", header, &mut seen);
        check::<aldis_uslp_rule>("aldis_uslp_rule", header, &mut seen);
        check::<aldis_uslp_security>("aldis_uslp_security", header, &mut seen);
        check::<aldis_uslp_stream>("aldis_uslp_stream", header, &mut seen);

        // Completeness: the checks above, the `enum`s of the header, and the `include` list of
        // `cbindgen.toml` name the same set, so a new mirror enum left out of any of them fails here.
        seen.sort();
        let mut in_header: Vec<String> = header
            .lines()
            .filter_map(|l| l.strip_prefix("enum aldis_"))
            .map(|n| format!("aldis_{}", n.trim()))
            .collect();
        in_header.sort();
        assert_eq!(seen, in_header, "enums of the header");
        let config = include_str!("../cbindgen.toml");
        let mut included: Vec<String> = config
            .lines()
            .filter_map(|l| l.trim().strip_prefix("\"aldis_"))
            .filter_map(|n| n.strip_suffix("\","))
            .map(|n| format!("aldis_{n}"))
            .collect();
        included.sort();
        assert_eq!(seen, included, "`include` of cbindgen.toml");
    }
}

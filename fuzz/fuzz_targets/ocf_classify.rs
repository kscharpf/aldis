//! `Ocf::decode` against an independent model of bits 0–1 (132.0 §4.1.5.4–5, 732.0 §4.1.5.4–5,
//! 732.1 §4.1.5.2.1–2): every four octets classify without panic, the classification follows
//! the Type Flag and bit 1, the octets come back unchanged, and exactly one report type's
//! checked constructor accepts them, the others failing with `InvalidField` and the bit they
//! need (CMN-010, CMN-011).
#![no_main]

// req: CMN-010, CMN-011, VER-022

use aldis_core::ErrorKind;
use aldis_core::field;
use aldis_core::ocf::{FsrReport, OCF_LEN, Ocf, ProjectSpecificReport, Type1Report};
use libfuzzer_sys::fuzz_target;

/// The model: 0 for a Type-1 Report, 2 for project-specific, 3 for an SDLS FSR.
const fn model(octets: [u8; OCF_LEN]) -> u8 {
    let word = u32::from_be_bytes(octets);
    let type_flag = word >> 31;
    let bit1 = (word >> 30) & 1;
    match (type_flag, bit1) {
        (0, _) => 0,
        (_, 0) => 2,
        _ => 3,
    }
}

fuzz_target!(|data: &[u8]| {
    for &octets in data.as_chunks::<OCF_LEN>().0 {
        let ocf = Ocf::decode(octets);
        assert_eq!(ocf.to_octets(), octets);
        assert_eq!(<[u8; OCF_LEN]>::from(ocf), octets);
        let kind = match ocf {
            Ocf::Clcw(r) => {
                assert_eq!(r.to_octets(), octets);
                0
            }
            Ocf::ProjectSpecific(r) => {
                assert_eq!(r.to_octets(), octets);
                2
            }
            Ocf::SdlsFsr(r) => {
                assert_eq!(r.to_octets(), octets);
                3
            }
        };
        assert_eq!(kind, model(octets));

        let type1 = Type1Report::try_from(octets);
        let project = ProjectSpecificReport::try_from(octets);
        let fsr = FsrReport::try_from(octets);
        assert_eq!(type1.ok().map(Ocf::from), (kind == 0).then_some(ocf));
        assert_eq!(project.ok().map(Ocf::from), (kind == 2).then_some(ocf));
        assert_eq!(fsr.ok().map(Ocf::from), (kind == 3).then_some(ocf));
        assert_eq!(Type1Report::new(octets).is_some(), kind == 0);
        assert_eq!(ProjectSpecificReport::new(octets).is_some(), kind == 2);
        assert_eq!(FsrReport::new(octets).is_some(), kind == 3);

        // A rejection names the first bit that disagrees.
        let named = |e: aldis_core::Error| {
            assert_eq!(e.kind(), ErrorKind::InvalidField);
            e.field()
        };
        if let Err(e) = type1 {
            assert_eq!(named(e), Some(field::OCF_TYPE_FLAG));
        }
        let wrong_type2 = if kind == 0 {
            field::OCF_TYPE_FLAG
        } else {
            field::OCF_REPORT_USE
        };
        if let Err(e) = project {
            assert_eq!(named(e), Some(wrong_type2));
        }
        if let Err(e) = fsr {
            assert_eq!(named(e), Some(wrong_type2));
        }
    }
});

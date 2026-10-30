//! Arbitrary CLCW words through `Clcw::decode`, against an independent model of 232.0 §4.2.1:
//! Control Word Type 1 and versions other than `00` fail in both modes; a spare bit fails in
//! strict mode and is an anomaly in lenient mode; every field is read from its bits; and the
//! decoded CLCW re-encodes to the word with its spares cleared (TC-030, TC-031, GEN-020).
#![no_main]

// req: TC-030, TC-031, API-031, API-032, GEN-020, VER-022

use aldis_core::{DecodeMode, ErrorKind};
use aldis_tc::{Clcw, TcAnomaly};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some(&word) = data.first_chunk::<4>() else {
        return;
    };
    let w = u32::from_be_bytes(word);
    let bits = |first: u32, width: u32| (w >> (32 - first - width)) & ((1 << width) - 1);
    let spares = bits(14, 2) != 0 || bits(23, 1) != 0;
    for mode in [DecodeMode::Strict, DecodeMode::Lenient] {
        let r = mode.dispatch(|| Clcw::decode(word), || Clcw::decode_lenient(word));
        if bits(0, 1) == 1 {
            assert_eq!(r.unwrap_err().kind(), ErrorKind::InvalidField);
        } else if bits(1, 2) != 0 {
            assert_eq!(r.unwrap_err().kind(), ErrorKind::UnsupportedVersion);
        } else if spares && mode == DecodeMode::Strict {
            assert_eq!(r.unwrap_err().kind(), ErrorKind::ReservedValue);
        } else {
            let (c, anomalies) = r.unwrap();
            assert_eq!(anomalies.contains(TcAnomaly::ClcwSpareSet), spares);
            assert_eq!(anomalies.len(), usize::from(spares));
            assert_eq!(u32::from(c.status.get()), bits(3, 3));
            assert_eq!(u32::from(c.cop_in_effect.get()), bits(6, 2));
            assert_eq!(u32::from(c.vc_id.get()), bits(8, 6));
            let flags = [
                c.no_rf_available,
                c.no_bit_lock,
                c.lockout,
                c.wait,
                c.retransmit,
            ];
            for (i, f) in (16..21).zip(flags) {
                assert_eq!(u32::from(f), bits(i, 1));
            }
            assert_eq!(u32::from(c.farm_b_counter.get()), bits(21, 2));
            assert_eq!(u32::from(c.report_value.get()), bits(24, 8));
            assert_eq!(c.to_word(), w & !0x0003_0100);
            assert_eq!(Clcw::decode(c.to_octets()).unwrap(), c);
        }
    }
});

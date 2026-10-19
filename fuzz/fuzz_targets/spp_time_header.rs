//! Time secondary headers from arbitrary input: the split never panics under any CUC or CDS
//! layout, with or without a P-field; a successful split's time code lies inside the secondary
//! header and re-encodes to the same octets, and a P-field always carries the configured time
//! code ID (SPP-008, OI-30).
#![no_main]

// req: SPP-008, SPP-007, GEN-020, VER-022

use aldis_core::DecodeMode;
use aldis_spp::{SecondaryHeaderLayout, SpacePacket, TimeHeaderLayout};
use aldis_time::{CdsDayLen, CdsEpoch, CdsFormat, CdsSubMs, CucEpoch, CucFormat, TimeFormat};
use libfuzzer_sys::fuzz_target;

fn format(selector: u8) -> TimeFormat {
    if selector & 0x80 == 0 {
        let epoch = if selector & 0x40 == 0 {
            CucEpoch::Tai1958
        } else {
            CucEpoch::Agency
        };
        let coarse = (selector & 0x07) % 7 + 1;
        let fine = (selector >> 3) & 0x07;
        TimeFormat::Cuc(CucFormat::new(epoch, coarse, fine).unwrap())
    } else {
        let subms = match selector & 0x03 {
            0 => CdsSubMs::None,
            1 => CdsSubMs::Micros,
            _ => CdsSubMs::Picos,
        };
        let day = if selector & 0x04 == 0 {
            CdsDayLen::Bits16
        } else {
            CdsDayLen::Bits24
        };
        let epoch = if selector & 0x08 == 0 {
            CdsEpoch::Ccsds1958
        } else {
            CdsEpoch::Agency
        };
        TimeFormat::Cds(CdsFormat::new(epoch, day, subms))
    }
}

fuzz_target!(|data: &[u8]| {
    let [selector, options, packet @ ..] = data else {
        return;
    };
    let fmt = format(*selector);
    let pfield = options & 1 != 0;
    let mode = if options & 2 == 0 {
        DecodeMode::Strict
    } else {
        DecodeMode::Lenient
    };
    let layout = TimeHeaderLayout::new(fmt, pfield, usize::from(options >> 4)).unwrap();
    let Ok(p) = SpacePacket::decode(packet, mode) else {
        return;
    };
    let Ok(split) = p.split(&SecondaryHeaderLayout::Time(layout)) else {
        return;
    };
    let time = split.time_code().unwrap();
    let header = split.secondary_header();
    assert!(p.has_secondary_header());
    assert_eq!(header.len() + split.user_data().len(), p.data_field().len());
    assert_eq!(header.len(), time.len() + layout.ancillary_len());
    assert_eq!(split.ancillary_data(), &header[time.len()..]);
    assert_eq!(time.pfield().is_some(), pfield);
    if let Some(pf) = time.pfield() {
        assert_eq!(pf.id(), fmt.pfield().id());
    }
    // The T-field re-encodes exactly; a received P-field may use a redundant extension octet,
    // so only the T-field is compared.
    let p_len = time.pfield().map_or(0, |pf| pf.len());
    let mut out = [0_u8; 32];
    let n = time.code().encode(false, &mut out).unwrap();
    assert_eq!(&out[..n], &header[p_len..time.len()]);
});

//! The facade feature `spp-time` makes the Space Packet time secondary header (133.0
//! §4.1.4.2.2, SPP-008) reachable through `aldis` without `full`.
#![cfg(all(feature = "spp-time", feature = "core"))]

use aldis::core::DecodeMode;
use aldis::spp::{SecondaryHeaderLayout, SpacePacket, TimeHeaderLayout};
use aldis::time::{CucEpoch, CucFormat, TimeCode, TimeFormat};

#[test]
fn time_secondary_header_through_the_facade() {
    // req: SPP-008
    // APID 0x123 with the Secondary Header Flag set, unsegmented, count 0; the data field is a
    // CUC 4+0 T-field (no P-field) of coarse time 0x0102_0304, then one user data octet.
    let packet = [
        0x09, 0x23, 0xC0, 0x00, 0x00, 0x04, 0x01, 0x02, 0x03, 0x04, 0xAA,
    ];
    let format = TimeFormat::Cuc(CucFormat::new(CucEpoch::Tai1958, 4, 0).unwrap());
    let layout = SecondaryHeaderLayout::Time(TimeHeaderLayout::new(format, false, 0).unwrap());
    let split = SpacePacket::decode(&packet, DecodeMode::Strict)
        .unwrap()
        .split(&layout)
        .unwrap();
    let time = split.time_code().unwrap();
    assert!(matches!(time.code(), TimeCode::Cuc(c) if c.value().coarse() == 0x0102_0304));
    assert_eq!(split.user_data(), &[0xAA][..]);
}

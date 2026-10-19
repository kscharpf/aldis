//! Calls every public `aldis-epp` API (VER-010, M1 D7). Inputs pass through `black_box`.

use core::hint::black_box as bb;

use aldis_core::{AnomalySet, DecodeMode, ErrorKind};
use aldis_epp::{
    EncapsulationFields, EncapsulationHeader, EncapsulationPacket, EncapsulationWriter, Epi,
    EpiExtension, EpiSet, EppAnomaly, EppConfig, ExtendedEpiSet, HeaderLen, HeaderSize,
    UserDefined, encode, idle,
};

use crate::exercise::Tally;

/// Managed parameters that accept every EPI, extended EPI, and data unit length.
fn any_config() -> Option<EppConfig> {
    let epis = (0..=7).fold(EpiSet::EMPTY, |s, e| {
        s.with(Epi::new(e).unwrap_or_default())
    });
    let ext = (0..=15).fold(ExtendedEpiSet::EMPTY, |s, e| {
        s.with(EpiExtension::new(e).unwrap_or_default())
    });
    EppConfig::new(0, aldis_epp::config::MAX_DATA_LEN, epis, ext).ok()
}

fn ids_and_header(t: &mut Tally) {
    let ext = EpiExtension::new(bb(9)).unwrap_or_default();
    let user = UserDefined::try_from(bb(5_u8)).unwrap_or_default();
    t.check(Epi::new(bb(8)).is_none() && Epi::try_from(bb(9_u8)).is_err());
    t.check(u8::from(ext) == 9 && user.get() == 5 && UserDefined::new(bb(16)).is_none());
    t.check(EpiExtension::try_from(bb(16_u8)).is_err() && u8::from(Epi::MISSION_SPECIFIC) == 7);
    t.check(Epi::IDLE.is_idle() && Epi::EXTENDED.get() == 6 && Epi::MAX == 7);
    for lol in 0..4_u8 {
        let len = HeaderLen::from_length_of_length(bb(lol));
        t.check(len.length_of_length() == lol && len.octets() >= 1 && len.max_packet_len() >= 1);
        t.check(len.has_extension() == (lol >= 2));
    }
    let fields = EncapsulationFields::extended(bb(ext)).with_user_defined(bb(user));
    t.check(
        fields.epi() == Epi::EXTENDED && fields.extension() == ext && fields.user_defined() == user,
    );
    t.check(EncapsulationFields::from_parts(Epi::MISSION_SPECIFIC, bb(ext), user).is_err());
    t.check(EncapsulationFields::from_parts(Epi::EXTENDED, bb(ext), user) == Ok(fields));
    let Some(any) = any_config() else {
        t.check(false);
        return;
    };
    let header = any.header(bb(fields), bb(HeaderSize::Auto), bb(3));
    t.check(header.is_ok_and(|h| {
        h.header_len() == HeaderLen::Four
            && h.epi() == Epi::EXTENDED
            && h.extension() == Some(ext)
            && h.user_defined() == Some(user)
            && h.ccsds_defined().is_none()
            && h.packet_len() == 7
            && h.data_len() == 3
    }));
    let mut buf = [0_u8; 8];
    t.check(header.and_then(|h| h.encode(bb(&mut buf))) == Ok(4) && buf[..4] == [0xFA, 0x59, 0, 7]);
    t.check(
        EncapsulationHeader::idle(bb(70_000)).is_ok_and(|h| h.header_len() == HeaderLen::Eight),
    );
    t.check(EncapsulationHeader::idle(bb(0)).is_err());
    let e = any.header(
        EncapsulationFields::new(Epi::MISSION_SPECIFIC),
        HeaderSize::Fixed(HeaderLen::Two),
        bb(usize::MAX),
    );
    t.check(e.is_err_and(|e| e.kind() == ErrorKind::OutOfRange));
}

fn view_and_writers(t: &mut Tally) {
    let bytes = [0xE7, 0x01, 0x00, 0x01, 0, 0, 0, 9, 0xAB, 0xFF];
    let strict = EncapsulationPacket::decode(bb(&bytes), DecodeMode::Strict);
    t.check(strict.is_err_and(|e| e.kind() == ErrorKind::ReservedValue));
    match EncapsulationPacket::decode(bb(&bytes), bb(DecodeMode::Lenient)) {
        Ok(p) => {
            t.check(p.len() == 9 && !p.is_empty() && !p.is_idle() && p.data() == [0xAB]);
            t.check(p.epi().get() == 1 && p.extension().map(EpiExtension::get) == Some(1));
            t.check(
                p.user_defined().map(UserDefined::get) == Some(0) && p.ccsds_defined() == Some(1),
            );
            t.check(p.header_len() == HeaderLen::Eight && p.header().packet_len() == 9);
            let expected = AnomalySet::from_flags(&[
                EppAnomaly::NonZeroEpiExtension,
                EppAnomaly::NonZeroCcsdsDefined,
            ]);
            t.check(p.anomalies() == expected);
            let cfg = EppConfig::new(
                bb(1),
                bb(4),
                EpiSet::EMPTY.with(Epi::new(1).unwrap_or_default()),
                ExtendedEpiSet::default(),
            );
            t.check(cfg.is_ok_and(|c| p.validate(&c).is_ok()));
        }
        Err(_) => t.check(false),
    }
    t.check(EncapsulationPacket::peek_length(bb(&bytes)) == Ok(9));
    t.check(EncapsulationPacket::peek_length(bb(&bytes[..3])).is_err());

    let mission = EncapsulationFields::new(Epi::MISSION_SPECIFIC);
    let Some(any) = any_config() else {
        t.check(false);
        return;
    };
    let mut out = [0_u8; 16];
    match any.data_unit(bb(mission), bb(&[1, 2])) {
        Ok(unit) => {
            t.check(unit.fields() == mission && unit.data() == [1, 2]);
            t.check(encode(&unit, HeaderSize::Auto, &mut out) == Ok(4));
        }
        Err(_) => t.check(false),
    }
    t.check(out[..4] == [0xFD, 4, 1, 2]);
    let big = any.data_unit(bb(mission), bb(&[0; 20]));
    t.check(big.is_ok_and(|unit| encode(&unit, HeaderSize::Auto, &mut out).is_err()));
    t.check(any.data_unit(bb(mission), bb(&[])).is_err());
    t.check(idle(bb(5), bb(0x55), &mut out) == Ok(5) && out[..5] == [0xE1, 5, 0x55, 0x55, 0x55]);
    let mut two = [0_u8; 16];
    match EncapsulationWriter::begin(&mut two, &any, bb(mission), HeaderSize::Auto, bb(4)) {
        Ok(mut w) => {
            if let Some(d) = w.data_mut().first_mut() {
                *d = bb(0x42);
            }
            t.check(w.finish(bb(1)) == Ok(3));
        }
        Err(_) => t.check(false),
    }
    t.check(two[..3] == [0xFD, 3, 0x42]);
    let narrow = EppConfig::new(bb(2), bb(3), EpiSet::EMPTY, ExtendedEpiSet::EMPTY);
    t.check(narrow.is_ok_and(|c| {
        EncapsulationWriter::begin(&mut two, &c, mission, HeaderSize::Auto, 3).is_err()
    }));
}

fn config(t: &mut Tally) {
    let ext = EpiExtension::new(bb(3)).unwrap_or_default();
    let set = EpiSet::EMPTY.with(Epi::EXTENDED);
    let xset = ExtendedEpiSet::EMPTY.with(ext);
    t.check(set.contains(Epi::EXTENDED) && set.bits() == 0x40 && xset.bits() == 0x08);
    t.check(xset.contains(ext) && !EpiSet::default().contains(Epi::IDLE));
    match EppConfig::new(bb(0), bb(8), set, xset) {
        Ok(c) => {
            t.check(c.min_data_len() == 0 && c.max_data_len() == 8);
            t.check(c.valid_epis() == set && c.valid_extended_epis() == xset);
            t.check(c.check(Epi::EXTENDED, Some(ext), bb(8)).is_ok());
            t.check(c.check(Epi::EXTENDED, None, bb(8)).is_err());
            t.check(c.check(Epi::MISSION_SPECIFIC, None, bb(1)).is_err());
            t.check(c.check(Epi::IDLE, None, bb(1000)).is_ok());
        }
        Err(_) => t.check(false),
    }
    t.check(EppConfig::new(bb(9), bb(8), set, xset).is_err());
}

/// Runs the `aldis-epp` group.
pub fn run(t: &mut Tally) {
    ids_and_header(t);
    view_and_writers(t);
    config(t);
}

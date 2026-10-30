//! A buffer of concatenated packets decoded one after another: every packet starts where the
//! previous one ended, each consumes exactly its length, and the walk ends at the end of the
//! buffer or at the first error, never past it (SPP-002, API-011, GEN-020).
#![no_main]

// req: SPP-002, API-011, GEN-020, VER-022

use aldis_core::{DecodeMode, ErrorKind};
use aldis_spp::SpacePacket;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&selector, stream)) = data.split_first() else {
        return;
    };
    let mode = if selector & 1 == 0 {
        DecodeMode::Strict
    } else {
        DecodeMode::Lenient
    };
    let mut at = 0;
    while let Some(rest) = stream.get(at..).filter(|r| !r.is_empty()) {
        match mode
            .dispatch(
                || SpacePacket::decode(rest),
                || SpacePacket::decode_lenient(rest),
            )
            .map(|(p, _)| p)
        {
            Ok(p) => {
                assert!(p.len() >= 7 && p.len() <= rest.len());
                assert_eq!(p.header().packet_len(), p.len());
                at += p.len();
            }
            Err(e) => {
                // A truncated tail asks for more than is left.
                if e.kind() == ErrorKind::Incomplete {
                    assert!(
                        SpacePacket::peek_length(rest).map_or(rest.len() < 6, |n| n > rest.len())
                    );
                }
                break;
            }
        }
    }
    assert!(at <= stream.len());
});

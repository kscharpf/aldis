//! The Attached Sync Marker and the CADU attach function (131.0 §9.2.1, §9.3.1 Figure 9-1,
//! §9.1.2 Table 9-1, §3.4.1 d; SYNC-001, SYNC-003, SYNC-006).
//!
//! A Channel Access Data Unit is the ASM followed by the (randomized) Transfer Frame
//! (SYNC-003). The marker is attached after the frame has been randomized, never before: the
//! caller calls [`randomize`](crate::randomize) and then [`attach_asm`] (SYNC-006).

use aldis_core::{Error, ErrorKind, Result, field};

use crate::config::SyncConfig;

/// The length of the ASM in octets (131.0 §9.3.1; SYNC-001).
pub const ASM_LEN: usize = 4;

/// The ASM, `1ACFFC1D` in hexadecimal: the octets `1A CF FC 1D`, each sent most significant bit
/// first, the first octet first (131.0 §9.3.1, Figure 9-1; SYNC-001).
///
/// 131.0 §9 also calls it the Code Synchronization Marker (CSM); with no code and no slicing it
/// is the same marker (§9.2.2).
pub const ASM: [u8; ASM_LEN] = [0x1A, 0xCF, 0xFC, 0x1D];

/// Writes the CADU of `frame` to `out` and returns its length, 4 + the frame length.
///
/// The CADU is the [`ASM`], then the frame octets as given, with nothing between and no change
/// to the frame (131.0 §9.2.1, §9.1.2 Table 9-1, §3.4.1 d; SYNC-003).
///
/// `frame` is the frame as it goes on the channel: randomized by the caller when the channel
/// randomizes (SYNC-006).
///
/// # Errors
/// `LengthMismatch` naming `frame_len` when the frame is not `config.frame_len()` octets;
/// `BufferTooSmall{needed}` when `out` is shorter than the CADU. Nothing is written on error
/// (SYNC-003).
pub fn attach_asm(config: &SyncConfig, frame: &[u8], out: &mut [u8]) -> Result<usize> {
    if frame.len() != config.frame_len() {
        return Err(Error::new(ErrorKind::LengthMismatch).with_field(field::FRAME_LEN));
    }
    let needed = ASM_LEN.saturating_add(frame.len());
    let Some(area) = out.get_mut(..needed) else {
        return Err(Error::buffer_too_small(needed));
    };
    // The ASM, then the frame: `area` is exactly as long as the two together.
    for (d, s) in area.iter_mut().zip(ASM.iter().chain(frame)) {
        *d = *s;
    }
    Ok(needed)
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{ASM, ASM_LEN, attach_asm};
    use crate::config::{AsmErrors, SyncConfig, SyncProtocol};
    use aldis_core::{ErrorDetail, ErrorKind, field};

    fn config(len: usize) -> SyncConfig {
        SyncConfig::new(SyncProtocol::Tm, len, AsmErrors::NONE).unwrap()
    }

    #[test]
    fn the_marker_is_1acffc1d() {
        // req: SYNC-001
        assert_eq!(u32::from_be_bytes(ASM), 0x1ACF_FC1D);
        assert_eq!(ASM.len(), ASM_LEN);
    }

    #[test]
    fn attach_writes_the_marker_then_the_frame() {
        // req: SYNC-003
        let mut out = [0xEE; 8];
        assert_eq!(attach_asm(&config(3), &[1, 2, 3], &mut out), Ok(7));
        assert_eq!(out, [0x1A, 0xCF, 0xFC, 0x1D, 1, 2, 3, 0xEE]);
    }

    #[test]
    fn attach_errors_write_nothing() {
        // req: SYNC-003
        let mut out = [0xEE; 6];
        for frame in [&[1, 2][..], &[1, 2, 3, 4][..], &[][..]] {
            let e = attach_asm(&config(3), frame, &mut out).unwrap_err();
            assert_eq!(e.kind(), ErrorKind::LengthMismatch);
            assert_eq!(e.field(), Some(field::FRAME_LEN));
        }
        let e = attach_asm(&config(3), &[1, 2, 3], &mut out).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::BufferTooSmall);
        assert_eq!(e.detail(), ErrorDetail::Needed(7));
        assert_eq!(out, [0xEE; 6]);
    }
}

//! The `Plain` proof: a frame whose SDLS processing is done (RS-023, API-017, API-073, API-074).
//!
//! A frame is `Received` (the octets), then decoded and FECF-checked (a [`Checked`] view when it
//! also matches the managed parameters of its channel), then `Plain`. `Plain<F>` and
//! [`Checked<F>`](crate::checked::Checked) are orthogonal wrappers of a frame `F`, next to
//! [`Lenient`](crate::mode::Lenient): a receiving engine's frame input takes `Plain<F>` and
//! FARM-1's intake takes a checked frame, and `Plain<Checked<F>>` is both.
//!
//! [`Plain`] has no public constructor. A value of it is made only by
//! [`SecuritySlices::into_plain`], which needs a frame with no Security Header or Trailer to
//! take through [`NoSecurity`]. [`SecuritySlices`] has no type parameter, so a frame type has
//! one implementation, and only the crate that defines the frame can write it (the orphan
//! rule: a foreign trait for a foreign type with no local type in it is refused, E0117): no
//! other crate can make a `Plain` codec frame without the codec's own accessors. The
//! compile-fail example on `aldis_tm`'s implementation shows the attack. The SDLS `verify` step
//! of a channel that has SDLS is issue #230.

use core::ops::Deref;

use crate::checked::Checked;
use crate::crc::{verify_fecf, write_fecf};
use crate::error::{Error, ErrorDetail, ErrorKind, Result};
use crate::field;
use crate::finishing::{require_layout, require_unsecured};
use crate::security::{FrameCtx, NoSecurity, SecuredFields, SecurityHook, SecurityLayout};

/// A frame `F` whose SDLS processing is done: the frame of a channel with no Security Header or
/// Trailer, taken through [`NoSecurity`] (355.0 §2.3; RS-023, API-074).
///
/// [`SecuritySlices::into_plain`] returns it. It dereferences to the frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plain<F> {
    frame: F,
}

impl<F> Plain<F> {
    /// The frame, without the proof that its SDLS processing is done.
    #[must_use]
    pub fn into_frame(self) -> F {
        self.frame
    }

    /// The frame, by reference; usable where `Deref` is not applied implicitly.
    #[must_use]
    pub const fn as_frame(&self) -> &F {
        &self.frame
    }

    /// Verifies an SDLS-protected frame in place with `hook`, then decodes the verified buffer
    /// into a `Plain<F>` proof (355.0 §2.3, §4.2.4; API-074, RS-023).
    ///
    /// When `fecf` is `true`, the wire FECF is checked first (`verify_fecf`, API-074), then
    /// `hook.verify(ctx, buf, fields)` authenticates and decrypts the data field in place, and
    /// the FECF in `buf` is refreshed over the decrypted bytes so `decode(buf)` succeeds with the
    /// channel's strict FECF check. A [`NoSecurity`] hook is rejected for any non-zero Security
    /// Header or Trailer.
    ///
    /// # Errors
    /// `CrcMismatch` if `fecf` is `true` and the wire FECF fails; `InvalidConfig` if `fields`
    /// exceeds `SecurityLayout` limits; `InvalidField` if `hook.layout()` or the decoded frame's
    /// security lengths do not match `fields`; `Security` if `hook.verify` rejects the frame; or
    /// any error returned by `decode`.
    pub fn verify_with<'a, H, D>(
        hook: &mut H,
        ctx: &FrameCtx,
        buf: &'a mut [u8],
        fields: &SecuredFields,
        fecf: bool,
        decode: D,
    ) -> Result<Self>
    where
        F: SecuritySlices,
        H: SecurityHook,
        D: FnOnce(&'a [u8]) -> Result<F>,
    {
        let expected = verify_buffer(hook, *ctx, buf, fields, fecf)?;
        let frame = decode(buf)?;
        check_decoded_security(
            frame.security_header().len(),
            frame.security_trailer().len(),
            expected,
        )?;
        Ok(Self { frame })
    }
}

fn verify_buffer(
    hook: &mut dyn SecurityHook,
    ctx: FrameCtx,
    buf: &mut [u8],
    fields: &SecuredFields,
    fecf: bool,
) -> Result<SecurityLayout> {
    let expected = SecurityLayout::new(fields.header().len(), fields.trailer().len())?;
    require_layout(hook.layout(), expected.header_len(), expected.trailer_len())?;
    if fecf {
        verify_fecf(buf)?;
    }
    hook.verify(&ctx, buf, fields)?;
    if fecf {
        // `verify_fecf` above already proved `buf.len() >= 2`.
        let _ = write_fecf(buf);
    }
    Ok(expected)
}

#[expect(
    clippy::as_conversions,
    reason = "usize widens losslessly to u64 on 32- and 64-bit targets"
)]
const fn usize_to_u64(v: usize) -> u64 {
    v as u64
}

const fn check_decoded_security(
    header_len: usize,
    trailer_len: usize,
    expected: SecurityLayout,
) -> Result<()> {
    if header_len != expected.header_len() {
        return Err(Error::new(ErrorKind::InvalidField)
            .with_field(field::SECURITY_HEADER_LEN)
            .with_detail(ErrorDetail::Expected {
                expected: usize_to_u64(expected.header_len()),
                found: usize_to_u64(header_len),
            }));
    }
    if trailer_len != expected.trailer_len() {
        return Err(Error::new(ErrorKind::InvalidField)
            .with_field(field::SECURITY_TRAILER_LEN)
            .with_detail(ErrorDetail::Expected {
                expected: usize_to_u64(expected.trailer_len()),
                found: usize_to_u64(trailer_len),
            }));
    }
    Ok(())
}

impl<F> Deref for Plain<F> {
    type Target = F;

    fn deref(&self) -> &F {
        &self.frame
    }
}

/// A frame that is both checked and plain gives up its channel check where only the SDLS proof
/// matters, such as a receiving engine's frame input (RS-023, API-016, API-017).
///
/// Only the [`Checked`] proof is dropped: the result has the same [`Plain`] proof as the
/// original, so no proof is made, only forgotten.
impl<F> From<Plain<Checked<F>>> for Plain<F> {
    fn from(plain: Plain<Checked<F>>) -> Self {
        Self {
            frame: plain.frame.into_frame(),
        }
    }
}

/// A frame that exposes the Security Header and Trailer of its channel (355.0 §2.3; RS-023,
/// API-070, API-074).
///
/// A codec implements the two accessors for its frame; callers use
/// [`into_plain`](Self::into_plain), the only way to a [`Plain`] frame. The trait has no type
/// parameter, so a frame type has at most one implementation, and only the crate that defines
/// the frame can write it (see the module docs). [`Checked<F>`] implements it when `F` does, so
/// `into_plain` takes a frame or a checked frame alike.
pub trait SecuritySlices: Sized {
    /// The Security Header octets: empty on a channel with no SDLS.
    fn security_header(&self) -> &[u8];

    /// The Security Trailer octets: empty on a channel with no SDLS.
    fn security_trailer(&self) -> &[u8];

    /// The decoded-view to `Plain` step for a channel with no SDLS: `hook` is [`NoSecurity`], the
    /// identity, so the frame must have no Security Header or Trailer (RS-023, API-070,
    /// API-074). A channel with SDLS verifies the frame with its `SecurityHook` before the
    /// engine sees it, which v1 does not provide for the engine input (issue #230).
    ///
    /// # Errors
    /// `InvalidField` (`security_header_len`, or `security_trailer_len`) if the frame has a
    /// Security Header or Trailer: [`NoSecurity`] cannot verify it.
    fn into_plain(self, _hook: NoSecurity) -> Result<Plain<Self>> {
        require_unsecured(self.security_header().len(), self.security_trailer().len())?;
        Ok(Plain { frame: self })
    }
}

impl<F: SecuritySlices> SecuritySlices for Checked<F> {
    fn security_header(&self) -> &[u8] {
        self.as_frame().security_header()
    }

    fn security_trailer(&self) -> &[u8] {
        self.as_frame().security_trailer()
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{Plain, SecuritySlices};
    use crate::checked::{Check, Checked, Rejection};
    use crate::error::{ErrorDetail, ErrorKind};
    use crate::field;
    use crate::security::NoSecurity;

    /// A frame of a header and a trailer.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct Demo(&'static [u8], &'static [u8]);

    impl SecuritySlices for Demo {
        fn security_header(&self) -> &[u8] {
            self.0
        }

        fn security_trailer(&self) -> &[u8] {
            self.1
        }
    }

    impl Check for Demo {
        type Reason = ();
        type Lookup = ();

        fn verify(&self, (): &()) -> Result<(), Rejection<()>> {
            Ok(())
        }
    }

    #[test]
    fn a_frame_without_sdls_becomes_plain() {
        // req: RS-023, API-017, API-074
        let plain: Plain<Demo> = Demo(&[], &[]).into_plain(NoSecurity).unwrap();
        assert_eq!(plain.security_header(), []);
        assert_eq!(plain.as_frame(), &Demo(&[], &[]));
        assert_eq!(plain.into_frame(), Demo(&[], &[]));
    }

    #[test]
    fn a_checked_frame_becomes_plain_and_keeps_its_proof() {
        // req: RS-023, API-016, API-074
        let checked = Demo(&[], &[]).check(&()).unwrap();
        let plain: Plain<Checked<Demo>> = checked.into_plain(NoSecurity).unwrap();
        assert_eq!(plain.into_frame().into_frame(), Demo(&[], &[]));
    }

    #[test]
    fn a_checked_plain_frame_gives_up_the_check_and_keeps_plain() {
        // req: RS-023, API-016, API-017
        let both: Plain<Checked<Demo>> = Demo(&[], &[])
            .check(&())
            .unwrap()
            .into_plain(NoSecurity)
            .unwrap();
        let plain: Plain<Demo> = both.into();
        assert_eq!(plain.into_frame(), Demo(&[], &[]));
    }

    #[test]
    fn a_frame_with_sdls_fields_does_not() {
        // req: RS-023, API-070
        let e = Demo(&[1], &[]).into_plain(NoSecurity).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::InvalidField);
        let e = Demo(&[], &[1]).into_plain(NoSecurity).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::InvalidField);
        let checked = Demo(&[1], &[]).check(&()).unwrap();
        assert!(checked.into_plain(NoSecurity).is_err());
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "full verify_with error-path test matrix"
    )]
    fn verify_with_checks_fecf_and_hook() {
        // req: RS-023, API-070, API-074
        use crate::crc::{verify_fecf, write_fecf};
        use crate::ids::{SpacecraftId, VcId};
        use crate::security::{FrameCtx, HookLayout, SecuredFields, SecurityHook, SecurityLayout};

        struct FlipHook;
        impl SecurityHook for FlipHook {
            fn layout(&self) -> HookLayout {
                HookLayout::Exact(SecurityLayout::new(2, 2).unwrap())
            }
            fn protect(
                &mut self,
                _: &FrameCtx,
                _: &mut [u8],
                _: &SecuredFields,
            ) -> crate::Result<()> {
                Ok(())
            }
            fn verify(
                &mut self,
                _: &FrameCtx,
                frame: &mut [u8],
                fields: &SecuredFields,
            ) -> crate::Result<core::ops::Range<usize>> {
                if frame.first() == Some(&0xFF) {
                    return Err(crate::Error::new(ErrorKind::Security));
                }
                for b in frame.get_mut(fields.data()).unwrap_or_default() {
                    *b ^= 0xAA;
                }
                Ok(fields.data())
            }
        }

        let ctx = FrameCtx::Tm {
            scid: SpacecraftId::<10>::new(1).unwrap(),
            vcid: VcId::<3>::new(0).unwrap(),
        };
        let fields = SecuredFields::adjacent(2, 2, 2, 2);
        let mut buf = [0x01, 0x02, 0x10, 0x20, 0xAB, 0xCD, 0x30, 0x40, 0, 0];
        write_fecf(&mut buf).unwrap();

        // Single closure type so `verify_with` is monomorphized once across all outcomes.
        let decode_outcome = core::cell::Cell::new(Ok(Demo(&[1, 2], &[3, 4])));
        let decode_fn = |b: &[u8]| {
            if b.get(8..10) != Some(&[0, 0]) {
                let _ = verify_fecf(b);
            }
            decode_outcome.get()
        };

        // NoSecurity is rejected on non-zero security layout.
        assert_eq!(
            Plain::verify_with(&mut NoSecurity, &ctx, &mut buf, &fields, true, decode_fn)
                .unwrap_err()
                .kind(),
            ErrorKind::InvalidField
        );

        // Bad wire FECF fails before hook runs.
        buf[9] ^= 1;
        assert_eq!(
            Plain::verify_with(&mut FlipHook, &ctx, &mut buf, &fields, true, decode_fn)
                .unwrap_err()
                .kind(),
            ErrorKind::CrcMismatch
        );
        buf[9] ^= 1;

        // Valid wire FECF + hook verify succeeds (with fecf = true).
        let p =
            Plain::verify_with(&mut FlipHook, &ctx, &mut buf, &fields, true, decode_fn).unwrap();
        assert_eq!(p.as_frame(), &Demo(&[1, 2], &[3, 4]));

        // Decoded header/trailer length mismatch is rejected with distinct field and detail
        // (with fecf = false).
        decode_outcome.set(Ok(Demo(&[1], &[3, 4])));
        let hdr_err = Plain::verify_with(&mut FlipHook, &ctx, &mut buf, &fields, false, decode_fn)
            .unwrap_err();
        assert_eq!(hdr_err.kind(), ErrorKind::InvalidField);
        assert_eq!(hdr_err.field(), Some(field::SECURITY_HEADER_LEN));
        assert_eq!(
            hdr_err.detail(),
            ErrorDetail::Expected {
                expected: 2,
                found: 1,
            }
        );

        decode_outcome.set(Ok(Demo(&[1, 2], &[3])));
        let trl_err = Plain::verify_with(&mut FlipHook, &ctx, &mut buf, &fields, false, decode_fn)
            .unwrap_err();
        assert_eq!(trl_err.kind(), ErrorKind::InvalidField);
        assert_eq!(trl_err.field(), Some(field::SECURITY_TRAILER_LEN));
        assert_eq!(
            trl_err.detail(),
            ErrorDetail::Expected {
                expected: 2,
                found: 1,
            }
        );

        // Decode error propagates.
        decode_outcome.set(Err(crate::Error::new(ErrorKind::Malformed)));
        assert_eq!(
            Plain::<Demo>::verify_with(&mut FlipHook, &ctx, &mut buf, &fields, false, decode_fn)
                .unwrap_err()
                .kind(),
            ErrorKind::Malformed
        );

        // Hook error propagates.
        decode_outcome.set(Ok(Demo(&[1, 2], &[3, 4])));
        buf[0] = 0xFF;
        assert_eq!(
            Plain::verify_with(&mut FlipHook, &ctx, &mut buf, &fields, false, decode_fn)
                .unwrap_err()
                .kind(),
            ErrorKind::Security
        );

        // Header > MAX_HEADER_LEN fails SecurityLayout::new.
        let huge = SecuredFields::adjacent(0, 65, 1, 0);
        assert_eq!(
            Plain::verify_with(&mut FlipHook, &ctx, &mut buf, &huge, false, decode_fn)
                .unwrap_err()
                .kind(),
            ErrorKind::InvalidConfig
        );
    }
}

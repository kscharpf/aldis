//! A decoded frame checked against the configured channels (732.0-B-5 §4.3.5.4, §4.3.6.3;
//! AOS-010, AOS-016, API-016).

use aldis_core::{Check, Error, ErrorKind, Rejection};

use crate::config::{VcLookup, unknown_channel_field};
use crate::field;
use crate::view::AosFrame;

/// Rejects a frame whose channel the `VcLookup` it was decoded with did not hold (732.0
/// §4.3.5.4, §4.3.6.3; AOS-010, AOS-016, API-016, RS-023): `frame.check(&vcs)` returns the
/// others as an `aldis_core::Checked<AosFrame>`, which dereferences to the frame. Holding one
/// is the proof that the frame's Spacecraft ID (an OID frame) or Spacecraft ID and VCID were
/// configured, so the data field, OCF, and SDLS fields were read as the channel says. `vcs`
/// must be the lookup the frame was decoded with.
///
/// The decoder reads such a frame as opaque octets with no OCF and no SDLS fields so that an
/// engine can count it; the Blue Book discards it.
///
/// The lookup is a `dyn VcLookup`, so the trait has no type parameter and only this crate can
/// implement `Check` for `AosFrame` (see `aldis_core::checked`); `&vcs` of any sized lookup
/// coerces to it.
///
/// # Errors
/// `UnknownChannel` (`spacecraft_id`) for a Spacecraft ID that is not valid on the physical
/// channel, and (`vc_id`) for a VCID of a valid Spacecraft ID that is not configured.
impl Check for AosFrame<'_> {
    type Reason = ();
    type Lookup = dyn VcLookup;

    fn verify(&self, vcs: &dyn VcLookup) -> Result<(), Rejection<()>> {
        let header = self.header();
        let (scid, vcid) = (header.scid(), header.vcid());
        let unknown = if header.is_oid() {
            (!vcs.has_spacecraft(scid)).then_some(field::SPACECRAFT_ID)
        } else {
            vcs.vc(scid, vcid)
                .is_none()
                .then(|| unknown_channel_field(vcs, scid))
        };
        match unknown {
            Some(field) => Err(Error::new(ErrorKind::UnknownChannel)
                .with_field(field)
                .into()),
            None => Ok(()),
        }
    }
}

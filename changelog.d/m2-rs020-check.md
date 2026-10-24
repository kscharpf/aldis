### Added
- `xtask`: `cargo xtask rs020`, part of `cargo xtask ci` and the verify check `spec`, flags every bare
  integer in a public field or enum payload of a codec crate unless `xtask/rs020-exemptions.toml`
  exempts it with a reason, and every exemption without a reason or that matches nothing (RS-020,
  M2 plan WP-7).
- `aldis-aos`: `PacketOffset`, the M_PDU packet offset of 0 to `0xFFFD`, with `new`, `get`, and
  `TryFrom<u16>` (RS-020).
- `aldis-uslp`: `TfdzOffset`, the offset of 0 to `0xFFFE` that the First Header and Last Valid
  Octet Pointers hold below `POINTER_NONE`, with `new`, `get`, and `TryFrom<u16>` (RS-020).

### Changed
- **Breaking:** `aldis-aos`: `PacketPointer::StartsAt` holds a `PacketOffset` instead of a `u16`, so
  an offset at a reserved pointer value can no longer be built (RS-020).
- **Breaking:** `aldis-uslp`: `FirstHeaderPointer::PacketStartsAt` and `LastValidOctet::At` hold a
  `TfdzOffset` instead of a `u16`, so an offset of `0xFFFF`, which `raw` wrote as "none", can no
  longer be built (RS-020).

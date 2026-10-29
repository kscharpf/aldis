# aldis-tc

Frame codec for the CCSDS 232.0-B-4 TC Space Data Link Protocol: TC Transfer Frames, the Segment Header, Control Commands, the CLCW, frame delimiting, and frame validation.

**Status:** implemented in Milestone 2 ([roadmap](../../docs/08-roadmap.md) §1, [M2 plan](../../docs/milestones/m2-frame-codecs.md) WP-3), modules 1–7: the channel configuration and its `VcLookup` (`config`), the zero-copy decoder (`view`), the one-shot and two-phase writers (`writer`), Control Commands (`control`), lenient-mode anomalies (`anomaly`), the CLCW (`clcw`), Frame Delimiting and Fill Removal (`delimit`), and the Frame Validation Checks (`validate`); module 8, its C ABI, is in `aldis-ffi` (`aldis_tc_*`, `aldis_clcw_*`, [README](../aldis-ffi/README.md)). `TcFrame::peek_header` reads the Primary Header alone, for example to find the Spacecraft ID of a frame on a physical channel that carries several master channels.

## Example

```rust
use aldis_core::DecodeMode;
use aldis_core::ids::FrameSeqNum;
use aldis_core::security::SecurityLayout;
use aldis_tc::{
    FrameData, FrameDelimiter, FrameFields, FrameType, PhysicalChannelConfig, SegmentHeaderUse,
    TcSpacecraftId, TcVcId, VcConfig, VcTable, encode, validate_frame,
};

let scid = TcSpacecraftId::new(0x155).unwrap();
let vcid = TcVcId::new(1).unwrap();
let vc = VcConfig::new(scid, vcid, 1024, SegmentHeaderUse::Absent(SecurityLayout::NONE)).unwrap();
let channels = VcTable::<1, 0>::new(PhysicalChannelConfig::new(1024, true).unwrap(), &[vc], &[]).unwrap();

// A Type-AD frame with three octets of User Data; the Frame Length and FECF are computed.
let fields = FrameFields {
    frame_type: FrameType::Ad,
    spacecraft_id: scid,
    vc_id: vcid,
    sequence_number: FrameSeqNum::new(0),
};
let mut unit = [0x55_u8; 16];
let data = FrameData::Fdu { segment_header: None, user_data: &[1, 2, 3] };
let n = encode(&channels, fields, data, &mut unit).unwrap();
assert_eq!(n, 10);

// At the receiving end: delimit the data unit, then validate each frame (232.0 §4.4.9).
let mut frames = FrameDelimiter::new(&unit);
let frame = frames.next().unwrap();
let tc = validate_frame(frame.octets(), &channels, DecodeMode::Strict).unwrap();
assert_eq!(tc.data_field(), [1, 2, 3]);
assert_eq!(frames.count(), 1); // the six octets of 0x55 fill
```

## Verification

- Unit tests in each module; the shared vectors of [`test-vectors/tc/`](../../test-vectors/tc/), whose expected values come from the independent model [`test-vectors/models/tc_frame.py`](../../test-vectors/models/tc_frame.py), with one frame and one CLCW worked by hand in [docs/verification/derivations/tc-frame.md](../../docs/verification/derivations/tc-frame.md).
- Property tests in [`tests/properties.rs`](tests/properties.rs) drawn from the shared strategies of `aldis-testkit` (`strategies-tc`); the fuzz targets `tc_decode`, `tc_roundtrip`, `tc_delimit`, and `tc_clcw`; the exerciser on three bare-metal targets; 100 % coverage (VER-024); the [GEN-022/023 analysis record](../../docs/verification/records/GEN-022.md).

## Where it sits

Layer 1 (codecs) of the [architecture](../../docs/01-architecture.md) §2. It depends only on `aldis-core`. The COP-1 engines in `aldis-cop1` build on it; TM, AOS, and USLP views return the CLCW as an opaque `aldis_core::ocf::Ocf`, which `Clcw::decode_report` decodes (CMN-011).

## Specification

- [docs/standards/tc-frame.md](../../docs/standards/tc-frame.md): requirement area TC, the check order and validation reason codes (§4.1), the delimiter (§4.2), and the anomaly flags (§6).
- [docs/standards/common-link-features.md](../../docs/standards/common-link-features.md): the FECF, counters, and field placement, implemented in `aldis-core`.
- [docs/09-open-issues.md](../../docs/09-open-issues.md): OI-41 (empty User Data, and the delimiter's valid header).
- [docs/README.md](../../docs/README.md): the index of the specification.

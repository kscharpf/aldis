# aldis-tc

Frame codec for the CCSDS 232.0-B-4 TC Space Data Link Protocol: TC Transfer Frames, the Segment Header, Control Commands, the CLCW, frame delimiting, and frame validation.

## Example

```rust
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
let mut frames = FrameDelimiter::new(&unit, 6);
let frame = frames.next().unwrap();
let tc = validate_frame(frame.octets(), &channels).unwrap();
assert_eq!(tc.data_field(), [1, 2, 3]);
assert_eq!(frames.count(), 1); // the six octets of 0x55 fill
```

## Where it sits

Layer 1 (codecs) of the [architecture](../../docs/01-architecture.md) §2. It depends only on `aldis-core`. The COP-1 engines in `aldis-cop1` build on it; TM, AOS, and USLP views return the CLCW as an opaque `aldis_core::ocf::Ocf`, which `Clcw::decode_report` decodes (CMN-011).

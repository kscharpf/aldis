# aldis-cop1

The CCSDS 232.1-B-2 Communications Operation Procedure-1 as sans-I/O engines: FOP-1 at the sending end and FARM-1 at the receiving end, one instance of each per Virtual Channel.

```rust
use aldis_cop1::{Farm1, Farm1Config, Farm1Input, Farm1Window};
use aldis_core::{DecodeMode, ids::FrameSeqNum, security::SecurityLayout, time::Instant};
use aldis_tc::{
    ClcwStatus, FrameData, FrameFields, FrameType, PhysicalChannelConfig, SegmentHeaderUse,
    TcSpacecraftId, TcVcId, VcConfig, VcTable, check_frame, encode,
};

# fn main() -> aldis_core::Result<()> {
let scid = TcSpacecraftId::new(0x155).expect("SCID");
let vc = TcVcId::new(1).expect("VCID 1");
let pc = PhysicalChannelConfig::new(1024, true)?;
let vcc = VcConfig::new(scid, vc, 1024, SegmentHeaderUse::Absent(SecurityLayout::NONE))?;
let table = VcTable::<1, 0>::new(pc, &[vcc], &[])?;
let mut wire = [0_u8; 32];
let n = encode(
    &table,
    FrameFields {
        frame_type: FrameType::Ad,
        spacecraft_id: scid,
        vc_id: vc,
        sequence_number: FrameSeqNum::new(0),
    },
    FrameData::Fdu { segment_header: None, user_data: &[0xA1, 0xB2] },
    &mut wire,
)?;
let checked = check_frame(&wire[..n], &table, DecodeMode::Strict).expect("valid frame").into_view();

let config = Farm1Config {
    vc_id: vc,
    status: ClcwStatus::new(0).expect("status 0"),
    window: Farm1Window::retransmission_allowed(16)?,
};
let farm = Farm1::new(config, FrameSeqNum::new(0));
let input = Farm1Input::from_tc(&checked, true);
let mut out = [0_u8; 16];
let (farm, actions) = farm.step(input, Instant::from_micros(0), &mut out)?;
assert_eq!(farm.clcw().report_value, FrameSeqNum::new(1));
assert_eq!(actions.len(), 1); // Deliver: the FDU is in `out`
# Ok(())
# }
```

## Status

FARM-1 (M3 WP-3) and FOP-1 (M3 WP-4) are implemented for TC frames, FOP-1 builds USLP frames with the feature `uslp` (M3 WP-5b), and a closed-loop test connects the two engines across a simulated link (M3 WP-5); the C ABI (WP-6) is not yet built.

## Where it sits

Layer 2 (engines) of the [architecture](../../docs/01-architecture.md) §2. It depends on `aldis-core` and `aldis-tc`, and on `aldis-uslp` with the feature `uslp`. As a library, the application calls the engines directly; the planned framework (`aldis-framework`, deferred, no crate yet) is to use the same engines.

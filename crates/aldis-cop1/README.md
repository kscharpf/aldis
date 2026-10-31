# aldis-cop1

The CCSDS 232.1-B-2 Communications Operation Procedure-1 as sans-I/O engines: FOP-1 at the sending end and FARM-1 at the receiving end, one instance of each per Virtual Channel.

```rust
use aldis_cop1::{Farm1, Farm1Config, Farm1Frame, Farm1Input, Farm1Window};
use aldis_core::{ids::FrameSeqNum, time::Instant};
use aldis_tc::{ClcwStatus, TcVcId};

# fn main() -> aldis_core::Result<()> {
let config = Farm1Config {
    vc_id: TcVcId::new(1).expect("VCID 1"),
    status: ClcwStatus::new(0).expect("status 0"),
    window: Farm1Window::retransmission_allowed(16)?,
};
let farm = Farm1::new(config, FrameSeqNum::new(0));
let fdu = [0xA1, 0xB2];
// A frame that passed `validate_frame` becomes the input with `Farm1Input::from_tc(&valid, true)`,
// which takes its VCID; from raw fields the caller names the VCID, and FARM-1 refuses another
// Virtual Channel than its own with `UnknownChannel`.
let input = Farm1Input::Frame {
    vc_id: TcVcId::new(1).expect("VCID 1"),
    frame: Farm1Frame::Ad { sequence_number: FrameSeqNum::new(0), data: &fdu },
    buffer_available: true,
};
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

Layer 2 (engines) of the [architecture](../../docs/01-architecture.md) §2. It depends on `aldis-core` and `aldis-tc`, and on `aldis-uslp` with the feature `uslp`. As a library, the application calls the engines directly; `aldis-framework` uses the same engines.

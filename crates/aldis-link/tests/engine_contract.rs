//! Engines are plain data that can move between threads and have no interior mutability
//! (GEN-033, RS-013). A type with a `Cell` or `RefCell` is not `Sync`, and one with a raw pointer
//! is neither `Send` nor `Sync`; the assertions below fail to compile for either. The crate
//! forbids `unsafe` (GEN-070), so no manual `Send` or `Sync` implementation can override them.

use aldis_link::access::AccessGenerator;
use aldis_link::bitstream::BitstreamGenerator;
use aldis_link::cltu::CltuPacker;
use aldis_link::extractor::PacketExtractor;
use aldis_link::generator::PacketGenerator;
use aldis_link::mux::ChannelMux;
use aldis_link::protocol::{Aos, Tc, Tm, Uslp};
use aldis_link::reassembler::MapReassembler;
use aldis_link::receiver::FrameReceiver;
use aldis_link::segmenter::MapSegmenter;

/// `T` can move to another thread, can be shared by reference (so it holds no `Cell`), and owns
/// its data.
const fn plain_engine<T: Send + Sync + Clone + 'static>() {}

#[test]
fn engines_move_between_threads_and_have_no_interior_mutability() {
    // req: GEN-033, RS-013
    plain_engine::<PacketGenerator<Tm, 24, 64>>();
    plain_engine::<PacketGenerator<Aos, 24, 64>>();
    plain_engine::<PacketGenerator<Uslp, 24, 64>>();
    plain_engine::<AccessGenerator<Aos, 64>>();
    plain_engine::<AccessGenerator<Uslp, 64>>();
    plain_engine::<BitstreamGenerator<Aos, 64>>();
    plain_engine::<ChannelMux<Tm, 4, 64>>();
    plain_engine::<ChannelMux<Aos, 4, 64>>();
    plain_engine::<ChannelMux<Uslp, 4, 64>>();
    plain_engine::<MapSegmenter<Tc, 4, 128>>();
    plain_engine::<MapSegmenter<Uslp, 4, 128>>();
    plain_engine::<CltuPacker<Tc, 64>>();
    plain_engine::<CltuPacker<Uslp, 64>>();
    plain_engine::<FrameReceiver<Tm, 8>>();
    plain_engine::<FrameReceiver<Aos, 8>>();
    plain_engine::<FrameReceiver<Tc, 8>>();
    plain_engine::<FrameReceiver<Uslp, 8>>();
    plain_engine::<PacketExtractor<Tm, 100, 24>>();
    plain_engine::<MapReassembler<Tc, 4, 64>>();
    plain_engine::<MapReassembler<Uslp, 4, 64>>();
}

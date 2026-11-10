//! The frame stream: delimiting, fill removal, and channel checking of a received data unit
//! (TC-020, TC-021, USLP-014, USLP-015, API-016, API-030, API-032).
//!
//! [`FrameStream`] is an in-place, allocation-free iterator over the octets of one clean data
//! unit (such as a CLTU's decoded codeblocks). It yields each frame as an
//! [`IntakeItem::Accepted`] that passed the checking pipeline of [`crate::checked`], and each
//! stretch of octets that is no frame as an [`IntakeItem::Discarded`] with the protocol's
//! discard reason. [`LenientFrameStream`] is its lenient twin (API-030).
//!
//! A [`Delimiter`] `D` carries what differs between protocols: the frame type, the discard
//! reason enum, the rule that reads the header at the start of the unit ([`Delimiter::peek`]) and
//! the rule for a frame that declares more octets than remain ([`Delimiter::incomplete`]). The
//! order of the steps is the common one: a header that declares a frame is cut and checked,
//! and whatever starts no valid header is Fill Data or, when longer than the coding sublayer's
//! longest fill, leftover fill (232.0 §4.4.9.2.6 note 2, 732.1 §4.3.11.2.4 note 2; OI-47).

use core::fmt;
use core::iter::FusedIterator;
use core::marker::PhantomData;
use core::num::NonZeroUsize;

use crate::checked::{CheckBytes, Checked, check_bytes_matched};
use crate::error::{Error, ErrorKind};
use crate::field::{FRAME_DATA_FIELD, FRAME_LEN};
use crate::lookup::{ChannelLookup, MapLookup, PhysicalLookup};
use crate::mode::{DecodeMode, Lenient};

/// What [`Delimiter::peek`] finds at the start of the unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Peek<'a, R> {
    /// A valid header declares a frame of this many octets.
    Frame(NonZeroUsize),
    /// The first octets are discarded with a reason and the stream goes on after them.
    Discard {
        /// The discarded octets and the octets after them.
        cut: Cut<'a>,
        /// The reason for discard.
        reason: R,
        /// The error detail of the discard.
        error: Error,
    },
    /// The whole rest of the unit is discarded with a reason.
    DiscardRest {
        /// The reason for discard.
        reason: R,
        /// The error detail of the discard.
        error: Error,
    },
    /// No valid header: the rest of the unit is Fill Data, or leftover fill if longer than the
    /// longest fill.
    Fill,
}

impl<R> Peek<'_, R> {
    /// A frame of `len` octets, or [`Peek::Fill`] if `len` is zero, which no header declares.
    #[must_use]
    pub const fn frame(len: usize) -> Self {
        match NonZeroUsize::new(len) {
            Some(len) => Peek::Frame(len),
            None => Peek::Fill,
        }
    }
}

/// A non-empty stretch of octets cut from the front of a slice, and the octets after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cut<'a> {
    octets: &'a [u8],
    rest: &'a [u8],
}

impl<'a> Cut<'a> {
    /// Cuts the first `len` octets of `whole`; `None` if `whole` holds fewer.
    #[must_use]
    pub fn new(whole: &'a [u8], len: NonZeroUsize) -> Option<Self> {
        let (octets, rest) = whole.split_at_checked(len.get())?;
        Some(Cut { octets, rest })
    }

    /// The cut octets, never empty.
    #[must_use]
    pub const fn octets(&self) -> &'a [u8] {
        self.octets
    }
}

/// The protocol side of a [`FrameStream`] (TC-020, USLP-014).
pub trait Delimiter {
    /// The frame the stream yields.
    type Frame<'a>: CheckBytes<
            'a,
            Reason = Self::Rejected,
            Anomaly = Self::Anomaly,
            Matched = (Self::Vc, Option<Self::Map>),
            Scid = Self::Scid,
            VcId = Self::VcId,
            MapId = Self::MapId,
            Vc = Self::Vc,
            Map = Self::Map,
            Physical = Self::Physical,
        >;
    /// The anomaly flags of the frame's lenient mode.
    type Anomaly: crate::anomaly::Anomaly;
    /// The reason type of the frame's checks.
    type Rejected: Copy;
    /// The stream's discard reason: the delimiting causes and the reasons of the checks.
    type Reason: Copy + From<Self::Rejected>;
    /// The Spacecraft ID type of the standard.
    type Scid: Copy;
    /// The Virtual Channel ID type of the standard.
    type VcId: Copy;
    /// The MAP ID type of the standard.
    type MapId: Copy;
    /// What a virtual channel carries for the decoder.
    type Vc;
    /// What a MAP carries for the decoder.
    type Map;
    /// The configuration of the Physical Channel.
    type Physical;

    /// The reason for octets discarded as Fill Data.
    const FILL: Self::Reason;
    /// The reason for octets discarded as leftover fill.
    const LEFTOVER_FILL: Self::Reason;

    /// Reads the header at the start of `rest`, which is not empty.
    fn peek<'a, L>(rest: &'a [u8], lookup: &L) -> Peek<'a, Self::Reason>
    where
        L: ChannelLookup<Self::Scid, Self::VcId, Vc = Self::Vc>
            + MapLookup<Self::Scid, Self::VcId, Self::MapId, Map = Self::Map>
            + PhysicalLookup<Physical = Self::Physical>
            + ?Sized;

    /// The discard of `rest`, which starts with a header that declares `declared_len` octets
    /// but holds fewer; `None` if the rest is treated as fill.
    fn incomplete<L>(
        rest: &[u8],
        declared_len: usize,
        lookup: &L,
        mode: DecodeMode,
    ) -> Option<(Self::Reason, Error)>
    where
        L: ChannelLookup<Self::Scid, Self::VcId, Vc = Self::Vc>
            + MapLookup<Self::Scid, Self::VcId, Self::MapId, Map = Self::Map>
            + PhysicalLookup<Physical = Self::Physical>
            + ?Sized;
}

/// A frame of a stream item: its octets (TC-020, USLP-014).
pub trait StreamFrame<'a> {
    /// The octets of the frame.
    fn octets(&self) -> &'a [u8];
}

impl<'a, F: CheckBytes<'a>> StreamFrame<'a> for Checked<F> {
    fn octets(&self) -> &'a [u8] {
        self.as_frame().octets()
    }
}

impl<'a, F: CheckBytes<'a>, A: crate::anomaly::Anomaly> StreamFrame<'a> for Lenient<Checked<F>, A> {
    fn octets(&self) -> &'a [u8] {
        self.as_frame().octets()
    }
}

/// An item yielded by [`FrameStream`] or [`LenientFrameStream`] (TC-020, USLP-014, API-030).
///
/// `F` is the accepted frame: a [`Checked`] frame from the strict stream, and the same in a
/// [`Lenient`] from the lenient one.
pub enum IntakeItem<'a, D: Delimiter, F = Checked<<D as Delimiter>::Frame<'a>>> {
    /// A checked frame, bundled with its matched Virtual Channel and MAP configurations.
    Accepted {
        /// The checked frame view.
        frame: F,
        /// The verified Virtual Channel configuration.
        vc: D::Vc,
        /// The verified MAP configuration, if the frame names one.
        map: Option<D::Map>,
    },
    /// Discarded wire octets with the reason and underlying error.
    Discarded {
        /// The reason for discard.
        reason: D::Reason,
        /// The discarded octets.
        octets: &'a [u8],
        /// The error detail of the rejection.
        error: Error,
    },
}

impl<D: Delimiter, F: fmt::Debug> fmt::Debug for IntakeItem<'_, D, F>
where
    D::Vc: fmt::Debug,
    D::Map: fmt::Debug,
    D::Reason: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IntakeItem::Accepted { frame, vc, map } => f
                .debug_struct("Accepted")
                .field("frame", frame)
                .field("vc", vc)
                .field("map", map)
                .finish(),
            IntakeItem::Discarded {
                reason,
                octets,
                error,
            } => f
                .debug_struct("Discarded")
                .field("reason", reason)
                .field("octets", octets)
                .field("error", error)
                .finish(),
        }
    }
}

impl<D: Delimiter, F: Clone> Clone for IntakeItem<'_, D, F>
where
    D::Vc: Clone,
    D::Map: Clone,
{
    fn clone(&self) -> Self {
        match self {
            IntakeItem::Accepted { frame, vc, map } => IntakeItem::Accepted {
                frame: frame.clone(),
                vc: vc.clone(),
                map: map.clone(),
            },
            IntakeItem::Discarded {
                reason,
                octets,
                error,
            } => IntakeItem::Discarded {
                reason: *reason,
                octets,
                error: *error,
            },
        }
    }
}

impl<D: Delimiter, F: PartialEq> PartialEq for IntakeItem<'_, D, F>
where
    D::Vc: PartialEq,
    D::Map: PartialEq,
    D::Reason: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                IntakeItem::Accepted { frame, vc, map },
                IntakeItem::Accepted {
                    frame: f2,
                    vc: v2,
                    map: m2,
                },
            ) => (frame, vc, map) == (f2, v2, m2),
            (
                IntakeItem::Discarded {
                    reason,
                    octets,
                    error,
                },
                IntakeItem::Discarded {
                    reason: r2,
                    octets: o2,
                    error: e2,
                },
            ) => (reason, octets, error) == (r2, o2, e2),
            _ => false,
        }
    }
}

impl<D: Delimiter, F: Eq> Eq for IntakeItem<'_, D, F>
where
    D::Vc: Eq,
    D::Map: Eq,
    D::Reason: Eq,
{
}

impl<'a, D: Delimiter, F> IntakeItem<'a, D, F> {
    /// The item with its accepted frame converted by `convert`; a discarded item is unchanged.
    pub fn map_frame<G>(self, convert: impl FnOnce(F) -> G) -> IntakeItem<'a, D, G> {
        match self {
            IntakeItem::Accepted { frame, vc, map } => IntakeItem::Accepted {
                frame: convert(frame),
                vc,
                map,
            },
            IntakeItem::Discarded {
                reason,
                octets,
                error,
            } => IntakeItem::Discarded {
                reason,
                octets,
                error,
            },
        }
    }
}

impl<'a, D: Delimiter, F: StreamFrame<'a>> IntakeItem<'a, D, F> {
    /// The octets of the item (the accepted frame or discarded octets).
    #[must_use]
    pub fn octets(&self) -> &'a [u8] {
        match self {
            IntakeItem::Accepted { frame, .. } => frame.octets(),
            IntakeItem::Discarded { octets, .. } => octets,
        }
    }

    /// Length in octets consumed from the input stream.
    #[must_use]
    pub fn len(&self) -> usize {
        self.octets().len()
    }

    /// Always `false`: every intake item consumes at least one octet.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        false
    }
}

/// The lenient item of a delimiter `D`: its accepted frame carries the anomalies tolerated.
pub type LenientItem<'a, D> =
    IntakeItem<'a, D, Lenient<Checked<<D as Delimiter>::Frame<'a>>, <D as Delimiter>::Anomaly>>;

/// In-place zero-allocation iterator over a received data unit, in strict mode (TC-020,
/// TC-021, USLP-014, USLP-015).
///
/// Fuses delimiting, fill removal, and channel validation in a single pass.
/// [`LenientFrameStream`] is its lenient twin (API-030).
pub struct FrameStream<'a, 'c, D, L: ?Sized> {
    rest: &'a [u8],
    channels: &'c L,
    max_fill: usize,
    protocol: PhantomData<D>,
}

/// The lenient twin of [`FrameStream`]: the frames it accepts are [`Lenient`] and carry the
/// anomalies of the deviations tolerated (API-030, API-032).
pub struct LenientFrameStream<'a, 'c, D, L: ?Sized> {
    rest: &'a [u8],
    channels: &'c L,
    max_fill: usize,
    protocol: PhantomData<D>,
}

macro_rules! stream_common {
    ($name:ident) => {
        impl<D, L: ?Sized> fmt::Debug for $name<'_, '_, D, L> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_struct(stringify!($name))
                    .field("rest", &self.rest)
                    .field("max_fill", &self.max_fill)
                    .finish_non_exhaustive()
            }
        }

        impl<D, L: ?Sized> Clone for $name<'_, '_, D, L> {
            fn clone(&self) -> Self {
                $name {
                    rest: self.rest,
                    channels: self.channels,
                    max_fill: self.max_fill,
                    protocol: PhantomData,
                }
            }
        }

        impl<'a, 'c, D, L: ?Sized> $name<'a, 'c, D, L> {
            /// Creates a stream over one clean data unit (such as a CLTU's decoded
            /// codeblocks), where `max_fill` is the longest fill the coding sublayer appends
            /// (six octets for the BCH codeblock; seven or 31 for the USLP LDPC codeblocks;
            /// 232.0 §4.4.9.2.6 note 2, 732.1 §4.3.11.2.4 note 2; OI-47).
            #[must_use]
            pub const fn new(data_unit: &'a [u8], channels: &'c L, max_fill: usize) -> Self {
                $name {
                    rest: data_unit,
                    channels,
                    max_fill,
                    protocol: PhantomData,
                }
            }

            /// The remaining unconsumed octets of the data unit.
            #[must_use]
            pub const fn remaining(&self) -> &'a [u8] {
                self.rest
            }
        }
    };
}

stream_common!(FrameStream);
stream_common!(LenientFrameStream);

impl<'a, D: Delimiter, L> Iterator for FrameStream<'a, '_, D, L>
where
    L: ChannelLookup<D::Scid, D::VcId, Vc = D::Vc>
        + MapLookup<D::Scid, D::VcId, D::MapId, Map = D::Map>
        + PhysicalLookup<Physical = D::Physical>
        + ?Sized,
{
    type Item = IntakeItem<'a, D>;

    fn next(&mut self) -> Option<Self::Item> {
        step::<D, L>(
            &mut self.rest,
            self.channels,
            self.max_fill,
            DecodeMode::Strict,
        )
        .map(|item| item.map_frame(Lenient::into_view))
    }
}

impl<'a, D: Delimiter, L> Iterator for LenientFrameStream<'a, '_, D, L>
where
    L: ChannelLookup<D::Scid, D::VcId, Vc = D::Vc>
        + MapLookup<D::Scid, D::VcId, D::MapId, Map = D::Map>
        + PhysicalLookup<Physical = D::Physical>
        + ?Sized,
{
    type Item = LenientItem<'a, D>;

    fn next(&mut self) -> Option<Self::Item> {
        step::<D, L>(
            &mut self.rest,
            self.channels,
            self.max_fill,
            DecodeMode::Lenient,
        )
    }
}

impl<D: Delimiter, L> FusedIterator for FrameStream<'_, '_, D, L> where
    L: ChannelLookup<D::Scid, D::VcId, Vc = D::Vc>
        + MapLookup<D::Scid, D::VcId, D::MapId, Map = D::Map>
        + PhysicalLookup<Physical = D::Physical>
        + ?Sized
{
}

impl<D: Delimiter, L> FusedIterator for LenientFrameStream<'_, '_, D, L> where
    L: ChannelLookup<D::Scid, D::VcId, Vc = D::Vc>
        + MapLookup<D::Scid, D::VcId, D::MapId, Map = D::Map>
        + PhysicalLookup<Physical = D::Physical>
        + ?Sized
{
}

/// One step of both streams: the next item of `rest`, which it advances.
fn step<'a, D: Delimiter, L>(
    rest: &mut &'a [u8],
    channels: &L,
    max_fill: usize,
    mode: DecodeMode,
) -> Option<LenientItem<'a, D>>
where
    L: ChannelLookup<D::Scid, D::VcId, Vc = D::Vc>
        + MapLookup<D::Scid, D::VcId, D::MapId, Map = D::Map>
        + PhysicalLookup<Physical = D::Physical>
        + ?Sized,
{
    if rest.is_empty() {
        return None;
    }
    let all: &'a [u8] = rest;
    match D::peek(all, channels) {
        Peek::Frame(len) => {
            if let Some((frame_bytes, remaining)) = all.split_at_checked(len.get()) {
                *rest = remaining;
                return Some(
                    match check_bytes_matched::<D::Frame<'a>, L>(frame_bytes, channels, mode) {
                        Ok((frame, (vc, map))) => IntakeItem::Accepted { frame, vc, map },
                        Err(rejection) => IntakeItem::Discarded {
                            reason: D::Reason::from(rejection.reason()),
                            octets: frame_bytes,
                            error: rejection.error(),
                        },
                    },
                );
            }
            match D::incomplete(all, len.get(), channels, mode) {
                Some((reason, error)) => {
                    *rest = &[];
                    Some(IntakeItem::Discarded {
                        reason,
                        octets: all,
                        error,
                    })
                }
                None => Some(fill::<D, _>(rest, all, max_fill)),
            }
        }
        Peek::Discard { cut, reason, error } => {
            *rest = cut.rest;
            Some(IntakeItem::Discarded {
                reason,
                octets: cut.octets,
                error,
            })
        }
        Peek::DiscardRest { reason, error } => {
            *rest = &[];
            Some(IntakeItem::Discarded {
                reason,
                octets: all,
                error,
            })
        }
        Peek::Fill => Some(fill::<D, _>(rest, all, max_fill)),
    }
}

/// The discard of the rest of the unit as Fill Data or, when longer than `max_fill`, as
/// leftover fill: a Frame Length error may have shortened a frame before it (232.0 §4.4.9.2.6
/// note 2, 732.1 §4.3.11.2.4 note 2; OI-47).
const fn fill<'a, D: Delimiter, F>(
    rest: &mut &'a [u8],
    all: &'a [u8],
    max_fill: usize,
) -> IntakeItem<'a, D, F> {
    *rest = &[];
    if all.len() > max_fill {
        IntakeItem::Discarded {
            reason: D::LEFTOVER_FILL,
            octets: all,
            error: Error::new(ErrorKind::Malformed).with_field(FRAME_LEN),
        }
    } else {
        IntakeItem::Discarded {
            reason: D::FILL,
            octets: all,
            error: Error::new(ErrorKind::Malformed).with_field(FRAME_DATA_FIELD),
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use core::num::NonZeroUsize;

    use super::{Cut, Delimiter, FrameStream, IntakeItem, LenientFrameStream, Peek, StreamFrame};
    use crate::anomaly::AnomalySet;
    use crate::checked::{CheckBytes, Rejection};
    use crate::error::{Error, ErrorKind};
    use crate::lookup::{ChannelLookup, MapLookup, PhysicalLookup};
    use crate::mode::DecodeMode;

    crate::anomaly_flags! {
        /// Flags of the demo frame.
        enum DemoAnomaly {
            /// Odd length.
            Odd = 0,
        }
    }

    /// A frame of `[len, ..]`, `len` octets long, whose second octet must be 0.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct Demo<'a>(&'a [u8]);

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Why {
        Bad,
        Fill,
        Leftover,
        Cut,
        Gone,
    }

    impl<'a> CheckBytes<'a> for Demo<'a> {
        type Reason = Why;
        type Anomaly = DemoAnomaly;
        type Matched = ((), Option<()>);
        type Scid = u8;
        type VcId = u8;
        type MapId = u8;
        type Vc = ();
        type Map = ();
        type Physical = ();

        fn parse_exact<L>(
            bytes: &'a [u8],
            _: &L,
            mode: DecodeMode,
        ) -> Result<(Self, AnomalySet<DemoAnomaly>, Self::Matched), Rejection<Why>>
        where
            L: ChannelLookup<u8, u8, Vc = ()>
                + MapLookup<u8, u8, u8, Map = ()>
                + PhysicalLookup<Physical = ()>
                + ?Sized,
        {
            if bytes.get(1) != Some(&0) {
                return Err(Rejection::new(Why::Bad, Error::new(ErrorKind::Malformed)));
            }
            let mut anomalies = AnomalySet::empty();
            if mode == DecodeMode::Lenient && bytes.len() % 2 == 1 {
                anomalies.insert(DemoAnomaly::Odd);
            }
            Ok((Demo(bytes), anomalies, ((), Some(()))))
        }

        fn octets(&self) -> &'a [u8] {
            self.0
        }
    }

    struct Any;

    impl ChannelLookup<u8, u8> for Any {
        type Vc = ();

        fn has_spacecraft(&self, _: u8) -> bool {
            true
        }

        fn vc(&self, _: u8, _: u8) -> Option<()> {
            Some(())
        }
    }

    impl MapLookup<u8, u8, u8> for Any {
        type Map = ();

        fn map(&self, _: u8, _: u8, _: u8) -> Option<()> {
            Some(())
        }
    }

    impl PhysicalLookup for Any {
        type Physical = ();

        fn physical_channel(&self) {}
    }

    /// First octet: the frame length; 9 discards 2 octets; 8 declares 8 octets and cuts the
    /// rest if short; any other 0xFF is a header declaring 9 octets treated as fill if short.
    struct Proto;

    /// A sink for `Debug` output.
    struct Sink;

    impl core::fmt::Write for Sink {
        fn write_str(&mut self, _: &str) -> core::fmt::Result {
            Ok(())
        }
    }

    impl Delimiter for Proto {
        type Frame<'a> = Demo<'a>;
        type Anomaly = DemoAnomaly;
        type Rejected = Why;
        type Reason = Why;
        type Scid = u8;
        type VcId = u8;
        type MapId = u8;
        type Vc = ();
        type Map = ();
        type Physical = ();

        const FILL: Why = Why::Fill;
        const LEFTOVER_FILL: Why = Why::Leftover;

        fn peek<'a, L>(rest: &'a [u8], _: &L) -> Peek<'a, Why>
        where
            L: ChannelLookup<u8, u8, Vc = ()>
                + MapLookup<u8, u8, u8, Map = ()>
                + PhysicalLookup<Physical = ()>
                + ?Sized,
        {
            match rest.first().copied() {
                Some(9) => match Cut::new(rest, NonZeroUsize::MIN.saturating_add(1)) {
                    Some(cut) => Peek::Discard {
                        cut,
                        reason: Why::Gone,
                        error: Error::new(ErrorKind::Malformed),
                    },
                    None => Peek::DiscardRest {
                        reason: Why::Gone,
                        error: Error::new(ErrorKind::Malformed),
                    },
                },
                Some(n @ (2..=8)) => Peek::frame(usize::from(n)),
                Some(0xFF) => Peek::frame(9),
                Some(0) => Peek::frame(0),
                _ => Peek::Fill,
            }
        }

        fn incomplete<L>(
            rest: &[u8],
            declared_len: usize,
            _: &L,
            _: DecodeMode,
        ) -> Option<(Why, Error)>
        where
            L: ChannelLookup<u8, u8, Vc = ()>
                + MapLookup<u8, u8, u8, Map = ()>
                + PhysicalLookup<Physical = ()>
                + ?Sized,
        {
            (rest.first() != Some(&0xFF)).then_some((Why::Cut, Error::incomplete(declared_len)))
        }
    }

    type Item<'a> = IntakeItem<'a, Proto>;

    #[test]
    fn the_stream_cuts_checks_and_discards() {
        // req: TC-020, TC-021, USLP-014, USLP-015
        let unit = [3, 0, 7, 4, 1, 1, 1, 9, 5, 0xFF, 0, 0, 0, 0];
        let mut s = FrameStream::<Proto, _>::new(&unit, &Any, 6);
        assert_eq!(s.remaining().len(), 14);
        assert_eq!(DemoAnomaly::ALL, &[DemoAnomaly::Odd]);
        let first = s.next().unwrap();
        assert_eq!(first.octets(), [3, 0, 7]);
        assert!(matches!(first, Item::Accepted { map: Some(()), .. }));
        assert_eq!((first.len(), first.is_empty()), (3, false));
        let bad = s.next().unwrap();
        assert!(matches!(
            &bad,
            Item::Discarded {
                reason: Why::Bad,
                ..
            }
        ));
        assert_eq!(bad.octets(), [4, 1, 1, 1]);
        let gone = s.next().unwrap();
        assert!(matches!(
            &gone,
            Item::Discarded {
                reason: Why::Gone,
                ..
            }
        ));
        assert_eq!(gone.octets(), [9, 5]);
        // A header declaring 9 octets with 5 left is treated as fill: 5 are no more than 6.
        let fill = s.next().unwrap();
        assert!(matches!(
            &fill,
            Item::Discarded {
                reason: Why::Fill,
                ..
            }
        ));
        assert_eq!(s.next(), None);
        assert_eq!(s.remaining(), []);
        assert_eq!(s.clone().next(), None);
        core::fmt::write(&mut Sink, format_args!("{s:?}")).unwrap();
    }

    #[test]
    fn long_leftovers_and_incomplete_frames_are_discarded() {
        // req: TC-020, USLP-014, USLP-015
        let mut s = FrameStream::<Proto, _>::new(&[8, 0, 0], &Any, 6);
        let cut = s.next().unwrap();
        assert!(matches!(
            &cut,
            Item::Discarded {
                reason: Why::Cut,
                ..
            }
        ));
        let mut s = FrameStream::<Proto, _>::new(&[1, 1, 1, 1, 1, 1, 1], &Any, 6);
        assert!(matches!(
            s.next().unwrap(),
            Item::Discarded {
                reason: Why::Leftover,
                ..
            }
        ));
        assert_eq!(s.next(), None);
        let mut s = FrameStream::<Proto, _>::new(&[0xFF, 0, 0, 0, 0, 0, 0], &Any, 6);
        assert!(matches!(
            s.next().unwrap(),
            Item::Discarded {
                reason: Why::Leftover,
                ..
            }
        ));
    }

    #[test]
    fn items_clone_compare_and_print() {
        // req: TC-020, USLP-014
        let mut s = FrameStream::<Proto, _>::new(&[4, 1, 1, 1, 3, 0, 0, 5, 5, 5, 5, 5], &Any, 6);
        let bad = s.next().unwrap();
        let good = s.next().unwrap();
        let fill = s.next().unwrap();
        assert!(s.next().is_none());
        for item in [&bad, &good, &fill] {
            assert_eq!(item, &item.clone());
            core::fmt::write(&mut Sink, format_args!("{item:?}")).unwrap();
        }
        assert_ne!(bad, fill);
        assert_ne!(bad, good);
        let other = FrameStream::<Proto, _>::new(&[3, 0, 1], &Any, 6)
            .next()
            .unwrap();
        assert_ne!(good, other);
        let mut again = FrameStream::<Proto, _>::new(&[3, 0, 0, 4, 1, 1, 1], &Any, 6);
        let _ = again.next();
        let bad2 = again.next().unwrap();
        assert_eq!(bad, bad2);
        let other_reason = FrameStream::<Proto, _>::new(&[9, 5], &Any, 6)
            .next()
            .unwrap();
        assert_ne!(bad, other_reason);
    }

    #[test]
    fn a_discard_that_outruns_the_unit_takes_the_rest_and_a_zero_length_is_fill() {
        // req: TC-020, USLP-014
        let mut short = FrameStream::<Proto, _>::new(&[9], &Any, 6);
        let IntakeItem::Discarded { reason, octets, .. } = short.next().unwrap() else {
            panic!("discarded")
        };
        assert_eq!((reason, octets), (Why::Gone, &[9][..]));
        assert_eq!(short.next(), None);
        assert_eq!(Peek::<Why>::frame(0), Peek::Fill);
        assert_eq!(
            Peek::<Why>::frame(3),
            Peek::Frame(NonZeroUsize::new(3).unwrap())
        );
        let mut zero = FrameStream::<Proto, _>::new(&[0, 1], &Any, 6);
        let IntakeItem::Discarded { reason, octets, .. } = zero.next().unwrap() else {
            panic!("discarded")
        };
        assert_eq!((reason, octets), (Why::Fill, &[0, 1][..]));
    }

    #[test]
    fn the_lenient_stream_carries_anomalies() {
        // req: API-030, API-032, TC-020, USLP-014
        let mut s = LenientFrameStream::<Proto, _>::new(&[3, 0, 0, 2, 0], &Any, 6);
        let first = s.next().unwrap();
        let IntakeItem::Accepted { frame, vc, map } = first.clone() else {
            panic!("accepted")
        };
        assert!(frame.anomalies().contains(DemoAnomaly::Odd));
        assert_eq!(frame.octets(), [3, 0, 0]);
        assert_eq!((vc, map), ((), Some(())));
        assert_eq!(first.octets(), [3, 0, 0]);
        assert_ne!(first, s.next().unwrap());
        assert_eq!(s.next(), None);
        core::fmt::write(&mut Sink, format_args!("{s:?} {first:?}")).unwrap();
        assert_eq!(s.clone().remaining(), []);
        let again = LenientFrameStream::<Proto, _>::new(&[3, 0, 0], &Any, 6)
            .next()
            .unwrap();
        assert_eq!(again, again.clone());
    }
}

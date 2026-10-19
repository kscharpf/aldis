//! Calls every public `aldis-core` API and checks every embedded vector case (VER-010).
//!
//! Inputs pass through `black_box`, so the optimizer cannot fold a case to a constant and
//! drop panic paths that other inputs could reach. Failures are counted, never panicked on.

use core::fmt::Write as _;
use core::hint::black_box as bb;

use aldis_core::anomaly::{Anomaly, AnomalySet, CAPACITY};
use aldis_core::bits;
use aldis_core::container::{ByteQueue, FixedQueue, FixedVec};
use aldis_core::counter::{Continuity, ModCounter};
use aldis_core::crc::{
    PRESET, crc16, crc16_bitwise, crc16_table, crc16_update, crc16_update_bitwise,
    crc16_update_table, syndrome, verify_fecf, write_fecf,
};
use aldis_core::error::{Error, ErrorDetail, ErrorKind, FieldId};
use aldis_core::ids::{Apid, FrameSeqNum, MapId, PacketVersion, SeqCount, SpacecraftId, VcId};
use aldis_core::layout::{FrameLayout, LayoutSpec};
use aldis_core::mode::DecodeMode;
use aldis_core::oid::{OidGenerator, OidGeneratorGalois};
use aldis_core::security::{
    FixedPatternSecurity, FrameCtx, NoSecurity, Protocol, SecuredFields, SecurityHook,
    SecurityLayout,
};
use aldis_core::time::{Duration, Instant};

use crate::cases::{BITS_GET, BITS_SET, COUNTERS, CRC, CounterExpect, ERRORS, LAYOUTS, OID_START};

/// Failure counter.
#[derive(Default)]
pub struct Tally {
    pub cases: u32,
    pub failures: u32,
}

impl Tally {
    pub(crate) const fn check(&mut self, ok: bool) {
        self.cases = self.cases.saturating_add(1);
        if !ok {
            self.failures = self.failures.saturating_add(1);
        }
    }
}

/// A fixed-size `fmt::Write` sink (no allocation).
struct Text {
    buf: [u8; 96],
    len: usize,
}

impl core::fmt::Write for Text {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let end = self.len.checked_add(s.len()).ok_or(core::fmt::Error)?;
        self.buf
            .get_mut(self.len..end)
            .ok_or(core::fmt::Error)?
            .copy_from_slice(s.as_bytes());
        self.len = end;
        Ok(())
    }
}

fn vectors(t: &mut Tally) {
    for c in CRC {
        t.check(crc16(bb(c.input)) == c.value);
        t.check(crc16_bitwise(bb(c.input)) == c.value);
        t.check(crc16_table(bb(c.input)) == c.value);
    }
    let (mut fib, mut gal) = (OidGenerator::new(), OidGeneratorGalois::new());
    for &expected in OID_START {
        t.check(fib.next_octet() == expected);
        t.check(gal.next_octet() == expected);
    }
    for c in ERRORS {
        let kind = ErrorKind::from_name(bb(c.name));
        t.check(kind.map(ErrorKind::code) == Some(c.code));
        t.check(
            kind.is_some_and(|k| ErrorKind::from_code(k.code()) == Some(k) && k.name() == c.name),
        );
    }
    for c in BITS_GET {
        t.check(bits::get(bb(c.input), bb(c.offset), bb(c.width)).ok() == c.expect);
    }
    for c in BITS_SET {
        let mut buf = [0_u8; 16];
        let Some(dst) = buf.get_mut(..c.input.len()) else {
            t.check(false);
            continue;
        };
        dst.copy_from_slice(c.input);
        let r = bits::set(bb(dst), bb(c.offset), bb(c.width), bb(c.value));
        match c.expect {
            Some(out) => t.check(r.is_ok() && *dst == *out),
            None => {
                t.check(r.is_err_and(|e| e.kind() == ErrorKind::OutOfRange) && *dst == *c.input);
            }
        }
    }
    for c in COUNTERS {
        let counter = ModCounter::new(bb(c.bits), bb(c.a));
        let ok = match (&c.expect, c.classify) {
            (CounterExpect::OutOfRange, _) => {
                counter.is_err() || counter.is_ok_and(|k| k.classify(bb(c.b)).is_err())
            }
            (CounterExpect::Value(v), false) => counter.is_ok_and(|k| k.next().value() == *v),
            (expect, true) => {
                counter
                    .and_then(|k| k.classify(bb(c.b)))
                    .is_ok_and(|got| match (expect, got) {
                        (CounterExpect::InSequence, Continuity::InSequence)
                        | (CounterExpect::Repeat, Continuity::Repeat)
                        | (CounterExpect::Backwards, Continuity::Backwards) => true,
                        (CounterExpect::Gap(m), Continuity::Gap { missing }) => *m == missing,
                        _ => false,
                    })
            }
            _ => false,
        };
        t.check(ok);
    }
    for c in LAYOUTS {
        let [frame_len, primary_header_len, secondary_len, sh, st] = c.sizes;
        let got = SecurityLayout::new(bb(sh), bb(st)).and_then(|security| {
            FrameLayout::compute(&LayoutSpec {
                frame_len: bb(frame_len),
                primary_header_len: bb(primary_header_len),
                secondary_len: bb(secondary_len),
                security,
                ocf: bb(c.ocf),
                fecf: bb(c.fecf),
            })
        });
        if let Ok(l) = &got {
            let accessors = [
                l.primary_header(),
                l.secondary(),
                l.security_header(),
                l.data(),
                l.security_trailer(),
                l.ocf(),
                l.fecf(),
            ];
            tally_layout(t, l, &accessors);
        }
        match (got, c.expect) {
            (Ok(l), Some(expected)) => {
                let mut flat = [0_usize; 14];
                for (pair, r) in flat.as_chunks_mut::<2>().0.iter_mut().zip(l.ranges()) {
                    pair.copy_from_slice(&[r.start, r.end]);
                }
                t.check(flat == expected);
            }
            (Err(e), None) => t.check(e.kind() == ErrorKind::InvalidConfig),
            _ => t.check(false),
        }
    }
}

/// The named `FrameLayout` accessors agree with `ranges()`, and the secured fields derived from
/// the layout match its security and data ranges.
fn tally_layout(t: &mut Tally, l: &FrameLayout, accessors: &[core::ops::Range<usize>; 7]) {
    t.check(*accessors == l.ranges() && l.frame_len() == l.fecf().end);
    let s = SecuredFields::from_layout(l);
    t.check(s.header() == l.security_header() && s.data() == l.data());
    t.check(s.trailer() == l.security_trailer());
}

fn apis_errors_ids(tally: &mut Tally) {
    // error
    let err0 = Error::out_of_range(bb("apid"), 2047, bb(4096))
        .with_field(FieldId::new("apid"))
        .with_detail(ErrorDetail::Expected {
            expected: 2047,
            found: 4096,
        });
    tally.check(
        err0.kind() == ErrorKind::OutOfRange && err0.field().map(FieldId::name) == Some("apid"),
    );
    tally.check(err0.detail() != ErrorDetail::None);
    tally.check(Error::from(ErrorKind::Malformed) == Error::new(ErrorKind::Malformed));
    for err in [
        err0,
        Error::buffer_too_small(bb(9)),
        Error::incomplete(bb(3)),
        Error::new(ErrorKind::Security),
    ] {
        let mut text = Text {
            buf: [0; 96],
            len: 0,
        };
        tally.check(write!(text, "{err}").is_ok() && text.len > 0);
    }
    tally.check(ErrorKind::ALL.len() == 17);

    // bits octet helpers
    let mut octets = [0_u8; 8];
    tally.check(
        bits::write_u16(bb(&mut octets), bb(0), bb(0xCAFE)).is_ok()
            && bits::read_u16(bb(&octets), bb(0)) == Ok(0xCAFE)
            && bits::write_u32(bb(&mut octets), bb(2), bb(0x0102_0304)).is_ok()
            && bits::read_u32(bb(&octets), bb(2)) == Ok(0x0102_0304)
            && bits::write_u64(bb(&mut octets), bb(0), bb(0x0102_0304_0506_0708)).is_ok()
            && bits::read_u64(bb(&octets), bb(0)) == Ok(0x0102_0304_0506_0708),
    );
    let mut short = [0_u8; 2];
    tally.check(
        bits::read_u16(bb(&octets), bb(7)).is_err()
            && bits::write_u32(bb(&mut octets), bb(5), 0).is_err()
            && bits::read_u32(bb(&short), bb(0)).is_err()
            && bits::write_u32(bb(&mut short), bb(0), 0).is_err()
            && bits::read_u64(bb(&octets), bb(1)).is_err()
            && bits::write_u64(bb(&mut octets), bb(1), 0).is_err(),
    );

    // ids
    tally.check(SpacecraftId::<10>::new(bb(1023)).map(SpacecraftId::get) == Some(1023));
    tally.check(SpacecraftId::<8>::try_from(bb(256_u16)).is_err());
    tally.check(SpacecraftId::<16>::try_from(bb(7_u16)).map(u16::from) == Ok(7));
    tally.check(VcId::<3>::new(bb(8)).is_none() && VcId::<6>::try_from(bb(63_u8)).is_ok());
    tally.check(MapId::<4>::new(bb(15)).map(u8::from) == Some(15) && MapId::<6>::MAX == 63);
    tally.check(
        Apid::new(bb(2047)).is_some_and(Apid::is_idle) && u16::from(Apid::IDLE) == Apid::MAX,
    );
    tally.check(Apid::try_from(bb(4096_u16)).is_err() && Apid::IDLE.get() == 2047);
    let seq = SeqCount::new(bb(SeqCount::MAX)).unwrap_or_default();
    tally.check(
        seq.next().get() == 0
            && u16::from(seq) == SeqCount::MAX
            && ModCounter::from(seq).value() == u64::from(SeqCount::MAX)
            && seq.classify(bb(seq.next())) == Continuity::InSequence
            && SeqCount::try_from(bb(1_u16)).is_ok(),
    );
    let fsn = FrameSeqNum::from(bb(FrameSeqNum::MAX));
    tally.check(
        FrameSeqNum::new(bb(0)).get() == 0
            && fsn.next() == FrameSeqNum::new(0)
            && u8::from(fsn) == 255
            && ModCounter::from(fsn).value() == 255
            && fsn.classify(bb(fsn.next())) == Continuity::InSequence,
    );
    tally.check(PacketVersion::new(bb(7)) == Some(PacketVersion::ENCAPSULATION));
    tally.check(
        PacketVersion::try_from(bb(9_u8)).is_err()
            && PacketVersion::SPACE_PACKET.get() == 0
            && u8::from(PacketVersion::ENCAPSULATION) == PacketVersion::MAX,
    );
}

fn apis_time_crc_oid_counter(tally: &mut Tally) {
    // time
    let now = Instant::from_micros(bb(10));
    let later =
        now.saturating_add(Duration::from_millis(bb(2)).saturating_add(Duration::from_secs(bb(1))));
    tally.check(
        later.duration_since(now).as_micros() == 1_002_000
            && now.duration_since(later) == Duration::ZERO,
    );
    tally.check(later.as_micros() == 1_002_010 && Duration::from_micros(bb(5)).as_micros() == 5);
    tally.check(Duration::MAX.saturating_add(Duration::from_micros(bb(1))) == Duration::MAX);

    // crc
    let mut frame = [0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0, 0];
    tally.check(
        write_fecf(bb(&mut frame)).is_ok()
            && verify_fecf(bb(&frame)).is_ok()
            && syndrome(bb(&frame)) == 0,
    );
    tally.check(
        crc16_update(bb(PRESET), bb(&frame)) == 0
            && crc16_update_bitwise(bb(PRESET), bb(&frame)) == 0
            && crc16_update_table(bb(PRESET), bb(&frame)) == 0,
    );
    tally.check(verify_fecf(bb(&[0])).is_err() && write_fecf(bb(&mut [0])).is_err());

    // oid
    let mut fill = [0_u8; 5];
    let mut gen_fib = OidGenerator::default();
    gen_fib.fill(bb(&mut fill));
    tally.check(fill.first() == Some(&0xFF) && gen_fib.next_bit() <= 1);
    let mut gg = OidGeneratorGalois::default();
    gg.fill(bb(&mut fill));
    tally.check(gg.next_bit() <= 1);

    // counter
    if let Ok(mut counter) = ModCounter::new(bb(24), bb(16_777_215)) {
        counter.increment();
        tally.check(
            counter.value() == 0 && counter.bits() == 24 && counter.distance_to(bb(5)) == Ok(5),
        );
    } else {
        tally.check(false);
    }
}

fn apis_containers_security(tally: &mut Tally) {
    // containers
    let mut queue: FixedQueue<u16, 4> = FixedQueue::default();
    tally.check(
        queue.push(bb(1)).is_ok()
            && queue.peek() == Some(&1)
            && queue.len() == 1
            && !queue.is_full(),
    );
    tally.check(queue.pop() == Some(1) && queue.is_empty() && queue.capacity() == 4);
    queue.clear();
    let mut bytes: ByteQueue<64> = ByteQueue::default();
    tally.check(
        bytes.push(bb(&[1, 2, 3])).is_ok() && bytes.push(bb(&[4])).is_ok() && bytes.records() == 2,
    );
    tally.check(
        bytes.peek_len() == Some(3)
            && bytes.free() == 64 - 12
            && bytes.capacity() == 64
            && !bytes.is_empty(),
    );
    let mut out = [0_u8; 8];
    tally.check(bytes.pop_into(bb(&mut out)) == Ok(Some(3)) && bytes.discard() == Some(1));
    bytes.clear();
    let mut vec: FixedVec<u8, 2> = FixedVec::default();
    tally.check(
        vec.push(bb(9)).is_ok()
            && vec.as_slice() == [9]
            && vec.len() == 1
            && vec.capacity() == 2
            && !vec.is_empty()
            && !vec.is_full(),
    );
    if let Some(first) = vec.as_mut_slice().first_mut() {
        *first = bb(10);
    }
    let mut other: FixedVec<u8, 2> = FixedVec::default();
    let _ = other.push(bb(10));
    let _ = other.push(bb(99));
    let _ = other.pop();
    let mut diff: FixedVec<u8, 2> = FixedVec::default();
    let _ = diff.push(bb(11));
    tally.check(vec == other && vec != diff && vec.pop() == Some(10));
    vec.clear();

    // security
    let (Some(scid), Some(vcid), Some(map)) = (
        SpacecraftId::<16>::new(bb(1)),
        VcId::<6>::new(bb(2)),
        MapId::<4>::new(bb(3)),
    ) else {
        tally.check(false);
        return;
    };
    let ctx = FrameCtx::Uslp {
        scid,
        vcid,
        map_id: Some(map),
    };
    tally.check(ctx.protocol() == bb(Protocol::Uslp) && ctx.scid() == 1 && ctx.vcid() == 2);
    tally.check(ctx.map_id() == Some(3));
    let Ok(fields) = SecuredFields::new(bb(0..2), 2..4, 4..6) else {
        tally.check(false);
        return;
    };
    tally.check(fields.header() == (0..2) && fields.data() == (2..4) && fields.trailer() == (4..6));
    tally.check(SecuredFields::new(bb(0..2), 3..4, 4..6).is_err());
    let mut buf = [0_u8; 6];
    tally.check(NoSecurity.protect(&ctx, bb(&mut buf), &fields).is_ok());
    tally.check(NoSecurity.verify(&ctx, bb(&mut buf), &fields) == Ok(2..4));
    let mut hook = FixedPatternSecurity;
    tally.check(
        hook.protect(&ctx, bb(&mut buf), &fields).is_ok()
            && hook.verify(&ctx, bb(&mut buf), &fields).is_ok(),
    );
    let layout = SecurityLayout::new(bb(6), bb(16));
    tally.check(layout == Ok(SecurityLayout::USLP_TRUNCATED) && SecurityLayout::NONE.is_none());
    tally.check(layout.is_ok_and(|l| l.header_len() == 6 && l.trailer_len() == 16));
    tally.check(
        SecurityLayout::new(
            bb(SecurityLayout::MAX_HEADER_LEN),
            SecurityLayout::MAX_TRAILER_LEN,
        )
        .is_ok()
            && SecurityLayout::new(bb(65), 0).is_err(),
    );
}

/// A flag type whose index is its value, so out-of-range indices reach `AnomalySet` too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Flag(u8);

// A deliberately unconstrained test double: implements the sealing trait directly.
impl aldis_core::anomaly::__private::Sealed for Flag {}

impl Anomaly for Flag {
    fn bit(self) -> u8 {
        self.0
    }

    fn from_bit(bit: u8) -> Option<Flag> {
        (bit < CAPACITY).then_some(Flag(bit))
    }
}

fn apis_mode_anomaly(tally: &mut Tally) {
    // mode
    tally.check(bb(DecodeMode::default()) == DecodeMode::Strict);
    tally.check(bb(DecodeMode::Lenient) != DecodeMode::Strict);

    // anomaly
    let mut set: AnomalySet<Flag> = AnomalySet::default();
    tally.check(set.is_empty() && set.bits() == 0);
    set.insert(bb(Flag(3)));
    set.insert(bb(Flag(31)));
    set.insert(bb(Flag(32)));
    tally.check(set.contains(bb(Flag(3))) && !set.contains(bb(Flag(32))) && set.len() == 2);
    tally.check(set.bits() == 0x8000_0008 && !set.is_empty());
    tally.check(set.iter().map(|f| f.0).eq([3, 31]) && (&set).into_iter().count() == 2);
    tally.check(set.iter().size_hint() == (0, Some(2)) && set.into_iter().count() == 2);
    let rebuilt = AnomalySet::from_flags(bb(&[Flag(31), Flag(3)]));
    tally.check(rebuilt == set && rebuilt != AnomalySet::empty());
    let mut text = Text {
        buf: [0; 96],
        len: 0,
    };
    tally.check(write!(text, "{set:?}").is_ok() && text.len > 0);
}

/// Runs everything; returns the tally.
pub fn run() -> Tally {
    let mut t = Tally::default();
    vectors(&mut t);
    apis_errors_ids(&mut t);
    apis_time_crc_oid_counter(&mut t);
    apis_containers_security(&mut t);
    apis_mode_anomaly(&mut t);
    crate::epp::run(&mut t);
    crate::time::run(&mut t);
    #[cfg(feature = "inject-panic")]
    canary(&mut t);
    t
}

/// Canary for the link check: a panic the optimizer cannot remove (feature `inject-panic`).
#[cfg(feature = "inject-panic")]
#[expect(
    clippy::panic,
    reason = "deliberate panic: the link check must fail with this feature"
)]
fn canary(t: &mut Tally) {
    if bb(t.cases) == u32::MAX {
        panic!("canary");
    }
}

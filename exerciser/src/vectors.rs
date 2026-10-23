//! The `aldis-core` OCF, `aldis-time`, `aldis-spp`, `aldis-epp`, `aldis-aos`, `aldis-tm`, and
//! `aldis-tc` vector cases on the target (M1 WP-5, D7; M2 WP-1, WP-2, WP-3, D16).
//!
//! `build.rs` embeds every live case of `test-vectors/core/ocf.json` and
//! `test-vectors/{time,spp,epp,aos,tm,tc}/` as a value tree. Each handler (`ocf.rs`, `time.rs`,
//! `spp.rs`, `epp.rs`, `aos.rs`, `tm.rs`, `tc.rs`) mirrors the host suite of its crate
//! (`crates/aldis-*/tests/vectors.rs`): it reads the parameters, calls the API with inputs
//! through `black_box`, and reports every output value to an [`Out`], which compares them with
//! the expected values exactly, with no missing and no extra keys (TV-012).

use core::fmt::Write as _;

use aldis_core::Error;

use crate::exercise::Tally;

/// A value of a vector file.
#[derive(Clone, Copy)]
pub enum Val {
    Null,
    Bool(bool),
    Num(u64),
    Str(&'static str),
    /// A string under a key ending in `_hex`, decoded.
    Bytes(&'static [u8]),
    List(&'static [Val]),
    Obj(Map),
}

/// An object of a vector file.
pub type Map = &'static [(&'static str, Val)];

/// The crate, or the `aldis-core` module, a case belongs to.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Ocf,
    Time,
    Spp,
    Epp,
    Aos,
    Tm,
    Tc,
}

/// The expected outcome of a case.
pub enum Expect {
    /// Success with exactly these output values.
    Ok(Map),
    /// Failure with this error kind (catalogue name) and, when the case gives one, the code of
    /// the field the error names (03-api-model.md §9.1; API-082, M2 plan D3).
    Err(&'static str, Option<u16>),
}

/// One codec case.
pub struct Case {
    pub id: &'static str,
    pub group: Group,
    pub op: &'static str,
    pub lenient: bool,
    pub input: &'static [u8],
    pub params: Map,
    pub config: Map,
    pub expect: Expect,
}

#[expect(
    clippy::unreadable_literal,
    reason = "machine-generated vector data from build.rs"
)]
mod generated {
    use super::{Case, Expect, Group, Val};
    include!(concat!(env!("OUT_DIR"), "/codec.rs"));
}
pub use generated::CODEC;

/// Why a handler did not produce an outcome to compare.
pub enum Fail {
    /// The API returned this error: the outcome of the case.
    Api(Error),
    /// The case cannot be run as written (a parameter is missing or out of range for its
    /// type), or a consistency check the host suite asserts did not hold.
    Vector,
    /// A length parameter does not fit in this target's `usize`.
    NotRepresentable,
}

impl From<Error> for Fail {
    fn from(e: Error) -> Fail {
        Fail::Api(e)
    }
}

/// Handler result.
pub type Run = Result<(), Fail>;

/// `Some(x)`, or the vector is malformed.
pub fn need<T>(x: Option<T>) -> Result<T, Fail> {
    x.ok_or(Fail::Vector)
}

/// The entry `key` of `map`.
pub fn get(map: Map, key: &str) -> Option<Val> {
    map.iter().find(|(k, _)| *k == key).map(|&(_, v)| v)
}

impl Val {
    pub const fn num(self) -> Option<u64> {
        match self {
            Val::Num(n) => Some(n),
            _ => None,
        }
    }

    pub const fn bool(self) -> Option<bool> {
        match self {
            Val::Bool(b) => Some(b),
            _ => None,
        }
    }

    pub const fn str(self) -> Option<&'static str> {
        match self {
            Val::Str(s) => Some(s),
            _ => None,
        }
    }

    /// The octets of a `_hex` value, or of any other string.
    pub const fn bytes(self) -> Option<&'static [u8]> {
        match self {
            Val::Bytes(b) => Some(b),
            Val::Str(s) => Some(s.as_bytes()),
            _ => None,
        }
    }

    pub const fn list(self) -> Option<&'static [Val]> {
        match self {
            Val::List(l) => Some(l),
            _ => None,
        }
    }

    pub const fn obj(self) -> Option<Map> {
        match self {
            Val::Obj(m) => Some(m),
            _ => None,
        }
    }

    pub const fn is_null(self) -> bool {
        matches!(self, Val::Null)
    }
}

/// Typed access to the parameters of a case.
pub trait Params {
    fn val(self, key: &str) -> Result<Val, Fail>;

    fn num<T: TryFrom<u64>>(self, key: &str) -> Result<T, Fail>
    where
        Self: Sized,
    {
        T::try_from(need(self.val(key)?.num())?).map_err(|_| Fail::Vector)
    }

    /// A length: [`Fail::NotRepresentable`] when it does not fit in `usize`.
    fn size(self, key: &str) -> Result<usize, Fail>
    where
        Self: Sized,
    {
        usize::try_from(need(self.val(key)?.num())?).map_err(|_| Fail::NotRepresentable)
    }

    fn flag(self, key: &str) -> Result<bool, Fail>
    where
        Self: Sized,
    {
        need(self.val(key)?.bool())
    }

    fn text(self, key: &str) -> Result<&'static str, Fail>
    where
        Self: Sized,
    {
        need(self.val(key)?.str())
    }

    fn bytes(self, key: &str) -> Result<&'static [u8], Fail>
    where
        Self: Sized,
    {
        need(self.val(key)?.bytes())
    }

    fn obj(self, key: &str) -> Result<Map, Fail>
    where
        Self: Sized,
    {
        need(self.val(key)?.obj())
    }

    fn list(self, key: &str) -> Result<&'static [Val], Fail>
    where
        Self: Sized,
    {
        need(self.val(key)?.list())
    }
}

impl Params for Map {
    fn val(self, key: &str) -> Result<Val, Fail> {
        need(get(self, key))
    }
}

/// Collects a handler's output values and compares them with the expected object exactly.
pub struct Out {
    expect: Map,
    /// Bit `i` is set once the key at index `i` of `expect` has been produced. An object with
    /// more than 64 keys cannot be tracked and fails the comparison (the vectors have at most 23).
    seen: u64,
    ok: bool,
}

impl Out {
    pub const fn new(expect: Map) -> Out {
        Out {
            expect,
            seen: 0,
            ok: true,
        }
    }

    /// The expected value of `key`. A key that is not expected, or one produced twice, fails the
    /// comparison.
    fn take(&mut self, key: &str) -> Option<Val> {
        let found = self.expect.iter().enumerate().find(|(_, (k, _))| *k == key);
        let Some((index, &(_, v))) = found else {
            self.ok = false;
            return None;
        };
        let bit = u32::try_from(index).ok().and_then(|i| 1_u64.checked_shl(i));
        match bit {
            Some(bit) if self.seen & bit == 0 => self.seen |= bit,
            _ => self.ok = false,
        }
        Some(v)
    }

    const fn agree(&mut self, ok: bool) {
        self.ok &= ok;
    }

    pub fn num(&mut self, key: &str, v: impl Into<u64>) {
        let ok = self.take(key).and_then(Val::num) == Some(v.into());
        self.agree(ok);
    }

    /// A number, or `null` when absent.
    pub fn opt_num(&mut self, key: &str, v: Option<impl Into<u64>>) {
        match v {
            Some(n) => self.num(key, n),
            None => self.null(key),
        }
    }

    pub fn null(&mut self, key: &str) {
        let ok = self.take(key).is_some_and(Val::is_null);
        self.agree(ok);
    }

    pub fn flag(&mut self, key: &str, v: bool) {
        let ok = self.take(key).and_then(Val::bool) == Some(v);
        self.agree(ok);
    }

    /// Octets (a `_hex` key) or text.
    pub fn bytes(&mut self, key: &str, v: &[u8]) {
        let ok = self.take(key).and_then(Val::bytes) == Some(v);
        self.agree(ok);
    }

    pub fn text(&mut self, key: &str, v: &str) {
        self.bytes(key, v.as_bytes());
    }

    /// Text, or `null` when absent.
    pub fn opt_text(&mut self, key: &str, v: Option<&str>) {
        match v {
            Some(s) => self.text(key, s),
            None => self.null(key),
        }
    }

    /// A list of `Debug` names (anomaly sets).
    pub fn names<T: core::fmt::Debug>(&mut self, key: &str, items: impl Iterator<Item = T>) {
        let mut list = self.list(key);
        for item in items {
            let mut name = Text::new();
            let ok = write!(name, "{item:?}").is_ok();
            list.check(|v| ok && v.bytes() == Some(name.as_bytes()));
        }
        self.end(&list);
    }

    /// A nested object, compared exactly.
    pub fn obj(&mut self, key: &str, f: impl FnOnce(&mut Out)) {
        let ok = match self.take(key).and_then(Val::obj) {
            Some(map) => {
                let mut inner = Out::new(map);
                f(&mut inner);
                inner.finish()
            }
            None => false,
        };
        self.agree(ok);
    }

    /// A list, filled item by item through the returned cursor and closed with [`Out::end`].
    pub fn list(&mut self, key: &str) -> List {
        let items = self.take(key).and_then(Val::list);
        List {
            ok: items.is_some(),
            items: items.unwrap_or_default(),
            next: 0,
        }
    }

    /// Closes a list: every expected item was produced.
    pub const fn end(&mut self, list: &List) {
        self.agree(list.ok && list.next == list.items.len());
    }

    /// Every expected key was produced exactly once, and every produced value matched.
    pub fn finish(&self) -> bool {
        self.ok && usize::try_from(self.seen.count_ones()).is_ok_and(|n| n == self.expect.len())
    }
}

/// Cursor over an expected list.
pub struct List {
    items: &'static [Val],
    next: usize,
    ok: bool,
}

impl List {
    fn check(&mut self, f: impl FnOnce(Val) -> bool) {
        let ok = self.items.get(self.next).is_some_and(|&v| f(v));
        self.ok &= ok;
        self.next = self.next.saturating_add(1);
    }

    pub fn bytes(&mut self, v: &[u8]) {
        self.check(|x| x.bytes() == Some(v));
    }

    /// An object item, compared exactly.
    pub fn obj(&mut self, f: impl FnOnce(&mut Out)) {
        self.check(|x| {
            x.obj().is_some_and(|map| {
                let mut inner = Out::new(map);
                f(&mut inner);
                inner.finish()
            })
        });
    }
}

/// A fixed-size text sink for `Debug` names (no allocation).
pub struct Text {
    buf: [u8; 48],
    len: usize,
}

impl Text {
    pub const fn new() -> Text {
        Text {
            buf: [0; 48],
            len: 0,
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.buf.get(..self.len).unwrap_or_default()
    }
}

impl core::fmt::Write for Text {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let end = self.len.checked_add(s.len()).ok_or(core::fmt::Error)?;
        let dst = self.buf.get_mut(self.len..end).ok_or(core::fmt::Error)?;
        copy(dst, s.as_bytes())
            .then_some(())
            .ok_or(core::fmt::Error)?;
        self.len = end;
        Ok(())
    }
}

/// Copies `src` into `dst` when their lengths agree (no panic path, unlike
/// `copy_from_slice`).
pub fn copy(dst: &mut [u8], src: &[u8]) -> bool {
    if dst.len() != src.len() {
        return false;
    }
    for (d, s) in dst.iter_mut().zip(src) {
        *d = *s;
    }
    true
}

/// The first `len` octets of `buf`, zeroed: a fresh output buffer of exactly the size the host
/// suite allocates, since a buffer's size can decide the outcome (`BufferTooSmall`).
pub fn fresh(buf: &mut [u8], len: usize) -> Result<&mut [u8], Fail> {
    let out = need(buf.get_mut(..len))?;
    out.fill(0);
    Ok(out)
}

/// Two output buffers, each large enough for the largest Space Packet.
pub struct Scratch {
    pub a: [u8; aldis_spp::MAX_PACKET_LEN],
    pub b: [u8; aldis_spp::MAX_PACKET_LEN],
}

/// Runs every embedded case through its crate's handler. A case whose length parameter does
/// not fit in this target's `usize` cannot be expressed here and is counted apart.
#[expect(
    clippy::large_stack_arrays,
    reason = "two buffers for the largest Space Packet; the stack is 256 KiB (link-*.x)"
)]
pub fn run(t: &mut Tally) {
    let mut scratch = Scratch {
        a: [0; aldis_spp::MAX_PACKET_LEN],
        b: [0; aldis_spp::MAX_PACKET_LEN],
    };
    for case in CODEC {
        let mut out = Out::new(match case.expect {
            Expect::Ok(map) => map,
            Expect::Err(..) => &[],
        });
        let result = match case.group {
            Group::Ocf => crate::ocf::vector(case, &mut out),
            Group::Time => crate::time::vector(case, &mut out, &mut scratch),
            Group::Spp => crate::spp::vector(case, &mut out, &mut scratch),
            Group::Epp => crate::epp::vector(case, &mut out, &mut scratch),
            Group::Aos => crate::aos::vector(case, &mut out),
            Group::Tm => crate::tm::vector(case, &mut out, &mut scratch),
            Group::Tc => crate::tc::vector(case, &mut out, &mut scratch),
        };
        let passed = match (result, &case.expect) {
            (Err(Fail::NotRepresentable), _) => {
                t.not_representable();
                continue;
            }
            (Ok(()), Expect::Ok(_)) => out.finish(),
            (Err(Fail::Api(e)), Expect::Err(kind, field)) => {
                e.kind().name() == *kind
                    && field.is_none_or(|code| e.field().map(|f| f.code().get()) == Some(code))
            }
            _ => false,
        };
        t.check(passed);
        if !passed {
            crate::target::failed(case.id);
        }
    }
}

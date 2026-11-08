//! `ByteQueue` (of `aldis-testkit`) and `FixedVec` against `VecDeque`/`Vec` models under
//! arbitrary operation sequences (GEN-010…GEN-012, API-061, GEN-042).
#![no_main]

// req: GEN-010, GEN-011, GEN-012, API-061, GEN-042, VER-022

use std::collections::VecDeque;

use aldis_core::ErrorKind;
use aldis_core::container::FixedVec;
use aldis_testkit::byte_queue::{ByteQueue, ByteQueueError};
use libfuzzer_sys::arbitrary::{Result, Unstructured};
use libfuzzer_sys::fuzz_target;

const N: usize = 7;
const BYTES: usize = 40;
const PREFIX: usize = 4;

fn byte_queue(u: &mut Unstructured<'_>) -> Result<()> {
    let mut q: ByteQueue<BYTES> = ByteQueue::new();
    let mut model: VecDeque<Vec<u8>> = VecDeque::new();
    let used = |m: &VecDeque<Vec<u8>>| m.iter().map(|r| r.len() + PREFIX).sum::<usize>();
    while !u.is_empty() {
        match u.int_in_range(0..=3)? {
            0 => {
                let len = usize::from(u.int_in_range(0_u8..=40)?);
                let data = u.bytes(len)?.to_vec();
                let r = q.push(&data);
                if data.len() + PREFIX > BYTES {
                    assert!(matches!(r, Err(ByteQueueError::RecordTooLong { .. })));
                } else if data.len() + PREFIX > BYTES - used(&model) {
                    assert_eq!(r, Err(ByteQueueError::WouldBlock));
                } else {
                    assert_eq!(r, Ok(()));
                    model.push_back(data);
                }
            }
            1 => {
                let cap = usize::from(u.int_in_range(0_u8..=40)?);
                let mut out = vec![0_u8; cap];
                let r = q.pop_into(&mut out);
                match model.front() {
                    None => assert_eq!(r, Ok(None)),
                    Some(rec) if rec.len() > cap => {
                        assert_eq!(r, Err(ByteQueueError::BufferTooSmall { needed: rec.len() }));
                        // API-061: the record stays queued.
                    }
                    Some(rec) => {
                        assert_eq!(r, Ok(Some(rec.len())));
                        assert_eq!(&out[..rec.len()], rec.as_slice());
                        model.pop_front();
                    }
                }
            }
            2 => assert_eq!(q.peek_len(), model.front().map(Vec::len)),
            _ => assert_eq!(q.discard(), model.pop_front().map(|r| r.len())),
        }
        assert_eq!(q.records(), model.len());
        assert_eq!(q.is_empty(), model.is_empty());
        assert_eq!(q.free(), BYTES - used(&model));
        assert_eq!(q.capacity(), BYTES);
    }
    Ok(())
}

fn fixed_vec(u: &mut Unstructured<'_>) -> Result<()> {
    let mut v: FixedVec<u16, N> = FixedVec::new();
    let mut model: Vec<u16> = Vec::new();
    while !u.is_empty() {
        match u.int_in_range(0..=2)? {
            0 => {
                let x: u16 = u.arbitrary()?;
                let r = v.push(x).map_err(|e| e.kind());
                if model.len() < N {
                    assert_eq!(r, Ok(()));
                    model.push(x);
                } else {
                    assert_eq!(r, Err(ErrorKind::CapacityExceeded));
                }
            }
            1 => assert_eq!(v.pop(), model.pop()),
            _ => {
                v.clear();
                model.clear();
            }
        }
        assert_eq!(v.as_slice(), model.as_slice());
        assert_eq!(v.len(), model.len());
        assert_eq!(v.is_empty(), model.is_empty());
    }
    Ok(())
}

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    let _ = match u.int_in_range(0..=1) {
        Ok(0) => byte_queue(&mut u),
        Ok(_) => fixed_vec(&mut u),
        Err(e) => Err(e),
    };
});

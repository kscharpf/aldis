### Changed
- **Breaking:** Missed in the 0.2.0-m1 notes: `aldis-core::bits::get` and `set` report a field
  wider than the whole buffer as `buf_bits` (expected: the field width in bits, found: the
  buffer length in bits) instead of `bit_offset`. `bits::get(&[0], 0, 16)` returned
  `OutOfRange (bit_offset): expected 8, found 0` in 0.1.0-m0 and returns
  `OutOfRange (buf_bits): expected 16, found 8` since 0.2.0-m1.
- **Breaking:** Missed in the 0.2.0-m1 notes: `aldis-core::bits::get` and `set` report width 0
  as `OutOfRange (width): expected 1, found 0` instead of `expected 64, found 0`.
- **Breaking:** Missed in the 0.2.0-m1 notes: `aldis-core::container::ByteQueue::push` on a
  queue smaller than its 4-octet length prefix reports `capacity` (expected 4, found the
  capacity) instead of `record_len`. `ByteQueue::<0>::push(&[])` returned
  `OutOfRange (record_len): expected 0, found 0` in 0.1.0-m0 and returns
  `OutOfRange (capacity): expected 4, found 0` since 0.2.0-m1.
- **Breaking:** Missed in the 0.2.0-m1 notes: the `bit_offset` error of `aldis-core::bits::get`
  and `set` gives the largest valid offset as `expected`, not the buffer length in bits.
  `bits::get(&[0; 2], 9, 8)` returned `OutOfRange (bit_offset): expected 16, found 9` in
  0.1.0-m0 and returns `expected 8, found 9` since 0.2.0-m1.
- **Breaking:** Missed in the 0.2.0-m1 notes: `aldis-core::bits::read_u16` and `write_u16` give
  the largest valid offset as `expected`, and report a buffer shorter than two octets as
  `buf_len`. `read_u16(&[0; 4], 3)` went from `OutOfRange (offset): expected 4, found 3` to
  `expected 2, found 3`, and `read_u16(&[0], 0)` from `OutOfRange (offset): expected 1, found 0`
  to `OutOfRange (buf_len): expected 2, found 1`.
- **Breaking:** Missed in the 0.2.0-m1 notes: `aldis-core::counter::ModCounter::new` reports
  0 bits as `OutOfRange (bits): expected 1, found 0` instead of `expected 64, found 0`.
- TV-021: `superseded.by` is optional, as the vector schema and the `aldis-testkit` model already
  allowed. A wrong released case with no correct replacement is superseded with a reason only;
  a `by` that is present must name an existing case that is not itself superseded.
  `cargo xtask vectors validate` no longer requires it.

### Fixed
- `cargo xtask vectors validate` (TV-021): compares only with release tags (`v[0-9]*`) reachable
  from HEAD, so a branch or `git bisect` step that predates a newer release, or a stray tag
  such as `vendor-snapshot`, no longer fails validation; reads released files without the
  `git cat-file --batch` deadlock that hung on tags with thousands of files; lists released
  files with `git ls-tree -z`, so a released file with a non-ASCII name can no longer be
  deleted unnoticed; reports a problem with one tag or file and goes on instead of stopping;
  reports a current file that is not valid JSON once, as a parse error, instead of also as
  "removed or renamed"; reports each problem once, naming the oldest release, instead of
  once per release and twice for a missing `by`. Its git logic is tested against temporary
  repositories.
- `aldis-core`: the internal `usize` narrowing helpers also check against `u8::MAX` and
  `u16::MAX`, so a limit above the target type can never truncate a value (GEN-020). No
  public behaviour changes.

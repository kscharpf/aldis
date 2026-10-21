# Record: CABI-001, CABI-002, CABI-003

| | |
|---|---|
| Requirements | CABI-001, CABI-002, CABI-003 |
| Method | A |
| Check | Verify check `header`: `cargo xtask header` |

Verify check `header` (`xtask/src/header.rs`), nightly `ci.yml` job `header`. `cargo xtask header` regenerates `include/aldis/aldis.h` with the pinned `cbindgen` 0.29.4 and fails when the committed header differs or `cbindgen` is another version (CABI-002), and when the header includes anything but `<stdint.h>`, `<stddef.h>`, and `<stdbool.h>` or has no `extern "C"` block under `__cplusplus` (CABI-003). `cargo xtask header c-smoke gcc-14 g++-14` and `c-smoke clang clang++`, with the pinned GCC and Clang, build the static and shared host libraries and fail if either is missing (CABI-001), then compile `crates/aldis-ffi/tests/c/smoke.c` as C99 against the static library and as C++11 against the shared one with `-pedantic -Wall -Wextra -Werror`, link, and run both (CABI-003). The static library for bare metal: verify checks `no-std-<target>` (CABI-001). VER-050's full compiler matrix stays in M5

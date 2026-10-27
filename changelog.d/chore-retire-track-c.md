### Removed
- Retired Track C (native C++) from v1 and deferred to roadmap Phase 3; archived native C++ codebase at git tag `c0-archive`. Deleted `cpp/` and `.github/workflows/cpp.yml`.
- Withdrew 34 `CPP-*` requirements in `docs/profiles/cpp.md`, plus `VER-052`, `VER-053`, `CABI-070`, and `CABI-071`. Dropped CMake, Ninja, CodeQL, and GNU bare-metal toolchains from `Containerfile`.

### Changed
- Decoupled shared toolchain pins (`QEMU_VERSION`, `LLVM_VERSION`, `GCC_VERSION`, `GCC_S390X_*`) to root `toolchains.lock`.
- Reopened OI-08: LEON/SPARC support remains best-effort Rust (tier 3 nightly with `-Z build-std=core`, non-gating check `rust-sparc-leon3`); no qualified LEON path is claimed for v1.
- Updated `docs/08-roadmap.md`, `README.md`, `docs/hardware-assumptions.md`, `docs/profiles/c-abi.md`, and `docs/profiles/cpp.md`.

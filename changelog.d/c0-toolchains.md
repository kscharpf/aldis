### Added
- C0 WP-0: pinned C++ toolchains in `cpp/toolchains.lock` (CPP-001, CPP-003): LLVM 23.1.3, Ubuntu 24.04 GCC 14.2.0 (host and s390x cross), Arm GNU Toolchain 14.3.Rel1, xPack RISC-V GCC 14.3.0-1, Gaisler BCC2 2.2.4 (GCC 10.2.0, LEON3, non-gating), CMake 4.4.4, Ninja 1.13.2, QEMU 8.2.2, and the CodeQL 2.21.4 bundle with the Coding Standards packs 2.62.0, each with its download URL and SHA-256 or exact package version.
- Composite action `.github/actions/setup-cpp`, the counterpart of `setup-rust`: installs exactly the locked versions, caches archives by checksum, and checks and prints every version.
- `cpp/cmake/AldisToolchainCheck.cmake`: configure fails when the compiler differs from the lock (D1).
- Workflow `.github/workflows/cpp.yml` (name "C++", D17) with job `cpp-toolchains`, which installs the whole set on `ubuntu-24.04` and `ubuntu-24.04-arm` and checks that a changed checksum and a mismatching compiler are rejected.

### Changed
- Milestone C0 started ([docs/milestones/c0-foundations.md](docs/milestones/c0-foundations.md)).

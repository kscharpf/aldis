//! The checks of the verification run (VER-056): one per job of the former pull-request
//! workflows, with the same names (`.github/workflows/ci.yml`, `cpp.yml`), so that
//! `docs/verification/analysis-records.md` can name them.
//!
//! Each check is a Bash script run with `-euo pipefail` from the root of the checkout, with
//! `RUNNER_TEMP` set to an empty directory of its own.

/// Bare-metal Rust targets of the matrix (docs/06-verification.md §4).
const RUST_BARE_METAL: &[&str] = &[
    "thumbv7em-none-eabihf",
    "armv7r-none-eabihf",
    "riscv32imac-unknown-none-elf",
];

/// Where a check can run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Host {
    /// In the verification image, on any supported host: a portable check.
    Container,
    /// In the verification image, only on an `x86_64` host (the tool exists only for it).
    ContainerX64,
    /// On the host itself, only when it is macOS.
    MacOs,
}

impl Host {
    /// The name recorded in the evidence record.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Container => "container",
            Self::ContainerX64 => "container-x86_64",
            Self::MacOs => "macos",
        }
    }
}

/// One check of the verification run.
#[derive(Clone, Debug)]
pub struct Check {
    /// Stable name, as the former CI job (`clippy`, `exerciser-thumbv7em-none-eabihf`, ...).
    pub name: String,
    /// Where it runs.
    pub host: Host,
    /// A C++ check, skipped when the change does not touch the C++ inputs (VER-056).
    pub cpp: bool,
    /// A failure fails the run. Only the LEON3 build is reported without gating (OI-08).
    pub gating: bool,
    /// The Bash script.
    pub script: String,
}

fn check(name: impl Into<String>, host: Host, cpp: bool, script: impl Into<String>) -> Check {
    Check {
        name: name.into(),
        host,
        cpp,
        gating: true,
        script: script.into(),
    }
}

/// Every check, in the order they run.
pub fn all() -> Vec<Check> {
    let mut out = rust();
    out.extend(cpp());
    out
}

fn rust() -> Vec<Check> {
    let c = Host::Container;
    let mut out = vec![
        check("fmt", c, false, FMT),
        check("clippy", c, false, CLIPPY),
        check("test", c, false, TEST),
        check("test-macos", Host::MacOs, false, TEST),
        // The alloc and std features are additive (GEN-006).
        check("features", c, false, "cargo xtask features\n"),
    ];
    for t in RUST_BARE_METAL {
        out.push(check(
            format!("no-std-{t}"),
            c,
            false,
            format!(
                "cargo build -p aldis --no-default-features --features full --target {t}\n\
                 cargo clippy -p aldis --no-default-features --features full --target {t}\n\
                 cargo build -p aldis-ffi --no-default-features --features panic-handler \
                 --target {t} --release\n"
            ),
        ));
    }
    out.extend([
        check("big-endian", c, false, BIG_ENDIAN),
        check("layers", c, false, LAYERS),
        check("refapp", c, false, REFAPP),
    ]);
    for t in RUST_BARE_METAL {
        out.push(check(format!("exerciser-{t}"), c, false, exerciser(t)));
    }
    out.extend([
        check("coverage", c, false, "cargo xtask coverage\n"),
        check("spec", c, false, SPEC),
        check("deny", c, false, DENY),
    ]);
    out
}

/// The exerciser check of one bare-metal target: the panic-freedom link check, its canary, and
/// the vector run under QEMU (VER-010, M1 D6). The QEMU command of each target is its runner in
/// `exerciser/.cargo/config.toml`.
fn exerciser(target: &str) -> String {
    format!(
        "cd exerciser\n\
         # Panic-freedom link check (VER-010).\n\
         cargo build --release --locked --target {target}\n\
         cargo clippy --release --locked --target {target}\n\
         # Canary: a reachable panic must fail the link.\n\
         if cargo build --release --locked --target {target} --features inject-panic \
         2> \"$RUNNER_TEMP/canary.log\"; then\n\
         \x20 echo 'exerciser linked with a reachable panic; the VER-010 check is broken' >&2\n\
         \x20 exit 1\n\
         fi\n\
         grep -q aldis_panic_is_unreachable \"$RUNNER_TEMP/canary.log\"\n\
         echo 'canary failed to link, as it should'\n\
         # Every vector case on the target under QEMU, through semihosting (M1 D6). QEMU can\n\
         # misbehave on a busy host (m0 plan risk table): one retry, each run time-limited.\n\
         timeout 120 cargo run --release --locked --target {target} \\\n\
         \x20 || timeout 120 cargo run --release --locked --target {target}\n"
    )
}

fn cpp() -> Vec<Check> {
    let c = Host::Container;
    let mut out = vec![
        check("cpp-toolchains", c, true, CPP_TOOLCHAINS),
        check("cpp-format", c, true, CPP_FORMAT),
    ];
    for (compiler, cxx) in [("gcc", "g++-14"), ("clang", "clang++")] {
        out.push(check(
            format!("cpp-build-{compiler}"),
            c,
            true,
            format!(
                "cd cpp\n\
                 cmake --preset host-{compiler}\n\
                 cmake --build --preset host-{compiler}\n\
                 ctest --preset host-{compiler}\n\
                 # Install, then build and run a consumer of the package (CPP-012).\n\
                 cmake --install ../target/cpp/host-{compiler} --prefix \"$RUNNER_TEMP/aldis\"\n\
                 cmake -S tests/package -B \"$RUNNER_TEMP/consumer\" -G Ninja \\\n\
                 \x20 -DCMAKE_CXX_COMPILER={cxx} -DCMAKE_PREFIX_PATH=\"$RUNNER_TEMP/aldis\"\n\
                 cmake --build \"$RUNNER_TEMP/consumer\"\n\
                 \"$RUNNER_TEMP/consumer/consumer\"\n"
            ),
        ));
    }
    out.extend([
        check("cpp-tidy", c, true, CPP_TIDY),
        check("cpp-sanitize", c, true, CPP_SANITIZE),
        check("cpp-big-endian", c, true, CPP_BIG_ENDIAN),
    ]);
    for target in ["cortex-m4f", "cortex-r5f", "rv32imac"] {
        for compiler in ["gcc", "clang"] {
            out.push(check(
                format!("cpp-bare-metal-{target}-{compiler}"),
                c,
                true,
                format!(
                    "cd cpp\n\
                     # Header checks; the exerciser, link check, and QEMU join in C0 WP-5.\n\
                     cmake --preset {target}-{compiler}\n\
                     cmake --build --preset {target}-{compiler}\n"
                ),
            ));
        }
    }
    out.push(Check {
        gating: false,
        ..check("cpp-leon3", Host::ContainerX64, true, CPP_LEON3)
    });
    out.push(check("cpp-layers", c, true, CPP_LAYERS));
    out
}

const FMT: &str = "\
cargo fmt --all --check
# The exerciser, fuzz targets, and cross-checks are separate workspaces.
cargo fmt --all --check --manifest-path exerciser/Cargo.toml
cargo fmt --all --check --manifest-path fuzz/Cargo.toml
cargo fmt --all --check --manifest-path crosscheck/Cargo.toml
";

const CLIPPY: &str = "\
cargo clippy --workspace --all-features --all-targets
cargo doc --workspace --all-features --no-deps
# Fuzz targets build on stable; only running them needs nightly (RS-002).
cargo clippy --manifest-path fuzz/Cargo.toml --all-targets
cargo clippy --manifest-path crosscheck/Cargo.toml --all-targets
";

const TEST: &str = "cargo test --workspace --all-features\n";

const BIG_ENDIAN: &str = "\
# s390x under QEMU user mode, linked with the pinned cross GCC (VER-030, GEN-007).
export CARGO_TARGET_S390X_UNKNOWN_LINUX_GNU_LINKER=s390x-linux-gnu-gcc-14
export CARGO_TARGET_S390X_UNKNOWN_LINUX_GNU_RUNNER='qemu-s390x -L /usr/s390x-linux-gnu'
cargo test --workspace --all-features --target s390x-unknown-linux-gnu
";

const LAYERS: &str = "\
cargo xtask layers
cargo xtask symbols
# refapp/ uses Aldis only through the facade and forbids unsafe (APP-100, APP-105).
cargo xtask refapp-deps
";

const REFAPP: &str = "\
# refapp/ is its own workspace (docs/reference-application.md §9, APP-106).
cd refapp
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked
cargo test --workspace --locked
# Flight node, bare metal (APP-102).
cargo build -p refapp-flight --target thumbv7em-none-eabihf --locked
cargo clippy -p refapp-flight --target thumbv7em-none-eabihf --locked
# Generated scenario files are current (APP-107).
cargo run -p refapp-harness --locked -- gen-scenario --check
# Golden traces are canonical and reproduced (APP-080, APP-085).
cargo run -p refapp-harness --locked -- check-golden
cargo run -p refapp-harness --locked -- compare
";

const SPEC: &str = "\
cargo xtask spec-lint
cargo xtask links
cargo xtask vectors validate
cargo xtask vectors schema --check
# Independent cross-check of the vectors (VER-040); fails on any disagreement. Its own tests
# first: the harness must report a difference it is shown.
cargo test --manifest-path crosscheck/Cargo.toml
cargo run --manifest-path crosscheck/Cargo.toml
cargo xtask changelog check
# M0 is closed: every in-scope requirement must keep an artefact (VER-020).
cargo xtask trace --scope m0 --strict
# M1 is closed: every in-scope requirement must keep an artefact (VER-020, m1 plan D8).
cargo xtask trace --scope m1 --strict
# C0 is open: its trace is reported, and becomes --strict at the C0 exit (c0 plan D17).
cargo xtask trace --scope c0
";

const DENY: &str = "\
cargo deny --all-features check
# The reference application's own workspace, same policy (APP-106).
cargo deny --manifest-path refapp/Cargo.toml --all-features --log-level info check
# The cross-check workspace's own deny.toml: the root policy plus the exceptions recorded in
# docs/verification/cross-checks.md §1.
cargo deny --manifest-path crosscheck/Cargo.toml --all-features --log-level info check
";

const CPP_TOOLCHAINS: &str = r#"
setup=.github/actions/setup-cpp/setup_cpp.py
python3 -I "$setup" check-lock
# The image installed every pinned tool for this architecture; check what is on PATH.
python3 -I "$setup" versions all
# A changed checksum fails the install: change the last hex digit of the Ninja checksum.
python3 -I - "$RUNNER_TEMP/changed.lock" <<'EOF'
import platform, sys
key = "NINJA_" + ("X64" if platform.machine() == "x86_64" else "ARM64") + "_SHA256="
lines = open("cpp/toolchains.lock", encoding="utf-8").read().splitlines()
for i, line in enumerate(lines):
    if line.startswith(key):
        lines[i] = line[:-1] + ("1" if line[-1] == "0" else "0")
open(sys.argv[1], "w", encoding="utf-8").write("\n".join(lines) + "\n")
EOF
if cmp -s cpp/toolchains.lock "$RUNNER_TEMP/changed.lock"; then
  echo "the test did not change the checksum" >&2
  exit 1
fi
if python3 -I "$setup" --lock "$RUNNER_TEMP/changed.lock" --downloads "$RUNNER_TEMP/downloads" \
     --tools-dir "$RUNNER_TEMP/tools" install ninja 2> "$RUNNER_TEMP/changed.log"; then
  echo "setup-cpp accepted an archive whose checksum differs from the lock" >&2
  exit 1
fi
cat "$RUNNER_TEMP/changed.log"
grep -q "checksum mismatch" "$RUNNER_TEMP/changed.log"
echo "the changed checksum was rejected, as it should be"
# The configure-time compiler check accepts the pinned compilers ...
cmake -S cpp/cmake/selftest/toolchain-check -B "$RUNNER_TEMP/tc-gcc" -G Ninja \
  -DCMAKE_CXX_COMPILER=g++-14 -DALDIS_TOOLCHAIN=GCC
cmake -S cpp/cmake/selftest/toolchain-check -B "$RUNNER_TEMP/tc-clang" -G Ninja \
  -DCMAKE_CXX_COMPILER=clang++ -DALDIS_TOOLCHAIN=LLVM
# ... and rejects other compilers and versions.
sed -E 's/^GCC_VERSION=.*/GCC_VERSION=14.1.0/' cpp/toolchains.lock > "$RUNNER_TEMP/old-gcc.lock"
for args in "-DCMAKE_CXX_COMPILER=g++-14 -DALDIS_TOOLCHAIN=LLVM" \
            "-DCMAKE_CXX_COMPILER=clang++ -DALDIS_TOOLCHAIN=GCC" \
            "-DCMAKE_CXX_COMPILER=g++-14 -DALDIS_TOOLCHAIN=GCC -DALDIS_TOOLCHAINS_LOCK=$RUNNER_TEMP/old-gcc.lock" \
            "-DCMAKE_CXX_COMPILER=g++-14 -DALDIS_TOOLCHAIN=LLVM -DALDIS_ALLOW_UNPINNED_COMPILER=ON"; do
  rm -rf "$RUNNER_TEMP/tc-bad"
  # args is split into words on purpose.
  if cmake -S cpp/cmake/selftest/toolchain-check -B "$RUNNER_TEMP/tc-bad" -G Ninja $args; then
    echo "configure accepted a compiler that differs from the lock ($args)" >&2
    exit 1
  fi
done
echo "every mismatching compiler was rejected, as it should be"
"#;

const CPP_FORMAT: &str = "\
git ls-files -z -- 'cpp/*.hpp' 'cpp/*.cpp' 'cpp/*.h' \
| xargs -0 --no-run-if-empty clang-format --dry-run --Werror
";

const CPP_TIDY: &str = "\
cd cpp
cmake --preset host-clang
# Findings are errors (WarningsAsErrors in cpp/.clang-tidy); GoogleTest is not analysed.
run-clang-tidy -quiet -p ../target/cpp/host-clang '/header_check/|/cpp/tests/'
";

const CPP_SANITIZE: &str = "\
cd cpp
cmake --preset host-clang-sanitize
cmake --build --preset host-clang-sanitize
ctest --preset host-clang-sanitize
";

const CPP_BIG_ENDIAN: &str = "\
cd cpp
cmake --preset s390x-gcc
cmake --build --preset s390x-gcc
ctest --preset s390x-gcc
";

const CPP_LEON3: &str = "\
# Reported against OI-08, never a gate (c0 plan D2).
cd cpp
cmake --preset leon3-gcc
cmake --build --preset leon3-gcc
";

const CPP_LAYERS: &str = "\
# Bounds-checking opt-outs only in aldis/core/bytes.hpp; every NOLINT names its check.
cargo xtask cpp unsafe-optouts
(cd cpp && cmake --preset host-gcc)
# CMake target graph against the layer rules.
cargo xtask cpp layers target/cpp/host-gcc
";

#[cfg(test)]
mod tests {
    use super::*;

    // req: VER-056
    #[test]
    fn names_are_unique_and_stable() {
        let checks = all();
        let mut names: Vec<&str> = checks.iter().map(|c| c.name.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), checks.len());
        assert!(names.iter().all(|n| {
            n.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        }));
    }

    // req: VER-056
    #[test]
    fn only_the_leon3_build_is_non_gating() {
        let non_gating: Vec<String> = all()
            .into_iter()
            .filter(|c| !c.gating)
            .map(|c| c.name)
            .collect();
        assert_eq!(non_gating, ["cpp-leon3"]);
    }

    // req: VER-010, GEN-021
    #[test]
    fn every_exerciser_check_links_the_canary_and_runs_under_qemu() {
        for target in RUST_BARE_METAL {
            let name = format!("exerciser-{target}");
            let check = all().into_iter().find(|c| c.name == name);
            let script = check.map(|c| c.script).unwrap_or_default();
            assert!(script.contains("--features inject-panic"), "{name}");
            assert!(
                script.contains(&format!(
                    "timeout 120 cargo run --release --locked --target {target}"
                )),
                "{name}"
            );
        }
    }
}

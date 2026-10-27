//! The checks of the verification run (VER-056): one per job of the former pull-request
//! workflows, with the same names (`.github/workflows/ci.yml`, `cpp.yml`), so that
//! `docs/verification/analysis-records.md` can name them.
//!
//! Each check is a Bash script run with `-euo pipefail` from the root of the checkout, with
//! `RUNNER_TEMP` set to an empty directory of its own.

/// The reason recorded for a nightly-only check ([`Check::nightly`]) in a run without `--all`.
pub const NIGHTLY_ONLY: &str = "nightly only (VER-059)";

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

/// A named list of path patterns: `*` matches within one path segment, `**` any number of
/// segments (`scope::matches`).
#[derive(Debug, PartialEq, Eq)]
pub struct PathList {
    /// Shown by `cargo xtask verify list`.
    pub name: &'static str,
    /// The patterns, relative to the repository root.
    pub patterns: &'static [&'static str],
}

// The inputs of the checks (VER-056): every path a check builds, reads, or is run by. A check
// runs when a changed path matches one of its inputs, and a changed path that matches no check's
// inputs and is in none of the lists of `SPEC_ONLY` runs every check. Precision never at the cost of soundness: when a
// check starts reading a new path, add the path here in the same change; when unsure, include it.
//
// The differential tests of track C (`difftest/`, VER-053, milestone C1) will read both the Rust
// crates and `cpp/`: their checks take `Inputs::Paths(&[&RUN, &RUST, &CPP, &DIFFTEST])`, with a
// `DIFFTEST` list naming `difftest/**` and whatever else they read.

/// Read by every check: the verification run itself. `xtask/` holds the harness that runs and
/// judges every check, the image definition (`xtask/verify/`), and every `cargo xtask` command;
/// building xtask reads the workspace manifests, `.cargo/`, and `aldis-testkit`; the image is
/// built from the files of `image::INPUTS`.
pub const RUN: PathList = PathList {
    name: "run",
    patterns: &[
        "xtask/**",
        "Cargo.toml",
        "Cargo.lock",
        ".cargo/**",
        "crates/*/Cargo.toml",
        "crates/aldis-testkit/**",
        "rust-toolchain.toml",
        "tooling-nightly",
        "cpp/toolchains.lock",
        ".github/actions/setup-cpp/setup_cpp.py",
    ],
};

/// Read by every Rust check: the crates and the separate workspaces (`exerciser/`, `fuzz/`,
/// `refapp/`, `crosscheck/`), their lint, format, licence, and coverage settings, the vectors that
/// their tests and the exerciser load, and the SANA snapshots a test compares with the generated
/// tables (`aldis-uslp/tests/restricted.rs`). One list for all of them: a finer split would save
/// little and risk missing a path. The workflows, which no check reads, are in `GITHUB`.
pub const RUST: PathList = PathList {
    name: "rust",
    patterns: &[
        "crates/**",
        "exerciser/**",
        "fuzz/**",
        "refapp/**",
        "crosscheck/**",
        "test-vectors/**",
        "specs/sana/**",
        "clippy.toml",
        "rustfmt.toml",
        ".rustfmt.toml",
        "deny.toml",
        "coverage-waivers.toml",
        "LICENSE-MIT",
        "LICENSE-APACHE",
    ],
};

/// Also read by the checks that run the xtask unit tests (`test`, `test-macos`, `big-endian`):
/// `spec::tests::cpp_sources_are_checked_for_requirement_ids` lists the C++ files (CPP-057).
pub const XTASK_TESTS: PathList = PathList {
    name: "xtask-tests",
    patterns: &["cpp/**"],
};

/// Read by the C++ checks: the C++ tree and the vectors (read by the C++ tests from C1). The C++
/// setup script, which the image runs, is in `RUN`; the action's definition and the workflow
/// `cpp.yml` are in `GITHUB`.
pub const CPP: PathList = PathList {
    name: "cpp",
    patterns: &["cpp/**", "test-vectors/**"],
};

/// Read by the `header` check (CABI-001…003): the C ABI crate `aldis-ffi`, every crate that its
/// `aldis` dependency builds with `full`, which the check compiles into the static and shared
/// libraries, and the committed header `include/`. One pattern for the crates: `full` enables
/// all of them, and a narrower list would miss a crate when the facade gains it.
pub const FFI: PathList = PathList {
    name: "ffi",
    patterns: &["crates/**", "include/**"],
};

/// Read by the non-gating check `kani` (OI-42): the crates with Kani proof harnesses, whose
/// `proofs.rs` and the code it calls are what the check proves. Their manifests, the workspace,
/// and `xtask` (which runs the harnesses) are in `RUN`.
pub const KANI: PathList = PathList {
    name: "kani",
    patterns: &[
        "crates/aldis-core/**",
        "crates/aldis-spp/**",
        "crates/aldis-time/**",
    ],
};

/// Read by the check `stack` (VER-011, GEN-022): the library crates and `aldis-ffi`, whose
/// static library it builds for each bare-metal target; the exerciser, which it also builds, and
/// the vectors that the exerciser's build script embeds. Its budgets and exemptions
/// (`xtask/stack-budgets.toml`), its module, and the tooling nightly that builds it are in `RUN`.
pub const STACK: PathList = PathList {
    name: "stack",
    patterns: &["crates/**", "exerciser/**", "test-vectors/**"],
};

/// Documentation: read by `spec` only (spec lint, links, vectors, field catalogue, traces,
/// changelog check), so a change to these paths alone runs only `spec`; a change to the field
/// catalogue in `docs/03-api-model.md` alone fails `spec` until `cargo xtask fields --write`
/// regenerates its files, which are Rust and C++ inputs. Not documentation: the crate READMEs,
/// which are doctests, `test-vectors/README.md`, which lies under the Rust and C++ input
/// `test-vectors/`, and every other `.md` file outside these paths, such as `cpp/README.md`
/// (installed by the C++ build) and `refapp/golden/CHANGES.md`.
pub const DOCS: PathList = PathList {
    name: "docs",
    patterns: &[
        "docs/**",
        "changelog.d/**",
        "*.md",
        "specs/README.md",
        "specs/manifest.tsv",
    ],
};

/// The GitHub configuration that no check reads but `spec` (which reads every path: spec lint
/// scans the workflows for requirement IDs, `consistency` reads `nightly.yml`, and `links` the
/// pull-request template), so that a change to these paths alone runs only `spec`, as
/// documentation does: the workflows, the definitions of the composite actions, the pull-request
/// template, the code owners, and the Dependabot configuration. A check takes its tools from the
/// image and gives the same result on a contributor's machine, which never reads these files, as
/// in `verify.yml` (VER-060); the nightly runs, which use the workflows, run every check
/// (VER-059). Not here: `.github/actions/setup-cpp/setup_cpp.py`, which the image runs (`RUN`),
/// and any other file of `.github/`, which is in no list and so runs every check.
pub const GITHUB: PathList = PathList {
    name: "github",
    patterns: &[
        ".github/workflows/**",
        ".github/actions/setup-rust/**",
        ".github/actions/setup-cpp/action.yml",
        ".github/pull_request_template.md",
        ".github/CODEOWNERS",
        ".github/dependabot.yml",
    ],
};

/// The root configuration that no check reads but `spec` (which reads every path), so that a
/// change to these paths alone runs only `spec`, as documentation does: `.gitignore`, which says
/// only which files git leaves untracked. Every check runs in a fresh checkout of the commit,
/// which has no untracked files, and none lists untracked files (`git ls-files` without
/// `--others`; the directories that xtask skips are its own list, `repo::SKIP`). Not here:
/// `.gitattributes`, whose attributes can change the bytes a checkout writes (`text`, `eol`,
/// `filter`), and every other root file, which is in no list and so runs every check.
pub const ROOT_CONFIG: PathList = PathList {
    name: "root-config",
    patterns: &[".gitignore"],
};

/// The lists that `spec` alone reads (VER-056): documentation, the GitHub configuration, and the
/// root configuration that no other check reads.
pub const SPEC_ONLY: [&PathList; 3] = [&DOCS, &GITHUB, &ROOT_CONFIG];

/// What a check reads (VER-056).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Inputs {
    /// Every path of the repository: the check runs on every change.
    Everything,
    /// The paths that match a pattern of one of these lists.
    Paths(&'static [&'static PathList]),
}

impl Inputs {
    /// Whether `path` is one of them.
    pub fn contains(self, path: &str) -> bool {
        match self {
            Self::Everything => true,
            Self::Paths(lists) => lists.iter().any(|l| l.contains(path)),
        }
    }

    /// Every pattern, as the record lists them.
    pub fn patterns(self) -> Vec<&'static str> {
        match self {
            Self::Everything => vec!["**"],
            Self::Paths(lists) => lists
                .iter()
                .flat_map(|l| l.patterns.iter().copied())
                .collect(),
        }
    }

    /// The names of the lists, for `cargo xtask verify list`.
    pub fn label(self) -> String {
        match self {
            Self::Everything => "every path".to_owned(),
            Self::Paths(lists) => lists.iter().map(|l| l.name).collect::<Vec<_>>().join("+"),
        }
    }
}

impl PathList {
    /// Whether `path` matches one of the patterns.
    pub fn contains(&self, path: &str) -> bool {
        self.patterns.iter().any(|p| super::scope::matches(p, path))
    }
}

const RUST_INPUTS: Inputs = Inputs::Paths(&[&RUN, &RUST]);
const RUST_TEST_INPUTS: Inputs = Inputs::Paths(&[&RUN, &RUST, &XTASK_TESTS]);
const CPP_INPUTS: Inputs = Inputs::Paths(&[&RUN, &CPP]);
const FFI_INPUTS: Inputs = Inputs::Paths(&[&RUN, &FFI]);
const KANI_INPUTS: Inputs = Inputs::Paths(&[&RUN, &KANI]);
const STACK_INPUTS: Inputs = Inputs::Paths(&[&RUN, &STACK]);

/// One check of the verification run.
#[derive(Clone, Debug)]
pub struct Check {
    /// Stable name, as the former CI job (`clippy`, `exerciser-thumbv7em-none-eabihf`, ...).
    pub name: String,
    /// Where it runs.
    pub host: Host,
    /// A C++ check: `--all-cpp` runs it even when none of its inputs changed.
    pub cpp: bool,
    /// What it reads: it is skipped when none of these changed (VER-056).
    pub inputs: Inputs,
    /// A failure fails the run. Only the LEON3 checks, Rust and C++ (OI-08), and the Kani proofs
    /// (OI-42) are reported without gating.
    pub gating: bool,
    /// A nightly-only check: it runs only in a run with `--all`, which is what the nightly
    /// verification run uses (VER-059), and every other run records it as not run with
    /// [`NIGHTLY_ONLY`]. It is never skipped, reused, or a source (VER-056). Only `kani` (OI-42,
    /// maintainer decision 2026-10-24).
    pub nightly: bool,
    /// Its typical duration in seconds, which orders the run only ([`run_order`]): the median
    /// of its durations in the published records of the GitHub-hosted runs of VER-060
    /// (`x86_64`, an empty build cache) from 2026-10-19 to 2026-10-24, rounded; for
    /// `test-macos`, which runs on macOS only, the median of the local runs. A value that has
    /// gone stale changes the order in which the checks run, never a result.
    pub secs: u16,
    /// The Bash script.
    pub script: String,
}

fn check(
    name: impl Into<String>,
    host: Host,
    cpp: bool,
    secs: u16,
    script: impl Into<String>,
) -> Check {
    Check {
        name: name.into(),
        host,
        cpp,
        inputs: if cpp { CPP_INPUTS } else { RUST_INPUTS },
        gating: true,
        nightly: false,
        secs,
        script: script.into(),
    }
}

/// `check` with other inputs than its kind's.
fn reading(inputs: Inputs, check: Check) -> Check {
    Check { inputs, ..check }
}

/// Every check, in table order: the order of the record and of `verify list`.
pub fn all() -> Vec<Check> {
    let mut out = rust();
    out.extend(cpp());
    out
}

/// Every check, in the order a run executes them: the gating checks first, the quickest first
/// (by [`Check::secs`], ties in table order), then the non-gating ones the same way. A run that
/// stops at its first gating failure (`--fail-fast`, the default with `--publish`) then stops
/// as early as it can, and a non-gating check, which never stops a run, never delays a stop.
/// No check depends on another having run before it: each builds what it needs.
pub fn run_order() -> Vec<Check> {
    let mut out = all();
    out.sort_by_key(|c| (!c.gating, c.secs));
    out
}

fn rust() -> Vec<Check> {
    let c = Host::Container;
    let mut out = vec![
        check("fmt", c, false, 1, FMT),
        check("clippy", c, false, 29, CLIPPY),
        reading(RUST_TEST_INPUTS, check("test", c, false, 18, TEST)),
        reading(
            RUST_TEST_INPUTS,
            check("test-macos", Host::MacOs, false, 23, TEST),
        ),
        // The alloc and std features are additive (GEN-006).
        check("features", c, false, 37, "cargo xtask features\n"),
        reading(FFI_INPUTS, check("header", c, false, 7, HEADER)),
    ];
    for t in RUST_BARE_METAL {
        out.push(check(
            format!("no-std-{t}"),
            c,
            false,
            3,
            format!(
                "cargo build --locked -p aldis --no-default-features --features full --target {t}\n\
                 cargo clippy --locked -p aldis --no-default-features --features full --target {t}\n\
                 cargo build --locked -p aldis-ffi --no-default-features --features panic-handler \
                 --target {t} --release\n\
                 # The C ABI static library needs no heap, threads, or C runtime (CABI-004).\n\
                 cargo xtask ffi-imports {t}\n"
            ),
        ));
    }
    out.extend([
        reading(
            RUST_TEST_INPUTS,
            check("big-endian", c, false, 63, BIG_ENDIAN),
        ),
        check("layers", c, false, 1, LAYERS),
        check("refapp", c, false, 10, REFAPP),
    ]);
    for t in RUST_BARE_METAL {
        out.push(check(format!("exerciser-{t}"), c, false, 10, exerciser(t)));
    }
    // The worst-case stack of every entry point within its budget, the call graph complete
    // (VER-011, GEN-022; M2 plan WP-7). Per target, the C ABI library and the exerciser on the
    // tooling nightly with `core` from source: about 35 s per target from a warm cache on a
    // macOS host; the duration is an estimate until the GitHub-hosted runs measure it.
    out.push(reading(
        STACK_INPUTS,
        check("stack", c, false, 150, "cargo xtask stack\n"),
    ));
    out.push(Check {
        gating: false,
        ..check("rust-sparc-leon3", c, false, 45, RUST_LEON3)
    });
    // The Kani proof harnesses (OI-42, docs/verification/formal-proofs.md): reported, not gating,
    // and run only at night (maintainer decision 2026-10-24).
    out.push(Check {
        gating: false,
        nightly: true,
        ..reading(
            KANI_INPUTS,
            check("kani", c, false, 641, "cargo xtask kani\n"),
        )
    });
    out.extend([
        check("coverage", c, false, 48, "cargo xtask coverage\n"),
        // Spec lint, links, vectors, traces, and changelog: it reads every part of the tree.
        reading(Inputs::Everything, check("spec", c, false, 15, SPEC)),
        check("deny", c, false, 2, DENY),
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

/// The LEON3 (SPARC V8) check, reported against OI-08 and never a gate (docs/06-verification.md
/// §4). `sparc-unknown-none-elf` is tier 3, so `core` is built from source on the tooling
/// nightly (`-Z build-std`, RS-002) for `-C target-cpu=leon3`. The library builds as in
/// `no-std-<target>`, and the exerciser runs as in `exerciser-<target>`: the panic-freedom link
/// check, its canary, and every vector case under QEMU `leon3_generic` (VER-010). The linker
/// and the QEMU command are in `exerciser/.cargo/config.toml`.
const RUST_LEON3: &str = r#"
# Reported against OI-08, never a gate.
t=sparc-unknown-none-elf
nightly="$(cat tooling-nightly)"
std=(-Z build-std=core)
# The flag of exerciser/.cargo/config.toml, which RUSTFLAGS replaces. xtask, a host build,
# keeps the flags it had.
host_flags="${RUSTFLAGS:-}"
export RUSTFLAGS="$host_flags -C target-cpu=leon3"
cargo +"$nightly" build "${std[@]}" -p aldis --no-default-features --features full --target $t
cargo +"$nightly" clippy "${std[@]}" -p aldis --no-default-features --features full --target $t
cargo +"$nightly" build "${std[@]}" -p aldis-ffi --no-default-features --features panic-handler \
  --target $t --release
# CABI-004 is reported, not checked, on this target: the compiler_builtins that build-std makes
# for 32-bit SPARC refers to the quad-float routines of the SPARC ABI (_Q_add, ...), which
# nothing in the archive defines (OI-08).
RUSTFLAGS="$host_flags" cargo xtask ffi-imports $t || echo "ffi-imports $t: CABI-004 not met (OI-08)"
cd exerciser
# Panic-freedom link check (VER-010).
cargo +"$nightly" build "${std[@]}" --release --locked --target $t
cargo +"$nightly" clippy "${std[@]}" --release --locked --target $t
# Canary: a reachable panic must fail the link.
if cargo +"$nightly" build "${std[@]}" --release --locked --target $t --features inject-panic \
     2> "$RUNNER_TEMP/canary.log"; then
  echo 'exerciser linked with a reachable panic; the VER-010 check is broken' >&2
  exit 1
fi
grep -q aldis_panic_is_unreachable "$RUNNER_TEMP/canary.log"
echo 'canary failed to link, as it should'
# Every vector case under QEMU leon3_generic: one retry, each run time-limited.
timeout 120 cargo +"$nightly" run "${std[@]}" --release --locked --target $t \
  || timeout 120 cargo +"$nightly" run "${std[@]}" --release --locked --target $t
"#;

fn cpp() -> Vec<Check> {
    let c = Host::Container;
    let mut out = vec![
        check("cpp-toolchains", c, true, 2, CPP_TOOLCHAINS),
        check("cpp-format", c, true, 0, CPP_FORMAT),
    ];
    for (compiler, cxx, secs) in [("gcc", "g++-14", 32), ("clang", "clang++", 24)] {
        out.push(check(
            format!("cpp-build-{compiler}"),
            c,
            true,
            secs,
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
        check("cpp-tidy", c, true, 90, CPP_TIDY),
        check("cpp-sanitize", c, true, 26, CPP_SANITIZE),
        check("cpp-big-endian", c, true, 39, CPP_BIG_ENDIAN),
    ]);
    for target in ["cortex-m4f", "cortex-r5f", "rv32imac"] {
        for compiler in ["gcc", "clang"] {
            out.push(check(
                format!("cpp-bare-metal-{target}-{compiler}"),
                c,
                true,
                10,
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
        ..check("cpp-leon3", Host::ContainerX64, true, 1, CPP_LEON3)
    });
    out.push(check("cpp-layers", c, true, 0, CPP_LAYERS));
    out
}

const FMT: &str = "\
cargo fmt --all --check
# The exerciser, fuzz targets, and cross-checks are separate workspaces.
cargo fmt --all --check --manifest-path exerciser/Cargo.toml
cargo fmt --all --check --manifest-path fuzz/Cargo.toml
cargo fmt --all --check --manifest-path crosscheck/Cargo.toml
";

// The workspace builds of the checks pass `--locked`, so that a commit whose `Cargo.lock` is
// stale fails instead of being resolved again in the checkout; `verify inside` refuses a checkout
// that building xtask changed before any check ran (`inside::unchanged`), which is how a stale
// root `Cargo.lock` shows there, since `cargo xtask` itself does not pass `--locked`.
const CLIPPY: &str = "\
cargo clippy --locked --workspace --all-features --all-targets
cargo doc --locked --workspace --all-features --no-deps
# Fuzz targets build on stable; only running them needs nightly (RS-002).
cargo clippy --locked --manifest-path fuzz/Cargo.toml --all-targets
cargo clippy --locked --manifest-path crosscheck/Cargo.toml --all-targets
";

const TEST: &str = "cargo test --locked --workspace --all-features\n";

const HEADER: &str = "\
# The committed C header is a fresh generation by the pinned cbindgen (CABI-002), includes only
# <stdint.h>, <stddef.h>, and <stdbool.h>, and wraps its declarations in extern \"C\" (CABI-003).
cargo xtask header
# The static and shared libraries on the host (CABI-001), and the header as C99 and C++11 with
# the pinned GCC and Clang: a C program built against each library runs (CABI-003).
cargo xtask header c-smoke gcc-14 g++-14
cargo xtask header c-smoke clang clang++
";

const BIG_ENDIAN: &str = "\
# s390x under QEMU user mode, linked with the pinned cross GCC (VER-030, GEN-007).
export CARGO_TARGET_S390X_UNKNOWN_LINUX_GNU_LINKER=s390x-linux-gnu-gcc-14
export CARGO_TARGET_S390X_UNKNOWN_LINUX_GNU_RUNNER='qemu-s390x -L /usr/s390x-linux-gnu'
cargo test --locked --workspace --all-features --target s390x-unknown-linux-gnu
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
# The decision log is well formed (VER-062).
cargo xtask decisions
cargo xtask vectors validate
cargo xtask vectors schema --check
# The field catalogue of docs/03-api-model.md §9.1 and the files generated from it (API-082).
cargo xtask fields
# The SANA registry snapshots of specs/sana/ and the tables generated from them (M2 plan D13).
cargo xtask sana
# The per-crate registrations (coverage, features, mutants, exerciser, layers, fuzz matrix,
# vector index) agree (docs/milestones/checklists.md).
cargo xtask consistency
# No bare integer in the public API of a codec crate unless exempted with a reason (RS-020).
cargo xtask rs020
# Every restricted type of a codec crate has a property test naming it (RS-020, VER-023).
cargo xtask restricted
# Every loop of the library code is in its crate's GEN-022/023 inspection record with a bound,
# and every loop a record names exists (GEN-022).
cargo xtask loops
# Every reference of the hazard analysis and the assurance case resolves (VER-065, VER-066).
cargo xtask assurance --strict
# Independent cross-check of the vectors (VER-040); fails on any disagreement. Its own tests
# first: the harness must report a difference it is shown.
cargo test --manifest-path crosscheck/Cargo.toml
cargo run --manifest-path crosscheck/Cargo.toml
cargo xtask changelog check
# M0 is closed: every in-scope requirement must keep an artefact (VER-020).
cargo xtask trace --scope m0 --strict
# M1 is closed: every in-scope requirement must keep an artefact (VER-020, m1 plan D8).
cargo xtask trace --scope m1 --strict
# M2 is closed: every in-scope requirement must keep an artefact (VER-020, m2 plan D1).
cargo xtask trace --scope m2 --strict
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
# The tests include the generated vector cases (c0 plan D5): generate them first.
cmake --build --preset host-clang --target aldis_vectors_generate
# Findings are errors (WarningsAsErrors in cpp/.clang-tidy); GoogleTest is not analysed.
run-clang-tidy -quiet -p ../target/cpp/host-clang '/header_check/|/cpp/tests/|/cpp/vectors/'
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
    fn the_run_order_puts_the_quickest_gating_checks_first() {
        let table = all();
        let order = run_order();
        // Every check once.
        let mut a: Vec<&str> = table.iter().map(|c| c.name.as_str()).collect();
        let mut b: Vec<&str> = order.iter().map(|c| c.name.as_str()).collect();
        a.sort_unstable();
        b.sort_unstable();
        assert_eq!(a, b);
        // The gating checks, then the non-gating ones, each by typical duration, ties in table
        // order.
        let gating = order.iter().take_while(|c| c.gating).count();
        assert!(order[gating..].iter().all(|c| !c.gating));
        assert_eq!(gating, table.iter().filter(|c| c.gating).count());
        let index = |name: &str| table.iter().position(|c| c.name == name).unwrap();
        for part in [&order[..gating], &order[gating..]] {
            for w in part.windows(2) {
                assert!(
                    (w[0].secs, index(&w[0].name)) < (w[1].secs, index(&w[1].name)),
                    "{} before {}",
                    w[0].name,
                    w[1].name
                );
            }
        }
        // The checks that fail in seconds come before the builds; kani is last.
        let at = |name: &str| order.iter().position(|c| c.name == name).unwrap();
        for quick in ["fmt", "layers", "deny", "header", "cpp-format"] {
            for slow in ["clippy", "test", "spec", "coverage", "cpp-tidy"] {
                assert!(at(quick) < at(slow), "{quick} before {slow}");
            }
        }
        assert_eq!(order.last().map(|c| c.name.as_str()), Some("kani"));
    }

    // req: VER-056, VER-059
    #[test]
    fn only_kani_is_nightly_only_and_it_does_not_gate() {
        let nightly: Vec<Check> = all().into_iter().filter(|c| c.nightly).collect();
        assert_eq!(nightly.len(), 1);
        assert_eq!(nightly[0].name, "kani");
        assert!(!nightly[0].gating);
        assert_eq!(nightly[0].host, Host::Container);
    }

    // req: VER-056
    #[test]
    fn only_the_leon3_and_kani_checks_are_non_gating() {
        let non_gating: Vec<String> = all()
            .into_iter()
            .filter(|c| !c.gating)
            .map(|c| c.name)
            .collect();
        assert_eq!(non_gating, ["rust-sparc-leon3", "kani", "cpp-leon3"]);
    }

    // req: VER-010, VER-056
    #[test]
    fn the_rust_leon3_check_builds_links_and_runs_on_the_tooling_nightly() {
        let check = all()
            .into_iter()
            .find(|c| c.name == "rust-sparc-leon3")
            .unwrap();
        assert_eq!(check.host, Host::Container);
        assert!(!check.cpp);
        assert_eq!(check.inputs, Inputs::Paths(&[&RUN, &RUST]));
        for path in [
            "exerciser/src/target.rs",
            "exerciser/link-leon3.x",
            "exerciser/.cargo/config.toml",
            "crates/aldis-core/src/lib.rs",
            "test-vectors/spp/encode.json",
            "tooling-nightly",
            "xtask/verify/Containerfile",
        ] {
            assert!(check.inputs.contains(path), "{path}");
        }
        assert!(!check.inputs.contains("cpp/src/x.cpp"));
        let s = &check.script;
        assert!(s.contains(r#"nightly="$(cat tooling-nightly)""#));
        assert!(s.contains("std=(-Z build-std=core)"));
        assert!(s.contains("-C target-cpu=leon3"));
        assert!(s.contains("--no-default-features --features full --target $t"));
        assert!(s.contains("--features inject-panic"));
        assert!(s.contains("grep -q aldis_panic_is_unreachable"));
        assert!(s.contains(r#"timeout 120 cargo +"$nightly" run "${std[@]}" --release"#));
        // xtask, a host build, runs without the LEON3 CPU flag.
        assert!(s.contains(r#"RUSTFLAGS="$host_flags" cargo xtask ffi-imports $t"#));
        // Every other cargo command of the check builds core from source on the nightly.
        for line in s.lines().filter(|l| l.contains("cargo +")) {
            assert!(line.contains(r#"cargo +"$nightly" "#), "{line}");
            assert!(line.contains(r#""${std[@]}""#), "{line}");
        }
    }

    // req: VER-056
    #[test]
    fn inputs_of_every_check() {
        for c in all() {
            let lists: &[&PathList] = match c.inputs {
                Inputs::Everything => {
                    assert_eq!(c.name, "spec");
                    continue;
                }
                Inputs::Paths(lists) => lists,
            };
            assert!(lists.contains(&&RUN), "{}", c.name);
            for only in SPEC_ONLY {
                assert!(!lists.contains(&only), "{} {}", c.name, only.name);
            }
            if c.name == "header" {
                assert_eq!(lists, [&RUN, &FFI]);
                continue;
            }
            if c.name == "kani" {
                assert_eq!(lists, [&RUN, &KANI]);
                continue;
            }
            if c.name == "stack" {
                assert_eq!(lists, [&RUN, &STACK]);
                continue;
            }
            assert_eq!(lists.contains(&&CPP), c.cpp, "{}", c.name);
            assert_eq!(lists.contains(&&RUST), !c.cpp, "{}", c.name);
            let tests = matches!(c.name.as_str(), "test" | "test-macos" | "big-endian");
            assert_eq!(lists.contains(&&XTASK_TESTS), tests, "{}", c.name);
        }
    }

    // req: VER-056
    #[test]
    fn every_check_reads_the_image_inputs() {
        for path in super::super::image::INPUTS {
            assert!(RUN.contains(path), "{path}");
        }
    }

    // req: VER-056
    #[test]
    fn documentation_is_an_input_of_spec_only() {
        let docs = [
            "docs/06-verification.md",
            "docs/milestones/m2-plan.md",
            "changelog.d/chore-x.md",
            "README.md",
            "AGENTS.md",
            "CHANGELOG.md",
            "specs/README.md",
            "specs/manifest.tsv",
        ];
        for path in docs {
            assert!(DOCS.contains(path), "{path}");
            for c in all() {
                assert_eq!(
                    c.inputs.contains(path),
                    c.name == "spec",
                    "{path}: {}",
                    c.name
                );
            }
        }
        let not_docs = [
            "crates/aldis-spp/README.md",
            "cpp/README.md",
            "refapp/README.md",
            "refapp/golden/CHANGES.md",
            "test-vectors/README.md",
            "exerciser/README.md",
            "xtask/README.md",
            "specs/fetch.sh",
            ".github/pull_request_template.md",
            "docs.md/x",
        ];
        for path in not_docs {
            assert!(!DOCS.contains(path), "{path}");
        }
    }

    /// `path` with `.` and `..` resolved lexically, relative to the repository root.
    fn normalize(path: &std::path::Path) -> String {
        let mut out = std::path::PathBuf::new();
        for part in path.components() {
            match part {
                std::path::Component::ParentDir => {
                    out.pop();
                }
                std::path::Component::CurDir => {}
                other => out.push(other),
            }
        }
        crate::repo::rel(&out)
    }

    // req: VER-056
    #[test]
    fn every_included_file_is_an_input_of_the_rust_checks() {
        // A file that Rust code includes is read by every check that builds it.
        let rust = Inputs::Paths(&[&RUN, &RUST]);
        let macros = [
            concat!("include_", "str!(\""),
            concat!("include_", "bytes!(\""),
        ];
        let mut seen = 0;
        for file in crate::repo::files(".", &["rs"]) {
            if DOCS.contains(&crate::repo::rel(&file)) {
                continue;
            }
            let text = std::fs::read_to_string(&file).unwrap_or_default();
            for mac in macros {
                for (at, _) in text.match_indices(mac) {
                    let tail = &text[at + mac.len()..];
                    let Some(end) = tail.find('"') else { continue };
                    let dir = file.parent().unwrap_or(&file);
                    let target = normalize(&dir.join(&tail[..end]));
                    assert!(rust.contains(&target), "{}: {target}", file.display());
                    seen += 1;
                }
            }
        }
        assert!(seen > 0, "no include_str! found");
    }

    // req: CABI-002, VER-056
    #[test]
    fn the_header_check_reads_the_ffi_crates_and_the_header() {
        let header = all().into_iter().find(|c| c.name == "header").unwrap();
        for path in [
            "crates/aldis-ffi/src/spp.rs",
            "crates/aldis-ffi/cbindgen.toml",
            "crates/aldis-ffi/capacities.toml",
            "crates/aldis-spp/src/view.rs",
            "crates/aldis-tm/src/lib.rs",
            "include/aldis/aldis.h",
            "xtask/src/header.rs",
            "xtask/verify/Containerfile",
        ] {
            assert!(header.inputs.contains(path), "{path}");
        }
        assert!(!header.inputs.contains("test-vectors/spp/encode.json"));
        assert!(!header.inputs.contains("cpp/src/x.cpp"));
        assert!(header.script.contains("cargo xtask header\n"));
        assert!(header.script.contains("c-smoke gcc-14 g++-14"));
        assert!(header.script.contains("c-smoke clang clang++"));
    }

    // req: VER-011, VER-056
    #[test]
    fn the_stack_check_reads_the_library_its_budgets_and_the_nightly() {
        let stack = all().into_iter().find(|c| c.name == "stack").unwrap();
        assert!(stack.gating && !stack.nightly);
        for path in [
            "crates/aldis-core/src/crc.rs",
            "crates/aldis-ffi/src/tc.rs",
            "crates/aldis-ffi/capacities.toml",
            "xtask/stack-budgets.toml",
            "xtask/src/stack.rs",
            "tooling-nightly",
            "Cargo.lock",
        ] {
            assert!(stack.inputs.contains(path), "{path}");
        }
        for path in [
            "exerciser/src/main.rs",
            "exerciser/build.rs",
            "test-vectors/spp/encode.json",
        ] {
            assert!(stack.inputs.contains(path), "{path}");
        }
        for path in [
            "cpp/src/x.cpp",
            "docs/06-verification.md",
            "include/aldis/aldis.h",
        ] {
            assert!(!stack.inputs.contains(path), "{path}");
        }
        assert_eq!(stack.script, "cargo xtask stack\n");
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

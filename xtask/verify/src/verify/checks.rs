//! The checks of the verification run (VER-056): one per job of the former pull-request
//! workflows, with the same names (`.github/workflows/ci.yml`, `cpp.yml`), so that
//! `docs/verification/analysis-records.md` can name them.
//!
//! Each check is a Bash script run with `-euo pipefail` from the root of the checkout, with
//! `RUNNER_TEMP` set to an empty directory of its own.

use std::collections::BTreeSet;

use xtask_lib::Tool;

use super::graph::{Graph, XTASK_DIR};

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
// inputs and is not in `DOCS` runs every check. Precision never at the cost of soundness: when a
// check starts reading a new path, add the path here in the same change; when unsure, include it.
//
// The xtask code a check runs is not listed here: it is derived from the crate graph
// ([`derived`], docs/06-verification.md §7). Every check runs the harness (`xtask-verify`, with
// the crates it depends on) and the dispatcher (`xtask`); a check whose script runs
// `cargo xtask <command>` also runs the crate whose `Tool` claims the command, with the crates it
// depends on; and the checks that build, lint, or test the whole workspace ([`Uses::Workspace`])
// read every xtask crate.
//
// The differential tests of track C (`difftest/`, VER-053, milestone C1) will read both the Rust
// crates and `cpp/`: their checks take the lists `&[&RUN, &RUST, &CPP, &DIFFTEST]`, with a
// `DIFFTEST` list naming `difftest/**` and whatever else they read.

/// Read by every check: the verification run itself, apart from the xtask crates, which are
/// derived (see above). Building xtask reads every workspace manifest and `.cargo/`, and the
/// manifests decide the features of the dependencies the xtask crates share; the image is built
/// from the files of `image::INPUTS`.
pub const RUN: PathList = PathList {
    name: "run",
    patterns: &[
        "Cargo.toml",
        "Cargo.lock",
        ".cargo/**",
        "crates/*/Cargo.toml",
        "xtask/*/Cargo.toml",
        "xtask/verify/Containerfile",
        "xtask/verify/entrypoint.sh",
        "rust-toolchain.toml",
        "tooling-nightly",
        "cpp/toolchains.lock",
        ".github/actions/setup-cpp/setup_cpp.py",
    ],
};

/// Read by every Rust check: the crates and the separate workspaces (`exerciser/`, `fuzz/`,
/// `refapp/`, `crosscheck/`), their lint, format, licence, and coverage settings, the vectors that
/// their tests and the exerciser load, and the Rust setup of the workflows. One list for all of
/// them: a finer split would save little and risk missing a path.
pub const RUST: PathList = PathList {
    name: "rust",
    patterns: &[
        "crates/**",
        "exerciser/**",
        "fuzz/**",
        "refapp/**",
        "crosscheck/**",
        "test-vectors/**",
        "clippy.toml",
        "rustfmt.toml",
        ".rustfmt.toml",
        "deny.toml",
        "coverage-waivers.toml",
        "LICENSE",
        ".github/actions/setup-rust/**",
        ".github/workflows/**",
    ],
};

/// Also read by the checks that run the xtask unit tests (`test`, `test-macos`, `big-endian`):
/// `spec::tests::cpp_sources_are_checked_for_requirement_ids` lists the C++ files (CPP-057).
pub const XTASK_TESTS: PathList = PathList {
    name: "xtask-tests",
    patterns: &["cpp/**"],
};

/// Read by the C++ checks: the C++ tree, the vectors (read by the C++ tests from C1), and the
/// C++ setup action and workflow (formerly the `changes` job of `cpp.yml`).
pub const CPP: PathList = PathList {
    name: "cpp",
    patterns: &[
        "cpp/**",
        "test-vectors/**",
        ".github/actions/setup-cpp/**",
        ".github/workflows/cpp.yml",
    ],
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

/// What a check reads (VER-056).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Inputs {
    /// Every path of the repository: the check runs on every change.
    Everything,
    /// The paths that match a pattern of one of these lists, or of the xtask crates it runs.
    Paths(&'static [&'static PathList], Crates),
}

/// The xtask crates a check runs (VER-056): their directories, derived from the crate graph, or
/// every xtask path when they are not derived.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Crates {
    /// Shown by `cargo xtask verify list`: `xtask/**` when not derived, `xtask` for every xtask
    /// crate, else the tool crates the check's commands name; empty for the harness alone.
    pub label: String,
    /// The patterns, relative to the repository root.
    pub patterns: Vec<String>,
}

impl Crates {
    /// Every xtask path, and `aldis-testkit`, which xtask builds: the inputs when the crate graph
    /// is not known.
    pub fn every() -> Self {
        Self {
            label: "xtask/**".to_owned(),
            patterns: vec!["xtask/**".to_owned(), "crates/aldis-testkit/**".to_owned()],
        }
    }
}

impl Inputs {
    /// Whether `path` is one of them.
    pub fn contains(&self, path: &str) -> bool {
        match self {
            Self::Everything => true,
            Self::Paths(lists, crates) => {
                lists.iter().any(|l| l.contains(path))
                    || crates
                        .patterns
                        .iter()
                        .any(|p| super::scope::matches(p, path))
            }
        }
    }

    /// Every pattern, as the record lists them.
    pub fn patterns(&self) -> Vec<String> {
        match self {
            Self::Everything => vec!["**".to_owned()],
            Self::Paths(lists, crates) => lists
                .iter()
                .flat_map(|l| l.patterns.iter().map(|p| (*p).to_owned()))
                .chain(crates.patterns.iter().cloned())
                .collect(),
        }
    }

    /// The names of the lists and of the xtask crates, for `cargo xtask verify list`.
    pub fn label(&self) -> String {
        match self {
            Self::Everything => "every path".to_owned(),
            Self::Paths(lists, crates) => lists
                .iter()
                .map(|l| l.name)
                .chain(Some(crates.label.as_str()).filter(|l| !l.is_empty()))
                .collect::<Vec<_>>()
                .join("+"),
        }
    }
}

impl PathList {
    /// Whether `path` matches one of the patterns.
    pub fn contains(&self, path: &str) -> bool {
        self.patterns.iter().any(|p| super::scope::matches(p, path))
    }
}

const RUST_INPUTS: &[&PathList] = &[&RUN, &RUST];
const RUST_TEST_INPUTS: &[&PathList] = &[&RUN, &RUST, &XTASK_TESTS];
const CPP_INPUTS: &[&PathList] = &[&RUN, &CPP];

/// Which xtask crates a check runs besides the harness and the dispatcher (VER-056).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Uses {
    /// The crates of the `cargo xtask <command>` lines of its script.
    Commands,
    /// Every xtask crate: the check builds, lints, formats, tests, or audits the whole workspace.
    Workspace,
}

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
    /// Which xtask crates it runs, from which [`derived`] finds the xtask part of its inputs.
    pub uses: Uses,
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
        inputs: Inputs::Paths(if cpp { CPP_INPUTS } else { RUST_INPUTS }, Crates::every()),
        uses: Uses::Commands,
        gating: true,
        script: script.into(),
    }
}

/// `check` with other lists than its kind's.
fn reading(lists: &'static [&'static PathList], check: Check) -> Check {
    Check {
        inputs: Inputs::Paths(lists, Crates::every()),
        ..check
    }
}

/// `check`, reading every path.
fn everything(check: Check) -> Check {
    Check {
        inputs: Inputs::Everything,
        ..check
    }
}

/// `check`, building, linting, formatting, testing, or auditing the whole workspace.
fn whole(check: Check) -> Check {
    Check {
        uses: Uses::Workspace,
        ..check
    }
}

/// The commands that the harness itself runs (`verify`) or that print the help text; every check
/// already runs the harness and the dispatcher, so no tool may claim them.
const RESERVED: &[&str] = &["verify", "help", "-h", "--help"];
/// The package of the harness.
const HARNESS: &str = "xtask-verify";
/// The package of the dispatcher, the `xtask` binary. Its own directory is an input of every
/// check; the crates it depends on are inputs only of the checks that run them.
const DISPATCHER: &str = "xtask";

/// The first word after each `cargo xtask` in `script`; `None` when `xtask` also appears in
/// another form, so that the script may run xtask code that cannot be told.
fn commands(script: &str) -> Option<Vec<&str>> {
    let words: Vec<&str> = script
        .split(|c: char| c.is_whitespace() || "();&|`\"'".contains(c))
        .filter(|w| !w.is_empty())
        .collect();
    let mut out = Vec::new();
    let mut seen = 0;
    for (i, word) in words.iter().enumerate() {
        if !word.contains("xtask") {
            continue;
        }
        seen += 1;
        let after_cargo = i.checked_sub(1).and_then(|j| words.get(j)) == Some(&"cargo");
        if *word != "xtask" || !after_cargo {
            return None;
        }
        out.push(*words.get(i + 1)?);
    }
    (script.matches("xtask").count() == seen).then_some(out)
}

/// The derivation of the xtask inputs from the crate graph and the dispatcher's tools.
struct Derivation<'a> {
    graph: &'a Graph,
    tools: &'a [Tool],
    /// The packages every check runs: the harness with its dependencies, and the dispatcher.
    base: BTreeSet<String>,
}

impl<'a> Derivation<'a> {
    fn new(graph: &'a Graph, tools: &'a [Tool]) -> Result<Self, String> {
        let mut claimed: Vec<&str> = RESERVED.to_vec();
        for tool in tools {
            let dir = graph
                .packages
                .get(tool.package)
                .map(|p| p.dir.as_str())
                .ok_or_else(|| format!("no workspace package {}", tool.package))?;
            if !dir.starts_with(XTASK_DIR) {
                return Err(format!(
                    "the tool crate {} is not below xtask/",
                    tool.package
                ));
            }
            for command in tool.commands {
                if claimed.contains(command) {
                    return Err(format!(
                        "the command `{command}` of {} is claimed twice",
                        tool.package
                    ));
                }
                claimed.push(command);
            }
        }
        if !graph.packages.contains_key(DISPATCHER) {
            return Err(format!("no workspace package {DISPATCHER}"));
        }
        let mut base = graph.closure(HARNESS)?;
        base.insert(DISPATCHER.to_owned());
        Ok(Self { graph, tools, base })
    }

    /// The xtask crates `check` runs, with their dependencies; every xtask path when its script
    /// runs a command no tool claims, or xtask in another form.
    fn crates(&self, check: &Check) -> Result<Crates, String> {
        let mut packages = self.base.clone();
        let label = match check.uses {
            Uses::Workspace => {
                for name in self.graph.xtask() {
                    packages.extend(self.graph.closure(name)?);
                }
                "xtask".to_owned()
            }
            Uses::Commands => {
                let Some(commands) = commands(&check.script) else {
                    return Ok(Crates::every());
                };
                let mut named = Vec::new();
                for command in commands {
                    if command == "verify" {
                        continue;
                    }
                    let Some(tool) = self.tools.iter().find(|t| t.commands.contains(&command))
                    else {
                        return Ok(Crates::every());
                    };
                    packages.extend(self.graph.closure(tool.package)?);
                    if !named.contains(&tool.package) {
                        named.push(tool.package);
                    }
                }
                named.join("+")
            }
        };
        let mut patterns: Vec<String> = packages
            .iter()
            .filter_map(|p| self.graph.packages.get(p))
            .map(|p| format!("{}/**", p.dir))
            .collect();
        patterns.sort();
        patterns.dedup();
        Ok(Crates { label, patterns })
    }
}

/// Every check with the xtask part of its inputs derived from `graph` and `tools`, the commands
/// the dispatcher runs (VER-056).
///
/// # Errors
///
/// When the tools do not match the graph or a command is claimed twice; [`all`] then holds.
pub fn derive(graph: &Graph, tools: &[Tool]) -> Result<Vec<Check>, String> {
    let d = Derivation::new(graph, tools)?;
    all()
        .into_iter()
        .map(|c| match &c.inputs {
            Inputs::Everything => Ok(c),
            Inputs::Paths(lists, _) => Ok(Check {
                inputs: Inputs::Paths(lists, d.crates(&c)?),
                ..c
            }),
        })
        .collect()
}

/// [`derive`] from the crate graph of the repository (`graph::Graph::load`).
///
/// # Errors
///
/// When the graph cannot be read or is refused, or [`derive`] fails.
pub fn derived(tools: &[Tool]) -> Result<Vec<Check>, String> {
    derive(&Graph::load(&crate::repo::root())?, tools)
}

/// Every check, in the order they run, with every xtask path among the inputs of each: the table
/// when the xtask inputs are not derived ([`derived`]).
pub fn all() -> Vec<Check> {
    let mut out = rust();
    out.extend(cpp());
    out
}

fn rust() -> Vec<Check> {
    let c = Host::Container;
    let mut out = vec![
        whole(check("fmt", c, false, FMT)),
        whole(check("clippy", c, false, CLIPPY)),
        whole(reading(RUST_TEST_INPUTS, check("test", c, false, TEST))),
        whole(reading(
            RUST_TEST_INPUTS,
            check("test-macos", Host::MacOs, false, TEST),
        )),
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
        whole(reading(
            RUST_TEST_INPUTS,
            check("big-endian", c, false, BIG_ENDIAN),
        )),
        check("layers", c, false, LAYERS),
        check("refapp", c, false, REFAPP),
    ]);
    for t in RUST_BARE_METAL {
        out.push(check(format!("exerciser-{t}"), c, false, exerciser(t)));
    }
    out.extend([
        check("coverage", c, false, "cargo xtask coverage\n"),
        // Spec lint, links, vectors, traces, and changelog: it reads every part of the tree.
        everything(check("spec", c, false, SPEC)),
        // cargo-deny reads every crate of the workspace.
        whole(check("deny", c, false, DENY)),
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
# The field catalogue of docs/03-api-model.md §9.1 and the files generated from it (API-082).
cargo xtask fields
# Independent cross-check of the vectors (VER-040); fails on any disagreement. Its own tests
# first: the harness must report a difference it is shown.
cargo test --manifest-path crosscheck/Cargo.toml
cargo run --manifest-path crosscheck/Cargo.toml
cargo xtask changelog check
# M0 is closed: every in-scope requirement must keep an artefact (VER-020).
cargo xtask trace --scope m0 --strict
# M1 is closed: every in-scope requirement must keep an artefact (VER-020, m1 plan D8).
cargo xtask trace --scope m1 --strict
# M2 is open: its trace is reported, and becomes --strict at the M2 exit (m2 plan D1).
cargo xtask trace --scope m2
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
    fn only_the_leon3_build_is_non_gating() {
        let non_gating: Vec<String> = all()
            .into_iter()
            .filter(|c| !c.gating)
            .map(|c| c.name)
            .collect();
        assert_eq!(non_gating, ["cpp-leon3"]);
    }

    // req: VER-056
    #[test]
    fn inputs_of_every_check() {
        for c in all() {
            let lists: &[&PathList] = match &c.inputs {
                Inputs::Everything => {
                    assert_eq!(c.name, "spec");
                    continue;
                }
                Inputs::Paths(lists, crates) => {
                    assert_eq!(*crates, Crates::every(), "{}", c.name);
                    lists
                }
            };
            assert!(lists.contains(&&RUN), "{}", c.name);
            assert!(!lists.contains(&&DOCS), "{}", c.name);
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
        // An xtask crate includes no file at all (`graph::Graph::load`).
        let rust = Inputs::Paths(&[&RUN, &RUST], Crates::every());
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

    // req: VER-056
    #[test]
    fn the_xtask_commands_of_a_script() {
        assert_eq!(
            commands(LAYERS),
            Some(vec!["layers", "symbols", "refapp-deps"])
        );
        assert_eq!(commands(CPP_LAYERS), Some(vec!["cpp", "cpp"]));
        assert_eq!(commands(FMT), Some(vec![]));
        assert_eq!(
            commands("x=$(cargo xtask features)\n"),
            Some(vec!["features"])
        );
        // xtask in any other form: not known.
        for script in [
            "cargo run -p xtask -- features\n",
            "cargo run -p xtask-spec\n",
            "cd xtask && cargo test\n",
            "cargo xtask\n",
            "cargo +nightly xtask coverage\n",
        ] {
            assert_eq!(commands(script), None, "{script}");
        }
    }

    // req: VER-056
    #[test]
    fn the_checks_of_the_whole_workspace() {
        let whole: Vec<String> = all()
            .into_iter()
            .filter(|c| c.uses == Uses::Workspace)
            .map(|c| c.name)
            .collect();
        assert_eq!(
            whole,
            ["fmt", "clippy", "test", "test-macos", "big-endian", "deny"]
        );
        // A script that runs cargo on the root workspace other than through `cargo xtask` and
        // without naming its packages builds every xtask crate.
        for c in all() {
            if c.script.lines().any(|l| l.trim_start().starts_with("cd ")) {
                continue;
            }
            for line in c.script.lines().map(str::trim) {
                let words: Vec<&str> = line.split_whitespace().collect();
                if words.first() != Some(&"cargo") || words.get(1) == Some(&"xtask") {
                    continue;
                }
                let named = words
                    .iter()
                    .any(|w| matches!(*w, "-p" | "--package" | "--manifest-path"));
                assert!(
                    named || c.uses == Uses::Workspace || c.inputs == Inputs::Everything,
                    "{}: {line}",
                    c.name
                );
                assert!(
                    !words.iter().any(|w| w.starts_with("xtask")),
                    "{}: {line}",
                    c.name
                );
            }
        }
    }

    fn crates_of(checks: &[Check], name: &str) -> Crates {
        match checks.iter().find(|c| c.name == name).map(|c| &c.inputs) {
            Some(Inputs::Paths(_, crates)) => crates.clone(),
            other => panic!("{name}: {other:?}"),
        }
    }

    // req: VER-056
    #[test]
    fn the_xtask_inputs_derived_from_the_crate_graph() {
        let checks = derive(&fixture::graph(), fixture::TOOLS).unwrap();
        let base = ["xtask/cli/**", "xtask/lib/**", "xtask/verify/**"];
        let with = |extra: &[&str]| -> Vec<String> {
            let mut v: Vec<String> = base.iter().chain(extra).map(|p| (*p).to_owned()).collect();
            v.sort();
            v
        };
        // A check that runs no xtask command: the harness and the dispatcher only.
        for name in ["no-std-thumbv7em-none-eabihf", "refapp", "cpp-tidy"] {
            assert_eq!(
                crates_of(&checks, name),
                Crates {
                    label: String::new(),
                    patterns: with(&[])
                }
            );
        }
        assert_eq!(
            crates_of(&checks, "features").patterns,
            with(&["xtask/features/**"])
        );
        let layers = crates_of(&checks, "layers");
        assert_eq!(layers.label, "xtask-layers+xtask-binaries");
        assert_eq!(
            layers.patterns,
            with(&["xtask/binaries/**", "xtask/layers/**"])
        );
        // xtask-cpp depends on xtask-layers.
        assert_eq!(
            crates_of(&checks, "cpp-layers").patterns,
            with(&["xtask/cpp/**", "xtask/layers/**"])
        );
        // The whole workspace: every xtask crate and what they depend on.
        let fmt = crates_of(&checks, "fmt");
        assert_eq!(fmt.label, "xtask");
        assert!(fmt.patterns.contains(&"xtask/ci/**".to_owned()));
        assert!(fmt.patterns.contains(&"crates/aldis-testkit/**".to_owned()));
        // spec reads every path anyway.
        let spec = checks.iter().find(|c| c.name == "spec").unwrap();
        assert_eq!(spec.inputs, Inputs::Everything);
        // The label shown by `verify list`.
        let features = checks.iter().find(|c| c.name == "features").unwrap();
        assert_eq!(features.inputs.label(), "run+rust+xtask-features");
    }

    // req: VER-056
    #[test]
    fn the_xtask_inputs_are_every_xtask_path_when_not_derivable() {
        let graph = fixture::graph();
        // A command no tool claims: that check reads every xtask path.
        let without_coverage: Vec<Tool> = fixture::TOOLS
            .iter()
            .filter(|t| t.package != "xtask-coverage")
            .copied()
            .collect();
        let checks = derive(&graph, &without_coverage).unwrap();
        assert_eq!(crates_of(&checks, "coverage"), Crates::every());
        assert_ne!(crates_of(&checks, "features"), Crates::every());
        // A command claimed twice, or a reserved one: nothing is derived.
        let mut twice = fixture::TOOLS.to_vec();
        twice.push(fixture::tool("xtask-ci", &["features"]));
        assert!(
            derive(&graph, &twice)
                .unwrap_err()
                .contains("claimed twice")
        );
        let mut reserved = fixture::TOOLS.to_vec();
        reserved.push(fixture::tool("xtask-ci", &["verify"]));
        assert!(derive(&graph, &reserved).is_err());
        // A tool that is not an xtask crate, or not in the graph.
        for package in ["aldis-testkit", "xtask-nowhere"] {
            let mut odd = fixture::TOOLS.to_vec();
            odd.push(fixture::tool(package, &["odd"]));
            assert!(derive(&graph, &odd).is_err(), "{package}");
        }
        // No harness in the graph.
        let mut no_harness = graph;
        no_harness.packages.remove("xtask-verify");
        assert!(derive(&no_harness, fixture::TOOLS).is_err());
    }

    // req: VER-056
    #[test]
    fn the_repository_crate_graph_is_accepted() {
        // The real graph: no build script, procedural macro, or included file in an xtask
        // crate, and every xtask manifest forbids unsafe code.
        let graph = Graph::load(&crate::repo::root()).unwrap();
        let xtask: Vec<&String> = graph.xtask().collect();
        assert!(xtask.iter().any(|n| *n == "xtask"), "{xtask:?}");
        assert!(xtask.iter().any(|n| *n == "xtask-verify"), "{xtask:?}");
        // Every tracked path below xtask/ is in the directory of an xtask crate, so that the
        // checks of the whole workspace read it and no path there is an input of no check.
        let files = std::process::Command::new("git")
            .args(["ls-files", "-z", "--", "xtask"])
            .current_dir(crate::repo::root())
            .output()
            .unwrap();
        let dirs: Vec<String> = xtask
            .iter()
            .map(|n| format!("{}/", graph.packages[*n].dir))
            .collect();
        for file in String::from_utf8(files.stdout).unwrap().split('\0') {
            if !file.is_empty() {
                assert!(dirs.iter().any(|d| file.starts_with(d)), "{file}");
            }
        }
    }
}

/// A workspace and the tools of its dispatcher like those of the repository, for tests.
#[cfg(test)]
pub(super) mod fixture {
    use xtask_lib::Tool;

    use super::super::graph::{Graph, Package};

    const fn none(_: &[&str]) -> Option<Result<usize, String>> {
        None
    }

    /// A tool of `package` claiming `commands`.
    pub(in super::super) const fn tool(
        package: &'static str,
        commands: &'static [&'static str],
    ) -> Tool {
        Tool {
            package,
            commands,
            help: &[],
            run: none,
        }
    }

    /// The tools.
    pub(in super::super) const TOOLS: &[Tool] = &[
        tool(
            "xtask-spec",
            &["spec-lint", "trace", "vectors", "fields", "links"],
        ),
        tool("xtask-layers", &["layers", "refapp-deps"]),
        tool("xtask-binaries", &["symbols", "stack-sizes"]),
        tool("xtask-features", &["features"]),
        tool("xtask-coverage", &["coverage"]),
        tool("xtask-changelog", &["changelog"]),
        tool("xtask-ci", &["ci"]),
        tool("xtask-cpp", &["cpp"]),
    ];

    /// The packages: name, directory, and the workspace packages it depends on.
    pub(in super::super) const PACKAGES: &[(&str, &str, &[&str])] = &[
        (
            "xtask",
            "xtask/cli",
            &[
                "xtask-binaries",
                "xtask-changelog",
                "xtask-ci",
                "xtask-coverage",
                "xtask-cpp",
                "xtask-features",
                "xtask-layers",
                "xtask-lib",
                "xtask-spec",
                "xtask-verify",
            ],
        ),
        ("xtask-lib", "xtask/lib", &[]),
        ("xtask-verify", "xtask/verify", &["xtask-lib"]),
        ("xtask-spec", "xtask/spec", &["aldis-testkit", "xtask-lib"]),
        ("xtask-changelog", "xtask/changelog", &["xtask-lib"]),
        ("xtask-layers", "xtask/layers", &["xtask-lib"]),
        ("xtask-binaries", "xtask/binaries", &["xtask-lib"]),
        ("xtask-features", "xtask/features", &["xtask-lib"]),
        ("xtask-coverage", "xtask/coverage", &["xtask-lib"]),
        ("xtask-cpp", "xtask/cpp", &["xtask-layers", "xtask-lib"]),
        (
            "xtask-ci",
            "xtask/ci",
            &[
                "xtask-changelog",
                "xtask-cpp",
                "xtask-layers",
                "xtask-lib",
                "xtask-spec",
            ],
        ),
        ("aldis-testkit", "crates/aldis-testkit", &[]),
    ];

    /// The graph of [`PACKAGES`].
    pub(in super::super) fn graph() -> Graph {
        Graph {
            packages: PACKAGES
                .iter()
                .map(|(name, dir, deps)| {
                    let package = Package {
                        dir: (*dir).to_owned(),
                        deps: deps.iter().map(|d| (*d).to_owned()).collect(),
                    };
                    ((*name).to_owned(), package)
                })
                .collect(),
        }
    }
}

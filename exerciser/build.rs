//! Embeds the core conformance vectors as static data for the bare-metal exerciser and selects
//! the linker script for the target (docs/milestones/m0-foundations.md WP-5).
//!
//! Parsing uses the same `aldis-testkit` model as the host test suites, so the exerciser runs
//! exactly the cases in `test-vectors/core/` (superseded cases excluded).
#![expect(
    clippy::panic,
    clippy::too_many_lines,
    reason = "host build script (tooling, RS-017): malformed vectors must fail the build loudly"
)]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use aldis_testkit::{CodecCase, CodecFile, VectorFile, hex};
use serde_json::Value;

fn load(dir: &Path, name: &str) -> CodecFile {
    let path = dir.join(name);
    println!("cargo:rerun-if-changed={}", path.display());
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    match VectorFile::from_json(&text) {
        Ok(VectorFile::Codec(f)) => f,
        Ok(VectorFile::EngineScenario(_)) => panic!("{name}: expected a codec file"),
        Err(e) => panic!("{name}: {e}"),
    }
}

fn live(file: &CodecFile, op: &str) -> Vec<CodecCase> {
    file.cases
        .iter()
        .filter(|c| c.superseded.is_none() && c.operation == op)
        .cloned()
        .collect()
}

fn bytes(octets: &[u8]) -> String {
    let body: Vec<String> = octets.iter().map(|b| format!("0x{b:02X}")).collect();
    format!("&[{}]", body.join(", "))
}

fn input(c: &CodecCase) -> Vec<u8> {
    hex::decode(c.input_hex.as_deref().unwrap_or("")).unwrap_or_else(|e| panic!("{}: {e}", c.id))
}

fn param(c: &CodecCase, name: &str) -> u64 {
    c.params
        .as_ref()
        .and_then(|p| p.get(name))
        .and_then(Value::as_u64)
        .unwrap_or_else(|| panic!("{}: param {name}", c.id))
}

fn flag(c: &CodecCase, name: &str) -> bool {
    c.params
        .as_ref()
        .and_then(|p| p.get(name))
        .and_then(Value::as_bool)
        .unwrap_or_else(|| panic!("{}: {name}", c.id))
}

fn expect_str<'a>(c: &'a CodecCase, name: &str) -> &'a str {
    c.expect
        .values
        .get(name)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("{}: expect.{name}", c.id))
}

fn expect_u64(c: &CodecCase, name: &str) -> u64 {
    c.expect
        .values
        .get(name)
        .and_then(Value::as_u64)
        .unwrap_or_else(|| panic!("{}: expect.{name}", c.id))
}

/// Writes the `aldis-core` OCF cases and the `aldis-time`, `aldis-spp`, `aldis-epp`, and
/// `aldis-aos`, and `aldis-tc` cases as a generic value tree
/// (`exerciser/src/vectors.rs`), which the per-crate handlers read and compare exactly, as the
/// host suites do (TV-012). Strings under keys ending in `_hex` become octets.
struct Codec {
    out: String,
    out_dir: PathBuf,
    blobs: usize,
}

impl Codec {
    /// Octets up to this length are written inline; longer ones go to a file in `OUT_DIR`.
    const INLINE: usize = 256;

    fn bytes(&mut self, octets: &[u8]) -> String {
        if octets.len() <= Self::INLINE {
            return bytes(octets);
        }
        let name = format!("blob{}.bin", self.blobs);
        self.blobs = self.blobs.saturating_add(1);
        std::fs::write(self.out_dir.join(&name), octets).unwrap_or_else(|e| panic!("{name}: {e}"));
        format!("include_bytes!(concat!(env!(\"OUT_DIR\"), \"/{name}\"))")
    }

    fn value(&mut self, id: &str, key: &str, v: &Value) -> String {
        match v {
            Value::Null => "Val::Null".to_owned(),
            Value::Bool(b) => format!("Val::Bool({b})"),
            Value::Number(n) => {
                let n = n.as_u64().unwrap_or_else(|| panic!("{id}: {key} = {n}"));
                format!("Val::Num({n})")
            }
            Value::String(s) if key.ends_with("_hex") => {
                let octets = hex::decode(s).unwrap_or_else(|e| panic!("{id}: {key}: {e}"));
                format!("Val::Bytes({})", self.bytes(&octets))
            }
            Value::String(s) => format!("Val::Str({s:?})"),
            Value::Array(items) => {
                let items: Vec<String> = items.iter().map(|x| self.value(id, key, x)).collect();
                format!("Val::List(&[{}])", items.join(", "))
            }
            Value::Object(map) => format!("Val::Obj({})", self.map(id, Some(map))),
        }
    }

    fn map(&mut self, id: &str, map: Option<&serde_json::Map<String, Value>>) -> String {
        let fields: Vec<String> = map
            .into_iter()
            .flatten()
            .map(|(k, v)| format!("({k:?}, {})", self.value(id, k, v)))
            .collect();
        format!("&[{}]", fields.join(", "))
    }

    fn case(&mut self, group: &str, c: &CodecCase) {
        let lenient = c.mode == Some(aldis_testkit::Mode::Lenient);
        let input = self.bytes(&input(c));
        let params = self.map(&c.id, c.params.as_ref());
        let config = self.map(&c.id, c.config.as_ref());
        let expect = if c.expect.ok {
            format!("Expect::Ok({})", self.map(&c.id, Some(&c.expect.values)))
        } else {
            let kind = c
                .expect
                .error
                .as_deref()
                .unwrap_or_else(|| panic!("{}: no error kind", c.id));
            // The field code is compared only when the case gives one (M2 plan D3).
            let field = c
                .expect
                .field
                .map_or_else(|| "None".to_owned(), |f| format!("Some({f})"));
            format!("Expect::Err({kind:?}, {field})")
        };
        let _ = writeln!(
            self.out,
            "    Case {{ id: {:?}, group: Group::{group}, op: {:?}, lenient: {lenient}, \
             input: {input}, params: {params}, config: {config}, expect: {expect} }},",
            c.id, c.operation
        );
    }
}

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    let dir = manifest.join("../test-vectors/core");
    let mut out =
        String::from("// Generated by exerciser/build.rs from test-vectors/core. Do not edit.\n");

    let crc = load(&dir, "crc16.json");
    let _ = writeln!(out, "pub static CRC: &[CrcCase] = &[");
    for c in live(&crc, "crc16") {
        let value = u16::from_str_radix(expect_str(&c, "value_hex"), 16)
            .unwrap_or_else(|e| panic!("{}: {e}", c.id));
        let _ = writeln!(
            out,
            "    CrcCase {{ input: {}, value: 0x{value:04X} }},",
            bytes(&input(&c))
        );
    }
    out.push_str("];\n");

    let oid = load(&dir, "oid.json");
    let start = live(&oid, "oid_fill")
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("oid.json: no oid_fill case"));
    let seq = hex::decode(expect_str(&start, "output_hex")).unwrap_or_else(|e| panic!("{e}"));
    let _ = writeln!(out, "pub static OID_START: &[u8] = {};", bytes(&seq));

    let errors = load(&dir, "errors.json");
    out.push_str("pub static ERRORS: &[ErrorCase] = &[\n");
    for c in live(&errors, "error_code") {
        let name = c
            .params
            .as_ref()
            .and_then(|p| p.get("kind"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let _ = writeln!(
            out,
            "    ErrorCase {{ name: {name:?}, code: {} }},",
            expect_u64(&c, "code")
        );
    }
    out.push_str("];\n");

    let bits = load(&dir, "bits.json");
    out.push_str("pub static BITS_GET: &[BitsGetCase] = &[\n");
    for c in live(&bits, "bits_get") {
        let expect = if c.expect.ok {
            format!("Some({})", expect_u64(&c, "value"))
        } else {
            "None".to_owned()
        };
        let _ = writeln!(
            out,
            "    BitsGetCase {{ input: {}, offset: {}, width: {}, expect: {expect} }},",
            bytes(&input(&c)),
            param(&c, "bit_offset"),
            param(&c, "width")
        );
    }
    out.push_str("];\npub static BITS_SET: &[BitsSetCase] = &[\n");
    for c in live(&bits, "bits_set") {
        let expect = if c.expect.ok {
            format!(
                "Some({})",
                bytes(&hex::decode(expect_str(&c, "output_hex")).unwrap_or_default())
            )
        } else {
            "None".to_owned()
        };
        let _ = writeln!(
            out,
            "    BitsSetCase {{ input: {}, offset: {}, width: {}, value: {}, expect: {expect} }},",
            bytes(&input(&c)),
            param(&c, "bit_offset"),
            param(&c, "width"),
            param(&c, "value")
        );
    }
    out.push_str("];\n");

    let counters = load(&dir, "counters.json");
    out.push_str("pub static COUNTERS: &[CounterCase] = &[\n");
    for c in counters.cases.iter().filter(|c| c.superseded.is_none()) {
        let classify = c.operation == "counter_classify";
        let (a, b) = if classify {
            (param(c, "prev"), param(c, "next"))
        } else {
            (param(c, "value"), 0)
        };
        let expect = if !c.expect.ok {
            "CounterExpect::OutOfRange".to_owned()
        } else if classify {
            match expect_str(c, "class") {
                "in-sequence" => "CounterExpect::InSequence".to_owned(),
                "repeat" => "CounterExpect::Repeat".to_owned(),
                "backwards" => "CounterExpect::Backwards".to_owned(),
                "gap" => format!("CounterExpect::Gap({})", expect_u64(c, "missing")),
                other => panic!("{}: class {other}", c.id),
            }
        } else {
            format!("CounterExpect::Value({})", expect_u64(c, "value"))
        };
        let _ = writeln!(
            out,
            "    CounterCase {{ classify: {classify}, bits: {}, a: {a}, b: {b}, expect: {expect} }},",
            param(c, "bits")
        );
    }
    out.push_str("];\n");

    let layout = load(&dir, "layout.json");
    out.push_str("pub static LAYOUTS: &[LayoutCase] = &[\n");
    for c in live(&layout, "frame_layout") {
        let expect = if c.expect.ok {
            let mut v = Vec::new();
            for f in [
                "primary_header",
                "secondary",
                "security_header",
                "data",
                "security_trailer",
                "ocf",
                "fecf",
            ] {
                let pair = c
                    .expect
                    .values
                    .get(f)
                    .and_then(Value::as_array)
                    .unwrap_or_else(|| panic!("{}: {f}", c.id));
                v.extend(
                    pair.iter()
                        .map(|x| x.as_u64().unwrap_or_default().to_string()),
                );
            }
            format!("Some([{}])", v.join(", "))
        } else {
            "None".to_owned()
        };
        let _ = writeln!(
            out,
            "    LayoutCase {{ sizes: [{}, {}, {}, {}, {}], ocf: {}, fecf: {}, expect: {expect} }},",
            param(&c, "frame_len"),
            param(&c, "primary_header_len"),
            param(&c, "secondary_len"),
            param(&c, "security_header_len"),
            param(&c, "security_trailer_len"),
            flag(&c, "ocf"),
            flag(&c, "fecf")
        );
    }
    out.push_str("];\n");

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap_or_default());
    std::fs::write(out_dir.join("vectors.rs"), out).unwrap_or_else(|e| panic!("vectors.rs: {e}"));

    let mut codec = Codec {
        out: String::from(
            "// Generated by exerciser/build.rs from test-vectors/core/ocf.json and \
             test-vectors/{time,spp,epp,aos,tc}. Do not edit.\n\
             pub static CODEC: &[Case] = &[\n",
        ),
        out_dir,
        blobs: 0,
    };
    // The `aldis-core` OCF cases (M2 WP-1) share the value tree; the other core files have the
    // typed cases above.
    for c in load(&dir, "ocf.json")
        .cases
        .iter()
        .filter(|c| c.superseded.is_none())
    {
        codec.case("Ocf", c);
    }
    for (group, variant) in [
        ("time", "Time"),
        ("spp", "Spp"),
        ("epp", "Epp"),
        ("aos", "Aos"),
        ("tc", "Tc"),
    ] {
        let dir = manifest.join("../test-vectors").join(group);
        println!("cargo:rerun-if-changed={}", dir.display());
        let mut names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|path| path.extension().is_some_and(|x| x == "json"))
            .filter_map(|path| path.file_name()?.to_str().map(str::to_owned))
            .collect();
        names.sort();
        for name in names {
            for c in load(&dir, &name)
                .cases
                .iter()
                .filter(|c| c.superseded.is_none())
            {
                codec.case(variant, c);
            }
        }
    }
    codec.out.push_str("];\n");
    std::fs::write(codec.out_dir.join("codec.rs"), codec.out)
        .unwrap_or_else(|e| panic!("codec.rs: {e}"));

    // One start-up, semihosting trap, and linker script per architecture (M1 D6).
    let target = std::env::var("TARGET").unwrap_or_default();
    println!("cargo::rustc-check-cfg=cfg(cortex_m, cortex_r, riscv, sparc)");
    let (cfg, script) = if target.starts_with("thumbv") {
        ("cortex_m", "link-cortex-m.x")
    } else if target.starts_with("armv7r") {
        ("cortex_r", "link-cortex-r.x")
    } else if target.starts_with("riscv32") {
        ("riscv", "link-riscv.x")
    } else if target.starts_with("sparc-") {
        // LEON3 (SPARC V8), non-gating (OI-08).
        ("sparc", "link-leon3.x")
    } else {
        panic!("{target}: no start-up code for this target (exerciser/src/target.rs)")
    };
    println!("cargo::rustc-cfg={cfg}");
    println!("cargo:rerun-if-changed={script}");
    println!("cargo:rustc-link-arg=-T{}", manifest.join(script).display());
}

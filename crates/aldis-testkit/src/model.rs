//! Data model of the conformance test-vector files (docs/07-test-vectors.md).
//!
//! These types are the single source of truth for the vector format: `xtask vectors
//! validate` deserializes every file with them, and `xtask vectors schema` generates the
//! published JSON Schemas from them, so validator, runner, and schema cannot drift apart.

use std::num::NonZeroU16;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Where a vector file's expected values come from (TV-020).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum Provenance {
    /// Example printed in a CCSDS Blue Book.
    BlueBookExample,
    /// Derived by hand from the standard; `provenance_note` explains the derivation.
    HandDerived,
    /// Agreed with an independent implementation named in `provenance_note`.
    CrossChecked,
    /// Regression case produced by fuzzing.
    FuzzRegression,
}

/// Decoding mode of a codec case (API-030).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    /// Reject any deviation from the standard (default).
    #[default]
    Strict,
    /// Accept reserved-field deviations and report anomalies.
    Lenient,
}

/// Marks a released case as replaced or retired; released cases are never edited (TV-021).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Superseded {
    /// Why the case was superseded; required and not empty.
    pub reason: String,
    /// Identifier of the replacing case, if there is one. When present it must name an
    /// existing case that is not itself superseded; a wrong case with no correct replacement
    /// leaves it out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
}

/// Value of the `kind` field of a codec vector file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum CodecKind {
    /// The only value: `"codec"`.
    Codec,
}

/// A file of stateless codec cases (TV-001…TV-003).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CodecFile {
    /// Always `"codec"`.
    pub kind: CodecKind,
    /// CCSDS document(s) and clause(s) the cases exercise.
    pub standard: String,
    /// Origin of the expected values.
    pub provenance: Provenance,
    /// Derivation or cross-check details; required unless provenance is `blue-book-example`
    /// or `fuzz-regression` (checked by `xtask vectors validate`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance_note: Option<String>,
    /// The cases.
    pub cases: Vec<CodecCase>,
}

/// One codec case.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CodecCase {
    /// Globally unique case identifier.
    pub id: String,
    /// Requirement IDs the case verifies (TV-003).
    pub requirements: Vec<String>,
    /// Human-readable description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Operation name, interpreted by the crate's test handler.
    pub operation: String,
    /// Decoding mode; strict when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<Mode>,
    /// Managed parameters (configuration) for the operation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<Map<String, Value>>,
    /// Operation parameters that are not configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Map<String, Value>>,
    /// Input octets as upper- or lower-case hex without separators.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_hex: Option<String>,
    /// Expected outcome.
    pub expect: Expect,
    /// Present when this released case has been replaced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superseded: Option<Superseded>,
}

/// Expected outcome of a codec case.
///
/// When `ok` is true, every other key is an expected output value and the handler's output
/// must match them **exactly**: no missing and no extra keys (TV-002, TV-012). When `ok` is
/// false, `error` names the expected error kind from the catalogue (03-api-model.md §9), and
/// `field`, when present, the code of the field the error names (03-api-model.md §9.1;
/// API-082, M2 plan D3).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Expect {
    /// Whether the operation succeeds.
    pub ok: bool,
    /// Expected error kind when `ok` is false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Code of the field the expected error names (03-api-model.md §9.1), when `ok` is false.
    /// Compared only when present: cases released before field codes existed leave it out
    /// (TV-021).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<NonZeroU16>,
    /// Expected output values when `ok` is true.
    #[serde(flatten)]
    pub values: Map<String, Value>,
}

/// Value of the `kind` field of an engine scenario file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum ScenarioKind {
    /// The only value: `"engine-scenario"`.
    EngineScenario,
}

/// A file of scripted scenarios for one engine (TV-010…TV-012). The provenance, the
/// configuration, and the named setups are given once per file; each scenario refers to a setup
/// and adds its own steps.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ScenarioFile {
    /// Always `"engine-scenario"`.
    pub kind: ScenarioKind,
    /// Engine name (e.g. `fop1`, `farm1`, `packet-extractor`).
    pub engine: String,
    /// Origin of the expected values, for every scenario of the file.
    pub provenance: Provenance,
    /// Derivation or cross-check details, for every scenario of the file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance_note: Option<String>,
    /// Engine configuration (managed parameters), unless a scenario gives its own.
    pub config: Map<String, Value>,
    /// Named input sequences that reach a state, defined once and used by the scenarios.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub setups: Vec<Setup>,
    /// The scenarios, in file order.
    pub scenarios: Vec<Scenario>,
}

/// A named input sequence that reaches a state. Its steps are run, with their expectations,
/// before the steps of a scenario that uses it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Setup {
    /// The name a scenario or another setup refers to; unique in the file.
    pub name: String,
    /// The setup that runs first, when this one continues it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extends: Option<String>,
    /// Ordered steps.
    pub steps: Vec<Step>,
}

/// One scenario: an optional setup, then its own steps.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    /// Globally unique scenario identifier.
    pub id: String,
    /// Human-readable description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Requirement IDs the scenario verifies (TV-003).
    pub requirements: Vec<String>,
    /// The setup to run first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup: Option<String>,
    /// Engine configuration that replaces the file's, for this scenario.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<Map<String, Value>>,
    /// Ordered steps, after the setup.
    pub steps: Vec<Step>,
    /// Present when this released scenario has been replaced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superseded: Option<Superseded>,
}

/// A scenario with its setup chain expanded, ready to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedScenario<'a> {
    /// The scenario.
    pub scenario: &'a Scenario,
    /// The configuration in force: the scenario's own, else the file's.
    pub config: &'a Map<String, Value>,
    /// The steps to run in order: the setup chain, then the scenario's.
    pub steps: Vec<&'a Step>,
}

impl ScenarioFile {
    /// Expands every scenario's setup chain.
    ///
    /// # Errors
    /// Returns a message if a setup is named twice, a setup or scenario refers to a setup that
    /// does not exist, or the `extends` chain loops.
    pub fn resolve(&self) -> Result<Vec<ResolvedScenario<'_>>, String> {
        let mut by_name = std::collections::BTreeMap::<&str, &Setup>::new();
        for setup in &self.setups {
            if by_name.insert(&setup.name, setup).is_some() {
                return Err(format!("setup {:?} is defined twice", setup.name));
            }
        }
        let mut out = Vec::with_capacity(self.scenarios.len());
        for scenario in &self.scenarios {
            let mut chain: Vec<&Setup> = Vec::new();
            let mut next = scenario.setup.as_deref();
            while let Some(name) = next {
                let setup = by_name
                    .get(name)
                    .ok_or_else(|| format!("{}: the setup {name:?} does not exist", scenario.id))?;
                if chain.len() > self.setups.len() {
                    return Err(format!("{}: the setups loop at {name:?}", scenario.id));
                }
                chain.push(setup);
                next = setup.extends.as_deref();
            }
            let mut steps: Vec<&Step> = chain.iter().rev().flat_map(|s| &s.steps).collect();
            steps.extend(&scenario.steps);
            out.push(ResolvedScenario {
                scenario,
                config: scenario.config.as_ref().unwrap_or(&self.config),
                steps,
            });
        }
        Ok(out)
    }
}

/// One scenario step: one input at an absolute instant, and the complete ordered outputs.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Step {
    /// Absolute instant in microseconds (`Instant`).
    pub at_us: u64,
    /// The single input of this step.
    pub input: Map<String, Value>,
    /// Complete, ordered outputs expected before the next step; anything else is a failure.
    pub expect: Vec<Map<String, Value>>,
    /// Expected engine state after the step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expect_state: Option<String>,
    /// Expected `next_deadline` after the step, in microseconds: absent means not asserted,
    /// `null` means no deadline, a number is the deadline.
    #[serde(
        default,
        deserialize_with = "present_or_null",
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(feature = "schema", schemars(with = "Option<u64>"))]
    #[expect(
        clippy::option_option,
        reason = "three states are the point: absent, null (no deadline), a number"
    )]
    pub expect_timeout_us: Option<Option<u64>>,
}

/// Reads a key that is present as `Some(value)`, where `null` is `Some(None)`; a key that is
/// absent stays `None` through `#[serde(default)]`.
#[expect(
    clippy::option_option,
    reason = "three states are the point: absent, null (no deadline), a number"
)]
fn present_or_null<'de, D>(deserializer: D) -> Result<Option<Option<u64>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<u64>::deserialize(deserializer).map(Some)
}

/// Kind of a field-catalogue entry (03-api-model.md §9.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum FieldKind {
    /// A field of a data unit defined by a Blue Book.
    Field,
    /// A managed parameter, configuration value, argument, or length the caller passes.
    Parameter,
}

/// One entry of the field catalogue (03-api-model.md §9.1, API-082).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FieldEntry {
    /// The field code; never 0, which means "no field" in the C ABI.
    pub code: NonZeroU16,
    /// The display name, never compared (API-082).
    pub name: String,
    /// Whether the code names a Blue Book field or a parameter.
    pub kind: FieldKind,
}

/// `test-vectors/core/fields.json`: the field catalogue of 03-api-model.md §9.1 for the C++
/// and binding runners, generated by `cargo xtask fields --write` (API-082, M2 plan D2).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FieldCatalogue {
    /// Where the entries come from; the file is never edited by hand.
    pub source: String,
    /// Every entry, in code order from 1. The codes of one block run without a gap; the blocks
    /// that are not full leave gaps between them (03-api-model.md §9.1).
    pub fields: Vec<FieldEntry>,
}

/// Any vector file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VectorFile {
    /// Codec cases.
    Codec(CodecFile),
    /// Engine scenario.
    EngineScenario(ScenarioFile),
}

impl VectorFile {
    /// Parses a vector file, dispatching on its `kind` field.
    ///
    /// # Errors
    /// Returns a message if the JSON is malformed, the kind is unknown, or the content does
    /// not match the model (unknown fields are rejected).
    pub fn from_json(text: &str) -> Result<Self, String> {
        let value: Value = serde_json::from_str(text).map_err(|e| format!("invalid JSON: {e}"))?;
        match value.get("kind").and_then(Value::as_str) {
            Some("codec") => serde_json::from_value(value)
                .map(VectorFile::Codec)
                .map_err(|e| format!("codec file: {e}")),
            Some("engine-scenario") => serde_json::from_value(value)
                .map(VectorFile::EngineScenario)
                .map_err(|e| format!("engine-scenario file: {e}")),
            Some(other) => Err(format!("unknown kind {other:?}")),
            None => Err("missing string field \"kind\"".to_owned()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = r#"{
      "kind": "engine-scenario", "engine": "e", "provenance": "hand-derived",
      "provenance_note": "n", "config": {"a": 1},
      "setups": [
        {"name": "one", "steps": [{"at_us": 0, "input": {"x": 1}, "expect": []}]},
        {"name": "two", "extends": "one",
         "steps": [{"at_us": 5, "input": {"x": 2}, "expect": [], "expect_timeout_us": null}]}
      ],
      "scenarios": [
        {"id": "s", "requirements": ["COP-001"], "setup": "two",
         "steps": [{"at_us": 9, "input": {"x": 3}, "expect": [], "expect_timeout_us": 7}]},
        {"id": "t", "requirements": ["COP-001"], "config": {"a": 2},
         "steps": [{"at_us": 0, "input": {}, "expect": []}]}
      ]
    }"#;

    #[test]
    fn a_scenario_file_expands_its_setup_chain() {
        // req: TV-010
        let Ok(VectorFile::EngineScenario(file)) = VectorFile::from_json(FILE) else {
            panic!("not a scenario file");
        };
        let resolved = file.resolve().unwrap();
        let at: Vec<u64> = resolved[0].steps.iter().map(|s| s.at_us).collect();
        assert_eq!(at, [0, 5, 9]);
        assert_eq!(resolved[0].config["a"], 1);
        assert_eq!(resolved[1].config["a"], 2);
        assert_eq!(resolved[1].steps.len(), 1);
    }

    #[test]
    fn the_deadline_is_absent_null_or_a_number() {
        // req: TV-010
        let Ok(VectorFile::EngineScenario(file)) = VectorFile::from_json(FILE) else {
            panic!("not a scenario file");
        };
        assert_eq!(file.setups[0].steps[0].expect_timeout_us, None);
        assert_eq!(file.setups[1].steps[0].expect_timeout_us, Some(None));
        assert_eq!(file.scenarios[0].steps[0].expect_timeout_us, Some(Some(7)));
    }

    #[test]
    fn unknown_and_looping_setups_are_refused() {
        // req: TV-010
        let missing = FILE.replace("\"setup\": \"two\"", "\"setup\": \"nope\"");
        let Ok(VectorFile::EngineScenario(file)) = VectorFile::from_json(&missing) else {
            panic!("not a scenario file");
        };
        assert!(file.resolve().is_err());
        let looped = FILE.replace(
            "\"name\": \"one\",",
            "\"name\": \"one\", \"extends\": \"two\",",
        );
        let Ok(VectorFile::EngineScenario(file)) = VectorFile::from_json(&looped) else {
            panic!("not a scenario file");
        };
        assert!(file.resolve().is_err());
    }
}

//! Data model of the conformance test-vector files (docs/07-test-vectors.md).
//!
//! These types are the single source of truth for the vector format: `xtask vectors
//! validate` deserializes every file with them, and `xtask vectors schema` generates the
//! published JSON Schemas from them, so validator, runner, and schema cannot drift apart.

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

/// Marks a released case as replaced; released cases are never edited (TV-021).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Superseded {
    /// Why the case was superseded.
    pub reason: String,
    /// Identifier of the replacing case, if any.
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
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
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
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
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
/// false, `error` names the expected error kind from the catalogue (03-api-model.md §9).
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Expect {
    /// Whether the operation succeeds.
    pub ok: bool,
    /// Expected error kind when `ok` is false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
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

/// A scripted sequence of inputs and expected outputs for one engine (TV-010…TV-012).
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ScenarioFile {
    /// Always `"engine-scenario"`.
    pub kind: ScenarioKind,
    /// Engine name (e.g. `fop1`, `farm1`, `packet-extractor`).
    pub engine: String,
    /// Globally unique scenario identifier.
    pub id: String,
    /// Requirement IDs the scenario verifies (TV-003).
    pub requirements: Vec<String>,
    /// Human-readable description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Origin of the expected values.
    pub provenance: Provenance,
    /// Derivation or cross-check details.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance_note: Option<String>,
    /// Engine configuration (managed parameters).
    pub config: Map<String, Value>,
    /// Ordered steps.
    pub steps: Vec<Step>,
    /// Present when this released scenario has been replaced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superseded: Option<Superseded>,
}

/// One scenario step: one input at an absolute instant, and the complete ordered outputs.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
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
    /// Expected `poll_timeout` after the step, in microseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expect_timeout_us: Option<u64>,
}

/// Any vector file.
#[derive(Debug, Clone, PartialEq)]
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

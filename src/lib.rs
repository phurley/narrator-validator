//! Shared validation engine for complete Narrator story repositories.
//!
//! The core API accepts source text rather than filesystem paths so the same
//! implementation can run in the backend, a browser Web Worker, or the CLI.

mod diagnostic;
mod end_state;
mod playability;
mod reference_text;
mod ruleset;
mod scanner_control;
mod solution;
mod validator;

pub use diagnostic::{
    Diagnostic, Position, RelatedLocation, Severity, SourceFile, SourceRange, ValidationReport,
};
pub use end_state::{
    end_state_contract_metadata, end_state_contract_metadata_json, EndStateContractMetadata,
    OutcomeResolution, END_STATE_STORY_FORMAT_VERSION,
};
pub use playability::{
    DeductionGraphAnalysis, NotebookPolicyAnalysis, PlayabilityBlocker, PlayabilityLowerBound,
    PlayabilityReport, PlayabilityRequiredWait, PlayabilityStatus, PlayabilityStep,
    SolutionAnswerability, TerminalPathAnalysis,
};
pub use reference_text::{
    parse_reference_text, parse_reference_text_result, reference_kind,
    reference_text_metadata_json, ConsumerField, DisclosureClass, ParsedReferenceText,
    ReferenceExpression, ReferenceKind, ReferenceParseError, ReferencePath, ReferenceProvenance,
    ReferenceTextParseResult, ReferenceTextSegment, ResolvedReferenceText, CONSUMER_FIELDS,
    REFERENCE_KINDS, REFERENCE_TEXT_FEATURE, SUPPORTED_FEATURES,
};
pub use ruleset::{
    resolve_ruleset, ResolvedRuleset, RulesetCommandCapability, RulesetError, RulesetReference,
    STANDARD_MYSTERY_RULESET_ID, STANDARD_MYSTERY_RULESET_VERSION,
    STANDARD_MYSTERY_RULESET_VERSION_1, STANDARD_MYSTERY_RULESET_VERSION_2,
    STANDARD_MYSTERY_RULESET_VERSION_3, STANDARD_MYSTERY_RULESET_VERSION_4,
    STANDARD_MYSTERY_RULESET_VERSION_5,
};
pub use scanner_control::{
    reserved_scanner_control_tags, scanner_control_dart_source, scanner_control_manifest_json,
    scanner_control_role_for_tag_id, ScannerControlManifestEntry, ScannerControlRole,
    ENTER_1_TAG_ID, ENTER_2_TAG_ID, RESERVED_SCANNER_CONTROL_TAG_IDS,
};
pub use solution::{
    solution_answer_matches, solution_contract_metadata, solution_contract_metadata_json,
    SolutionContractMetadata, MAX_SOLUTION_ANSWER_CARDS, MAX_SOLUTION_QUESTIONS,
    MIN_SOLUTION_ANSWER_CARDS, MIN_SOLUTION_QUESTIONS, SOLUTION_STORY_FORMAT_VERSION,
};
pub use validator::{
    validate, validate_with_supported_features, validate_without_playability,
    validate_without_playability_with_features,
};

pub const VALIDATOR_VERSION: &str = env!("CARGO_PKG_VERSION");
/// Latest story format authored by this release.
pub const STORY_FORMAT_VERSION: &str = "3.5.0";
/// Semantic-version range this release can structurally validate. Format 3.2+
/// features still require successful `case.features` negotiation, while the
/// Format 3.3 Solve contract is selected by its exact ruleset version.
pub const SUPPORTED_STORY_FORMATS: &str = ">=1.0.0, <2.0.0 or >=3.0.0, <4.0.0";

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod wasm {
    use wasm_bindgen::prelude::*;

    use crate::{
        end_state_contract_metadata_json, parse_reference_text_result,
        reference_text_metadata_json, solution_answer_matches, solution_contract_metadata_json,
        validate, validate_with_supported_features, validate_without_playability_with_features,
        SourceFile,
    };

    /// Validate a JSON array of `{ "path": string, "source": string }` values.
    ///
    /// Returning JSON keeps the JavaScript boundary stable and avoids exposing
    /// Rust/wasm-bindgen implementation types to TypeScript callers.
    #[wasm_bindgen]
    pub fn validate_json(files_json: &str) -> Result<String, JsValue> {
        let files: Vec<SourceFile> = serde_json::from_str(files_json)
            .map_err(|error| JsValue::from_str(&format!("invalid source-file JSON: {error}")))?;
        serde_json::to_string(&validate(&files))
            .map_err(|error| JsValue::from_str(&format!("could not serialize report: {error}")))
    }

    /// Validate with the exact capabilities advertised by a browser consumer.
    #[wasm_bindgen]
    pub fn validate_json_with_features(
        files_json: &str,
        supported_features_json: &str,
    ) -> Result<String, JsValue> {
        let files: Vec<SourceFile> = serde_json::from_str(files_json)
            .map_err(|error| JsValue::from_str(&format!("invalid source-file JSON: {error}")))?;
        let features: Vec<String> =
            serde_json::from_str(supported_features_json).map_err(|error| {
                JsValue::from_str(&format!("invalid supported-feature JSON: {error}"))
            })?;
        serde_json::to_string(&validate_with_supported_features(&files, &features))
            .map_err(|error| JsValue::from_str(&format!("could not serialize report: {error}")))
    }

    /// Validate structure and references without the bounded playability search.
    ///
    /// This export also gives profiling tools a mechanically comparable baseline
    /// for separating validation work from state-space exploration.
    #[wasm_bindgen]
    pub fn validate_json_without_playability_with_features(
        files_json: &str,
        supported_features_json: &str,
    ) -> Result<String, JsValue> {
        let files: Vec<SourceFile> = serde_json::from_str(files_json)
            .map_err(|error| JsValue::from_str(&format!("invalid source-file JSON: {error}")))?;
        let features: Vec<String> =
            serde_json::from_str(supported_features_json).map_err(|error| {
                JsValue::from_str(&format!("invalid supported-feature JSON: {error}"))
            })?;
        serde_json::to_string(&validate_without_playability_with_features(
            &files, &features,
        ))
        .map_err(|error| JsValue::from_str(&format!("could not serialize report: {error}")))
    }

    #[wasm_bindgen]
    pub fn reference_text_metadata_json_export() -> String {
        reference_text_metadata_json()
    }

    #[wasm_bindgen]
    pub fn solution_contract_metadata_json_export() -> String {
        solution_contract_metadata_json()
    }

    #[wasm_bindgen]
    pub fn end_state_contract_metadata_json_export() -> String {
        end_state_contract_metadata_json()
    }

    #[wasm_bindgen]
    pub fn solution_answer_matches_json(
        expected_json: &str,
        submitted_json: &str,
        ordered: bool,
    ) -> Result<bool, JsValue> {
        let expected: Vec<String> = serde_json::from_str(expected_json).map_err(|error| {
            JsValue::from_str(&format!("invalid expected-answer JSON: {error}"))
        })?;
        let submitted: Vec<String> = serde_json::from_str(submitted_json).map_err(|error| {
            JsValue::from_str(&format!("invalid submitted-answer JSON: {error}"))
        })?;
        Ok(solution_answer_matches(&expected, &submitted, ordered))
    }

    /// Parse reference-aware prose without validating a repository.
    #[wasm_bindgen]
    pub fn parse_reference_text_json(source: &str) -> Result<String, JsValue> {
        serde_json::to_string(&parse_reference_text_result(source)).map_err(|error| {
            JsValue::from_str(&format!("could not serialize parse result: {error}"))
        })
    }
}

//! Shared validation engine for complete Narrator story repositories.
//!
//! The core API accepts source text rather than filesystem paths so the same
//! implementation can run in the backend, a browser Web Worker, or the CLI.

mod diagnostic;
mod validator;

pub use diagnostic::{
    Diagnostic, Position, RelatedLocation, Severity, SourceFile, SourceRange, ValidationReport,
};
pub use validator::validate;

pub const VALIDATOR_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod wasm {
    use wasm_bindgen::prelude::*;

    use crate::{validate, SourceFile};

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
}

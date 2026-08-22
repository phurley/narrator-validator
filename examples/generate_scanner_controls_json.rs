//! Print the reserved scanner-control tag manifest as pretty JSON to stdout.
//!
//! `docs/scanner-controls.json` is generated from this crate's actual
//! reservation (`narrator_validator::reserved_scanner_control_tags`), not
//! hand-maintained, so it cannot drift from `src/scanner_control.rs` --
//! `tests/validation.rs`'s
//! `checked_in_scanner_control_artifacts_match_the_source` test fails if it
//! ever does.
//!
//! Regenerate after any change to the reservation:
//!
//! ```text
//! cargo run --example generate_scanner_controls_json > docs/scanner-controls.json
//! ```

fn main() {
    print!("{}", narrator_validator::scanner_control_manifest_json());
}

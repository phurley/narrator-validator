//! Print the reserved scanner-control tag IDs as generated Dart source to
//! stdout, for narrator-app to consume.
//!
//! `docs/scanner_control.dart` is generated from this crate's actual
//! reservation (`narrator_validator::scanner_control_dart_source`), not
//! hand-maintained, so it cannot drift from `src/scanner_control.rs` --
//! `tests/validation.rs`'s
//! `checked_in_scanner_control_artifacts_match_the_source` test fails if it
//! ever does. narrator-app vendors this file verbatim as
//! `lib/core/apriltag/scanner_control.dart`.
//!
//! Regenerate after any change to the reservation:
//!
//! ```text
//! cargo run --example generate_scanner_controls_dart > docs/scanner_control.dart
//! ```

fn main() {
    print!("{}", narrator_validator::scanner_control_dart_source());
}

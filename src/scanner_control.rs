//! Reserved tagStandard41h12 identities for the physical ENTER scanner
//! control cards.
//!
//! ENTER-1 and ENTER-2 are recognised by the camera before any story deck is
//! parsed, so they must never be assignable to a story `subject` in
//! `deck.yaml`. Consumers (the app's scanner and the printable-fixture
//! generator) need a cheap, unambiguous way to recognise a scanned tag as a
//! scanner-control card *before* clustering it against any resolved deck, so
//! the reservation is exposed as a plain integer comparison rather than a
//! lookup that depends on a resolved card.
//!
//! This module is the single authority for the reservation. Non-Rust
//! consumers (narrator-app's Dart scanner) must not hand-copy these
//! constants; instead they consume the generated artifacts in
//! `docs/scanner-controls.json` and `docs/scanner_control.dart`, produced
//! from [`reserved_scanner_control_tags`] by `examples/generate_scanner_controls_json.rs`
//! and `examples/generate_scanner_controls_dart.rs`. `tests/validation.rs`'s
//! `checked_in_scanner_control_artifacts_match_the_source` fails if either
//! checked-in copy drifts from this source. Adding a third reserved control
//! id means editing only [`RESERVED_SCANNER_CONTROL_TAG_IDS`] and
//! [`ScannerControlRole`] here, then regenerating both artifacts.

/// Reserved tagStandard41h12 ID for the ENTER-1 scanner control card.
///
/// Chosen from the top of the tagStandard41h12 range (0 through 2114) so
/// reservations are unlikely to collide with low, densely-authored IDs in
/// existing decks.
pub const ENTER_1_TAG_ID: i64 = 2114;

/// Reserved tagStandard41h12 ID for the ENTER-2 scanner control card.
pub const ENTER_2_TAG_ID: i64 = 2113;

/// The two tagStandard41h12 IDs permanently reserved for scanner control
/// cards, in a stable, iterable form so guards can assert their count.
pub const RESERVED_SCANNER_CONTROL_TAG_IDS: [(ScannerControlRole, i64); 2] = [
    (ScannerControlRole::Enter1, ENTER_1_TAG_ID),
    (ScannerControlRole::Enter2, ENTER_2_TAG_ID),
];

/// A scanner-control role permanently excluded from story deck assignment.
///
/// These are not `command.*` items, deck subjects, candidates, or
/// game-engine action cards; the app removes them before creating an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScannerControlRole {
    Enter1,
    Enter2,
}

impl ScannerControlRole {
    /// The reserved tagStandard41h12 ID for this role.
    pub const fn tag_id(self) -> i64 {
        match self {
            ScannerControlRole::Enter1 => ENTER_1_TAG_ID,
            ScannerControlRole::Enter2 => ENTER_2_TAG_ID,
        }
    }

    /// The stable, human-readable name for this role, e.g. in diagnostics.
    pub const fn name(self) -> &'static str {
        match self {
            ScannerControlRole::Enter1 => "ENTER-1",
            ScannerControlRole::Enter2 => "ENTER-2",
        }
    }

    /// The stable lower_snake_case identifier used in generated artifacts.
    ///
    /// This is the field non-Rust consumers key their generated enum
    /// variants and constants on, so it must also stay a valid Dart
    /// identifier.
    pub const fn identifier(self) -> &'static str {
        match self {
            ScannerControlRole::Enter1 => "enter1",
            ScannerControlRole::Enter2 => "enter2",
        }
    }
}

/// Returns the scanner-control role reserving `tag_id`, if any.
///
/// This is the cheap, unambiguous check consumers should use to recognise a
/// scanned tag as a scanner control before resolving it against any deck.
pub fn scanner_control_role_for_tag_id(tag_id: i64) -> Option<ScannerControlRole> {
    RESERVED_SCANNER_CONTROL_TAG_IDS
        .iter()
        .find(|(_, id)| *id == tag_id)
        .map(|(role, _)| *role)
}

/// One entry of the reserved scanner-control manifest, in the shape shared
/// by the generated JSON and Dart artifacts.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ScannerControlManifestEntry {
    /// Lower_snake_case identifier, e.g. `"enter1"`.
    pub role: &'static str,
    /// Human-readable label, e.g. `"ENTER-1"`.
    pub label: &'static str,
    /// The reserved tagStandard41h12 ID.
    pub tag_id: i64,
}

/// The reserved scanner-control tags in the stable order consumed by the
/// generated artifacts.
pub fn reserved_scanner_control_tags() -> Vec<ScannerControlManifestEntry> {
    RESERVED_SCANNER_CONTROL_TAG_IDS
        .iter()
        .map(|(role, tag_id)| ScannerControlManifestEntry {
            role: role.identifier(),
            label: role.name(),
            tag_id: *tag_id,
        })
        .collect()
}

/// The pretty-printed JSON manifest checked in at `docs/scanner-controls.json`.
pub fn scanner_control_manifest_json() -> String {
    #[derive(serde::Serialize)]
    struct Manifest {
        reserved_scanner_control_tags: Vec<ScannerControlManifestEntry>,
    }

    let manifest = Manifest {
        reserved_scanner_control_tags: reserved_scanner_control_tags(),
    };
    let mut json = serde_json::to_string_pretty(&manifest)
        .expect("static scanner control manifest serializes");
    json.push('\n');
    json
}

/// The generated Dart source checked in at
/// `lib/core/apriltag/scanner_control.dart` in narrator-app, generated here
/// as `docs/scanner_control.dart`.
pub fn scanner_control_dart_source() -> String {
    let entries = reserved_scanner_control_tags();

    let mut out = String::new();
    out.push_str("// GENERATED FILE. DO NOT EDIT BY HAND.\n");
    out.push_str("//\n");
    out.push_str("// Generated from narrator-validator's src/scanner_control.rs, the single\n");
    out.push_str("// authority for the reserved ENTER scanner-control tag IDs. Regenerate with:\n");
    out.push_str("//\n");
    out.push_str(
        "//   cargo run --example generate_scanner_controls_dart > docs/scanner_control.dart\n",
    );
    out.push_str("//\n");
    out.push_str("// in narrator-validator, then copy the file into narrator-app at\n");
    out.push_str("// lib/core/apriltag/scanner_control.dart.\n");
    out.push('\n');
    out.push_str("/// Reserved tagStandard41h12 identities for the physical ENTER scanner\n");
    out.push_str("/// control cards. These are never assignable to a story `subject` and must\n");
    out.push_str(
        "/// be recognised before any scanned tag is clustered against a resolved deck.\n",
    );
    out.push_str("enum ScannerControlRole {\n");
    for entry in &entries {
        out.push_str(&format!("  {},\n", entry.role));
    }
    out.push_str("}\n");
    out.push('\n');
    out.push_str("extension ScannerControlRoleTagId on ScannerControlRole {\n");
    out.push_str("  /// The reserved tagStandard41h12 ID for this role.\n");
    out.push_str("  int get tagId {\n");
    out.push_str("    switch (this) {\n");
    for entry in &entries {
        out.push_str(&format!(
            "      case ScannerControlRole.{}:\n        return {};\n",
            entry.role, entry.tag_id
        ));
    }
    out.push_str("    }\n");
    out.push_str("  }\n");
    out.push('\n');
    out.push_str("  /// The stable, human-readable name for this role, e.g. in diagnostics.\n");
    out.push_str("  String get label {\n");
    out.push_str("    switch (this) {\n");
    for entry in &entries {
        out.push_str(&format!(
            "      case ScannerControlRole.{}:\n        return '{}';\n",
            entry.role, entry.label
        ));
    }
    out.push_str("    }\n");
    out.push_str("  }\n");
    out.push_str("}\n");
    out.push('\n');
    out.push_str("/// Returns the scanner-control role reserving [tagId], if any.\n");
    out.push_str("ScannerControlRole? scannerControlRoleForTagId(int tagId) {\n");
    out.push_str("  for (final role in ScannerControlRole.values) {\n");
    out.push_str("    if (role.tagId == tagId) return role;\n");
    out.push_str("  }\n");
    out.push_str("  return null;\n");
    out.push_str("}\n");
    out
}

//! Story Format 3.4 authored terminal-state contract.

use serde::Serialize;

pub const END_STATE_STORY_FORMAT_VERSION: &str = "3.4.0";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EndStateContractMetadata {
    pub story_format_version: &'static str,
    pub canonical_section: &'static str,
    pub canonical_file: &'static str,
    pub legacy_section: &'static str,
    pub precedence: &'static str,
    pub evaluation_timing: &'static str,
    pub score_semantics: &'static str,
    pub legacy_outcome: &'static str,
    pub legacy_resolution: &'static str,
    pub legal_outcome_resolutions: &'static [OutcomeResolution],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OutcomeResolution {
    pub outcome: &'static str,
    pub resolutions: &'static [&'static str],
}

const WON_RESOLUTIONS: &[&str] = &["full", "partial"];
const LOST_RESOLUTIONS: &[&str] = &["failure"];
const LEGAL_OUTCOME_RESOLUTIONS: &[OutcomeResolution] = &[
    OutcomeResolution {
        outcome: "won",
        resolutions: WON_RESOLUTIONS,
    },
    OutcomeResolution {
        outcome: "lost",
        resolutions: LOST_RESOLUTIONS,
    },
];

pub fn end_state_contract_metadata() -> EndStateContractMetadata {
    EndStateContractMetadata {
        story_format_version: END_STATE_STORY_FORMAT_VERSION,
        canonical_section: "end_states",
        canonical_file: "end_states.yaml",
        legacy_section: "win_states",
        precedence: "authored_order_first_satisfied",
        evaluation_timing: "after_every_resolved_turn",
        score_semantics: "snapshot_and_minimum_gate",
        legacy_outcome: "won",
        legacy_resolution: "full",
        legal_outcome_resolutions: LEGAL_OUTCOME_RESOLUTIONS,
    }
}

pub fn end_state_contract_metadata_json() -> String {
    serde_json::to_string(&end_state_contract_metadata())
        .expect("static end-state contract metadata serializes")
}

/// Root directory for ADR-021 story tests: `scripts/<end_state id>/*.json`.
pub const STORY_TEST_ROOT: &str = "scripts";

/// Whether `path` names an ADR-021 story test script.
///
/// Story tests live exactly one directory below [`STORY_TEST_ROOT`]. JSON
/// files elsewhere under that root are story-owned support data, and
/// `<name>.meta.json` files are optional story-test sidecars.
pub fn is_story_test_path(path: &str) -> bool {
    let mut segments = path.split('/');
    let (Some(root), Some(end_state), Some(name), None) = (
        segments.next(),
        segments.next(),
        segments.next(),
        segments.next(),
    ) else {
        return false;
    };

    root == STORY_TEST_ROOT
        && !end_state.is_empty()
        && !name.is_empty()
        && name.ends_with(".json")
        && !name.ends_with(".meta.json")
}

/// The directory an end state's ADR-021 story tests live under.
pub fn story_test_directory(end_state_id: &str) -> String {
    format!("{STORY_TEST_ROOT}/{end_state_id}")
}

#[cfg(test)]
mod tests {
    use super::is_story_test_path;

    #[test]
    fn story_test_paths_have_exactly_an_end_state_and_json_name() {
        assert!(is_story_test_path("scripts/end.full_solution/a.json"));

        for path in [
            "scripts/shared-deck.json",
            "scripts/end.full_solution/a.meta.json",
            "scripts/end.full_solution/deep/a.json",
            "other/end.full_solution/a.json",
            "scripts//a.json",
        ] {
            assert!(!is_story_test_path(path), "{path}");
        }
    }
}

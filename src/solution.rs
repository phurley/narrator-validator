//! Shared Story Format 3.3 authored-solution contract.

use std::collections::BTreeSet;

use serde::Serialize;

use crate::{DisclosureClass, STANDARD_MYSTERY_RULESET_ID, STANDARD_MYSTERY_RULESET_VERSION_3};

pub const MIN_SOLUTION_QUESTIONS: usize = 1;
pub const MAX_SOLUTION_QUESTIONS: usize = 4;
pub const MIN_SOLUTION_ANSWER_CARDS: usize = 1;
pub const MAX_SOLUTION_ANSWER_CARDS: usize = 5;
pub const SOLUTION_STORY_FORMAT_VERSION: &str = "3.3.0";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SolutionContractMetadata {
    pub story_format_version: &'static str,
    pub ruleset_id: &'static str,
    pub ruleset_version: &'static str,
    pub min_questions: usize,
    pub max_questions: usize,
    pub min_answer_cards: usize,
    pub max_answer_cards: usize,
    pub ordered_default: bool,
    pub prompt_disclosure: DisclosureClass,
    pub expected_answer_disclosure: DisclosureClass,
}

pub fn solution_contract_metadata() -> SolutionContractMetadata {
    SolutionContractMetadata {
        story_format_version: SOLUTION_STORY_FORMAT_VERSION,
        ruleset_id: STANDARD_MYSTERY_RULESET_ID,
        ruleset_version: STANDARD_MYSTERY_RULESET_VERSION_3,
        min_questions: MIN_SOLUTION_QUESTIONS,
        max_questions: MAX_SOLUTION_QUESTIONS,
        min_answer_cards: MIN_SOLUTION_ANSWER_CARDS,
        max_answer_cards: MAX_SOLUTION_ANSWER_CARDS,
        ordered_default: false,
        prompt_disclosure: DisclosureClass::PlayerSafe,
        expected_answer_disclosure: DisclosureClass::PrivateNarrator,
    }
}

pub fn solution_contract_metadata_json() -> String {
    serde_json::to_string(&solution_contract_metadata())
        .expect("static solution contract metadata is serializable")
}

/// Compare one submitted answer row using the exact Format 3.3 semantics.
/// Validation separately guarantees that authored answers contain unique IDs.
pub fn solution_answer_matches(expected: &[String], submitted: &[String], ordered: bool) -> bool {
    if expected.len() != submitted.len() {
        return false;
    }
    if ordered {
        return expected == submitted;
    }
    let expected_len = expected.len();
    let submitted_len = submitted.len();
    let expected = expected.iter().collect::<BTreeSet<_>>();
    let submitted = submitted.iter().collect::<BTreeSet<_>>();
    expected.len() == expected_len && submitted.len() == submitted_len && expected == submitted
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn unordered_answers_require_the_exact_set() {
        let expected = ids(&["entity.knife", "entity.bottle"]);
        assert!(solution_answer_matches(
            &expected,
            &ids(&["entity.bottle", "entity.knife"]),
            false
        ));
        assert!(!solution_answer_matches(
            &expected,
            &ids(&["entity.knife"]),
            false
        ));
        assert!(!solution_answer_matches(
            &expected,
            &ids(&["entity.knife", "entity.rope"]),
            false
        ));
        assert!(!solution_answer_matches(
            &expected,
            &ids(&["entity.knife", "entity.knife"]),
            false
        ));
        assert!(!solution_answer_matches(
            &ids(&["entity.knife", "entity.knife"]),
            &ids(&["entity.knife", "entity.knife"]),
            false
        ));
    }

    #[test]
    fn ordered_answers_require_the_exact_sequence() {
        let expected = ids(&["setting.shed", "setting.observatory"]);
        assert!(solution_answer_matches(&expected, &expected, true));
        assert!(!solution_answer_matches(
            &expected,
            &ids(&["setting.observatory", "setting.shed"]),
            true
        ));
    }
}

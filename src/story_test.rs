//! ADR-021 story test script shape: a JSON array of step objects living at
//! `scripts/<end_state>/<name>.json` (see [`crate::end_state::story_test_directory`]).
//!
//! This module only checks *structure* -- the shape a story test script must
//! have to be executable at all. It never resolves an id an `expect` block
//! names (a room, flag, fact, trigger, or end state) against the story's own
//! content; the engine that actually runs the script does that, against
//! compiled state this crate does not build.

use serde_json::Value;

/// One structural problem found in a story test script. `pointer` is a JSON
/// Pointer into the script's own JSON document (not a story YAML file), so
/// it is always relative to the script, e.g. `/2/expect/end_state` for the
/// third step's `expect.end_state`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoryTestProblem {
    pub pointer: String,
    pub message: String,
}

/// The `expect` block's known assertion fields (narrator-backend#781):
/// `verdict`, `location`, `flags`, `clock_minutes`, `notebook`,
/// `fired_triggers`, and `end_state`. Any other key is invalid.
const KNOWN_EXPECT_FIELDS: &[&str] = &[
    "verdict",
    "location",
    "flags",
    "clock_minutes",
    "notebook",
    "fired_triggers",
    "end_state",
];

/// The step's own known fields: `actor`, exactly one of `action`/`solve`,
/// and an optional `expect`.
const KNOWN_STEP_FIELDS: &[&str] = &["actor", "action", "solve", "expect"];

/// Checks one story test script's shape: valid JSON, a top-level array of
/// step objects, each with a string `actor` and exactly one of
/// `action`/`solve`, and -- when present -- an `expect` block built only
/// from the known assertion fields, with `end_state` legal only on the
/// script's last step (a terminal end state can never be followed by
/// another step). Returns every problem found, not just the first, so an
/// author sees the whole shape violation at once.
pub fn validate_story_test_script(source: &str) -> Vec<StoryTestProblem> {
    let value: Value = match serde_json::from_str(source) {
        Ok(value) => value,
        Err(error) => {
            return vec![StoryTestProblem {
                pointer: String::new(),
                message: format!("invalid JSON: {error}"),
            }]
        }
    };
    let Some(steps) = value.as_array() else {
        return vec![StoryTestProblem {
            pointer: String::new(),
            message: "story test must be a JSON array of step objects".to_string(),
        }];
    };
    let mut problems = Vec::new();
    let last_index = steps.len().checked_sub(1);
    for (index, step) in steps.iter().enumerate() {
        let pointer = format!("/{index}");
        let Some(step) = step.as_object() else {
            problems.push(StoryTestProblem {
                pointer,
                message: "story test step must be a JSON object".to_string(),
            });
            continue;
        };
        for key in step.keys() {
            if !KNOWN_STEP_FIELDS.contains(&key.as_str()) {
                problems.push(StoryTestProblem {
                    pointer: format!("{pointer}/{key}"),
                    message: format!("`{key}` is not a recognized story test step field"),
                });
            }
        }
        if !step.get("actor").is_some_and(Value::is_string) {
            problems.push(StoryTestProblem {
                pointer: format!("{pointer}/actor"),
                message: "story test step is missing a string `actor`".to_string(),
            });
        }
        let has_action = step.contains_key("action");
        let has_solve = step.contains_key("solve");
        if has_action == has_solve {
            problems.push(StoryTestProblem {
                pointer: pointer.clone(),
                message: "story test step must have exactly one of `action` or `solve`".to_string(),
            });
        }
        if let Some(expect) = step.get("expect") {
            validate_expect(expect, &pointer, last_index == Some(index), &mut problems);
        }
    }
    problems
}

fn validate_expect(
    expect: &Value,
    step_pointer: &str,
    is_last_step: bool,
    problems: &mut Vec<StoryTestProblem>,
) {
    let pointer = format!("{step_pointer}/expect");
    match expect {
        // A bare string is shorthand for `{"verdict": <string>}`.
        Value::String(_) => {}
        Value::Object(fields) => {
            // `{"rejected": "<code>"}` is shorthand for
            // `{"verdict": {"rejected": "<code>"}}`. A `rejected` key mixed
            // with anything else is not the shorthand, so it falls through
            // to the unknown-field check below like any other stray key.
            if fields.len() == 1 && fields.contains_key("rejected") {
                return;
            }
            for key in fields.keys() {
                if !KNOWN_EXPECT_FIELDS.contains(&key.as_str()) {
                    problems.push(StoryTestProblem {
                        pointer: format!("{pointer}/{key}"),
                        message: format!("`{key}` is not a recognized `expect` field"),
                    });
                }
            }
            if !is_last_step && fields.contains_key("end_state") {
                problems.push(StoryTestProblem {
                    pointer: format!("{pointer}/end_state"),
                    message: "`end_state` is only valid on a story test's last step".to_string(),
                });
            }
        }
        _ => {
            problems.push(StoryTestProblem {
                pointer,
                message: "`expect` must be a string or an object".to_string(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_array_has_no_problems() {
        assert!(validate_story_test_script("[]").is_empty());
    }

    #[test]
    fn well_formed_steps_have_no_problems() {
        let source = r#"[
            {"actor": "player.1", "action": [[]], "expect": "accepted"},
            {"actor": "player.1", "solve": {"sequence": 1, "grid": [], "expect": "accepted"},
             "expect": {"rejected": "not_ready"}},
            {"actor": "player.1", "action": [[]],
             "expect": {"verdict": "accepted", "clock_minutes": 30, "end_state": "end_state.solved"}}
        ]"#;
        assert!(validate_story_test_script(source).is_empty());
    }

    #[test]
    fn non_array_root_is_invalid() {
        let problems = validate_story_test_script(r#"{"actor": "player.1"}"#);
        assert_eq!(problems.len(), 1);
        assert!(problems[0].message.contains("JSON array"));
    }

    #[test]
    fn invalid_json_is_invalid() {
        let problems = validate_story_test_script("not json");
        assert_eq!(problems.len(), 1);
        assert!(problems[0].message.contains("invalid JSON"));
    }

    #[test]
    fn missing_actor_is_invalid() {
        let problems = validate_story_test_script(r#"[{"action": [[]]}]"#);
        assert!(problems.iter().any(|problem| problem.pointer == "/0/actor"));
    }

    #[test]
    fn neither_action_nor_solve_is_invalid() {
        let problems = validate_story_test_script(r#"[{"actor": "player.1"}]"#);
        assert!(problems.iter().any(|problem| problem.pointer == "/0"));
    }

    #[test]
    fn both_action_and_solve_is_invalid() {
        let source = r#"[{"actor": "player.1", "action": [[]], "solve": {"sequence": 1, "grid": [], "expect": "accepted"}}]"#;
        let problems = validate_story_test_script(source);
        assert!(problems.iter().any(|problem| problem.pointer == "/0"));
    }

    #[test]
    fn unknown_expect_field_is_invalid() {
        let problems = validate_story_test_script(
            r#"[{"actor": "player.1", "action": [[]], "expect": {"not_a_field": true}}]"#,
        );
        assert!(problems
            .iter()
            .any(|problem| problem.pointer == "/0/expect/not_a_field"));
    }

    #[test]
    fn end_state_on_non_final_step_is_invalid() {
        let source = r#"[
            {"actor": "player.1", "action": [[]], "expect": {"end_state": "end_state.solved"}},
            {"actor": "player.1", "action": [[]]}
        ]"#;
        let problems = validate_story_test_script(source);
        assert!(problems
            .iter()
            .any(|problem| problem.pointer == "/0/expect/end_state"));
    }

    #[test]
    fn end_state_on_final_step_is_valid() {
        let source = r#"[{"actor": "player.1", "action": [[]], "expect": {"end_state": "end_state.solved"}}]"#;
        assert!(validate_story_test_script(source).is_empty());
    }

    #[test]
    fn unknown_step_field_is_invalid() {
        let problems =
            validate_story_test_script(r#"[{"actor": "player.1", "action": [[]], "bogus": true}]"#);
        assert!(problems.iter().any(|problem| problem.pointer == "/0/bogus"));
    }
}

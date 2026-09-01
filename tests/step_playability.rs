//! Format 3.7 multi-step Solve playability: per-step answer reachability.
//!
//! Mirrors `tests/validation.rs`'s `story_files` splitting so these
//! synthetic single-string fixtures land in the canonical per-section
//! filenames the validator expects.

use std::collections::BTreeMap;

use narrator_validator::{validate, PlayabilityStatus, SourceFile};
use serde_yaml::{Mapping, Value};

fn story_files(source: String) -> Vec<SourceFile> {
    let Ok(Value::Mapping(root)) = serde_yaml::from_str::<Value>(&source) else {
        return vec![SourceFile {
            path: "story.yaml".to_string(),
            source,
        }];
    };
    let mut documents: BTreeMap<&str, Mapping> = BTreeMap::new();
    for (key, value) in root {
        let path = match key.as_str() {
            Some("case" | "solution") => "case.yaml",
            Some("settings" | "routes") => "settings.yaml",
            Some("win_states") => "win_states.yaml",
            Some("end_states") => "end_states.yaml",
            Some("characters") => "characters.yaml",
            Some("entities") => "entities.yaml",
            Some("events") => "events.yaml",
            Some("clues") => "clues.yaml",
            Some("facts") => "story.yaml",
            Some("deductions") => "deductions.yaml",
            Some("tags") => "tags.yaml",
            Some("flags") => "flags.yaml",
            Some("commands") => "commands.yaml",
            Some("triggers") => "triggers.yaml",
            Some("cards") => "deck.yaml",
            Some("command_costs") => "costs.yaml",
            _ => "story.yaml",
        };
        documents.entry(path).or_default().insert(key, value);
    }
    documents
        .into_iter()
        .map(|(path, document)| SourceFile {
            path: path.to_string(),
            source: serde_yaml::to_string(&Value::Mapping(document))
                .expect("test story serializes as YAML"),
        })
        .collect()
}

fn report(source: impl Into<String>) -> narrator_validator::ValidationReport {
    validate(&story_files(source.into()))
}

/// The default auto-facts/auto-deductions notebook policy, the one
/// `PlayabilityReport::terminal_paths` mirrors.
fn default_policy(
    report: &narrator_validator::ValidationReport,
) -> narrator_validator::NotebookPolicyAnalysis {
    report
        .playability
        .as_ref()
        .expect("format 3.7 story carries a playability report")
        .notebook_policies
        .iter()
        .find(|policy| policy.auto_facts && policy.auto_deductions)
        .expect("default auto-facts/auto-deductions notebook policy")
        .clone()
}

const BASE: &str = r#"
case:
  id: case.example
  format_version: "3.7.0"
  ruleset:
    id: ruleset.standard_mystery
    version: "7.0.0"
  players:
    min: 1
    max: 4
  initial_time: "21:00"
  entry_settings: [setting.foyer]
  exit_settings: [setting.foyer]
solution:
  max_attempts: 2
  steps:
    - id: step.name_motive
      prompt: What was the motive?
      time_cost_minutes: 0
      rows:
        - match: n_of_m
          n: 1
          cards: [answer.motive.jealousy]
      on_success:
        effects:
          - operation: set_flag
            flag: flag.motive_named
            value: true
      on_failure:
        points: -1
end_states:
  - id: end.solved
    name: Solved
    outcome: won
    resolution: full
    requires: [flag.motive_named]
    text: You name the motive.
settings:
  - id: setting.world
    type: island
    navigable: false
    description: The world containing the playable rooms.
  - id: setting.foyer
    type: room
    description: The entry foyer.
    parent: setting.world
routes: []
characters: []
entities: []
events: []
deductions: []
flags:
  - id: flag.motive_named
    name: Motive named
    description: Whether the motive has been named.
    initial_state: false
cards:
  - tag_id: 0
    subject: setting.foyer
  - tag_id: 5
    subject: command.solve
  - tag_id: 2111
    subject: answer.motive.jealousy
"#;

fn base_story() -> String {
    BASE.to_string()
}

#[test]
fn step_demanding_an_unwitnessed_answer_is_unlearnable() {
    let report = report(base_story());
    assert!(report.valid, "{:#?}", report.diagnostics);
    let policy = default_policy(&report);

    let step = policy
        .step_answerability
        .iter()
        .find(|step| step.id == "step.name_motive")
        .expect("step.name_motive answerability");
    assert_eq!(step.status, PlayabilityStatus::NotProved, "{step:#?}");
    assert_eq!(
        step.blocker.as_ref().unwrap().code,
        "playability.unlearnable_answer"
    );

    let end = policy
        .terminal_paths
        .iter()
        .find(|end| end.id == "end.solved")
        .expect("end.solved terminal path");
    assert_eq!(end.status, PlayabilityStatus::NotProved, "{end:#?}");
    assert_eq!(
        end.blocker.as_ref().unwrap().code,
        "playability.unlearnable_answer"
    );
}

#[test]
fn step_demanding_a_witnessed_answer_is_proved() {
    let witnessed = base_story().replace(
        "  - id: setting.foyer\n    type: room\n    description: The entry foyer.\n    parent: setting.world",
        "  - id: setting.foyer\n    type: room\n    description: The entry foyer.\n    parent: setting.world\n    facts:\n      - id: fact.motive_hint\n        statement: A jealous rage seems to explain everything.\n        about: [answer.motive.jealousy]",
    );
    let report = report(witnessed);
    assert!(report.valid, "{:#?}", report.diagnostics);
    let policy = default_policy(&report);

    let step = policy
        .step_answerability
        .iter()
        .find(|step| step.id == "step.name_motive")
        .expect("step.name_motive answerability");
    assert_eq!(step.status, PlayabilityStatus::Proved, "{step:#?}");

    let end = policy
        .terminal_paths
        .iter()
        .find(|end| end.id == "end.solved")
        .expect("end.solved terminal path");
    assert_eq!(end.status, PlayabilityStatus::Proved, "{end:#?}");
}

#[test]
fn mutually_exclusive_time_windows_falsify_independent_reachability() {
    // Two rooms off the foyer with different, coprime-ish travel times (7
    // and 11 minutes), each holding a fact gated on an *exact* elapsed-time
    // match (`Predicate::TimeEqual`, i.e. `when.time.relation: at`). No
    // combination of trips summing only 7s and 11s ever lands on both 7
    // and 11 as the *current* elapsed total after a single move (the first
    // move commits to one parity and can never return to the other exact
    // value, since elapsed only increases) -- so each answer is
    // individually reachable, but never both in the same play-through. If
    // this comes back Proved, the implementation checked each answer's
    // reachability independently instead of searching one simultaneous,
    // sequenced play-through -- the unsound shortcut the ticket calls out.
    let story = base_story()
        .replace(
            "solution:\n  max_attempts: 2\n  steps:\n    - id: step.name_motive\n      prompt: What was the motive?\n      time_cost_minutes: 0\n      rows:\n        - match: n_of_m\n          n: 1\n          cards: [answer.motive.jealousy]\n      on_success:\n        effects:\n          - operation: set_flag\n            flag: flag.motive_named\n            value: true\n      on_failure:\n        points: -1",
            "solution:\n  max_attempts: 2\n  steps:\n    - id: step.name_motive\n      prompt: What was the motive?\n      time_cost_minutes: 0\n      rows:\n        - match: n_of_m\n          n: 1\n          cards: [answer.motive.jealousy]\n      on_success:\n        effects:\n          - operation: set_flag\n            flag: flag.motive_named\n            value: true\n    - id: step.name_time\n      prompt: When did it happen?\n      time_cost_minutes: 0\n      rows:\n        - match: n_of_m\n          n: 1\n          cards: [answer.time.night]\n      on_success:\n        effects:\n          - operation: set_flag\n            flag: flag.time_named\n            value: true",
        )
        .replace(
            "  requires: [flag.motive_named]",
            "  requires: [flag.motive_named, flag.time_named]",
        )
        .replace(
            "  - id: setting.foyer\n    type: room\n    description: The entry foyer.\n    parent: setting.world",
            "  - id: setting.foyer\n    type: room\n    description: The entry foyer.\n    parent: setting.world\n  - id: setting.den\n    type: room\n    description: A den seven minutes from the foyer.\n    parent: setting.world\n    facts:\n      - id: fact.motive_hint\n        statement: A jealous rage seems to explain everything.\n        about: [answer.motive.jealousy]\n        when:\n          all:\n            - time:\n                relation: at\n                value: \"21:07\"\n  - id: setting.attic\n    type: room\n    description: An attic eleven minutes from the foyer.\n    parent: setting.world\n    facts:\n      - id: fact.time_hint\n        statement: The clock in the attic stopped at the moment of the crime.\n        about: [answer.time.night]\n        when:\n          all:\n            - time:\n                relation: at\n                value: \"21:11\"",
        )
        .replace(
            "routes: []",
            "routes:\n  - id: route.foyer_den\n    from: setting.foyer\n    to: setting.den\n    bidirectional: true\n    travel_minutes: 7\n  - id: route.foyer_attic\n    from: setting.foyer\n    to: setting.attic\n    bidirectional: true\n    travel_minutes: 11",
        )
        .replace(
            "  - tag_id: 2111\n    subject: answer.motive.jealousy",
            "  - tag_id: 2111\n    subject: answer.motive.jealousy\n  - tag_id: 2097\n    subject: answer.time.night",
        )
        .replace(
            "    initial_state: false",
            "    initial_state: false\n  - id: flag.time_named\n    name: Time named\n    description: Whether the time has been named.\n    initial_state: false",
        );
    let report = report(story);
    assert!(report.valid, "{:#?}", report.diagnostics);
    let policy = default_policy(&report);
    let end = policy
        .terminal_paths
        .iter()
        .find(|end| end.id == "end.solved")
        .expect("end.solved terminal path");
    assert_eq!(
        end.status,
        PlayabilityStatus::NotProved,
        "the two facts' exact-time windows can never both be hit in one play-through: {end:#?}"
    );
}

/// Two steps, each with an `on_failure` flag; `end.gave_up` requires
/// *both* flags. Reaching that combination genuinely needs two separate
/// failed commits: fail step 1 (consumes one attempt, resets to step 1),
/// succeed step 1 on the retry (free), then fail step 2 (a second
/// attempt). With `max_attempts: 2` that fits; with `max_attempts: 1` the
/// second failure is never available (`solve_step_fail` is gated on
/// `attempts_used < max_attempts`), so the combination is unreachable.
fn two_failure_story(max_attempts: u32) -> String {
    base_story()
        .replace(
            "      on_failure:\n        points: -1",
            "      on_failure:\n        effects:\n          - operation: set_flag\n            flag: flag.step1_failed\n            value: true\n        points: -1",
        )
        .replace(
            "solution:\n  max_attempts: 2\n  steps:\n    - id: step.name_motive\n      prompt: What was the motive?\n      time_cost_minutes: 0\n      rows:\n        - match: n_of_m\n          n: 1\n          cards: [answer.motive.jealousy]\n      on_success:\n        effects:\n          - operation: set_flag\n            flag: flag.motive_named\n            value: true\n      on_failure:\n        effects:\n          - operation: set_flag\n            flag: flag.step1_failed\n            value: true\n        points: -1",
            "solution:\n  max_attempts: 2\n  steps:\n    - id: step.name_motive\n      prompt: What was the motive?\n      time_cost_minutes: 0\n      rows:\n        - match: n_of_m\n          n: 1\n          cards: [answer.motive.jealousy]\n      on_success:\n        effects:\n          - operation: set_flag\n            flag: flag.motive_named\n            value: true\n      on_failure:\n        effects:\n          - operation: set_flag\n            flag: flag.step1_failed\n            value: true\n        points: -1\n    - id: step.name_time\n      prompt: When did it happen?\n      time_cost_minutes: 0\n      rows:\n        - match: n_of_m\n          n: 1\n          cards: [answer.motive.jealousy]\n      on_failure:\n        effects:\n          - operation: set_flag\n            flag: flag.step2_failed\n            value: true\n        points: -1",
        )
        .replace(
            "end_states:\n  - id: end.solved",
            "end_states:\n  - id: end.gave_up\n    name: Gave up\n    outcome: lost\n    resolution: failure\n    requires: [flag.step1_failed, flag.step2_failed]\n    text: Two wrong names were spoken.\n  - id: end.solved",
        )
        .replace(
            "  - id: setting.foyer\n    type: room\n    description: The entry foyer.\n    parent: setting.world",
            "  - id: setting.foyer\n    type: room\n    description: The entry foyer.\n    parent: setting.world\n    facts:\n      - id: fact.motive_hint\n        statement: A jealous rage seems to explain everything.\n        about: [answer.motive.jealousy]",
        )
        .replace(
            "    initial_state: false",
            "    initial_state: false\n  - id: flag.step1_failed\n    name: Step one failed\n    description: Whether the first attempt at step one failed.\n    initial_state: false\n  - id: flag.step2_failed\n    name: Step two failed\n    description: Whether the second step failed.\n    initial_state: false",
        )
        // Substituted last so it can't accidentally shadow-match any of
        // the literal "max_attempts: 2" text above before those replaces run.
        .replacen("max_attempts: 2", &format!("max_attempts: {max_attempts}"), 1)
}

#[test]
fn on_failure_flag_combination_is_reachable_within_max_attempts_but_not_past_it() {
    let reachable_report = report(two_failure_story(2));
    assert!(
        reachable_report.valid,
        "{:#?}",
        reachable_report.diagnostics
    );
    let policy = default_policy(&reachable_report);
    let end = policy
        .terminal_paths
        .iter()
        .find(|end| end.id == "end.gave_up")
        .expect("end.gave_up terminal path");
    assert_eq!(end.status, PlayabilityStatus::Proved, "{end:#?}");

    let unreachable_report = report(two_failure_story(1));
    assert!(
        unreachable_report.valid,
        "{:#?}",
        unreachable_report.diagnostics
    );
    let policy = default_policy(&unreachable_report);
    let end = policy
        .terminal_paths
        .iter()
        .find(|end| end.id == "end.gave_up")
        .expect("end.gave_up terminal path");
    assert_ne!(
        end.status,
        PlayabilityStatus::Proved,
        "the second failure needs an attempt the first one already consumed: {end:#?}"
    );
}

#[test]
fn unrelated_unsupported_construct_demotes_an_otherwise_unreachable_step_answer_to_inconclusive() {
    // The card the step demands DOES have a witness (so it isn't the
    // static "hard unlearnable" tier), but the witness is gated behind an
    // unsupported `owns` predicate, so no supported action can ever reach
    // it -- an unrelated `unsupported_*` construct anywhere in the story
    // must demote this to Inconclusive, not NotProved.
    let story = base_story()
        .replace(
            "  - id: setting.foyer\n    type: room\n    description: The entry foyer.\n    parent: setting.world",
            "  - id: setting.foyer\n    type: room\n    description: The entry foyer.\n    parent: setting.world\n    facts:\n      - id: fact.motive_hint\n        statement: A jealous rage seems to explain everything.\n        about: [answer.motive.jealousy]\n        when:\n          all:\n            - owns: entity.diary",
        )
        .replace(
            "entities: []",
            "entities:\n  - id: entity.diary\n    description: A locked diary.",
        );
    let report = report(story);
    assert!(report.valid, "{:#?}", report.diagnostics);
    let policy = default_policy(&report);

    let step = policy
        .step_answerability
        .iter()
        .find(|step| step.id == "step.name_motive")
        .expect("step.name_motive answerability");
    assert_eq!(
        step.status,
        PlayabilityStatus::Inconclusive,
        "an unrelated unsupported construct must demote, not hard-fail: {step:#?}"
    );

    let end = policy
        .terminal_paths
        .iter()
        .find(|end| end.id == "end.solved")
        .expect("end.solved terminal path");
    assert_eq!(end.status, PlayabilityStatus::Inconclusive, "{end:#?}");
}

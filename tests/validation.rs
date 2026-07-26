use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use narrator_validator::{validate, Severity, SourceFile};

const VALID_STORY: &str = r#"
case:
  id: case.example
  format_version: 1
  entry_settings: [setting.foyer]
  exit_settings: [setting.foyer]
solution:
  victim: character.victim
  culprit: character.culprit
  weapon: entity.knife
  location: setting.study
settings:
  - id: setting.world
    type: island
  - id: setting.foyer
    type: room
    parent: setting.world
  - id: setting.study
    type: room
    parent: setting.world
routes:
  - id: route.foyer_study
    from: setting.foyer
    to: setting.study
    bidirectional: true
    travel_minutes: 1
characters:
  - id: character.victim
  - id: character.culprit
entities:
  - id: entity.knife
    initial:
      container: setting.study
events:
  - id: event.murder
    day: 0
    time: "21:18"
    duration_minutes: 0
    location: setting.study
    participants: [character.victim, character.culprit]
clues:
  - id: clue.weapon
    discover_by:
      target: entity.knife
deductions:
  - id: deduction.solution
    supported_by: [clue.weapon]
tags:
  - id: tag.evidence
    members: [entity.knife]
  - id: tag.gameplay
    members: [route.foyer_study, command.examine, trigger.examine_knife]
commands:
  - id: command.examine
    name: Examine
    aliases: [inspect]
    parameters:
      - name: target
        accepts: [entity]
        required: true
triggers:
  - id: trigger.examine_knife
    name: Examine the knife
    command: command.examine
    once: true
    conditions:
      - left: $target
        operator: equals
        right: entity.knife
    effects:
      - operation: discover
        target: clue.weapon
        value: visible
"#;

const VALID_FORMAT_2_STORY: &str = r#"
case:
  id: case.example
  format_version: 2
  entry_settings: [setting.foyer]
  exit_settings: [setting.foyer]
solution:
  victim: character.victim
  culprit: character.culprit
  weapon: entity.knife
  location: setting.study
  deduction: deduction.solution
settings:
  - id: setting.world
    type: island
  - id: setting.foyer
    type: room
    parent: setting.world
  - id: setting.study
    type: room
    parent: setting.world
routes:
  - id: route.foyer_study
    from: setting.foyer
    to: setting.study
    bidirectional: true
    travel_minutes: 1
characters:
  - id: character.victim
  - id: character.culprit
facts:
  - id: fact.knife_is_present
    statement: The knife is present.
  - id: fact.knife_has_blood
    statement: The knife carries the victim's blood.
    about: [entity.knife, character.victim]
    requires: tag.knife_analysis_complete
  - id: fact.knife_connects_to_scene
    statement: The knife connects the culprit to the study.
    requires: [fact.knife_is_present, entity.knife]
entities:
  - id: entity.knife
    initial:
      container: setting.study
events:
  - id: event.murder
    day: 0
    time: "21:18"
    duration_minutes: 0
    location: setting.study
    participants: [character.victim, character.culprit]
deductions:
  - id: deduction.solution
    conclusion: The knife was used in the study.
    inputs: [fact.knife_has_blood, fact.knife_connects_to_scene]
    truth: true
tags:
  - id: tag.evidence
    members: [entity.knife]
  - id: tag.knife_analysis_complete
    state: true
commands:
  - id: command.claim
    name: Claim
    parameters:
      - name: fact
        accepts: [fact]
        required: true
  - id: command.investigate
    name: Investigate
    parameters:
      - name: target
        accepts: [entity]
        required: true
triggers:
  - id: trigger.investigate_knife
    name: Investigate the knife
    command: command.investigate
    effects:
      - operation: give_after
        target: tag.knife_analysis_complete
        value: 20m
"#;

fn report(source: impl Into<String>) -> narrator_validator::ValidationReport {
    validate(&[SourceFile {
        path: "story.yaml".to_string(),
        source: source.into(),
    }])
}

fn codes(source: impl Into<String>) -> Vec<String> {
    report(source)
        .diagnostics
        .into_iter()
        .map(|diagnostic| diagnostic.code)
        .collect()
}

fn story_with_facts() -> String {
    VALID_STORY
        .replace(
            "  location: setting.study\nsettings:",
            "  location: setting.study\n  deduction: deduction.solution\nsettings:",
        )
        .replace(
            "  - id: character.culprit\nentities:",
            "  - id: character.culprit\n    knowledge:\n      - fact: fact.knife_has_blood\n        source: entity.knife\nfacts:\n  - id: fact.knife_has_blood\n    statement: The knife carries the victim's blood.\n    about: [entity.knife, character.victim]\n    sources: [entity.knife, clue.weapon, command.examine, trigger.examine_knife]\n    initially_known: false\n  - id: fact.knife_was_at_scene\n    statement: The knife was in the study.\n    about: [entity.knife, setting.study]\n    sources: [entity.knife]\nentities:",
        )
        .replace(
            "    initial:\n      container: setting.study",
            "    initial:\n      container: setting.study\n    facts:\n      examined: [fact.knife_has_blood, fact.knife_was_at_scene]",
        )
        .replace(
            "    discover_by:\n      target: entity.knife",
            "    discover_by:\n      target: entity.knife\n    establishes: [fact.knife_has_blood]",
        )
        .replace(
            "  - id: deduction.solution\n    supported_by: [clue.weapon]",
            "  - id: deduction.solution\n    conclusion: The knife was used in the study.\n    supported_by: [clue.weapon]\n    inputs: [fact.knife_has_blood, fact.knife_was_at_scene]\n    truth: true",
        )
        .replace(
            "      - operation: discover\n        target: clue.weapon\n        value: visible",
            "      - operation: discover\n        target: clue.weapon\n        value: visible\n      - operation: learn_after\n        target: fact.knife_has_blood\n        value: 20m",
        )
}

#[test]
fn valid_repository_has_no_diagnostics() {
    let report = report(VALID_STORY);
    assert!(report.valid, "{:#?}", report.diagnostics);
    assert_eq!(report.format_version, Some(1));
    assert!(report.diagnostics.is_empty());
}

#[test]
fn valid_format_2_repository_has_no_diagnostics() {
    let report = report(VALID_FORMAT_2_STORY);
    assert!(report.valid, "{:#?}", report.diagnostics);
    assert_eq!(report.format_version, Some(2));
    assert!(report.diagnostics.is_empty());
}

#[test]
fn format_2_requires_facts_and_rejects_clues() {
    let without_facts = VALID_FORMAT_2_STORY.replace("facts:", "unused_facts:");
    assert!(codes(without_facts).contains(&"schema.missing_section".to_string()));

    let with_clues =
        VALID_FORMAT_2_STORY.replace("deductions:", "clues:\n  - id: clue.legacy\ndeductions:");
    assert!(codes(with_clues).contains(&"format.clues_removed".to_string()));
}

#[test]
fn format_2_validates_fact_requirements_and_cycles() {
    let malformed = VALID_FORMAT_2_STORY
        .replace("requires: tag.knife_analysis_complete", "requires: []")
        .replace(
            "requires: [fact.knife_is_present, entity.knife]",
            "requires: [fact.knife_is_present, case.example]",
        );
    let result = codes(malformed);
    assert!(result.contains(&"fact.requires_type".to_string()));
    assert!(result.contains(&"reference.wrong_type".to_string()));

    let cyclic = VALID_FORMAT_2_STORY.replace(
        "statement: The knife is present.",
        "statement: The knife is present.\n    requires: fact.knife_connects_to_scene",
    );
    assert!(codes(cyclic).contains(&"fact.requirement_cycle".to_string()));
}

#[test]
fn format_2_enforces_unclaimed_opening_facts_and_central_requirements() {
    let source = VALID_FORMAT_2_STORY
        .replace(
            "statement: The knife is present.",
            "statement: The knife is present.\n    initially_known: true",
        )
        .replace(
            "    initial:\n      container: setting.study",
            "    initial:\n      container: setting.study\n    facts: [fact.knife_is_present]",
        )
        .replace(
            "    inputs: [fact.knife_has_blood, fact.knife_connects_to_scene]",
            "    inputs: [fact.knife_has_blood, fact.knife_connects_to_scene]\n    supported_by: [clue.legacy]",
        );
    let result = codes(source);
    assert!(result.contains(&"fact.initially_known_removed".to_string()));
    assert!(result.contains(&"fact.associations_removed".to_string()));
    assert!(result.contains(&"deduction.supported_by_removed".to_string()));
}

#[test]
fn format_2_requires_a_fact_accepting_claim_command() {
    let missing = VALID_FORMAT_2_STORY.replace("command.claim", "command.accept");
    assert!(codes(missing).contains(&"fact.claim_command_missing".to_string()));

    let invalid = VALID_FORMAT_2_STORY.replace("accepts: [fact]", "accepts: [entity]");
    assert!(codes(invalid).contains(&"fact.claim_command_invalid".to_string()));
}

#[test]
fn format_2_validates_state_tags_and_delayed_give_effects() {
    let invalid = VALID_FORMAT_2_STORY
        .replace("state: true", "state: sometimes")
        .replace(
            "target: tag.knife_analysis_complete\n        value: 20m",
            "target: entity.knife\n        value: 0m",
        );
    let result = codes(invalid);
    assert!(result.contains(&"tag.state_type".to_string()));
    assert!(result.contains(&"trigger.effect_target_type".to_string()));
    assert!(result.contains(&"trigger.effect_delay_invalid".to_string()));

    let legacy = VALID_FORMAT_2_STORY.replace("operation: give_after", "operation: learn_after");
    assert!(codes(legacy).contains(&"trigger.legacy_knowledge_effect".to_string()));
}

#[test]
fn reports_yaml_errors_with_a_source_position() {
    let report = report("case: [unterminated\n");
    assert!(!report.valid);
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|item| item.code == "yaml.invalid")
        .expect("syntax diagnostic");
    assert_eq!(diagnostic.path, "story.yaml");
    assert!(diagnostic.range.is_some());
}

#[test]
fn reports_unknown_and_wrong_type_references() {
    let source = VALID_STORY
        .replace("members: [entity.knife]", "members: [not_an_id]")
        .replace("location: setting.study", "location: entity.knife");
    let report = report(source);
    assert!(!report.valid);
    assert!(report
        .diagnostics
        .iter()
        .any(|item| item.code == "reference.unknown"
            && item.subject_id.as_deref() == Some("not_an_id")));
    assert!(report
        .diagnostics
        .iter()
        .any(|item| item.code == "reference.wrong_type"
            && item.subject_id.as_deref() == Some("entity.knife")));
}

#[test]
fn reports_duplicate_ids_with_the_original_location() {
    let source = VALID_STORY.replace(
        "character.culprit\nentities:",
        "character.victim\nentities:",
    );
    let report = report(source);
    let duplicate = report
        .diagnostics
        .iter()
        .find(|item| item.code == "id.duplicate")
        .expect("duplicate diagnostic");
    assert_eq!(duplicate.related.len(), 1);
    assert!(duplicate.related[0]
        .pointer
        .as_deref()
        .unwrap()
        .ends_with("/id"));
}

#[test]
fn reports_parent_and_dependency_cycles() {
    let source = VALID_STORY
        .replace(
            "parent: setting.world\n  - id: setting.study",
            "parent: setting.study\n  - id: setting.study",
        )
        .replace(
            "parent: setting.world\nroutes:",
            "parent: setting.foyer\nroutes:",
        )
        .replace(
            "supported_by: [clue.weapon]",
            "supported_by: [clue.weapon]\n    requires: [deduction.solution]",
        );
    let result = codes(source);
    assert!(result.contains(&"setting.parent_cycle".to_string()));
    assert!(result.contains(&"deduction.dependency_cycle".to_string()));
}

#[test]
fn reports_unreachable_and_inescapable_settings() {
    let source = VALID_STORY.replace(
        "routes:\n  - id: route.foyer_study\n    from: setting.foyer\n    to: setting.study\n    bidirectional: true\n    travel_minutes: 1",
        "routes: []",
    );
    let result = codes(source);
    assert!(result.contains(&"navigation.unreachable".to_string()));
    assert!(result.contains(&"navigation.no_exit".to_string()));
}

#[test]
fn explicit_navigation_contract_cannot_be_empty() {
    let source = VALID_STORY
        .replace("entry_settings: [setting.foyer]", "entry_settings: []")
        .replace("exit_settings: [setting.foyer]", "exit_settings: []");
    let result = codes(source);
    assert!(result.contains(&"navigation.entry_missing".to_string()));
    assert!(result.contains(&"navigation.exit_missing".to_string()));
    assert!(!result.contains(&"navigation.implicit_contract".to_string()));
}

#[test]
fn validates_event_and_route_values() {
    let source = VALID_STORY
        .replace("travel_minutes: 1", "travel_minutes: 0")
        .replace("day: 0", "day: -1")
        .replace("time: \"21:18\"", "time: \"25:90\"")
        .replace("duration_minutes: 0", "duration_minutes: -2");
    let result = codes(source);
    assert!(result.contains(&"route.invalid_travel_minutes".to_string()));
    assert!(result.contains(&"event.invalid_day".to_string()));
    assert!(result.contains(&"event.invalid_time".to_string()));
    assert!(result.contains(&"event.invalid_duration".to_string()));
}

#[test]
fn validates_required_reference_shapes() {
    let source = VALID_STORY
        .replace("  victim: character.victim\n", "")
        .replace("    from: setting.foyer", "    from: 42")
        .replace(
            "    participants: [character.victim, character.culprit]",
            "    participants: character.victim",
        )
        .replace("    members: [entity.knife]", "    members: entity.knife");
    let result = codes(source);
    assert!(result.contains(&"solution.missing_reference".to_string()));
    assert!(result.contains(&"route.missing_endpoint".to_string()));
    assert!(result.contains(&"event.participants_type".to_string()));
    assert!(result.contains(&"tag.members_type".to_string()));
}

#[test]
fn validates_command_and_trigger_shapes() {
    let source = VALID_STORY
        .replace("aliases: [inspect]", "aliases: inspect")
        .replace("accepts: [entity]", "accepts: []")
        .replace("required: true", "required: sometimes")
        .replace("once: true", "once: sometimes")
        .replace("operator: equals", "operator: \"\"")
        .replace("value: visible", "value: 42");
    let result = codes(source);
    assert!(result.contains(&"command.aliases_type".to_string()));
    assert!(result.contains(&"command.parameter_accepts".to_string()));
    assert!(result.contains(&"command.parameter_required_type".to_string()));
    assert!(result.contains(&"trigger.once_type".to_string()));
    assert!(result.contains(&"trigger.condition_field".to_string()));
    assert!(result.contains(&"trigger.effect_value_type".to_string()));
}

#[test]
fn validates_trigger_command_reference_type() {
    let source = VALID_STORY.replace("command: command.examine", "command: entity.knife");
    let report = report(source);
    assert!(!report.valid);
    assert!(report.diagnostics.iter().any(|item| {
        item.code == "reference.wrong_type"
            && item.subject_id.as_deref() == Some("entity.knife")
            && item.pointer.as_deref() == Some("/triggers/0/command")
    }));
}

#[test]
fn validates_optional_facts_and_deduction_inputs() {
    let source = story_with_facts();

    let report = report(source.clone());
    assert!(report.valid, "{:#?}", report.diagnostics);

    let malformed = source
        .replace(
            "statement: The knife carries the victim's blood.",
            "statement: \"\"",
        )
        .replace("initially_known: false", "initially_known: sometimes")
        .replace(
            "inputs: [fact.knife_has_blood, fact.knife_was_at_scene]",
            "inputs: [fact.knife_has_blood]",
        )
        .replace("truth: true", "truth: perhaps");
    let result = codes(malformed);
    assert!(result.contains(&"fact.missing_statement".to_string()));
    assert!(result.contains(&"fact.initially_known_type".to_string()));
    assert!(result.contains(&"deduction.inputs_type".to_string()));
    assert!(result.contains(&"deduction.truth_type".to_string()));
}

#[test]
fn validates_fact_reference_types() {
    let source = story_with_facts().replace("fact: fact.knife_has_blood", "fact: entity.knife");
    let report = report(source);
    assert!(!report.valid);
    assert!(report.diagnostics.iter().any(|item| {
        item.code == "reference.wrong_type"
            && item.subject_id.as_deref() == Some("entity.knife")
            && item
                .pointer
                .as_deref()
                .is_some_and(|pointer| pointer.ends_with("/knowledge/0/fact"))
    }));
}

#[test]
fn legacy_character_knowledge_and_clue_prose_remain_valid_without_facts() {
    let source = VALID_STORY
        .replace(
            "  - id: character.culprit\nentities:",
            "  - id: character.culprit\n    knowledge:\n      - fact: The knife was left in the study.\n        source: entity.knife\nentities:",
        )
        .replace(
            "    discover_by:\n      target: entity.knife",
            "    discover_by:\n      target: entity.knife\n    establishes: [The knife was used in the murder.]",
        );
    let report = report(source);
    assert!(report.valid, "{:#?}", report.diagnostics);
}

#[test]
fn validates_fact_associations_and_clue_fact_references() {
    let source = story_with_facts()
        .replace(
            "examined: [fact.knife_has_blood, fact.knife_was_at_scene]",
            "examined: entity.knife",
        )
        .replace(
            "establishes: [fact.knife_has_blood]",
            "establishes: [entity.knife]",
        );
    let result = codes(source);
    assert!(result.contains(&"fact.association_type".to_string()));
    assert!(result.contains(&"reference.wrong_type".to_string()));
}

#[test]
fn validates_learning_effect_targets_and_delays() {
    let source = story_with_facts()
        .replace(
            "operation: discover\n        target: clue.weapon",
            "operation: discover\n        target: fact.knife_has_blood",
        )
        .replace(
            "operation: learn_after\n        target: fact.knife_has_blood\n        value: 20m",
            "operation: learn_after\n        target: clue.weapon\n        value: 0m",
        );
    let result = codes(source);
    assert!(result.contains(&"trigger.effect_target_type".to_string()));
    assert!(result.contains(&"trigger.effect_delay_invalid".to_string()));

    let missing_delay = story_with_facts().replace("        value: 20m\n", "");
    assert!(codes(missing_delay).contains(&"trigger.effect_delay_missing".to_string()));
}

#[test]
fn detects_deduction_cycles_through_inputs() {
    let source = story_with_facts()
        .replace(
            "inputs: [fact.knife_has_blood, fact.knife_was_at_scene]",
            "inputs: [deduction.loop, fact.knife_was_at_scene]",
        )
        .replace(
            "tags:",
            "  - id: deduction.loop\n    conclusion: The reasoning loops back on itself.\n    inputs: [deduction.solution, fact.knife_has_blood]\n    truth: false\n    contradicted_by: [fact.knife_was_at_scene]\ntags:",
        );
    assert!(codes(source).contains(&"deduction.dependency_cycle".to_string()));
}

#[test]
fn warns_for_unreachable_facts_and_unrefutable_false_deductions() {
    let source = story_with_facts()
        .replace(
            "entities:",
            "  - id: fact.orphan\n    statement: This fact cannot be learned.\n    sources: [entity.knife]\nentities:",
        )
        .replace("truth: true", "truth: false");
    let report = report(source);
    assert!(report.valid, "{:#?}", report.diagnostics);
    assert!(report
        .diagnostics
        .iter()
        .any(|item| item.code == "fact.unreachable"));
    assert!(report
        .diagnostics
        .iter()
        .any(|item| item.code == "deduction.false_without_contradiction"));
}

#[test]
fn validates_deduction_solution_shape() {
    let source = story_with_facts().replace(
        "truth: true",
        "truth: true\n    solves:\n      culprit: character.culprit\n      weapon: entity.knife\n      location: setting.study\n      time: \"29:00\"",
    );
    assert!(codes(source).contains(&"deduction.solves_invalid_time".to_string()));
}

#[test]
fn commands_and_triggers_remain_optional() {
    let without_gameplay_tag = VALID_STORY.replace(
        "  - id: tag.gameplay\n    members: [route.foyer_study, command.examine, trigger.examine_knife]\n",
        "",
    );
    let source = without_gameplay_tag
        .split("commands:\n")
        .next()
        .expect("story before optional sections");
    let report = report(source);
    assert!(report.valid, "{:#?}", report.diagnostics);
}

#[test]
fn rejects_yaml_aliases_in_sequence_items() {
    let source = VALID_STORY.replace(
        "  - id: character.victim",
        "  - &victim\n    id: character.victim",
    );
    let result = codes(source);
    assert!(result.contains(&"yaml.alias_unsupported".to_string()));
}

#[test]
fn ignores_id_shaped_text_inside_prose() {
    let source = VALID_STORY.replace(
        "type: room\n    parent: setting.world",
        "type: room\n    description: Ask character.not_real about it.\n    parent: setting.world",
    );
    assert!(!codes(source).contains(&"reference.unknown".to_string()));
}

#[test]
fn warnings_do_not_invalidate_a_repository() {
    let source = VALID_STORY
        .replace("  format_version: 1\n", "")
        .replace("  entry_settings: [setting.foyer]\n", "")
        .replace("  exit_settings: [setting.foyer]\n", "");
    let report = report(source);
    assert!(report.valid, "{:#?}", report.diagnostics);
    assert!(report
        .diagnostics
        .iter()
        .all(|item| item.severity == Severity::Warning));
}

#[test]
fn github_cli_emits_annotations_and_failure_status() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "narrator-validator-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir(&directory).unwrap();
    fs::write(
        directory.join("story.yaml"),
        VALID_STORY.replace("entity.knife]", "entity.missing]"),
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_narrator-validator"))
        .args(["--format", "github", directory.to_str().unwrap()])
        .output()
        .unwrap();
    fs::remove_dir_all(directory).unwrap();

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("::error file=story.yaml"));
    assert!(stdout.contains("title=reference.unknown"));
}

#[test]
fn github_cli_supports_repository_level_annotations() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "narrator-validator-empty-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("story.yaml"), "{}\n").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_narrator-validator"))
        .args(["--format", "github", directory.to_str().unwrap()])
        .output()
        .unwrap();
    fs::remove_dir_all(directory).unwrap();

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("::error title=schema.missing_section::"));
    assert!(!stdout.contains("file=,"));
}

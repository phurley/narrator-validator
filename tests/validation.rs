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

#[test]
fn valid_repository_has_no_diagnostics() {
    let report = report(VALID_STORY);
    assert!(report.valid, "{:#?}", report.diagnostics);
    assert_eq!(report.format_version, Some(1));
    assert!(report.diagnostics.is_empty());
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

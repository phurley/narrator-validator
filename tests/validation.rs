use std::collections::BTreeMap;
use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use narrator_validator::{validate, Severity, SourceFile};
use serde_yaml::{Mapping, Value};

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
flags:
  - id: flag.knife_examined
    name: Knife examined
    description: Whether the knife has been examined.
    initial_state: false
commands:
  - id: command.examine
    name: Examine
    description: Inspect an entity.
    parameters:
      - name: target
        type: entity
        required: true
    effects: []
triggers:
  - id: trigger.examine_knife
    name: Examine the knife
    command: command.examine
    once: true
    time:
      relation: at
      value: "21:18"
    location: setting.study
    any_of: [character.culprit, entity.knife]
    all_of: [flag.knife_examined]
    effects:
      - operation: discover
        target: clue.weapon
        value: visible
"#;

const VALID_FORMAT_2_STORY: &str = r#"
case:
  id: case.example
  format_version: 2
  initial_time: "21:00"
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
entities:
  - id: entity.knife
    initial:
      container: setting.study
    facts:
      - id: fact.knife_is_present
        statement: The knife is present.
      - id: fact.knife_has_blood
        statement: The knife carries the victim's blood.
        about: [entity.knife, character.victim]
        requires: flag.knife_analysis_complete
      - id: fact.knife_connects_to_scene
        statement: The knife connects the culprit to the study.
        requires: [fact.knife_is_present, entity.knife]
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
flags:
  - id: flag.knife_examined
    name: Knife examined
    description: Whether the knife has been examined.
    initial_state: false
  - id: flag.knife_analysis_complete
    name: Knife analysis complete
    description: Whether the delayed knife analysis has completed.
    initial_state: false
commands:
  - id: command.claim
    name: Claim
    description: Learn that the knife is present.
    effects:
      - operation: learn_fact
        fact_id: fact.knife_is_present
  - id: command.investigate
    name: Investigate
    description: Resolve the authored effects of investigating an entity.
    parameters:
      - name: target
        type: entity
        required: true
      - name: destination
        type: setting
        required: false
      - name: companion
        type: character
        required: false
      - name: conclusion
        type: deduction
        required: false
      - name: incident
        type: event
        required: false
    effects:
      - operation: advance_time
        minutes: 12
      - operation: move
        subjects: [player, character.culprit, param1, param3]
        setting: param2
      - operation: transform
        entity_from: param1
        entity_to: entity.knife
      - operation: learn_fact
        fact_id: fact.knife_has_blood
      - operation: establish_deduction
        deduction_id: param4
      - operation: describe
        text: The examination reveals a carefully staged scene.
      - operation: trigger
        trigger_id: trigger.investigate_knife
      - operation: win
        text: The player has solved the mystery.
      - operation: lose
        text: The trail has gone cold.
triggers:
  - id: trigger.investigate_knife
    name: Investigate the knife
    command: command.investigate
    effects:
      - operation: give_after
        target: flag.knife_analysis_complete
        value: 20m
"#;

fn report(source: impl Into<String>) -> narrator_validator::ValidationReport {
    validate(&story_files(source.into()))
}

fn codes(source: impl Into<String>) -> Vec<String> {
    report(source)
        .diagnostics
        .into_iter()
        .map(|diagnostic| diagnostic.code)
        .collect()
}

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
            Some("case" | "solution" | "settings" | "routes") => "settings.yaml",
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
fn format_2_requires_a_runtime_clock_and_whole_minute_effects() {
    let missing = report(VALID_FORMAT_2_STORY.replace("  initial_time: \"21:00\"\n", ""));
    assert!(missing
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "case.initial_time_missing"));

    let invalid = codes(
        VALID_FORMAT_2_STORY
            .replace("initial_time: \"21:00\"", "initial_time: \"24:00\"")
            .replace("minutes: 12", "minutes: 12.5"),
    );
    assert!(invalid.contains(&"case.initial_time".to_string()));
    assert!(invalid.contains(&"command.effect_minutes".to_string()));
}

#[test]
fn validates_runtime_trigger_effect_contracts() {
    let invalid = VALID_FORMAT_2_STORY
        .replace(
            "    command: command.investigate\n    effects:",
            "    command: command.investigate\n    time:\n      relation: after\n      value: \"99:00\"\n    effects:",
        )
        .replace(
            "      - operation: give_after\n        target: flag.knife_analysis_complete\n        value: 20m",
            "      - operation: move\n        target: $actor\n        value: $target\n        surprise: true\n      - operation: rewrite_reality\n        target: flag.knife_analysis_complete",
        );
    let result = codes(invalid);
    assert!(result.contains(&"trigger.time_value".to_string()));
    assert!(result.contains(&"trigger.effect_parameter_type".to_string()));
    assert!(result.contains(&"trigger.effect_unknown_field".to_string()));
    assert!(result.contains(&"trigger.effect_unknown_operation".to_string()));

    let missing_value =
        codes(VALID_FORMAT_2_STORY.replace("        value: 20m", "        omitted_value: 20m"));
    assert!(missing_value.contains(&"trigger.effect_value_missing".to_string()));
}

#[test]
fn rejects_removed_tags_section_and_action_effects() {
    let source = VALID_FORMAT_2_STORY
        .replace(
            "flags:",
            "tags:\n  - id: tag.legacy\n    members: [entity.knife]\nflags:",
        )
        .replace(
            "      - operation: describe",
            "      - operation: add_tag\n        tag_id: tag.legacy\n      - operation: describe",
        );
    let result = codes(source);
    assert!(result.contains(&"format.tags_removed".to_string()));
    assert!(result.contains(&"command.effect_unknown_operation".to_string()));
}

#[test]
fn rejects_sections_outside_their_canonical_files() {
    let mut files = story_files(VALID_STORY.to_string());
    files
        .iter_mut()
        .find(|file| file.path == "characters.yaml")
        .expect("character document")
        .path = "cast.yaml".to_string();

    let report = validate(&files);

    assert!(!report.valid);
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "schema.noncanonical_filename"
            && diagnostic.path == "cast.yaml"
            && diagnostic.pointer.as_deref() == Some("/characters")
            && diagnostic.message.contains("`characters.yaml`")
    }));
}

#[test]
fn rejects_flags_outside_flags_yaml() {
    let mut files = story_files(VALID_STORY.to_string());
    files
        .iter_mut()
        .find(|file| file.path == "flags.yaml")
        .expect("flag document")
        .path = "state.yaml".to_string();

    let report = validate(&files);

    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "schema.noncanonical_filename"
            && diagnostic.path == "state.yaml"
            && diagnostic.pointer.as_deref() == Some("/flags")
            && diagnostic.message.contains("`flags.yaml`")
    }));
}

#[test]
fn validates_required_flag_fields() {
    let source = VALID_STORY
        .replacen("  - id: flag.knife_examined", "  - id: \"\"", 1)
        .replace("    name: Knife examined", "    name: \"\"")
        .replace(
            "    description: Whether the knife has been examined.",
            "    description: 42",
        )
        .replace("    initial_state: false", "    initial_state: disabled");
    let result = codes(source);

    assert!(result.contains(&"id.invalid".to_string()));
    assert!(result.contains(&"flag.name".to_string()));
    assert!(result.contains(&"flag.description".to_string()));
    assert!(result.contains(&"flag.initial_state".to_string()));
}

#[test]
fn flags_section_is_required() {
    let source = VALID_STORY.replace(
        "flags:\n  - id: flag.knife_examined\n    name: Knife examined\n    description: Whether the knife has been examined.\n    initial_state: false\n",
        "",
    );
    let report = report(source);

    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "schema.missing_section"
            && diagnostic.pointer.as_deref() == Some("/flags")
    }));
}

#[test]
fn format_2_nests_facts_and_rejects_top_level_facts_and_clues() {
    let with_top_level_facts = VALID_FORMAT_2_STORY.replace(
        "entities:",
        "facts:\n  - id: fact.legacy\n    statement: This no longer belongs at the root.\nentities:",
    );
    assert!(codes(with_top_level_facts).contains(&"format.facts_section_removed".to_string()));

    let with_clues =
        VALID_FORMAT_2_STORY.replace("deductions:", "clues:\n  - id: clue.legacy\ndeductions:");
    assert!(codes(with_clues).contains(&"format.clues_removed".to_string()));
}

#[test]
fn format_2_allows_empty_fact_lists_and_rejects_non_mapping_facts() {
    let empty = VALID_FORMAT_2_STORY.replace(
        "  - id: character.culprit",
        "  - id: character.culprit\n    facts: []",
    );
    let report = report(empty);
    assert!(report.valid, "{:#?}", report.diagnostics);

    let malformed = VALID_FORMAT_2_STORY.replace(
        "    facts:",
        "    facts:\n      - fact.legacy_reference\n    ignored_facts:",
    );
    assert!(codes(malformed).contains(&"fact.item_type".to_string()));
}

#[test]
fn format_2_validates_fact_requirements_and_cycles() {
    let malformed = VALID_FORMAT_2_STORY
        .replace("requires: flag.knife_analysis_complete", "requires: []")
        .replace(
            "requires: [fact.knife_is_present, entity.knife]",
            "requires: [fact.knife_is_present, case.example]",
        );
    let result = codes(malformed);
    assert!(result.contains(&"fact.requires_type".to_string()));
    assert!(result.contains(&"reference.wrong_type".to_string()));

    let cyclic = VALID_FORMAT_2_STORY.replace(
        "        statement: The knife is present.",
        "        statement: The knife is present.\n        requires: fact.knife_connects_to_scene",
    );
    assert!(codes(cyclic).contains(&"fact.requirement_cycle".to_string()));
}

#[test]
fn format_2_enforces_unclaimed_opening_facts_and_central_requirements() {
    let source = VALID_FORMAT_2_STORY
        .replace(
            "        statement: The knife is present.",
            "        statement: The knife is present.\n        initially_known: true",
        )
        .replace(
            "    travel_minutes: 1",
            "    travel_minutes: 1\n    facts: []",
        )
        .replace(
            "    inputs: [fact.knife_has_blood, fact.knife_connects_to_scene]",
            "    inputs: [fact.knife_has_blood, fact.knife_connects_to_scene]\n    supported_by: [clue.legacy]",
        );
    let result = codes(source);
    assert!(result.contains(&"fact.initially_known_removed".to_string()));
    assert!(result.contains(&"fact.owner_type".to_string()));
    assert!(result.contains(&"deduction.supported_by_removed".to_string()));
}

#[test]
fn format_2_validates_state_flags_and_delayed_give_effects() {
    let invalid = VALID_FORMAT_2_STORY
        .replace(
            "  - id: flag.knife_analysis_complete\n    name: Knife analysis complete\n    description: Whether the delayed knife analysis has completed.\n    initial_state: false",
            "  - id: flag.knife_analysis_complete\n    name: Knife analysis complete\n    description: Whether the delayed knife analysis has completed.\n    initial_state: sometimes",
        )
        .replace(
            "target: flag.knife_analysis_complete\n        value: 20m",
            "target: entity.knife\n        value: 0m",
        );
    let result = codes(invalid);
    assert!(result.contains(&"flag.initial_state".to_string()));
    assert!(result.contains(&"trigger.effect_target_type".to_string()));
    assert!(result.contains(&"trigger.effect_delay_invalid".to_string()));

    let legacy = VALID_FORMAT_2_STORY.replace("operation: give_after", "operation: learn_after");
    assert!(codes(legacy).contains(&"trigger.legacy_knowledge_effect".to_string()));

    for delay in ["1turn", "4294967296turns", "4294967296m"] {
        assert!(
            codes(VALID_FORMAT_2_STORY.replace("value: 20m", &format!("value: {delay}")))
                .contains(&"trigger.effect_delay_invalid".to_string()),
            "{delay} must be rejected before runtime"
        );
    }
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
        .replace("all_of: [flag.knife_examined]", "all_of: [not_an_id]")
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

    assert!(
        codes(VALID_STORY.replace("travel_minutes: 1", "travel_minutes: 4294967296"))
            .contains(&"route.invalid_travel_minutes".to_string())
    );
}

#[test]
fn validates_required_reference_shapes() {
    let source = VALID_STORY
        .replace("  victim: character.victim\n", "")
        .replace("    from: setting.foyer", "    from: 42")
        .replace(
            "    participants: [character.victim, character.culprit]",
            "    participants: character.victim",
        );
    let result = codes(source);
    assert!(result.contains(&"solution.missing_reference".to_string()));
    assert!(result.contains(&"route.missing_endpoint".to_string()));
    assert!(result.contains(&"event.participants_type".to_string()));
}

#[test]
fn validates_command_and_trigger_shapes() {
    let source = VALID_STORY
        .replace("description: Inspect an entity.", "aliases: [inspect]")
        .replace("type: entity", "accepts: [entity]")
        .replace("required: true", "required: sometimes")
        .replace("once: true", "once: sometimes")
        .replace(
            "    time:\n      relation: at",
            "    conditions: []\n    time:\n      relation: at",
        )
        .replace("value: visible", "value: 42");
    let result = codes(source);
    assert!(result.contains(&"command.aliases_removed".to_string()));
    assert!(result.contains(&"command.parameter_accepts_removed".to_string()));
    assert!(result.contains(&"command.parameter_kind".to_string()));
    assert!(result.contains(&"command.parameter_required_type".to_string()));
    assert!(result.contains(&"trigger.once_type".to_string()));
    assert!(result.contains(&"trigger.conditions_removed".to_string()));
    assert!(result.contains(&"trigger.effect_value_type".to_string()));
}

#[test]
fn validates_runtime_command_signatures_and_unique_parameter_names() {
    let duplicate =
        codes(VALID_FORMAT_2_STORY.replace("      - name: destination", "      - name: target"));
    assert!(duplicate.contains(&"command.parameter_name_duplicate".to_string()));

    let reserved = codes(VALID_FORMAT_2_STORY.replace(
        "    description: Learn that the knife is present.\n    effects:",
        "    description: Learn that the knife is present.\n    parameters:\n      - name: target\n        type: entity\n        required: true\n    effects:",
    ));
    assert!(reserved.contains(&"command.runtime_signature".to_string()));
}

#[test]
fn validates_action_effect_container_and_operation_shapes() {
    for (source, expected_code) in [
        (
            VALID_FORMAT_2_STORY.replace(
                "    effects:\n      - operation: advance_time",
                "    effects: advance_time\n    ignored_effects:\n      - operation: advance_time",
            ),
            "command.effects_type",
        ),
        (
            VALID_FORMAT_2_STORY.replace(
                "      - operation: advance_time\n        minutes: 12",
                "      - advance_time",
            ),
            "command.effect_type",
        ),
        (
            VALID_FORMAT_2_STORY.replace(
                "      - operation: advance_time\n        minutes: 12",
                "      - minutes: 12",
            ),
            "command.effect_operation",
        ),
        (
            VALID_FORMAT_2_STORY.replace("operation: advance_time", "operation: warp_time"),
            "command.effect_unknown_operation",
        ),
    ] {
        let result = codes(source);
        assert!(
            result.contains(&expected_code.to_string()),
            "missing {expected_code}: {result:#?}"
        );
    }
}

#[test]
fn validates_every_action_effect_payload() {
    let cases = [
        (
            "        minutes: 12",
            "        minutes: 0",
            "command.effect_minutes",
            "/commands/1/effects/0/minutes",
        ),
        (
            "        subjects: [player, character.culprit, param1, param3]",
            "        subjects: []",
            "command.effect_subjects",
            "/commands/1/effects/1/subjects",
        ),
        (
            "        setting: param2",
            "        setting: entity.knife",
            "reference.wrong_type",
            "/commands/1/effects/1/setting",
        ),
        (
            "        entity_from: param1",
            "        entity_from: setting.study",
            "reference.wrong_type",
            "/commands/1/effects/2/entity_from",
        ),
        (
            "        entity_to: entity.knife",
            "        entity_to: 42",
            "command.effect_reference",
            "/commands/1/effects/2/entity_to",
        ),
        (
            "        fact_id: fact.knife_has_blood",
            "        fact_id: entity.knife",
            "reference.wrong_type",
            "/commands/1/effects/3/fact_id",
        ),
        (
            "        deduction_id: param4",
            "        deduction_id: entity.knife",
            "reference.wrong_type",
            "/commands/1/effects/4/deduction_id",
        ),
        (
            "        text: The examination reveals a carefully staged scene.",
            "        text: \"\"",
            "command.effect_text",
            "/commands/1/effects/5/text",
        ),
        (
            "        trigger_id: trigger.investigate_knife",
            "        trigger_id: entity.knife",
            "reference.wrong_type",
            "/commands/1/effects/6/trigger_id",
        ),
        (
            "        text: The player has solved the mystery.",
            "        text: 42",
            "command.effect_text",
            "/commands/1/effects/7/text",
        ),
        (
            "        text: The trail has gone cold.",
            "        text: \"\"",
            "command.effect_text",
            "/commands/1/effects/8/text",
        ),
        (
            "        text: The examination reveals a carefully staged scene.",
            "        text: The examination reveals a carefully staged scene.\n        target: player",
            "command.effect_unknown_field",
            "/commands/1/effects/5/target",
        ),
    ];

    for (before, after, expected_code, expected_pointer) in cases {
        let report = report(VALID_FORMAT_2_STORY.replace(before, after));
        assert!(
            report.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == expected_code
                    && diagnostic.pointer.as_deref() == Some(expected_pointer)
            }),
            "missing {expected_code} at {expected_pointer}: {:#?}",
            report.diagnostics
        );
    }
}

#[test]
fn validates_action_effect_parameter_references() {
    let cases = [
        (
            "        setting: param2",
            "        setting: param1",
            "command.effect_parameter_type",
        ),
        (
            "        entity_from: param1",
            "        entity_from: param5",
            "command.effect_parameter_type",
        ),
        (
            "        entity_from: param1",
            "        entity_from: param6",
            "command.effect_parameter_unknown",
        ),
        (
            "        entity_from: param1",
            "        entity_from: param0",
            "command.effect_parameter_unknown",
        ),
    ];

    for (before, after, expected_code) in cases {
        let result = codes(VALID_FORMAT_2_STORY.replace(before, after));
        assert!(
            result.contains(&expected_code.to_string()),
            "missing {expected_code}: {result:#?}"
        );
    }
}

#[test]
fn action_effects_report_unknown_authored_ids() {
    let report = report(VALID_FORMAT_2_STORY.replace(
        "        fact_id: fact.knife_has_blood",
        "        fact_id: fact.not_authored",
    ));
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "reference.unknown"
            && diagnostic.pointer.as_deref() == Some("/commands/1/effects/3/fact_id")
            && diagnostic.subject_id.as_deref() == Some("fact.not_authored")
    }));
}

#[test]
fn format_2_no_longer_requires_a_fact_accepting_claim_action() {
    let report = report(VALID_FORMAT_2_STORY.replace("command.claim", "command.observe"));
    assert!(report.valid, "{:#?}", report.diagnostics);
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
fn validates_structured_trigger_time() {
    let cases = [
        (
            "    time:\n      relation: at\n      value: \"21:18\"",
            "    time: \"21:18\"",
            "trigger.time_type",
            "/triggers/0/time",
        ),
        (
            "      relation: at",
            "      relation: during",
            "trigger.time_relation",
            "/triggers/0/time/relation",
        ),
        (
            "      value: \"21:18\"",
            "      value: \"\"",
            "trigger.time_value",
            "/triggers/0/time/value",
        ),
        (
            "      value: \"21:18\"",
            "      value: \"21:18\"\n      timezone: local",
            "trigger.time_unknown_field",
            "/triggers/0/time/timezone",
        ),
    ];

    for (before, after, expected_code, expected_pointer) in cases {
        let report = report(VALID_STORY.replace(before, after));
        assert!(
            report.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == expected_code
                    && diagnostic.pointer.as_deref() == Some(expected_pointer)
            }),
            "missing {expected_code} at {expected_pointer}: {:#?}",
            report.diagnostics
        );
    }
}

#[test]
fn trigger_time_may_be_omitted() {
    let source = VALID_STORY.replace(
        "    time:\n      relation: at\n      value: \"21:18\"\n",
        "",
    );
    let omitted = report(source);
    assert!(omitted.valid, "{:#?}", omitted.diagnostics);

    let blank = report(VALID_STORY.replace(
        "    time:\n      relation: at\n      value: \"21:18\"",
        "    time:",
    ));
    assert!(blank.valid, "{:#?}", blank.diagnostics);
}

#[test]
fn validates_trigger_location_and_allows_blank_for_everywhere() {
    for replacement in [
        "    location: \"\"\n    any_of:",
        "    location:\n    any_of:",
    ] {
        let source = VALID_STORY.replace("    location: setting.study\n    any_of:", replacement);
        let report = report(source);
        assert!(report.valid, "{:#?}", report.diagnostics);
    }

    let malformed = report(VALID_STORY.replace(
        "    location: setting.study\n    any_of:",
        "    location: 42\n    any_of:",
    ));
    assert!(malformed.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "trigger.location_type"
            && diagnostic.pointer.as_deref() == Some("/triggers/0/location")
    }));

    let wrong_type = report(VALID_STORY.replace(
        "    location: setting.study\n    any_of:",
        "    location: entity.knife\n    any_of:",
    ));
    assert!(wrong_type.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "reference.wrong_type"
            && diagnostic.pointer.as_deref() == Some("/triggers/0/location")
    }));

    let unknown = report(VALID_STORY.replace(
        "    location: setting.study\n    any_of:",
        "    location: setting.not_authored\n    any_of:",
    ));
    assert!(unknown.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "reference.unknown"
            && diagnostic.pointer.as_deref() == Some("/triggers/0/location")
    }));
}

#[test]
fn validates_trigger_any_of_and_all_of_lists() {
    let cases = [
        (
            "    any_of: [character.culprit, entity.knife]",
            "    any_of: character.culprit",
            "trigger.any_of_type",
            "/triggers/0/any_of",
        ),
        (
            "    any_of: [character.culprit, entity.knife]",
            "    any_of: [character.culprit, 42]",
            "trigger.any_of_reference",
            "/triggers/0/any_of/1",
        ),
        (
            "    any_of: [character.culprit, entity.knife]",
            "    any_of: [character.culprit, setting.study]",
            "reference.wrong_type",
            "/triggers/0/any_of/1",
        ),
        (
            "    all_of: [flag.knife_examined]",
            "    all_of: [flag.not_authored]",
            "reference.unknown",
            "/triggers/0/all_of/0",
        ),
        (
            "    all_of: [flag.knife_examined]",
            "    all_of: [entity.knife, entity.knife]",
            "list.duplicate_reference",
            "/triggers/0/all_of/1",
        ),
    ];

    for (before, after, expected_code, expected_pointer) in cases {
        let report = report(VALID_STORY.replace(before, after));
        assert!(
            report.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == expected_code
                    && diagnostic.pointer.as_deref() == Some(expected_pointer)
            }),
            "missing {expected_code} at {expected_pointer}: {:#?}",
            report.diagnostics
        );
    }
}

#[test]
fn rejects_legacy_trigger_conditions() {
    let source = VALID_STORY.replace(
        "    time:\n",
        "    conditions:\n      - left: $target\n        operator: equals\n        right: entity.knife\n    time:\n",
    );
    let report = report(source);

    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "trigger.conditions_removed"
            && diagnostic.pointer.as_deref() == Some("/triggers/0/conditions")
    }));
}

#[test]
fn validates_optional_facts_and_deduction_inputs() {
    let source = VALID_FORMAT_2_STORY.to_string();

    let report = report(source.clone());
    assert!(report.valid, "{:#?}", report.diagnostics);

    let malformed = source
        .replace(
            "statement: The knife carries the victim's blood.",
            "statement: \"\"",
        )
        .replace(
            "inputs: [fact.knife_has_blood, fact.knife_connects_to_scene]",
            "inputs: [fact.knife_has_blood]",
        )
        .replace("truth: true", "truth: perhaps");
    let result = codes(malformed);
    assert!(result.contains(&"fact.missing_statement".to_string()));
    assert!(result.contains(&"deduction.inputs_type".to_string()));
    assert!(result.contains(&"deduction.truth_type".to_string()));
}

#[test]
fn validates_fact_reference_types() {
    let source = VALID_FORMAT_2_STORY.replace(
        "  - id: character.culprit",
        "  - id: character.culprit\n    knowledge:\n      - fact: entity.knife\n        source: entity.knife",
    );
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
fn detects_deduction_cycles_through_inputs() {
    let source = VALID_FORMAT_2_STORY
        .replace(
            "inputs: [fact.knife_has_blood, fact.knife_connects_to_scene]",
            "inputs: [deduction.loop, fact.knife_connects_to_scene]",
        )
        .replace(
            "flags:",
            "  - id: deduction.loop\n    conclusion: The reasoning loops back on itself.\n    inputs: [deduction.solution, fact.knife_has_blood]\n    truth: false\n    contradicted_by: [fact.knife_connects_to_scene]\nflags:",
        );
    assert!(codes(source).contains(&"deduction.dependency_cycle".to_string()));
}

#[test]
fn warns_for_unrefutable_false_deductions() {
    let source = VALID_FORMAT_2_STORY.replace("truth: true", "truth: false");
    let report = report(source);
    assert!(report.valid, "{:#?}", report.diagnostics);
    assert!(report
        .diagnostics
        .iter()
        .any(|item| item.code == "deduction.false_without_contradiction"));
}

#[test]
fn validates_deduction_solution_shape() {
    let source = VALID_FORMAT_2_STORY.replace(
        "truth: true",
        "truth: true\n    solves:\n      culprit: character.culprit\n      weapon: entity.knife\n      location: setting.study\n      time: \"29:00\"",
    );
    assert!(codes(source).contains(&"deduction.solves_invalid_time".to_string()));
}

#[test]
fn commands_and_triggers_remain_optional() {
    let source = VALID_STORY
        .split("commands:\n")
        .next()
        .expect("story before optional sections");
    let report = report(source);
    assert!(report.valid, "{:#?}", report.diagnostics);
}

#[test]
fn rejects_yaml_aliases_in_sequence_items() {
    let report = validate(&[SourceFile {
        path: "characters.yaml".to_string(),
        source: "characters:\n  - &victim\n    id: character.victim\n".to_string(),
    }]);
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "yaml.alias_unsupported"));
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

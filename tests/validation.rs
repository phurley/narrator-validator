use std::collections::BTreeMap;
use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use narrator_validator::{
    end_state_contract_metadata, validate, validate_with_supported_features, Severity, SourceFile,
};
use serde_yaml::{Mapping, Value};

const VALID_STORY: &str = include_str!("fixtures/valid-story.yaml");

const VALID_FORMAT_3_STORY: &str = include_str!("fixtures/valid-format-3-story.yaml");

fn report(source: impl Into<String>) -> narrator_validator::ValidationReport {
    validate(&story_files(source.into()))
}

#[test]
fn playability_report_proves_reachable_path_and_explains_action_and_time_blocks() {
    let root = std::path::Path::new("tests/fixtures/playability-analysis");
    let mut files = std::fs::read_dir(root)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            narrator_validator::SourceFile {
                path: path.file_name().unwrap().to_string_lossy().into_owned(),
                source: std::fs::read_to_string(path).unwrap(),
            }
        })
        .collect::<Vec<_>>();
    files.sort_by(|left, right| left.path.cmp(&right.path));
    let report = narrator_validator::validate(&files);
    let analysis = report.playability.expect("format 3 playability report");
    let paths = analysis
        .terminal_paths
        .iter()
        .map(|path| (path.id.as_str(), path))
        .collect::<std::collections::BTreeMap<_, _>>();

    let proved = paths["end.proved"];
    assert_eq!(proved.status, narrator_validator::PlayabilityStatus::Proved);
    let bound = proved.lower_bound.as_ref().unwrap();
    assert_eq!(bound.action_count, 2);
    assert_eq!(bound.route_action_count, 1);
    assert_eq!(bound.elapsed_minutes, 10);
    assert_eq!(bound.ordered_steps[0].kind, "route");
    assert!(bound
        .pivotal_unlocks
        .contains(&"fact.sample_matches".to_string()));
    assert!(bound
        .pivotal_unlocks
        .contains(&"deduction.solution".to_string()));
    assert!(bound
        .pivotal_unlocks
        .contains(&"score:deduction:deduction.solution".to_string()));

    let delayed = paths["end.delayed"];
    assert_eq!(
        delayed.status,
        narrator_validator::PlayabilityStatus::Proved
    );
    let delayed_bound = delayed.lower_bound.as_ref().unwrap();
    assert_eq!(delayed_bound.action_count, 4);
    assert_eq!(delayed_bound.elapsed_minutes, 35);
    assert_eq!(
        delayed_bound.required_waits[0].trigger,
        "trigger.delayed_analysis"
    );
    assert_eq!(delayed_bound.required_waits[0].delay_minutes, 20);

    let missing = paths["end.missing_action"];
    assert_eq!(
        missing.status,
        narrator_validator::PlayabilityStatus::NotProved
    );
    assert_eq!(
        missing.blocker.as_ref().unwrap().code,
        "playability.missing_requirement"
    );
    assert_eq!(
        missing.blocker.as_ref().unwrap().pointer,
        "/end_states/2/requires/0"
    );

    let again = narrator_validator::validate(&files).playability.unwrap();
    assert_eq!(
        analysis, again,
        "analysis must be byte-stable over identical input"
    );
    assert_eq!(analysis.notebook_policies.len(), 4);
    let manual = analysis
        .notebook_policies
        .iter()
        .find(|policy| !policy.auto_facts && !policy.auto_deductions)
        .expect("explicit fully manual notebook analysis");
    let manual_proved = manual
        .terminal_paths
        .iter()
        .find(|path| path.id == "end.proved")
        .expect("manual terminal path");
    assert_eq!(
        manual_proved.status,
        narrator_validator::PlayabilityStatus::Inconclusive,
        "ruleset 2 has no Claim command, so fully manual fact acquisition is explicitly unsupported"
    );
    assert_eq!(analysis.deduction_graph.maximum_depth, 1);
    assert_eq!(analysis.deduction_graph.largest_cascade_size, 1);
}

#[test]
fn playability_json_cli_matches_native_report() {
    let root = std::path::Path::new("tests/fixtures/playability-analysis");
    let mut files = std::fs::read_dir(root)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            narrator_validator::SourceFile {
                path: path.file_name().unwrap().to_string_lossy().into_owned(),
                source: std::fs::read_to_string(path).unwrap(),
            }
        })
        .collect::<Vec<_>>();
    files.sort_by(|left, right| left.path.cmp(&right.path));
    let native = narrator_validator::validate(&files);

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_narrator-validator"))
        .args(["--format", "json", root.to_str().unwrap()])
        .output()
        .unwrap();
    let cli: narrator_validator::ValidationReport = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(cli.playability, native.playability);
}

#[test]
fn author_save_performance_fixture_reports_all_policy_work_without_hitting_state_cap() {
    let files: Vec<SourceFile> = serde_json::from_str(include_str!(
        "fixtures/performance/author-save-two-question.json"
    ))
    .unwrap();
    let report = validate_with_supported_features(&files, &["reference_text_v1".to_string()]);
    assert!(report.valid, "{:#?}", report.diagnostics);
    let policies = &report.playability.unwrap().notebook_policies;
    assert_eq!(policies.len(), 4);
    assert_eq!(
        policies
            .iter()
            .map(|policy| policy.explored_states)
            .collect::<Vec<_>>(),
        // The two manual-notebook policies (auto_facts: false) shrank 4x
        // once testimony-revealed facts stopped being free at action 0
        // (narrator-validator#74): those facts now require the
        // corresponding `command.question` action before they can be
        // claimed, which prunes states that were previously reachable by
        // claiming them immediately.
        [32, 40, 352, 416]
    );
    assert!(policies.iter().all(|policy| !policy.bounded));
}

#[test]
fn deduction_graph_reports_large_branching_cascades_deterministically() {
    let source = VALID_FORMAT_3_STORY.replace(
        "    truth: true\nflags:",
        "    truth: true\n  - id: deduction.branch_a\n    conclusion: Blood and access identify a coordinated act.\n    inputs: [deduction.solution]\n    truth: true\n  - id: deduction.branch_b\n    conclusion: The same evidence fixes the sequence.\n    inputs: [deduction.solution]\n    truth: true\n  - id: deduction.joined\n    conclusion: The two branches support one chronology.\n    inputs: [deduction.branch_a, deduction.branch_b]\n    truth: true\n  - id: deduction.deep\n    conclusion: The chronology explains the final movement.\n    inputs: [deduction.joined]\n    truth: true\nflags:",
    );
    let first = report(source.clone()).playability.unwrap();
    let second = report(source).playability.unwrap();
    assert_eq!(first.deduction_graph, second.deduction_graph);
    assert_eq!(first.deduction_graph.maximum_depth, 4);
    assert_eq!(first.deduction_graph.largest_cascade_size, 5);
    assert_eq!(
        first.deduction_graph.largest_cascade_root.as_deref(),
        Some("fact.knife_connects_to_scene")
    );
    assert_eq!(
        first.deduction_graph.largest_cascade,
        [
            "deduction.branch_a",
            "deduction.branch_b",
            "deduction.deep",
            "deduction.joined",
            "deduction.solution",
        ]
    );
}

#[test]
fn mechanical_relay_nodes_receive_focused_review_evidence() {
    let source = VALID_FORMAT_3_STORY.replace(
        "    truth: true\nflags:",
        "    truth: true\n  - id: deduction.relay\n    conclusion: Therefore.\n    inputs: [deduction.solution]\n    truth: true\n  - id: deduction.followup\n    conclusion: The combined evidence fixes the chronology.\n    inputs: [deduction.relay]\n    truth: true\nflags:",
    );
    let report = report(source);
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "deduction.mechanical_relay"
            && diagnostic.pointer.as_deref() == Some("/deductions/1/conclusion")
            && diagnostic.subject_id.as_deref() == Some("deduction.relay")
    }));
}

#[test]
fn solution_answerability_is_proved_only_from_explicit_structural_overlap() {
    let root = std::path::Path::new("tests/fixtures/playability-analysis");
    let mut files = std::fs::read_dir(root)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let mut source = std::fs::read_to_string(path).unwrap();
            if name == "case.yaml" {
                source.push_str(
                    "solution:\n  win_state: end.proved\n  questions:\n    - prompt: Which sample proves the case?\n      answer: [entity.sample]\n",
                );
            } else if name == "deductions.yaml" {
                source.push_str("    solves:\n      weapon: entity.sample\n");
            }
            narrator_validator::SourceFile { path: name, source }
        })
        .collect::<Vec<_>>();
    files.sort_by(|left, right| left.path.cmp(&right.path));
    let analysis = narrator_validator::validate(&files).playability.unwrap();
    let automatic = analysis
        .notebook_policies
        .iter()
        .find(|policy| policy.auto_facts && policy.auto_deductions)
        .unwrap();
    assert_eq!(
        automatic.solution_answerability.status,
        narrator_validator::PlayabilityStatus::Proved
    );
    assert_eq!(
        automatic
            .solution_answerability
            .solution_equivalent_deductions,
        ["deduction.solution"]
    );

    let deductions = files
        .iter_mut()
        .find(|file| file.path == "deductions.yaml")
        .unwrap();
    deductions.source = deductions
        .source
        .replace("    solves:\n      weapon: entity.sample\n", "");
    let ambiguous = narrator_validator::validate(&files).playability.unwrap();
    assert!(ambiguous.notebook_policies.iter().all(|policy| {
        policy.solution_answerability.status != narrator_validator::PlayabilityStatus::Proved
    }));
}

#[test]
fn playability_reports_route_time_block_when_network_cannot_advance_enough() {
    let root = std::path::Path::new("tests/fixtures/playability-analysis");
    let mut files = std::fs::read_dir(root)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            narrator_validator::SourceFile {
                path: path.file_name().unwrap().to_string_lossy().into_owned(),
                source: std::fs::read_to_string(path).unwrap(),
            }
        })
        .collect::<Vec<_>>();
    let settings = files
        .iter_mut()
        .find(|file| file.path == "settings.yaml")
        .unwrap();
    settings.source = settings.source.replace(
        "  - id: route.lab_entry\n    from: setting.lab\n    to: setting.entry\n    bidirectional: false\n    travel_minutes: 15\n",
        "",
    );
    files
        .iter_mut()
        .find(|file| file.path == "commands.yaml")
        .unwrap()
        .source = "commands: []\n".to_string();
    files
        .iter_mut()
        .find(|file| file.path == "triggers.yaml")
        .unwrap()
        .source = "triggers: []\n".to_string();
    files
        .iter_mut()
        .find(|file| file.path == "end_states.yaml")
        .unwrap()
        .source = r#"end_states:
  - id: end.route_time_blocked
    name: Route time blocked
    outcome: lost
    resolution: failure
    requires: [flag.clock_gate]
    at_or_after: "23:00"
    text: The route network cannot advance enough time.
"#
    .to_string();
    let path = narrator_validator::validate(&files)
        .playability
        .unwrap()
        .terminal_paths
        .remove(0);
    assert_eq!(
        path.status,
        narrator_validator::PlayabilityStatus::NotProved
    );
    assert_eq!(path.blocker.unwrap().code, "playability.route_time_blocked");
}

#[test]
fn removing_a_path_action_cannot_leave_delayed_terminal_proved() {
    let root = std::path::Path::new("tests/fixtures/playability-analysis");
    let mut files = std::fs::read_dir(root)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            narrator_validator::SourceFile {
                path: path.file_name().unwrap().to_string_lossy().into_owned(),
                source: std::fs::read_to_string(path).unwrap(),
            }
        })
        .collect::<Vec<_>>();
    files
        .iter_mut()
        .find(|file| file.path == "commands.yaml")
        .unwrap()
        .source = "commands: []\n".to_string();
    let report = narrator_validator::validate(&files);
    let delayed = report
        .playability
        .unwrap()
        .terminal_paths
        .into_iter()
        .find(|path| path.id == "end.delayed")
        .unwrap();
    assert_ne!(
        delayed.status,
        narrator_validator::PlayabilityStatus::Proved
    );
}

#[test]
fn unsupported_dynamic_effect_makes_unresolved_paths_conspicuously_inconclusive() {
    let root = std::path::Path::new("tests/fixtures/playability-analysis");
    let mut files = std::fs::read_dir(root)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            narrator_validator::SourceFile {
                path: path.file_name().unwrap().to_string_lossy().into_owned(),
                source: std::fs::read_to_string(path).unwrap(),
            }
        })
        .collect::<Vec<_>>();
    files.iter_mut().find(|file| file.path == "commands.yaml").unwrap().source.push_str(
        "  - id: command.dynamic\n    name: Dynamic reset\n    effects:\n      - operation: set_flag\n        flag: flag.missing_action\n        value: true\n      - operation: set_flag\n        flag: flag.clock_gate\n        value: false\n",
    );
    let report = narrator_validator::validate(&files);
    let missing = report
        .playability
        .unwrap()
        .terminal_paths
        .into_iter()
        .find(|path| path.id == "end.missing_action")
        .unwrap();
    assert_eq!(
        missing.status,
        narrator_validator::PlayabilityStatus::Inconclusive
    );
    let blocker = missing.blocker.unwrap();
    assert_eq!(blocker.code, "playability.unsupported_effect");
    assert_eq!(blocker.path, "commands.yaml");
    assert_eq!(blocker.pointer, "/commands/1/effects/1");
}

#[test]
fn playability_never_traverses_an_unmet_authored_route_requirement() {
    let root = std::path::Path::new("tests/fixtures/playability-analysis");
    let mut files = std::fs::read_dir(root)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            narrator_validator::SourceFile {
                path: path.file_name().unwrap().to_string_lossy().into_owned(),
                source: std::fs::read_to_string(path).unwrap(),
            }
        })
        .collect::<Vec<_>>();
    files
        .iter_mut()
        .find(|file| file.path == "settings.yaml")
        .unwrap()
        .source = files
        .iter()
        .find(|file| file.path == "settings.yaml")
        .unwrap()
        .source
        .replace(
            "    travel_minutes: 10\n",
            "    travel_minutes: 10\n    requires: [entity.sample]\n",
        );

    let report = narrator_validator::validate(&files);
    assert!(report.valid, "{:#?}", report.diagnostics);
    let terminal = report
        .playability
        .as_ref()
        .unwrap()
        .terminal_paths
        .iter()
        .find(|path| path.id == "end.proved")
        .unwrap();
    assert_eq!(
        terminal.status,
        narrator_validator::PlayabilityStatus::Inconclusive
    );
    let blocker = terminal.blocker.as_ref().unwrap();
    assert_eq!(blocker.code, "playability.unsupported_inventory_condition");
    assert_eq!(blocker.path, "settings.yaml");
    assert_eq!(blocker.pointer, "/routes/0/requires/0");
}

#[test]
fn duplicate_deduction_input_sets_are_runtime_ambiguous_and_inconclusive() {
    let root = std::path::Path::new("tests/fixtures/playability-analysis");
    let mut files = std::fs::read_dir(root)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            narrator_validator::SourceFile {
                path: path.file_name().unwrap().to_string_lossy().into_owned(),
                source: std::fs::read_to_string(path).unwrap(),
            }
        })
        .collect::<Vec<_>>();
    files
        .iter_mut()
        .find(|file| file.path == "deductions.yaml")
        .unwrap()
        .source
        .push_str(
            "  - id: deduction.ambiguous\n    conclusion: The same submitted evidence could select this deduction.\n    inputs: [fact.sample_matches]\n    truth: true\n",
        );

    let report = narrator_validator::validate(&files);
    assert!(report.valid, "{:#?}", report.diagnostics);
    let terminal = report
        .playability
        .unwrap()
        .terminal_paths
        .into_iter()
        .find(|path| path.id == "end.proved")
        .unwrap();
    assert_eq!(
        terminal.status,
        narrator_validator::PlayabilityStatus::Inconclusive
    );
    let blocker = terminal.blocker.unwrap();
    assert_eq!(blocker.code, "playability.unsupported_ambiguous_deduction");
    assert_eq!(blocker.path, "deductions.yaml");
    assert_eq!(blocker.pointer, "/deductions/0/inputs");
    assert_eq!(blocker.range.unwrap().start.line, 4);
}

#[test]
fn unsupported_matching_trigger_downgrades_an_apparent_proof_to_inconclusive() {
    let root = std::path::Path::new("tests/fixtures/playability-analysis");
    let mut files = std::fs::read_dir(root)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            narrator_validator::SourceFile {
                path: path.file_name().unwrap().to_string_lossy().into_owned(),
                source: std::fs::read_to_string(path).unwrap(),
            }
        })
        .collect::<Vec<_>>();
    files
        .iter_mut()
        .find(|file| file.path == "end_states.yaml")
        .unwrap()
        .source = files
        .iter()
        .find(|file| file.path == "end_states.yaml")
        .unwrap()
        .source
        .replace(
            "    requires: [deduction.solution]\n",
            "    requires: [deduction.solution, flag.clock_gate]\n",
        );
    files
        .iter_mut()
        .find(|file| file.path == "triggers.yaml")
        .unwrap()
        .source
        .push_str(
            "  - id: trigger.invalidate_proof\n    name: Invalidate proof\n    on:\n      command: command.examine\n      parameters: { target: entity.sample }\n    effects:\n      - operation: set_flag\n        flag: flag.clock_gate\n        value: false\n",
        );

    let report = narrator_validator::validate(&files);
    assert!(report.valid, "{:#?}", report.diagnostics);
    let terminal = report
        .playability
        .unwrap()
        .terminal_paths
        .into_iter()
        .find(|path| path.id == "end.proved")
        .unwrap();
    assert_eq!(
        terminal.status,
        narrator_validator::PlayabilityStatus::Inconclusive
    );
    assert_eq!(
        terminal.blocker.unwrap().code,
        "playability.unsupported_effect"
    );
}

#[test]
fn trigger_advance_time_participates_in_cost_deadlines_and_precedence() {
    let root = std::path::Path::new("tests/fixtures/playability-analysis");
    let mut files = std::fs::read_dir(root)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            narrator_validator::SourceFile {
                path: path.file_name().unwrap().to_string_lossy().into_owned(),
                source: std::fs::read_to_string(path).unwrap(),
            }
        })
        .collect::<Vec<_>>();
    let settings = files
        .iter_mut()
        .find(|file| file.path == "settings.yaml")
        .unwrap();
    settings.source = settings.source.replace(
        "  - id: route.lab_entry\n    from: setting.lab\n    to: setting.entry\n    bidirectional: false\n    travel_minutes: 15\n",
        "",
    );
    files
        .iter_mut()
        .find(|file| file.path == "triggers.yaml")
        .unwrap()
        .source
        .push_str(
            "  - id: trigger.advance_clock\n    name: Advance clock\n    on:\n      command: command.examine\n      parameters: { target: entity.sample }\n    effects:\n      - operation: advance_time\n        minutes: 20\n",
        );
    let ends = files
        .iter_mut()
        .find(|file| file.path == "end_states.yaml")
        .unwrap();
    ends.source = ends.source.replacen(
        "end_states:\n",
        "end_states:\n  - id: end.deadline\n    name: Deadline\n    outcome: lost\n    resolution: failure\n    at_or_after: \"20:15\"\n    text: The clock expired.\n",
        1,
    );

    let report = narrator_validator::validate(&files);
    assert!(report.valid, "{:#?}", report.diagnostics);
    let analysis = report.playability.unwrap();
    let deadline = analysis
        .terminal_paths
        .iter()
        .find(|path| path.id == "end.deadline")
        .unwrap();
    assert_eq!(
        deadline.status,
        narrator_validator::PlayabilityStatus::Proved
    );
    let bound = deadline.lower_bound.as_ref().unwrap();
    assert_eq!(bound.action_count, 2);
    assert_eq!(bound.elapsed_minutes, 30);
    assert_eq!(bound.wait_minutes, 20);
    assert_ne!(
        analysis
            .terminal_paths
            .iter()
            .find(|path| path.id == "end.proved")
            .unwrap()
            .status,
        narrator_validator::PlayabilityStatus::Proved
    );
}

#[test]
fn action_triggers_snapshot_pre_effect_flags_and_time_before_deadline_precedence() {
    let root = std::path::Path::new("tests/fixtures/playability-analysis");
    let mut files = std::fs::read_dir(root)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            narrator_validator::SourceFile {
                path: path.file_name().unwrap().to_string_lossy().into_owned(),
                source: std::fs::read_to_string(path).unwrap(),
            }
        })
        .collect::<Vec<_>>();
    let settings = files
        .iter_mut()
        .find(|file| file.path == "settings.yaml")
        .unwrap();
    settings.source = settings.source.replace(
        "  - id: route.lab_entry\n    from: setting.lab\n    to: setting.entry\n    bidirectional: false\n    travel_minutes: 15\n",
        "",
    );
    let commands = files
        .iter_mut()
        .find(|file| file.path == "commands.yaml")
        .unwrap();
    commands.source = commands.source.replace(
        "        candidates: { from: [current_location] }\n",
        "        candidates: { from: [current_location] }\n    effects:\n      - operation: set_flag\n        flag: flag.command_set\n        value: true\n      - operation: advance_time\n        minutes: 20\n",
    );
    files
        .iter_mut()
        .find(|file| file.path == "flags.yaml")
        .unwrap()
        .source
        .push_str(
            "  - id: flag.command_set\n    name: Command-set gate\n    description: Set by the triggering command itself.\n    initial_state: false\n  - id: flag.triggered\n    name: Trigger completed\n    description: Set only when the pre-action conditions match.\n    initial_state: false\n",
        );
    files
        .iter_mut()
        .find(|file| file.path == "triggers.yaml")
        .unwrap()
        .source
        .push_str(
            "  - id: trigger.pre_effect_gate\n    name: Pre-effect flag and time gate\n    on:\n      command: command.investigate\n      parameters: { target: entity.sample }\n    when:\n      all:\n        - flag: flag.command_set\n        - time:\n            relation: after\n            value: \"20:20\"\n    effects:\n      - operation: set_flag\n        flag: flag.triggered\n        value: true\n",
        );
    let ends = files
        .iter_mut()
        .find(|file| file.path == "end_states.yaml")
        .unwrap();
    ends.source = ends.source.replacen(
        "end_states:\n",
        "end_states:\n  - id: end.deadline\n    name: Deadline\n    outcome: lost\n    resolution: failure\n    at_or_after: \"20:40\"\n    text: The deadline wins ordered precedence.\n  - id: end.pre_effect_trigger\n    name: Triggered before deadline\n    outcome: won\n    resolution: full\n    requires: [flag.triggered]\n    text: The trigger completed before the deadline.\n",
        1,
    );

    let report = narrator_validator::validate(&files);
    assert!(report.valid, "{:#?}", report.diagnostics);
    let analysis = report.playability.unwrap();
    let deadline = analysis
        .terminal_paths
        .iter()
        .find(|path| path.id == "end.deadline")
        .unwrap();
    assert_eq!(
        deadline.status,
        narrator_validator::PlayabilityStatus::Proved
    );
    let bound = deadline.lower_bound.as_ref().unwrap();
    assert_eq!(bound.action_count, 3);
    assert_eq!(bound.elapsed_minutes, 50);
    assert_eq!(bound.wait_minutes, 40);
    assert!(bound
        .pivotal_unlocks
        .contains(&"trigger.pre_effect_gate".to_string()));
    assert!(bound
        .pivotal_unlocks
        .contains(&"flag.triggered".to_string()));
    let triggered = analysis
        .terminal_paths
        .iter()
        .find(|path| path.id == "end.pre_effect_trigger")
        .unwrap();
    assert_eq!(
        triggered.status,
        narrator_validator::PlayabilityStatus::NotProved
    );
    assert_eq!(
        triggered.blocker.as_ref().unwrap().code,
        "playability.precedence_blocked"
    );
}

#[test]
fn matched_trigger_effects_settle_before_same_action_fact_and_ordered_end_evaluation() {
    let root = std::path::Path::new("tests/fixtures/playability-analysis");
    let mut files = std::fs::read_dir(root)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            narrator_validator::SourceFile {
                path: path.file_name().unwrap().to_string_lossy().into_owned(),
                source: std::fs::read_to_string(path).unwrap(),
            }
        })
        .collect::<Vec<_>>();
    let settings = files
        .iter_mut()
        .find(|file| file.path == "settings.yaml")
        .unwrap();
    settings.source = settings.source.replace(
        "  - id: route.lab_entry\n    from: setting.lab\n    to: setting.entry\n    bidirectional: false\n    travel_minutes: 15\n",
        "",
    );
    files
        .iter_mut()
        .find(|file| file.path == "flags.yaml")
        .unwrap()
        .source
        .push_str(
            "  - id: flag.trigger_set\n    name: Trigger-set gate\n    description: Set by the matching trigger before fact discovery.\n    initial_state: false\n",
        );
    let entities = files
        .iter_mut()
        .find(|file| file.path == "entities.yaml")
        .unwrap();
    entities.source = entities.source.replace(
        "          command: command.examine\n          parameters: { target: owner }\n",
        "          command: command.investigate\n          parameters: { target: owner }\n        when:\n          all:\n            - flag: flag.trigger_set\n",
    );
    files
        .iter_mut()
        .find(|file| file.path == "triggers.yaml")
        .unwrap()
        .source
        .push_str(
            "  - id: trigger.same_action_gate\n    name: Same-action fact gate\n    on:\n      command: command.investigate\n      parameters: { target: entity.sample }\n    effects:\n      - operation: set_flag\n        flag: flag.trigger_set\n        value: true\n      - operation: advance_time\n        minutes: 20\n",
        );
    let ends = files
        .iter_mut()
        .find(|file| file.path == "end_states.yaml")
        .unwrap();
    ends.source = ends.source.replacen(
        "end_states:\n",
        "end_states:\n  - id: end.same_action_fact\n    name: Same-action fact win\n    outcome: won\n    resolution: full\n    requires: [fact.sample_matches]\n    text: The settled trigger reveals the decisive fact.\n  - id: end.same_turn_deadline\n    name: Same-turn deadline\n    outcome: lost\n    resolution: failure\n    at_or_after: \"20:30\"\n    text: The deadline loses to the earlier satisfied fact ending.\n",
        1,
    );

    let report = narrator_validator::validate(&files);
    assert!(report.valid, "{:#?}", report.diagnostics);
    let analysis = report.playability.unwrap();
    let win = analysis
        .terminal_paths
        .iter()
        .find(|path| path.id == "end.same_action_fact")
        .unwrap();
    assert_eq!(win.status, narrator_validator::PlayabilityStatus::Proved);
    let bound = win.lower_bound.as_ref().unwrap();
    assert_eq!(bound.action_count, 2);
    assert_eq!(bound.elapsed_minutes, 30);
    assert!(bound
        .pivotal_unlocks
        .contains(&"flag.trigger_set".to_string()));
    assert!(bound
        .pivotal_unlocks
        .contains(&"fact.sample_matches".to_string()));
    let deadline = analysis
        .terminal_paths
        .iter()
        .find(|path| path.id == "end.same_turn_deadline")
        .unwrap();
    assert_eq!(
        deadline.status,
        narrator_validator::PlayabilityStatus::NotProved
    );
    assert_eq!(
        deadline.blocker.as_ref().unwrap().code,
        "playability.precedence_blocked"
    );
}

#[test]
fn elapsed_search_bound_allows_the_exact_limit_but_never_proves_beyond_it() {
    let root = std::path::Path::new("tests/fixtures/playability-analysis");
    for (minutes, expected) in [
        (2_880, narrator_validator::PlayabilityStatus::Proved),
        (2_881, narrator_validator::PlayabilityStatus::Inconclusive),
    ] {
        let mut files = std::fs::read_dir(root)
            .unwrap()
            .map(|entry| {
                let path = entry.unwrap().path();
                narrator_validator::SourceFile {
                    path: path.file_name().unwrap().to_string_lossy().into_owned(),
                    source: std::fs::read_to_string(path).unwrap(),
                }
            })
            .collect::<Vec<_>>();
        files
            .iter_mut()
            .find(|file| file.path == "commands.yaml")
            .unwrap()
            .source
            .push_str(&format!(
                "  - id: command.long_wait\n    name: Long wait\n    effects:\n      - operation: set_flag\n        flag: flag.missing_action\n        value: true\n      - operation: advance_time\n        minutes: {minutes}\n"
            ));

        let report = narrator_validator::validate(&files);
        assert!(report.valid, "{:#?}", report.diagnostics);
        let analysis = report.playability.unwrap();
        let terminal = analysis
            .terminal_paths
            .iter()
            .find(|path| path.id == "end.missing_action")
            .unwrap();
        assert_eq!(terminal.status, expected, "{minutes} minutes");
        if minutes == 2_880 {
            assert_eq!(
                terminal.lower_bound.as_ref().unwrap().elapsed_minutes,
                2_880
            );
        } else {
            assert!(analysis.bounded);
            assert!(terminal.lower_bound.is_none());
            assert_eq!(
                terminal.blocker.as_ref().unwrap().code,
                "playability.search_bound"
            );
        }
    }
}

#[test]
fn authored_solution_questions_produce_one_exact_supported_solve_action() {
    let root = std::path::Path::new("tests/fixtures/format-3.3-solve-card-sets");
    let mut files = std::fs::read_dir(root)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            narrator_validator::SourceFile {
                path: path.file_name().unwrap().to_string_lossy().into_owned(),
                source: std::fs::read_to_string(path).unwrap(),
            }
        })
        .collect::<Vec<_>>();
    files.sort_by(|left, right| left.path.cmp(&right.path));

    let report = narrator_validator::validate(&files);
    assert!(report.valid, "{:#?}", report.diagnostics);
    let terminal = report
        .playability
        .unwrap()
        .terminal_paths
        .into_iter()
        .find(|path| path.id == "win.solve_case")
        .unwrap();
    assert_eq!(
        terminal.status,
        narrator_validator::PlayabilityStatus::Proved
    );
    let bound = terminal.lower_bound.unwrap();
    assert_eq!(bound.action_count, 1);
    assert_eq!(
        bound.ordered_steps[0].action,
        "command.solve [character.culprit] [setting.study entity.knife]"
    );
    assert!(bound
        .pivotal_unlocks
        .contains(&"solution.correct".to_string()));
}

/// Renaming a referenced definition (Format 3.2+ `[[...]]` reference text) is
/// validator behaviour: every resolved pointer that references the renamed
/// definition must pick up the new display value on the next validation, and
/// none may still show the old value or leak an unresolved `[[...]]` marker
/// or raw `kind.id` token. This used to be proved incidentally by
/// `simple_mystery` mutating a YAML snapshot and re-validating in-process via
/// the WASM API; it moved here when story repos standardised on the `@v1`
/// action (a single action invocation only produces one report, so the story
/// repo can no longer validate twice). See narrator-system#15 and #39.
#[test]
fn renaming_a_referenced_definition_propagates_to_every_resolved_pointer() {
    let root = std::path::Path::new("tests/fixtures/format-3.3-solve-card-sets");
    let mut files = std::fs::read_dir(root)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            narrator_validator::SourceFile {
                path: path.file_name().unwrap().to_string_lossy().into_owned(),
                source: std::fs::read_to_string(path).unwrap(),
            }
        })
        .collect::<Vec<_>>();
    files.sort_by(|left, right| left.path.cmp(&right.path));

    let before = narrator_validator::validate(&files);
    assert!(before.valid, "{:#?}", before.diagnostics);
    let prompt_before = before
        .reference_text
        .iter()
        .find(|field| field.pointer == "/solution/questions/0/prompt")
        .expect("solution question prompt is a registered reference-text consumer");
    assert_eq!(
        prompt_before.resolved,
        "Who planned the crime against Rowan Vale?"
    );

    let characters = files
        .iter_mut()
        .find(|file| file.path == "characters.yaml")
        .expect("characters.yaml fixture file");
    assert!(
        characters.source.contains("name: Rowan Vale"),
        "fixture must still define the name this test renames"
    );
    characters.source = characters
        .source
        .replace("name: Rowan Vale", "name: Rowan Ashford");

    let after = narrator_validator::validate(&files);
    assert!(after.valid, "{:#?}", after.diagnostics);
    let prompt_after = after
        .reference_text
        .iter()
        .find(|field| field.pointer == "/solution/questions/0/prompt")
        .expect("solution question prompt is still a registered reference-text consumer");
    assert_eq!(
        prompt_after.resolved,
        "Who planned the crime against Rowan Ashford?"
    );
    assert!(!prompt_after.resolved.contains("Rowan Vale"));
    assert!(!prompt_after.resolved.contains("[["));
    assert!(!prompt_after.resolved.contains("]]"));
    assert!(!prompt_after.resolved.contains("character.victim"));
}

fn codes(source: impl Into<String>) -> Vec<String> {
    report(source)
        .diagnostics
        .into_iter()
        .map(|diagnostic| diagnostic.code)
        .collect()
}

fn format_3_story_with_narrative_details() -> String {
    include_str!("fixtures/format-3-story-with-narrative-details.yaml").to_string()
}

fn format_3_story_with_occurrences_on_every_fact_owner() -> String {
    include_str!("fixtures/format-3-story-with-occurrences-on-every-fact-owner.yaml").to_string()
}

fn format_3_story_with_entity_occurrence(occurred_at: &str) -> String {
    VALID_FORMAT_3_STORY.replace(
        "        statement: The knife is present.",
        &format!("        statement: The knife is present.\n{occurred_at}"),
    )
}

fn format_3_story_with_character_fields(fields: &str) -> String {
    VALID_FORMAT_3_STORY
        .replace(
            "  - id: character.culprit\n    description: A suspect with a carefully guarded secret.\nentities:",
            &format!("  - id: character.culprit\n    description: A suspect with a carefully guarded secret.\n{fields}entities:"),
        )
        .replace(
            "commands:\n",
            "commands:\n  - id: command.question\n    name: Question\n    parameters:\n      - name: character\n        types: [character]\n        min: 1\n        max: 1\n",
        )
}

fn format_3_story_with_player_safe_character_behavior() -> String {
    include_str!("fixtures/format-3-story-with-player-safe-character-behavior.yaml").to_string()
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

#[test]
fn valid_repository_has_no_diagnostics() {
    let report = report(VALID_STORY);
    assert!(report.valid, "{:#?}", report.diagnostics);
    assert_eq!(report.format_version.as_deref(), Some("1.0.0"));
    assert!(report.diagnostics.is_empty());
}

#[test]
fn valid_format_3_repository_has_no_diagnostics() {
    let report = report(VALID_FORMAT_3_STORY);
    assert!(report.valid, "{:#?}", report.diagnostics);
    assert_eq!(report.validator_version, env!("CARGO_PKG_VERSION"));
    assert_eq!(report.format_version.as_deref(), Some("3.0.0"));
    assert!(report.diagnostics.is_empty());
}

fn with_standard_ruleset(source: &str) -> String {
    source.replace(
        "  format_version: \"3.0.0\"",
        "  format_version: \"3.0.0\"\n  ruleset:\n    id: ruleset.standard_mystery\n    version: \"1.0.0\"",
    )
}

#[test]
fn standard_ruleset_commands_join_global_validation_and_allow_extensions() {
    let source = with_standard_ruleset(VALID_FORMAT_3_STORY)
        .replace("  - id: command.claim\n    name: Claim\n    description: Learn that the knife is present.\n    effects:\n      - operation: learn_fact\n        fact_id: fact.knife_is_present\n", "")
        .replace("    subject: command.claim", "    subject: command.move");
    let report = report(source);
    assert!(report.valid, "{:#?}", report.diagnostics);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
}

#[test]
fn standard_ruleset_2_0_validates_with_format_3_1() {
    let source = with_standard_ruleset(VALID_FORMAT_3_STORY)
        .replace("format_version: \"3.0.0\"", "format_version: \"3.1.0\"")
        .replace("version: \"1.0.0\"", "version: \"2.0.0\"")
        .replace("  - id: command.claim\n    name: Claim\n    description: Learn that the knife is present.\n    effects:\n      - operation: learn_fact\n        fact_id: fact.knife_is_present\n", "")
        .replace("    subject: command.claim", "    subject: command.move");
    let report = report(source);
    assert!(report.valid, "{:#?}", report.diagnostics);
}

#[test]
fn standard_ruleset_2_0_requires_format_3_1() {
    let source = with_standard_ruleset(VALID_FORMAT_3_STORY)
        .replace("version: \"1.0.0\"", "version: \"2.0.0\"");
    let report = report(source);
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "ruleset.format_incompatible"
            && diagnostic.pointer.as_deref() == Some("/case/ruleset/version")
    }));
}

#[test]
fn standard_ruleset_4_0_is_the_format_3_4_manual_notebook_catalog() {
    let source = format_3_3_question_story()
        .replacen("format_version: \"3.3.0\"", "format_version: \"3.4.0\"", 1)
        .replacen("version: \"3.0.0\"", "version: \"4.0.0\"", 1);
    let valid_report = report(source);
    assert!(valid_report.valid, "{:#?}", valid_report.diagnostics);
    assert!(valid_report
        .diagnostics
        .iter()
        .all(|diagnostic| { diagnostic.code == "win_states.legacy_compatibility" }));
    assert_eq!(valid_report.playability.unwrap().notebook_policies.len(), 4);

    let incompatible =
        report(format_3_3_question_story().replacen("version: \"3.0.0\"", "version: \"4.0.0\"", 1));
    assert!(incompatible.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "ruleset.format_incompatible"
            && diagnostic.pointer.as_deref() == Some("/case/ruleset/version")
    }));
}

#[test]
fn standard_ruleset_5_0_is_format_3_4_and_adds_reconciliation() {
    let source =
        format_3_4_end_state_story().replacen("version: \"3.0.0\"", "version: \"5.0.0\"", 1);
    let valid_report = report(source);
    assert!(valid_report.valid, "{:#?}", valid_report.diagnostics);
    assert_eq!(valid_report.playability.unwrap().notebook_policies.len(), 4);

    let incompatible =
        report(format_3_3_question_story().replacen("version: \"3.0.0\"", "version: \"5.0.0\"", 1));
    assert!(incompatible.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "ruleset.format_incompatible"
            && diagnostic.pointer.as_deref() == Some("/case/ruleset/version")
    }));
}

#[test]
fn standard_ruleset_6_0_validates_the_same_solution_contract_as_5_0() {
    let source =
        format_3_4_end_state_story().replacen("version: \"3.0.0\"", "version: \"6.0.0\"", 1);
    let valid_report = report(source);
    assert!(valid_report.valid, "{:#?}", valid_report.diagnostics);
    assert_eq!(valid_report.playability.unwrap().notebook_policies.len(), 4);

    let incompatible =
        report(format_3_3_question_story().replacen("version: \"3.0.0\"", "version: \"6.0.0\"", 1));
    assert!(incompatible.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "ruleset.format_incompatible"
            && diagnostic.pointer.as_deref() == Some("/case/ruleset/version")
    }));
}

fn format_3_3_question_story() -> String {
    include_str!("fixtures/format-3.3-question-story.yaml").to_string()
}

fn format_3_4_end_state_story() -> String {
    include_str!("fixtures/format-3.4-end-state-story.yaml").to_string()
}

fn format_3_4_ruleset_5_solution_prerequisite_story() -> String {
    include_str!("fixtures/format-3.4-ruleset-5-solution-prerequisite-story.yaml").to_string()
}

fn format_3_testimony_gated_playability_story() -> String {
    include_str!("fixtures/format-3-testimony-gated-playability-story.yaml").to_string()
}

fn format_3_7_step_story() -> String {
    include_str!("fixtures/format-3.7-step-story.yaml").to_string()
}

#[test]
fn fact_revealed_only_by_testimony_requires_the_question_action_not_free_at_action_zero() {
    // `fact.knife_is_present` has no `on`/`when` of its own; it is revealed
    // exclusively by `character.culprit`'s testimony. The playability model
    // must not treat it as available at action 0 -- the deduction (and the
    // end state that depends on it) can only be proved after actually
    // questioning the culprit.
    let report = report(format_3_testimony_gated_playability_story());
    assert!(report.valid, "{:#?}", report.diagnostics);
    let analysis = report.playability.expect("format 3 playability report");
    let policy = analysis
        .notebook_policies
        .iter()
        .find(|policy| policy.auto_facts && policy.auto_deductions)
        .expect("default auto-facts/auto-deductions notebook policy");
    let proved = policy
        .terminal_paths
        .iter()
        .find(|path| path.id == "end.proved")
        .expect("terminal path for the testimony-gated deduction");
    assert_eq!(
        proved.status,
        narrator_validator::PlayabilityStatus::Proved,
        "{proved:#?}"
    );
    let bound = proved
        .lower_bound
        .as_ref()
        .expect("proved path carries a lower bound");
    assert!(
        bound.action_count >= 1,
        "the fact-revealing question must be a required action, not free: {bound:#?}"
    );
    assert!(
        bound
            .ordered_steps
            .iter()
            .any(|step| step.action.starts_with("command.question")),
        "the proof must include a `command.question` step against the culprit: {bound:#?}"
    );
    assert!(
        bound
            .ordered_steps
            .iter()
            .any(|step| step.unlocks.contains(&"fact.knife_is_present".to_string())),
        "the fact must be unlocked by an action, not present at action 0: {bound:#?}"
    );
}

fn format_3_5_command_costs_story() -> String {
    include_str!("fixtures/format-3.5-command-costs-story.yaml").to_string()
}

#[test]
fn command_cost_override_validates_end_to_end() {
    let report = report(format_3_5_command_costs_story());
    assert!(report.valid, "{:#?}", report.diagnostics);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    assert_eq!(report.format_version.as_deref(), Some("3.5.0"));
}

#[test]
fn command_costs_require_format_3_5() {
    let source = format_3_5_command_costs_story().replacen(
        "format_version: \"3.5.0\"",
        "format_version: \"3.4.0\"",
        1,
    );
    let report = report(source);
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "command_costs.format_incompatible"
            && diagnostic.pointer.as_deref() == Some("/command_costs")
    }));
}

#[test]
fn command_costs_reject_unknown_command() {
    let source = format_3_5_command_costs_story().replace(
        "command: command.investigate",
        "command: command.nonexistent",
    );
    let report = report(source);
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "command_costs.command_unknown")
        .expect("unknown command diagnostic");
    assert_eq!(
        diagnostic.pointer.as_deref(),
        Some("/command_costs/0/command")
    );
    assert_eq!(
        diagnostic.subject_id.as_deref(),
        Some("cost.investigate_knife")
    );
    assert!(diagnostic.message.contains("command.nonexistent"));
}

#[test]
fn command_costs_reject_unknown_target() {
    let source = format_3_5_command_costs_story().replace(
        "target: entity.knife\n    minutes: 8",
        "target: entity.nonexistent\n    minutes: 8",
    );
    let report = report(source);
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "command_costs.target_unknown")
        .expect("unknown target diagnostic");
    assert_eq!(
        diagnostic.pointer.as_deref(),
        Some("/command_costs/0/target")
    );
    assert!(diagnostic.message.contains("entity.nonexistent"));
}

#[test]
fn command_costs_reject_mismatched_target_kind() {
    let source = format_3_5_command_costs_story().replace(
        "target: entity.knife\n    minutes: 8",
        "target: character.victim\n    minutes: 8",
    );
    let report = report(source);
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "command_costs.target_kind_mismatch")
        .expect("target kind mismatch diagnostic");
    assert_eq!(
        diagnostic.pointer.as_deref(),
        Some("/command_costs/0/target")
    );
    assert!(diagnostic.message.contains("character.victim"));
    assert!(diagnostic.message.contains("entity"));
}

#[test]
fn command_costs_reject_duplicate_command_target_pairs() {
    let source = format_3_5_command_costs_story().replace(
        "command_costs:\n  - id: cost.investigate_knife\n    command: command.investigate\n    target: entity.knife\n    minutes: 8\n",
        "command_costs:\n  - id: cost.investigate_knife\n    command: command.investigate\n    target: entity.knife\n    minutes: 8\n  - id: cost.investigate_knife_again\n    command: command.investigate\n    target: entity.knife\n    minutes: 3\n",
    );
    let report = report(source);
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "command_costs.duplicate_pair")
        .expect("duplicate pair diagnostic");
    assert_eq!(
        diagnostic.subject_id.as_deref(),
        Some("cost.investigate_knife_again")
    );
    assert_eq!(diagnostic.related.len(), 1);
    assert_eq!(diagnostic.related[0].message, "first defined here");
}

#[test]
fn command_costs_reject_out_of_bounds_minutes() {
    let source = format_3_5_command_costs_story().replace("minutes: 8", "minutes: -1");
    let report = report(source);
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "command_costs.minutes_invalid")
        .expect("minutes invalid diagnostic");
    assert_eq!(
        diagnostic.pointer.as_deref(),
        Some("/command_costs/0/minutes")
    );
}

#[test]
fn command_costs_reject_command_move_override() {
    let source = format_3_5_command_costs_story().replace(
        "command_costs:\n  - id: cost.investigate_knife\n    command: command.investigate\n    target: entity.knife\n    minutes: 8\n",
        "command_costs:\n  - id: cost.move_override\n    command: command.move\n    target: route.foyer_study\n    minutes: 8\n",
    );
    let report = report(source);
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "command_costs.command_move_disallowed")
        .expect("command.move disallowed diagnostic");
    assert_eq!(
        diagnostic.pointer.as_deref(),
        Some("/command_costs/0/command")
    );
}

fn format_3_6_players_story() -> String {
    include_str!("fixtures/format-3.6-players-story.yaml").to_string()
}

#[test]
fn players_description_and_personas_validate_end_to_end() {
    let report = report(format_3_6_players_story());
    assert!(report.valid, "{:#?}", report.diagnostics);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    assert_eq!(report.format_version.as_deref(), Some("3.6.0"));
}

#[test]
fn players_description_and_personas_require_format_3_6() {
    let source = format_3_6_players_story().replacen(
        "format_version: \"3.6.0\"",
        "format_version: \"3.5.0\"",
        1,
    );
    let report = report(source);
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "case.players_personas_format_incompatible")
        .expect("versioned personas diagnostic");
    assert_eq!(diagnostic.pointer.as_deref(), Some("/case/players"));
    assert!(diagnostic.message.contains("3.6.0"));
}

#[test]
fn player_condition_predicate_requires_format_3_6() {
    let source = format_3_6_players_story()
        .replacen("format_version: \"3.6.0\"", "format_version: \"3.5.0\"", 1)
        // Drop the case-level fields that are already versioned-gated so
        // this test isolates the `when.all` predicate gating.
        .replacen(
            "    description: One of you is secretly the detective; the rest play witnesses.\n    personas:\n      - id: persona.detective\n        name: The Detective\n        description: Leads the investigation and asks the questions.\n        narrator_guidance: Address this player directly when revealing deductions.\n      - id: persona.witness\n        name: A Witness\n        description: Has firsthand knowledge of the night's events.\n",
            "",
            1,
        );
    let report = report(source);
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "condition.player_format_incompatible")
        .expect("versioned player-predicate diagnostic");
    assert!(diagnostic.message.contains("3.6.0"));
}

#[test]
fn duplicate_persona_ids_are_rejected() {
    let source = format_3_6_players_story().replace(
        "      - id: persona.witness\n        name: A Witness\n        description: Has firsthand knowledge of the night's events.\n",
        "      - id: persona.detective\n        name: Also The Detective\n",
    );
    let report = report(source);
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| {
            diagnostic.code == "id.duplicate"
                && diagnostic.subject_id.as_deref() == Some("persona.detective")
        })
        .expect("duplicate persona id diagnostic");
    assert_eq!(
        diagnostic.pointer.as_deref(),
        Some("/case/players/personas/1/id")
    );
}

#[test]
fn empty_persona_name_is_rejected() {
    let source =
        format_3_6_players_story().replace("        name: The Detective\n", "        name: \"\"\n");
    let report = report(source);
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "case.players_persona_name")
        .expect("empty persona name diagnostic");
    assert_eq!(
        diagnostic.pointer.as_deref(),
        Some("/case/players/personas/0/name")
    );
}

#[test]
fn personas_cannot_exceed_players_max() {
    let source = format_3_6_players_story().replace("    max: 4\n", "    max: 1\n");
    let report = report(source);
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "case.players_personas_max")
        .expect("personas over max diagnostic");
    assert_eq!(
        diagnostic.pointer.as_deref(),
        Some("/case/players/personas")
    );
    assert!(diagnostic.message.contains("max"));
}

#[test]
fn persona_id_colliding_with_another_namespace_is_rejected() {
    let source = format_3_6_players_story().replace(
        "  - id: character.victim\n    description: The victim at the center of the mystery.\n",
        "  - id: persona.detective\n    description: The victim at the center of the mystery.\n",
    );
    let report = report(source);
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "id.duplicate"
            && diagnostic.subject_id.as_deref() == Some("persona.detective")));
}

#[test]
fn trigger_when_can_reference_a_declared_persona() {
    let source = format_3_6_players_story();
    let report = report(source);
    assert!(report.valid, "{:#?}", report.diagnostics);
}

#[test]
fn trigger_when_rejects_undeclared_persona() {
    let source = format_3_6_players_story().replace(
        "player: persona.detective\n    effects:",
        "player: persona.undeclared\n    effects:",
    );
    let report = report(source);
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| {
            diagnostic.code == "reference.unknown"
                && diagnostic.subject_id.as_deref() == Some("persona.undeclared")
        })
        .expect("undeclared persona reference diagnostic");
    assert!(diagnostic
        .pointer
        .as_deref()
        .unwrap()
        .contains("/triggers/"));
}

#[test]
fn trigger_when_rejects_out_of_range_player_slot() {
    let source = format_3_6_players_story().replace(
        "player: persona.detective\n    effects:",
        "player: player.9\n    effects:",
    );
    let report = report(source);
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| {
            diagnostic.code == "reference.unknown"
                && diagnostic.subject_id.as_deref() == Some("player.9")
        })
        .expect("out-of-range player slot diagnostic");
    assert!(diagnostic
        .pointer
        .as_deref()
        .unwrap()
        .contains("/triggers/"));
}

#[test]
fn trigger_when_accepts_in_range_player_slot() {
    let source = format_3_6_players_story().replace(
        "player: persona.detective\n    effects:",
        "player: player.2\n    effects:",
    );
    let report = report(source);
    assert!(report.valid, "{:#?}", report.diagnostics);
}

#[test]
fn persona_gated_fact_does_not_block_playability_when_solution_does_not_need_it() {
    // The story's `win.solve_case` end state does not require the
    // persona-gated `fact.detective_hunch`. A supported path to it exists
    // (found by the deterministic search), so the analyzer must never
    // report it as unwinnable (`NotProved`); consistent with every other
    // globally-unsupported predicate, the presence of the persona
    // condition elsewhere in the story is allowed to make the analyzer
    // cautious (`Inconclusive`) but must not flip an otherwise-reachable
    // path to a hard failure.
    let report = report(format_3_6_players_story());
    assert!(report.valid, "{:#?}", report.diagnostics);
    let analysis = report.playability.expect("format 3 playability report");
    let policy = &analysis.notebook_policies[0];
    let win = policy
        .terminal_paths
        .iter()
        .find(|path| path.id == "win.solve_case")
        .expect("win state terminal path");
    assert_ne!(
        win.status,
        narrator_validator::PlayabilityStatus::NotProved,
        "{win:#?}"
    );
}

#[test]
fn persona_gated_fact_required_by_solution_reports_a_clear_playability_diagnostic() {
    let source = format_3_6_players_story().replace(
        "end_states:\n  - id: win.solve_case\n    name: The whole truth\n    outcome: won\n    resolution: full\n    text: You answer every question and explain the complete case.\n",
        "end_states:\n  - id: win.solve_case\n    name: The whole truth\n    outcome: won\n    resolution: full\n    text: You answer every question and explain the complete case.\n  - id: end.detective_insight\n    name: The detective's private insight\n    outcome: won\n    resolution: partial\n    requires: [fact.detective_hunch]\n    minimum_points: 0\n    text: The detective alone puts it together.\n",
    );
    let report = report(source);
    assert!(report.valid, "{:#?}", report.diagnostics);
    let analysis = report.playability.expect("format 3 playability report");
    let policy = &analysis.notebook_policies[0];
    let detective_only = policy
        .terminal_paths
        .iter()
        .find(|path| path.id == "end.detective_insight")
        .expect("persona-gated end state terminal path");
    assert_ne!(
        detective_only.status,
        narrator_validator::PlayabilityStatus::Proved,
        "a persona-gated fact must not be provably reachable by the default, persona-less playthrough: {detective_only:#?}"
    );
    let blocker = detective_only
        .blocker
        .as_ref()
        .expect("unreachable persona-gated end state carries a blocker");
    assert_eq!(blocker.code, "playability.unsupported_player_condition");
}

#[test]
fn format_3_3_authored_questions_are_private_exact_card_sets() {
    let report = report(format_3_3_question_story());
    assert!(report.valid, "{:#?}", report.diagnostics);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let prompt = report
        .reference_text
        .iter()
        .find(|field| field.pointer == "/solution/questions/0/prompt")
        .expect("solution question prompt is a registered consumer");
    assert_eq!(
        prompt.resolved,
        "Who killed The victim at the center of the mystery.?"
    );
    assert_eq!(
        prompt.provenance[0].expression.target_id,
        "character.victim"
    );
    assert!(report
        .reference_text
        .iter()
        .all(|field| !field.pointer.contains("/answer")));
}

#[test]
fn format_3_3_accepts_one_unordered_question_with_default_policy() {
    let source = format_3_3_question_story().replace(
        "    - prompt: Which cards identify the weapon and location?\n      answer: [entity.knife, setting.study]\n      ordered: true\n",
        "",
    );
    let report = report(source);
    assert!(report.valid, "{:#?}", report.diagnostics);
}

#[test]
fn format_3_3_solution_questions_enforce_bounds_uniqueness_deck_and_types() {
    let source = format_3_3_question_story()
        .replace(
            "answer: [character.culprit]",
            "answer: [character.culprit, character.culprit]",
        )
        .replace(
            "answer: [entity.knife, setting.study]",
            "answer: [character.culprit, deduction.solution, command.solve]",
        );
    let report = report(source);
    for (code, pointer) in [
        (
            "solution.question_answer_duplicate",
            "/solution/questions/0/answer/1",
        ),
        ("solution.answer_reused", "/solution/questions/1/answer/0"),
        (
            "solution.question_answer_type",
            "/solution/questions/1/answer/1",
        ),
        (
            "solution.question_answer_type",
            "/solution/questions/1/answer/2",
        ),
    ] {
        assert!(
            report.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == code && diagnostic.pointer.as_deref() == Some(pointer)
            }),
            "missing {code} at {pointer}: {:#?}",
            report.diagnostics
        );
    }
}

#[test]
fn format_3_3_solution_questions_reject_every_shape_and_scanner_limit_violation() {
    let cases = [
        (
            format_3_3_question_story().replace(
                "  questions:\n    - prompt: Who killed",
                "  questions: []\n  ignored_questions:\n    - prompt: Who killed",
            ),
            "solution.questions_count",
            "/solution/questions",
        ),
        (
            format_3_3_question_story().replace(
                "prompt: Who killed [[character.victim.description]]?",
                "prompt: ''",
            ),
            "solution.question_prompt",
            "/solution/questions/0/prompt",
        ),
        (
            format_3_3_question_story().replace(
                "answer: [character.culprit]",
                "answer: []",
            ),
            "solution.question_answer_count",
            "/solution/questions/0/answer",
        ),
        (
            format_3_3_question_story().replace("ordered: true", "ordered: first"),
            "solution.question_ordered_type",
            "/solution/questions/1/ordered",
        ),
        (
            format_3_3_question_story().replace(
                "      ordered: true",
                "      ordered: true\n      explanation: private",
            ),
            "solution.question.unknown_field",
            "/solution/questions/1/explanation",
        ),
        (
            format_3_3_question_story().replace(
                "answer: [entity.knife, setting.study]",
                "answer: [entity.knife, setting.study, character.victim, character.culprit, entity.knife, setting.study]",
            ),
            "solution.question_answer_count",
            "/solution/questions/1/answer",
        ),
        (
            format_3_3_question_story().replace(
                "answer: [character.culprit]",
                "answer: [character.unknown]",
            ),
            "solution.question_answer_unknown",
            "/solution/questions/0/answer/0",
        ),
        (
            format_3_3_question_story().replace(
                "  - tag_id: 4\n    subject: entity.knife\n",
                "",
            ),
            "solution.question_answer_not_in_deck",
            "/solution/questions/1/answer/0",
        ),
    ];
    for (source, code, pointer) in cases {
        let report = report(source);
        assert!(
            report.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == code && diagnostic.pointer.as_deref() == Some(pointer)
            }),
            "missing {code} at {pointer}: {:#?}",
            report.diagnostics
        );
    }

    let five_questions = report(format_3_3_question_story().replace(
        "      ordered: true",
        "      ordered: true\n    - { prompt: Third?, answer: [character.victim] }\n    - { prompt: Fourth?, answer: [setting.foyer] }\n    - { prompt: Fifth?, answer: [entity.knife] }",
    ));
    assert!(five_questions.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "solution.questions_count"
            && diagnostic.pointer.as_deref() == Some("/solution/questions")
    }));
}

#[test]
fn format_3_3_solution_win_state_is_exclusive_and_ruleset_requires_the_contract() {
    let conditional = report(format_3_3_question_story().replace(
        "    text: You answer every question correctly.",
        "    text: You answer every question correctly.\n    requires: [deduction.solution]",
    ));
    assert!(conditional.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "win_states.solution_condition_conflict"
            && diagnostic.pointer.as_deref() == Some("/win_states/0")
    }));

    let missing = report(format_3_3_question_story().replace(
        "solution:\n  win_state: win.solve_case\n  questions:\n    - prompt: Who killed [[character.victim.description]]?\n      answer: [character.culprit]\n    - prompt: Which cards identify the weapon and location?\n      answer: [entity.knife, setting.study]\n      ordered: true\n",
        "",
    ));
    assert!(missing.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "solution.missing_question_contract"
            && diagnostic.pointer.as_deref() == Some("/solution")
    }));

    for (replacement, code) in [
        ("win.missing", "solution.win_state_unknown"),
        ("character.culprit", "solution.win_state_type"),
    ] {
        let report = report(format_3_3_question_story().replace(
            "win_state: win.solve_case",
            &format!("win_state: {replacement}"),
        ));
        assert!(
            report.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == code
                    && diagnostic.pointer.as_deref() == Some("/solution/win_state")
            }),
            "missing {code}: {:#?}",
            report.diagnostics
        );
    }
}

#[test]
fn format_3_3_canonical_fixtures_match_native_and_cli_contracts() {
    let fixture_root = "tests/fixtures/format-3.3-solve-card-sets";
    let mut files = fs::read_dir(fixture_root)
        .expect("fixture directory")
        .map(|entry| {
            let entry = entry.expect("fixture entry");
            SourceFile {
                path: entry.file_name().to_string_lossy().into_owned(),
                source: fs::read_to_string(entry.path()).expect("fixture source"),
            }
        })
        .collect::<Vec<_>>();
    files.sort_by(|left, right| left.path.cmp(&right.path));
    let valid = validate(&files);
    assert!(valid.valid, "{:#?}", valid.diagnostics);

    let invalid_case =
        fs::read_to_string("tests/fixtures/format-3.3-solve-card-sets-invalid-case.yaml")
            .expect("invalid case fixture");
    files
        .iter_mut()
        .find(|file| file.path == "case.yaml")
        .expect("case fixture")
        .source = invalid_case;
    let invalid = validate(&files);
    for code in [
        "solution.contract_mixed",
        "solution.questions_count",
        "solution.question_prompt",
        "solution.question_answer_duplicate",
        "solution.question_answer_type",
        "solution.question_ordered_type",
        "solution.answer_reused",
        "solution.question_answer_count",
    ] {
        assert!(
            invalid
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == code),
            "missing {code}: {:#?}",
            invalid.diagnostics
        );
    }

    let output = Command::new(env!("CARGO_BIN_EXE_narrator-validator"))
        .arg("--format")
        .arg("json")
        .arg(fixture_root)
        .output()
        .expect("run CLI fixture validation");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let cli_report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("CLI JSON report");
    assert_eq!(cli_report["valid"], true);
    assert_eq!(cli_report["format_version"], "3.3.0");

    let action_output = Command::new(env!("CARGO_BIN_EXE_narrator-validator"))
        .arg("--format")
        .arg("github")
        .arg(fixture_root)
        .output()
        .expect("run action-format fixture validation");
    assert!(
        action_output.status.success(),
        "{}",
        String::from_utf8_lossy(&action_output.stderr)
    );
}

#[test]
fn format_3_3_answers_require_the_canonical_physical_deck() {
    let mut files = fs::read_dir("tests/fixtures/format-3.3-solve-card-sets")
        .expect("fixture directory")
        .map(|entry| {
            let entry = entry.expect("fixture entry");
            SourceFile {
                path: entry.file_name().to_string_lossy().into_owned(),
                source: fs::read_to_string(entry.path()).expect("fixture source"),
            }
        })
        .collect::<Vec<_>>();
    let deck = files
        .iter_mut()
        .find(|file| file.path == "deck.yaml")
        .expect("deck");
    deck.source = deck
        .source
        .replace("  - { tag_id: 2, subject: entity.knife }\n", "");
    files.push(SourceFile {
        path: "notes.yaml".to_string(),
        source: "cards:\n  - { tag_id: 99, subject: entity.knife }\n".to_string(),
    });
    let report = validate(&files);
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "solution.question_answer_not_in_deck"
            && diagnostic.pointer.as_deref() == Some("/solution/questions/1/answer/1")
    }));
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "schema.noncanonical_filename" && diagnostic.path == "notes.yaml"
    }));
}

#[test]
fn format_3_3_solution_contract_fails_closed_across_migration_boundaries() {
    let old_format = report(format_3_3_question_story().replace("3.3.0", "3.2.0"));
    assert!(old_format.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "ruleset.format_incompatible"
            && diagnostic.pointer.as_deref() == Some("/case/ruleset/version")
    }));

    let old_ruleset =
        report(format_3_3_question_story().replace("version: \"3.0.0\"", "version: \"2.0.0\""));
    assert!(old_ruleset.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "solution.ruleset_incompatible"
            && diagnostic.pointer.as_deref() == Some("/solution")
    }));

    let mixed = report(format_3_3_question_story().replace(
        "  win_state: win.solve_case",
        "  win_state: win.solve_case\n  culprit: character.culprit",
    ));
    assert!(mixed.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "solution.contract_mixed"
            && diagnostic.pointer.as_deref() == Some("/solution")
    }));

    let copied_legacy_solve = report(format_3_3_question_story().replace(
        "commands:\n",
        "commands:\n  - id: command.solve\n    name: Legacy Solve\n    parameters:\n      - { name: suspect, types: [character], min: 1, max: 1 }\n      - { name: theory, types: [deduction], min: 1, max: 1 }\n",
    ));
    assert!(copied_legacy_solve.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "ruleset.command_conflict"
            && diagnostic.subject_id.as_deref() == Some("command.solve")
    }));
}

#[test]
fn rejects_unknown_and_incompatible_rulesets_with_version_guidance() {
    let unknown = report(
        with_standard_ruleset(VALID_FORMAT_3_STORY)
            .replace("ruleset.standard_mystery", "ruleset.private_mystery"),
    );
    let diagnostic = unknown
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "ruleset.unsupported")
        .expect("unknown ruleset diagnostic");
    assert!(diagnostic.message.contains("supported rulesets"));

    let incompatible = report(
        with_standard_ruleset(VALID_FORMAT_3_STORY)
            .replace("version: \"1.0.0\"", "version: \"9.0.0\""),
    );
    let diagnostic = incompatible
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "ruleset.unsupported")
        .expect("incompatible ruleset diagnostic");
    assert!(diagnostic
        .message
        .contains("1.0.0, 2.0.0, 3.0.0, 4.0.0, 5.0.0, 6.0.0, or 7.0.0"));
}

#[test]
fn rejects_local_overrides_but_accepts_distinct_extension_ids() {
    let source = with_standard_ruleset(VALID_FORMAT_3_STORY).replace(
        "commands:\n",
        "commands:\n  - id: command.move\n    name: Story move\n    parameters: []\n",
    );
    assert!(codes(source).contains(&"ruleset.command_conflict".to_string()));
}

#[test]
fn diagnoses_copied_standard_catalogs_and_legacy_parameter_shapes() {
    let source = VALID_FORMAT_3_STORY.replace(
        "  - id: command.claim\n",
        "  - id: command.move\n    name: Move\n    parameters:\n      - name: destination\n        type: setting\n        required: true\n  - id: command.examine\n    name: Examine\n  - id: command.question\n    name: Question\n      \n  - id: command.claim\n",
    );
    let report = report(source);
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "ruleset.copied_standard_commands"));
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "ruleset.legacy_command_parameter"));
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "command.parameter_type_removed"));
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "command.parameter_required_removed"));
}

#[test]
fn validates_physical_deck_bindings() {
    let malformed = report(VALID_FORMAT_3_STORY.replace(
        "cards:\n  - tag_id: 0\n    subject: setting.foyer\n  - tag_id: 1\n    subject: setting.study\n  - tag_id: 2\n    subject: character.victim\n  - tag_id: 3\n    subject: character.culprit\n  - tag_id: 4\n    subject: entity.knife\n  - tag_id: 5\n    subject: command.claim\n  - tag_id: 6\n    subject: command.investigate\n",
        "cards: not-a-sequence\n",
    ));
    assert!(malformed.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "schema.section_type" && diagnostic.pointer.as_deref() == Some("/cards")
    }));

    for (value, code) in [
        ("-1", "deck.tag_id_out_of_range"),
        ("2115", "deck.tag_id_out_of_range"),
        ("1.5", "deck.tag_id_invalid"),
        ("not-a-number", "deck.tag_id_invalid"),
    ] {
        let report =
            report(VALID_FORMAT_3_STORY.replacen("tag_id: 4", &format!("tag_id: {value}"), 1));
        assert!(
            report.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == code && diagnostic.pointer.as_deref() == Some("/cards/4/tag_id")
            }),
            "{value}: {:#?}",
            report.diagnostics
        );
    }

    let duplicate = report(VALID_FORMAT_3_STORY.replacen("tag_id: 4", "tag_id: 0", 1))
        .diagnostics
        .into_iter()
        .find(|diagnostic| diagnostic.code == "deck.tag_id_duplicate")
        .expect("duplicate tag ID diagnostic");
    assert_eq!(duplicate.pointer.as_deref(), Some("/cards/4/tag_id"));
    assert_eq!(
        duplicate.related[0].pointer.as_deref(),
        Some("/cards/0/tag_id")
    );

    for (subject, code) in [
        ("entity.unknown", "deck.subject_unknown"),
        ("event.murder", "deck.subject_unsupported"),
    ] {
        let report = report(VALID_FORMAT_3_STORY.replacen(
            "subject: entity.knife",
            &format!("subject: {subject}"),
            1,
        ));
        assert!(
            report.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == code && diagnostic.pointer.as_deref() == Some("/cards/4/subject")
            }),
            "{subject}: {:#?}",
            report.diagnostics
        );
    }

    let uncarded =
        report(VALID_FORMAT_3_STORY.replace("  - tag_id: 4\n    subject: entity.knife\n", ""));
    assert!(uncarded.valid, "{:#?}", uncarded.diagnostics);

    let legacy = report(VALID_FORMAT_3_STORY.replace(
        "  - id: entity.knife\n",
        "  - id: entity.knife\n    tag_id: 42\n",
    ));
    assert!(legacy.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "deck.legacy_inline_tag_id"
            && diagnostic.pointer.as_deref() == Some("/entities/0/tag_id")
            && diagnostic.message.contains("deck.yaml")
    }));
}

#[test]
fn rejects_deck_bindings_of_reserved_enter_scanner_control_ids() {
    use narrator_validator::{scanner_control_role_for_tag_id, RESERVED_SCANNER_CONTROL_TAG_IDS};

    // Guard against a future refactor silently checking zero reservations.
    assert_eq!(
        RESERVED_SCANNER_CONTROL_TAG_IDS.len(),
        2,
        "expected exactly two reserved scanner-control tag IDs (ENTER-1, ENTER-2)"
    );

    for (role, tag_id) in RESERVED_SCANNER_CONTROL_TAG_IDS {
        let source = VALID_FORMAT_3_STORY.replacen("tag_id: 4", &format!("tag_id: {tag_id}"), 1);
        let report = report(source);
        let diagnostic = report
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == "deck.tag_id_reserved_scanner_control")
            .unwrap_or_else(|| {
                panic!(
                    "expected reserved-ID diagnostic for {tag_id}: {:#?}",
                    report.diagnostics
                )
            });
        assert_eq!(diagnostic.pointer.as_deref(), Some("/cards/4/tag_id"));
        assert!(
            diagnostic.message.contains(role.name()),
            "diagnostic should name the reserved role {}: {}",
            role.name(),
            diagnostic.message
        );
    }

    // Adjacent, non-reserved IDs continue to validate normally.
    let lowest_reserved = RESERVED_SCANNER_CONTROL_TAG_IDS
        .iter()
        .map(|(_, id)| *id)
        .min()
        .unwrap();
    for tag_id in [lowest_reserved - 1, 0] {
        assert!(scanner_control_role_for_tag_id(tag_id).is_none());
        let source = VALID_FORMAT_3_STORY.replacen("tag_id: 4", &format!("tag_id: {tag_id}"), 1);
        let report = report(source);
        assert!(
            !report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "deck.tag_id_reserved_scanner_control"),
            "{tag_id} should not be rejected as reserved: {:#?}",
            report.diagnostics
        );
    }
}

#[test]
fn checked_in_scanner_control_artifacts_match_the_source() {
    let generated_json = narrator_validator::scanner_control_manifest_json();
    let checked_in_json = fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/scanner-controls.json"),
    )
    .expect("docs/scanner-controls.json must exist");
    assert_eq!(
        checked_in_json.trim_end(),
        generated_json.trim_end(),
        "docs/scanner-controls.json is out of date -- regenerate with:\n\
         cargo run --example generate_scanner_controls_json > docs/scanner-controls.json"
    );

    let generated_dart = narrator_validator::scanner_control_dart_source();
    let checked_in_dart = fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/scanner_control.dart"),
    )
    .expect("docs/scanner_control.dart must exist");
    assert_eq!(
        checked_in_dart.trim_end(),
        generated_dart.trim_end(),
        "docs/scanner_control.dart is out of date -- regenerate with:\n\
         cargo run --example generate_scanner_controls_dart > docs/scanner_control.dart"
    );
}

#[test]
fn accepts_semantically_compatible_story_format_versions() {
    for version in ["1.7.3", "3.4.1"] {
        let source = if version.starts_with('1') {
            VALID_STORY.replace("\"1.0.0\"", &format!("\"{version}\""))
        } else {
            format_3_3_question_story().replacen("\"3.3.0\"", &format!("\"{version}\""), 1)
        };
        let report = report(source);
        assert!(report.valid, "{version}: {:#?}", report.diagnostics);
        assert_eq!(report.format_version.as_deref(), Some(version));
    }
}

#[test]
fn rejects_older_and_newer_incompatible_story_formats_with_migration_guidance() {
    for (version, expected_text) in [
        ("0.9.0", "too old"),
        ("2.0.0", "pre-migration"),
        ("4.0.0", "newer"),
    ] {
        let source = VALID_FORMAT_3_STORY.replace("\"3.0.0\"", &format!("\"{version}\""));
        let report = report(source);
        assert!(!report.valid);
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(report.diagnostics[0].code, "format.incompatible_version");
        assert!(report.diagnostics[0].message.contains(expected_text));
    }
}

#[test]
fn pre_v3_demo_layouts_fail_once_with_focused_migration_guidance() {
    for fixture in ["simple-mystery.yaml", "island-retreat.yaml"] {
        let report = validate(&[SourceFile {
            path: "settings.yaml".to_string(),
            source: fs::read_to_string(format!("tests/fixtures/pre-v3/{fixture}"))
                .expect("migration fixture"),
        }]);
        assert!(!report.valid, "{fixture}");
        assert_eq!(
            report.diagnostics.len(),
            1,
            "{fixture}: {:#?}",
            report.diagnostics
        );
        assert_eq!(report.diagnostics[0].code, "format.incompatible_version");
        assert_eq!(
            report.diagnostics[0].pointer.as_deref(),
            Some("/case/format_version")
        );
        assert!(report.diagnostics[0].message.contains("MIGRATION.md"));
        assert!(report.diagnostics[0].message.contains("v1.0.0"));
    }
}

#[test]
fn format_3_rejects_unknown_fields_on_every_known_item_kind() {
    let mutations = [
        ("    max: 4\n", "    max: 4\n  titel: typo\n", "case.unknown_field", "/case/titel"),
        ("  deduction: deduction.solution\n", "  deduction: deduction.solution\n  methd: typo\n", "solution.unknown_field", "/solution/methd"),
        ("    description: The entry foyer.\n", "    description: The entry foyer.\n    parnt: setting.world\n", "setting.unknown_field", "/settings/1/parnt"),
        ("    travel_minutes: 1\n", "    travel_minutes: 1\n    travle_minutes: 1\n", "route.unknown_field", "/routes/0/travle_minutes"),
        ("    description: The victim at the center of the mystery.\n", "    description: The victim at the center of the mystery.\n    ocupation: author only\n", "character.unknown_field", "/characters/0/ocupation"),
        ("    description: A knife found in the study.\n", "    description: A knife found in the study.\n    examinined: hidden prose\n", "entity.unknown_field", "/entities/0/examinined"),
        ("    duration_minutes: 0\n", "    duration_minutes: 0\n    sumar: hidden prose\n", "event.unknown_field", "/events/0/sumar"),
        ("        statement: The knife is present.\n", "        statement: The knife is present.\n        narative_detail: typo\n", "fact.unknown_field", "/entities/0/facts/0/narative_detail"),
        ("    truth: true\n", "    truth: true\n    conclsion_note: typo\n", "deduction.unknown_field", "/deductions/0/conclsion_note"),
        ("    initial_state: false\n", "    initial_state: false\n    initital_state: false\n", "flag.unknown_field", "/flags/0/initital_state"),
        ("    name: Claim\n", "    name: Claim\n    alaises: [claim]\n", "command.unknown_field", "/commands/0/alaises"),
        ("    name: Investigate the knife\n", "    name: Investigate the knife\n    conditons: []\n", "trigger.unknown_field", "/triggers/0/conditons"),
    ];
    for (needle, replacement, code, pointer) in mutations {
        let report = report(VALID_FORMAT_3_STORY.replacen(needle, replacement, 1));
        assert!(
            report.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == code && diagnostic.pointer.as_deref() == Some(pointer)
            }),
            "missing {code} at {pointer}: {:#?}",
            report.diagnostics
        );
    }
}

#[test]
fn format_3_requires_typed_player_limits_and_baseline_safe_descriptions() {
    for (players, code, pointer) in [
        ("1-4", "case.players_type", "/case/players"),
        ("{min: 0, max: 4}", "case.players_min", "/case/players/min"),
        ("{min: 5, max: 4}", "case.players_order", "/case/players"),
        (
            "{min: 1, max: many}",
            "case.players_max",
            "/case/players/max",
        ),
    ] {
        let source = VALID_FORMAT_3_STORY.replace(
            "  players:\n    min: 1\n    max: 4",
            &format!("  players: {players}"),
        );
        let report = report(source);
        assert!(
            report.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == code && diagnostic.pointer.as_deref() == Some(pointer)
            }),
            "{players}: {:#?}",
            report.diagnostics
        );
    }

    for (line, code, pointer) in [
        (
            "    description: The entry foyer.\n",
            "setting.description",
            "/settings/1/description",
        ),
        (
            "    description: The victim at the center of the mystery.\n",
            "character.description",
            "/characters/0/description",
        ),
        (
            "    description: A knife found in the study.\n",
            "entity.description",
            "/entities/0/description",
        ),
    ] {
        let report = report(VALID_FORMAT_3_STORY.replacen(line, "", 1));
        assert!(
            report.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == code && diagnostic.pointer.as_deref() == Some(pointer)
            }),
            "{code}: {:#?}",
            report.diagnostics
        );
    }
}

#[test]
fn format_3_separates_narrator_guidance_from_open_author_notes() {
    let accepted = report(VALID_FORMAT_3_STORY.replace(
        "    description: A suspect with a carefully guarded secret.\n",
        "    description: A suspect with a carefully guarded secret.\n    narrator_guidance:\n      goal: Keep the truth hidden.\n      testimony_guidance:\n        default: Give the safe ordered testimony.\n    author_notes:\n      research_link: https://example.invalid/private\n",
    ));
    assert!(accepted.valid, "{:#?}", accepted.diagnostics);

    let rejected = report(VALID_FORMAT_3_STORY.replace(
        "    description: A suspect with a carefully guarded secret.\n",
        "    description: A suspect with a carefully guarded secret.\n    narrator_guidance:\n      testimony: Ambiguous private testimony.\n",
    ));
    assert!(rejected.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "narrator_guidance.unknown_field"
            && diagnostic.pointer.as_deref() == Some("/characters/1/narrator_guidance/testimony")
    }));
}

#[test]
fn rejects_legacy_integer_story_formats_before_schema_validation() {
    let report = report(VALID_FORMAT_3_STORY.replace("\"3.0.0\"", "2"));
    assert!(!report.valid);
    assert_eq!(report.diagnostics.len(), 1);
    assert_eq!(report.diagnostics[0].code, "format.incompatible_version");
    assert!(report.diagnostics[0]
        .message
        .contains("legacy format version 2"));
}

#[test]
fn rejects_missing_story_format_with_friendly_migration_guidance() {
    let report = report(VALID_STORY.replace("  format_version: \"1.0.0\"\n", ""));
    assert!(!report.valid);
    assert_eq!(report.diagnostics.len(), 1);
    assert_eq!(report.diagnostics[0].code, "format.version_missing");
    assert!(report.diagnostics[0].message.contains("cannot safely open"));
}

#[test]
fn generic_win_states_allow_a_non_murder_story_and_preserve_authored_precedence() {
    let source = VALID_FORMAT_3_STORY
        .replace(
            "solution:\n  victim: character.victim\n  culprit: character.culprit\n  weapon: entity.knife\n  location: setting.study\n  deduction: deduction.solution\n",
            "",
        )
        .replace(
            "settings:\n",
            "win_states:\n  - id: win.escape\n    name: Escaped the house\n    requires: [flag.knife_examined, entity.knife]\n    minimum_points: 50\n    text: You reach the road.\n  - id: win.solve\n    name: Solved the case\n    requires: [deduction.solution]\n    minimum_points: 20\n    text: You explain the answer.\nsettings:\n",
        );
    let report = report(source);
    assert!(report.valid, "{:#?}", report.diagnostics);
}

#[test]
fn format_3_4_end_states_express_full_partial_deadline_and_condition_outcomes() {
    let report = report(format_3_4_end_state_story());
    assert!(report.valid, "{:#?}", report.diagnostics);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    assert_eq!(report.format_version.as_deref(), Some("3.4.0"));
}

#[test]
fn ruleset_5_allows_only_persistent_requirements_on_the_solution_target() {
    for requirement in [
        "flag.knife_examined",
        "fact.knife_is_present",
        "deduction.solution",
    ] {
        let valid = report(format_3_4_ruleset_5_solution_prerequisite_story().replacen(
            "    requires: [flag.knife_examined]\n    text: You answer every question",
            &format!("    requires: [{requirement}]\n    text: You answer every question"),
            1,
        ));
        assert!(valid.valid, "{requirement}: {:#?}", valid.diagnostics);
    }

    for requirement in ["setting.study", "entity.knife", "trigger.investigate_knife"] {
        let invalid = report(format_3_4_ruleset_5_solution_prerequisite_story().replacen(
            "    requires: [flag.knife_examined]\n    text: You answer every question",
            &format!("    requires: [{requirement}]\n    text: You answer every question"),
            1,
        ));
        assert!(
            invalid.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "end_states.solution_requirement_kind"
                    && diagnostic.pointer.as_deref() == Some("/end_states/0/requires/0")
            }),
            "missing precise unsupported-kind diagnostic for {requirement}: {:#?}",
            invalid.diagnostics
        );
    }

    let ruleset_4 = report(format_3_4_ruleset_5_solution_prerequisite_story().replacen(
        "version: \"5.0.0\"",
        "version: \"4.0.0\"",
        1,
    ));
    assert!(ruleset_4.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "end_states.solution_condition_conflict"
            && diagnostic.pointer.as_deref() == Some("/end_states/0")
    }));

    for (field, pointer) in [
        ("    minimum_points: 0\n", "/end_states/0/minimum_points"),
        ("    minimum_points: 1\n", "/end_states/0/minimum_points"),
        ("    at_or_after: \"21:00\"\n", "/end_states/0/at_or_after"),
    ] {
        let invalid = report(format_3_4_ruleset_5_solution_prerequisite_story().replacen(
            "    requires: [flag.knife_examined]\n",
            &format!("    requires: [flag.knife_examined]\n{field}"),
            1,
        ));
        assert!(
            invalid.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "end_states.solution_condition_conflict"
                    && diagnostic.pointer.as_deref() == Some(pointer)
            }),
            "missing precise {pointer}: {:#?}",
            invalid.diagnostics
        );
    }
}

#[test]
fn ruleset_6_0_allows_only_persistent_requirements_on_the_solution_target() {
    let source = format_3_4_ruleset_5_solution_prerequisite_story().replacen(
        "version: \"5.0.0\"",
        "version: \"6.0.0\"",
        1,
    );

    let valid = report(source.clone());
    assert!(valid.valid, "{:#?}", valid.diagnostics);

    let invalid = report(source.replacen(
        "    requires: [flag.knife_examined]\n    text: You answer every question",
        "    requires: [entity.knife]\n    text: You answer every question",
        1,
    ));
    assert!(
        invalid.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "end_states.solution_requirement_kind"
                && diagnostic.pointer.as_deref() == Some("/end_states/0/requires/0")
        }),
        "missing precise unsupported-kind diagnostic: {:#?}",
        invalid.diagnostics
    );
}

#[test]
fn format_3_4_end_state_contract_metadata_makes_precedence_and_migration_explicit() {
    let metadata = end_state_contract_metadata();
    assert_eq!(metadata.story_format_version, "3.4.0");
    assert_eq!(metadata.canonical_section, "end_states");
    assert_eq!(metadata.precedence, "authored_order_first_satisfied");
    assert_eq!(metadata.evaluation_timing, "after_every_resolved_turn");
    assert_eq!(metadata.score_semantics, "snapshot_and_minimum_gate");
    assert_eq!(metadata.legacy_outcome, "won");
    assert_eq!(metadata.legacy_resolution, "full");
}

#[test]
fn format_3_4_accepts_quiet_kennel_legacy_win_state_without_behavioral_reinterpretation() {
    let legacy = fs::read_to_string("tests/fixtures/quiet-kennel-legacy-win-states.yaml")
        .expect("Quiet Kennel legacy fixture");
    let source = format_3_3_question_story()
        .replacen("format_version: \"3.3.0\"", "format_version: \"3.4.0\"", 1)
        .replace("win_state: win.solve_case", "win_state: win.echo_safe_and_case_reconstructed")
        .replace(
            "win_states:\n  - id: win.solve_case\n    name: Solved the case\n    text: You answer every question correctly.\n",
            &legacy,
        );
    let report = report(source);
    assert!(report.valid, "{:#?}", report.diagnostics);
    assert_eq!(
        report
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "win_states.legacy_compatibility")
            .count(),
        1
    );
    let metadata = end_state_contract_metadata();
    assert_eq!(
        (metadata.legacy_outcome, metadata.legacy_resolution),
        ("won", "full")
    );
}

#[test]
fn format_3_4_end_states_validate_time_tier_duplicates_and_references() {
    let invalid = format_3_4_end_state_story()
        .replace("at_or_after: \"23:00\"", "at_or_after: \"25:00\"")
        .replace("resolution: partial", "resolution: failure")
        .replace("id: end.condition_failure", "id: end.deadline_failure")
        .replace(
            "requires: [flag.knife_analysis_complete]",
            "requires: [flag.unknown]",
        );
    let report = report(invalid);
    for (code, pointer) in [
        ("end_states.outcome_resolution_conflict", "/end_states/1"),
        ("end_states.at_or_after", "/end_states/2/at_or_after"),
        ("id.duplicate", "/end_states/3/id"),
        ("reference.unknown", "/end_states/3/requires/0"),
    ] {
        assert!(
            report.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == code && diagnostic.pointer.as_deref() == Some(pointer)
            }),
            "missing {code} at {pointer}: {:#?}",
            report.diagnostics
        );
    }
}

#[test]
fn format_3_4_solve_selected_end_state_must_be_a_win() {
    let report = report(format_3_4_end_state_story().replacen(
        "outcome: won\n    resolution: full",
        "outcome: lost\n    resolution: failure",
        1,
    ));
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "end_states.solution_outcome_conflict"
            && diagnostic.pointer.as_deref() == Some("/end_states/0/outcome")
    }));
}

#[test]
fn format_3_4_rejects_duplicate_and_deterministically_shadowed_precedence() {
    let duplicate = format_3_4_end_state_story().replace(
        "  - id: end.partial_truth\n    name: A defensible theory\n    outcome: won\n    resolution: partial\n    requires: [deduction.solution]\n    minimum_points: 20\n    text: You establish the central theory, but part of the case remains unresolved.\n  - id: end.deadline_failure\n    name: The trail goes cold\n    outcome: lost\n    resolution: failure\n    requires: [flag.knife_examined]\n    at_or_after: \"23:00\"\n    text: The deadline passes before you can preserve the evidence.\n",
        "  - id: end.partial_truth\n    name: A defensible theory\n    outcome: won\n    resolution: partial\n    requires: [flag.knife_examined]\n    text: You preserve part of the truth.\n  - id: end.deadline_failure\n    name: The trail goes cold\n    outcome: lost\n    resolution: failure\n    requires: [flag.knife_examined]\n    text: The evidence is lost.\n",
    );
    let duplicate_report = report(duplicate);
    assert!(duplicate_report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "end_states.duplicate_precedence"
            && diagnostic.pointer.as_deref() == Some("/end_states/2")
    }));

    let shadowed = format_3_4_end_state_story().replace(
        "    requires: [deduction.solution]\n    minimum_points: 20",
        "    requires: [flag.knife_examined]",
    );
    let shadowed_report = report(shadowed);
    assert!(shadowed_report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "end_states.unreachable_precedence"
            && diagnostic.pointer.as_deref() == Some("/end_states/2")
    }));
}

#[test]
fn format_3_4_allows_specific_full_resolution_before_broader_partial_resolution() {
    let canonical = fs::read_to_string("tests/fixtures/format-3.4-end-states.yaml")
        .expect("Format 3.4 end-state fixture");
    let correctly_ordered = r#"end_states:
  - id: win.solve_case
    name: The whole truth
    outcome: won
    resolution: full
    text: You answer every question and explain the complete case.
  - id: end.full_documented
    name: The documented truth
    outcome: won
    resolution: full
    requires: [flag.knife_examined, flag.knife_analysis_complete]
    minimum_points: 20
    text: You prove every material element.
  - id: end.partial_truth
    name: A defensible theory
    outcome: won
    resolution: partial
    requires: [flag.knife_examined]
    text: You establish only part of the truth.
"#;
    let source = format_3_4_end_state_story().replace(&canonical, correctly_ordered);
    let report = report(source);
    assert!(report.valid, "{:#?}", report.diagnostics);
    assert!(!report.diagnostics.iter().any(|diagnostic| {
        matches!(
            diagnostic.code.as_str(),
            "end_states.duplicate_precedence" | "end_states.unreachable_precedence"
        )
    }));
}

#[test]
fn ruleset_5_solution_prerequisite_native_and_cli_reports_are_in_parity() {
    let files = story_files(format_3_4_ruleset_5_solution_prerequisite_story());
    let native = validate(&files);
    assert!(native.valid, "{:#?}", native.diagnostics);

    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "narrator-validator-end-states-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir(&directory).unwrap();
    for file in &files {
        fs::write(directory.join(&file.path), &file.source).unwrap();
    }
    let output = Command::new(env!("CARGO_BIN_EXE_narrator-validator"))
        .arg("--format")
        .arg("json")
        .arg(&directory)
        .output()
        .expect("run Format 3.4 CLI validation");
    fs::remove_dir_all(&directory).unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let cli: narrator_validator::ValidationReport =
        serde_json::from_slice(&output.stdout).expect("CLI report JSON");
    assert_eq!(cli, native);
}

#[test]
fn win_states_validate_shape_requirement_kinds_and_thresholds() {
    let source = VALID_FORMAT_3_STORY.replace(
        "settings:\n",
        "win_states:\n  - id: win.invalid\n    name: ''\n    requires: [character.victim, setting.unknown]\n    minimum_points: -1\n    text: ''\n    secret: never expose this\nsettings:\n",
    );
    let report = report(source);
    let diagnostics = report
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.pointer.as_deref()))
        .collect::<Vec<_>>();
    for expected in [
        ("win_states.name", Some("/win_states/0/name")),
        ("win_states.text", Some("/win_states/0/text")),
        (
            "win_states.minimum_points",
            Some("/win_states/0/minimum_points"),
        ),
        ("win_states.unknown_field", Some("/win_states/0/secret")),
        ("reference.wrong_type", Some("/win_states/0/requires/0")),
        ("reference.unknown", Some("/win_states/0/requires/1")),
    ] {
        assert!(
            diagnostics.contains(&expected),
            "missing {expected:?}: {diagnostics:#?}"
        );
    }
}

#[test]
fn repository_requires_a_generic_or_legacy_terminal_configuration() {
    let source = VALID_FORMAT_3_STORY.replace(
        "solution:\n  victim: character.victim\n  culprit: character.culprit\n  weapon: entity.knife\n  location: setting.study\n  deduction: deduction.solution\n",
        "",
    );
    let report = report(source);
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "win_states.missing_terminal_configuration"
            && diagnostic.pointer.as_deref() == Some("/win_states")
    }));
}

#[test]
fn win_states_must_use_the_canonical_root_filename() {
    let source = SourceFile {
        path: "goals.yaml".to_string(),
        source: "win_states:\n  - id: win.escape\n    name: Escape\n    text: You escape.\n"
            .to_string(),
    };
    let mut files = story_files(VALID_FORMAT_3_STORY.to_string());
    files.push(source);
    let report = validate(&files);
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "schema.noncanonical_filename"
            && diagnostic.path == "goals.yaml"
            && diagnostic.pointer.as_deref() == Some("/win_states")
    }));
}

#[test]
fn end_states_require_the_canonical_file_and_cannot_mix_with_legacy_precedence() {
    let mut misplaced = story_files(format_3_4_end_state_story());
    misplaced
        .iter_mut()
        .find(|file| file.path == "end_states.yaml")
        .expect("end states")
        .path = "goals.yaml".to_string();
    let misplaced_report = validate(&misplaced);
    assert!(misplaced_report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "schema.noncanonical_filename"
            && diagnostic.path == "goals.yaml"
            && diagnostic.pointer.as_deref() == Some("/end_states")
    }));

    let mixed = format_3_4_end_state_story().replace(
        "settings:\n",
        "win_states:\n  - id: win.legacy\n    name: Legacy\n    requires: [flag.knife_examined]\n    text: Legacy ending.\nsettings:\n",
    );
    let mixed_report = report(mixed);
    assert!(mixed_report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "end_states.mixed_legacy_section"
            && diagnostic.pointer.as_deref() == Some("/end_states")
    }));
}

#[test]
fn point_awards_are_valid_on_every_supported_owner_with_default_claim_count() {
    let source = VALID_FORMAT_3_STORY
        .replace(
            "  - id: setting.foyer\n    type: room",
            "  - id: setting.foyer\n    type: room\n    points:\n      value: 5",
        )
        .replace(
            "  - id: entity.knife\n    initial:",
            "  - id: entity.knife\n    points:\n      value: 10\n      requires: [setting.study]\n    initial:",
        )
        .replace(
            "  - id: deduction.solution\n    conclusion:",
            "  - id: deduction.solution\n    points:\n      value: 20\n    conclusion:",
        )
        .replace(
            "  - id: command.investigate\n    name:",
            "  - id: command.investigate\n    points:\n      value: 3\n      max_claim_count: 4\n      requires: [setting.study, entity.knife, fact.knife_is_present, deduction.solution, flag.knife_examined, trigger.investigate_knife]\n    name:",
        );
    let report = report(source);
    assert!(report.valid, "{:#?}", report.diagnostics);
}

#[test]
fn point_awards_reject_invalid_values_owners_and_requirement_kinds() {
    let source = VALID_FORMAT_3_STORY
        .replace(
            "  - id: setting.foyer\n    type: room",
            "  - id: setting.foyer\n    type: room\n    points:\n      value: 0\n      max_claim_count: -1\n      requires: [character.victim, setting.unknown]",
        )
        .replace(
            "  - id: character.victim",
            "  - id: character.victim\n    points:\n      value: 1",
        );
    let report = report(source);
    let diagnostics = report
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.pointer.as_deref()))
        .collect::<Vec<_>>();
    assert!(diagnostics.contains(&("points.value", Some("/settings/1/points/value"))));
    assert!(diagnostics.contains(&(
        "points.max_claim_count",
        Some("/settings/1/points/max_claim_count")
    )));
    assert!(diagnostics.contains(&(
        "reference.wrong_type",
        Some("/settings/1/points/requires/0")
    )));
    assert!(diagnostics.contains(&("reference.unknown", Some("/settings/1/points/requires/1"))));
    assert!(diagnostics.contains(&("points.owner", Some("/characters/0/points"))));
}

#[test]
fn point_awards_report_exact_shape_and_field_diagnostics() {
    for (points, code, pointer) in [
        ("points: 4", "points.type", "/settings/1/points"),
        (
            "points:\n      requires: []",
            "points.value",
            "/settings/1/points/value",
        ),
        (
            "points:\n      value: 1.5",
            "points.value",
            "/settings/1/points/value",
        ),
        (
            "points:\n      value: 1\n      max_claim_count: 0",
            "points.max_claim_count",
            "/settings/1/points/max_claim_count",
        ),
        (
            "points:\n      value: 1\n      requires: setting.study",
            "points.requires_type",
            "/settings/1/points/requires",
        ),
        (
            "points:\n      value: 1\n      bonus: 2",
            "points.unknown_field",
            "/settings/1/points/bonus",
        ),
    ] {
        let source = VALID_FORMAT_3_STORY.replace(
            "  - id: setting.foyer\n    type: room",
            &format!("  - id: setting.foyer\n    type: room\n    {points}"),
        );
        let report = report(source);
        assert!(
            report.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == code && diagnostic.pointer.as_deref() == Some(pointer)
            }),
            "{points}: {:#?}",
            report.diagnostics
        );
    }
}

#[test]
fn accepts_player_safe_portrayal_and_ordered_testimony_for_all_characters() {
    let report = report(format_3_story_with_player_safe_character_behavior());

    assert!(report.valid, "{:#?}", report.diagnostics);
    assert!(report.diagnostics.is_empty());
}

#[test]
fn character_voice_id_is_optional_and_validates_elevenlabs_identifier_shape() {
    let valid = report(format_3_story_with_character_fields(
        "    voice_id: JBFqnCBsd6RMkjVDRZzb\n",
    ));
    assert!(
        !valid
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "character.voice_id"),
        "unexpected voice diagnostic: {:#?}",
        valid.diagnostics
    );

    for invalid in ["''", "' voice-id'", "voice/id", "[]"] {
        let report = report(format_3_story_with_character_fields(&format!(
            "    voice_id: {invalid}\n"
        )));
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "character.voice_id"
                && diagnostic.pointer.as_deref() == Some("/characters/1/voice_id")
                && diagnostic.subject_id.as_deref() == Some("character.culprit")
        }));
    }

    let too_long = "v".repeat(129);
    let report = report(format_3_story_with_character_fields(&format!(
        "    voice_id: {too_long}\n"
    )));
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "character.voice_id"));
}

#[test]
fn portrayal_may_be_absent_but_present_portrayal_must_be_supported_and_nonempty() {
    let absent = report(VALID_FORMAT_3_STORY);
    assert!(absent.valid, "{:#?}", absent.diagnostics);

    let invalid = report(
        format_3_story_with_player_safe_character_behavior()
            .replace("demeanor: Quietly formal.", "demeanor: \"   \"")
            .replace(
                "speech_style: Precise, restrained sentences.",
                "speech_style: 42\n      must_not_confirm: SENTINEL_HIDDEN_FACT",
            ),
    );
    for (code, pointer, subject) in [
        (
            "character.portrayal_value",
            "/characters/0/portrayal/demeanor",
            "character.victim",
        ),
        (
            "character.portrayal_value",
            "/characters/1/portrayal/speech_style",
            "character.culprit",
        ),
        (
            "character.portrayal_unknown_field",
            "/characters/1/portrayal/must_not_confirm",
            "character.culprit",
        ),
    ] {
        assert!(
            invalid.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == code
                    && diagnostic.pointer.as_deref() == Some(pointer)
                    && diagnostic.subject_id.as_deref() == Some(subject)
            }),
            "missing {code} at {pointer}: {:#?}",
            invalid.diagnostics
        );
    }

    for (value, code) in [
        ("[]", "character.portrayal_type"),
        ("{}", "character.portrayal_empty"),
        (
            "{must_not_confirm: SENTINEL_HIDDEN_FACT}",
            "character.portrayal_empty",
        ),
    ] {
        let invalid = report(format_3_story_with_character_fields(&format!(
            "    portrayal: {value}\n"
        )));
        assert!(
            invalid.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == code
                    && diagnostic.pointer.as_deref() == Some("/characters/1/portrayal")
                    && diagnostic.subject_id.as_deref() == Some("character.culprit")
            }),
            "{value}: {:#?}",
            invalid.diagnostics
        );
    }
}

#[test]
fn testimony_requires_safe_shape_text_and_explicit_question_target_gates() {
    let malformed = report(format_3_story_with_character_fields(
        "    testimony:\n      - id: testimony.bad_shape\n        text: \"   \"\n        requires: []\n        reveals: fact.knife_is_present\n        must_not_confirm: SENTINEL_HIDDEN_FACT\n",
    ));
    for (code, pointer) in [
        ("character.testimony_text", "/characters/1/testimony/0/text"),
        (
            "character.testimony_requires_type",
            "/characters/1/testimony/0/requires",
        ),
        (
            "character.testimony_reveals_type",
            "/characters/1/testimony/0/reveals",
        ),
        (
            "character.testimony_unknown_field",
            "/characters/1/testimony/0/must_not_confirm",
        ),
    ] {
        assert!(
            malformed.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == code
                    && diagnostic.pointer.as_deref() == Some(pointer)
                    && diagnostic.subject_id.as_deref() == Some("testimony.bad_shape")
            }),
            "missing {code}: {:#?}",
            malformed.diagnostics
        );
    }

    let unsafe_gates = report(format_3_story_with_character_fields(
        "    testimony:\n      - id: testimony.missing_safe_gates\n        text: A structurally safe statement.\n        requires: [fact.knife_is_present]\n",
    ));
    for code in [
        "character.testimony_question_requirement",
        "character.testimony_character_requirement",
    ] {
        assert!(
            unsafe_gates.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == code
                    && diagnostic.pointer.as_deref() == Some("/characters/1/testimony/0/requires")
                    && diagnostic.subject_id.as_deref() == Some("testimony.missing_safe_gates")
            }),
            "missing {code}: {:#?}",
            unsafe_gates.diagnostics
        );
    }
}

#[test]
fn testimony_rejects_collection_entry_and_id_shapes() {
    let mapping = report(format_3_story_with_character_fields(
        "    testimony: {default: SENTINEL_PRIVATE_LEGACY_TEXT}\n",
    ));
    assert!(mapping.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "character.testimony_type"
            && diagnostic.pointer.as_deref() == Some("/characters/1/testimony")
            && diagnostic.subject_id.as_deref() == Some("character.culprit")
    }));

    let entries = report(format_3_story_with_character_fields(
        "    testimony:\n      - SENTINEL_NOT_A_MAPPING\n      - text: Missing an ID.\n        requires: [command.question, character.culprit]\n      - id: fact.wrong_prefix_but_unique\n        text: Wrong prefix.\n        requires: [command.question, character.culprit]\n      - id: Testimony.Invalid\n        text: Invalid ID shape.\n        requires: [command.question, character.culprit]\n",
    ));
    for (code, pointer) in [
        (
            "character.testimony_entry_type",
            "/characters/1/testimony/0",
        ),
        ("id.missing", "/characters/1/testimony/1/id"),
        ("id.wrong_prefix", "/characters/1/testimony/2/id"),
        ("id.invalid", "/characters/1/testimony/3/id"),
    ] {
        assert!(
            entries.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == code && diagnostic.pointer.as_deref() == Some(pointer)
            }),
            "missing {code}: {:#?}",
            entries.diagnostics
        );
    }
}

#[test]
fn testimony_ids_and_reference_lists_are_unique_and_typed() {
    let duplicate_id = report(
        format_3_story_with_player_safe_character_behavior().replace(
            "    testimony: []",
            "    testimony:\n      - id: testimony.culprit_opening_account\n        text: Duplicate across character owners.\n        requires: [command.question, character.victim]",
        ),
    );
    let duplicate = duplicate_id
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "id.duplicate")
        .expect("duplicate testimony ID diagnostic");
    assert_eq!(
        duplicate.pointer.as_deref(),
        Some("/characters/1/testimony/0/id")
    );
    assert_eq!(
        duplicate.subject_id.as_deref(),
        Some("testimony.culprit_opening_account")
    );
    assert_eq!(duplicate.related.len(), 1);

    let bad_refs = report(format_3_story_with_character_fields(
        "    testimony:\n      - id: testimony.bad_refs\n        text: References are structurally checked.\n        requires: [command.question, character.culprit, flag.not_authored, command.question]\n        reveals: [entity.knife, fact.not_authored, entity.knife]\n",
    ));
    for (code, pointer, subject) in [
        (
            "reference.unknown",
            "/characters/1/testimony/0/requires/2",
            "flag.not_authored",
        ),
        (
            "list.duplicate_reference",
            "/characters/1/testimony/0/requires/3",
            "command.question",
        ),
        (
            "reference.wrong_type",
            "/characters/1/testimony/0/reveals/0",
            "entity.knife",
        ),
        (
            "reference.unknown",
            "/characters/1/testimony/0/reveals/1",
            "fact.not_authored",
        ),
        (
            "list.duplicate_reference",
            "/characters/1/testimony/0/reveals/2",
            "entity.knife",
        ),
    ] {
        assert!(
            bad_refs.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == code
                    && diagnostic.pointer.as_deref() == Some(pointer)
                    && diagnostic.subject_id.as_deref() == Some(subject)
            }),
            "missing {code} at {pointer}: {:#?}",
            bad_refs.diagnostics
        );
    }
}

#[test]
fn testimony_rejects_every_non_question_command_gate_and_preserves_duplicate_diagnostics() {
    let source = format_3_story_with_character_fields(
        "    testimony:\n      - id: testimony.impossible_commands\n        text: This can only be selected by questioning.\n        requires: [command.question, character.culprit, command.examine, command.investigate, command.examine]\n",
    )
    .replace(
        "  - id: command.claim",
        "  - id: command.examine\n    name: Examine\n  - id: command.claim",
    );
    let report = report(source);

    for (pointer, subject) in [
        (
            "/characters/1/testimony/0/requires/2",
            "testimony.impossible_commands",
        ),
        (
            "/characters/1/testimony/0/requires/3",
            "testimony.impossible_commands",
        ),
        (
            "/characters/1/testimony/0/requires/4",
            "testimony.impossible_commands",
        ),
    ] {
        assert!(
            report.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "character.testimony_command_requirement"
                    && diagnostic.pointer.as_deref() == Some(pointer)
                    && diagnostic.subject_id.as_deref() == Some(subject)
            }),
            "missing forbidden command diagnostic at {pointer}: {:#?}",
            report.diagnostics
        );
    }
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "list.duplicate_reference"
            && diagnostic.pointer.as_deref() == Some("/characters/1/testimony/0/requires/4")
            && diagnostic.subject_id.as_deref() == Some("command.examine")
    }));
}

#[test]
fn testimony_accepts_exact_question_gate_with_real_non_command_prerequisites() {
    let source = format_3_story_with_character_fields(
        "    testimony:\n      - id: testimony.real_prerequisites\n        text: Every additional prerequisite can be satisfied independently.\n        requires:\n          - command.question\n          - character.culprit\n          - character.victim\n          - entity.knife\n          - event.murder\n          - fact.knife_is_present\n          - deduction.solution\n          - flag.knife_examined\n          - setting.study\n          - route.foyer_study\n          - trigger.investigate_knife\n        reveals: []\n",
    );
    let report = report(source);

    assert!(report.valid, "{:#?}", report.diagnostics);
    assert!(report.diagnostics.is_empty());
}

#[test]
fn authored_testimony_requires_a_first_required_character_question_target() {
    let base = format_3_story_with_player_safe_character_behavior();
    let valid_parameters = "    parameters:\n      - name: character\n        types: [character]\n        min: 1\n        max: 1\n";
    for (replacement, code, pointer) in [
        (
            "",
            "character.testimony_question_parameters",
            "/commands/0/parameters",
        ),
        (
            "    parameters: []\n",
            "character.testimony_question_target_missing",
            "/commands/0/parameters/0",
        ),
        (
            "    parameters:\n      - name: character\n        types: [entity]\n        min: 1\n        max: 1\n",
            "character.testimony_question_target_type",
            "/commands/0/parameters/0/types",
        ),
        (
            "    parameters:\n      - name: character\n        types: [character]\n        min: 0\n        max: 1\n",
            "character.testimony_question_target_required",
            "/commands/0/parameters/0/min",
        ),
    ] {
        let report = report(base.replace(valid_parameters, replacement));
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == code
                && diagnostic.pointer.as_deref() == Some(pointer)
                && diagnostic.subject_id.as_deref() == Some("command.question")
        }), "missing {code} at {pointer}: {:#?}", report.diagnostics);
    }
}

#[test]
fn question_character_target_requires_exact_name_without_target_diagnostic_cascades() {
    let base = format_3_story_with_player_safe_character_behavior();
    let valid_parameters = "    parameters:\n      - name: character\n        types: [character]\n        min: 1\n        max: 1\n";
    for replacement in [
        "    parameters:\n      - types: [character]\n        min: 1\n        max: 1\n",
        "    parameters:\n      - name: null\n        types: [character]\n        min: 1\n        max: 1\n",
        "    parameters:\n      - name: 42\n        types: [character]\n        min: 1\n        max: 1\n",
        "    parameters:\n      - name: \"\"\n        types: [character]\n        min: 1\n        max: 1\n",
        "    parameters:\n      - name: \"   \"\n        types: [character]\n        min: 1\n        max: 1\n",
        "    parameters:\n      - name: suspect\n        types: [character]\n        min: 1\n        max: 1\n",
    ] {
        let invalid = report(base.replace(valid_parameters, replacement));
        let target_diagnostics = invalid
            .diagnostics
            .iter()
            .filter(|diagnostic| {
                diagnostic
                    .code
                    .starts_with("character.testimony_question_target_")
            })
            .collect::<Vec<_>>();

        assert_eq!(
            target_diagnostics.len(),
            1,
            "invalid signature {replacement:?}: {:#?}",
            invalid.diagnostics
        );
        assert_eq!(
            target_diagnostics[0].code,
            "character.testimony_question_target_name"
        );
        assert_eq!(
            target_diagnostics[0].pointer.as_deref(),
            Some("/commands/0/parameters/0/name")
        );
        assert_eq!(
            target_diagnostics[0].subject_id.as_deref(),
            Some("command.question")
        );
    }

    let valid = report(base);
    assert!(valid.valid, "{:#?}", valid.diagnostics);
    assert!(!valid
        .diagnostics
        .iter()
        .any(|diagnostic| { diagnostic.code == "character.testimony_question_target_name" }));
}

#[test]
fn question_parameters_after_the_character_target_must_be_optional_typed_topics() {
    let base = format_3_story_with_player_safe_character_behavior();
    let valid_parameters = "    parameters:\n      - name: character\n        types: [character]\n        min: 1\n        max: 1\n";
    let malformed = base.replace(
        valid_parameters,
        "    parameters:\n      - name: character\n        types: [character]\n        min: 1\n        max: 1\n      - name: topic\n        types: [entity]\n        min: 1\n        max: 1\n      - name: other\n        types: [character, setting, event, entity, deduction]\n        min: 0\n        max: 5\n",
    );
    let malformed_report = report(malformed);
    for (code, pointer) in [
        (
            "character.testimony_question_topic_name",
            "/commands/0/parameters/2/name",
        ),
        (
            "character.testimony_question_topic_required",
            "/commands/0/parameters/1/min",
        ),
        (
            "character.testimony_question_topic_type",
            "/commands/0/parameters/1/types",
        ),
    ] {
        assert!(
            malformed_report.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == code
                    && diagnostic.pointer.as_deref() == Some(pointer)
                    && diagnostic.subject_id.as_deref() == Some("command.question")
            }),
            "missing {code} at {pointer}: {:#?}",
            malformed_report.diagnostics
        );
    }

    let wrong_order = base.replace(
        valid_parameters,
        "    parameters:\n      - name: topic\n        types: [character, setting, event, entity, deduction]\n        min: 0\n        max: 5\n      - name: character\n        types: [character]\n        min: 1\n        max: 1\n",
    );
    let report = report(wrong_order);
    for (code, pointer) in [
        (
            "character.testimony_question_target_type",
            "/commands/0/parameters/0/types",
        ),
        (
            "character.testimony_question_target_required",
            "/commands/0/parameters/0/min",
        ),
        (
            "character.testimony_question_topic_name",
            "/commands/0/parameters/1/name",
        ),
    ] {
        assert!(
            report.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == code
                    && diagnostic.pointer.as_deref() == Some(pointer)
                    && diagnostic.subject_id.as_deref() == Some("command.question")
            }),
            "wrong order missing {code}: {:#?}",
            report.diagnostics
        );
    }
}

#[test]
fn question_signature_rejects_split_legacy_topics() {
    let source = format_3_story_with_player_safe_character_behavior().replace(
        "    parameters:\n      - name: character\n        types: [character]\n        min: 1\n        max: 1\n",
        "    parameters:\n      - name: character\n        types: [character]\n        min: 1\n        max: 1\n      - name: topic_character\n        types: [character]\n        min: 0\n        max: 1\n",
    );
    let report = report(source);

    assert!(!report.valid);
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "character.testimony_question_topic_name"
            && diagnostic.pointer.as_deref() == Some("/commands/0/parameters/1/name")
    }));
}

#[test]
fn question_signature_accepts_canonical_union_topic_cardinality() {
    let source = format_3_story_with_player_safe_character_behavior().replace(
        "    parameters:\n      - name: character\n        types: [character]\n        min: 1\n        max: 1\n",
        "    parameters:\n      - name: character\n        types: [character]\n        min: 1\n        max: 1\n      - name: topic\n        types: [character, setting, event, entity, deduction]\n        min: 0\n        max: 5\n",
    );
    let report = report(source);

    assert!(report.valid, "{:#?}", report.diagnostics);
    assert!(report.diagnostics.is_empty());
}

#[test]
fn omitted_and_empty_reveals_match_and_narrator_guidance_is_not_safe_content() {
    let source = format_3_story_with_character_fields(
        "    narrator_guidance:\n      goal: SENTINEL_PRIVATE_GOAL\n      testimony_guidance:\n        under_pressure: SENTINEL_PRIVATE_UNDER_PRESSURE\n        must_not_confirm: SENTINEL_HIDDEN_FACT\n    portrayal:\n      demeanor: Player-safe demeanor.\n    testimony:\n      - id: testimony.no_reveal_omitted\n        text: This entry reveals no fact.\n        requires: [command.question, character.culprit]\n      - id: testimony.no_reveal_empty\n        text: This entry also reveals no fact.\n        requires: [command.question, character.culprit]\n        reveals: []\n",
    );
    let report = report(source);

    assert!(report.valid, "{:#?}", report.diagnostics);
    assert!(report.diagnostics.is_empty());
}

#[test]
fn format_3_accepts_narrative_detail_on_every_supported_fact_owner() {
    let report = report(format_3_story_with_narrative_details());

    assert!(report.valid, "{:#?}", report.diagnostics);
    assert!(report.diagnostics.is_empty());
}

#[test]
fn format_3_rejects_blank_narrative_detail_on_every_supported_fact_owner() {
    for blank in ["\"\"", "\"   \""] {
        let source =
            format_3_story_with_narrative_details().replace("SAFE_NARRATIVE_DETAIL", blank);
        let report = report(source);
        let pointers: Vec<_> = report
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "fact.narrative_detail")
            .filter_map(|diagnostic| diagnostic.pointer.as_deref())
            .collect();

        assert_eq!(
            pointers,
            [
                "/characters/1/facts/0/narrative_detail",
                "/entities/0/facts/0/narrative_detail",
                "/events/0/facts/0/narrative_detail",
                "/settings/1/facts/0/narrative_detail",
                "/triggers/0/facts/0/narrative_detail",
            ],
            "{blank}"
        );
    }
}

#[test]
fn format_3_rejects_every_non_string_narrative_detail_shape() {
    for value in ["null", "false", "42", "[detail]", "{text: detail}"] {
        let report = report(VALID_FORMAT_3_STORY.replace(
            "        statement: The knife is present.",
            &format!("        statement: The knife is present.\n        narrative_detail: {value}"),
        ));
        let diagnostics: Vec<_> = report
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "fact.narrative_detail")
            .collect();

        assert_eq!(diagnostics.len(), 1, "{value}: {:#?}", report.diagnostics);
        assert_eq!(
            diagnostics[0].pointer.as_deref(),
            Some("/entities/0/facts/0/narrative_detail")
        );
        assert_eq!(
            diagnostics[0].subject_id.as_deref(),
            Some("fact.knife_is_present")
        );
    }
}

#[test]
fn format_3_accepts_exact_occurrence_on_every_supported_fact_owner() {
    let all_owners = report(format_3_story_with_occurrences_on_every_fact_owner());

    assert!(all_owners.valid, "{:#?}", all_owners.diagnostics);
    assert!(all_owners.diagnostics.is_empty());

    let maximum_day = report(format_3_story_with_entity_occurrence(
        "        occurred_at:\n          day: 2147483647\n          time: \"23:59\"",
    ));
    assert!(maximum_day.valid, "{:#?}", maximum_day.diagnostics);
    assert!(maximum_day.diagnostics.is_empty());
}

#[test]
fn format_3_occurrence_requires_an_exact_mapping_and_both_fields() {
    for value in ["null", "[]", "\"21:18\"", "false"] {
        let report = report(format_3_story_with_entity_occurrence(&format!(
            "        occurred_at: {value}"
        )));
        let diagnostic = report
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == "fact.occurred_at_type")
            .unwrap_or_else(|| {
                panic!(
                    "missing type diagnostic for {value}: {:#?}",
                    report.diagnostics
                )
            });
        assert_eq!(
            diagnostic.pointer.as_deref(),
            Some("/entities/0/facts/0/occurred_at")
        );
        assert_eq!(
            diagnostic.subject_id.as_deref(),
            Some("fact.knife_is_present")
        );
    }

    for (mapping, code, pointer) in [
        (
            "        occurred_at:\n          time: \"21:18\"",
            "fact.occurred_at_day_missing",
            "/entities/0/facts/0/occurred_at/day",
        ),
        (
            "        occurred_at:\n          day: 0",
            "fact.occurred_at_time_missing",
            "/entities/0/facts/0/occurred_at/time",
        ),
    ] {
        let report = report(format_3_story_with_entity_occurrence(mapping));
        assert!(
            report.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == code
                    && diagnostic.pointer.as_deref() == Some(pointer)
                    && diagnostic.subject_id.as_deref() == Some("fact.knife_is_present")
            }),
            "missing {code}: {:#?}",
            report.diagnostics
        );
    }
}

#[test]
fn format_3_occurrence_day_is_a_bounded_nonnegative_integer() {
    for day in ["-1", "true", "1.5", "\"0\"", "2147483648"] {
        let report = report(format_3_story_with_entity_occurrence(&format!(
            "        occurred_at:\n          day: {day}\n          time: \"21:18\""
        )));
        let diagnostic = report
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == "fact.occurred_at_day")
            .unwrap_or_else(|| {
                panic!(
                    "missing day diagnostic for {day}: {:#?}",
                    report.diagnostics
                )
            });
        assert_eq!(
            diagnostic.pointer.as_deref(),
            Some("/entities/0/facts/0/occurred_at/day")
        );
        assert_eq!(
            diagnostic.subject_id.as_deref(),
            Some("fact.knife_is_present")
        );
    }
}

#[test]
fn format_3_occurrence_time_requires_canonical_exact_hh_mm() {
    for time in [
        "\"1:02\"",
        "\"01:2\"",
        "\"24:00\"",
        "\"23:60\"",
        "\" 01:02\"",
        "\"01:02 \"",
        "42",
        "null",
    ] {
        let report = report(format_3_story_with_entity_occurrence(&format!(
            "        occurred_at:\n          day: 0\n          time: {time}"
        )));
        let diagnostic = report
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == "fact.occurred_at_time")
            .unwrap_or_else(|| {
                panic!(
                    "missing time diagnostic for {time}: {:#?}",
                    report.diagnostics
                )
            });
        assert_eq!(
            diagnostic.pointer.as_deref(),
            Some("/entities/0/facts/0/occurred_at/time")
        );
        assert_eq!(
            diagnostic.subject_id.as_deref(),
            Some("fact.knife_is_present")
        );
    }
}

#[test]
fn format_3_occurrence_rejects_unknown_fields_at_the_exact_pointer() {
    let report = report(format_3_story_with_entity_occurrence(
        "        occurred_at:\n          time: \"21:18\"\n          timezone: UTC\n          day: 0",
    ));
    let diagnostics = report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "fact.occurred_at_unknown_field")
        .collect::<Vec<_>>();

    assert_eq!(diagnostics.len(), 1, "{:#?}", report.diagnostics);
    assert_eq!(
        diagnostics[0].pointer.as_deref(),
        Some("/entities/0/facts/0/occurred_at/timezone")
    );
    assert_eq!(
        diagnostics[0].subject_id.as_deref(),
        Some("fact.knife_is_present")
    );
}

#[test]
fn format_1_does_not_treat_clue_metadata_as_fact_narrative_detail() {
    let source = VALID_STORY.replace(
        "  - id: clue.weapon\n    discover_by:",
        "  - id: clue.weapon\n    narrative_detail: 42\n    discover_by:",
    );
    let report = report(source);

    assert!(report.valid, "{:#?}", report.diagnostics);
    assert!(!report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "fact.narrative_detail"));
}

#[test]
fn format_1_does_not_treat_clue_metadata_as_fact_occurrence() {
    let source = VALID_STORY.replace(
        "  - id: clue.weapon\n    discover_by:",
        "  - id: clue.weapon\n    occurred_at:\n      day: invalid\n      time: invalid\n    discover_by:",
    );
    let report = report(source);

    assert!(report.valid, "{:#?}", report.diagnostics);
    assert!(!report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code.starts_with("fact.occurred_at")));
}

#[test]
fn validates_optional_case_voice_metadata() {
    let with_voice = VALID_FORMAT_3_STORY.replace(
        "  initial_time: \"21:00\"",
        "  initial_time: \"21:00\"\n  genre: closed-circle mystery\n  tone: [elegant, storm-bound, quietly menacing]",
    );
    let with_voice_report = report(with_voice);
    assert!(
        with_voice_report.valid,
        "{:#?}",
        with_voice_report.diagnostics
    );

    let without_voice = report(VALID_FORMAT_3_STORY);
    assert!(without_voice.valid, "{:#?}", without_voice.diagnostics);
}

#[test]
fn rejects_blank_and_wrong_type_case_genre() {
    for genre in ["\"   \"", "[mystery]", "42"] {
        let report = report(VALID_FORMAT_3_STORY.replace(
            "  initial_time: \"21:00\"",
            &format!("  initial_time: \"21:00\"\n  genre: {genre}"),
        ));
        let diagnostic = report
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == "case.genre")
            .expect("genre diagnostic");
        assert_eq!(diagnostic.pointer.as_deref(), Some("/case/genre"));
    }
}

#[test]
fn rejects_wrong_type_blank_and_duplicate_case_tones() {
    let wrong_type = report(VALID_FORMAT_3_STORY.replace(
        "  initial_time: \"21:00\"",
        "  initial_time: \"21:00\"\n  tone: elegant",
    ));
    assert!(wrong_type
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "case.tone_type"
            && diagnostic.pointer.as_deref() == Some("/case/tone")));

    let invalid_entries = report(VALID_FORMAT_3_STORY.replace(
        "  initial_time: \"21:00\"",
        "  initial_time: \"21:00\"\n  tone: [elegant, \"   \", 42, \" elegant \"]",
    ));
    let entry_pointers: Vec<_> = invalid_entries
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "case.tone_entry")
        .filter_map(|diagnostic| diagnostic.pointer.as_deref())
        .collect();
    assert_eq!(entry_pointers, ["/case/tone/1", "/case/tone/2"]);
    assert!(invalid_entries.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "case.tone_duplicate"
            && diagnostic.pointer.as_deref() == Some("/case/tone/3")
    }));
}

#[test]
fn format_3_requires_a_runtime_clock_and_whole_minute_effects() {
    let missing = report(VALID_FORMAT_3_STORY.replace("  initial_time: \"21:00\"\n", ""));
    assert!(missing
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "case.initial_time_missing"));

    let invalid = codes(
        VALID_FORMAT_3_STORY
            .replace("initial_time: \"21:00\"", "initial_time: \"24:00\"")
            .replace("minutes: 12", "minutes: 12.5"),
    );
    assert!(invalid.contains(&"case.initial_time".to_string()));
    assert!(invalid.contains(&"command.effect_minutes".to_string()));
}

#[test]
fn validates_runtime_trigger_effect_contracts() {
    let invalid = VALID_FORMAT_3_STORY
        .replace(
            "    effects:",
            "    when:\n      all:\n        - time:\n            relation: after\n            value: \"99:00\"\n    effects:",
        )
        .replace(
            "      - operation: set_flag\n        flag: flag.knife_analysis_complete\n        value: true\n        after: 20m",
            "      - operation: move\n        subjects: [player]\n        setting: param1\n        surprise: true\n      - operation: rewrite_reality\n        flag: flag.knife_analysis_complete",
        );
    let result = codes(invalid);
    assert!(result.contains(&"condition.time_value".to_string()));
    assert!(result.contains(&"command.effect_parameter_type".to_string()));
    assert!(result.contains(&"command.effect_unknown_field".to_string()));
    assert!(result.contains(&"command.effect_unknown_operation".to_string()));

    let missing_value =
        codes(VALID_FORMAT_3_STORY.replace("        value: true", "        omitted_value: true"));
    assert!(missing_value.contains(&"effect.flag_value".to_string()));
}

#[test]
fn rejects_removed_tags_section_and_action_effects() {
    let source = VALID_FORMAT_3_STORY
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
fn format_3_nests_facts_and_rejects_top_level_facts_and_clues() {
    let with_top_level_facts = VALID_FORMAT_3_STORY.replace(
        "entities:",
        "facts:\n  - id: fact.legacy\n    statement: This no longer belongs at the root.\nentities:",
    );
    assert!(codes(with_top_level_facts).contains(&"format.facts_section_removed".to_string()));

    let with_clues =
        VALID_FORMAT_3_STORY.replace("deductions:", "clues:\n  - id: clue.legacy\ndeductions:");
    assert!(codes(with_clues).contains(&"format.clues_removed".to_string()));
}

#[test]
fn format_3_allows_empty_fact_lists_and_rejects_non_mapping_facts() {
    let empty = VALID_FORMAT_3_STORY.replace(
        "  - id: character.culprit",
        "  - id: character.culprit\n    facts: []",
    );
    let report = report(empty);
    assert!(report.valid, "{:#?}", report.diagnostics);

    let malformed = VALID_FORMAT_3_STORY.replace(
        "    facts:",
        "    facts:\n      - fact.legacy_reference\n    ignored_facts:",
    );
    assert!(codes(malformed).contains(&"fact.item_type".to_string()));
}

#[test]
fn format_3_validates_fact_requirements_and_cycles() {
    let malformed = VALID_FORMAT_3_STORY
        .replace(
            "          all:\n            - flag: flag.knife_analysis_complete",
            "          all: []",
        )
        .replace(
            "            - owns: entity.knife",
            "            - owns: case.example",
        );
    let result = codes(malformed);
    assert!(result.contains(&"condition.all_type".to_string()));
    assert!(result.contains(&"reference.wrong_type".to_string()));

    let cyclic = VALID_FORMAT_3_STORY.replace(
        "        statement: The knife is present.",
        "        statement: The knife is present.\n        when:\n          all:\n            - knows: fact.knife_connects_to_scene",
    );
    assert!(codes(cyclic).contains(&"fact.requirement_cycle".to_string()));
}

#[test]
fn format_3_accepts_opening_action_persistent_and_delayed_fact_discovery() {
    let source = VALID_FORMAT_3_STORY
        .replace(
            "        statement: The knife is present.",
            "        statement: The knife is present.\n        on:\n          command: command.investigate\n          parameters:\n            target: owner",
        )
        .replace(
            "    effects:\n      - operation: set_flag\n        flag: flag.knife_analysis_complete\n        value: true\n        after: 20m",
            "    after: 20m\n    facts:\n      - id: fact.delayed_result\n        statement: The delayed result is ready.",
        );
    let valid_report = report(source);
    assert!(valid_report.valid, "{:#?}", valid_report.diagnostics);
}

#[test]
fn format_3_rejects_immediate_effects_on_a_delayed_trigger() {
    let source = VALID_FORMAT_3_STORY.replace(
        "    effects:\n      - operation: set_flag",
        "    after: 20m\n    effects:\n      - operation: set_flag",
    );
    let result = report(source);
    assert!(result.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "trigger.delayed_effects"
            && diagnostic.pointer.as_deref() == Some("/triggers/0/effects")
    }));
}

#[test]
fn action_matches_validate_semantic_roles_unions_cardinality_actor_and_owner() {
    let source = VALID_FORMAT_3_STORY
        .replace(
            "triggers:",
            "  - id: command.compare\n    name: Compare\n    parameters:\n      - name: evidence\n        types: [entity, setting]\n        min: 1\n        max: 2\n    effects: []\ntriggers:",
        )
        .replace(
            "  - tag_id: 6\n    subject: command.investigate",
            "  - tag_id: 6\n    subject: command.investigate\n  - tag_id: 7\n    subject: command.compare",
        )
        .replace(
            "      command: command.investigate",
            "      command: command.compare",
        )
        .replace(
            "        target: entity.knife",
            "        evidence: [entity.knife, setting.study]",
        )
        .replace(
            "      parameters:\n        evidence: [entity.knife, setting.study]",
            "      actor: character.culprit\n      parameters:\n        evidence: [entity.knife, setting.study]",
        );
    let valid_report = report(source);
    assert!(valid_report.valid, "{:#?}", valid_report.diagnostics);

    for (replacement, code, pointer) in [
        (
            "        missing: entity.knife",
            "action_match.parameter_unknown",
            "/triggers/0/on/parameters/missing",
        ),
        (
            "        target: character.culprit",
            "reference.wrong_type",
            "/triggers/0/on/parameters/target",
        ),
        (
            "        target: [entity.knife, entity.knife, entity.knife]",
            "action_match.parameter_cardinality",
            "/triggers/0/on/parameters/target",
        ),
    ] {
        let invalid = VALID_FORMAT_3_STORY.replace("        target: entity.knife", replacement);
        assert!(report(invalid).diagnostics.iter().any(|diagnostic| {
            diagnostic.code == code && diagnostic.pointer.as_deref() == Some(pointer)
        }));
    }

    let bad_actor = report(VALID_FORMAT_3_STORY.replace(
        "      command: command.investigate",
        "      command: command.investigate\n      actor: entity.knife",
    ));
    assert!(bad_actor.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "reference.wrong_type"
            && diagnostic.pointer.as_deref() == Some("/triggers/0/on/actor")
    }));
}

#[test]
fn persistent_conditions_validate_every_role_and_reject_ambiguous_or_wrong_kinds() {
    let valid = VALID_FORMAT_3_STORY.replace(
        "            - flag: flag.knife_analysis_complete",
        "            - at: setting.study\n            - owns: entity.knife\n            - knows: fact.knife_is_present\n            - knows: deduction.solution\n            - flag: flag.knife_analysis_complete\n            - completed: trigger.investigate_knife\n            - time:\n                relation: after\n                value: \"21:00\"",
    );
    let valid_report = report(valid);
    assert!(valid_report.valid, "{:#?}", valid_report.diagnostics);

    for (predicate, code) in [
        ("near: setting.study", "condition.predicate_kind"),
        ("at: entity.knife", "reference.wrong_type"),
        ("owns: character.culprit", "reference.wrong_type"),
        ("knows: entity.knife", "reference.wrong_type"),
        ("flag: fact.knife_is_present", "reference.wrong_type"),
        ("completed: command.investigate", "reference.wrong_type"),
        ("time: soon", "condition.time_type"),
    ] {
        let invalid = VALID_FORMAT_3_STORY.replace(
            "            - flag: flag.knife_analysis_complete",
            &format!("            - {predicate}"),
        );
        assert!(
            report(invalid)
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == code),
            "{predicate} should produce {code}"
        );
    }
}

#[test]
fn format_3_rejects_legacy_noop_and_cyclic_triggers_deterministically() {
    let legacy = report(VALID_FORMAT_3_STORY.replace(
        "    on:\n      command: command.investigate\n      parameters:\n        target: entity.knife",
        "    command: command.investigate",
    ));
    assert!(
        legacy
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "trigger.legacy_match_field"),
        "{:#?}",
        legacy.diagnostics
    );

    let noop = report(VALID_FORMAT_3_STORY.replace(
        "    effects:\n      - operation: set_flag\n        flag: flag.knife_analysis_complete\n        value: true\n        after: 20m",
        "    effects: []",
    ));
    assert!(noop
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "trigger.no_observable_result"));

    let cyclic = VALID_FORMAT_3_STORY
        .replace(
            "triggers:\n  - id: trigger.investigate_knife",
            "triggers:\n  - id: trigger.prior\n    name: Prior\n    on:\n      command: command.investigate\n    when:\n      all:\n        - completed: trigger.investigate_knife\n  - id: trigger.investigate_knife",
        )
        .replace(
            "    effects:\n      - operation: set_flag",
            "    when:\n      all:\n        - completed: trigger.prior\n    effects:\n      - operation: set_flag",
        );
    let cycles: Vec<_> = report(cyclic)
        .diagnostics
        .into_iter()
        .filter(|diagnostic| diagnostic.code == "trigger.reference_cycle")
        .collect();
    assert_eq!(cycles.len(), 1);
    assert!(cycles[0]
        .message
        .contains("trigger.investigate_knife -> trigger.prior -> trigger.investigate_knife"));
}

#[test]
fn format_3_enforces_automatic_opening_facts_and_central_requirements() {
    let source = VALID_FORMAT_3_STORY
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
fn format_3_validates_flags_and_delayed_assignment() {
    let invalid = VALID_FORMAT_3_STORY
        .replace(
            "  - id: flag.knife_analysis_complete\n    name: Knife analysis complete\n    description: Whether the delayed knife analysis has completed.\n    initial_state: false",
            "  - id: flag.knife_analysis_complete\n    name: Knife analysis complete\n    description: Whether the delayed knife analysis has completed.\n    initial_state: sometimes",
        )
        .replace(
            "flag: flag.knife_analysis_complete\n        value: true\n        after: 20m",
            "flag: entity.knife\n        value: true\n        after: 0m",
        );
    let result = codes(invalid);
    assert!(result.contains(&"flag.initial_state".to_string()));
    assert!(result.contains(&"reference.wrong_type".to_string()));
    assert!(result.contains(&"effect.delay".to_string()));

    let legacy = VALID_FORMAT_3_STORY.replace("operation: set_flag", "operation: learn_after");
    assert!(codes(legacy).contains(&"command.effect_unknown_operation".to_string()));

    for delay in ["1turn", "4294967296turns", "4294967296m"] {
        assert!(
            codes(VALID_FORMAT_3_STORY.replace("after: 20m", &format!("after: {delay}")))
                .contains(&"effect.delay".to_string()),
            "{delay} must be rejected before runtime"
        );
    }
}

#[test]
fn delayed_set_flag_rejects_false_assignment() {
    let report = report(VALID_FORMAT_3_STORY.replace(
        "        value: true\n        after: 20m",
        "        value: false\n        after: 20m",
    ));
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "effect.delay"
            && diagnostic.pointer.as_deref() == Some("/triggers/0/effects/0/after")
    }));
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
fn validates_entity_placement_visibility_and_portability() {
    let source = VALID_FORMAT_3_STORY
        .replace(
            "  - id: entity.knife\n    initial:\n      container: setting.study",
            "  - id: entity.knife\n    initial:\n      container: entity.box\n    physical:\n      portable: true\n    visibility:\n      requires: flag.knife_examined\n  - id: entity.box\n    initial:\n      container: setting.study\n  - id: entity.bookshelf\n    initial:\n      container: setting.study\n    visibility:\n      requires: [setting.study, entity.box, fact.knife_is_present, deduction.solution, flag.knife_examined, trigger.investigate_knife]",
        );
    let report = report(source);
    assert!(report.valid, "{:#?}", report.diagnostics);
}

#[test]
fn validates_format_3_1_character_placement_and_presence() {
    let source = VALID_FORMAT_3_STORY
        .replace("\"3.0.0\"", "\"3.1.0\"")
        .replace(
            "  - id: character.culprit\n    description: A suspect with a carefully guarded secret.",
            "  - id: character.culprit\n    description: A suspect with a carefully guarded secret.\n    initial:\n      location: setting.study\n    presence:\n      requires: [fact.knife_is_present, flag.knife_examined, trigger.investigate_knife]",
        );
    let report = report(source);
    assert!(report.valid, "{:#?}", report.diagnostics);
    assert_eq!(report.format_version.as_deref(), Some("3.1.0"));
}

#[test]
fn normalizes_scalar_and_sequence_character_presence_requirements() {
    for requires in [
        "flag.knife_examined",
        "[fact.knife_is_present, flag.knife_examined, trigger.investigate_knife]",
    ] {
        let source = VALID_FORMAT_3_STORY
            .replace("\"3.0.0\"", "\"3.1.0\"")
            .replace(
                "  - id: character.culprit\n    description: A suspect with a carefully guarded secret.",
                &format!(
                    "  - id: character.culprit\n    description: A suspect with a carefully guarded secret.\n    initial:\n      location: setting.study\n    presence:\n      requires: {requires}"
                ),
            );
        let report = report(source);
        assert!(report.valid, "{requires}: {:#?}", report.diagnostics);
    }
}

#[test]
fn keeps_character_placement_out_of_the_format_3_0_contract() {
    let source = VALID_FORMAT_3_STORY.replace(
        "  - id: character.culprit\n    description: A suspect with a carefully guarded secret.",
        "  - id: character.culprit\n    description: A suspect with a carefully guarded secret.\n    initial:\n      location: setting.study",
    );
    let result = report(source);
    assert!(result.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "character.unknown_field"
            && diagnostic.pointer.as_deref() == Some("/characters/1/initial")
    }));
}

#[test]
fn rejects_invalid_character_placement_and_presence_at_exact_pointers() {
    let format_3_1 = VALID_FORMAT_3_STORY.replace("\"3.0.0\"", "\"3.1.0\"");
    let cases = [
        (
            "    initial: setting.study",
            "character.initial_type",
            "/characters/1/initial",
        ),
        (
            "    initial:\n      location: 42",
            "character.location_type",
            "/characters/1/initial/location",
        ),
        (
            "    initial:\n      location: setting.not_authored",
            "reference.unknown",
            "/characters/1/initial/location",
        ),
        (
            "    initial:\n      location: entity.knife",
            "reference.wrong_type",
            "/characters/1/initial/location",
        ),
        (
            "    initial:\n      location: setting.study\n      container: setting.foyer",
            "character.initial.unknown_field",
            "/characters/1/initial/container",
        ),
        (
            "    initial:\n      location: setting.study\n    presence: visible",
            "character.presence_type",
            "/characters/1/presence",
        ),
        (
            "    initial:\n      location: setting.study\n    presence:\n      requires: []",
            "character.presence_requires_type",
            "/characters/1/presence/requires",
        ),
        (
            "    initial:\n      location: setting.study\n    presence:\n      requires: [flag.knife_examined, 42]",
            "character.presence_requirement_type",
            "/characters/1/presence/requires/1",
        ),
        (
            "    initial:\n      location: setting.study\n    presence:\n      requires: command.investigate",
            "reference.wrong_type",
            "/characters/1/presence/requires",
        ),
        (
            "    initial:\n      location: setting.study\n    presence:\n      when: flag.knife_examined",
            "character.presence.unknown_field",
            "/characters/1/presence/when",
        ),
        (
            "    presence:\n      requires: flag.knife_examined",
            "character.presence_without_location",
            "/characters/1/presence",
        ),
    ];

    for (addition, code, pointer) in cases {
        let source = format_3_1.replace(
            "  - id: character.culprit\n    description: A suspect with a carefully guarded secret.",
            &format!(
                "  - id: character.culprit\n    description: A suspect with a carefully guarded secret.\n{addition}"
            ),
        );
        let report = report(source);
        assert!(
            report.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == code && diagnostic.pointer.as_deref() == Some(pointer)
            }),
            "missing {code} at {pointer}: {:#?}",
            report.diagnostics
        );
    }
}

#[test]
fn rejects_unknown_and_duplicate_character_presence_requirements() {
    let source = VALID_FORMAT_3_STORY
        .replace("\"3.0.0\"", "\"3.1.0\"")
        .replace(
            "  - id: character.culprit\n    description: A suspect with a carefully guarded secret.",
            "  - id: character.culprit\n    description: A suspect with a carefully guarded secret.\n    initial:\n      location: setting.study\n    presence:\n      requires: [flag.not_authored, flag.knife_examined, flag.knife_examined]",
        );
    let report = report(source);
    for (code, pointer) in [
        ("reference.unknown", "/characters/1/presence/requires/0"),
        (
            "list.duplicate_reference",
            "/characters/1/presence/requires/2",
        ),
    ] {
        assert!(
            report.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == code && diagnostic.pointer.as_deref() == Some(pointer)
            }),
            "missing {code} at {pointer}: {:#?}",
            report.diagnostics
        );
    }
}

#[test]
fn rejects_invalid_entity_physical_and_visibility_shapes_at_exact_pointers() {
    for (replacement, code, pointer) in [
        (
            "    physical: portable",
            "entity.physical_type",
            "/entities/0/physical",
        ),
        (
            "    physical:\n      portable: sometimes",
            "entity.portable_type",
            "/entities/0/physical/portable",
        ),
        (
            "    physical:\n      takeable: true",
            "entity.physical_unknown_field",
            "/entities/0/physical/takeable",
        ),
        (
            "    visibility: hidden",
            "entity.visibility_type",
            "/entities/0/visibility",
        ),
        (
            "    visibility:\n      requires: []",
            "entity.visibility_requires_type",
            "/entities/0/visibility/requires",
        ),
        (
            "    visibility:\n      requires: [flag.knife_examined, 42]",
            "entity.visibility_requirement_type",
            "/entities/0/visibility/requires/1",
        ),
        (
            "    visibility:\n      searchable: true",
            "entity.visibility_unknown_field",
            "/entities/0/visibility/searchable",
        ),
    ] {
        let source = VALID_FORMAT_3_STORY.replace(
            "    initial:\n      container: setting.study",
            &format!("    initial:\n      container: setting.study\n{replacement}"),
        );
        let report = report(source);
        assert!(
            report.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == code && diagnostic.pointer.as_deref() == Some(pointer)
            }),
            "missing {code} at {pointer}: {:#?}",
            report.diagnostics
        );
    }
}

#[test]
fn rejects_unknown_ephemeral_and_duplicate_visibility_requirements() {
    let source = VALID_FORMAT_3_STORY.replace(
        "    initial:\n      container: setting.study",
        "    initial:\n      container: setting.study\n    visibility:\n      requires: [flag.not_authored, command.investigate, flag.knife_examined, flag.knife_examined]",
    );
    let report = report(source);
    for (code, pointer) in [
        ("reference.unknown", "/entities/0/visibility/requires/0"),
        ("reference.wrong_type", "/entities/0/visibility/requires/1"),
        (
            "list.duplicate_reference",
            "/entities/0/visibility/requires/3",
        ),
    ] {
        assert!(
            report.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == code && diagnostic.pointer.as_deref() == Some(pointer)
            }),
            "missing {code} at {pointer}: {:#?}",
            report.diagnostics
        );
    }
}

#[test]
fn rejects_entity_container_type_and_cycles_at_the_container_pointer() {
    let wrong_type =
        report(VALID_FORMAT_3_STORY.replace("container: setting.study", "container: 42"));
    assert!(wrong_type.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "entity.container_type"
            && diagnostic.pointer.as_deref() == Some("/entities/0/initial/container")
    }));

    let cyclic = VALID_FORMAT_3_STORY
        .replace("container: setting.study", "container: entity.box")
        .replace(
            "events:",
            "  - id: entity.box\n    initial:\n      container: entity.knife\nevents:",
        );
    let report = report(cyclic);
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "entity.containment_cycle")
        .expect("entity containment cycle diagnostic");
    assert_eq!(
        diagnostic.pointer.as_deref(),
        Some("/entities/1/initial/container")
    );
    assert_eq!(diagnostic.subject_id.as_deref(), Some("entity.box"));
    assert!(diagnostic.range.is_some());
}

#[test]
fn rejects_misplaced_entity_capability_booleans() {
    for field in ["portable", "searchable", "investigatable", "takeable"] {
        let source = VALID_FORMAT_3_STORY.replace(
            "    initial:\n      container: setting.study",
            &format!("    initial:\n      container: setting.study\n    {field}: true"),
        );
        let report = report(source);
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "entity.capability_field"
                && diagnostic.pointer.as_deref() == Some(&format!("/entities/0/{field}"))
        }));
    }
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
        .replace("effects: []", "effects: invalid");
    let result = codes(source);
    assert!(result.contains(&"command.aliases_removed".to_string()));
    assert!(result.contains(&"command.parameter_accepts_removed".to_string()));
    assert!(result.contains(&"command.parameter_kind".to_string()));
    assert!(result.contains(&"command.parameter_required_type".to_string()));
    assert!(result.contains(&"trigger.once_type".to_string()));
    assert!(result.contains(&"trigger.conditions_removed".to_string()));
    assert!(result.contains(&"command.effects_type".to_string()));
}

#[test]
fn validates_typed_trigger_parameter_bindings() {
    let valid = VALID_STORY.replace(
        "    command: command.examine\n    once: true",
        "    command: command.examine\n    parameters:\n      target: entity.knife\n    once: true",
    );
    let result = report(valid.clone());
    assert!(result.valid, "{:#?}", result.diagnostics);

    let cases = [
        (
            valid.replace(
                "    parameters:\n      target: entity.knife",
                "    parameters: []",
            ),
            "trigger.parameters_type",
        ),
        (
            valid.replace("target: entity.knife", "missing: entity.knife"),
            "trigger.parameter_unknown",
        ),
        (
            valid.replace("target: entity.knife", "target: 42"),
            "trigger.parameter_reference",
        ),
        (
            valid.replace("target: entity.knife", "target: character.culprit"),
            "reference.wrong_type",
        ),
        (
            valid.replace("target: entity.knife", "target: entity.missing"),
            "reference.unknown",
        ),
    ];
    for (source, expected) in cases {
        let result = codes(source);
        assert!(
            result.contains(&expected.to_string()),
            "{expected}: {result:?}"
        );
    }
}

#[test]
fn validates_runtime_command_signatures_and_unique_parameter_names() {
    let duplicate =
        codes(VALID_FORMAT_3_STORY.replace("      - name: destination", "      - name: target"));
    assert!(duplicate.contains(&"command.parameter_name_duplicate".to_string()));

    let reserved = codes(VALID_FORMAT_3_STORY.replace(
        "    description: Learn that the knife is present.\n    effects:",
        "    description: Learn that the knife is present.\n    parameters:\n      - name: target\n        types: [entity]\n        min: 1\n        max: 1\n    effects:",
    ));
    assert!(reserved.contains(&"command.runtime_signature".to_string()));
}

#[test]
fn validates_union_parameter_kinds_and_cardinality() {
    let canonical = VALID_STORY.replace(
        "        type: entity\n        required: true",
        "        types: [entity, character, setting]\n        min: 1\n        max: 3",
    );
    let accepted = report(canonical.clone());
    assert!(accepted.valid, "{:#?}", accepted.diagnostics);

    for (source, code) in [
        (
            canonical.replace(
                "types: [entity, character, setting]",
                "types: [entity, entity]",
            ),
            "command.parameter_kind_duplicate",
        ),
        (
            canonical.replace("types: [entity, character, setting]", "types: []"),
            "command.parameter_types_empty",
        ),
        (
            canonical.replace("min: 1\n        max: 3", "min: 4\n        max: 3"),
            "command.parameter_cardinality",
        ),
        (
            canonical.replace("max: 3", "max: nope"),
            "command.parameter_cardinality_type",
        ),
        (
            canonical.replace(
                "types: [entity, character, setting]",
                "type: entity\n        types: [entity]",
            ),
            "command.parameter_kind",
        ),
    ] {
        assert!(codes(source).contains(&code.to_string()), "missing {code}");
    }
}

#[test]
fn validates_format_3_1_declarative_command_candidates() {
    let source = VALID_FORMAT_3_STORY
        .replace("\"3.0.0\"", "\"3.1.0\"")
        .replace(
            "      - name: target\n        types: [entity]\n        min: 1\n        max: 1",
            "      - name: target\n        types: [entity]\n        min: 1\n        max: 1\n        candidates:\n          from: [current_location, inventory, known]\n          capabilities: [portable]",
        )
        .replace(
            "      - name: destination\n        types: [setting]\n        min: 0\n        max: 1",
            "      - name: destination\n        types: [setting]\n        min: 0\n        max: 1\n        candidates:\n          from: [reachable]",
        )
        .replace(
            "      - name: companion\n        types: [character]\n        min: 0\n        max: 1",
            "      - name: companion\n        types: [character]\n        min: 0\n        max: 1\n        candidates:\n          from: [all, known]",
        )
        .replace(
            "      - name: conclusion\n        types: [deduction]\n        min: 0\n        max: 1",
            "      - name: conclusion\n        types: [deduction]\n        min: 0\n        max: 1\n        candidates:\n          from: [established]",
        )
        .replace(
            "      - name: incident\n        types: [event]\n        min: 0\n        max: 1",
            "      - name: incident\n        types: [event]\n        min: 0\n        max: 1\n        candidates:\n          from: [known]",
        );
    let report = report(source);
    assert!(report.valid, "{:#?}", report.diagnostics);
}

#[test]
fn keeps_declarative_command_candidates_out_of_the_format_3_0_contract() {
    let source = VALID_FORMAT_3_STORY.replace(
        "      - name: target\n        types: [entity]\n        min: 1\n        max: 1",
        "      - name: target\n        types: [entity]\n        min: 1\n        max: 1\n        candidates:\n          from: [current_location]",
    );
    let report = report(source);
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "command.parameter_candidates_version"
            && diagnostic.pointer.as_deref() == Some("/commands/1/parameters/0/candidates")
    }));
}

#[test]
fn rejects_invalid_candidate_shapes_names_and_duplicates_at_exact_pointers() {
    let format_3_1 = VALID_FORMAT_3_STORY.replace("\"3.0.0\"", "\"3.1.0\"");
    let cases = [
        (
            "        candidates: current_location",
            "command.candidates_type",
            "/commands/1/parameters/0/candidates",
        ),
        (
            "        candidates:\n          from: current_location",
            "command.candidates_from_type",
            "/commands/1/parameters/0/candidates/from",
        ),
        (
            "        candidates:\n          from: []",
            "command.candidates_from_empty",
            "/commands/1/parameters/0/candidates/from",
        ),
        (
            "        candidates:\n          from: [nearby]",
            "command.candidates_source_unknown",
            "/commands/1/parameters/0/candidates/from/0",
        ),
        (
            "        candidates:\n          from: [current_location, current_location]",
            "command.candidates_source_duplicate",
            "/commands/1/parameters/0/candidates/from/1",
        ),
        (
            "        candidates:\n          from: [current_location]\n          capabilities: portable",
            "command.candidates_capabilities_type",
            "/commands/1/parameters/0/candidates/capabilities",
        ),
        (
            "        candidates:\n          from: [current_location]\n          capabilities: []",
            "command.candidates_capabilities_empty",
            "/commands/1/parameters/0/candidates/capabilities",
        ),
        (
            "        candidates:\n          from: [current_location]\n          capabilities: [searchable]",
            "command.candidates_capability_unknown",
            "/commands/1/parameters/0/candidates/capabilities/0",
        ),
        (
            "        candidates:\n          from: [current_location]\n          capabilities: [portable, portable]",
            "command.candidates_capability_duplicate",
            "/commands/1/parameters/0/candidates/capabilities/1",
        ),
        (
            "        candidates:\n          from: [current_location]\n          where: visible",
            "command.candidates.unknown_field",
            "/commands/1/parameters/0/candidates/where",
        ),
        (
            "        canddiates:\n          from: [current_location]",
            "command.parameter.unknown_field",
            "/commands/1/parameters/0/canddiates",
        ),
    ];

    for (addition, code, pointer) in cases {
        let source = format_3_1.replace(
            "      - name: target\n        types: [entity]\n        min: 1\n        max: 1",
            &format!(
                "      - name: target\n        types: [entity]\n        min: 1\n        max: 1\n{addition}"
            ),
        );
        let report = report(source);
        assert!(
            report.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == code && diagnostic.pointer.as_deref() == Some(pointer)
            }),
            "missing {code} at {pointer}: {:#?}",
            report.diagnostics
        );
    }
}

#[test]
fn rejects_candidate_sources_and_capabilities_incompatible_with_parameter_types() {
    let format_3_1 = VALID_FORMAT_3_STORY.replace("\"3.0.0\"", "\"3.1.0\"");
    for (types, source) in [
        ("event", "current_location"),
        ("character", "inventory"),
        ("entity", "reachable"),
        ("event", "established"),
    ] {
        let source = format_3_1.replace(
            "      - name: target\n        types: [entity]\n        min: 1\n        max: 1",
            &format!(
                "      - name: target\n        types: [{types}]\n        min: 1\n        max: 1\n        candidates:\n          from: [{source}]"
            ),
        );
        let report = report(source);
        assert!(
            report.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "command.candidates_source_incompatible"
                    && diagnostic.pointer.as_deref()
                        == Some("/commands/1/parameters/0/candidates/from/0")
            }),
            "missing source compatibility diagnostic: {:#?}",
            report.diagnostics
        );
    }

    let source = format_3_1.replace(
        "      - name: target\n        types: [entity]\n        min: 1\n        max: 1",
        "      - name: target\n        types: [setting]\n        min: 1\n        max: 1\n        candidates:\n          from: [current_location]\n          capabilities: [portable]",
    );
    let setting_result = report(source);
    assert!(setting_result.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "command.candidates_capability_incompatible"
            && diagnostic.pointer.as_deref()
                == Some("/commands/1/parameters/0/candidates/capabilities/0")
    }));

    let source = format_3_1.replace(
        "      - name: target\n        types: [entity]\n        min: 1\n        max: 1",
        "      - name: target\n        types: [entity, setting]\n        min: 1\n        max: 1\n        candidates:\n          from: [reachable]\n          capabilities: [portable]",
    );
    let source_result = report(source);
    assert!(source_result.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "command.candidates_capability_incompatible"
            && diagnostic.pointer.as_deref()
                == Some("/commands/1/parameters/0/candidates/capabilities/0")
    }));
}

#[test]
fn take_and_drop_require_one_required_entity_parameter() {
    for command in ["command.take", "command.drop"] {
        let accepted = report(VALID_FORMAT_3_STORY.replace(
            "commands:\n",
            &format!(
                "commands:\n  - id: {command}\n    name: Inventory command\n    parameters:\n      - name: item\n        types: [entity]\n        min: 1\n        max: 1\n"
            ),
        ));
        assert!(accepted.valid, "{command}: {:#?}", accepted.diagnostics);

        for (case, parameters, pointer) in [
            ("zero", "", "/commands/0/parameters"),
            (
                "optional",
                "    parameters:\n      - name: item\n        types: [entity]\n        min: 0\n        max: 1\n",
                "/commands/0/parameters/0/min",
            ),
            (
                "multiple",
                "    parameters:\n      - name: item\n        types: [entity]\n        min: 1\n        max: 1\n      - name: other\n        types: [entity]\n        min: 1\n        max: 1\n",
                "/commands/0/parameters/1",
            ),
            (
                "wrong kind",
                "    parameters:\n      - name: item\n        types: [setting]\n        min: 1\n        max: 1\n",
                "/commands/0/parameters/0/types",
            ),
        ] {
            let report = report(VALID_FORMAT_3_STORY.replace(
                "commands:\n",
                &format!(
                    "commands:\n  - id: {command}\n    name: Inventory command\n{parameters}"
                ),
            ));
            let diagnostic = report
                .diagnostics
                .iter()
                .find(|diagnostic| {
                    diagnostic.code == "command.runtime_signature"
                        && diagnostic.subject_id.as_deref() == Some(command)
                })
                .unwrap_or_else(|| {
                    panic!(
                        "{command} {case} missing runtime signature: {:#?}",
                        report.diagnostics
                    )
                });
            assert!(!report.valid, "{command} {case}");
            assert_eq!(
                diagnostic.pointer.as_deref(),
                Some(pointer),
                "{command} {case}"
            );
            assert_eq!(
                diagnostic.message,
                format!("`{command}` must declare exactly one required entity parameter"),
                "{command} {case}"
            );
        }
    }
}

#[test]
fn validates_action_effect_container_and_operation_shapes() {
    for (source, expected_code) in [
        (
            VALID_FORMAT_3_STORY.replace(
                "    effects:\n      - operation: advance_time",
                "    effects: advance_time\n    ignored_effects:\n      - operation: advance_time",
            ),
            "command.effects_type",
        ),
        (
            VALID_FORMAT_3_STORY.replace(
                "      - operation: advance_time\n        minutes: 12",
                "      - advance_time",
            ),
            "command.effect_type",
        ),
        (
            VALID_FORMAT_3_STORY.replace(
                "      - operation: advance_time\n        minutes: 12",
                "      - minutes: 12",
            ),
            "command.effect_operation",
        ),
        (
            VALID_FORMAT_3_STORY.replace("operation: advance_time", "operation: warp_time"),
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
            "        text: The player has solved the mystery.",
            "        text: 42",
            "command.effect_text",
            "/commands/1/effects/6/text",
        ),
        (
            "        text: The trail has gone cold.",
            "        text: \"\"",
            "command.effect_text",
            "/commands/1/effects/7/text",
        ),
        (
            "        text: The examination reveals a carefully staged scene.",
            "        text: The examination reveals a carefully staged scene.\n        target: player",
            "command.effect_unknown_field",
            "/commands/1/effects/5/target",
        ),
    ];

    for (before, after, expected_code, expected_pointer) in cases {
        let report = report(VALID_FORMAT_3_STORY.replace(before, after));
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
fn every_world_effect_uses_the_same_command_and_trigger_validator() {
    let effects = [
        "operation: set_flag\n        flag: flag.knife_examined\n        value: true",
        "operation: move\n        subjects: [player]\n        setting: setting.study",
        "operation: transform\n        entity_from: entity.knife\n        entity_to: entity.replacement",
        "operation: reveal\n        entity: entity.knife",
        "operation: conceal\n        entity: entity.knife",
        "operation: learn_fact\n        fact_id: fact.knife_has_blood",
        "operation: establish_deduction\n        deduction_id: deduction.solution",
        "operation: describe\n        text: A precise authored beat.",
        "operation: advance_time\n        minutes: 1",
        "operation: win\n        text: Solved.",
        "operation: lose\n        text: Lost.",
    ];
    let base = VALID_FORMAT_3_STORY.replace(
        "  - id: entity.knife\n    initial:",
        "  - id: entity.replacement\n  - id: entity.knife\n    initial:",
    );
    for effect in effects {
        for (owner, source) in [
            (
                "command",
                base.replace(
                    "    description: Learn that the knife is present.\n    effects:\n      - operation: learn_fact\n        fact_id: fact.knife_is_present",
                    &format!("    description: Learn that the knife is present.\n    effects:\n      - {effect}"),
                ),
            ),
            (
                "trigger",
                base.replace(
                    "    effects:\n      - operation: set_flag\n        flag: flag.knife_analysis_complete\n        value: true\n        after: 20m",
                    &format!("    effects:\n      - {effect}"),
                ),
            ),
        ] {
            let diagnostics = report(source)
                .diagnostics
                .into_iter()
                .filter(|diagnostic| diagnostic.code.contains("effect"))
                .collect::<Vec<_>>();
            assert!(diagnostics.is_empty(), "{owner} {effect}: {diagnostics:#?}");
        }
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
        let result = codes(VALID_FORMAT_3_STORY.replace(before, after));
        assert!(
            result.contains(&expected_code.to_string()),
            "missing {expected_code}: {result:#?}"
        );
    }
}

#[test]
fn action_effects_report_unknown_authored_ids() {
    let report = report(VALID_FORMAT_3_STORY.replace(
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
fn format_3_no_longer_requires_a_fact_accepting_claim_action() {
    let report = report(VALID_FORMAT_3_STORY.replace("command.claim", "command.observe"));
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
    let source = VALID_FORMAT_3_STORY.to_string();

    let valid_report = report(source.clone());
    assert!(valid_report.valid, "{:#?}", valid_report.diagnostics);

    let single_input = source.replace(
        "inputs: [fact.knife_has_blood, fact.knife_connects_to_scene]",
        "inputs: [fact.knife_has_blood]",
    );
    let single_input_report = report(single_input);
    assert!(
        single_input_report.valid,
        "{:#?}",
        single_input_report.diagnostics
    );

    let malformed = source
        .replace(
            "statement: The knife carries the victim's blood.",
            "statement: \"\"",
        )
        .replace(
            "inputs: [fact.knife_has_blood, fact.knife_connects_to_scene]",
            "inputs: []",
        )
        .replace("truth: true", "truth: perhaps");
    let result = codes(malformed);
    assert!(result.contains(&"fact.missing_statement".to_string()));
    assert!(result.contains(&"deduction.inputs_type".to_string()));
    assert!(result.contains(&"deduction.truth_type".to_string()));
}

#[test]
fn validates_fact_reference_types() {
    let source = VALID_FORMAT_3_STORY.replace(
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
    let source = VALID_FORMAT_3_STORY
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
fn rejects_false_deductions_that_automatic_policy_would_establish() {
    let source = VALID_FORMAT_3_STORY
        .replace("truth: true", "truth: false")
        .replace(
            "deductions:\n",
            "deductions:\n  - id: deduction.earlier_false\n    conclusion: An earlier false theory.\n    inputs: [fact.knife_has_blood]\n    truth: false\n    contradicted_by: [fact.knife_connects_to_scene]\n",
        );
    let files = story_files(source);
    let deductions_source = &files
        .iter()
        .find(|file| file.path == "deductions.yaml")
        .expect("deductions file")
        .source;
    let (expected_line_index, expected_line_source) = deductions_source
        .lines()
        .enumerate()
        .filter(|(_, line)| line.trim() == "truth: false")
        .nth(1)
        .expect("second deduction truth line");
    let expected_line = expected_line_index + 1;
    let expected_column = expected_line_source.find("false").expect("false value") + 1;
    let report = validate(&files);
    assert!(!report.valid, "{:#?}", report.diagnostics);
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|item| {
            item.code == "deduction.automatic_false"
                && item.pointer.as_deref() == Some("/deductions/1/truth")
        })
        .expect("automatic false diagnostic");
    assert_eq!(diagnostic.pointer.as_deref(), Some("/deductions/1/truth"));
    assert_eq!(diagnostic.range.as_ref().unwrap().start.line, expected_line);
    assert_eq!(
        diagnostic.range.as_ref().unwrap().start.column,
        expected_column
    );
    assert!(report
        .diagnostics
        .iter()
        .any(|item| item.code == "deduction.false_without_contradiction"));
}

#[test]
fn automatic_deduction_health_finds_exact_solution_overlap_without_flagging_intermediate_notes() {
    let exact = format_3_3_question_story().replace(
        "    truth: true",
        "    truth: true\n    solves:\n      culprit: character.culprit",
    );
    let exact_report = report(exact);
    assert!(exact_report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "deduction.solution_equivalent"
            && diagnostic.pointer.as_deref() == Some("/deductions/0/conclusion")
            && diagnostic.subject_id.as_deref() == Some("deduction.solution")
    }));

    let partial = format_3_3_question_story().replace(
        "    truth: true",
        "    truth: true\n    solves:\n      weapon: entity.knife",
    );
    let partial_report = report(partial);
    assert!(partial_report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "deduction.solution_answer_overlap"
            && diagnostic.pointer.as_deref() == Some("/deductions/0/solves")
    }));
    assert!(!partial_report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "deduction.solution_equivalent"));

    for source in [
        format_3_3_question_story().replace(
            "    truth: true",
            "    truth: true\n    solves:\n      weapon: entity.knife\n      location: setting.study",
        ),
        format_3_3_question_story()
            .replace("      ordered: true", "")
            .replace(
                "    truth: true",
                "    truth: true\n    solves:\n      weapon: entity.knife\n      location: setting.study",
            ),
    ] {
        assert!(report(source)
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "deduction.solution_equivalent"));
    }

    let intermediate = report(format_3_3_question_story().replace(
        "The knife was used in the study.",
        "The suspect handled an object before entering the study.",
    ));
    assert!(!intermediate.diagnostics.iter().any(|diagnostic| {
        matches!(
            diagnostic.code.as_str(),
            "deduction.solution_equivalent" | "deduction.solution_answer_overlap"
        )
    }));
    let inconclusive = report(format_3_3_question_story().replace(
        "The knife was used in the study.",
        "The evidence bears on several possible answers.",
    ));
    assert!(!inconclusive.diagnostics.iter().any(|diagnostic| {
        matches!(
            diagnostic.code.as_str(),
            "deduction.solution_equivalent" | "deduction.solution_answer_overlap"
        )
    }));
}

#[test]
fn speculative_automatic_deduction_is_reviewable_and_source_located() {
    for conclusion in [
        "Perhaps the knife was used in the study.",
        "The suspect might have entered through the study.",
        "The culprit possibly entered through the study.",
        "The culprit could have used the knife.",
    ] {
        let source = VALID_FORMAT_3_STORY.replace("The knife was used in the study.", conclusion);
        let files = story_files(source);
        let deductions_source = &files
            .iter()
            .find(|file| file.path == "deductions.yaml")
            .expect("deductions file")
            .source;
        let expected_line = deductions_source
            .lines()
            .position(|line| line.contains(conclusion))
            .expect("conclusion line")
            + 1;
        let report = validate(&files);
        let diagnostic = report
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == "deduction.automatic_speculative")
            .expect("speculative deduction diagnostic");
        assert_eq!(
            diagnostic.pointer.as_deref(),
            Some("/deductions/0/conclusion")
        );
        assert_eq!(diagnostic.range.as_ref().unwrap().start.line, expected_line);
    }
}

#[test]
fn contradictable_and_terminal_solution_notes_are_explicit_case_health_findings() {
    let source = format_3_3_question_story()
        .replace(
            "    truth: true",
            "    truth: true\n    contradicted_by: [fact.knife_has_blood]\n    solves:\n      culprit: character.culprit",
        )
        .replace(
            "    text: You answer every question correctly.",
            "    text: You answer every question correctly.\n    requires: [deduction.solution]",
        );
    let files = story_files(source);
    let win_states_source = &files
        .iter()
        .find(|file| file.path == "win_states.yaml")
        .expect("win states file")
        .source;
    let terminal_requirement_line = win_states_source
        .lines()
        .position(|line| line.trim() == "- deduction.solution")
        .expect("terminal requirement line")
        + 1;
    let report = validate(&files);
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "deduction.automatic_contradictable"
            && diagnostic.pointer.as_deref() == Some("/deductions/0/contradicted_by")
    }));
    let terminal = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "end_state.solution_equivalent_deduction")
        .expect("terminal solution-equivalent diagnostic");
    assert_eq!(
        terminal.pointer.as_deref(),
        Some("/win_states/0/requires/0")
    );
    assert_eq!(
        terminal.range.as_ref().unwrap().start.line,
        terminal_requirement_line
    );
}

#[test]
fn validates_deduction_solution_shape() {
    let source = VALID_FORMAT_3_STORY.replace(
        "truth: true",
        "truth: true\n    solves:\n      culprit: character.culprit\n      weapon: entity.knife\n      location: setting.study\n      time: \"29:00\"",
    );
    assert!(codes(source).contains(&"deduction.solves_invalid_time".to_string()));
}

#[test]
fn commands_and_triggers_remain_optional() {
    let semantic = VALID_STORY
        .split("commands:\n")
        .next()
        .expect("story before optional sections");
    let source = format!(
        "{semantic}cards:\n  - tag_id: 0\n    subject: setting.foyer\n  - tag_id: 1\n    subject: setting.study\n  - tag_id: 2\n    subject: character.victim\n  - tag_id: 3\n    subject: character.culprit\n  - tag_id: 4\n    subject: entity.knife\n"
    );
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

fn format_3_2_reference_story() -> String {
    include_str!("fixtures/format-3.2-reference-story.yaml").to_string()
}

#[test]
fn format_3_2_negotiates_features_and_retains_ordered_reference_provenance() {
    let report = report(format_3_2_reference_story());
    assert!(report.valid, "{:#?}", report.diagnostics);
    assert_eq!(report.features, ["reference_text_v1"]);
    let opening = report
        .reference_text
        .iter()
        .find(|field| field.pointer == "/case/opening")
        .expect("opening was resolved");
    assert_eq!(opening.resolved, "Morgan Vale waits in The Foyer.");
    assert_eq!(opening.provenance.len(), 2);
    assert_eq!(
        opening.provenance[0].expression.target_id,
        "character.victim"
    );
    assert_eq!(opening.provenance[0].resolved_path, "name");
    assert_eq!(opening.provenance[1].expression.target_id, "setting.foyer");
    assert_eq!(opening.provenance[1].resolved_path, "name");
}

#[test]
fn format_3_2_feature_negotiation_fails_closed_before_reference_interpretation() {
    let files = story_files(format_3_2_reference_story());
    let consumer_report = validate_with_supported_features(&files, &[]);
    assert_eq!(consumer_report.features, ["reference_text_v1"]);
    assert!(consumer_report.reference_text.is_empty());
    assert_eq!(consumer_report.diagnostics.len(), 1);
    assert_eq!(
        consumer_report.diagnostics[0].code,
        "feature.consumer_unsupported"
    );

    let duplicate = report(format_3_2_reference_story().replace(
        "features: [reference_text_v1]",
        "features: [reference_text_v1, reference_text_v1]",
    ));
    assert!(duplicate
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "feature.duplicate"));

    let unknown = report(format_3_2_reference_story().replace(
        "features: [reference_text_v1]",
        "features: [reference_text_v2]",
    ));
    assert!(unknown
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "feature.unknown"));
}

#[test]
fn reference_text_requires_opt_in_and_preserves_format_3_1_literals() {
    let without_feature =
        format_3_2_reference_story().replace("  features: [reference_text_v1]\n", "");
    assert!(report(without_feature)
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "reference_text.feature_required"));

    let format_3_1 = format_3_2_reference_story()
        .replace("\"3.2.0\"", "\"3.1.0\"")
        .replace("  features: [reference_text_v1]\n", "");
    let report = report(format_3_1);
    assert!(report.valid, "{:#?}", report.diagnostics);
    assert!(report.reference_text.is_empty());
}

#[test]
fn reference_text_enforces_paths_disclosure_and_cycles() {
    let mechanical = format_3_2_reference_story().replace(
        "[[character.victim]] waits",
        "[[character.victim.voice_id]] waits",
    );
    assert!(report(mechanical)
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "reference_text.path_disallowed"));

    let private = format_3_2_reference_story()
        .replace(
            "    name: Morgan Vale\n",
            "    name: Morgan Vale\n    narrator_guidance:\n      secret: Morgan knows the answer.\n",
        )
        .replace("[[character.victim]] waits", "[[character.victim.narrator_guidance.secret]] waits");
    let private_report = report(private);
    let private_diagnostic = private_report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "reference_text.disclosure")
        .expect("private disclosure diagnostic");
    assert_eq!(private_diagnostic.pointer.as_deref(), Some("/case/opening"));
    assert!(private_diagnostic.range.is_some());
    assert_eq!(
        private_diagnostic.message,
        "baseline player-safe prose cannot reference private narrator path `character.victim.narrator_guidance.secret`"
    );

    let cycle = format_3_2_reference_story()
        .replace("name: Morgan Vale", "name: '[[setting.foyer.name]]'")
        .replace("name: The Foyer", "name: '[[character.victim.name]]'");
    let cycle_report = report(cycle);
    let diagnostic = cycle_report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "reference_text.cycle")
        .expect("cycle diagnostic");
    assert!(diagnostic.related.len() >= 2);
    assert!(diagnostic
        .related
        .iter()
        .all(|location| location.pointer.is_some() && location.range.is_some()));
}

#[test]
fn reference_text_resolves_multihop_nested_facts_and_testimony() {
    let multihop =
        format_3_2_reference_story().replace("name: Morgan Vale", "name: '[[setting.foyer.name]]'");
    let multihop_report = report(multihop);
    assert!(multihop_report.valid, "{:#?}", multihop_report.diagnostics);
    let opening = multihop_report
        .reference_text
        .iter()
        .find(|field| field.pointer == "/case/opening")
        .unwrap();
    assert_eq!(opening.resolved, "The Foyer waits in The Foyer.");
    assert_eq!(opening.provenance.len(), 3);
    assert!(
        opening
            .provenance
            .iter()
            .all(|origin| origin.range.is_some()),
        "{opening:#?}"
    );

    let fact = format_3_2_reference_story()
        .replace(
            "The knife is present.",
            "'[[character.victim.name]] saw the knife.'",
        )
        .replace(
            "conclusion: The knife was used in the study.",
            "conclusion: '[[fact.knife_is_present]]'",
        );
    let fact_report = report(fact);
    assert!(fact_report.valid, "{:#?}", fact_report.diagnostics);
    assert!(fact_report.reference_text.iter().any(|field| {
        field.pointer == "/deductions/0/conclusion"
            && field.resolved == "Morgan Vale saw the knife."
    }));

    let testimony = format_3_2_reference_story()
        .replace(
            "  format_version: \"3.2.0\"",
            "  format_version: \"3.2.0\"\n  ruleset:\n    id: ruleset.standard_mystery\n    version: \"2.0.0\"",
        )
        .replace(
            "    name: Morgan Vale\n",
            "    name: Morgan Vale\n    testimony:\n      - id: testimony.victim_account\n        text: Morgan heard the storm.\n        requires: [command.question, character.victim]\n        reveals: []\n",
        )
        .replace(
            "conclusion: The knife was used in the study.",
            "conclusion: '[[testimony.victim_account]]'",
        );
    let testimony_report = report(testimony);
    assert!(
        testimony_report.valid,
        "{:#?}",
        testimony_report.diagnostics
    );
    assert!(testimony_report.reference_text.iter().any(|field| {
        field.pointer == "/deductions/0/conclusion" && field.resolved == "Morgan heard the storm."
    }));
}

#[test]
fn reference_text_reports_unknown_missing_non_string_empty_and_malformed_targets() {
    let cases = [
        ("[[character.unknown]]", "reference_text.unknown_id"),
        (
            "[[character.victim.portrayal.demeanor]]",
            "reference_text.unknown_path",
        ),
        ("[[character.victim.role]]", "reference_text.non_string"),
        ("[[character.victim.name]]", "reference_text.empty"),
        ("[[character.victim", "reference_text.malformed"),
    ];
    for (expression, expected) in cases {
        let mut source = format_3_2_reference_story().replace(
            "[[character.victim]] waits in [[setting.foyer.name]].",
            expression,
        );
        if expected == "reference_text.non_string" {
            source = source.replace("name: Morgan Vale", "name: Morgan Vale\n    role: 42");
        }
        if expected == "reference_text.empty" {
            source = source.replace("name: Morgan Vale", "name: ''");
        }
        let report = report(source);
        let diagnostic = report
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == expected)
            .unwrap_or_else(|| panic!("missing {expected}: {:#?}", report.diagnostics));
        assert_eq!(diagnostic.pointer.as_deref(), Some("/case/opening"));
        assert!(diagnostic.range.is_some(), "{diagnostic:#?}");
    }

    for non_string in ["{}", "[lead]"] {
        let source = format_3_2_reference_story()
            .replace(
                "[[character.victim]] waits in [[setting.foyer.name]].",
                "[[character.victim.role]]",
            )
            .replace(
                "name: Morgan Vale",
                &format!("name: Morgan Vale\n    role: {non_string}"),
            );
        assert!(report(source)
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "reference_text.non_string"));
    }

    let nested_malformed =
        format_3_2_reference_story().replace("name: Morgan Vale", "name: '[[setting.foyer'");
    let nested_report = report(nested_malformed);
    let nested_diagnostic = nested_report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "reference_text.malformed")
        .expect("nested malformed diagnostic");
    assert_eq!(
        nested_diagnostic.pointer.as_deref(),
        Some("/characters/0/name")
    );
    assert!(nested_diagnostic.range.is_some());
}

#[test]
fn private_reference_text_can_reference_public_and_private_but_public_cannot_reveal_gated() {
    let private = format_3_2_reference_story()
        .replace(
            "    name: Morgan Vale\n",
            "    name: Morgan Vale\n    narrator_guidance:\n      secret: The private answer.\n      goal: 'Protect [[character.victim.name]] and [[character.victim.narrator_guidance.secret]]'\n",
        );
    let private_report = report(private);
    assert!(private_report.valid, "{:#?}", private_report.diagnostics);
    assert!(private_report.reference_text.iter().any(|field| {
        field.pointer == "/characters/0/narrator_guidance/goal"
            && field.resolved == "Protect Morgan Vale and The private answer."
    }));

    let gated_leak = format_3_2_reference_story().replace(
        "[[character.victim]] waits in [[setting.foyer.name]].",
        "[[fact.knife_is_present]]",
    );
    let gated_report = report(gated_leak);
    let gated_diagnostic = gated_report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "reference_text.disclosure")
        .expect("gated disclosure diagnostic");
    assert_eq!(gated_diagnostic.pointer.as_deref(), Some("/case/opening"));
    assert!(gated_diagnostic.range.is_some());
    assert_eq!(
        gated_diagnostic.message,
        "baseline player-safe prose cannot reference gated player-safe path `fact.knife_is_present.statement`; this would bypass the target's disclosure gate"
    );

    let gated_to_private = format_3_2_reference_story()
        .replace(
            "    name: Morgan Vale\n",
            "    name: Morgan Vale\n    narrator_guidance:\n      secret: The private answer.\n",
        )
        .replace(
            "statement: The knife is present.",
            "statement: '[[character.victim.narrator_guidance.secret]]'",
        );
    let gated_private_report = report(gated_to_private);
    let gated_private_diagnostic = gated_private_report
        .diagnostics
        .iter()
        .find(|diagnostic| {
            diagnostic.code == "reference_text.disclosure"
                && diagnostic.pointer.as_deref() == Some("/entities/0/facts/0/statement")
        })
        .expect("gated-to-private disclosure diagnostic");
    assert!(gated_private_diagnostic.range.is_some());
    assert_eq!(
        gated_private_diagnostic.message,
        "gated player-safe prose cannot reference private narrator path `character.victim.narrator_guidance.secret`"
    );
}

#[test]
fn reference_text_handles_multiline_repeated_and_escaped_expressions() {
    let source = format_3_2_reference_story().replace(
        "  opening: \"[[character.victim]] waits in [[setting.foyer.name]].\"",
        "  opening: |\n    [[character.victim]] greets [[character.victim]].\n    \\[[character.victim]] stays literal.",
    );
    let report = report(source);
    assert!(report.valid, "{:#?}", report.diagnostics);
    let opening = report
        .reference_text
        .iter()
        .find(|field| field.pointer == "/case/opening")
        .unwrap();
    assert_eq!(
        opening.resolved,
        "Morgan Vale greets Morgan Vale.\n[[character.victim]] stays literal.\n"
    );
    assert_eq!(opening.provenance.len(), 2);
    let ranges = opening
        .provenance
        .iter()
        .map(|origin| origin.range.unwrap())
        .collect::<Vec<_>>();
    assert_ne!(ranges[0], ranges[1]);
}

#[test]
fn features_are_rejected_before_format_3_2() {
    let source = format_3_2_reference_story().replace("\"3.2.0\"", "\"3.1.0\"");
    let report = report(source);
    assert_eq!(report.diagnostics.len(), 1, "{:#?}", report.diagnostics);
    assert_eq!(report.diagnostics[0].code, "feature.format_incompatible");
    assert!(report.reference_text.is_empty());
}

fn reference_range_report(
    case_source: &str,
    character_source: &str,
) -> narrator_validator::ValidationReport {
    validate(&[
        SourceFile {
            path: "case.yaml".to_string(),
            source: case_source.to_string(),
        },
        SourceFile {
            path: "characters.yaml".to_string(),
            source: character_source.to_string(),
        },
    ])
}

#[test]
fn reference_ranges_are_anchored_to_identical_owning_scalars_and_repetitions() {
    let report = reference_range_report(
        "case:\n  id: case.ranges\n  format_version: \"3.2.0\"\n  features: [reference_text_v1]\n  players: { min: 1, max: 1 }\n  initial_time: \"20:00\"\n  title: \"[[character.echo]]\"\n  premise: \"[[character.echo]]\"\n  opening: \"[[character.echo]] then [[character.echo]]\"\n",
        "characters:\n  - id: character.echo\n    name: Echo Vale\n    description: A witness.\n",
    );
    let field = |pointer: &str| {
        report
            .reference_text
            .iter()
            .find(|field| field.pointer == pointer)
            .unwrap_or_else(|| panic!("missing {pointer}: {:#?}", report.diagnostics))
    };
    assert_eq!(
        field("/case/title").provenance[0].range.unwrap().start.line,
        7
    );
    assert_eq!(
        field("/case/premise").provenance[0]
            .range
            .unwrap()
            .start
            .line,
        8
    );
    let opening = field("/case/opening");
    assert_eq!(opening.provenance[0].range.unwrap().start.line, 9);
    assert_eq!(opening.provenance[1].range.unwrap().start.line, 9);
    assert_ne!(
        opening.provenance[0].range.unwrap().start.column,
        opening.provenance[1].range.unwrap().start.column
    );
}

#[test]
fn reference_ranges_follow_literal_and_folded_block_scalar_lines() {
    let report = reference_range_report(
        "case:\n  id: case.blocks\n  format_version: \"3.2.0\"\n  features: [reference_text_v1]\n  players: { min: 1, max: 1 }\n  initial_time: \"20:00\"\n  title: Block ranges\n  premise: |\n    First line.\n    [[character.echo]] literal line.\n  opening: >\n    Folded lead\n    [[character.echo]] folded line.\n",
        "characters:\n  - id: character.echo\n    name: Echo Vale\n    description: A witness.\n",
    );
    let premise = report
        .reference_text
        .iter()
        .find(|field| field.pointer == "/case/premise")
        .unwrap();
    let opening = report
        .reference_text
        .iter()
        .find(|field| field.pointer == "/case/opening")
        .unwrap();
    assert_eq!(premise.provenance[0].range.unwrap().start.line, 10);
    assert_eq!(opening.provenance[0].range.unwrap().start.line, 13);
}

#[test]
fn reference_ranges_follow_flow_mapping_and_sequence_pointers() {
    let report = reference_range_report(
        "case: { id: case.flow, format_version: \"3.2.0\", features: [reference_text_v1], players: { min: 1, max: 1 }, initial_time: \"20:00\", title: \"[[character.echo]]\", opening: \"[[character.echo]]\" }\n",
        "characters: [{ id: character.echo, name: Echo Vale, description: A witness. }]\n",
    );
    let title = report
        .reference_text
        .iter()
        .find(|field| field.pointer == "/case/title")
        .unwrap();
    let opening = report
        .reference_text
        .iter()
        .find(|field| field.pointer == "/case/opening")
        .unwrap();
    assert_eq!(title.provenance[0].range.unwrap().start.line, 1);
    assert_eq!(opening.provenance[0].range.unwrap().start.line, 1);
    assert_ne!(
        title.provenance[0].range.unwrap().start.column,
        opening.provenance[0].range.unwrap().start.column
    );
}

#[test]
fn malformed_and_cycle_ranges_match_their_exact_field_pointers() {
    let malformed = reference_range_report(
        "case:\n  id: case.malformed\n  format_version: \"3.2.0\"\n  features: [reference_text_v1]\n  players: { min: 1, max: 1 }\n  initial_time: \"20:00\"\n  title: \"[[character.echo\"\n  opening: \"[[character.echo\"\n",
        "characters:\n  - id: character.echo\n    name: Echo Vale\n    description: A witness.\n",
    );
    let malformed_fields = malformed
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "reference_text.malformed")
        .map(|diagnostic| {
            (
                diagnostic.pointer.as_deref().unwrap(),
                diagnostic.range.unwrap().start.line,
            )
        })
        .collect::<BTreeMap<_, _>>();
    assert_eq!(malformed_fields["/case/title"], 7);
    assert_eq!(malformed_fields["/case/opening"], 8);

    let cycle = reference_range_report(
        "case:\n  id: case.cycle_ranges\n  format_version: \"3.2.0\"\n  features: [reference_text_v1]\n  players: { min: 1, max: 1 }\n  initial_time: \"20:00\"\n  title: Cycle ranges\n  opening: \"[[character.alpha]]\"\n",
        "characters:\n  - id: character.alpha\n    name: \"[[character.beta]]\"\n    description: Alpha.\n  - id: character.beta\n    name: \"[[character.alpha]]\"\n    description: Beta.\n",
    );
    let diagnostic = cycle
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "reference_text.cycle")
        .unwrap();
    for related in &diagnostic.related {
        let pointer = related.pointer.as_deref().unwrap();
        let line = related.range.unwrap().start.line;
        match pointer {
            "/characters/0/name" => assert_eq!(line, 3),
            "/characters/1/name" => assert_eq!(line, 6),
            other => panic!("unexpected cycle pointer {other}"),
        }
    }
}

#[test]
fn cycle_ranges_follow_the_participating_expression_not_the_first_reference() {
    let characters = "characters:\n  - id: character.alpha\n    name: \"[[setting.foyer]] and [[character.beta]]\"\n    description: Alpha.\n  - id: character.beta\n    name: \"[[character.alpha]]\"\n    description: Beta.\n";
    let report = validate(&[
        SourceFile {
            path: "case.yaml".to_string(),
            source: "case:\n  id: case.cycle_edge\n  format_version: \"3.2.0\"\n  features: [reference_text_v1]\n  players: { min: 1, max: 1 }\n  initial_time: \"20:00\"\n  title: Cycle edge\n  opening: \"[[character.alpha]]\"\n"
                .to_string(),
        },
        SourceFile {
            path: "characters.yaml".to_string(),
            source: characters.to_string(),
        },
        SourceFile {
            path: "settings.yaml".to_string(),
            source: "settings:\n  - id: setting.foyer\n    name: The Foyer\n    description: An entry room.\nroutes: []\n".to_string(),
        },
    ]);
    let diagnostic = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "reference_text.cycle")
        .expect("cycle diagnostic");
    let alpha_edge = diagnostic
        .related
        .iter()
        .find(|location| location.pointer.as_deref() == Some("/characters/0/name"))
        .expect("alpha cycle edge");
    let range = alpha_edge.range.expect("alpha edge range");
    let alpha_line = characters.lines().nth(2).unwrap();
    let expected_column = alpha_line.find("[[character.beta]]").unwrap() + 1;
    let unrelated_column = alpha_line.find("[[setting.foyer]]").unwrap() + 1;
    assert_eq!(range.start.line, 3);
    assert_eq!(range.start.column, expected_column);
    assert_ne!(range.start.column, unrelated_column);
    assert_eq!(
        range.end.column - range.start.column,
        "[[character.beta]]".len()
    );
}

#[test]
fn format_3_7_step_story_validates_clean() {
    let report = report(format_3_7_step_story());
    assert!(report.valid, "{:#?}", report.diagnostics);
    assert_eq!(report.format_version.as_deref(), Some("3.7.0"));
    assert!(report.diagnostics.is_empty());
}

#[test]
fn format_3_7_step_schema_rejects_every_malformed_shape_at_exact_pointers() {
    let cases: Vec<(String, &str, &str)> = vec![
        (
            format_3_7_step_story().replace("id: step.name_culprit", "id: Step.NameCulprit"),
            "id.invalid",
            "/solution/steps/0/id",
        ),
        (
            format_3_7_step_story().replace("id: step.name_culprit", "id: action.name_culprit"),
            "id.wrong_prefix",
            "/solution/steps/0/id",
        ),
        (
            format_3_7_step_story().replace(
                "id: step.identify_weapon_location_and_method",
                "id: step.name_culprit",
            ),
            "id.duplicate",
            "/solution/steps/1/id",
        ),
        (
            format_3_7_step_story().replace(
                "    - id: step.name_culprit",
                "    - bogus_field: true\n      id: step.name_culprit",
            ),
            "solution.step_unknown_field",
            "/solution/steps/0/bogus_field",
        ),
        (
            format_3_7_step_story().replace(
                "cards: [answer.method.stabbed, answer.method.struck]",
                "cards: [answer.method.stabbed, entity.knife]",
            ),
            "solution.step_card_duplicate",
            "/solution/steps/1/rows/1/cards/1",
        ),
        (
            format_3_7_step_story().replace(
                "        - match: ordered\n          cards: [entity.knife, setting.study]",
                "        - match: ordered\n          n: 2\n          cards: [entity.knife, setting.study]",
            ),
            "solution.row_ordered_has_n",
            "/solution/steps/1/rows/0/n",
        ),
        (
            format_3_7_step_story().replace(
                "      on_success:\n        effects:\n          - operation: set_flag\n            flag: flag.culprit_named\n            value: true",
                "      on_success:\n        bogus: true\n        effects:\n          - operation: set_flag\n            flag: flag.culprit_named\n            value: true",
            ),
            "solution.step_outcome_unknown_field",
            "/solution/steps/0/on_success/bogus",
        ),
        (
            format_3_7_step_story().replace(
                "          - operation: set_flag\n            flag: flag.culprit_named\n            value: true",
                "          - operation: advance_time\n            minutes: 5",
            ),
            "solution.step_effect_operation_unsupported",
            "/solution/steps/0/on_success/effects/0/operation",
        ),
        (
            format_3_7_step_story().replace("max_attempts: 3", "max_attempts: 0"),
            "solution.max_attempts_invalid",
            "/solution/max_attempts",
        ),
        (
            format_3_7_step_story().replace(
                "solution:\n  max_attempts: 3",
                "solution:\n  win_state: end.full_solution\n  max_attempts: 3",
            ),
            "solution.legacy_contract",
            "/solution",
        ),
    ];
    for (source, code, pointer) in cases {
        let report = report(source);
        assert!(
            report.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == code && diagnostic.pointer.as_deref() == Some(pointer)
            }),
            "missing {code} at {pointer}: {:#?}",
            report.diagnostics
        );
    }
}

#[test]
fn format_3_7_answer_deck_bindings_reject_unknown_reserved_and_mismatched_tags() {
    // `deck.subject_unknown` extends to the `answer.*` namespace.
    let unknown = report(format_3_7_step_story().replace(
        "subject: answer.method.struck",
        "subject: answer.method.nonexistent",
    ));
    assert!(unknown.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "deck.subject_unknown"
            && diagnostic.subject_id.as_deref() == Some("answer.method.nonexistent")
    }));

    // `deck.answer_tag_id_mismatch` when an `answer.*` card's `deck.yaml`
    // `tag_id` doesn't match its ruleset-assigned value exactly.
    let mismatched = report(format_3_7_step_story().replace(
        "tag_id: 2093\n    subject: answer.method.stabbed",
        "tag_id: 2091\n    subject: answer.method.stabbed",
    ));
    assert!(mismatched.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "deck.answer_tag_id_mismatch"
            && diagnostic.subject_id.as_deref() == Some("answer.method.stabbed")
    }));

    // `deck.tag_id_reserved_ruleset_answer_deck`: a setting/character/
    // entity/command subject cannot bind a tag inside 2000-2112.
    let reserved = report(
        format_3_7_step_story()
            .replace(
                "entities:\n  - id: entity.knife",
                "entities:\n  - id: entity.spare\n    description: An unused spare object.\n  - id: entity.knife",
            )
            .replace(
                "cards:\n  - tag_id: 0",
                "cards:\n  - tag_id: 2050\n    subject: entity.spare\n  - tag_id: 0",
            ),
    );
    assert!(reserved.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "deck.tag_id_reserved_ruleset_answer_deck"
            && diagnostic.subject_id.as_deref() == Some("entity.spare")
    }));
}

#[test]
fn format_3_7_answer_subjects_are_knowledge_never_world_state() {
    // Never a container/placement target.
    let container = report(format_3_7_step_story().replace(
        "initial:\n      container: setting.study",
        "initial:\n      container: answer.method.stabbed",
    ));
    assert!(container.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "subject.answer_no_world_state"
            && diagnostic.pointer.as_deref() == Some("/entities/0/initial/container")
    }));

    // Never `at`-eligible in a persistent condition.
    let predicate = report(format_3_7_step_story().replace(
        "entities:\n  - id: entity.knife\n    description: A knife found in the study.\n    initial:\n      container: setting.study",
        "entities:\n  - id: entity.knife\n    description: A knife found in the study.\n    initial:\n      container: setting.study\n    facts:\n      - id: fact.spurious\n        statement: Nonsense.\n        when:\n          all:\n            - at: answer.method.stabbed",
    ));
    assert!(predicate.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "subject.answer_no_world_state"
    }));
}

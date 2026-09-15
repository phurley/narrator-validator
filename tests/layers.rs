//! Integration coverage for `merge_layers` (ADR-022 §2) against a real,
//! self-contained fixture story rather than a sibling checkout, per this
//! workspace's rule that component-repo tests never depend on one.

use std::fs;
use std::path::Path;

use narrator_validator::{merge_layers, validate, SourceFile};

fn read_fixture_dir(dir: &Path) -> Vec<SourceFile> {
    let mut files: Vec<SourceFile> = fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("reading fixture dir {dir:?}: {error}"))
        .map(|entry| {
            let path = entry.unwrap().path();
            SourceFile {
                path: path.file_name().unwrap().to_string_lossy().into_owned(),
                source: fs::read_to_string(&path).unwrap(),
            }
        })
        .collect();
    files.sort_by(|left, right| left.path.cmp(&right.path));
    files
}

/// A real story's `case.yaml` sets `format_version` and `ruleset`, which are
/// deck-only fields from Format 3.12 (ADR-022 §3). Merging it with an empty
/// deck layer is therefore an ownership violation, not a no-op: a bare story
/// is only a valid *import candidate* once the backend strips those fields
/// (narrator-backend#809); the CLI path is narrator-validator#137.
#[test]
fn empty_deck_rejects_a_real_story_that_still_sets_deck_only_fields() {
    let story = read_fixture_dir(Path::new("tests/fixtures/format-3.3-solve-card-sets"));
    let error = merge_layers(&[], &story).unwrap_err();

    assert!(error.iter().any(|d| d.code == "layer.deck_only_field"
        && d.pointer.as_deref() == Some("/case/format_version")));
    assert!(error.iter().any(
        |d| d.code == "layer.deck_only_field" && d.pointer.as_deref() == Some("/case/ruleset")
    ));
}

/// The realistic shape of the same fixture once `format_version` and
/// `ruleset` live on the deck instead of the story: merging reproduces the
/// original story byte-for-byte and validates identically.
#[test]
fn a_deck_holding_only_deck_only_fields_round_trips_the_rest_of_a_real_story() {
    let story = read_fixture_dir(Path::new("tests/fixtures/format-3.3-solve-card-sets"));

    let deck = vec![SourceFile {
        path: "case.yaml".to_string(),
        source: "case:\n  format_version: \"3.3.0\"\n  ruleset:\n    id: ruleset.standard_mystery\n    version: \"3.0.0\"\n"
            .to_string(),
    }];
    let story_without_deck_only_fields: Vec<SourceFile> = story
        .iter()
        .cloned()
        .map(|file| {
            if file.path != "case.yaml" {
                return file;
            }
            let mut value: serde_yaml::Value = serde_yaml::from_str(&file.source).unwrap();
            let case = value.get_mut("case").unwrap().as_mapping_mut().unwrap();
            case.remove("format_version");
            case.remove("ruleset");
            SourceFile {
                path: file.path,
                source: serde_yaml::to_string(&value).unwrap(),
            }
        })
        .collect();

    let merged = merge_layers(&deck, &story_without_deck_only_fields)
        .expect("deck-only fields living on the deck merge without diagnostics");

    let merged_case_value: serde_yaml::Value = serde_yaml::from_str(
        &merged
            .iter()
            .find(|f| f.path == "case.yaml")
            .unwrap()
            .source,
    )
    .unwrap();
    let original_case_value: serde_yaml::Value =
        serde_yaml::from_str(&story.iter().find(|f| f.path == "case.yaml").unwrap().source)
            .unwrap();
    assert_eq!(merged_case_value, original_case_value);

    let before = validate(&story);
    let after = validate(&merged);
    assert_eq!(before.diagnostics, after.diagnostics);
    assert_eq!(before.valid, after.valid);
}

/// A deck layer holding the shared world of a complete story (its own
/// deck-only and story-only `case` fields split off per ADR-022 §3),
/// overlaid by a story layer that supplies the story-only `case` content,
/// adds a flag, overrides a character's name, and adds a new setting. The
/// merged effective set must still validate cleanly.
#[test]
fn merging_a_fixture_deck_and_story_overlay_produces_no_validation_errors() {
    let fixture = read_fixture_dir(Path::new("tests/fixtures/format-3.3-solve-card-sets"));
    let fixture_case: serde_yaml::Value = serde_yaml::from_str(
        &fixture
            .iter()
            .find(|f| f.path == "case.yaml")
            .unwrap()
            .source,
    )
    .unwrap();
    let case_map = fixture_case.get("case").unwrap().as_mapping().unwrap();

    // The deck keeps the shared world plus its deck-only `case` fields; the
    // story-only fields (`id`, `opening`, root `solution`) and the
    // story-only `win_states.yaml` file move to the story layer.
    let mut deck_case_map = case_map.clone();
    for field in ["id", "opening"] {
        deck_case_map.remove(field);
    }
    let deck_case = SourceFile {
        path: "case.yaml".to_string(),
        source: serde_yaml::to_string(&serde_yaml::Mapping::from_iter([(
            serde_yaml::Value::String("case".to_string()),
            serde_yaml::Value::Mapping(deck_case_map),
        )]))
        .unwrap(),
    };
    let deck: Vec<SourceFile> = fixture
        .iter()
        .filter(|f| f.path != "case.yaml" && f.path != "win_states.yaml")
        .cloned()
        .chain([deck_case])
        .collect();

    let mut story_case_map = serde_yaml::Mapping::new();
    for field in ["id", "opening"] {
        story_case_map.insert(
            serde_yaml::Value::String(field.to_string()),
            case_map.get(field).unwrap().clone(),
        );
    }
    let story_case_value = serde_yaml::Mapping::from_iter([
        (
            serde_yaml::Value::String("case".to_string()),
            serde_yaml::Value::Mapping(story_case_map),
        ),
        (
            serde_yaml::Value::String("solution".to_string()),
            fixture_case.get("solution").unwrap().clone(),
        ),
    ]);

    let story = vec![
        SourceFile {
            path: "case.yaml".to_string(),
            source: serde_yaml::to_string(&story_case_value).unwrap(),
        },
        fixture
            .iter()
            .find(|f| f.path == "win_states.yaml")
            .unwrap()
            .clone(),
        SourceFile {
            path: "flags.yaml".to_string(),
            source: "flags:\n- id: flag.overlay_added\n  name: Overlay added\n  description: Added by the story layer.\n  initial_state: false\n"
                .to_string(),
        },
        SourceFile {
            path: "characters.yaml".to_string(),
            source: "characters:\n- id: character.culprit\n  name: Mara Voss-Renner\n  description: A guest with a carefully guarded secret, now under a married name.\n"
                .to_string(),
        },
    ];

    let merged =
        merge_layers(&deck, &story).expect("deck and story overlay merges without diagnostics");

    let characters: serde_yaml::Value = serde_yaml::from_str(
        &merged
            .iter()
            .find(|f| f.path == "characters.yaml")
            .unwrap()
            .source,
    )
    .unwrap();
    let culprit = characters
        .get("characters")
        .unwrap()
        .as_sequence()
        .unwrap()
        .iter()
        .find(|c| c.get("id").unwrap().as_str() == Some("character.culprit"))
        .unwrap();
    assert_eq!(
        culprit.get("name").unwrap().as_str(),
        Some("Mara Voss-Renner")
    );

    let report = validate(&merged);
    assert!(
        report
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.severity != narrator_validator::Severity::Error),
        "{:#?}",
        report.diagnostics
    );
    assert!(report.valid, "{:#?}", report.diagnostics);
}

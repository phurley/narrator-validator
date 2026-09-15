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

#[test]
fn empty_deck_round_trips_a_real_story_byte_for_byte() {
    let story = read_fixture_dir(Path::new("tests/fixtures/format-3.3-solve-card-sets"));
    let merged = merge_layers(&[], &story).expect("merging with an empty deck layer never fails");

    assert_eq!(merged.len(), story.len());
    for expected in &story {
        let actual = merged
            .iter()
            .find(|file| file.path == expected.path)
            .unwrap_or_else(|| panic!("{} missing from merged output", expected.path));
        assert_eq!(actual.source, expected.source, "{} changed", expected.path);
    }

    let before = validate(&story);
    let after = validate(&merged);
    assert_eq!(before.diagnostics, after.diagnostics);
    assert_eq!(before.valid, after.valid);
}

/// A deck layer holding a complete story, overlaid by a story layer that
/// adds a flag, overrides a character's name, and adds a new setting. The
/// merged effective set must still validate cleanly.
#[test]
fn merging_a_fixture_deck_and_story_overlay_produces_no_validation_errors() {
    let deck = read_fixture_dir(Path::new("tests/fixtures/format-3.3-solve-card-sets"));

    let story = vec![
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

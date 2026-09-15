//! CLI integration coverage for `--deck-dir` (narrator-validator#137):
//! merging a deck directory into a story directory before validating, the
//! same way the backend and Author reach `merge_layers`, but from the
//! binary.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_narrator-validator")
}

fn temp_dir(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "narrator-validator-{label}-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn copy_fixture_story(destination: &Path) {
    let source = Path::new("tests/fixtures/format-3.3-solve-card-sets");
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        assert!(path.is_file(), "fixture story is flat");
        fs::copy(&path, destination.join(path.file_name().unwrap())).unwrap();
    }
}

#[test]
fn deck_dir_with_strip_deck_only_validates_a_real_story_against_an_empty_deck() {
    let story_dir = temp_dir("story");
    copy_fixture_story(&story_dir);
    let empty_deck_dir = temp_dir("empty-deck");

    let plain = Command::new(binary())
        .args(["--format", "json", story_dir.to_str().unwrap()])
        .output()
        .unwrap();

    let with_deck = Command::new(binary())
        .args([
            "--format",
            "json",
            "--deck-dir",
            empty_deck_dir.to_str().unwrap(),
            "--strip-deck-only",
            story_dir.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    fs::remove_dir_all(&story_dir).unwrap();
    fs::remove_dir_all(&empty_deck_dir).unwrap();

    assert_eq!(with_deck.status.code(), Some(0), "{:?}", with_deck);

    let plain_report: narrator_validator::ValidationReport =
        serde_json::from_slice(&plain.stdout).unwrap();
    let deck_report: narrator_validator::ValidationReport =
        serde_json::from_slice(&with_deck.stdout).unwrap();
    assert!(plain_report.valid, "{:#?}", plain_report.diagnostics);
    assert!(deck_report.valid, "{:#?}", deck_report.diagnostics);
    assert_eq!(plain_report.diagnostics, deck_report.diagnostics);
}

#[test]
fn without_strip_deck_only_a_story_carrying_ruleset_reports_deck_only_field_and_fails() {
    let story_dir = temp_dir("story-unstripped");
    copy_fixture_story(&story_dir);
    let empty_deck_dir = temp_dir("empty-deck-unstripped");

    let output = Command::new(binary())
        .args([
            "--format",
            "json",
            "--deck-dir",
            empty_deck_dir.to_str().unwrap(),
            story_dir.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    fs::remove_dir_all(&story_dir).unwrap();
    fs::remove_dir_all(&empty_deck_dir).unwrap();

    assert_ne!(output.status.code(), Some(0));
    let report: narrator_validator::ValidationReport =
        serde_json::from_slice(&output.stdout).unwrap();
    assert!(!report.valid);
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "layer.deck_only_field"
            && diagnostic.pointer.as_deref() == Some("/case/ruleset")));
}

#[test]
fn emit_effective_writes_a_merged_set_without_a_story_tombstoned_character() {
    let story_dir = temp_dir("story-tombstone");
    copy_fixture_story(&story_dir);

    let deck_dir = temp_dir("deck-tombstone");
    fs::write(
        deck_dir.join("characters.yaml"),
        "characters:\n- id: character.deck_only\n  name: Deck Only Character\n  description: Provided only by the deck layer.\n",
    )
    .unwrap();

    let story_characters_path = story_dir.join("characters.yaml");
    let existing = fs::read_to_string(&story_characters_path).unwrap();
    fs::write(
        &story_characters_path,
        format!("{existing}  - id: character.deck_only\n    remove: true\n"),
    )
    .unwrap();

    let effective_dir = temp_dir("effective");
    fs::remove_dir_all(&effective_dir).unwrap();

    let output = Command::new(binary())
        .args([
            "--format",
            "json",
            "--deck-dir",
            deck_dir.to_str().unwrap(),
            "--strip-deck-only",
            "--emit-effective",
            effective_dir.to_str().unwrap(),
            story_dir.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    let merged_characters = fs::read_to_string(effective_dir.join("characters.yaml")).unwrap();

    fs::remove_dir_all(&story_dir).unwrap();
    fs::remove_dir_all(&deck_dir).unwrap();
    fs::remove_dir_all(&effective_dir).unwrap();

    assert_eq!(output.status.code(), Some(0), "{:?}", output);
    assert!(!merged_characters.contains("character.deck_only"));
}

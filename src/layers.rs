//! Overlays a deck-layer file set with a story-layer file set, per
//! [ADR-022 §2](https://github.com/phurley/narrator-system/blob/fb02269/ADR-022-deck-defaults-and-story-overrides.md).
//!
//! This module is a pure function over two file lists using the same
//! [`SourceFile`] representation [`crate::validate`] takes, so the CLI, the
//! backend and the WASM package share one implementation. It does not
//! validate the merged result; call [`crate::validate`] on the output.

use std::collections::{HashSet, VecDeque};

use serde_yaml::{Mapping, Value};

use crate::{Diagnostic, RelatedLocation, Severity, SourceFile};

/// Canonical section files this module knows how to merge by id, key, or
/// top-level field, per ADR-022 §2. Every other file (including `deck.yaml`,
/// which the backend synthesizes separately, and `maps/<name>.svg`) is a
/// whole-file passthrough: the story's copy wins when both layers provide
/// the same path, and either layer's copy is inherited unchanged when the
/// other layer omits it.
struct SectionConfig {
    /// A top-level list section merged by id, plus the nested list and map
    /// fields (if any) that get their own by-id / by-key merge inside a
    /// matched entry rather than being replaced whole with the rest of it.
    lists: &'static [(
        &'static str,
        &'static [&'static str],
        &'static [&'static str],
        bool,
    )],
}

fn section_config(path: &str) -> Option<SectionConfig> {
    match path {
        "settings.yaml" => Some(SectionConfig {
            lists: &[
                ("settings", &[], &["command_overrides"], true),
                ("routes", &[], &[], true),
            ],
        }),
        "characters.yaml" => Some(SectionConfig {
            lists: &[(
                "characters",
                &["facts", "testimony"],
                &["command_overrides"],
                true,
            )],
        }),
        "entities.yaml" => Some(SectionConfig {
            lists: &[("entities", &["facts"], &["command_overrides"], true)],
        }),
        "events.yaml" => Some(SectionConfig {
            lists: &[("events", &[], &["command_overrides"], true)],
        }),
        "flags.yaml" => Some(SectionConfig {
            lists: &[("flags", &[], &[], true)],
        }),
        "commands.yaml" => Some(SectionConfig {
            lists: &[("commands", &[], &[], true)],
        }),
        "triggers.yaml" => Some(SectionConfig {
            lists: &[("triggers", &[], &[], true)],
        }),
        "deductions.yaml" => Some(SectionConfig {
            lists: &[("deductions", &[], &["command_overrides"], true)],
        }),
        // `command_costs` is a map keyed by cost id: the story's id wins,
        // deck ids the story does not mention are inherited, and there is no
        // tombstone (ADR-022 §2: "a story that wants 'no cost' sets the
        // cost").
        "costs.yaml" => Some(SectionConfig {
            lists: &[("command_costs", &[], &[], false)],
        }),
        // `reference-literals.yaml` and `wait.yaml` are maps merged per
        // top-level key with no id-listed section inside them.
        "reference-literals.yaml" | "wait.yaml" => Some(SectionConfig { lists: &[] }),
        // `case.yaml` (`case` and `solution`) needs its own handling for
        // `case.map`'s mixed per-key/by-id rule; see `merge_case_file`.
        "case.yaml" => None,
        _ => None,
    }
}

/// Overlays `deck` beneath `story` and returns the effective file set.
///
/// Returns `Err` with every diagnostic found (never stopping at the first)
/// when the merge itself is invalid, for example a story tombstone whose id
/// does not exist in the deck layer (`layer.remove_unknown_id`) or a
/// canonical section file that is not valid YAML. Otherwise returns `Ok`
/// with the merged files; call [`crate::validate`] on them to check the
/// *content* of the effective set.
pub fn merge_layers(
    deck: &[SourceFile],
    story: &[SourceFile],
) -> Result<Vec<SourceFile>, Vec<Diagnostic>> {
    let mut diagnostics = Vec::new();

    let mut order: Vec<&str> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    for file in deck.iter().chain(story.iter()) {
        if seen.insert(file.path.as_str()) {
            order.push(file.path.as_str());
        }
    }

    let mut output = Vec::with_capacity(order.len());
    for path in order {
        let deck_file = deck.iter().find(|f| f.path == path);
        let story_file = story.iter().find(|f| f.path == path);
        let merged = match (deck_file, story_file) {
            (Some(d), None) => d.clone(),
            (None, Some(s)) => s.clone(),
            (Some(d), Some(s)) => merge_shared_file(d, s, &mut diagnostics),
            (None, None) => unreachable!("path came from one of the two lists"),
        };
        output.push(merged);
    }

    if diagnostics.is_empty() {
        Ok(output)
    } else {
        Err(diagnostics)
    }
}

fn merge_shared_file(
    deck: &SourceFile,
    story: &SourceFile,
    diagnostics: &mut Vec<Diagnostic>,
) -> SourceFile {
    if deck.path != "case.yaml" && section_config(&deck.path).is_none() {
        // Not a section file this module merges: story wins whole.
        return story.clone();
    }

    let deck_value: Value = match serde_yaml::from_str(&deck.source) {
        Ok(value) => value,
        Err(error) => {
            diagnostics.push(invalid_yaml_diagnostic(&deck.path, &error.to_string()));
            return story.clone();
        }
    };
    let story_value: Value = match serde_yaml::from_str(&story.source) {
        Ok(value) => value,
        Err(error) => {
            diagnostics.push(invalid_yaml_diagnostic(&story.path, &error.to_string()));
            return story.clone();
        }
    };

    let merged_value = if deck.path == "case.yaml" {
        merge_case_file(&deck_value, &story_value, &story.path, diagnostics)
    } else {
        let config = section_config(&story.path).expect("checked above");
        merge_sections(
            config.lists,
            &deck_value,
            &story_value,
            &story.path,
            diagnostics,
        )
    };

    let source = serde_yaml::to_string(&merged_value).unwrap_or_else(|_| story.source.clone());
    SourceFile {
        path: story.path.clone(),
        source,
    }
}

fn invalid_yaml_diagnostic(path: &str, message: &str) -> Diagnostic {
    Diagnostic {
        severity: Severity::Error,
        code: "layer.invalid_yaml".to_string(),
        message: format!("could not parse `{path}` while merging layers: {message}"),
        path: path.to_string(),
        pointer: None,
        range: None,
        subject_id: None,
        related: Vec::new(),
    }
}

/// Merges every top-level key present in either mapping. A key present in
/// `story` wins and replaces the deck's value whole; a key only in `deck` is
/// inherited unchanged. Mirrors ADR-022 §2's `case`/`wait` rule and doubles
/// as the whole-file merge for sections with no id-listed content
/// (`reference-literals.yaml`, `wait.yaml`).
fn merge_scalar_map(deck: Option<&Mapping>, story: Option<&Mapping>) -> Mapping {
    let mut merged = Mapping::new();
    if let Some(deck) = deck {
        for (key, value) in deck {
            merged.insert(key.clone(), value.clone());
        }
    }
    if let Some(story) = story {
        for (key, value) in story {
            merged.insert(key.clone(), value.clone());
        }
    }
    merged
}

/// Merges the id-listed sections of a file (e.g. `characters`, `routes`) on
/// top of a scalar merge of any other top-level keys the file might carry.
fn merge_sections(
    lists: &[(&str, &[&str], &[&str], bool)],
    deck_value: &Value,
    story_value: &Value,
    path: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Value {
    let deck_root = deck_value.as_mapping();
    let story_root = story_value.as_mapping();
    let mut merged = merge_scalar_map(deck_root, story_root);

    for (key, nested_lists, nested_maps, supports_tombstone) in lists {
        let deck_present = deck_root.and_then(|m| m.get(*key)).is_some();
        let story_present = story_root.and_then(|m| m.get(*key)).is_some();
        if !deck_present && !story_present {
            continue;
        }
        let deck_items = deck_root
            .and_then(|m| m.get(*key))
            .and_then(Value::as_sequence)
            .cloned()
            .unwrap_or_default();
        let story_items = story_root
            .and_then(|m| m.get(*key))
            .and_then(Value::as_sequence)
            .cloned()
            .unwrap_or_default();
        let merged_items = merge_id_list(
            &deck_items,
            &story_items,
            path,
            key,
            *supports_tombstone,
            nested_lists,
            nested_maps,
            diagnostics,
        );
        merged.insert(
            Value::String((*key).to_string()),
            Value::Sequence(merged_items),
        );
    }

    Value::Mapping(merged)
}

/// `case.yaml` holds `case` and `solution` at the root. `solution` is
/// story-only (ADR-022 §3) and every other root key follows the generic
/// scalar-map rule, but `case` itself merges per inner key
/// (ADR-022 §2), and `case.map` is a further exception: `preamble` is
/// per-key while `variants` merges by id like any other id-listed section.
fn merge_case_file(
    deck_value: &Value,
    story_value: &Value,
    path: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Value {
    let deck_root = deck_value.as_mapping();
    let story_root = story_value.as_mapping();
    let mut merged = merge_scalar_map(deck_root, story_root);

    let deck_case = deck_root
        .and_then(|m| m.get("case"))
        .and_then(Value::as_mapping);
    let story_case = story_root
        .and_then(|m| m.get("case"))
        .and_then(Value::as_mapping);
    if deck_case.is_some() || story_case.is_some() {
        let mut merged_case = merge_scalar_map(deck_case, story_case);

        let deck_map = deck_case
            .and_then(|m| m.get("map"))
            .and_then(Value::as_mapping);
        let story_map = story_case
            .and_then(|m| m.get("map"))
            .and_then(Value::as_mapping);
        if deck_map.is_some() || story_map.is_some() {
            let mut merged_map = merge_scalar_map(deck_map, story_map);

            let deck_variants_present = deck_map.and_then(|m| m.get("variants")).is_some();
            let story_variants_present = story_map.and_then(|m| m.get("variants")).is_some();
            if deck_variants_present || story_variants_present {
                let deck_variants = deck_map
                    .and_then(|m| m.get("variants"))
                    .and_then(Value::as_sequence)
                    .cloned()
                    .unwrap_or_default();
                let story_variants = story_map
                    .and_then(|m| m.get("variants"))
                    .and_then(Value::as_sequence)
                    .cloned()
                    .unwrap_or_default();
                let merged_variants = merge_id_list(
                    &deck_variants,
                    &story_variants,
                    path,
                    "case/map/variants",
                    true,
                    &[],
                    &[],
                    diagnostics,
                );
                merged_map.insert(
                    Value::String("variants".to_string()),
                    Value::Sequence(merged_variants),
                );
            }
            merged_case.insert(Value::String("map".to_string()), Value::Mapping(merged_map));
        }
        merged.insert(
            Value::String("case".to_string()),
            Value::Mapping(merged_case),
        );
    }

    Value::Mapping(merged)
}

/// Merges two lists of id-bearing mappings per ADR-022 §2: a story entry
/// whose id matches a deck entry replaces it whole (subject to
/// `nested_lists`/`nested_maps` below); deck entries the story does not
/// mention are inherited; story entries with new ids are appended in story
/// order after the (deck-ordered) inherited/replaced entries.
///
/// `nested_lists` names fields (e.g. `facts`, `testimony`) that, inside a
/// *matched* entry, get their own by-id merge instead of being replaced
/// whole along with the rest of the entry. `nested_maps` names fields (e.g.
/// `command_overrides`) that get their own by-key merge the same way.
///
/// When `supports_tombstone` is set, a story entry of the exact shape
/// `{ id: X, remove: true }` drops deck entry `X` instead of replacing it.
/// A tombstone whose id is not present in the deck layer emits
/// `layer.remove_unknown_id` and is otherwise ignored, rather than stopping
/// the merge.
#[allow(clippy::too_many_arguments)]
fn merge_id_list(
    deck_items: &[Value],
    story_items: &[Value],
    file_path: &str,
    pointer_prefix: &str,
    supports_tombstone: bool,
    nested_lists: &[&str],
    nested_maps: &[&str],
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<Value> {
    let mut consumed: HashSet<&str> = HashSet::new();
    let mut story_by_id: VecDeque<(&str, &Value)> = VecDeque::new();
    for item in story_items {
        if let Some(id) = item.get("id").and_then(Value::as_str) {
            story_by_id.push_back((id, item));
        }
    }
    let find_story = |id: &str| {
        story_by_id
            .iter()
            .find(|(sid, _)| *sid == id)
            .map(|(_, v)| *v)
    };

    let mut output = Vec::with_capacity(deck_items.len() + story_items.len());

    for item in deck_items {
        let Some(id) = item.get("id").and_then(Value::as_str) else {
            output.push(item.clone());
            continue;
        };
        match find_story(id) {
            None => output.push(item.clone()),
            Some(story_item) => {
                consumed.insert(id);
                if supports_tombstone && is_tombstone(story_item) {
                    // Dropped: the story removed this inherited entry.
                } else {
                    output.push(merge_matched_item(
                        item,
                        story_item,
                        file_path,
                        pointer_prefix,
                        nested_lists,
                        nested_maps,
                        diagnostics,
                    ));
                }
            }
        }
    }

    for item in story_items {
        let Some(id) = item.get("id").and_then(Value::as_str) else {
            output.push(item.clone());
            continue;
        };
        if consumed.contains(id) {
            continue;
        }
        if supports_tombstone && is_tombstone(item) {
            diagnostics.push(Diagnostic {
                severity: Severity::Error,
                code: "layer.remove_unknown_id".to_string(),
                message: format!(
                    "story tombstone removes `{id}`, which is not present in the deck layer"
                ),
                path: file_path.to_string(),
                pointer: Some(format!("{pointer_prefix}/{}", escape_pointer(id))),
                range: None,
                subject_id: Some(id.to_string()),
                related: vec![RelatedLocation {
                    message: "tombstone entry".to_string(),
                    path: file_path.to_string(),
                    pointer: Some(format!("{pointer_prefix}/{}", escape_pointer(id))),
                    range: None,
                }],
            });
            continue;
        }
        output.push(item.clone());
    }

    output
}

#[allow(clippy::too_many_arguments)]
fn merge_matched_item(
    deck_item: &Value,
    story_item: &Value,
    file_path: &str,
    pointer_prefix: &str,
    nested_lists: &[&str],
    nested_maps: &[&str],
    diagnostics: &mut Vec<Diagnostic>,
) -> Value {
    let mut merged = story_item.clone();
    let id = story_item
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    let Some(merged_map) = merged.as_mapping_mut() else {
        return merged;
    };

    for &field in nested_lists {
        let deck_present = deck_item.get(field).is_some();
        let story_present = story_item.get(field).is_some();
        if !deck_present && !story_present {
            continue;
        }
        let deck_list = deck_item
            .get(field)
            .and_then(Value::as_sequence)
            .cloned()
            .unwrap_or_default();
        let story_list = story_item
            .get(field)
            .and_then(Value::as_sequence)
            .cloned()
            .unwrap_or_default();
        let nested_pointer = format!("{pointer_prefix}/{}/{field}", escape_pointer(&id));
        let merged_list = merge_id_list(
            &deck_list,
            &story_list,
            file_path,
            &nested_pointer,
            true,
            &[],
            &[],
            diagnostics,
        );
        merged_map.insert(
            Value::String(field.to_string()),
            Value::Sequence(merged_list),
        );
    }

    for &field in nested_maps {
        let deck_field = deck_item.get(field).and_then(Value::as_mapping);
        let story_field = story_item.get(field).and_then(Value::as_mapping);
        if deck_field.is_none() && story_field.is_none() {
            continue;
        }
        let merged_field = merge_scalar_map(deck_field, story_field);
        merged_map.insert(
            Value::String(field.to_string()),
            Value::Mapping(merged_field),
        );
    }

    merged
}

/// A tombstone is `{ id: <string>, remove: true }` with no other keys.
fn is_tombstone(value: &Value) -> bool {
    let Some(mapping) = value.as_mapping() else {
        return false;
    };
    mapping.len() == 2
        && mapping.get("id").and_then(Value::as_str).is_some()
        && mapping.get("remove").and_then(Value::as_bool) == Some(true)
}

/// RFC 6901 JSON Pointer token escaping, mirroring `validator::escape_pointer`.
fn escape_pointer(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, source: &str) -> SourceFile {
        SourceFile {
            path: path.to_string(),
            source: source.to_string(),
        }
    }

    fn find<'a>(files: &'a [SourceFile], path: &str) -> &'a SourceFile {
        files
            .iter()
            .find(|f| f.path == path)
            .unwrap_or_else(|| panic!("expected {path} in merged output"))
    }

    fn parsed(files: &[SourceFile], path: &str) -> Value {
        serde_yaml::from_str(&find(files, path).source).unwrap()
    }

    #[test]
    fn empty_deck_returns_story_unchanged() {
        let story = vec![
            file("case.yaml", "case:\n  id: case.a\n"),
            file(
                "characters.yaml",
                "characters:\n- id: character.a\n  name: A\n",
            ),
        ];
        let merged = merge_layers(&[], &story).unwrap();
        assert_eq!(merged.len(), story.len());
        for expected in &story {
            assert_eq!(find(&merged, &expected.path).source, expected.source);
        }
    }

    #[test]
    fn empty_story_returns_deck_unchanged() {
        let deck = vec![
            file(
                "characters.yaml",
                "characters:\n- id: character.a\n  name: A\n",
            ),
            file("maps/house.svg", "<svg/>"),
        ];
        let merged = merge_layers(&deck, &[]).unwrap();
        assert_eq!(merged, deck);
    }

    #[test]
    fn id_listed_section_replaces_matched_entry_whole() {
        let deck = vec![file(
            "flags.yaml",
            "flags:\n- id: flag.a\n  name: Deck A\n  initial_state: false\n",
        )];
        let story = vec![file(
            "flags.yaml",
            "flags:\n- id: flag.a\n  name: Story A\n  initial_state: true\n",
        )];
        let merged = merge_layers(&deck, &story).unwrap();
        let value = parsed(&merged, "flags.yaml");
        let flags = value.get("flags").unwrap().as_sequence().unwrap();
        assert_eq!(flags.len(), 1);
        assert_eq!(flags[0].get("name").unwrap().as_str(), Some("Story A"));
        assert_eq!(flags[0].get("initial_state").unwrap().as_bool(), Some(true));
    }

    #[test]
    fn id_listed_section_inherits_unmentioned_deck_entries_and_appends_new_story_entries() {
        let deck = vec![file(
            "flags.yaml",
            "flags:\n- id: flag.a\n  name: A\n- id: flag.b\n  name: B\n",
        )];
        let story = vec![file("flags.yaml", "flags:\n- id: flag.c\n  name: C\n")];
        let merged = merge_layers(&deck, &story).unwrap();
        let value = parsed(&merged, "flags.yaml");
        let ids: Vec<&str> = value
            .get("flags")
            .unwrap()
            .as_sequence()
            .unwrap()
            .iter()
            .map(|item| item.get("id").unwrap().as_str().unwrap())
            .collect();
        assert_eq!(ids, vec!["flag.a", "flag.b", "flag.c"]);
    }

    #[test]
    fn tombstone_drops_matched_deck_entry() {
        let deck = vec![file(
            "flags.yaml",
            "flags:\n- id: flag.a\n  name: A\n- id: flag.b\n  name: B\n",
        )];
        let story = vec![file("flags.yaml", "flags:\n- id: flag.a\n  remove: true\n")];
        let merged = merge_layers(&deck, &story).unwrap();
        let value = parsed(&merged, "flags.yaml");
        let ids: Vec<&str> = value
            .get("flags")
            .unwrap()
            .as_sequence()
            .unwrap()
            .iter()
            .map(|item| item.get("id").unwrap().as_str().unwrap())
            .collect();
        assert_eq!(ids, vec!["flag.b"]);
    }

    #[test]
    fn tombstone_of_unknown_id_emits_diagnostic_without_dropping_other_diagnostics() {
        let deck = vec![
            file("flags.yaml", "flags:\n- id: flag.a\n  name: A\n"),
            file(
                "entities.yaml",
                "entities:\n- id: entity.a\n  type: object\n",
            ),
        ];
        let story = vec![
            file("flags.yaml", "flags:\n- id: flag.missing\n  remove: true\n"),
            file(
                "entities.yaml",
                "entities:\n- id: entity.missing\n  remove: true\n",
            ),
        ];
        let error = merge_layers(&deck, &story).unwrap_err();
        assert_eq!(error.len(), 2);
        assert!(error.iter().all(|d| d.code == "layer.remove_unknown_id"));
        let subjects: HashSet<&str> = error
            .iter()
            .map(|d| d.subject_id.as_deref().unwrap())
            .collect();
        assert_eq!(subjects, HashSet::from(["flag.missing", "entity.missing"]));
        assert_eq!(error[0].path, "flags.yaml");
        assert_eq!(error[1].path, "entities.yaml");
    }

    #[test]
    fn map_keyed_command_costs_merges_per_key_with_no_tombstone() {
        let deck = vec![file(
            "costs.yaml",
            "command_costs:\n- id: cost.a\n  command: command.examine\n  target: entity.a\n  minutes: 1\n",
        )];
        let story = vec![file(
            "costs.yaml",
            "command_costs:\n- id: cost.a\n  command: command.examine\n  target: entity.a\n  minutes: 5\n- id: cost.a\n  remove: true\n",
        )];
        // A `{ id, remove: true }` shape is not special-cased for command_costs:
        // it is treated as two literal entries sharing an id (the second wins
        // when read downstream), never as a deletion, and never produces
        // `layer.remove_unknown_id`.
        let merged = merge_layers(&deck, &story).unwrap();
        let value = parsed(&merged, "costs.yaml");
        let costs = value.get("command_costs").unwrap().as_sequence().unwrap();
        assert_eq!(costs.len(), 1);
        assert_eq!(costs[0].get("minutes").unwrap().as_i64(), Some(5));
    }

    #[test]
    fn map_keyed_command_overrides_merges_per_key_on_a_matched_subject() {
        let deck = vec![file(
            "entities.yaml",
            "entities:\n- id: entity.a\n  type: object\n  command_overrides:\n    command.examine: true\n    command.take: false\n",
        )];
        let story = vec![file(
            "entities.yaml",
            "entities:\n- id: entity.a\n  type: object\n  command_overrides:\n    command.take: true\n",
        )];
        let merged = merge_layers(&deck, &story).unwrap();
        let value = parsed(&merged, "entities.yaml");
        let entity = &value.get("entities").unwrap().as_sequence().unwrap()[0];
        let overrides = entity.get("command_overrides").unwrap();
        assert_eq!(
            overrides.get("command.examine").unwrap().as_bool(),
            Some(true)
        );
        assert_eq!(overrides.get("command.take").unwrap().as_bool(), Some(true));
    }

    #[test]
    fn scalar_case_section_merges_per_top_level_key() {
        let deck = vec![file(
            "case.yaml",
            "case:\n  id: case.deck_default\n  format_version: '3.11.0'\n  title: Deck Title\n  genre: mystery\n",
        )];
        let story = vec![file(
            "case.yaml",
            "case:\n  title: Story Title\nsolution:\n  max_attempts: 3\n",
        )];
        let merged = merge_layers(&deck, &story).unwrap();
        let value = parsed(&merged, "case.yaml");
        let case = value.get("case").unwrap();
        assert_eq!(case.get("title").unwrap().as_str(), Some("Story Title"));
        assert_eq!(case.get("genre").unwrap().as_str(), Some("mystery"));
        assert_eq!(case.get("format_version").unwrap().as_str(), Some("3.11.0"));
        assert_eq!(
            value
                .get("solution")
                .unwrap()
                .get("max_attempts")
                .unwrap()
                .as_i64(),
            Some(3)
        );
    }

    #[test]
    fn case_map_preamble_is_per_key_and_variants_merge_by_id() {
        let deck = vec![file(
            "case.yaml",
            "case:\n  id: case.a\n  map:\n    preamble: Deck preamble\n    variants:\n    - id: map.default\n      source: maps/deck.svg\n    - id: map.alt\n      source: maps/deck-alt.svg\n",
        )];
        let story = vec![file(
            "case.yaml",
            "case:\n  map:\n    variants:\n    - id: map.alt\n      source: maps/story-alt.svg\n    - id: map.new\n      source: maps/story-new.svg\n",
        )];
        let merged = merge_layers(&deck, &story).unwrap();
        let value = parsed(&merged, "case.yaml");
        let map = value.get("case").unwrap().get("map").unwrap();
        assert_eq!(map.get("preamble").unwrap().as_str(), Some("Deck preamble"));
        let variants = map.get("variants").unwrap().as_sequence().unwrap();
        let ids: Vec<&str> = variants
            .iter()
            .map(|v| v.get("id").unwrap().as_str().unwrap())
            .collect();
        assert_eq!(ids, vec!["map.default", "map.alt", "map.new"]);
        let alt = variants
            .iter()
            .find(|v| v.get("id").unwrap().as_str() == Some("map.alt"))
            .unwrap();
        assert_eq!(
            alt.get("source").unwrap().as_str(),
            Some("maps/story-alt.svg")
        );
    }

    #[test]
    fn map_svg_story_replaces_same_named_deck_file_and_inherits_others() {
        let deck = vec![
            file("maps/house.svg", "<svg id=\"deck\"/>"),
            file("maps/annex.svg", "<svg id=\"annex\"/>"),
        ];
        let story = vec![file("maps/house.svg", "<svg id=\"story\"/>")];
        let merged = merge_layers(&deck, &story).unwrap();
        assert_eq!(
            find(&merged, "maps/house.svg").source,
            "<svg id=\"story\"/>"
        );
        assert_eq!(
            find(&merged, "maps/annex.svg").source,
            "<svg id=\"annex\"/>"
        );
    }

    #[test]
    fn deck_yaml_is_never_merged_and_story_wins_unchanged() {
        let deck = vec![file("deck.yaml", "cards: []\n")];
        let story = vec![file("deck.yaml", "cards:\n- id: card.a\n")];
        let merged = merge_layers(&deck, &story).unwrap();
        assert_eq!(find(&merged, "deck.yaml").source, "cards:\n- id: card.a\n");
    }

    #[test]
    fn nested_facts_merge_by_id_inside_a_matched_character() {
        let deck = vec![file(
            "characters.yaml",
            "characters:\n- id: character.a\n  name: Deck Name\n  facts:\n  - id: fact.a\n    statement: Deck fact A\n  - id: fact.b\n    statement: Deck fact B\n",
        )];
        let story = vec![file(
            "characters.yaml",
            "characters:\n- id: character.a\n  name: Story Name\n  facts:\n  - id: fact.a\n    statement: Story fact A\n  - id: fact.c\n    statement: Story fact C\n",
        )];
        let merged = merge_layers(&deck, &story).unwrap();
        let value = parsed(&merged, "characters.yaml");
        let character = &value.get("characters").unwrap().as_sequence().unwrap()[0];
        assert_eq!(character.get("name").unwrap().as_str(), Some("Story Name"));
        let facts = character.get("facts").unwrap().as_sequence().unwrap();
        let ids: Vec<&str> = facts
            .iter()
            .map(|f| f.get("id").unwrap().as_str().unwrap())
            .collect();
        assert_eq!(ids, vec!["fact.a", "fact.b", "fact.c"]);
        let fact_a = &facts[0];
        assert_eq!(
            fact_a.get("statement").unwrap().as_str(),
            Some("Story fact A")
        );
    }

    #[test]
    fn combined_deck_and_story_overlay() {
        let deck = vec![
            file(
                "settings.yaml",
                "settings:\n- id: setting.parlor\n  type: room\n  name: Parlor\n- id: setting.study\n  type: room\n  name: Study\n",
            ),
            file(
                "characters.yaml",
                "characters:\n- id: character.a\n  name: Deck A\n  facts:\n  - id: fact.a\n    statement: Deck fact\n- id: character.b\n  name: Deck B\n",
            ),
            file(
                "case.yaml",
                "case:\n  id: case.deck\n  map:\n    variants:\n    - id: map.default\n      source: maps/deck.svg\n    - id: map.alt\n      source: maps/deck-alt.svg\n",
            ),
        ];
        let story = vec![
            file(
                "settings.yaml",
                "settings:\n- id: setting.parlor\n  remove: true\n",
            ),
            file(
                "characters.yaml",
                "characters:\n- id: character.a\n  name: Story A\n  facts:\n  - id: fact.a\n    statement: Story fact\n",
            ),
            file(
                "entities.yaml",
                "entities:\n- id: entity.new\n  type: object\n",
            ),
            file(
                "case.yaml",
                "case:\n  id: case.story\n  map:\n    variants:\n    - id: map.alt\n      source: maps/story-alt.svg\n",
            ),
            file(
                "costs.yaml",
                "command_costs:\n- id: cost.new\n  command: command.examine\n  target: entity.new\n  minutes: 2\n",
            ),
        ];

        let merged = merge_layers(&deck, &story).unwrap();

        let settings = parsed(&merged, "settings.yaml");
        let setting_ids: Vec<&str> = settings
            .get("settings")
            .unwrap()
            .as_sequence()
            .unwrap()
            .iter()
            .map(|s| s.get("id").unwrap().as_str().unwrap())
            .collect();
        assert_eq!(setting_ids, vec!["setting.study"]);

        let characters = parsed(&merged, "characters.yaml");
        let character_ids: Vec<&str> = characters
            .get("characters")
            .unwrap()
            .as_sequence()
            .unwrap()
            .iter()
            .map(|c| c.get("id").unwrap().as_str().unwrap())
            .collect();
        assert_eq!(character_ids, vec!["character.a", "character.b"]);

        let entities = parsed(&merged, "entities.yaml");
        assert_eq!(
            entities
                .get("entities")
                .unwrap()
                .as_sequence()
                .unwrap()
                .len(),
            1
        );

        let case = parsed(&merged, "case.yaml");
        assert_eq!(
            case.get("case").unwrap().get("id").unwrap().as_str(),
            Some("case.story")
        );
        let variants = case
            .get("case")
            .unwrap()
            .get("map")
            .unwrap()
            .get("variants")
            .unwrap()
            .as_sequence()
            .unwrap();
        let variant_ids: Vec<&str> = variants
            .iter()
            .map(|v| v.get("id").unwrap().as_str().unwrap())
            .collect();
        assert_eq!(variant_ids, vec!["map.default", "map.alt"]);

        let costs = parsed(&merged, "costs.yaml");
        assert_eq!(
            costs
                .get("command_costs")
                .unwrap()
                .as_sequence()
                .unwrap()
                .len(),
            1
        );
    }
}

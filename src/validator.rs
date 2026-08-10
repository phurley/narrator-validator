use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

use serde::Deserialize;
use serde_yaml::{Mapping, Value};

use crate::{
    Diagnostic, Position, RelatedLocation, Severity, SourceFile, SourceRange, ValidationReport,
    VALIDATOR_VERSION,
};

const MAX_REPOSITORY_FILES: usize = 512;
const MAX_REPOSITORY_BYTES: usize = 1024 * 1024;
const MAX_FILE_BYTES: usize = 256 * 1024;
const MAX_YAML_DEPTH: usize = 64;
const MAX_YAML_NODES: usize = 100_000;

const REQUIRED_SECTIONS: &[&str] = &[
    "case",
    "solution",
    "settings",
    "routes",
    "characters",
    "entities",
    "events",
    "deductions",
    "flags",
];
const SINGLE_SECTIONS: &[&str] = &["clues", "commands", "triggers"];
const CANONICAL_SECTION_FILES: &[(&str, &str)] = &[
    ("case", "settings.yaml"),
    ("solution", "settings.yaml"),
    ("settings", "settings.yaml"),
    ("routes", "settings.yaml"),
    ("characters", "characters.yaml"),
    ("entities", "entities.yaml"),
    ("events", "events.yaml"),
    ("clues", "clues.yaml"),
    ("deductions", "deductions.yaml"),
    ("flags", "flags.yaml"),
    ("commands", "commands.yaml"),
    ("triggers", "triggers.yaml"),
];

#[derive(Debug)]
struct ParsedFile<'a> {
    path: &'a str,
    source: &'a str,
    value: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Case,
    Setting,
    Route,
    Character,
    Entity,
    Event,
    Clue,
    Fact,
    Deduction,
    Flag,
    Command,
    Trigger,
    Testimony,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandParameterType {
    Character,
    Entity,
    Setting,
    Deduction,
    Event,
}

impl CommandParameterType {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "character" => Some(Self::Character),
            "entity" => Some(Self::Entity),
            "setting" => Some(Self::Setting),
            "deduction" => Some(Self::Deduction),
            "event" => Some(Self::Event),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Character => "character",
            Self::Entity => "entity",
            Self::Setting => "setting",
            Self::Deduction => "deduction",
            Self::Event => "event",
        }
    }

    fn kind(self) -> Kind {
        match self {
            Self::Character => Kind::Character,
            Self::Entity => Kind::Entity,
            Self::Setting => Kind::Setting,
            Self::Deduction => Kind::Deduction,
            Self::Event => Kind::Event,
        }
    }
}

impl Kind {
    fn prefix(self) -> &'static str {
        match self {
            Self::Case => "case",
            Self::Setting => "setting",
            Self::Route => "route",
            Self::Character => "character",
            Self::Entity => "entity",
            Self::Event => "event",
            Self::Clue => "clue",
            Self::Fact => "fact",
            Self::Deduction => "deduction",
            Self::Flag => "flag",
            Self::Command => "command",
            Self::Trigger => "trigger",
            Self::Testimony => "testimony",
        }
    }

    fn name(self) -> &'static str {
        self.prefix()
    }
}

#[derive(Debug, Clone)]
struct Definition {
    kind: Kind,
    path: String,
    pointer: String,
    range: Option<SourceRange>,
}

#[derive(Debug, Clone)]
struct Item {
    id: String,
    path: String,
    source: String,
    pointer: String,
    mapping: Mapping,
}

#[derive(Debug, Clone)]
struct Route {
    id: String,
    path: String,
    pointer: String,
    from: Option<String>,
    to: Option<String>,
    bidirectional: bool,
}

struct Validator<'a> {
    files: &'a [SourceFile],
    parsed: Vec<ParsedFile<'a>>,
    diagnostics: Vec<Diagnostic>,
    definitions: BTreeMap<String, Definition>,
    sections: BTreeMap<String, Vec<(String, String)>>,
    format_version: Option<u64>,
}

/// Validate a complete, immutable repository snapshot.
pub fn validate(files: &[SourceFile]) -> ValidationReport {
    let mut validator = Validator {
        files,
        parsed: Vec::new(),
        diagnostics: Vec::new(),
        definitions: BTreeMap::new(),
        sections: BTreeMap::new(),
        format_version: None,
    };
    validator.run();
    validator.diagnostics.sort_by(|left, right| {
        (
            &left.path,
            left.range.map(|range| range.start.line).unwrap_or(0),
            left.range.map(|range| range.start.column).unwrap_or(0),
            &left.code,
            &left.message,
        )
            .cmp(&(
                &right.path,
                right.range.map(|range| range.start.line).unwrap_or(0),
                right.range.map(|range| range.start.column).unwrap_or(0),
                &right.code,
                &right.message,
            ))
    });
    let valid = validator
        .diagnostics
        .iter()
        .all(|diagnostic| diagnostic.severity != Severity::Error);
    ValidationReport {
        validator_version: VALIDATOR_VERSION.to_string(),
        format_version: validator.format_version,
        valid,
        diagnostics: validator.diagnostics,
    }
}

impl<'a> Validator<'a> {
    fn run(&mut self) {
        if !self.validate_repository_bounds() {
            return;
        }
        self.parse_files();
        self.index_sections();
        self.validate_section_filenames();

        let cases = self.items("case", Kind::Case, false);
        self.validate_case(&cases);
        self.validate_sections();
        let settings = self.items("settings", Kind::Setting, true);
        let routes = self.items("routes", Kind::Route, true);
        let characters = self.items("characters", Kind::Character, true);
        let entities = self.items("entities", Kind::Entity, true);
        let events = self.items("events", Kind::Event, true);
        let clues = self.items("clues", Kind::Clue, true);
        let deductions = self.items("deductions", Kind::Deduction, true);
        let flags = self.items("flags", Kind::Flag, true);
        let commands = self.items("commands", Kind::Command, true);
        let triggers = self.items("triggers", Kind::Trigger, true);
        let fact_claims_enabled = self.format_version == Some(2);
        let facts = if fact_claims_enabled {
            self.nested_facts(&[
                settings.as_slice(),
                characters.as_slice(),
                entities.as_slice(),
                events.as_slice(),
                triggers.as_slice(),
            ])
        } else {
            Vec::new()
        };
        self.nested_testimonies(&characters);
        let facts_enabled = !facts.is_empty();

        self.validate_solution();
        self.validate_references();
        self.validate_duplicate_lists();
        self.validate_event_values(&events);
        self.validate_route_values(&routes);
        self.validate_character_values(&characters, facts_enabled);
        self.validate_entity_values(&entities);
        if !fact_claims_enabled {
            self.validate_clue_values(&clues, facts_enabled);
        }
        self.validate_fact_values(&facts, fact_claims_enabled);
        if fact_claims_enabled {
            self.validate_disallowed_fact_owners(&routes);
            self.validate_disallowed_fact_owners(&commands);
        } else {
            for items in [
                settings.as_slice(),
                routes.as_slice(),
                characters.as_slice(),
                entities.as_slice(),
                events.as_slice(),
                commands.as_slice(),
                triggers.as_slice(),
            ] {
                self.validate_fact_associations(items);
            }
        }
        self.validate_deduction_values(&deductions, fact_claims_enabled);
        self.validate_command_values(&commands);
        self.validate_testimony_question_signature(&characters, &commands);
        self.validate_trigger_values(&triggers, &commands, fact_claims_enabled);
        if !fact_claims_enabled {
            self.validate_fact_reachability(
                &facts,
                &[
                    settings.as_slice(),
                    routes.as_slice(),
                    characters.as_slice(),
                    entities.as_slice(),
                    events.as_slice(),
                    commands.as_slice(),
                    triggers.as_slice(),
                ],
                &clues,
            );
        }
        self.validate_graphs(
            &settings,
            &entities,
            &clues,
            &facts,
            &deductions,
            fact_claims_enabled,
        );
        self.validate_navigation(&cases, &settings, &routes);
        self.validate_flag_values(&flags);
    }

    fn validate_repository_bounds(&mut self) -> bool {
        let mut within_bounds = true;
        if self.files.len() > MAX_REPOSITORY_FILES {
            within_bounds = false;
            self.push(
                Severity::Error,
                "repository.too_many_files",
                format!(
                    "repository has {} files; the limit is {MAX_REPOSITORY_FILES}",
                    self.files.len()
                ),
                "",
                None,
                None,
                None,
            );
        }
        let total = self
            .files
            .iter()
            .fold(0usize, |sum, file| sum.saturating_add(file.source.len()));
        if total > MAX_REPOSITORY_BYTES {
            within_bounds = false;
            self.push(
                Severity::Error,
                "repository.too_large",
                format!("repository is {total} bytes; the limit is {MAX_REPOSITORY_BYTES}"),
                "",
                None,
                None,
                None,
            );
        }
        for file in self.files {
            if file.source.len() > MAX_FILE_BYTES {
                within_bounds = false;
                self.push(
                    Severity::Error,
                    "repository.file_too_large",
                    format!(
                        "file is {} bytes; the limit is {MAX_FILE_BYTES}",
                        file.source.len()
                    ),
                    &file.path,
                    None,
                    None,
                    None,
                );
            }
        }
        within_bounds
    }

    fn parse_files(&mut self) {
        let mut seen_paths = HashSet::new();
        for file in self.files {
            if !seen_paths.insert(file.path.as_str()) {
                self.push(
                    Severity::Error,
                    "repository.duplicate_path",
                    format!("path `{}` occurs more than once", file.path),
                    &file.path,
                    None,
                    None,
                    None,
                );
                continue;
            }
            if contains_yaml_anchor_or_alias(&file.source) {
                self.push(
                    Severity::Error,
                    "yaml.alias_unsupported",
                    "YAML anchors and aliases are not supported".to_string(),
                    &file.path,
                    None,
                    None,
                    None,
                );
                continue;
            }

            let mut documents = serde_yaml::Deserializer::from_str(&file.source);
            let Some(document) = documents.next() else {
                self.push(
                    Severity::Error,
                    "yaml.document_count",
                    "expected exactly one YAML document, found 0".to_string(),
                    &file.path,
                    None,
                    None,
                    None,
                );
                continue;
            };
            let value = match Value::deserialize(document) {
                Ok(value) => value,
                Err(error) => {
                    let range = error.location().map(|location| {
                        point_range(Position {
                            line: location.line(),
                            column: location.column(),
                        })
                    });
                    self.push(
                        Severity::Error,
                        "yaml.invalid",
                        error.to_string(),
                        &file.path,
                        None,
                        range,
                        None,
                    );
                    continue;
                }
            };
            if documents.next().is_some() {
                self.push(
                    Severity::Error,
                    "yaml.document_count",
                    "expected exactly one YAML document, found more than one".to_string(),
                    &file.path,
                    None,
                    None,
                    None,
                );
                continue;
            }
            let mut nodes = 0;
            if let Err(message) = check_yaml_complexity(&value, 0, &mut nodes) {
                self.push(
                    Severity::Error,
                    "yaml.too_complex",
                    message,
                    &file.path,
                    None,
                    None,
                    None,
                );
                continue;
            }
            if !value.is_mapping() {
                self.push(
                    Severity::Error,
                    "schema.root_type",
                    "YAML document root must be a mapping".to_string(),
                    &file.path,
                    Some("".to_string()),
                    None,
                    None,
                );
                continue;
            }
            self.parsed.push(ParsedFile {
                path: &file.path,
                source: &file.source,
                value,
            });
        }
    }

    fn index_sections(&mut self) {
        for file in &self.parsed {
            let Some(root) = file.value.as_mapping() else {
                continue;
            };
            for key in root.keys().filter_map(Value::as_str) {
                self.sections
                    .entry(key.to_string())
                    .or_default()
                    .push((file.path.to_string(), format!("/{}", escape_pointer(key))));
            }
        }
    }

    fn validate_sections(&mut self) {
        for section in REQUIRED_SECTIONS {
            match self.sections.get(*section) {
                None => self.push(
                    Severity::Error,
                    "schema.missing_section",
                    format!("required top-level section `{section}` is missing"),
                    "",
                    Some(format!("/{}", escape_pointer(section))),
                    None,
                    None,
                ),
                Some(locations) if locations.len() > 1 => {
                    let locations = locations.clone();
                    for (path, pointer) in locations {
                        self.push(
                            Severity::Error,
                            "schema.duplicate_section",
                            format!("top-level section `{section}` is defined more than once"),
                            &path,
                            Some(pointer),
                            None,
                            None,
                        );
                    }
                }
                Some(_) => {}
            }
        }
        let knowledge_section = "clues";
        if self.format_version != Some(2) {
            match self.sections.get(knowledge_section) {
                None => self.push(
                    Severity::Error,
                    "schema.missing_section",
                    format!("required top-level section `{knowledge_section}` is missing"),
                    "",
                    Some(format!("/{}", escape_pointer(knowledge_section))),
                    None,
                    None,
                ),
                Some(locations) if locations.len() > 1 => {
                    let locations = locations.clone();
                    for (path, pointer) in locations {
                        self.push(
                            Severity::Error,
                            "schema.duplicate_section",
                            format!(
                                "top-level section `{knowledge_section}` is defined more than once"
                            ),
                            &path,
                            Some(pointer),
                            None,
                            None,
                        );
                    }
                }
                Some(_) => {}
            }
        }
        for section in SINGLE_SECTIONS {
            if *section == knowledge_section {
                continue;
            }
            if let Some(locations) = self.sections.get(*section).filter(|items| items.len() > 1) {
                let locations = locations.clone();
                for (path, pointer) in locations {
                    self.push(
                        Severity::Error,
                        "schema.duplicate_section",
                        format!("top-level section `{section}` is defined more than once"),
                        &path,
                        Some(pointer),
                        None,
                        None,
                    );
                }
            }
        }
        if self.format_version == Some(2) {
            if let Some(locations) = self.sections.get("clues").cloned() {
                for (path, pointer) in locations {
                    self.push(
                        Severity::Error,
                        "format.clues_removed",
                        "format 2 removes clues; express player knowledge in `facts`".to_string(),
                        &path,
                        Some(pointer),
                        None,
                        None,
                    );
                }
            }
        }
        if let Some(locations) = self.sections.get("facts").cloned() {
            for (path, pointer) in locations {
                self.push(
                    Severity::Error,
                    "format.facts_section_removed",
                    "top-level `facts` was removed; nest fact objects beneath characters, entities, settings, events, or triggers".to_string(),
                    &path,
                    Some(pointer),
                    None,
                    None,
                );
            }
        }
        if let Some(locations) = self.sections.get("tags").cloned() {
            for (path, pointer) in locations {
                self.push(
                    Severity::Error,
                    "format.tags_removed",
                    "top-level `tags` was removed; use authored boolean flags for world state"
                        .to_string(),
                    &path,
                    Some(pointer),
                    None,
                    None,
                );
            }
        }
    }

    fn validate_section_filenames(&mut self) {
        for (section, expected_path) in CANONICAL_SECTION_FILES {
            let Some(locations) = self.sections.get(*section).cloned() else {
                continue;
            };
            for (path, pointer) in locations {
                if path == *expected_path {
                    continue;
                }
                self.push(
                    Severity::Error,
                    "schema.noncanonical_filename",
                    format!(
                        "top-level section `{section}` must be defined in `{expected_path}`, not `{path}`"
                    ),
                    &path,
                    Some(pointer),
                    None,
                    None,
                );
            }
        }
    }

    fn items(&mut self, section: &str, kind: Kind, sequence: bool) -> Vec<Item> {
        let mut result = Vec::new();
        let parsed_len = self.parsed.len();
        for file_index in 0..parsed_len {
            let (path, source, section_value) = {
                let file = &self.parsed[file_index];
                let root = file.value.as_mapping().expect("root mappings were checked");
                (
                    file.path.to_string(),
                    file.source.to_string(),
                    root.get(Value::String(section.to_string())).cloned(),
                )
            };
            let Some(section_value) = section_value else {
                continue;
            };
            let values: Vec<(Mapping, String)> = if sequence {
                let Some(values) = section_value.as_sequence() else {
                    self.push(
                        Severity::Error,
                        "schema.section_type",
                        format!("`{section}` must be a sequence"),
                        &path,
                        Some(format!("/{}", escape_pointer(section))),
                        None,
                        None,
                    );
                    continue;
                };
                values
                    .iter()
                    .enumerate()
                    .filter_map(|(index, value)| {
                        if let Some(mapping) = value.as_mapping() {
                            Some((mapping.clone(), format!("/{section}/{index}")))
                        } else {
                            self.push(
                                Severity::Error,
                                "schema.item_type",
                                format!("items in `{section}` must be mappings"),
                                &path,
                                Some(format!("/{section}/{index}")),
                                None,
                                None,
                            );
                            None
                        }
                    })
                    .collect()
            } else {
                let Some(mapping) = section_value.as_mapping() else {
                    self.push(
                        Severity::Error,
                        "schema.section_type",
                        format!("`{section}` must be a mapping"),
                        &path,
                        Some(format!("/{}", escape_pointer(section))),
                        None,
                        None,
                    );
                    continue;
                };
                vec![(mapping.clone(), format!("/{section}"))]
            };

            for (mapping, pointer) in values {
                let id_pointer = format!("{pointer}/id");
                let Some(id) = string_field(&mapping, "id").map(str::to_string) else {
                    self.push(
                        Severity::Error,
                        "id.missing",
                        format!("{} is missing a string `id`", kind.name()),
                        &path,
                        Some(id_pointer),
                        None,
                        None,
                    );
                    continue;
                };
                let range = locate_id(&source, &id);
                if !valid_id(&id) {
                    self.push(
                        Severity::Error,
                        "id.invalid",
                        format!(
                            "`{id}` must contain a lowercase kind prefix, a dot, and a lowercase snake-case name"
                        ),
                        &path,
                        Some(id_pointer.clone()),
                        range,
                        Some(id.clone()),
                    );
                } else if id_prefix(&id) != Some(kind.prefix()) {
                    self.push(
                        Severity::Error,
                        "id.wrong_prefix",
                        format!(
                            "{} ID `{id}` must start with `{}.`",
                            kind.name(),
                            kind.prefix()
                        ),
                        &path,
                        Some(id_pointer.clone()),
                        range,
                        Some(id.clone()),
                    );
                }
                let definition = Definition {
                    kind,
                    path: path.clone(),
                    pointer: pointer.clone(),
                    range,
                };
                if let Some(first) = self.definitions.get(&id).cloned() {
                    self.diagnostics.push(Diagnostic {
                        severity: Severity::Error,
                        code: "id.duplicate".to_string(),
                        message: format!("ID `{id}` is defined more than once"),
                        path: path.clone(),
                        pointer: Some(id_pointer),
                        range,
                        subject_id: Some(id.clone()),
                        related: vec![RelatedLocation {
                            message: "first defined here".to_string(),
                            path: first.path,
                            pointer: Some(format!("{}/id", first.pointer)),
                            range: first.range,
                        }],
                    });
                } else {
                    self.definitions.insert(id.clone(), definition);
                }
                result.push(Item {
                    id,
                    path: path.clone(),
                    source: source.clone(),
                    pointer,
                    mapping,
                });
            }
        }
        result
    }

    fn nested_facts(&mut self, owners: &[&[Item]]) -> Vec<Item> {
        let mut result = Vec::new();
        for items in owners {
            for owner in *items {
                let Some(value) = owner.mapping.get(Value::String("facts".to_string())) else {
                    continue;
                };
                let collection_pointer = format!("{}/facts", owner.pointer);
                let Some(values) = value.as_sequence() else {
                    self.push(
                        Severity::Error,
                        "fact.collection_type",
                        "owner `facts` must be a sequence of fact mappings".to_string(),
                        &owner.path,
                        Some(collection_pointer),
                        None,
                        Some(owner.id.clone()),
                    );
                    continue;
                };
                for (index, value) in values.iter().enumerate() {
                    let pointer = format!("{}/facts/{index}", owner.pointer);
                    let Some(mapping) = value.as_mapping().cloned() else {
                        self.push(
                            Severity::Error,
                            "fact.item_type",
                            "items in owner `facts` must be mappings".to_string(),
                            &owner.path,
                            Some(pointer),
                            None,
                            Some(owner.id.clone()),
                        );
                        continue;
                    };
                    let id_pointer = format!("{pointer}/id");
                    let Some(id) = string_field(&mapping, "id").map(str::to_string) else {
                        self.push(
                            Severity::Error,
                            "id.missing",
                            "fact is missing a string `id`".to_string(),
                            &owner.path,
                            Some(id_pointer),
                            None,
                            None,
                        );
                        continue;
                    };
                    let range = locate_id(&owner.source, &id);
                    if !valid_id(&id) {
                        self.push(
                            Severity::Error,
                            "id.invalid",
                            format!(
                                "`{id}` must contain a lowercase kind prefix, a dot, and a lowercase snake-case name"
                            ),
                            &owner.path,
                            Some(id_pointer.clone()),
                            range,
                            Some(id.clone()),
                        );
                    } else if id_prefix(&id) != Some(Kind::Fact.prefix()) {
                        self.push(
                            Severity::Error,
                            "id.wrong_prefix",
                            format!("fact ID `{id}` must start with `fact.`"),
                            &owner.path,
                            Some(id_pointer.clone()),
                            range,
                            Some(id.clone()),
                        );
                    }
                    let definition = Definition {
                        kind: Kind::Fact,
                        path: owner.path.clone(),
                        pointer: pointer.clone(),
                        range,
                    };
                    if let Some(first) = self.definitions.get(&id).cloned() {
                        self.diagnostics.push(Diagnostic {
                            severity: Severity::Error,
                            code: "id.duplicate".to_string(),
                            message: format!("ID `{id}` is defined more than once"),
                            path: owner.path.clone(),
                            pointer: Some(id_pointer),
                            range,
                            subject_id: Some(id.clone()),
                            related: vec![RelatedLocation {
                                message: "first defined here".to_string(),
                                path: first.path,
                                pointer: Some(format!("{}/id", first.pointer)),
                                range: first.range,
                            }],
                        });
                    } else {
                        self.definitions.insert(id.clone(), definition);
                    }
                    result.push(Item {
                        id,
                        path: owner.path.clone(),
                        source: owner.source.clone(),
                        pointer,
                        mapping,
                    });
                }
            }
        }
        result
    }

    fn nested_testimonies(&mut self, characters: &[Item]) {
        for character in characters {
            let Some(value) = character
                .mapping
                .get(Value::String("testimony".to_string()))
            else {
                continue;
            };
            let collection_pointer = format!("{}/testimony", character.pointer);
            let Some(values) = value.as_sequence() else {
                self.push(
                    Severity::Error,
                    "character.testimony_type",
                    "character `testimony` must be a sequence of player-safe testimony mappings"
                        .to_string(),
                    &character.path,
                    Some(collection_pointer),
                    None,
                    Some(character.id.clone()),
                );
                continue;
            };
            for (index, value) in values.iter().enumerate() {
                let pointer = format!("{}/testimony/{index}", character.pointer);
                let Some(mapping) = value.as_mapping().cloned() else {
                    self.push(
                        Severity::Error,
                        "character.testimony_entry_type",
                        "character testimony entries must be mappings".to_string(),
                        &character.path,
                        Some(pointer),
                        None,
                        Some(character.id.clone()),
                    );
                    continue;
                };
                let id_pointer = format!("{pointer}/id");
                let Some(id) = string_field(&mapping, "id").map(str::to_string) else {
                    self.push(
                        Severity::Error,
                        "id.missing",
                        "character testimony is missing a string `id`".to_string(),
                        &character.path,
                        Some(id_pointer),
                        None,
                        Some(character.id.clone()),
                    );
                    continue;
                };
                let range = locate_id(&character.source, &id);
                if !valid_id(&id) {
                    self.push(
                        Severity::Error,
                        "id.invalid",
                        format!(
                            "`{id}` must contain a lowercase kind prefix, a dot, and a lowercase snake-case name"
                        ),
                        &character.path,
                        Some(id_pointer.clone()),
                        range,
                        Some(id.clone()),
                    );
                } else if id_prefix(&id) != Some(Kind::Testimony.prefix()) {
                    self.push(
                        Severity::Error,
                        "id.wrong_prefix",
                        format!("testimony ID `{id}` must start with `testimony.`"),
                        &character.path,
                        Some(id_pointer.clone()),
                        range,
                        Some(id.clone()),
                    );
                }
                let definition = Definition {
                    kind: Kind::Testimony,
                    path: character.path.clone(),
                    pointer: pointer.clone(),
                    range,
                };
                if let Some(first) = self.definitions.get(&id).cloned() {
                    self.diagnostics.push(Diagnostic {
                        severity: Severity::Error,
                        code: "id.duplicate".to_string(),
                        message: format!("ID `{id}` is defined more than once"),
                        path: character.path.clone(),
                        pointer: Some(id_pointer),
                        range,
                        subject_id: Some(id.clone()),
                        related: vec![RelatedLocation {
                            message: "first defined here".to_string(),
                            path: first.path,
                            pointer: Some(format!("{}/id", first.pointer)),
                            range: first.range,
                        }],
                    });
                } else {
                    self.definitions.insert(id, definition);
                }
            }
        }
    }

    fn validate_case(&mut self, cases: &[Item]) {
        if cases.len() != 1 {
            return;
        }
        let case = &cases[0];
        match case
            .mapping
            .get(Value::String("format_version".to_string()))
        {
            Some(Value::Number(number)) => {
                if let Some(version) = number.as_u64() {
                    self.format_version = Some(version);
                    if !matches!(version, 1 | 2) {
                        self.push(
                            Severity::Error,
                            "format.unsupported_version",
                            format!("format version {version} is not supported; expected 1 or 2"),
                            &case.path,
                            Some(format!("{}/format_version", case.pointer)),
                            locate_scalar(&case.source, &version.to_string()),
                            Some(case.id.clone()),
                        );
                    }
                } else {
                    self.push(
                        Severity::Error,
                        "format.version_type",
                        "`format_version` must be a positive integer".to_string(),
                        &case.path,
                        Some(format!("{}/format_version", case.pointer)),
                        None,
                        Some(case.id.clone()),
                    );
                }
            }
            Some(_) => self.push(
                Severity::Error,
                "format.version_type",
                "`format_version` must be a positive integer".to_string(),
                &case.path,
                Some(format!("{}/format_version", case.pointer)),
                None,
                Some(case.id.clone()),
            ),
            None => self.push(
                Severity::Warning,
                "format.version_missing",
                "add `case.format_version: 1` to make format evolution explicit".to_string(),
                &case.path,
                Some(format!("{}/format_version", case.pointer)),
                None,
                Some(case.id.clone()),
            ),
        }
        for field in ["entry_settings", "exit_settings"] {
            if let Some(value) = case.mapping.get(Value::String(field.to_string())) {
                if !is_string_sequence(value) {
                    self.push(
                        Severity::Error,
                        "navigation.contract_type",
                        format!("`{field}` must be a sequence of setting IDs"),
                        &case.path,
                        Some(format!("{}/{}", case.pointer, escape_pointer(field))),
                        None,
                        Some(case.id.clone()),
                    );
                }
            }
        }
        if case
            .mapping
            .get(Value::String("genre".to_string()))
            .is_some_and(|value| !value.as_str().is_some_and(|genre| !genre.trim().is_empty()))
        {
            self.push(
                Severity::Error,
                "case.genre",
                "`case.genre` must be a non-empty string".to_string(),
                &case.path,
                Some(format!("{}/genre", case.pointer)),
                None,
                Some(case.id.clone()),
            );
        }
        if let Some(tone) = case.mapping.get(Value::String("tone".to_string())) {
            if let Some(entries) = tone.as_sequence() {
                let mut seen = HashSet::new();
                for (index, entry) in entries.iter().enumerate() {
                    let pointer = format!("{}/tone/{index}", case.pointer);
                    let Some(value) = entry.as_str().filter(|value| !value.trim().is_empty())
                    else {
                        self.push(
                            Severity::Error,
                            "case.tone_entry",
                            "`case.tone` entries must be non-empty strings".to_string(),
                            &case.path,
                            Some(pointer),
                            None,
                            Some(case.id.clone()),
                        );
                        continue;
                    };
                    if !seen.insert(value.trim()) {
                        self.push(
                            Severity::Error,
                            "case.tone_duplicate",
                            format!("`case.tone` entry `{}` occurs more than once", value.trim()),
                            &case.path,
                            Some(pointer),
                            None,
                            Some(case.id.clone()),
                        );
                    }
                }
            } else {
                self.push(
                    Severity::Error,
                    "case.tone_type",
                    "`case.tone` must be a sequence of unique non-empty strings".to_string(),
                    &case.path,
                    Some(format!("{}/tone", case.pointer)),
                    None,
                    Some(case.id.clone()),
                );
            }
        }
        if self.format_version == Some(2) {
            match case
                .mapping
                .get(Value::String("initial_time".to_string()))
            {
                Some(Value::String(time)) if valid_time(time) => {}
                Some(_) => self.push(
                    Severity::Error,
                    "case.initial_time",
                    "`case.initial_time` must be a quoted 24-hour HH:MM value".to_string(),
                    &case.path,
                    Some(format!("{}/initial_time", case.pointer)),
                    None,
                    Some(case.id.clone()),
                ),
                None => self.push(
                    Severity::Error,
                    "case.initial_time_missing",
                    "format 2 games require `case.initial_time` so runtime time effects are deterministic"
                        .to_string(),
                    &case.path,
                    Some(format!("{}/initial_time", case.pointer)),
                    None,
                    Some(case.id.clone()),
                ),
            }
        }
    }

    fn validate_solution(&mut self) {
        let solutions: Vec<_> = self
            .parsed
            .iter()
            .filter_map(|file| {
                let solution = file
                    .value
                    .as_mapping()?
                    .get(Value::String("solution".to_string()))?
                    .as_mapping()?
                    .clone();
                Some((file.path.to_string(), file.source.to_string(), solution))
            })
            .collect();
        for (path, source, solution) in solutions {
            for field in ["victim", "culprit", "weapon", "location"] {
                if string_field(&solution, field).is_none() {
                    self.push(
                        Severity::Error,
                        "solution.missing_reference",
                        format!("solution `{field}` must be an ID"),
                        &path,
                        Some(format!("/solution/{}", escape_pointer(field))),
                        None,
                        None,
                    );
                }
            }
            match solution.get(Value::String("time".to_string())) {
                Some(Value::String(time)) if valid_time(time) => {}
                Some(_) => self.push(
                    Severity::Error,
                    "solution.invalid_time",
                    "solution `time` must be a quoted 24-hour HH:MM value".to_string(),
                    &path,
                    Some("/solution/time".to_string()),
                    string_field(&solution, "time").and_then(|time| locate_scalar(&source, time)),
                    None,
                ),
                None => {}
            }
        }
    }

    fn validate_references(&mut self) {
        let parsed_len = self.parsed.len();
        for file_index in 0..parsed_len {
            let (path, source, value) = {
                let file = &self.parsed[file_index];
                (file.path, file.source, file.value.clone())
            };
            let mut references = Vec::new();
            collect_references(&value, "", None, &mut references);
            for reference in references {
                let expected = expected_kind(&reference.pointer);
                match self.definitions.get(&reference.id) {
                    None => self.push(
                        Severity::Error,
                        "reference.unknown",
                        format!("`{}` does not refer to a defined ID", reference.id),
                        path,
                        Some(reference.pointer),
                        locate_scalar(source, &reference.id),
                        Some(reference.id),
                    ),
                    Some(definition)
                        if expected.is_some_and(|kinds| !kinds.contains(&definition.kind)) =>
                    {
                        let expected = expected
                            .unwrap()
                            .iter()
                            .map(|kind| kind.name())
                            .collect::<Vec<_>>()
                            .join(" or ");
                        self.push(
                            Severity::Error,
                            "reference.wrong_type",
                            format!(
                                "`{}` refers to a {}; expected {expected}",
                                reference.id,
                                definition.kind.name()
                            ),
                            path,
                            Some(reference.pointer),
                            locate_scalar(source, &reference.id),
                            Some(reference.id),
                        );
                    }
                    Some(_) => {}
                }
            }
        }
    }

    fn validate_duplicate_lists(&mut self) {
        let parsed_len = self.parsed.len();
        for file_index in 0..parsed_len {
            let (path, source, value) = {
                let file = &self.parsed[file_index];
                (file.path, file.source, file.value.clone())
            };
            let mut duplicates = Vec::new();
            collect_duplicate_id_lists(&value, "", &mut duplicates);
            for (pointer, id) in duplicates {
                self.push(
                    Severity::Error,
                    "list.duplicate_reference",
                    format!("`{id}` occurs more than once in this list"),
                    path,
                    Some(pointer),
                    locate_scalar(source, &id),
                    Some(id),
                );
            }
        }
    }

    fn validate_event_values(&mut self, events: &[Item]) {
        for event in events {
            if string_field(&event.mapping, "location").is_none() {
                self.push(
                    Severity::Error,
                    "event.missing_location",
                    "event `location` must be a setting ID".to_string(),
                    &event.path,
                    Some(format!("{}/location", event.pointer)),
                    None,
                    Some(event.id.clone()),
                );
            }
            if let Some(participants) = event.mapping.get(Value::String("participants".to_string()))
            {
                if !is_string_sequence(participants) {
                    self.push(
                        Severity::Error,
                        "event.participants_type",
                        "`participants` must be a sequence of character IDs".to_string(),
                        &event.path,
                        Some(format!("{}/participants", event.pointer)),
                        None,
                        Some(event.id.clone()),
                    );
                }
            }
            match event.mapping.get(Value::String("time".to_string())) {
                Some(Value::String(time)) if valid_time(time) => {}
                Some(_) => self.push(
                    Severity::Error,
                    "event.invalid_time",
                    "event `time` must be a quoted 24-hour HH:MM value".to_string(),
                    &event.path,
                    Some(format!("{}/time", event.pointer)),
                    None,
                    Some(event.id.clone()),
                ),
                None => self.push(
                    Severity::Error,
                    "event.missing_time",
                    "event is missing `time`".to_string(),
                    &event.path,
                    Some(format!("{}/time", event.pointer)),
                    None,
                    Some(event.id.clone()),
                ),
            }
            self.require_nonnegative_integer(event, "day", "event.invalid_day");
            self.require_nonnegative_integer(event, "duration_minutes", "event.invalid_duration");
        }
    }

    fn validate_route_values(&mut self, routes: &[Item]) {
        for route in routes {
            for field in ["from", "to"] {
                if string_field(&route.mapping, field).is_none() {
                    self.push(
                        Severity::Error,
                        "route.missing_endpoint",
                        format!("route `{field}` must be a setting ID"),
                        &route.path,
                        Some(format!("{}/{}", route.pointer, field)),
                        None,
                        Some(route.id.clone()),
                    );
                }
            }
            if let Some(requires) = route.mapping.get(Value::String("requires".to_string())) {
                if !is_string_sequence(requires) {
                    self.push(
                        Severity::Error,
                        "route.requires_type",
                        "`requires` must be a sequence of entity IDs".to_string(),
                        &route.path,
                        Some(format!("{}/requires", route.pointer)),
                        None,
                        Some(route.id.clone()),
                    );
                }
            }
            match integer_field(&route.mapping, "travel_minutes") {
                Some(value) if value > 0 && u32::try_from(value).is_ok() => {}
                _ => self.push(
                    Severity::Error,
                    "route.invalid_travel_minutes",
                    "`travel_minutes` must be a positive whole number supported by the runtime"
                        .to_string(),
                    &route.path,
                    Some(format!("{}/travel_minutes", route.pointer)),
                    None,
                    Some(route.id.clone()),
                ),
            }
            let from = string_field(&route.mapping, "from");
            let to = string_field(&route.mapping, "to");
            if from.is_some() && from == to {
                self.push(
                    Severity::Error,
                    "route.self_loop",
                    "route endpoints must be different settings".to_string(),
                    &route.path,
                    Some(route.pointer.clone()),
                    from.and_then(|id| locate_scalar(&route.source, id)),
                    Some(route.id.clone()),
                );
            }
        }
    }

    fn validate_character_values(&mut self, characters: &[Item], facts_enabled: bool) {
        for character in characters {
            self.validate_character_portrayal(character);
            self.validate_character_testimony(character);

            if let Some(knowledge) = character
                .mapping
                .get(Value::String("knowledge".to_string()))
            {
                let Some(knowledge) = knowledge.as_sequence() else {
                    self.push(
                        Severity::Error,
                        "character.knowledge_type",
                        "character `knowledge` must be a sequence".to_string(),
                        &character.path,
                        Some(format!("{}/knowledge", character.pointer)),
                        None,
                        Some(character.id.clone()),
                    );
                    continue;
                };
                for (index, entry) in knowledge.iter().enumerate() {
                    let pointer = format!("{}/knowledge/{index}", character.pointer);
                    let Some(entry) = entry.as_mapping() else {
                        self.push(
                            Severity::Error,
                            "character.knowledge_entry_type",
                            "character knowledge entries must be mappings".to_string(),
                            &character.path,
                            Some(pointer),
                            None,
                            Some(character.id.clone()),
                        );
                        continue;
                    };
                    let Some(fact) =
                        string_field(entry, "fact").filter(|value| !value.trim().is_empty())
                    else {
                        self.push(
                            Severity::Error,
                            "character.knowledge_fact",
                            "character knowledge `fact` must be a non-empty string".to_string(),
                            &character.path,
                            Some(format!("{pointer}/fact")),
                            None,
                            Some(character.id.clone()),
                        );
                        continue;
                    };
                    if facts_enabled {
                        if !looks_like_id(fact) {
                            self.push(
                                Severity::Error,
                                "character.knowledge_fact_reference",
                                "character knowledge `fact` must be a fact ID when facts are authored"
                                    .to_string(),
                                &character.path,
                                Some(format!("{pointer}/fact")),
                                locate_scalar(&character.source, fact),
                                Some(character.id.clone()),
                            );
                        } else if self
                            .definitions
                            .get(fact)
                            .is_some_and(|definition| definition.kind != Kind::Fact)
                        {
                            self.push(
                                Severity::Error,
                                "reference.wrong_type",
                                format!("`{fact}` does not refer to a fact"),
                                &character.path,
                                Some(format!("{pointer}/fact")),
                                locate_scalar(&character.source, fact),
                                Some(fact.to_string()),
                            );
                        }
                    }
                    if entry
                        .get(Value::String("source".to_string()))
                        .is_some_and(|source| {
                            !source
                                .as_str()
                                .is_some_and(|source| !source.trim().is_empty())
                        })
                    {
                        self.push(
                            Severity::Error,
                            "character.knowledge_source_type",
                            "character knowledge `source` must be a non-empty ID when present"
                                .to_string(),
                            &character.path,
                            Some(format!("{pointer}/source")),
                            None,
                            Some(character.id.clone()),
                        );
                    }
                }
            }
        }
    }

    fn validate_entity_values(&mut self, entities: &[Item]) {
        for entity in entities {
            for field in ["portable", "searchable", "investigatable", "takeable"] {
                if entity
                    .mapping
                    .contains_key(Value::String(field.to_string()))
                {
                    self.push(
                        Severity::Error,
                        "entity.capability_field",
                        format!(
                            "entity `{field}` is not supported; portability belongs at `physical.portable`, while actions are determined by commands and state"
                        ),
                        &entity.path,
                        Some(format!("{}/{}", entity.pointer, escape_pointer(field))),
                        None,
                        Some(entity.id.clone()),
                    );
                }
            }

            if let Some(initial) = entity.mapping.get(Value::String("initial".to_string())) {
                let pointer = format!("{}/initial", entity.pointer);
                let Some(initial) = initial.as_mapping() else {
                    self.push(
                        Severity::Error,
                        "entity.initial_type",
                        "entity `initial` must be a mapping".to_string(),
                        &entity.path,
                        Some(pointer),
                        None,
                        Some(entity.id.clone()),
                    );
                    continue;
                };
                if initial
                    .get(Value::String("container".to_string()))
                    .is_some_and(|container| {
                        !container
                            .as_str()
                            .is_some_and(|container| !container.trim().is_empty())
                    })
                {
                    self.push(
                        Severity::Error,
                        "entity.container_type",
                        "entity `initial.container` must be a non-empty setting, character, or entity ID"
                            .to_string(),
                        &entity.path,
                        Some(format!("{pointer}/container")),
                        None,
                        Some(entity.id.clone()),
                    );
                }
            }

            if let Some(physical) = entity.mapping.get(Value::String("physical".to_string())) {
                let pointer = format!("{}/physical", entity.pointer);
                let Some(physical) = physical.as_mapping() else {
                    self.push(
                        Severity::Error,
                        "entity.physical_type",
                        "entity `physical` must be a mapping".to_string(),
                        &entity.path,
                        Some(pointer),
                        None,
                        Some(entity.id.clone()),
                    );
                    continue;
                };
                for key in physical.keys() {
                    match key.as_str() {
                        Some("portable") => {}
                        Some(field) => self.push(
                            Severity::Error,
                            "entity.physical_unknown_field",
                            format!("`{field}` is not a supported entity physical field"),
                            &entity.path,
                            Some(format!("{pointer}/{}", escape_pointer(field))),
                            None,
                            Some(entity.id.clone()),
                        ),
                        None => self.push(
                            Severity::Error,
                            "entity.physical_field",
                            "entity physical field names must be strings".to_string(),
                            &entity.path,
                            Some(pointer.clone()),
                            None,
                            Some(entity.id.clone()),
                        ),
                    }
                }
                if physical
                    .get(Value::String("portable".to_string()))
                    .is_some_and(|portable| portable.as_bool().is_none())
                {
                    self.push(
                        Severity::Error,
                        "entity.portable_type",
                        "entity `physical.portable` must be a boolean".to_string(),
                        &entity.path,
                        Some(format!("{pointer}/portable")),
                        None,
                        Some(entity.id.clone()),
                    );
                }
            }

            let Some(visibility) = entity.mapping.get(Value::String("visibility".to_string()))
            else {
                continue;
            };
            let pointer = format!("{}/visibility", entity.pointer);
            let Some(visibility) = visibility.as_mapping() else {
                self.push(
                    Severity::Error,
                    "entity.visibility_type",
                    "entity `visibility` must be a mapping".to_string(),
                    &entity.path,
                    Some(pointer),
                    None,
                    Some(entity.id.clone()),
                );
                continue;
            };
            for key in visibility.keys() {
                match key.as_str() {
                    Some("requires") => {}
                    Some(field) => self.push(
                        Severity::Error,
                        "entity.visibility_unknown_field",
                        format!("`{field}` is not a supported entity visibility field"),
                        &entity.path,
                        Some(format!("{pointer}/{}", escape_pointer(field))),
                        None,
                        Some(entity.id.clone()),
                    ),
                    None => self.push(
                        Severity::Error,
                        "entity.visibility_field",
                        "entity visibility field names must be strings".to_string(),
                        &entity.path,
                        Some(pointer.clone()),
                        None,
                        Some(entity.id.clone()),
                    ),
                }
            }
            let Some(requires) = visibility.get(Value::String("requires".to_string())) else {
                continue;
            };
            let requires_pointer = format!("{pointer}/requires");
            match requires {
                Value::String(id) if !id.trim().is_empty() => {
                    // Reference existence and persistence are checked centrally.
                }
                Value::Sequence(values) if !values.is_empty() => {
                    values.iter().enumerate().for_each(|(index, value)| {
                        let item_pointer = format!("{requires_pointer}/{index}");
                        if value
                            .as_str()
                            .filter(|requirement| !requirement.trim().is_empty())
                            .is_none()
                        {
                            self.push(
                                Severity::Error,
                                "entity.visibility_requirement_type",
                                "entity visibility requirements must be non-empty IDs".to_string(),
                                &entity.path,
                                Some(item_pointer),
                                None,
                                Some(entity.id.clone()),
                            );
                        }
                    })
                }
                _ => {
                    self.push(
                        Severity::Error,
                        "entity.visibility_requires_type",
                        "entity `visibility.requires` must be one ID or a non-empty list of unique IDs"
                            .to_string(),
                        &entity.path,
                        Some(requires_pointer),
                        None,
                        Some(entity.id.clone()),
                    );
                }
            }
        }
    }

    fn validate_character_portrayal(&mut self, character: &Item) {
        let Some(portrayal) = character
            .mapping
            .get(Value::String("portrayal".to_string()))
        else {
            return;
        };
        let pointer = format!("{}/portrayal", character.pointer);
        let Some(portrayal) = portrayal.as_mapping() else {
            self.push(
                Severity::Error,
                "character.portrayal_type",
                "character `portrayal` must be a mapping".to_string(),
                &character.path,
                Some(pointer),
                None,
                Some(character.id.clone()),
            );
            return;
        };
        if portrayal.is_empty() {
            self.push(
                Severity::Error,
                "character.portrayal_empty",
                "character `portrayal` must contain `demeanor` or `speech_style`".to_string(),
                &character.path,
                Some(pointer.clone()),
                None,
                Some(character.id.clone()),
            );
        }
        let mut has_supported_field = false;
        for (key, value) in portrayal {
            let Some(key) = key.as_str() else {
                self.push(
                    Severity::Error,
                    "character.portrayal_field",
                    "character portrayal field names must be strings".to_string(),
                    &character.path,
                    Some(pointer.clone()),
                    None,
                    Some(character.id.clone()),
                );
                continue;
            };
            let field_pointer = format!("{pointer}/{}", escape_pointer(key));
            if !matches!(key, "demeanor" | "speech_style") {
                self.push(
                    Severity::Error,
                    "character.portrayal_unknown_field",
                    format!("`{key}` is not a supported player-safe portrayal field"),
                    &character.path,
                    Some(field_pointer),
                    None,
                    Some(character.id.clone()),
                );
                continue;
            }
            has_supported_field = true;
            if !value.as_str().is_some_and(|text| !text.trim().is_empty()) {
                self.push(
                    Severity::Error,
                    "character.portrayal_value",
                    format!("character portrayal `{key}` must be a non-empty string"),
                    &character.path,
                    Some(field_pointer),
                    None,
                    Some(character.id.clone()),
                );
            }
        }
        if !portrayal.is_empty() && !has_supported_field {
            self.push(
                Severity::Error,
                "character.portrayal_empty",
                "character `portrayal` must contain `demeanor` or `speech_style`".to_string(),
                &character.path,
                Some(pointer),
                None,
                Some(character.id.clone()),
            );
        }
    }

    fn validate_character_testimony(&mut self, character: &Item) {
        let Some(entries) = character
            .mapping
            .get(Value::String("testimony".to_string()))
            .and_then(Value::as_sequence)
        else {
            return;
        };
        for (index, entry) in entries.iter().enumerate() {
            let pointer = format!("{}/testimony/{index}", character.pointer);
            let Some(entry) = entry.as_mapping() else {
                continue;
            };
            let subject = string_field(entry, "id")
                .filter(|id| !id.trim().is_empty())
                .unwrap_or(&character.id)
                .to_string();
            for key in entry.keys() {
                let Some(key) = key.as_str() else {
                    self.push(
                        Severity::Error,
                        "character.testimony_field",
                        "character testimony field names must be strings".to_string(),
                        &character.path,
                        Some(pointer.clone()),
                        None,
                        Some(subject.clone()),
                    );
                    continue;
                };
                if !matches!(key, "id" | "text" | "requires" | "reveals") {
                    self.push(
                        Severity::Error,
                        "character.testimony_unknown_field",
                        format!("`{key}` is not a supported player-safe testimony field"),
                        &character.path,
                        Some(format!("{pointer}/{}", escape_pointer(key))),
                        None,
                        Some(subject.clone()),
                    );
                }
            }
            if !string_field(entry, "text").is_some_and(|text| !text.trim().is_empty()) {
                self.push(
                    Severity::Error,
                    "character.testimony_text",
                    "character testimony `text` must be a non-empty string".to_string(),
                    &character.path,
                    Some(format!("{pointer}/text")),
                    None,
                    Some(subject.clone()),
                );
            }
            let requires_pointer = format!("{pointer}/requires");
            let valid_requires = entry
                .get(Value::String("requires".to_string()))
                .is_some_and(|requires| is_nonempty_string_sequence(requires, false));
            if !valid_requires {
                self.push(
                    Severity::Error,
                    "character.testimony_requires_type",
                    "character testimony `requires` must be a non-empty sequence of non-empty IDs"
                        .to_string(),
                    &character.path,
                    Some(requires_pointer.clone()),
                    None,
                    Some(subject.clone()),
                );
            } else {
                let requirements = string_list_field(entry, "requires");
                if !requirements.iter().any(|id| id == "command.question") {
                    self.push(
                        Severity::Error,
                        "character.testimony_question_requirement",
                        "character testimony `requires` must include `command.question`"
                            .to_string(),
                        &character.path,
                        Some(requires_pointer.clone()),
                        None,
                        Some(subject.clone()),
                    );
                }
                if !requirements.iter().any(|id| id == &character.id) {
                    self.push(
                        Severity::Error,
                        "character.testimony_character_requirement",
                        format!(
                            "character testimony `requires` must include its owner `{}`",
                            character.id
                        ),
                        &character.path,
                        Some(requires_pointer.clone()),
                        None,
                        Some(subject.clone()),
                    );
                }
                for (requirement_index, requirement) in requirements.iter().enumerate() {
                    if id_prefix(requirement) == Some(Kind::Command.prefix())
                        && requirement != "command.question"
                    {
                        self.push(
                            Severity::Error,
                            "character.testimony_command_requirement",
                            format!(
                                "character testimony cannot require `{requirement}`; `command.question` is the only compatible command gate"
                            ),
                            &character.path,
                            Some(format!("{requires_pointer}/{requirement_index}")),
                            locate_scalar(&character.source, requirement),
                            Some(subject.clone()),
                        );
                    }
                }
            }
            if entry
                .get(Value::String("reveals".to_string()))
                .is_some_and(|reveals| !is_nonempty_string_sequence(reveals, true))
            {
                self.push(
                    Severity::Error,
                    "character.testimony_reveals_type",
                    "character testimony `reveals` must be a sequence of non-empty fact IDs"
                        .to_string(),
                    &character.path,
                    Some(format!("{pointer}/reveals")),
                    None,
                    Some(subject),
                );
            }
        }
    }

    fn validate_clue_values(&mut self, clues: &[Item], facts_enabled: bool) {
        for clue in clues {
            let Some(establishes) = clue.mapping.get(Value::String("establishes".to_string()))
            else {
                continue;
            };
            if !is_nonempty_string_sequence(establishes, true) {
                self.push(
                    Severity::Error,
                    "clue.establishes_type",
                    "clue `establishes` must be a sequence of non-empty strings".to_string(),
                    &clue.path,
                    Some(format!("{}/establishes", clue.pointer)),
                    None,
                    Some(clue.id.clone()),
                );
                continue;
            }
            if !facts_enabled {
                continue;
            }
            for (index, fact) in establishes
                .as_sequence()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .enumerate()
            {
                let pointer = format!("{}/establishes/{index}", clue.pointer);
                if !looks_like_id(fact) {
                    self.push(
                        Severity::Error,
                        "clue.establishes_reference",
                        "clue `establishes` entries must be fact IDs when facts are authored"
                            .to_string(),
                        &clue.path,
                        Some(pointer),
                        locate_scalar(&clue.source, fact),
                        Some(clue.id.clone()),
                    );
                } else if self
                    .definitions
                    .get(fact)
                    .is_some_and(|definition| definition.kind != Kind::Fact)
                {
                    self.push(
                        Severity::Error,
                        "reference.wrong_type",
                        format!("`{fact}` does not refer to a fact"),
                        &clue.path,
                        Some(pointer),
                        locate_scalar(&clue.source, fact),
                        Some(fact.to_string()),
                    );
                }
            }
        }
    }

    fn validate_fact_associations(&mut self, items: &[Item]) {
        for item in items {
            let Some(facts) = item.mapping.get(Value::String("facts".to_string())) else {
                continue;
            };
            if !valid_fact_association(facts) {
                self.push(
                    Severity::Error,
                    "fact.association_type",
                    "`facts` must be a non-empty sequence of fact IDs or a mapping of observation levels to non-empty fact-ID sequences".to_string(),
                    &item.path,
                    Some(format!("{}/facts", item.pointer)),
                    None,
                    Some(item.id.clone()),
                );
            }
        }
    }

    fn validate_disallowed_fact_owners(&mut self, items: &[Item]) {
        for item in items {
            if item
                .mapping
                .contains_key(Value::String("facts".to_string()))
            {
                self.push(
                    Severity::Error,
                    "fact.owner_type",
                    "facts may be nested only beneath characters, entities, settings, events, or triggers"
                        .to_string(),
                    &item.path,
                    Some(format!("{}/facts", item.pointer)),
                    None,
                    Some(item.id.clone()),
                );
            }
        }
    }

    fn validate_fact_values(&mut self, facts: &[Item], fact_claims_enabled: bool) {
        for fact in facts {
            if !string_field(&fact.mapping, "statement")
                .is_some_and(|statement| !statement.trim().is_empty())
            {
                self.push(
                    Severity::Error,
                    "fact.missing_statement",
                    "fact `statement` must be a non-empty string".to_string(),
                    &fact.path,
                    Some(format!("{}/statement", fact.pointer)),
                    None,
                    Some(fact.id.clone()),
                );
            }
            if fact
                .mapping
                .get(Value::String("narrative_detail".to_string()))
                .is_some_and(|detail| {
                    !detail
                        .as_str()
                        .is_some_and(|detail| !detail.trim().is_empty())
                })
            {
                self.push(
                    Severity::Error,
                    "fact.narrative_detail",
                    "fact `narrative_detail` must be a non-empty string".to_string(),
                    &fact.path,
                    Some(format!("{}/narrative_detail", fact.pointer)),
                    None,
                    Some(fact.id.clone()),
                );
            }
            if let Some(initially_known) = fact
                .mapping
                .get(Value::String("initially_known".to_string()))
            {
                if fact_claims_enabled {
                    self.push(
                        Severity::Error,
                        "fact.initially_known_removed",
                        "format 2 facts enter the notebook automatically; omit `requires` to add a fact when the player joins".to_string(),
                        &fact.path,
                        Some(format!("{}/initially_known", fact.pointer)),
                        None,
                        Some(fact.id.clone()),
                    );
                } else if initially_known.as_bool().is_none() {
                    self.push(
                        Severity::Error,
                        "fact.initially_known_type",
                        "fact `initially_known` must be a boolean".to_string(),
                        &fact.path,
                        Some(format!("{}/initially_known", fact.pointer)),
                        None,
                        Some(fact.id.clone()),
                    );
                }
            }
            if fact_claims_enabled {
                self.validate_fact_occurred_at(fact);
                if let Some(requires) = fact.mapping.get(Value::String("requires".to_string())) {
                    if !is_nonempty_string_or_sequence(requires) {
                        self.push(
                            Severity::Error,
                            "fact.requires_type",
                            "fact `requires` must be one ID or a non-empty sequence of IDs"
                                .to_string(),
                            &fact.path,
                            Some(format!("{}/requires", fact.pointer)),
                            None,
                            Some(fact.id.clone()),
                        );
                    }
                }
            }
            for field in ["about", "sources"] {
                if fact
                    .mapping
                    .get(Value::String(field.to_string()))
                    .is_some_and(|value| !is_nonempty_string_sequence(value, false))
                {
                    self.push(
                        Severity::Error,
                        "fact.reference_list_type",
                        format!("fact `{field}` must contain at least one ID"),
                        &fact.path,
                        Some(format!("{}/{}", fact.pointer, field)),
                        None,
                        Some(fact.id.clone()),
                    );
                }
            }
        }
    }

    fn validate_fact_occurred_at(&mut self, fact: &Item) {
        let Some(value) = fact.mapping.get(Value::String("occurred_at".to_string())) else {
            return;
        };
        let pointer = format!("{}/occurred_at", fact.pointer);
        let Some(occurred_at) = value.as_mapping() else {
            self.push(
                Severity::Error,
                "fact.occurred_at_type",
                "fact `occurred_at` must be a mapping with exactly `day` and `time`".to_string(),
                &fact.path,
                Some(pointer),
                None,
                Some(fact.id.clone()),
            );
            return;
        };

        let mut unknown_fields = occurred_at
            .keys()
            .filter_map(Value::as_str)
            .filter(|field| !matches!(*field, "day" | "time"))
            .collect::<Vec<_>>();
        unknown_fields.sort_unstable();
        for field in unknown_fields {
            self.push(
                Severity::Error,
                "fact.occurred_at_unknown_field",
                format!(
                    "fact `occurred_at` does not support `{field}`; expected only `day` and `time`"
                ),
                &fact.path,
                Some(format!("{pointer}/{}", escape_pointer(field))),
                locate_scalar(&fact.source, field),
                Some(fact.id.clone()),
            );
        }
        if occurred_at.keys().any(|key| key.as_str().is_none()) {
            self.push(
                Severity::Error,
                "fact.occurred_at_unknown_field",
                "fact `occurred_at` keys must be strings named only `day` and `time`".to_string(),
                &fact.path,
                Some(pointer.clone()),
                None,
                Some(fact.id.clone()),
            );
        }

        match occurred_at.get(Value::String("day".to_string())) {
            Some(Value::Number(day))
                if day
                    .as_i64()
                    .is_some_and(|day| day >= 0 && i32::try_from(day).is_ok()) =>
            {
            }
            Some(_) => self.push(
                Severity::Error,
                "fact.occurred_at_day",
                "fact `occurred_at.day` must be a non-negative whole number supported by the runtime"
                    .to_string(),
                &fact.path,
                Some(format!("{pointer}/day")),
                None,
                Some(fact.id.clone()),
            ),
            None => self.push(
                Severity::Error,
                "fact.occurred_at_day_missing",
                "fact `occurred_at` is missing required `day`".to_string(),
                &fact.path,
                Some(format!("{pointer}/day")),
                None,
                Some(fact.id.clone()),
            ),
        }

        match occurred_at.get(Value::String("time".to_string())) {
            Some(Value::String(time)) if valid_time(time) => {}
            Some(_) => self.push(
                Severity::Error,
                "fact.occurred_at_time",
                "fact `occurred_at.time` must be an exact quoted 24-hour HH:MM value".to_string(),
                &fact.path,
                Some(format!("{pointer}/time")),
                None,
                Some(fact.id.clone()),
            ),
            None => self.push(
                Severity::Error,
                "fact.occurred_at_time_missing",
                "fact `occurred_at` is missing required `time`".to_string(),
                &fact.path,
                Some(format!("{pointer}/time")),
                None,
                Some(fact.id.clone()),
            ),
        }
    }

    fn validate_fact_reachability(
        &mut self,
        facts: &[Item],
        fact_owners: &[&[Item]],
        clues: &[Item],
    ) {
        let mut reachable = BTreeSet::new();
        reachable.extend(
            facts
                .iter()
                .filter(|fact| bool_field(&fact.mapping, "initially_known") == Some(true))
                .map(|fact| fact.id.clone()),
        );
        for items in fact_owners {
            for item in *items {
                if let Some(value) = item.mapping.get(Value::String("facts".to_string())) {
                    collect_fact_association_ids(value, &mut reachable);
                }
            }
        }
        for clue in clues {
            reachable.extend(string_list_field(&clue.mapping, "establishes"));
        }
        for fact in facts.iter().filter(|fact| !reachable.contains(&fact.id)) {
            self.push(
                Severity::Warning,
                "fact.unreachable",
                format!(
                    "fact `{}` is neither initially known nor associated with a discoverable element or clue",
                    fact.id
                ),
                &fact.path,
                Some(fact.pointer.clone()),
                locate_scalar(&fact.source, &fact.id),
                Some(fact.id.clone()),
            );
        }
    }

    fn validate_deduction_values(&mut self, deductions: &[Item], fact_claims_enabled: bool) {
        for deduction in deductions {
            if fact_claims_enabled
                && deduction
                    .mapping
                    .contains_key(Value::String("supported_by".to_string()))
            {
                self.push(
                    Severity::Error,
                    "deduction.supported_by_removed",
                    "format 2 deductions use `inputs`; clue-based `supported_by` was removed"
                        .to_string(),
                    &deduction.path,
                    Some(format!("{}/supported_by", deduction.pointer)),
                    None,
                    Some(deduction.id.clone()),
                );
            }
            let expanded = ["inputs", "truth", "contradicted_by", "solves"]
                .iter()
                .any(|field| {
                    deduction
                        .mapping
                        .contains_key(Value::String((*field).to_string()))
                });
            if expanded
                && !string_field(&deduction.mapping, "conclusion")
                    .is_some_and(|conclusion| !conclusion.trim().is_empty())
            {
                self.push(
                    Severity::Error,
                    "deduction.missing_conclusion",
                    "an authored gameplay deduction needs a non-empty `conclusion`".to_string(),
                    &deduction.path,
                    Some(format!("{}/conclusion", deduction.pointer)),
                    None,
                    Some(deduction.id.clone()),
                );
            }
            if let Some(inputs) = deduction.mapping.get(Value::String("inputs".to_string())) {
                let valid = inputs.as_sequence().is_some_and(|values| {
                    (1..=3).contains(&values.len()) && values.iter().all(Value::is_string)
                });
                if !valid {
                    self.push(
                        Severity::Error,
                        "deduction.inputs_type",
                        "deduction `inputs` must contain one to three fact or deduction IDs"
                            .to_string(),
                        &deduction.path,
                        Some(format!("{}/inputs", deduction.pointer)),
                        None,
                        Some(deduction.id.clone()),
                    );
                }
            }
            if deduction
                .mapping
                .get(Value::String("truth".to_string()))
                .is_some_and(|value| value.as_bool().is_none())
            {
                self.push(
                    Severity::Error,
                    "deduction.truth_type",
                    "deduction `truth` must be a boolean".to_string(),
                    &deduction.path,
                    Some(format!("{}/truth", deduction.pointer)),
                    None,
                    Some(deduction.id.clone()),
                );
            }
            if deduction
                .mapping
                .get(Value::String("contradicted_by".to_string()))
                .is_some_and(|value| !is_nonempty_string_sequence(value, false))
            {
                self.push(
                    Severity::Error,
                    "deduction.contradicted_by_type",
                    "deduction `contradicted_by` must contain at least one fact or deduction ID"
                        .to_string(),
                    &deduction.path,
                    Some(format!("{}/contradicted_by", deduction.pointer)),
                    None,
                    Some(deduction.id.clone()),
                );
            }
            if bool_field(&deduction.mapping, "truth") == Some(false)
                && string_list_field(&deduction.mapping, "contradicted_by").is_empty()
            {
                self.push(
                    Severity::Warning,
                    "deduction.false_without_contradiction",
                    "a false deduction should identify at least one fact or deduction that can contradict it".to_string(),
                    &deduction.path,
                    Some(format!("{}/contradicted_by", deduction.pointer)),
                    None,
                    Some(deduction.id.clone()),
                );
            }
            if let Some(solves) = deduction.mapping.get(Value::String("solves".to_string())) {
                let Some(solves) = solves.as_mapping() else {
                    self.push(
                        Severity::Error,
                        "deduction.solves_type",
                        "deduction `solves` must be a mapping".to_string(),
                        &deduction.path,
                        Some(format!("{}/solves", deduction.pointer)),
                        None,
                        Some(deduction.id.clone()),
                    );
                    continue;
                };
                if solves
                    .get(Value::String("time".to_string()))
                    .is_some_and(|time| !time.as_str().is_some_and(valid_time))
                {
                    self.push(
                        Severity::Error,
                        "deduction.solves_invalid_time",
                        "deduction `solves.time` must be a quoted 24-hour HH:MM value".to_string(),
                        &deduction.path,
                        Some(format!("{}/solves/time", deduction.pointer)),
                        None,
                        Some(deduction.id.clone()),
                    );
                }
            }
        }
    }

    fn validate_command_values(&mut self, commands: &[Item]) {
        for command in commands {
            if !string_field(&command.mapping, "name").is_some_and(|name| !name.trim().is_empty()) {
                self.push(
                    Severity::Error,
                    "command.name",
                    "command `name` must be a non-empty string".to_string(),
                    &command.path,
                    Some(format!("{}/name", command.pointer)),
                    None,
                    Some(command.id.clone()),
                );
            }

            if command
                .mapping
                .get(Value::String("description".to_string()))
                .is_some_and(|description| description.as_str().is_none())
            {
                self.push(
                    Severity::Error,
                    "command.description_type",
                    "command `description` must be a string".to_string(),
                    &command.path,
                    Some(format!("{}/description", command.pointer)),
                    None,
                    Some(command.id.clone()),
                );
            }

            if command
                .mapping
                .contains_key(Value::String("aliases".to_string()))
            {
                self.push(
                    Severity::Error,
                    "command.aliases_removed",
                    "command `aliases` has been removed".to_string(),
                    &command.path,
                    Some(format!("{}/aliases", command.pointer)),
                    None,
                    Some(command.id.clone()),
                );
            }

            let parameter_types = self.validate_command_parameters(command);
            if self.format_version == Some(2) {
                self.validate_runtime_command_signature(command, &parameter_types);
            }
            self.validate_world_effects(command, &parameter_types);
        }
    }

    fn validate_testimony_question_signature(&mut self, characters: &[Item], commands: &[Item]) {
        let testimony_authored = characters.iter().any(|character| {
            character
                .mapping
                .get(Value::String("testimony".to_string()))
                .and_then(Value::as_sequence)
                .is_some_and(|entries| !entries.is_empty())
        });
        if !testimony_authored {
            return;
        }
        let Some(command) = commands
            .iter()
            .find(|command| command.id == "command.question")
        else {
            // Each structurally valid testimony already reports its exact
            // unknown `command.question` requirement pointer.
            return;
        };
        let parameters_pointer = format!("{}/parameters", command.pointer);
        let Some(parameters) = command
            .mapping
            .get(Value::String("parameters".to_string()))
            .and_then(Value::as_sequence)
        else {
            self.push(
                Severity::Error,
                "character.testimony_question_parameters",
                "`command.question` must declare its character target as the first parameter"
                    .to_string(),
                &command.path,
                Some(parameters_pointer),
                None,
                Some(command.id.clone()),
            );
            return;
        };
        let Some(first) = parameters.first() else {
            self.push(
                Severity::Error,
                "character.testimony_question_target_missing",
                "`command.question` must declare a required character target first".to_string(),
                &command.path,
                Some(format!("{parameters_pointer}/0")),
                None,
                Some(command.id.clone()),
            );
            return;
        };
        let first_pointer = format!("{parameters_pointer}/0");
        let Some(first) = first.as_mapping() else {
            self.push(
                Severity::Error,
                "character.testimony_question_target_type",
                "the first `command.question` parameter must be a character target mapping"
                    .to_string(),
                &command.path,
                Some(first_pointer),
                None,
                Some(command.id.clone()),
            );
            return;
        };
        if string_field(first, "name") != Some("character") {
            self.push(
                Severity::Error,
                "character.testimony_question_target_name",
                "the first `command.question` parameter must be named `character`".to_string(),
                &command.path,
                Some(format!("{first_pointer}/name")),
                None,
                Some(command.id.clone()),
            );
        }
        if string_field(first, "type").and_then(CommandParameterType::parse)
            != Some(CommandParameterType::Character)
        {
            self.push(
                Severity::Error,
                "character.testimony_question_target_type",
                "the first `command.question` parameter must have type `character`".to_string(),
                &command.path,
                Some(format!("{first_pointer}/type")),
                None,
                Some(command.id.clone()),
            );
        }
        if bool_field(first, "required") != Some(true) {
            self.push(
                Severity::Error,
                "character.testimony_question_target_required",
                "the first `command.question` character parameter must be required".to_string(),
                &command.path,
                Some(format!("{first_pointer}/required")),
                None,
                Some(command.id.clone()),
            );
        }

        for (index, parameter) in parameters.iter().enumerate().skip(1) {
            let pointer = format!("{parameters_pointer}/{index}");
            let Some(parameter) = parameter.as_mapping() else {
                self.push(
                    Severity::Error,
                    "character.testimony_question_topic_type",
                    "later `command.question` parameters must be optional topic mappings"
                        .to_string(),
                    &command.path,
                    Some(pointer),
                    None,
                    Some(command.id.clone()),
                );
                continue;
            };
            let expected_type = match string_field(parameter, "name") {
                Some("topic_character") => Some(CommandParameterType::Character),
                Some("topic_entity") => Some(CommandParameterType::Entity),
                Some("topic_setting") => Some(CommandParameterType::Setting),
                Some("topic_deduction") => Some(CommandParameterType::Deduction),
                Some("topic_event") => Some(CommandParameterType::Event),
                _ => None,
            };
            if expected_type.is_none() {
                self.push(
                    Severity::Error,
                    "character.testimony_question_topic_name",
                    "later `command.question` parameters must be named for a supported optional topic"
                        .to_string(),
                    &command.path,
                    Some(format!("{pointer}/name")),
                    None,
                    Some(command.id.clone()),
                );
            } else if string_field(parameter, "type").and_then(CommandParameterType::parse)
                != expected_type
            {
                self.push(
                    Severity::Error,
                    "character.testimony_question_topic_type",
                    "a `command.question` topic parameter type must match its topic name"
                        .to_string(),
                    &command.path,
                    Some(format!("{pointer}/type")),
                    None,
                    Some(command.id.clone()),
                );
            }
            if bool_field(parameter, "required") == Some(true) {
                self.push(
                    Severity::Error,
                    "character.testimony_question_topic_required",
                    "later `command.question` topic parameters must be optional".to_string(),
                    &command.path,
                    Some(format!("{pointer}/required")),
                    None,
                    Some(command.id.clone()),
                );
            }
        }
    }

    fn validate_command_parameters(&mut self, command: &Item) -> Vec<Option<CommandParameterType>> {
        let Some(parameters) = command.mapping.get(Value::String("parameters".to_string())) else {
            return Vec::new();
        };
        let Some(parameters) = parameters.as_sequence() else {
            self.push(
                Severity::Error,
                "command.parameters_type",
                "`parameters` must be a sequence".to_string(),
                &command.path,
                Some(format!("{}/parameters", command.pointer)),
                None,
                Some(command.id.clone()),
            );
            return Vec::new();
        };

        let mut names = HashSet::new();
        parameters
            .iter()
            .enumerate()
            .map(|(index, parameter)| {
                let pointer = format!("{}/parameters/{index}", command.pointer);
                let Some(parameter) = parameter.as_mapping() else {
                    self.push(
                        Severity::Error,
                        "command.parameter_type",
                        "command parameters must be mappings".to_string(),
                        &command.path,
                        Some(pointer),
                        None,
                        Some(command.id.clone()),
                    );
                    return None;
                };
                let name = string_field(parameter, "name");
                if !name.is_some_and(|name| !name.trim().is_empty()) {
                    self.push(
                        Severity::Error,
                        "command.parameter_name",
                        "command parameter `name` must be a non-empty string".to_string(),
                        &command.path,
                        Some(format!("{pointer}/name")),
                        None,
                        Some(command.id.clone()),
                    );
                } else if !names.insert(name.expect("validated parameter name")) {
                    self.push(
                        Severity::Error,
                        "command.parameter_name_duplicate",
                        "command parameter names must be unique".to_string(),
                        &command.path,
                        Some(format!("{pointer}/name")),
                        None,
                        Some(command.id.clone()),
                    );
                }
                if parameter
                    .get(Value::String("description".to_string()))
                    .is_some_and(|description| description.as_str().is_none())
                {
                    self.push(
                        Severity::Error,
                        "command.parameter_description_type",
                        "command parameter `description` must be a string".to_string(),
                        &command.path,
                        Some(format!("{pointer}/description")),
                        None,
                        Some(command.id.clone()),
                    );
                }
                if parameter
                    .contains_key(Value::String("accepts".to_string()))
                {
                    self.push(
                        Severity::Error,
                        "command.parameter_accepts_removed",
                        "command parameter `accepts` has been replaced by singular `type`"
                            .to_string(),
                        &command.path,
                        Some(format!("{pointer}/accepts")),
                        None,
                        Some(command.id.clone()),
                    );
                }
                let parameter_type = string_field(parameter, "type")
                    .and_then(CommandParameterType::parse);
                if parameter_type.is_none() {
                    self.push(
                        Severity::Error,
                        "command.parameter_kind",
                        "command parameter `type` must be `character`, `entity`, `setting`, `deduction`, or `event`".to_string(),
                        &command.path,
                        Some(format!("{pointer}/type")),
                        None,
                        Some(command.id.clone()),
                    );
                }
                if parameter
                    .get(Value::String("required".to_string()))
                    .is_some_and(|required| required.as_bool().is_none())
                {
                    self.push(
                        Severity::Error,
                        "command.parameter_required_type",
                        "command parameter `required` must be a boolean".to_string(),
                        &command.path,
                        Some(format!("{pointer}/required")),
                        None,
                        Some(command.id.clone()),
                    );
                }
                parameter_type
            })
            .collect()
    }

    fn validate_runtime_command_signature(
        &mut self,
        command: &Item,
        parameter_types: &[Option<CommandParameterType>],
    ) {
        if matches!(command.id.as_str(), "command.take" | "command.drop") {
            self.validate_inventory_command_signature(command, parameter_types);
            return;
        }
        let known = parameter_types
            .iter()
            .flatten()
            .copied()
            .collect::<Vec<_>>();
        let valid = match command.id.as_str() {
            "command.claim" | "command.deduce" => known.is_empty(),
            "command.move" => known == [CommandParameterType::Setting],
            "command.solve" => {
                known
                    == [
                        CommandParameterType::Character,
                        CommandParameterType::Deduction,
                    ]
            }
            _ => true,
        };
        if !valid {
            self.push(
                Severity::Error,
                "command.runtime_signature",
                match command.id.as_str() {
                    "command.claim" | "command.deduce" => {
                        format!("reserved `{}` must not declare parameters", command.id)
                    }
                    "command.move" => {
                        "`command.move` must declare exactly one setting parameter".to_string()
                    }
                    "command.solve" => {
                        "`command.solve` must declare character then deduction parameters"
                            .to_string()
                    }
                    _ => unreachable!("only reserved runtime commands are checked"),
                },
                &command.path,
                Some(format!("{}/parameters", command.pointer)),
                None,
                Some(command.id.clone()),
            );
        }
    }

    fn validate_inventory_command_signature(
        &mut self,
        command: &Item,
        parameter_types: &[Option<CommandParameterType>],
    ) {
        let parameters_pointer = format!("{}/parameters", command.pointer);
        let parameters = command
            .mapping
            .get(Value::String("parameters".to_string()))
            .and_then(Value::as_sequence);
        let Some([parameter]) = parameters.map(Vec::as_slice) else {
            let pointer = parameters
                .filter(|parameters| parameters.len() > 1)
                .map_or(parameters_pointer.clone(), |parameters| {
                    format!("{parameters_pointer}/{}", parameters.len() - 1)
                });
            self.push_inventory_signature_error(command, pointer);
            return;
        };
        if parameter_types != [Some(CommandParameterType::Entity)] {
            self.push_inventory_signature_error(command, format!("{parameters_pointer}/0/type"));
            return;
        }
        if parameter
            .as_mapping()
            .and_then(|parameter| bool_field(parameter, "required"))
            != Some(true)
        {
            self.push_inventory_signature_error(
                command,
                format!("{parameters_pointer}/0/required"),
            );
        }
    }

    fn push_inventory_signature_error(&mut self, command: &Item, pointer: String) {
        self.push(
            Severity::Error,
            "command.runtime_signature",
            format!(
                "`{}` must declare exactly one required entity parameter",
                command.id
            ),
            &command.path,
            Some(pointer),
            None,
            Some(command.id.clone()),
        );
    }

    fn validate_world_effects(
        &mut self,
        command: &Item,
        parameter_types: &[Option<CommandParameterType>],
    ) {
        let Some(effects) = command.mapping.get(Value::String("effects".to_string())) else {
            return;
        };
        let Some(effects) = effects.as_sequence() else {
            self.push(
                Severity::Error,
                "command.effects_type",
                "command `effects` must be a sequence".to_string(),
                &command.path,
                Some(format!("{}/effects", command.pointer)),
                None,
                Some(command.id.clone()),
            );
            return;
        };

        for (index, effect) in effects.iter().enumerate() {
            let pointer = format!("{}/effects/{index}", command.pointer);
            let Some(effect) = effect.as_mapping() else {
                self.push(
                    Severity::Error,
                    "command.effect_type",
                    "command effects must be mappings".to_string(),
                    &command.path,
                    Some(pointer),
                    None,
                    Some(command.id.clone()),
                );
                continue;
            };
            let Some(operation) =
                string_field(effect, "operation").filter(|operation| !operation.trim().is_empty())
            else {
                self.push(
                    Severity::Error,
                    "command.effect_operation",
                    "command effect `operation` must be a non-empty string".to_string(),
                    &command.path,
                    Some(format!("{pointer}/operation")),
                    None,
                    Some(command.id.clone()),
                );
                continue;
            };

            match operation {
                "set_flag" => {
                    self.validate_command_effect_fields(
                        command,
                        effect,
                        &pointer,
                        &["operation", "flag", "value", "after"],
                    );
                    self.validate_command_effect_reference_field(
                        command,
                        effect,
                        &pointer,
                        "flag",
                        &[Kind::Flag],
                        parameter_types,
                    );
                    if effect
                        .get(Value::String("value".to_string()))
                        .and_then(Value::as_bool)
                        .is_none()
                    {
                        self.push(
                            Severity::Error,
                            "effect.flag_value",
                            "`set_flag.value` must be true or false".to_string(),
                            &command.path,
                            Some(format!("{pointer}/value")),
                            None,
                            Some(command.id.clone()),
                        );
                    }
                    if let Some(after) = effect.get(Value::String("after".to_string())) {
                        if !command.id.starts_with("trigger.")
                            || !after.as_str().is_some_and(valid_delay)
                            || effect
                                .get(Value::String("value".to_string()))
                                .and_then(Value::as_bool)
                                != Some(true)
                        {
                            self.push(
                                Severity::Error,
                                "effect.delay",
                                "`set_flag.after` is available for trigger assignment to true and must be a positive delay such as `20m`, `1h`, or `2turns`".to_string(),
                                &command.path,
                                Some(format!("{pointer}/after")),
                                None,
                                Some(command.id.clone()),
                            );
                        }
                    }
                }
                "advance_time" => {
                    self.validate_command_effect_fields(
                        command,
                        effect,
                        &pointer,
                        &["operation", "minutes", "route"],
                    );
                    let valid_minutes = integer_field(effect, "minutes")
                        .is_some_and(|minutes| minutes > 0 && u32::try_from(minutes).is_ok());
                    let has_route = effect.contains_key(Value::String("route".to_string()));
                    if valid_minutes == has_route {
                        self.push(
                            Severity::Error,
                            "command.effect_minutes",
                            "`advance_time` requires exactly one positive whole `minutes` value or route reference"
                                .to_string(),
                            &command.path,
                            Some(format!("{pointer}/minutes")),
                            None,
                            Some(command.id.clone()),
                        );
                    }
                    if has_route {
                        self.validate_command_effect_reference_field(
                            command,
                            effect,
                            &pointer,
                            "route",
                            &[Kind::Route],
                            parameter_types,
                        );
                    }
                }
                "move" => {
                    self.validate_command_effect_fields(
                        command,
                        effect,
                        &pointer,
                        &["operation", "subjects", "setting"],
                    );
                    match effect
                        .get(Value::String("subjects".to_string()))
                        .and_then(Value::as_sequence)
                    {
                        Some(subjects) if !subjects.is_empty() => {
                            for (subject_index, subject) in subjects.iter().enumerate() {
                                self.validate_command_effect_reference(
                                    command,
                                    subject,
                                    &format!("{pointer}/subjects/{subject_index}"),
                                    &[Kind::Character, Kind::Entity],
                                    parameter_types,
                                    true,
                                );
                            }
                        }
                        _ => self.push(
                            Severity::Error,
                            "command.effect_subjects",
                            "`move.subjects` must be a non-empty sequence of players, characters, or entities".to_string(),
                            &command.path,
                            Some(format!("{pointer}/subjects")),
                            None,
                            Some(command.id.clone()),
                        ),
                    }
                    self.validate_command_effect_reference_field(
                        command,
                        effect,
                        &pointer,
                        "setting",
                        &[Kind::Setting],
                        parameter_types,
                    );
                }
                "transform" => {
                    self.validate_command_effect_fields(
                        command,
                        effect,
                        &pointer,
                        &["operation", "entity_from", "entity_to"],
                    );
                    for field in ["entity_from", "entity_to"] {
                        self.validate_command_effect_reference_field(
                            command,
                            effect,
                            &pointer,
                            field,
                            &[Kind::Entity],
                            parameter_types,
                        );
                    }
                }
                "reveal" | "conceal" => {
                    self.validate_command_effect_fields(
                        command,
                        effect,
                        &pointer,
                        &["operation", "entity"],
                    );
                    self.validate_command_effect_reference_field(
                        command,
                        effect,
                        &pointer,
                        "entity",
                        &[Kind::Entity],
                        parameter_types,
                    );
                }
                "learn_fact" => {
                    self.validate_command_effect_fields(
                        command,
                        effect,
                        &pointer,
                        &["operation", "fact_id"],
                    );
                    self.validate_command_effect_reference_field(
                        command,
                        effect,
                        &pointer,
                        "fact_id",
                        &[Kind::Fact],
                        parameter_types,
                    );
                }
                "establish_deduction" => {
                    self.validate_command_effect_fields(
                        command,
                        effect,
                        &pointer,
                        &["operation", "deduction_id"],
                    );
                    self.validate_command_effect_reference_field(
                        command,
                        effect,
                        &pointer,
                        "deduction_id",
                        &[Kind::Deduction],
                        parameter_types,
                    );
                }
                "describe" | "win" | "lose" => {
                    self.validate_command_effect_fields(
                        command,
                        effect,
                        &pointer,
                        &["operation", "text"],
                    );
                    if !string_field(effect, "text").is_some_and(|text| !text.trim().is_empty()) {
                        self.push(
                            Severity::Error,
                            "command.effect_text",
                            format!("`{operation}.text` must be a non-empty string"),
                            &command.path,
                            Some(format!("{pointer}/text")),
                            None,
                            Some(command.id.clone()),
                        );
                    }
                }
                _ => self.push(
                    Severity::Error,
                    "command.effect_unknown_operation",
                    format!("unknown command effect operation `{operation}`"),
                    &command.path,
                    Some(format!("{pointer}/operation")),
                    None,
                    Some(command.id.clone()),
                ),
            }
        }
    }

    fn validate_command_effect_fields(
        &mut self,
        command: &Item,
        effect: &Mapping,
        pointer: &str,
        allowed: &[&str],
    ) {
        for key in effect.keys() {
            let Some(key) = key.as_str() else {
                self.push(
                    Severity::Error,
                    "command.effect_field",
                    "command effect field names must be strings".to_string(),
                    &command.path,
                    Some(pointer.to_string()),
                    None,
                    Some(command.id.clone()),
                );
                continue;
            };
            if !allowed.contains(&key) {
                self.push(
                    Severity::Error,
                    "command.effect_unknown_field",
                    format!("`{key}` is not valid for this command effect"),
                    &command.path,
                    Some(format!("{pointer}/{}", escape_pointer(key))),
                    None,
                    Some(command.id.clone()),
                );
            }
        }
    }

    fn validate_command_effect_reference_field(
        &mut self,
        command: &Item,
        effect: &Mapping,
        pointer: &str,
        field: &str,
        expected: &[Kind],
        parameter_types: &[Option<CommandParameterType>],
    ) {
        let value = effect
            .get(Value::String(field.to_string()))
            .unwrap_or(&Value::Null);
        self.validate_command_effect_reference(
            command,
            value,
            &format!("{pointer}/{}", escape_pointer(field)),
            expected,
            parameter_types,
            false,
        );
    }

    fn validate_command_effect_reference(
        &mut self,
        command: &Item,
        value: &Value,
        pointer: &str,
        expected: &[Kind],
        parameter_types: &[Option<CommandParameterType>],
        allow_player: bool,
    ) {
        let Some(reference) = value
            .as_str()
            .filter(|reference| !reference.trim().is_empty())
        else {
            self.push(
                Severity::Error,
                "command.effect_reference",
                format!(
                    "command effect operand must be {}",
                    command_effect_expected_names(expected, allow_player)
                ),
                &command.path,
                Some(pointer.to_string()),
                None,
                Some(command.id.clone()),
            );
            return;
        };

        if allow_player && reference == "player" {
            return;
        }
        if reference == "route" && expected.contains(&Kind::Route) {
            return;
        }

        match command_parameter_reference(reference) {
            Ok(Some(index)) => {
                let Some(parameter_type) = parameter_types.get(index).copied().flatten() else {
                    self.push(
                        Severity::Error,
                        "command.effect_parameter_unknown",
                        format!("`{reference}` does not refer to a declared action parameter"),
                        &command.path,
                        Some(pointer.to_string()),
                        None,
                        Some(command.id.clone()),
                    );
                    return;
                };
                if !expected.contains(&parameter_type.kind()) {
                    self.push(
                        Severity::Error,
                        "command.effect_parameter_type",
                        format!(
                            "`{reference}` is a {} parameter; expected {}",
                            parameter_type.name(),
                            command_effect_expected_names(expected, allow_player)
                        ),
                        &command.path,
                        Some(pointer.to_string()),
                        None,
                        Some(command.id.clone()),
                    );
                }
                return;
            }
            Err(()) => {
                self.push(
                    Severity::Error,
                    "command.effect_parameter_unknown",
                    format!("`{reference}` is not a valid 1-based `paramN` reference"),
                    &command.path,
                    Some(pointer.to_string()),
                    None,
                    Some(command.id.clone()),
                );
                return;
            }
            Ok(None) => {}
        }

        if !looks_like_id(reference) {
            self.push(
                Severity::Error,
                "command.effect_reference",
                format!(
                    "`{reference}` must be {}",
                    command_effect_expected_names(expected, allow_player)
                ),
                &command.path,
                Some(pointer.to_string()),
                locate_scalar(&command.source, reference),
                Some(reference.to_string()),
            );
            return;
        }

        if let Some(definition) = self.definitions.get(reference) {
            if !expected.contains(&definition.kind) {
                let actual_kind = definition.kind;
                self.push(
                    Severity::Error,
                    "reference.wrong_type",
                    format!(
                        "`{reference}` refers to a {}; expected {}",
                        actual_kind.name(),
                        command_effect_expected_names(expected, allow_player)
                    ),
                    &command.path,
                    Some(pointer.to_string()),
                    locate_scalar(&command.source, reference),
                    Some(reference.to_string()),
                );
            }
        }
    }

    fn validate_trigger_values(
        &mut self,
        triggers: &[Item],
        commands: &[Item],
        _fact_claims_enabled: bool,
    ) {
        for trigger in triggers {
            let command = string_field(&trigger.mapping, "command")
                .and_then(|command_id| commands.iter().find(|command| command.id == command_id));
            let parameter_types = command.map(command_parameter_types).unwrap_or_default();
            if !string_field(&trigger.mapping, "command")
                .is_some_and(|command| !command.trim().is_empty())
            {
                self.push(
                    Severity::Error,
                    "trigger.missing_command",
                    "trigger `command` must be a command ID".to_string(),
                    &trigger.path,
                    Some(format!("{}/command", trigger.pointer)),
                    None,
                    Some(trigger.id.clone()),
                );
            }
            if trigger
                .mapping
                .get(Value::String("once".to_string()))
                .is_some_and(|once| once.as_bool().is_none())
            {
                self.push(
                    Severity::Error,
                    "trigger.once_type",
                    "trigger `once` must be a boolean".to_string(),
                    &trigger.path,
                    Some(format!("{}/once", trigger.pointer)),
                    None,
                    Some(trigger.id.clone()),
                );
            }
            if trigger
                .mapping
                .contains_key(Value::String("conditions".to_string()))
            {
                self.push(
                    Severity::Error,
                    "trigger.conditions_removed",
                    "trigger `conditions` has been replaced by `time`, `location`, `any_of`, and `all_of`".to_string(),
                    &trigger.path,
                    Some(format!("{}/conditions", trigger.pointer)),
                    None,
                    Some(trigger.id.clone()),
                );
            }
            self.validate_trigger_time(trigger);
            self.validate_trigger_location(trigger);
            self.validate_trigger_gate_list(trigger, "any_of");
            self.validate_trigger_gate_list(trigger, "all_of");
            self.validate_trigger_parameters(trigger, command);
            self.validate_world_effects(trigger, &parameter_types);
        }
    }

    fn validate_trigger_parameters(&mut self, trigger: &Item, command: Option<&Item>) {
        let Some(value) = trigger.mapping.get(Value::String("parameters".to_string())) else {
            return;
        };
        let pointer = format!("{}/parameters", trigger.pointer);
        let Some(bindings) = value.as_mapping() else {
            self.push(
                Severity::Error,
                "trigger.parameters_type",
                "trigger `parameters` must be a mapping from command parameter names to authored IDs"
                    .to_string(),
                &trigger.path,
                Some(pointer),
                None,
                Some(trigger.id.clone()),
            );
            return;
        };
        let parameters = command
            .and_then(|command| {
                command
                    .mapping
                    .get(Value::String("parameters".to_string()))
                    .and_then(Value::as_sequence)
            })
            .into_iter()
            .flatten()
            .filter_map(|parameter| {
                let parameter = parameter.as_mapping()?;
                Some((
                    string_field(parameter, "name")?.to_string(),
                    CommandParameterType::parse(string_field(parameter, "type")?)?,
                ))
            })
            .collect::<BTreeMap<_, _>>();

        for (raw_name, raw_reference) in bindings {
            let Some(name) = raw_name.as_str().filter(|name| !name.trim().is_empty()) else {
                self.push(
                    Severity::Error,
                    "trigger.parameter_name",
                    "trigger parameter binding names must be non-empty strings".to_string(),
                    &trigger.path,
                    Some(pointer.clone()),
                    None,
                    Some(trigger.id.clone()),
                );
                continue;
            };
            let binding_pointer = format!("{pointer}/{}", escape_pointer(name));
            let Some(expected) = parameters.get(name).copied() else {
                self.push(
                    Severity::Error,
                    "trigger.parameter_unknown",
                    format!(
                        "`{name}` is not a parameter of `{}`",
                        string_field(&trigger.mapping, "command").unwrap_or("the trigger command")
                    ),
                    &trigger.path,
                    Some(binding_pointer),
                    None,
                    Some(trigger.id.clone()),
                );
                continue;
            };
            let Some(reference) = raw_reference
                .as_str()
                .filter(|reference| !reference.trim().is_empty())
            else {
                self.push(
                    Severity::Error,
                    "trigger.parameter_reference",
                    format!("trigger parameter `{name}` must bind to a non-empty authored ID"),
                    &trigger.path,
                    Some(binding_pointer),
                    None,
                    Some(trigger.id.clone()),
                );
                continue;
            };
            match self.definitions.get(reference) {
                Some(definition) if definition.kind == expected.kind() => {}
                Some(definition) => self.push(
                    Severity::Error,
                    "reference.wrong_type",
                    format!(
                        "`{reference}` refers to a {}; parameter `{name}` expects {}",
                        definition.kind.name(),
                        expected.name()
                    ),
                    &trigger.path,
                    Some(binding_pointer),
                    locate_scalar(&trigger.source, reference),
                    Some(reference.to_string()),
                ),
                None => self.push(
                    Severity::Error,
                    "reference.unknown",
                    format!("reference `{reference}` is not defined"),
                    &trigger.path,
                    Some(binding_pointer),
                    locate_scalar(&trigger.source, reference),
                    Some(reference.to_string()),
                ),
            }
        }
    }

    fn validate_trigger_time(&mut self, trigger: &Item) {
        let Some(time) = trigger.mapping.get(Value::String("time".to_string())) else {
            return;
        };
        if time.is_null() {
            return;
        }
        let Some(time) = time.as_mapping() else {
            self.push(
                Severity::Error,
                "trigger.time_type",
                "trigger `time` must be a mapping with `relation` and `value`".to_string(),
                &trigger.path,
                Some(format!("{}/time", trigger.pointer)),
                None,
                Some(trigger.id.clone()),
            );
            return;
        };

        for key in time.keys() {
            let Some(key) = key.as_str() else {
                self.push(
                    Severity::Error,
                    "trigger.time_field",
                    "trigger `time` field names must be strings".to_string(),
                    &trigger.path,
                    Some(format!("{}/time", trigger.pointer)),
                    None,
                    Some(trigger.id.clone()),
                );
                continue;
            };
            if !matches!(key, "relation" | "value") {
                self.push(
                    Severity::Error,
                    "trigger.time_unknown_field",
                    format!("`{key}` is not valid in trigger `time`"),
                    &trigger.path,
                    Some(format!("{}/time/{}", trigger.pointer, escape_pointer(key))),
                    None,
                    Some(trigger.id.clone()),
                );
            }
        }

        if !string_field(time, "relation")
            .is_some_and(|relation| matches!(relation, "before" | "at" | "after"))
        {
            self.push(
                Severity::Error,
                "trigger.time_relation",
                "trigger `time.relation` must be `before`, `at`, or `after`".to_string(),
                &trigger.path,
                Some(format!("{}/time/relation", trigger.pointer)),
                None,
                Some(trigger.id.clone()),
            );
        }
        if !string_field(time, "value").is_some_and(valid_time) {
            self.push(
                Severity::Error,
                "trigger.time_value",
                "trigger `time.value` must be a quoted 24-hour HH:MM value".to_string(),
                &trigger.path,
                Some(format!("{}/time/value", trigger.pointer)),
                None,
                Some(trigger.id.clone()),
            );
        }
    }

    fn validate_trigger_location(&mut self, trigger: &Item) {
        let Some(location) = trigger.mapping.get(Value::String("location".to_string())) else {
            return;
        };
        if location.is_null()
            || location
                .as_str()
                .is_some_and(|location| location.trim().is_empty())
            || location.as_str().is_some()
        {
            return;
        }
        self.push(
            Severity::Error,
            "trigger.location_type",
            "trigger `location` must be a setting ID or blank".to_string(),
            &trigger.path,
            Some(format!("{}/location", trigger.pointer)),
            None,
            Some(trigger.id.clone()),
        );
    }

    fn validate_trigger_gate_list(&mut self, trigger: &Item, field: &str) {
        let Some(values) = trigger.mapping.get(Value::String(field.to_string())) else {
            return;
        };
        let Some(values) = values.as_sequence() else {
            self.push(
                Severity::Error,
                &format!("trigger.{field}_type"),
                format!("trigger `{field}` must be a sequence of character, entity, or flag IDs"),
                &trigger.path,
                Some(format!("{}/{}", trigger.pointer, escape_pointer(field))),
                None,
                Some(trigger.id.clone()),
            );
            return;
        };
        for (index, value) in values.iter().enumerate() {
            if !value
                .as_str()
                .is_some_and(|reference| !reference.trim().is_empty())
            {
                self.push(
                    Severity::Error,
                    &format!("trigger.{field}_reference"),
                    format!("trigger `{field}` entries must be character, entity, or flag IDs"),
                    &trigger.path,
                    Some(format!("{}/{field}/{index}", trigger.pointer)),
                    None,
                    Some(trigger.id.clone()),
                );
            }
        }
    }

    fn require_nonnegative_integer(&mut self, item: &Item, field: &str, code: &str) {
        if !matches!(integer_field(&item.mapping, field), Some(value) if value >= 0) {
            self.push(
                Severity::Error,
                code,
                format!("`{field}` must be a non-negative integer"),
                &item.path,
                Some(format!("{}/{}", item.pointer, escape_pointer(field))),
                None,
                Some(item.id.clone()),
            );
        }
    }

    fn validate_graphs(
        &mut self,
        settings: &[Item],
        entities: &[Item],
        clues: &[Item],
        facts: &[Item],
        deductions: &[Item],
        fact_claims_enabled: bool,
    ) {
        let setting_parents = settings
            .iter()
            .filter_map(|item| {
                string_field(&item.mapping, "parent")
                    .map(|parent| (item.id.clone(), vec![parent.to_string()]))
            })
            .collect();
        self.validate_cycles(
            setting_parents,
            "setting.parent_cycle",
            "setting parent hierarchy",
        );

        let entity_containment = entities
            .iter()
            .filter_map(|item| {
                nested_string_field(&item.mapping, &["initial", "container"])
                    .filter(|container| {
                        self.definitions
                            .get(*container)
                            .is_some_and(|definition| definition.kind == Kind::Entity)
                    })
                    .map(|container| (item.id.clone(), vec![container.to_string()]))
            })
            .collect();
        for cycle in find_cycles(&entity_containment) {
            let subject = cycle.first().cloned();
            let item = subject
                .as_deref()
                .and_then(|id| entities.iter().find(|item| item.id == id));
            let container =
                item.and_then(|item| nested_string_field(&item.mapping, &["initial", "container"]));
            self.push(
                Severity::Error,
                "entity.containment_cycle",
                format!(
                    "entity containment contains a cycle: {}",
                    cycle.join(" -> ")
                ),
                item.map_or("", |item| item.path.as_str()),
                item.map(|item| format!("{}/initial/container", item.pointer)),
                item.and_then(|item| container.and_then(|id| locate_scalar(&item.source, id))),
                subject,
            );
        }

        if fact_claims_enabled {
            self.validate_cycles(
                fact_dependency_graph(facts),
                "fact.requirement_cycle",
                "fact requirement",
            );
        } else {
            self.validate_cycles(
                dependency_graph(clues, "requires"),
                "clue.dependency_cycle",
                "clue dependency",
            );
        }
        self.validate_cycles(
            deduction_dependency_graph(deductions),
            "deduction.dependency_cycle",
            "deduction dependency",
        );
    }

    fn validate_cycles(&mut self, graph: BTreeMap<String, Vec<String>>, code: &str, label: &str) {
        for cycle in find_cycles(&graph) {
            let subject = cycle.first().cloned();
            let location = subject
                .as_deref()
                .and_then(|id| self.definitions.get(id))
                .cloned();
            self.push(
                Severity::Error,
                code,
                format!("{label} contains a cycle: {}", cycle.join(" -> ")),
                location
                    .as_ref()
                    .map_or("", |definition| definition.path.as_str()),
                location
                    .as_ref()
                    .map(|definition| definition.pointer.clone()),
                location.as_ref().and_then(|definition| definition.range),
                subject,
            );
        }
    }

    fn validate_navigation(&mut self, cases: &[Item], settings: &[Item], route_items: &[Item]) {
        let navigable: BTreeSet<String> = settings
            .iter()
            .filter(|setting| {
                bool_field(&setting.mapping, "navigable")
                    .unwrap_or_else(|| string_field(&setting.mapping, "type") != Some("island"))
            })
            .map(|setting| setting.id.clone())
            .collect();
        if navigable.is_empty() {
            return;
        }

        let routes: Vec<Route> = route_items
            .iter()
            .map(|item| Route {
                id: item.id.clone(),
                path: item.path.clone(),
                pointer: item.pointer.clone(),
                from: string_field(&item.mapping, "from").map(str::to_string),
                to: string_field(&item.mapping, "to").map(str::to_string),
                bidirectional: bool_field(&item.mapping, "bidirectional").unwrap_or(false),
            })
            .collect();
        let mut forward: BTreeMap<String, Vec<String>> = navigable
            .iter()
            .map(|id| (id.clone(), Vec::new()))
            .collect();
        let mut reverse = forward.clone();
        for route in routes {
            let (Some(from), Some(to)) = (route.from, route.to) else {
                continue;
            };
            if !navigable.contains(&from) || !navigable.contains(&to) {
                if self
                    .definitions
                    .get(&from)
                    .is_some_and(|definition| definition.kind == Kind::Setting)
                    || self
                        .definitions
                        .get(&to)
                        .is_some_and(|definition| definition.kind == Kind::Setting)
                {
                    self.push(
                        Severity::Error,
                        "route.non_navigable_endpoint",
                        format!(
                            "route `{}` connects a setting marked non-navigable",
                            route.id
                        ),
                        &route.path,
                        Some(route.pointer),
                        None,
                        Some(route.id),
                    );
                }
                continue;
            }
            forward.entry(from.clone()).or_default().push(to.clone());
            reverse.entry(to.clone()).or_default().push(from.clone());
            if route.bidirectional {
                forward.entry(to.clone()).or_default().push(from.clone());
                reverse.entry(from).or_default().push(to);
            }
        }

        let case = cases.first();
        let explicit_entries = case
            .map(|item| string_list_field(&item.mapping, "entry_settings"))
            .unwrap_or_default();
        let explicit_exits = case
            .map(|item| string_list_field(&item.mapping, "exit_settings"))
            .unwrap_or_default();
        let has_contract = case.is_some_and(|item| {
            item.mapping
                .contains_key(Value::String("entry_settings".to_string()))
                || item
                    .mapping
                    .contains_key(Value::String("exit_settings".to_string()))
        });
        if !has_contract {
            if let Some(case) = case {
                self.push(
                    Severity::Warning,
                    "navigation.implicit_contract",
                    "add `entry_settings` and `exit_settings`; compatibility mode requires every navigable setting to reach every other".to_string(),
                    &case.path,
                    Some(case.pointer.clone()),
                    None,
                    Some(case.id.clone()),
                );
            }
        }

        let entries: Vec<String> = if explicit_entries.is_empty() {
            if has_contract {
                Vec::new()
            } else {
                navigable.iter().next().cloned().into_iter().collect()
            }
        } else {
            explicit_entries
                .into_iter()
                .filter(|id| navigable.contains(id))
                .collect()
        };
        let exits: Vec<String> = if explicit_exits.is_empty() {
            if has_contract {
                Vec::new()
            } else {
                entries.clone()
            }
        } else {
            explicit_exits
                .into_iter()
                .filter(|id| navigable.contains(id))
                .collect()
        };

        if has_contract && entries.is_empty() {
            self.push_case_navigation_error(
                case,
                "navigation.entry_missing",
                "at least one valid, navigable entry setting is required",
                "entry_settings",
            );
        }
        if has_contract && exits.is_empty() {
            self.push_case_navigation_error(
                case,
                "navigation.exit_missing",
                "at least one valid, navigable exit setting is required",
                "exit_settings",
            );
        }

        let reachable_from_entry = reachable(&forward, &entries);
        let can_reach_exit = reachable(&reverse, &exits);
        for setting in settings.iter().filter(|item| navigable.contains(&item.id)) {
            if !entries.is_empty() && !reachable_from_entry.contains(&setting.id) {
                self.push(
                    Severity::Error,
                    "navigation.unreachable",
                    format!("setting `{}` cannot be reached from an entry", setting.id),
                    &setting.path,
                    Some(setting.pointer.clone()),
                    locate_scalar(&setting.source, &setting.id),
                    Some(setting.id.clone()),
                );
            }
            if !exits.is_empty() && !can_reach_exit.contains(&setting.id) {
                self.push(
                    Severity::Error,
                    "navigation.no_exit",
                    format!("setting `{}` cannot reach an exit", setting.id),
                    &setting.path,
                    Some(setting.pointer.clone()),
                    locate_scalar(&setting.source, &setting.id),
                    Some(setting.id.clone()),
                );
            }
        }
    }

    fn push_case_navigation_error(
        &mut self,
        case: Option<&Item>,
        code: &str,
        message: &str,
        field: &str,
    ) {
        self.push(
            Severity::Error,
            code,
            message.to_string(),
            case.map_or("", |item| item.path.as_str()),
            case.map(|item| format!("{}/{}", item.pointer, field)),
            None,
            case.map(|item| item.id.clone()),
        );
    }

    fn validate_flag_values(&mut self, flags: &[Item]) {
        for flag in flags {
            for field in ["name", "description"] {
                if !string_field(&flag.mapping, field).is_some_and(|value| !value.trim().is_empty())
                {
                    self.push(
                        Severity::Error,
                        &format!("flag.{field}"),
                        format!("flag `{field}` must be a non-empty string"),
                        &flag.path,
                        Some(format!("{}/{}", flag.pointer, escape_pointer(field))),
                        None,
                        Some(flag.id.clone()),
                    );
                }
            }
            if flag
                .mapping
                .get(Value::String("initial_state".to_string()))
                .and_then(Value::as_bool)
                .is_none()
            {
                self.push(
                    Severity::Error,
                    "flag.initial_state",
                    "flag `initial_state` must be a boolean".to_string(),
                    &flag.path,
                    Some(format!("{}/initial_state", flag.pointer)),
                    None,
                    Some(flag.id.clone()),
                );
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn push(
        &mut self,
        severity: Severity,
        code: &str,
        message: String,
        path: &str,
        pointer: Option<String>,
        range: Option<SourceRange>,
        subject_id: Option<String>,
    ) {
        self.diagnostics.push(Diagnostic {
            severity,
            code: code.to_string(),
            message,
            path: path.to_string(),
            pointer,
            range,
            subject_id,
            related: Vec::new(),
        });
    }
}

#[derive(Debug)]
struct Reference {
    id: String,
    pointer: String,
}

fn command_parameter_reference(value: &str) -> Result<Option<usize>, ()> {
    let Some(suffix) = value.strip_prefix("param") else {
        return Ok(None);
    };
    if suffix.is_empty() || !suffix.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(());
    }
    let number = suffix.parse::<usize>().map_err(|_| ())?;
    number.checked_sub(1).map(Some).ok_or(())
}

fn command_effect_expected_names(expected: &[Kind], allow_player: bool) -> String {
    let mut names: Vec<_> = expected.iter().map(|kind| kind.name()).collect();
    if allow_player {
        names.insert(0, "player");
    }
    match names.as_slice() {
        [] => "a compatible authored ID".to_string(),
        [name] => format!("a {name} ID or compatible `paramN` reference"),
        [first, second] => {
            format!("a {first} or {second} ID, or compatible `paramN` reference")
        }
        _ => format!(
            "a {} ID, or compatible `paramN` reference",
            names.join(", ")
        ),
    }
}

fn collect_references(
    value: &Value,
    pointer: &str,
    key: Option<&str>,
    references: &mut Vec<Reference>,
) {
    match value {
        Value::String(id)
            if key != Some("id")
                && !(id.trim().is_empty() && is_blankable_reference_pointer(pointer))
                && (looks_like_id(id) || is_reference_pointer(pointer)) =>
        {
            references.push(Reference {
                id: id.clone(),
                pointer: pointer.to_string(),
            });
        }
        Value::Sequence(values) => {
            for (index, value) in values.iter().enumerate() {
                collect_references(value, &format!("{pointer}/{index}"), key, references);
            }
        }
        Value::Mapping(values) => {
            for (map_key, value) in values {
                let Some(map_key) = map_key.as_str() else {
                    continue;
                };
                collect_references(
                    value,
                    &format!("{pointer}/{}", escape_pointer(map_key)),
                    Some(map_key),
                    references,
                );
            }
        }
        Value::Tagged(tagged) => {
            collect_references(&tagged.value, pointer, key, references);
        }
        _ => {}
    }
}

fn is_reference_pointer(pointer: &str) -> bool {
    expected_kind(pointer).is_some()
}

fn is_blankable_reference_pointer(pointer: &str) -> bool {
    pointer.starts_with("/triggers/") && pointer.ends_with("/location")
}

fn expected_kind(pointer: &str) -> Option<&'static [Kind]> {
    let field = pointer.rsplit('/').next().unwrap_or_default();
    let parent = pointer.rsplit('/').nth(1).unwrap_or_default();
    const SETTINGS: &[Kind] = &[Kind::Setting];
    const CHARACTERS: &[Kind] = &[Kind::Character];
    const ENTITIES: &[Kind] = &[Kind::Entity];
    const CLUES: &[Kind] = &[Kind::Clue];
    const FACTS: &[Kind] = &[Kind::Fact];
    const FACT_REQUIREMENTS: &[Kind] = &[
        Kind::Setting,
        Kind::Route,
        Kind::Character,
        Kind::Entity,
        Kind::Event,
        Kind::Fact,
        Kind::Deduction,
        Kind::Flag,
        Kind::Command,
        Kind::Trigger,
    ];
    const DEDUCTIONS: &[Kind] = &[Kind::Deduction];
    const TRIGGER_GATES: &[Kind] = &[Kind::Character, Kind::Entity, Kind::Flag];
    const FACT_OR_DEDUCTION: &[Kind] = &[Kind::Fact, Kind::Deduction];
    const COMMANDS: &[Kind] = &[Kind::Command];
    const CONTAINERS: &[Kind] = &[Kind::Setting, Kind::Character, Kind::Entity];
    const PERSISTENT_REQUIREMENTS: &[Kind] = &[
        Kind::Setting,
        Kind::Entity,
        Kind::Fact,
        Kind::Deduction,
        Kind::Flag,
        Kind::Trigger,
    ];
    const ROUTE_REQUIREMENTS: &[Kind] = &[Kind::Entity, Kind::Flag, Kind::Trigger];
    const CONTENT_SOURCES: &[Kind] = &[Kind::Setting, Kind::Character, Kind::Entity, Kind::Event];
    const FACT_TOPICS: &[Kind] = &[
        Kind::Setting,
        Kind::Route,
        Kind::Character,
        Kind::Entity,
        Kind::Event,
        Kind::Command,
        Kind::Trigger,
    ];
    const FACT_SOURCES: &[Kind] = &[
        Kind::Setting,
        Kind::Route,
        Kind::Character,
        Kind::Entity,
        Kind::Event,
        Kind::Clue,
        Kind::Command,
        Kind::Trigger,
    ];
    match field {
        "victim" | "culprit" | "testimony_source" => Some(CHARACTERS),
        "weapon" => Some(ENTITIES),
        "parent" | "from" | "to" | "location" => Some(SETTINGS),
        "container" => Some(CONTAINERS),
        "deduction" if pointer.contains("/solution/") => Some(DEDUCTIONS),
        _ if parent == "participants" => Some(CHARACTERS),
        _ if parent == "supported_by" => Some(CLUES),
        _ if parent == "inputs" && pointer.contains("/deductions/") => Some(FACT_OR_DEDUCTION),
        _ if parent == "contradicted_by" && pointer.contains("/deductions/") => {
            Some(FACT_OR_DEDUCTION)
        }
        _ if parent == "about" && pointer.contains("/facts/") => Some(FACT_TOPICS),
        _ if parent == "sources" && pointer.contains("/facts/") => Some(FACT_SOURCES),
        _ if (field == "requires" || parent == "requires") && pointer.contains("/facts/") => {
            Some(FACT_REQUIREMENTS)
        }
        _ if is_entity_visibility_requirement_pointer(pointer) => Some(PERSISTENT_REQUIREMENTS),
        _ if is_character_testimony_list_pointer(pointer, "requires") => Some(FACT_REQUIREMENTS),
        _ if is_character_testimony_list_pointer(pointer, "reveals") => Some(FACTS),
        _ if is_fact_association_pointer(pointer) => Some(FACTS),
        _ if parent == "requires" && pointer.contains("/routes/") => Some(ROUTE_REQUIREMENTS),
        _ if parent == "requires" && pointer.contains("/clues/") => Some(CLUES),
        _ if parent == "requires" && pointer.contains("/deductions/") => Some(DEDUCTIONS),
        _ if parent == "entry_settings" || parent == "exit_settings" => Some(SETTINGS),
        _ if field == "source" && pointer.contains("/characters/") => Some(CONTENT_SOURCES),
        _ if field == "target" && pointer.contains("/clues/") => Some(CONTENT_SOURCES),
        _ if parent == "targets" && pointer.contains("/clues/") => Some(CONTENT_SOURCES),
        _ if field == "command" && pointer.contains("/triggers/") => Some(COMMANDS),
        _ if matches!(parent, "any_of" | "all_of") && pointer.contains("/triggers/") => {
            Some(TRIGGER_GATES)
        }
        _ => None,
    }
}

fn is_entity_visibility_requirement_pointer(pointer: &str) -> bool {
    let parts = pointer
        .trim_start_matches('/')
        .split('/')
        .collect::<Vec<_>>();
    matches!(
        parts.as_slice(),
        ["entities", entity_index, "visibility", "requires"]
            if entity_index.parse::<usize>().is_ok()
    ) || matches!(
        parts.as_slice(),
        ["entities", entity_index, "visibility", "requires", requirement_index]
            if entity_index.parse::<usize>().is_ok()
                && requirement_index.parse::<usize>().is_ok()
    )
}

fn is_character_testimony_list_pointer(pointer: &str, field: &str) -> bool {
    let parts = pointer
        .trim_start_matches('/')
        .split('/')
        .collect::<Vec<_>>();
    matches!(
        parts.as_slice(),
        ["characters", character_index, "testimony", testimony_index, list_field, item_index]
            if character_index.parse::<usize>().is_ok()
                && testimony_index.parse::<usize>().is_ok()
                && *list_field == field
                && item_index.parse::<usize>().is_ok()
    )
}

fn is_fact_association_pointer(pointer: &str) -> bool {
    let Some((prefix, suffix)) = pointer.split_once("/facts/") else {
        return false;
    };
    if ![
        "/settings/",
        "/routes/",
        "/characters/",
        "/entities/",
        "/events/",
        "/commands/",
        "/triggers/",
    ]
    .iter()
    .any(|section| prefix.starts_with(section))
    {
        return false;
    }
    let parts: Vec<_> = suffix.split('/').collect();
    matches!(parts.as_slice(), [index] if index.parse::<usize>().is_ok())
        || matches!(parts.as_slice(), [level, index] if level.parse::<usize>().is_err() && index.parse::<usize>().is_ok())
}

fn valid_fact_association(value: &Value) -> bool {
    match value {
        Value::Sequence(_) => is_nonempty_string_sequence(value, false),
        Value::Mapping(levels) => {
            !levels.is_empty()
                && levels.iter().all(|(level, facts)| {
                    level.as_str().is_some_and(|level| !level.trim().is_empty())
                        && is_nonempty_string_sequence(facts, false)
                })
        }
        _ => false,
    }
}

fn collect_fact_association_ids(value: &Value, result: &mut BTreeSet<String>) {
    match value {
        Value::String(id) => {
            result.insert(id.to_string());
        }
        Value::Sequence(values) => {
            for value in values {
                collect_fact_association_ids(value, result);
            }
        }
        Value::Mapping(values) => {
            for value in values.values() {
                collect_fact_association_ids(value, result);
            }
        }
        Value::Tagged(tagged) => collect_fact_association_ids(&tagged.value, result),
        _ => {}
    }
}

fn collect_duplicate_id_lists(
    value: &Value,
    pointer: &str,
    duplicates: &mut Vec<(String, String)>,
) {
    match value {
        Value::Sequence(values) => {
            let ids: Vec<_> = values.iter().filter_map(Value::as_str).collect();
            let reference_list = is_reference_pointer(&format!("{pointer}/0"));
            if ids.len() == values.len()
                && (reference_list || ids.iter().all(|id| looks_like_id(id)))
            {
                let mut seen = HashSet::new();
                for (index, id) in ids.into_iter().enumerate() {
                    if !seen.insert(id) {
                        duplicates.push((format!("{pointer}/{index}"), id.to_string()));
                    }
                }
            }
            for (index, value) in values.iter().enumerate() {
                collect_duplicate_id_lists(value, &format!("{pointer}/{index}"), duplicates);
            }
        }
        Value::Mapping(values) => {
            for (key, value) in values {
                if let Some(key) = key.as_str() {
                    collect_duplicate_id_lists(
                        value,
                        &format!("{pointer}/{}", escape_pointer(key)),
                        duplicates,
                    );
                }
            }
        }
        Value::Tagged(tagged) => collect_duplicate_id_lists(&tagged.value, pointer, duplicates),
        _ => {}
    }
}

fn dependency_graph(items: &[Item], field: &str) -> BTreeMap<String, Vec<String>> {
    items
        .iter()
        .map(|item| (item.id.clone(), string_list_field(&item.mapping, field)))
        .collect()
}

fn fact_dependency_graph(items: &[Item]) -> BTreeMap<String, Vec<String>> {
    items
        .iter()
        .map(|item| {
            let dependencies = string_or_list_field(&item.mapping, "requires")
                .into_iter()
                .filter(|id| id_prefix(id) == Some("fact"))
                .collect();
            (item.id.clone(), dependencies)
        })
        .collect()
}

fn deduction_dependency_graph(items: &[Item]) -> BTreeMap<String, Vec<String>> {
    items
        .iter()
        .map(|item| {
            let mut dependencies: BTreeSet<String> = string_list_field(&item.mapping, "requires")
                .into_iter()
                .collect();
            dependencies.extend(
                string_list_field(&item.mapping, "inputs")
                    .into_iter()
                    .filter(|id| id_prefix(id) == Some("deduction")),
            );
            (item.id.clone(), dependencies.into_iter().collect())
        })
        .collect()
}

fn find_cycles(graph: &BTreeMap<String, Vec<String>>) -> Vec<Vec<String>> {
    fn visit(
        node: &str,
        graph: &BTreeMap<String, Vec<String>>,
        state: &mut HashMap<String, u8>,
        stack: &mut Vec<String>,
        cycles: &mut BTreeSet<Vec<String>>,
    ) {
        state.insert(node.to_string(), 1);
        stack.push(node.to_string());
        for next in graph.get(node).into_iter().flatten() {
            match state.get(next).copied().unwrap_or(0) {
                0 if graph.contains_key(next) => visit(next, graph, state, stack, cycles),
                1 => {
                    if let Some(start) = stack.iter().position(|value| value == next) {
                        let mut cycle = stack[start..].to_vec();
                        canonicalize_cycle(&mut cycle);
                        cycle.push(cycle[0].clone());
                        cycles.insert(cycle);
                    }
                }
                _ => {}
            }
        }
        stack.pop();
        state.insert(node.to_string(), 2);
    }

    let mut state = HashMap::new();
    let mut stack = Vec::new();
    let mut cycles = BTreeSet::new();
    for node in graph.keys() {
        if state.get(node).copied().unwrap_or(0) == 0 {
            visit(node, graph, &mut state, &mut stack, &mut cycles);
        }
    }
    cycles.into_iter().collect()
}

fn canonicalize_cycle(cycle: &mut [String]) {
    if let Some((index, _)) = cycle.iter().enumerate().min_by_key(|(_, value)| *value) {
        cycle.rotate_left(index);
    }
}

fn reachable(graph: &BTreeMap<String, Vec<String>>, starts: &[String]) -> HashSet<String> {
    let mut visited = HashSet::new();
    let mut queue: VecDeque<_> = starts.iter().cloned().collect();
    while let Some(node) = queue.pop_front() {
        if !visited.insert(node.clone()) {
            continue;
        }
        if let Some(next) = graph.get(&node) {
            queue.extend(next.iter().cloned());
        }
    }
    visited
}

fn command_parameter_types(command: &Item) -> Vec<Option<CommandParameterType>> {
    command
        .mapping
        .get(Value::String("parameters".to_string()))
        .and_then(Value::as_sequence)
        .into_iter()
        .flatten()
        .map(|parameter| {
            parameter
                .as_mapping()
                .and_then(|parameter| string_field(parameter, "type"))
                .and_then(CommandParameterType::parse)
        })
        .collect()
}

fn string_field<'a>(mapping: &'a Mapping, field: &str) -> Option<&'a str> {
    mapping
        .get(Value::String(field.to_string()))
        .and_then(Value::as_str)
}

fn nested_string_field<'a>(mapping: &'a Mapping, fields: &[&str]) -> Option<&'a str> {
    let mut value = mapping.get(Value::String(fields.first()?.to_string()))?;
    for field in &fields[1..] {
        value = value
            .as_mapping()?
            .get(Value::String((*field).to_string()))?;
    }
    value.as_str()
}

fn string_list_field(mapping: &Mapping, field: &str) -> Vec<String> {
    mapping
        .get(Value::String(field.to_string()))
        .and_then(Value::as_sequence)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn string_or_list_field(mapping: &Mapping, field: &str) -> Vec<String> {
    match mapping.get(Value::String(field.to_string())) {
        Some(Value::String(value)) => vec![value.clone()],
        Some(Value::Sequence(values)) => values
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

fn is_string_sequence(value: &Value) -> bool {
    value
        .as_sequence()
        .is_some_and(|values| values.iter().all(Value::is_string))
}

fn is_nonempty_string_or_sequence(value: &Value) -> bool {
    match value {
        Value::String(value) => !value.trim().is_empty(),
        Value::Sequence(_) => is_nonempty_string_sequence(value, false),
        _ => false,
    }
}

fn is_nonempty_string_sequence(value: &Value, allow_empty_sequence: bool) -> bool {
    value.as_sequence().is_some_and(|values| {
        (allow_empty_sequence || !values.is_empty())
            && values
                .iter()
                .all(|value| value.as_str().is_some_and(|text| !text.trim().is_empty()))
    })
}

fn bool_field(mapping: &Mapping, field: &str) -> Option<bool> {
    mapping
        .get(Value::String(field.to_string()))
        .and_then(Value::as_bool)
}

fn integer_field(mapping: &Mapping, field: &str) -> Option<i64> {
    mapping
        .get(Value::String(field.to_string()))
        .and_then(Value::as_i64)
}

fn id_prefix(id: &str) -> Option<&str> {
    let (prefix, _) = id.split_once('.')?;
    Some(prefix)
}

fn looks_like_id(value: &str) -> bool {
    let Some((prefix, suffix)) = value.split_once('.') else {
        return false;
    };
    !prefix.is_empty()
        && !suffix.is_empty()
        && prefix
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
        && suffix
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn valid_id(value: &str) -> bool {
    let Some((prefix, suffix)) = value.split_once('.') else {
        return false;
    };
    looks_like_id(value)
        && !prefix.starts_with('_')
        && !prefix.ends_with('_')
        && !suffix.starts_with('_')
        && !suffix.ends_with('_')
        && !value.contains("__")
}

fn valid_time(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 5
        || bytes[2] != b':'
        || !bytes[0..2].iter().all(u8::is_ascii_digit)
        || !bytes[3..5].iter().all(u8::is_ascii_digit)
    {
        return false;
    }
    let hour = (bytes[0] - b'0') * 10 + bytes[1] - b'0';
    let minute = (bytes[3] - b'0') * 10 + bytes[4] - b'0';
    hour < 24 && minute < 60
}

fn valid_delay(value: &str) -> bool {
    let value = value.trim();
    if let Some(number) = value.strip_suffix("turns") {
        return number.trim().parse::<u32>().is_ok_and(|number| number > 0);
    }
    if let Some(number) = value.strip_suffix('m') {
        return number.trim().parse::<u32>().is_ok_and(|number| number > 0);
    }
    if let Some(number) = value.strip_suffix('h') {
        return number
            .trim()
            .parse::<u32>()
            .is_ok_and(|number| number > 0 && number.checked_mul(60).is_some());
    }
    false
}

fn escape_pointer(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

fn locate_scalar(source: &str, scalar: &str) -> Option<SourceRange> {
    source.lines().enumerate().find_map(|(line_index, line)| {
        line.find(scalar).map(|column_index| SourceRange {
            start: Position {
                line: line_index + 1,
                column: column_index + 1,
            },
            end: Position {
                line: line_index + 1,
                column: column_index + scalar.len() + 1,
            },
        })
    })
}

fn locate_id(source: &str, id: &str) -> Option<SourceRange> {
    source.lines().enumerate().find_map(|(line_index, line)| {
        let key_index = line.find("id:")?;
        let prefix = line[..key_index].trim();
        if !prefix.is_empty() && prefix != "-" {
            return None;
        }
        let untrimmed = &line[key_index + 3..];
        let rest = untrimmed.trim_start();
        let spacing = untrimmed.len() - rest.len();
        let quote_bytes = usize::from(rest.starts_with(['\'', '"']));
        if !rest[quote_bytes..].starts_with(id) {
            return None;
        }
        let column_index = key_index + 3 + spacing + quote_bytes;
        Some(SourceRange {
            start: Position {
                line: line_index + 1,
                column: column_index + 1,
            },
            end: Position {
                line: line_index + 1,
                column: column_index + id.len() + 1,
            },
        })
    })
}

fn point_range(position: Position) -> SourceRange {
    SourceRange {
        start: position,
        end: position,
    }
}

fn check_yaml_complexity(value: &Value, depth: usize, nodes: &mut usize) -> Result<(), String> {
    if depth > MAX_YAML_DEPTH {
        return Err(format!(
            "YAML nesting exceeds the limit of {MAX_YAML_DEPTH}"
        ));
    }
    *nodes = nodes.saturating_add(1);
    if *nodes > MAX_YAML_NODES {
        return Err(format!(
            "YAML node count exceeds the limit of {MAX_YAML_NODES}"
        ));
    }
    match value {
        Value::Sequence(values) => {
            for value in values {
                check_yaml_complexity(value, depth + 1, nodes)?;
            }
        }
        Value::Mapping(values) => {
            for (key, value) in values {
                check_yaml_complexity(key, depth + 1, nodes)?;
                check_yaml_complexity(value, depth + 1, nodes)?;
            }
        }
        Value::Tagged(value) => check_yaml_complexity(&value.value, depth + 1, nodes)?,
        _ => {}
    }
    Ok(())
}

/// Conservatively reject unquoted YAML anchor/alias tokens. Quoted `&` and
/// `*` characters and comments remain valid scalar content.
fn contains_yaml_anchor_or_alias(source: &str) -> bool {
    let mut block_scalar_indent = None;
    for line in source.lines() {
        let bytes = line.as_bytes();
        let indentation = bytes
            .iter()
            .take_while(|byte| byte.is_ascii_whitespace())
            .count();
        if let Some(parent_indent) = block_scalar_indent {
            if line.trim().is_empty() || indentation > parent_indent {
                continue;
            }
            block_scalar_indent = None;
        }
        let mut single_quoted = false;
        let mut double_quoted = false;
        let mut expects_node = true;
        let mut index = 0;
        while index < bytes.len() {
            if expects_node
                && (bytes[index..].starts_with(b"---") || bytes[index..].starts_with(b"..."))
                && bytes
                    .get(index + 3)
                    .map_or(true, |byte| byte.is_ascii_whitespace())
            {
                index += 3;
                continue;
            }
            match bytes[index] {
                b'\'' if !double_quoted => {
                    if single_quoted && bytes.get(index + 1) == Some(&b'\'') {
                        index += 2;
                        continue;
                    }
                    single_quoted = !single_quoted;
                }
                b'"' if !single_quoted => {
                    let mut backslashes = 0;
                    let mut cursor = index;
                    while cursor > 0 && bytes[cursor - 1] == b'\\' {
                        backslashes += 1;
                        cursor -= 1;
                    }
                    if backslashes % 2 == 0 {
                        double_quoted = !double_quoted;
                    }
                }
                b'#' if !single_quoted && !double_quoted => break,
                b'&' | b'*' if !single_quoted && !double_quoted && expects_node => {
                    let next_is_name = bytes.get(index + 1).is_some_and(|byte| {
                        !byte.is_ascii_whitespace() && !b",]}:#".contains(byte)
                    });
                    if next_is_name {
                        return true;
                    }
                }
                b':' if !single_quoted
                    && !double_quoted
                    && bytes
                        .get(index + 1)
                        .map_or(true, |byte| byte.is_ascii_whitespace()) =>
                {
                    expects_node = true;
                }
                b',' | b'[' | b'{' if !single_quoted && !double_quoted => {
                    expects_node = true;
                }
                b'-' if !single_quoted
                    && !double_quoted
                    && expects_node
                    && bytes
                        .get(index + 1)
                        .is_some_and(|byte| byte.is_ascii_whitespace()) => {}
                b'?' if !single_quoted
                    && !double_quoted
                    && expects_node
                    && bytes
                        .get(index + 1)
                        .is_some_and(|byte| byte.is_ascii_whitespace()) => {}
                b'|' | b'>' if !single_quoted && !double_quoted && expects_node => {
                    block_scalar_indent = Some(indentation);
                    break;
                }
                byte if !single_quoted
                    && !double_quoted
                    && !byte.is_ascii_whitespace()
                    && !matches!(byte, b']' | b'}') =>
                {
                    expects_node = false;
                }
                _ => {}
            }
            index += 1;
        }
    }
    false
}

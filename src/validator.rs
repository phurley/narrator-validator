use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

use semver::Version;
use serde::Deserialize;
use serde_yaml::{Mapping, Value};

use crate::{
    parse_reference_text, reference_kind, resolve_ruleset, Diagnostic, DisclosureClass, Position,
    ReferenceProvenance, ReferenceTextSegment, RelatedLocation, ResolvedReferenceText,
    RulesetReference, Severity, SourceFile, SourceRange, ValidationReport, CONSUMER_FIELDS,
    MAX_SOLUTION_ANSWER_CARDS, MAX_SOLUTION_QUESTIONS, MIN_SOLUTION_ANSWER_CARDS,
    MIN_SOLUTION_QUESTIONS, REFERENCE_TEXT_FEATURE, STANDARD_MYSTERY_RULESET_ID,
    STANDARD_MYSTERY_RULESET_VERSION_2, STANDARD_MYSTERY_RULESET_VERSION_3, STORY_FORMAT_VERSION,
    SUPPORTED_FEATURES, VALIDATOR_VERSION,
};

const MAX_REPOSITORY_FILES: usize = 512;
const MAX_REPOSITORY_BYTES: usize = 1024 * 1024;
const MAX_FILE_BYTES: usize = 256 * 1024;
const MAX_YAML_DEPTH: usize = 64;
const MAX_YAML_NODES: usize = 100_000;
// tagStandard41h12 defines 2,115 codes, numbered from zero.
const TAG_STANDARD_41H12_MAX_ID: i64 = 2_114;

const REQUIRED_SECTIONS: &[&str] = &[
    "case",
    "settings",
    "routes",
    "characters",
    "entities",
    "events",
    "deductions",
    "flags",
    "cards",
];
const SINGLE_SECTIONS: &[&str] = &[
    "solution",
    "end_states",
    "win_states",
    "clues",
    "commands",
    "triggers",
    "cards",
];
const CANONICAL_SECTION_FILES: &[(&str, &str)] = &[
    ("case", "case.yaml"),
    ("solution", "case.yaml"),
    ("end_states", "end_states.yaml"),
    ("win_states", "win_states.yaml"),
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
    ("cards", "deck.yaml"),
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
    EndState,
    WinState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum CommandParameterType {
    Character,
    Entity,
    Setting,
    Deduction,
    Event,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum CandidateSource {
    All,
    CurrentLocation,
    Inventory,
    Reachable,
    Known,
    Established,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CommandParameterShape {
    types: Vec<CommandParameterType>,
    min: usize,
    max: usize,
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

impl CandidateSource {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "all" => Some(Self::All),
            "current_location" => Some(Self::CurrentLocation),
            "inventory" => Some(Self::Inventory),
            "reachable" => Some(Self::Reachable),
            "known" => Some(Self::Known),
            "established" => Some(Self::Established),
            _ => None,
        }
    }

    fn produced_types(self) -> &'static [CommandParameterType] {
        use CommandParameterType::{Character, Deduction, Entity, Event, Setting};
        match self {
            Self::All | Self::Known => &[Character, Entity, Setting, Deduction, Event],
            Self::CurrentLocation => &[Setting, Entity, Character],
            Self::Inventory => &[Entity],
            Self::Reachable => &[Setting],
            Self::Established => &[Deduction],
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
            Self::EndState => "end",
            Self::WinState => "win",
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
    kind: Kind,
    id: String,
    path: String,
    source: String,
    pointer: String,
    mapping: Mapping,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EndStateCondition {
    requirements: BTreeSet<String>,
    minimum_points: u64,
    at_or_after_minutes: Option<u16>,
}

impl EndStateCondition {
    fn from_item(item: &Item) -> Option<Self> {
        let requirements_value = item.mapping.get(Value::String("requires".to_string()));
        if requirements_value.is_some_and(|value| !is_string_sequence(value)) {
            return None;
        }
        let minimum_points = match item
            .mapping
            .get(Value::String("minimum_points".to_string()))
        {
            Some(value) => value.as_u64()?,
            None => 0,
        };
        let at_or_after_minutes = match string_field(&item.mapping, "at_or_after") {
            Some(time) if valid_time(time) => Some(time_to_minutes(time)),
            Some(_) => return None,
            None => None,
        };
        Some(Self {
            requirements: string_list_field(&item.mapping, "requires")
                .into_iter()
                .collect(),
            minimum_points,
            at_or_after_minutes,
        })
    }

    /// Whether this earlier condition is necessarily satisfied whenever the
    /// later condition is satisfied. Conditions are monotonic conjunctions.
    fn is_implied_by(&self, later: &Self) -> bool {
        self.requirements.is_subset(&later.requirements)
            && self.minimum_points <= later.minimum_points
            && match (self.at_or_after_minutes, later.at_or_after_minutes) {
                (None, _) => true,
                (Some(_), None) => false,
                (Some(earlier), Some(later)) => earlier <= later,
            }
    }
}

struct GraphInputs<'a> {
    settings: &'a [Item],
    entities: &'a [Item],
    clues: &'a [Item],
    facts: &'a [Item],
    deductions: &'a [Item],
    triggers: &'a [Item],
    fact_claims_enabled: bool,
}

fn item_kind(item: &Item) -> Option<&'static str> {
    Some(item.kind.name())
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
    format_version: Option<Version>,
    format_compatible: bool,
    ruleset: Option<RulesetReference>,
    features: Vec<String>,
    supported_features: BTreeSet<String>,
    feature_compatible: bool,
    reference_text: Vec<ResolvedReferenceText>,
}

/// Validate a complete, immutable repository snapshot.
pub fn validate(files: &[SourceFile]) -> ValidationReport {
    validate_with_supported_features(
        files,
        &SUPPORTED_FEATURES
            .iter()
            .map(|feature| (*feature).to_string())
            .collect::<Vec<_>>(),
    )
}

/// Validate using the exact feature set implemented by the calling consumer.
/// Declared capabilities not present in `supported_features` stop validation
/// before story prose is interpreted.
pub fn validate_with_supported_features(
    files: &[SourceFile],
    supported_features: &[String],
) -> ValidationReport {
    let mut validator = Validator {
        files,
        parsed: Vec::new(),
        diagnostics: Vec::new(),
        definitions: BTreeMap::new(),
        sections: BTreeMap::new(),
        format_version: None,
        format_compatible: true,
        ruleset: None,
        features: Vec::new(),
        supported_features: supported_features.iter().cloned().collect(),
        feature_compatible: true,
        reference_text: Vec::new(),
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
        format_version: validator.format_version.map(|version| version.to_string()),
        valid,
        diagnostics: validator.diagnostics,
        features: validator.features,
        reference_text: validator.reference_text,
    }
}

impl<'a> Validator<'a> {
    fn is_format_3(&self) -> bool {
        self.format_version
            .as_ref()
            .is_some_and(|version| version.major == 3)
    }

    fn is_format_3_1_or_later(&self) -> bool {
        self.format_version
            .as_ref()
            .is_some_and(|version| version.major == 3 && version.minor >= 1)
    }

    fn is_format_3_2_or_later(&self) -> bool {
        self.format_version
            .as_ref()
            .is_some_and(|version| version.major == 3 && version.minor >= 2)
    }

    fn is_format_3_3_or_later(&self) -> bool {
        self.format_version
            .as_ref()
            .is_some_and(|version| version.major == 3 && version.minor >= 3)
    }

    fn is_format_3_4_or_later(&self) -> bool {
        self.format_version
            .as_ref()
            .is_some_and(|version| version.major == 3 && version.minor >= 4)
    }

    fn uses_question_solution_ruleset(&self) -> bool {
        self.ruleset.as_ref().is_some_and(|ruleset| {
            ruleset.id == STANDARD_MYSTERY_RULESET_ID
                && ruleset.version == STANDARD_MYSTERY_RULESET_VERSION_3
        })
    }

    fn run(&mut self) {
        if !self.validate_repository_bounds() {
            return;
        }
        self.parse_files();
        self.index_sections();

        let cases = self.items("case", Kind::Case, false);
        self.validate_case(&cases);
        if !self.format_compatible || !self.feature_compatible {
            return;
        }
        self.validate_section_filenames();
        self.validate_sections();
        let settings = self.items("settings", Kind::Setting, true);
        let routes = self.items("routes", Kind::Route, true);
        let characters = self.items("characters", Kind::Character, true);
        let entities = self.items("entities", Kind::Entity, true);
        let events = self.items("events", Kind::Event, true);
        let clues = self.items("clues", Kind::Clue, true);
        let deductions = self.items("deductions", Kind::Deduction, true);
        let flags = self.items("flags", Kind::Flag, true);
        let local_commands = self.items("commands", Kind::Command, true);
        self.validate_command_migration(&local_commands);
        let commands = self.merge_ruleset_commands(local_commands);
        let triggers = self.items("triggers", Kind::Trigger, true);
        let win_states = self.items("win_states", Kind::WinState, true);
        let end_states = self.items_with_prefixes(
            "end_states",
            Kind::EndState,
            true,
            &[Kind::EndState.prefix(), Kind::WinState.prefix()],
        );
        let fact_claims_enabled = self.is_format_3();
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
        let testimonies = self.nested_testimonies(&characters);
        let facts_enabled = !facts.is_empty();

        if self.is_format_3() {
            self.validate_strict_field_contract(
                &cases,
                &settings,
                &routes,
                &characters,
                &entities,
                &events,
                &facts,
                &deductions,
                &flags,
                &commands,
                &triggers,
            );
        }

        if self.is_format_3_2_or_later() {
            self.validate_reference_text(
                &cases,
                &settings,
                &characters,
                &entities,
                &events,
                &facts,
                &deductions,
                &flags,
                &commands,
                &triggers,
                &testimonies,
                &win_states,
                &end_states,
            );
        }

        self.validate_terminal_configuration(&end_states, &win_states);
        self.validate_solution();
        self.validate_win_states(&win_states);
        self.validate_end_states(&end_states);
        self.validate_references();
        self.validate_duplicate_lists();
        self.validate_deck();
        self.validate_legacy_inline_tag_ids(&settings, &characters, &entities, &commands);
        self.validate_event_values(&events);
        self.validate_route_values(&routes);
        self.validate_character_values(&characters, facts_enabled);
        self.validate_entity_values(&entities);
        if !fact_claims_enabled {
            self.validate_clue_values(&clues, facts_enabled);
        }
        self.validate_fact_values(&facts, &commands, fact_claims_enabled);
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
        self.validate_point_awards(&[
            settings.as_slice(),
            entities.as_slice(),
            deductions.as_slice(),
            commands.as_slice(),
        ]);
        self.validate_disallowed_point_owners(&[
            routes.as_slice(),
            characters.as_slice(),
            events.as_slice(),
            triggers.as_slice(),
        ]);
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
        self.validate_graphs(GraphInputs {
            settings: &settings,
            entities: &entities,
            clues: &clues,
            facts: &facts,
            deductions: &deductions,
            triggers: &triggers,
            fact_claims_enabled,
        });
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
        if !self.is_format_3() {
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
        if self.is_format_3() {
            if let Some(locations) = self.sections.get("clues").cloned() {
                for (path, pointer) in locations {
                    self.push(
                        Severity::Error,
                        "format.clues_removed",
                        "format 3 removes clues; express player knowledge in `facts`".to_string(),
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
        self.items_with_prefixes(section, kind, sequence, &[kind.prefix()])
    }

    fn items_with_prefixes(
        &mut self,
        section: &str,
        kind: Kind,
        sequence: bool,
        allowed_prefixes: &[&str],
    ) -> Vec<Item> {
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
                } else if !id_prefix(&id).is_some_and(|prefix| allowed_prefixes.contains(&prefix)) {
                    self.push(
                        Severity::Error,
                        "id.wrong_prefix",
                        format!(
                            "{} ID `{id}` must start with {}",
                            kind.name(),
                            allowed_prefixes
                                .iter()
                                .map(|prefix| format!("`{prefix}.`"))
                                .collect::<Vec<_>>()
                                .join(" or ")
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
                    kind,
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
                        kind: Kind::Fact,
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

    fn nested_testimonies(&mut self, characters: &[Item]) -> Vec<Item> {
        let mut result = Vec::new();
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
                    self.definitions.insert(id.clone(), definition);
                }
                result.push(Item {
                    kind: Kind::Testimony,
                    id,
                    path: character.path.clone(),
                    source: character.source.clone(),
                    pointer,
                    mapping,
                });
            }
        }
        result
    }

    fn validate_case(&mut self, cases: &[Item]) {
        if cases.len() != 1 {
            return;
        }
        self.format_compatible = false;
        let case = &cases[0];
        let version_pointer = Some(format!("{}/format_version", case.pointer));
        match case
            .mapping
            .get(Value::String("format_version".to_string()))
        {
            Some(Value::String(raw_version)) => match Version::parse(raw_version) {
                Ok(version) => {
                    self.format_compatible = version.major == 1 || version.major == 3;
                    self.format_version = Some(version.clone());
                    if !self.format_compatible {
                        let message = if version.major == 2 {
                            format!(
                                "This story uses pre-migration format {version}. Please migrate it to story format {STORY_FORMAT_VERSION} before opening it. Follow the focused migration guide at https://github.com/phurley/narrator-validator/blob/v1.0.0/MIGRATION.md."
                            )
                        } else if version
                            < Version::parse("1.0.0").expect("minimum story format is valid")
                        {
                            format!(
                                "This story uses format {version}, which is too old for this version of Narrator. Please migrate it to story format {STORY_FORMAT_VERSION} before opening it."
                            )
                        } else {
                            format!(
                                "This story uses format {version}, which is newer than this version of Narrator supports. Please update Narrator before opening it."
                            )
                        };
                        self.push(
                            Severity::Error,
                            "format.incompatible_version",
                            message,
                            &case.path,
                            version_pointer.clone(),
                            locate_scalar(&case.source, raw_version),
                            Some(case.id.clone()),
                        );
                    }
                }
                Err(_) => self.push(
                    Severity::Error,
                    "format.version_invalid",
                    format!(
                        "`case.format_version` must be a valid quoted semantic version such as \"{STORY_FORMAT_VERSION}\""
                    ),
                    &case.path,
                    version_pointer.clone(),
                    locate_scalar(&case.source, raw_version),
                    Some(case.id.clone()),
                ),
            },
            Some(Value::Number(number)) => self.push(
                Severity::Error,
                "format.incompatible_version",
                format!(
                    "This story uses legacy format version {number}. Narrator now uses semantic story-format versions and cannot safely open it. Please migrate the story to format {STORY_FORMAT_VERSION} first."
                ),
                &case.path,
                version_pointer.clone(),
                locate_scalar(&case.source, &number.to_string()),
                Some(case.id.clone()),
            ),
            Some(_) => self.push(
                Severity::Error,
                "format.version_type",
                format!(
                    "`case.format_version` must be a quoted semantic version such as \"{STORY_FORMAT_VERSION}\""
                ),
                &case.path,
                version_pointer.clone(),
                None,
                Some(case.id.clone()),
            ),
            None => self.push(
                Severity::Error,
                "format.version_missing",
                format!(
                    "This story does not declare a semantic format version, so Narrator cannot safely open it. Please migrate it and add `case.format_version: \"{STORY_FORMAT_VERSION}\"`."
                ),
                &case.path,
                version_pointer,
                None,
                Some(case.id.clone()),
            ),
        }
        if !self.format_compatible {
            return;
        }
        self.validate_features(case);
        if !self.feature_compatible {
            return;
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
        if self.is_format_3() {
            self.validate_player_limits(case);
            self.validate_ruleset_reference(case);
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
        if self.is_format_3() {
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
                    "format 3 games require `case.initial_time` so runtime time effects are deterministic"
                        .to_string(),
                    &case.path,
                    Some(format!("{}/initial_time", case.pointer)),
                    None,
                    Some(case.id.clone()),
                ),
            }
        }
    }

    fn validate_features(&mut self, case: &Item) {
        let Some(value) = case.mapping.get(Value::String("features".to_string())) else {
            return;
        };
        let pointer = format!("{}/features", case.pointer);
        if !self.is_format_3_2_or_later() {
            self.feature_compatible = false;
            self.push(
                Severity::Error,
                "feature.format_incompatible",
                "`case.features` requires story format 3.2.0 or later".to_string(),
                &case.path,
                Some(pointer),
                None,
                Some(case.id.clone()),
            );
            return;
        }
        let Some(entries) = value.as_sequence() else {
            self.feature_compatible = false;
            self.push(
                Severity::Error,
                "feature.list_type",
                "`case.features` must be an ordered sequence of unique feature names".to_string(),
                &case.path,
                Some(pointer),
                None,
                Some(case.id.clone()),
            );
            return;
        };
        let mut seen = HashSet::new();
        for (index, entry) in entries.iter().enumerate() {
            let entry_pointer = format!("{pointer}/{index}");
            let Some(feature) = entry.as_str().filter(|feature| !feature.trim().is_empty()) else {
                self.feature_compatible = false;
                self.push(
                    Severity::Error,
                    "feature.name_type",
                    "feature names must be non-empty strings".to_string(),
                    &case.path,
                    Some(entry_pointer),
                    None,
                    Some(case.id.clone()),
                );
                continue;
            };
            self.features.push(feature.to_string());
            if !seen.insert(feature) {
                self.feature_compatible = false;
                self.push(
                    Severity::Error,
                    "feature.duplicate",
                    format!("feature `{feature}` is declared more than once"),
                    &case.path,
                    Some(entry_pointer),
                    locate_scalar(&case.source, feature),
                    Some(case.id.clone()),
                );
                continue;
            }
            if !SUPPORTED_FEATURES.contains(&feature) {
                self.feature_compatible = false;
                self.push(
                    Severity::Error,
                    "feature.unknown",
                    format!(
                        "feature `{feature}` is not recognized by validator {VALIDATOR_VERSION}"
                    ),
                    &case.path,
                    Some(entry_pointer),
                    locate_scalar(&case.source, feature),
                    Some(case.id.clone()),
                );
            } else if !self.supported_features.contains(feature) {
                self.feature_compatible = false;
                self.push(
                    Severity::Error,
                    "feature.consumer_unsupported",
                    format!("this consumer does not advertise support for feature `{feature}`"),
                    &case.path,
                    Some(entry_pointer),
                    locate_scalar(&case.source, feature),
                    Some(case.id.clone()),
                );
            }
        }
    }

    fn validate_ruleset_reference(&mut self, case: &Item) {
        let Some(value) = case.mapping.get(Value::String("ruleset".to_string())) else {
            return;
        };
        let pointer = format!("{}/ruleset", case.pointer);
        let reference = match serde_yaml::from_value::<RulesetReference>(value.clone()) {
            Ok(reference) => reference,
            Err(error) => {
                self.push(
                    Severity::Error,
                    "ruleset.reference_invalid",
                    format!(
                        "`case.ruleset` must contain exactly string `id` and `version` fields: {error}"
                    ),
                    &case.path,
                    Some(pointer),
                    None,
                    Some(case.id.clone()),
                );
                return;
            }
        };
        if reference.id == STANDARD_MYSTERY_RULESET_ID
            && reference.version == STANDARD_MYSTERY_RULESET_VERSION_2
            && !self.is_format_3_1_or_later()
        {
            self.push(
                Severity::Error,
                "ruleset.format_incompatible",
                "ruleset.standard_mystery@2.0.0 declares format-3.1 candidate semantics; set `case.format_version` to \"3.1.0\" or select ruleset version \"1.0.0\""
                    .to_string(),
                &case.path,
                Some(format!("{pointer}/version")),
                None,
                Some(case.id.clone()),
            );
            return;
        }
        if reference.id == STANDARD_MYSTERY_RULESET_ID
            && reference.version == STANDARD_MYSTERY_RULESET_VERSION_3
            && !self.is_format_3_3_or_later()
        {
            self.push(
                Severity::Error,
                "ruleset.format_incompatible",
                "ruleset.standard_mystery@3.0.0 declares the Format 3.3 authored-question Solve contract; set `case.format_version` to \"3.3.0\" or select an earlier ruleset version"
                    .to_string(),
                &case.path,
                Some(format!("{pointer}/version")),
                None,
                Some(case.id.clone()),
            );
            return;
        }
        match resolve_ruleset(&reference) {
            Ok(_) => self.ruleset = Some(reference),
            Err(error) => self.push(
                Severity::Error,
                "ruleset.unsupported",
                error.to_string(),
                &case.path,
                Some(pointer),
                None,
                Some(case.id.clone()),
            ),
        }
    }

    fn merge_ruleset_commands(&mut self, local_commands: Vec<Item>) -> Vec<Item> {
        let Some(reference) = self.ruleset.as_ref() else {
            return local_commands;
        };
        let resolved = resolve_ruleset(reference).expect("validated ruleset remains resolvable");
        let document: Value =
            serde_yaml::from_str(resolved.commands_yaml).expect("built-in ruleset YAML is valid");
        let commands = document["commands"]
            .as_sequence()
            .expect("built-in ruleset commands are a sequence");
        let local_by_id = local_commands
            .iter()
            .map(|command| (command.id.as_str(), command))
            .collect::<BTreeMap<_, _>>();
        let source_name = format!("{}@{}", reference.id, reference.version);
        let mut merged = Vec::with_capacity(commands.len() + local_commands.len());

        for (index, value) in commands.iter().enumerate() {
            let mapping = value
                .as_mapping()
                .expect("built-in ruleset commands are mappings")
                .clone();
            let id = string_field(&mapping, "id")
                .expect("built-in ruleset command has an id")
                .to_string();
            if let Some(local) = local_by_id.get(id.as_str()) {
                self.push(
                    Severity::Error,
                    "ruleset.command_conflict",
                    format!(
                        "local command `{id}` conflicts with {source_name}; ruleset overrides are deferred, so remove the copied command or choose a different command ID"
                    ),
                    &local.path,
                    Some(format!("{}/id", local.pointer)),
                    locate_id(&local.source, &local.id),
                    Some(id),
                );
                continue;
            }
            let pointer = format!("/commands/{index}");
            self.definitions.insert(
                id.clone(),
                Definition {
                    kind: Kind::Command,
                    path: source_name.clone(),
                    pointer: pointer.clone(),
                    range: None,
                },
            );
            merged.push(Item {
                kind: Kind::Command,
                id,
                path: source_name.clone(),
                source: resolved.commands_yaml.to_string(),
                pointer,
                mapping,
            });
        }
        merged.extend(local_commands);
        merged
    }

    fn validate_command_migration(&mut self, commands: &[Item]) {
        if !self.is_format_3() {
            return;
        }
        let standard_ids = [
            "command.move",
            "command.open",
            "command.search",
            "command.examine",
            "command.take",
            "command.drop",
            "command.use",
            "command.question",
            "command.deduce",
            "command.solve",
        ];
        let copied = commands
            .iter()
            .filter(|command| standard_ids.contains(&command.id.as_str()))
            .count();
        let copied_legacy_catalog =
            copied >= 3 && commands.iter().any(|command| command.id == "command.claim");
        for command in commands {
            let Some(parameters) = command
                .mapping
                .get(Value::String("parameters".to_string()))
                .and_then(Value::as_sequence)
            else {
                continue;
            };
            for (index, parameter) in parameters.iter().enumerate() {
                let Some(parameter) = parameter.as_mapping() else {
                    continue;
                };
                if copied_legacy_catalog
                    && standard_ids.contains(&command.id.as_str())
                    && (parameter.contains_key(Value::String("type".to_string()))
                        || parameter.contains_key(Value::String("required".to_string())))
                {
                    self.push(
                        Severity::Warning,
                        "ruleset.legacy_command_parameter",
                        "migrate legacy `type`/`required` to ordered `types` plus `min`/`max`; standard commands can be removed after selecting `case.ruleset`"
                            .to_string(),
                        &command.path,
                        Some(format!("{}/parameters/{index}", command.pointer)),
                        None,
                        Some(command.id.clone()),
                    );
                }
            }
        }

        if self.ruleset.is_none() && copied_legacy_catalog {
            let command = commands
                .iter()
                .find(|command| standard_ids.contains(&command.id.as_str()))
                .expect("copied command exists");
            self.push(
                Severity::Warning,
                "ruleset.copied_standard_commands",
                "this story copies maintained mystery commands; select `case.ruleset: { id: ruleset.standard_mystery, version: \"1.0.0\" }` and keep only story-specific extensions in `commands.yaml`"
                    .to_string(),
                &command.path,
                Some("/commands".to_string()),
                None,
                None,
            );
        }
    }

    fn validate_player_limits(&mut self, case: &Item) {
        let pointer = format!("{}/players", case.pointer);
        let Some(value) = case.mapping.get(Value::String("players".to_string())) else {
            self.push(
                Severity::Error,
                "case.players_missing",
                "format 3 requires structured `case.players` limits with `min` and `max`"
                    .to_string(),
                &case.path,
                Some(pointer),
                None,
                Some(case.id.clone()),
            );
            return;
        };
        let Some(players) = value.as_mapping() else {
            self.push(
                Severity::Error,
                "case.players_type",
                "`case.players` must be a mapping with positive whole-number `min` and `max`"
                    .to_string(),
                &case.path,
                Some(pointer),
                None,
                Some(case.id.clone()),
            );
            return;
        };
        self.validate_mapping_fields(
            players,
            &["min", "max"],
            "case.players",
            &case.path,
            &case.source,
            &pointer,
            Some(&case.id),
        );
        let min = players
            .get(Value::String("min".to_string()))
            .and_then(Value::as_i64);
        let max = players
            .get(Value::String("max".to_string()))
            .and_then(Value::as_i64);
        for (field, value) in [("min", min), ("max", max)] {
            if !value.is_some_and(|value| value > 0 && usize::try_from(value).is_ok()) {
                self.push(
                    Severity::Error,
                    &format!("case.players_{field}"),
                    format!("`case.players.{field}` must be a positive whole number"),
                    &case.path,
                    Some(format!("{pointer}/{field}")),
                    None,
                    Some(case.id.clone()),
                );
            }
        }
        if min.zip(max).is_some_and(|(min, max)| min > max) {
            self.push(
                Severity::Error,
                "case.players_order",
                "`case.players.min` cannot exceed `case.players.max`".to_string(),
                &case.path,
                Some(pointer),
                None,
                Some(case.id.clone()),
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn validate_strict_field_contract(
        &mut self,
        cases: &[Item],
        settings: &[Item],
        routes: &[Item],
        characters: &[Item],
        entities: &[Item],
        events: &[Item],
        facts: &[Item],
        deductions: &[Item],
        flags: &[Item],
        commands: &[Item],
        triggers: &[Item],
    ) {
        let mut case_fields = vec![
            "id",
            "format_version",
            "ruleset",
            "title",
            "genre",
            "tone",
            "players",
            "estimated_duration_minutes",
            "entry_settings",
            "exit_settings",
            "initial_time",
            "premise",
            "opening",
            "author_notes",
        ];
        if self.is_format_3_2_or_later() {
            case_fields.push("features");
        }
        self.validate_item_fields(cases, &case_fields);
        self.validate_item_fields(
            settings,
            &[
                "id",
                "tag_id",
                "type",
                "navigable",
                "name",
                "description",
                "parent",
                "facts",
                "points",
                "author_notes",
            ],
        );
        self.validate_item_fields(
            routes,
            &[
                "id",
                "from",
                "to",
                "bidirectional",
                "travel_minutes",
                "hidden",
                "requires",
                "author_notes",
            ],
        );
        let character_fields: &[&str] = if self.is_format_3_1_or_later() {
            &[
                "id",
                "tag_id",
                "name",
                "voice_id",
                "role",
                "age",
                "occupation",
                "description",
                "initial",
                "presence",
                "portrayal",
                "testimony",
                "narrator_guidance",
                "knowledge",
                "facts",
                "author_notes",
            ]
        } else {
            &[
                "id",
                "tag_id",
                "name",
                "voice_id",
                "role",
                "age",
                "occupation",
                "description",
                "portrayal",
                "testimony",
                "narrator_guidance",
                "knowledge",
                "facts",
                "author_notes",
            ]
        };
        self.validate_item_fields(characters, character_fields);
        self.validate_item_fields(
            entities,
            &[
                "id",
                "tag_id",
                "type",
                "name",
                "description",
                "physical",
                "visibility",
                "initial",
                "facts",
                "points",
                "author_notes",
            ],
        );
        self.validate_item_fields(
            events,
            &[
                "id",
                "day",
                "time",
                "duration_minutes",
                "location",
                "participants",
                "summary",
                "facts",
                "author_notes",
            ],
        );
        self.validate_item_fields(
            facts,
            &[
                "id",
                "statement",
                "narrative_detail",
                "category",
                "about",
                "requires",
                "sources",
                "occurred_at",
                "on",
                "when",
                "author_notes",
            ],
        );
        self.validate_item_fields(
            deductions,
            &[
                "id",
                "conclusion",
                "inputs",
                "truth",
                "contradicted_by",
                "requires",
                "solves",
                "points",
                "author_notes",
            ],
        );
        self.validate_item_fields(
            flags,
            &["id", "name", "description", "initial_state", "author_notes"],
        );
        self.validate_item_fields(
            commands,
            &[
                "id",
                "tag_id",
                "name",
                "description",
                "parameters",
                "effects",
                "points",
                "author_notes",
            ],
        );
        self.validate_item_fields(
            triggers,
            &[
                "id",
                "name",
                "description",
                "command",
                "once",
                "time",
                "location",
                "parameters",
                "any_of",
                "all_of",
                "effects",
                "facts",
                "on",
                "when",
                "after",
                "author_notes",
            ],
        );

        for items in [
            cases, settings, routes, characters, entities, events, facts, deductions, flags,
            commands, triggers,
        ] {
            for item in items {
                self.validate_optional_namespace(item, "author_notes", None);
            }
        }
        for character in characters {
            self.validate_optional_namespace(
                character,
                "narrator_guidance",
                Some(&[
                    "goal",
                    "secret",
                    "motive",
                    "method",
                    "cover_story",
                    "testimony_guidance",
                ]),
            );
        }
        for item in settings.iter().chain(characters).chain(entities) {
            if !string_field(&item.mapping, "description")
                .is_some_and(|description| !description.trim().is_empty())
            {
                self.push(
                    Severity::Error,
                    &format!("{}.description", item_kind(item).unwrap_or("item")),
                    "format 3 requires one non-empty baseline player-safe `description`"
                        .to_string(),
                    &item.path,
                    Some(format!("{}/description", item.pointer)),
                    None,
                    Some(item.id.clone()),
                );
            }
        }
        for setting in settings {
            if setting
                .mapping
                .get(Value::String("navigable".to_string()))
                .is_some_and(|value| value.as_bool().is_none())
            {
                self.push(
                    Severity::Error,
                    "setting.navigable_type",
                    "setting `navigable` must be a boolean".to_string(),
                    &setting.path,
                    Some(format!("{}/navigable", setting.pointer)),
                    None,
                    Some(setting.id.clone()),
                );
            }
        }
        for entity in entities {
            if let Some(initial) = entity
                .mapping
                .get(Value::String("initial".to_string()))
                .and_then(Value::as_mapping)
            {
                self.validate_mapping_fields(
                    initial,
                    &["container"],
                    "entity.initial",
                    &entity.path,
                    &entity.source,
                    &format!("{}/initial", entity.pointer),
                    Some(&entity.id),
                );
            }
        }
        if self.is_format_3_1_or_later() {
            for character in characters {
                if let Some(initial) = character
                    .mapping
                    .get(Value::String("initial".to_string()))
                    .and_then(Value::as_mapping)
                {
                    self.validate_mapping_fields(
                        initial,
                        &["location"],
                        "character.initial",
                        &character.path,
                        &character.source,
                        &format!("{}/initial", character.pointer),
                        Some(&character.id),
                    );
                }
                if let Some(presence) = character
                    .mapping
                    .get(Value::String("presence".to_string()))
                    .and_then(Value::as_mapping)
                {
                    self.validate_mapping_fields(
                        presence,
                        &["requires"],
                        "character.presence",
                        &character.path,
                        &character.source,
                        &format!("{}/presence", character.pointer),
                        Some(&character.id),
                    );
                }
            }
        }
        for deduction in deductions {
            if let Some(solves) = deduction
                .mapping
                .get(Value::String("solves".to_string()))
                .and_then(Value::as_mapping)
            {
                self.validate_mapping_fields(
                    solves,
                    &["culprit", "weapon", "location", "time"],
                    "deduction.solves",
                    &deduction.path,
                    &deduction.source,
                    &format!("{}/solves", deduction.pointer),
                    Some(&deduction.id),
                );
            }
        }
        self.validate_solution_contract();
    }

    fn validate_item_fields(&mut self, items: &[Item], allowed: &[&str]) {
        for item in items {
            self.validate_mapping_fields(
                &item.mapping,
                allowed,
                item_kind(item).unwrap_or("item"),
                &item.path,
                &item.source,
                &item.pointer,
                Some(&item.id),
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn validate_mapping_fields(
        &mut self,
        mapping: &Mapping,
        allowed: &[&str],
        namespace: &str,
        path: &str,
        source: &str,
        pointer: &str,
        subject_id: Option<&str>,
    ) {
        for key in mapping.keys() {
            let Some(key) = key.as_str() else {
                self.push(
                    Severity::Error,
                    &format!("{namespace}.field"),
                    format!("{namespace} field names must be strings"),
                    path,
                    Some(pointer.to_string()),
                    None,
                    subject_id.map(str::to_string),
                );
                continue;
            };
            if !allowed.contains(&key) {
                self.push(
                    Severity::Error,
                    &format!("{namespace}.unknown_field"),
                    format!("`{key}` is not supported by format 3 {namespace}"),
                    path,
                    Some(format!("{pointer}/{}", escape_pointer(key))),
                    locate_scalar(source, key),
                    subject_id.map(str::to_string),
                );
            }
        }
    }

    fn validate_optional_namespace(&mut self, item: &Item, field: &str, allowed: Option<&[&str]>) {
        let Some(value) = item.mapping.get(Value::String(field.to_string())) else {
            return;
        };
        let pointer = format!("{}/{}", item.pointer, escape_pointer(field));
        let Some(mapping) = value.as_mapping() else {
            self.push(
                Severity::Error,
                &format!("{field}.type"),
                format!("`{field}` must be a mapping namespace"),
                &item.path,
                Some(pointer),
                None,
                Some(item.id.clone()),
            );
            return;
        };
        if let Some(allowed) = allowed {
            self.validate_mapping_fields(
                mapping,
                allowed,
                field,
                &item.path,
                &item.source,
                &pointer,
                Some(&item.id),
            );
        }
    }

    fn validate_solution_contract(&mut self) {
        let solutions = self
            .parsed
            .iter()
            .filter_map(|file| {
                file.value
                    .as_mapping()?
                    .get(Value::String("solution".to_string()))?
                    .as_mapping()
                    .map(|solution| {
                        (
                            file.path.to_string(),
                            file.source.to_string(),
                            solution.clone(),
                        )
                    })
            })
            .collect::<Vec<_>>();
        for (path, source, solution) in solutions {
            let allowed = if self.is_format_3_3_or_later() {
                &[
                    "win_state",
                    "questions",
                    "victim",
                    "culprit",
                    "weapon",
                    "location",
                    "time",
                    "deduction",
                    "narrator_guidance",
                    "author_notes",
                ][..]
            } else {
                &[
                    "victim",
                    "culprit",
                    "weapon",
                    "location",
                    "time",
                    "deduction",
                    "narrator_guidance",
                    "author_notes",
                ][..]
            };
            self.validate_mapping_fields(
                &solution,
                allowed,
                "solution",
                &path,
                &source,
                "/solution",
                None,
            );
            for namespace in ["narrator_guidance", "author_notes"] {
                let Some(value) = solution.get(Value::String(namespace.to_string())) else {
                    continue;
                };
                let Some(mapping) = value.as_mapping() else {
                    self.push(
                        Severity::Error,
                        &format!("solution.{namespace}_type"),
                        format!("solution `{namespace}` must be a mapping namespace"),
                        &path,
                        Some(format!("/solution/{namespace}")),
                        None,
                        None,
                    );
                    continue;
                };
                if namespace == "narrator_guidance" {
                    self.validate_mapping_fields(
                        mapping,
                        &["motive", "method", "proof_summary"],
                        "solution.narrator_guidance",
                        &path,
                        &source,
                        "/solution/narrator_guidance",
                        None,
                    );
                }
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
        if self.uses_question_solution_ruleset() && solutions.is_empty() {
            self.push(
                Severity::Error,
                "solution.missing_question_contract",
                "ruleset.standard_mystery@3.0.0 requires a Format 3.3 `solution` block with `win_state` and `questions`"
                    .to_string(),
                "case.yaml",
                Some("/solution".to_string()),
                None,
                None,
            );
        }
        for (path, source, solution) in solutions {
            let legacy_fields = [
                "victim",
                "culprit",
                "weapon",
                "location",
                "time",
                "deduction",
            ];
            let has_legacy = legacy_fields
                .iter()
                .any(|field| solution.contains_key(Value::String((*field).to_string())));
            let has_questions = solution.contains_key(Value::String("questions".to_string()));
            let has_win_state = solution.contains_key(Value::String("win_state".to_string()));

            if self.is_format_3_3_or_later() {
                if has_legacy {
                    self.push(
                        Severity::Error,
                        if has_questions || has_win_state {
                            "solution.contract_mixed"
                        } else {
                            "solution.legacy_contract"
                        },
                        "Format 3.3 replaces the legacy culprit/weapon/location/deduction solution with `win_state` and one to four authored `questions`; migrate the complete block instead of mixing contracts"
                            .to_string(),
                        &path,
                        Some("/solution".to_string()),
                        None,
                        None,
                    );
                    if !has_questions && !has_win_state {
                        continue;
                    }
                }
                self.validate_question_solution(&path, &source, &solution);
                continue;
            }

            if has_questions || has_win_state {
                self.push(
                    Severity::Error,
                    "solution.format_incompatible",
                    "authored solution questions require Format 3.3 and ruleset.standard_mystery@3.0.0"
                        .to_string(),
                    &path,
                    Some("/solution".to_string()),
                    None,
                    None,
                );
                continue;
            }
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

    fn validate_question_solution(&mut self, path: &str, source: &str, solution: &Mapping) {
        if !self.uses_question_solution_ruleset() {
            self.push(
                Severity::Error,
                "solution.ruleset_incompatible",
                "Format 3.3 authored questions require `case.ruleset` ruleset.standard_mystery@3.0.0"
                    .to_string(),
                path,
                Some("/solution".to_string()),
                None,
                None,
            );
        }

        let win_state = string_field(solution, "win_state");
        match win_state.and_then(|id| self.definitions.get(id).map(|definition| (id, definition))) {
            Some((_, definition)) if matches!(definition.kind, Kind::WinState | Kind::EndState) => {
            }
            Some((id, _)) => self.push(
                Severity::Error,
                "solution.win_state_type",
                format!("solution `win_state` `{id}` must identify an end state"),
                path,
                Some("/solution/win_state".to_string()),
                locate_scalar(source, id),
                None,
            ),
            None => self.push(
                Severity::Error,
                "solution.win_state_unknown",
                "solution `win_state` must identify a defined end state".to_string(),
                path,
                Some("/solution/win_state".to_string()),
                win_state.and_then(|id| locate_scalar(source, id)),
                None,
            ),
        }

        let Some(questions) = solution
            .get(Value::String("questions".to_string()))
            .and_then(Value::as_sequence)
        else {
            self.push(
                Severity::Error,
                "solution.questions_type",
                "solution `questions` must contain one to four question mappings".to_string(),
                path,
                Some("/solution/questions".to_string()),
                None,
                None,
            );
            return;
        };
        if !(MIN_SOLUTION_QUESTIONS..=MAX_SOLUTION_QUESTIONS).contains(&questions.len()) {
            self.push(
                Severity::Error,
                "solution.questions_count",
                "solution `questions` must contain one to four questions".to_string(),
                path,
                Some("/solution/questions".to_string()),
                None,
                None,
            );
        }

        let deck_subjects = self.deck_subjects();
        let mut seen_answers = BTreeMap::<String, String>::new();
        for (question_index, question) in questions.iter().enumerate() {
            let question_pointer = format!("/solution/questions/{question_index}");
            let Some(question) = question.as_mapping() else {
                self.push(
                    Severity::Error,
                    "solution.question_type",
                    "each solution question must be a mapping with `prompt`, `answer`, and optional `ordered`"
                        .to_string(),
                    path,
                    Some(question_pointer),
                    None,
                    None,
                );
                continue;
            };
            self.validate_mapping_fields(
                question,
                &["prompt", "answer", "ordered"],
                "solution.question",
                path,
                source,
                &question_pointer,
                None,
            );
            if !string_field(question, "prompt").is_some_and(|prompt| !prompt.trim().is_empty()) {
                self.push(
                    Severity::Error,
                    "solution.question_prompt",
                    "solution question `prompt` must be a non-empty string".to_string(),
                    path,
                    Some(format!("{question_pointer}/prompt")),
                    None,
                    None,
                );
            }
            if question
                .get(Value::String("ordered".to_string()))
                .is_some_and(|ordered| ordered.as_bool().is_none())
            {
                self.push(
                    Severity::Error,
                    "solution.question_ordered_type",
                    "solution question `ordered` must be a boolean".to_string(),
                    path,
                    Some(format!("{question_pointer}/ordered")),
                    None,
                    None,
                );
            }
            let Some(answers) = question
                .get(Value::String("answer".to_string()))
                .and_then(Value::as_sequence)
            else {
                self.push(
                    Severity::Error,
                    "solution.question_answer_type",
                    "solution question `answer` must contain one to five physical-card IDs"
                        .to_string(),
                    path,
                    Some(format!("{question_pointer}/answer")),
                    None,
                    None,
                );
                continue;
            };
            if !(MIN_SOLUTION_ANSWER_CARDS..=MAX_SOLUTION_ANSWER_CARDS).contains(&answers.len()) {
                self.push(
                    Severity::Error,
                    "solution.question_answer_count",
                    "solution question `answer` must contain one to five IDs".to_string(),
                    path,
                    Some(format!("{question_pointer}/answer")),
                    None,
                    None,
                );
            }
            let mut row_answers = BTreeSet::new();
            for (answer_index, answer) in answers.iter().enumerate() {
                let answer_pointer = format!("{question_pointer}/answer/{answer_index}");
                let Some(answer) = answer.as_str().filter(|answer| !answer.trim().is_empty())
                else {
                    self.push(
                        Severity::Error,
                        "solution.question_answer_id",
                        "each solution answer must be a non-empty canonical ID".to_string(),
                        path,
                        Some(answer_pointer),
                        None,
                        None,
                    );
                    continue;
                };
                let answer_range = locate_scalar(source, answer);
                if !row_answers.insert(answer.to_string()) {
                    self.push(
                        Severity::Error,
                        "solution.question_answer_duplicate",
                        format!("answer `{answer}` occurs more than once in this question"),
                        path,
                        Some(answer_pointer.clone()),
                        answer_range,
                        Some(answer.to_string()),
                    );
                    continue;
                } else if let Some(previous_pointer) =
                    seen_answers.insert(answer.to_string(), answer_pointer.clone())
                {
                    self.push(
                        Severity::Error,
                        "solution.answer_reused",
                        format!("physical card `{answer}` may answer only one solution question; it was already used at `{previous_pointer}`"),
                        path,
                        Some(answer_pointer.clone()),
                        answer_range,
                        Some(answer.to_string()),
                    );
                }
                let physical_subject = match self.definitions.get(answer) {
                    Some(definition)
                        if matches!(
                            definition.kind,
                            Kind::Setting | Kind::Character | Kind::Entity
                        ) =>
                    {
                        true
                    }
                    Some(_) => {
                        self.push(
                            Severity::Error,
                            "solution.question_answer_type",
                            format!("solution answer `{answer}` must identify a setting, character, or entity physical card"),
                            path,
                            Some(answer_pointer.clone()),
                            answer_range,
                            Some(answer.to_string()),
                        );
                        false
                    }
                    None => {
                        self.push(
                            Severity::Error,
                            "solution.question_answer_unknown",
                            format!("solution answer `{answer}` is not defined"),
                            path,
                            Some(answer_pointer.clone()),
                            answer_range,
                            Some(answer.to_string()),
                        );
                        false
                    }
                };
                if physical_subject && !deck_subjects.contains(answer) {
                    self.push(
                        Severity::Error,
                        "solution.question_answer_not_in_deck",
                        format!(
                            "solution answer `{answer}` must have a physical card in `deck.yaml`"
                        ),
                        path,
                        Some(answer_pointer),
                        answer_range,
                        Some(answer.to_string()),
                    );
                }
            }
        }
    }

    fn deck_subjects(&self) -> BTreeSet<String> {
        self.parsed
            .iter()
            .filter(|file| file.path == "deck.yaml")
            .filter_map(|file| file.value.as_mapping())
            .filter_map(|root| root.get(Value::String("cards".to_string())))
            .filter_map(Value::as_sequence)
            .flatten()
            .filter_map(Value::as_mapping)
            .filter_map(|card| string_field(card, "subject"))
            .map(str::to_string)
            .collect()
    }

    fn validate_terminal_configuration(&mut self, end_states: &[Item], win_states: &[Item]) {
        let has_end_section = self.sections.contains_key("end_states");
        let has_win_section = self.sections.contains_key("win_states");
        if has_end_section && has_win_section {
            self.push(
                Severity::Error,
                "end_states.mixed_legacy_section",
                "define exactly one ordered terminal section; migrate `win_states` into `end_states` without keeping both roots"
                    .to_string(),
                "",
                Some("/end_states".to_string()),
                None,
                None,
            );
        }
        if has_end_section && !self.is_format_3_4_or_later() {
            self.push(
                Severity::Error,
                "end_states.format_incompatible",
                "canonical `end_states` require story format 3.4 or later".to_string(),
                end_states.first().map_or("", |state| state.path.as_str()),
                Some("/end_states".to_string()),
                None,
                None,
            );
        }
        if has_win_section && self.is_format_3_4_or_later() {
            self.push(
                Severity::Warning,
                "win_states.legacy_compatibility",
                "legacy `win_states` retain authored order and behave as `won`/`full`; migrate them to canonical `end_states` when editing the terminal contract"
                    .to_string(),
                win_states.first().map_or("", |state| state.path.as_str()),
                Some("/win_states".to_string()),
                None,
                None,
            );
        }
        let has_solution = self.parsed.iter().any(|file| {
            file.value
                .as_mapping()
                .and_then(|root| root.get(Value::String("solution".to_string())))
                .and_then(Value::as_mapping)
                .is_some_and(|solution| {
                    ["victim", "culprit", "weapon", "location"]
                        .iter()
                        .all(|field| string_field(solution, field).is_some())
                        || (string_field(solution, "win_state").is_some()
                            && solution
                                .get(Value::String("questions".to_string()))
                                .and_then(Value::as_sequence)
                                .is_some_and(|questions| !questions.is_empty()))
                })
        });
        if end_states.is_empty() && win_states.is_empty() && !has_solution {
            let canonical_end_states = self.is_format_3_4_or_later();
            self.push(
                Severity::Error,
                if canonical_end_states {
                    "end_states.missing_terminal_configuration"
                } else {
                    "win_states.missing_terminal_configuration"
                },
                if canonical_end_states {
                    "define at least one generic end state or a valid `solution` block".to_string()
                } else {
                    "define at least one generic win state or a valid `solution` block".to_string()
                },
                "",
                Some(if canonical_end_states {
                    "/end_states".to_string()
                } else {
                    "/win_states".to_string()
                }),
                None,
                None,
            );
        }
    }

    fn validate_win_states(&mut self, win_states: &[Item]) {
        let solution_win_state = if self.is_format_3_3_or_later() {
            self.parsed.iter().find_map(|file| {
                file.value
                    .as_mapping()?
                    .get(Value::String("solution".to_string()))?
                    .as_mapping()
                    .and_then(|solution| string_field(solution, "win_state"))
                    .map(str::to_string)
            })
        } else {
            None
        };
        for win_state in win_states {
            for field in ["name", "text"] {
                if string_field(&win_state.mapping, field)
                    .map_or(true, |value| value.trim().is_empty())
                {
                    self.push(
                        Severity::Error,
                        if field == "name" {
                            "win_states.name"
                        } else {
                            "win_states.text"
                        },
                        format!("win state `{field}` must be a non-empty string"),
                        &win_state.path,
                        Some(format!("{}/{}", win_state.pointer, escape_pointer(field))),
                        None,
                        Some(win_state.id.clone()),
                    );
                }
            }

            match win_state.mapping.get(Value::String("requires".to_string())) {
                Some(requires) if is_string_sequence(requires) => {}
                Some(_) => self.push(
                    Severity::Error,
                    "win_states.requires_type",
                    "win state `requires` must be a sequence of persistent requirement IDs"
                        .to_string(),
                    &win_state.path,
                    Some(format!("{}/requires", win_state.pointer)),
                    None,
                    Some(win_state.id.clone()),
                ),
                None => {}
            }

            match win_state
                .mapping
                .get(Value::String("minimum_points".to_string()))
            {
                Some(Value::Number(number)) if number.as_u64().is_some() => {}
                Some(_) => self.push(
                    Severity::Error,
                    "win_states.minimum_points",
                    "win state `minimum_points` must be a non-negative whole number".to_string(),
                    &win_state.path,
                    Some(format!("{}/minimum_points", win_state.pointer)),
                    None,
                    Some(win_state.id.clone()),
                ),
                None => {}
            }

            let has_requirements = win_state
                .mapping
                .get(Value::String("requires".to_string()))
                .and_then(Value::as_sequence)
                .is_some_and(|requirements| !requirements.is_empty());
            let has_point_threshold = win_state
                .mapping
                .get(Value::String("minimum_points".to_string()))
                .and_then(Value::as_u64)
                .is_some_and(|minimum| minimum > 0);
            let is_solution_target = solution_win_state.as_deref() == Some(&win_state.id);
            if is_solution_target && (has_requirements || has_point_threshold) {
                self.push(
                    Severity::Error,
                    "win_states.solution_condition_conflict",
                    "the win state selected by `solution.win_state` must not duplicate completion conditions; answering all solution questions is its condition"
                        .to_string(),
                    &win_state.path,
                    Some(win_state.pointer.clone()),
                    None,
                    Some(win_state.id.clone()),
                );
            } else if !is_solution_target && !has_requirements && !has_point_threshold {
                self.push(
                    Severity::Error,
                    "win_states.unconditional",
                    "win state must require at least one persistent condition or a positive point threshold"
                        .to_string(),
                    &win_state.path,
                    Some(win_state.pointer.clone()),
                    None,
                    Some(win_state.id.clone()),
                );
            }

            for key in win_state.mapping.keys().filter_map(Value::as_str) {
                if !matches!(key, "id" | "name" | "requires" | "minimum_points" | "text") {
                    self.push(
                        Severity::Error,
                        "win_states.unknown_field",
                        format!("unknown win-state field `{key}`"),
                        &win_state.path,
                        Some(format!("{}/{}", win_state.pointer, escape_pointer(key))),
                        None,
                        Some(win_state.id.clone()),
                    );
                }
            }
        }
    }

    fn validate_end_states(&mut self, end_states: &[Item]) {
        if end_states.is_empty() {
            return;
        }
        let solution_target = self.solution_terminal_state_id();
        for end_state in end_states {
            for field in ["name", "text"] {
                if string_field(&end_state.mapping, field)
                    .map_or(true, |value| value.trim().is_empty())
                {
                    self.push(
                        Severity::Error,
                        if field == "name" {
                            "end_states.name"
                        } else {
                            "end_states.text"
                        },
                        format!("end state `{field}` must be a non-empty string"),
                        &end_state.path,
                        Some(format!("{}/{}", end_state.pointer, escape_pointer(field))),
                        None,
                        Some(end_state.id.clone()),
                    );
                }
            }

            let outcome = string_field(&end_state.mapping, "outcome");
            if !matches!(outcome, Some("won" | "lost")) {
                self.push(
                    Severity::Error,
                    "end_states.outcome",
                    "end state `outcome` must be `won` or `lost`".to_string(),
                    &end_state.path,
                    Some(format!("{}/outcome", end_state.pointer)),
                    None,
                    Some(end_state.id.clone()),
                );
            }
            let resolution = string_field(&end_state.mapping, "resolution");
            if !matches!(resolution, Some("full" | "partial" | "failure")) {
                self.push(
                    Severity::Error,
                    "end_states.resolution",
                    "end state `resolution` must be `full`, `partial`, or `failure`".to_string(),
                    &end_state.path,
                    Some(format!("{}/resolution", end_state.pointer)),
                    None,
                    Some(end_state.id.clone()),
                );
            } else if matches!(
                (outcome, resolution),
                (Some("won"), Some("failure")) | (Some("lost"), Some("full" | "partial"))
            ) {
                self.push(
                    Severity::Error,
                    "end_states.outcome_resolution_conflict",
                    "`won` permits `full` or `partial`; `lost` requires `failure`".to_string(),
                    &end_state.path,
                    Some(end_state.pointer.clone()),
                    None,
                    Some(end_state.id.clone()),
                );
            }

            match end_state.mapping.get(Value::String("requires".to_string())) {
                Some(requires) if is_string_sequence(requires) => {}
                Some(_) => self.push(
                    Severity::Error,
                    "end_states.requires_type",
                    "end state `requires` must be a sequence of persistent requirement IDs"
                        .to_string(),
                    &end_state.path,
                    Some(format!("{}/requires", end_state.pointer)),
                    None,
                    Some(end_state.id.clone()),
                ),
                None => {}
            }
            match end_state
                .mapping
                .get(Value::String("minimum_points".to_string()))
            {
                Some(Value::Number(number)) if number.as_u64().is_some() => {}
                Some(_) => self.push(
                    Severity::Error,
                    "end_states.minimum_points",
                    "end state `minimum_points` must be a non-negative whole number".to_string(),
                    &end_state.path,
                    Some(format!("{}/minimum_points", end_state.pointer)),
                    None,
                    Some(end_state.id.clone()),
                ),
                None => {}
            }
            match end_state
                .mapping
                .get(Value::String("at_or_after".to_string()))
            {
                Some(Value::String(time)) if valid_time(time) => {}
                Some(_) => self.push(
                    Severity::Error,
                    "end_states.at_or_after",
                    "end state `at_or_after` must be a quoted 24-hour HH:MM value".to_string(),
                    &end_state.path,
                    Some(format!("{}/at_or_after", end_state.pointer)),
                    string_field(&end_state.mapping, "at_or_after")
                        .and_then(|time| locate_scalar(&end_state.source, time)),
                    Some(end_state.id.clone()),
                ),
                None => {}
            }

            let has_requirements = !string_list_field(&end_state.mapping, "requires").is_empty();
            let has_points = end_state
                .mapping
                .get(Value::String("minimum_points".to_string()))
                .and_then(Value::as_u64)
                .is_some_and(|minimum| minimum > 0);
            let has_time = string_field(&end_state.mapping, "at_or_after").is_some_and(valid_time);
            let is_solution_target = solution_target.as_deref() == Some(&end_state.id);
            if is_solution_target && outcome == Some("lost") {
                self.push(
                    Severity::Error,
                    "end_states.solution_outcome_conflict",
                    "the end state selected by `solution.win_state` must have outcome `won`"
                        .to_string(),
                    &end_state.path,
                    Some(format!("{}/outcome", end_state.pointer)),
                    None,
                    Some(end_state.id.clone()),
                );
            }
            if is_solution_target && (has_requirements || has_points || has_time) {
                self.push(
                    Severity::Error,
                    "end_states.solution_condition_conflict",
                    "the end state selected by `solution.win_state` must not duplicate completion conditions; answering all solution questions is its condition"
                        .to_string(),
                    &end_state.path,
                    Some(end_state.pointer.clone()),
                    None,
                    Some(end_state.id.clone()),
                );
            } else if !is_solution_target && !has_requirements && !has_points && !has_time {
                self.push(
                    Severity::Error,
                    "end_states.unconditional",
                    "end state must require a persistent condition, positive point threshold, or game-clock threshold"
                        .to_string(),
                    &end_state.path,
                    Some(end_state.pointer.clone()),
                    None,
                    Some(end_state.id.clone()),
                );
            }

            for key in end_state.mapping.keys().filter_map(Value::as_str) {
                if !matches!(
                    key,
                    "id" | "name"
                        | "outcome"
                        | "resolution"
                        | "requires"
                        | "minimum_points"
                        | "at_or_after"
                        | "text"
                ) {
                    self.push(
                        Severity::Error,
                        "end_states.unknown_field",
                        format!("unknown end-state field `{key}`"),
                        &end_state.path,
                        Some(format!("{}/{}", end_state.pointer, escape_pointer(key))),
                        None,
                        Some(end_state.id.clone()),
                    );
                }
            }
        }
        self.validate_end_state_precedence(end_states, solution_target.as_deref());
    }

    fn solution_terminal_state_id(&self) -> Option<String> {
        self.parsed.iter().find_map(|file| {
            file.value
                .as_mapping()?
                .get(Value::String("solution".to_string()))?
                .as_mapping()
                .and_then(|solution| string_field(solution, "win_state"))
                .map(str::to_string)
        })
    }

    fn validate_end_state_precedence(
        &mut self,
        end_states: &[Item],
        solution_target: Option<&str>,
    ) {
        for (later_index, later) in end_states.iter().enumerate() {
            if solution_target == Some(later.id.as_str()) {
                continue;
            }
            let Some(later_condition) = EndStateCondition::from_item(later) else {
                continue;
            };
            for earlier in &end_states[..later_index] {
                if solution_target == Some(earlier.id.as_str()) {
                    continue;
                }
                let Some(earlier_condition) = EndStateCondition::from_item(earlier) else {
                    continue;
                };
                let duplicate = earlier_condition == later_condition;
                if duplicate || earlier_condition.is_implied_by(&later_condition) {
                    self.diagnostics.push(Diagnostic {
                        severity: Severity::Error,
                        code: if duplicate {
                            "end_states.duplicate_precedence".to_string()
                        } else {
                            "end_states.unreachable_precedence".to_string()
                        },
                        message: if duplicate {
                            format!(
                                "end state `{}` repeats the exact condition of earlier `{}`; authored precedence would always select the earlier state",
                                later.id, earlier.id
                            )
                        } else {
                            format!(
                                "end state `{}` is unreachable because earlier `{}` is always satisfied whenever it is; move the more specific state earlier or make the conditions distinct",
                                later.id, earlier.id
                            )
                        },
                        path: later.path.clone(),
                        pointer: Some(later.pointer.clone()),
                        range: None,
                        subject_id: Some(later.id.clone()),
                        related: vec![RelatedLocation {
                            message: "earlier authored state is here".to_string(),
                            path: earlier.path.clone(),
                            pointer: Some(earlier.pointer.clone()),
                            range: None,
                        }],
                    });
                    break;
                }
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

    fn validate_legacy_inline_tag_ids(
        &mut self,
        settings: &[Item],
        characters: &[Item],
        entities: &[Item],
        commands: &[Item],
    ) {
        for item in settings
            .iter()
            .chain(characters)
            .chain(entities)
            .chain(commands)
        {
            let pointer = format!("{}/tag_id", item.pointer);
            let value = item.mapping.get(Value::String("tag_id".to_string()));
            if value.is_none() {
                continue;
            }
            let range = locate_item_field(&item.source, &item.id, "tag_id");
            self.push(
                Severity::Error,
                "deck.legacy_inline_tag_id",
                format!(
                    "legacy inline `tag_id` on `{}` must move to `deck.yaml` as `{{ tag_id: {}, subject: {} }}`",
                    item.id,
                    value.and_then(Value::as_i64).map_or("…".to_string(), |id| id.to_string()),
                    item.id
                ),
                &item.path,
                Some(pointer),
                range,
                Some(item.id.clone()),
            );
        }
    }

    fn validate_deck(&mut self) {
        let Some(locations) = self.sections.get("cards").cloned() else {
            return;
        };
        let mut seen_tags = BTreeMap::<i64, (String, String, Option<SourceRange>)>::new();
        let mut seen_subjects = BTreeMap::<String, (String, String, Option<SourceRange>)>::new();
        for (path, section_pointer) in locations {
            let Some(file) = self.parsed.iter().find(|file| file.path == path) else {
                continue;
            };
            let source = file.source.to_string();
            let root = file.value.as_mapping().expect("root mappings were checked");
            let Some(cards_value) = root.get(Value::String("cards".to_string())) else {
                continue;
            };
            let Some(cards) = cards_value.as_sequence() else {
                self.push(
                    Severity::Error,
                    "schema.section_type",
                    "`cards` must be a sequence".to_string(),
                    &path,
                    Some(section_pointer),
                    None,
                    None,
                );
                continue;
            };
            let cards = cards.clone();
            for (index, value) in cards.iter().enumerate() {
                let pointer = format!("{section_pointer}/{index}");
                let Some(card) = value.as_mapping() else {
                    self.push(
                        Severity::Error,
                        "deck.entry_type",
                        "deck entries must be mappings with `tag_id` and `subject`".to_string(),
                        &path,
                        Some(pointer),
                        None,
                        None,
                    );
                    continue;
                };
                let tag_pointer = format!("{pointer}/tag_id");
                let tag_value = card.get(Value::String("tag_id".to_string()));
                let tag_id = tag_value.and_then(Value::as_i64);
                if tag_id.is_none() {
                    self.push(
                        Severity::Error,
                        "deck.tag_id_invalid",
                        format!("deck `tag_id` must be a whole number from 0 through {TAG_STANDARD_41H12_MAX_ID}"),
                        &path,
                        Some(tag_pointer.clone()),
                        None,
                        None,
                    );
                } else if !((0..=TAG_STANDARD_41H12_MAX_ID).contains(&tag_id.unwrap())) {
                    self.push(
                        Severity::Error,
                        "deck.tag_id_out_of_range",
                        format!("deck `tag_id` {} is outside the tagStandard41h12 range 0 through {TAG_STANDARD_41H12_MAX_ID}", tag_id.unwrap()),
                        &path,
                        Some(tag_pointer.clone()),
                        None,
                        None,
                    );
                }

                let subject_pointer = format!("{pointer}/subject");
                let subject = string_field(card, "subject").map(str::to_string);
                if subject.is_none() {
                    self.push(
                        Severity::Error,
                        "deck.subject_invalid",
                        "deck `subject` must be a canonical setting, character, entity, or command ID".to_string(),
                        &path,
                        Some(subject_pointer.clone()),
                        None,
                        None,
                    );
                } else if let Some(subject) = subject.as_ref() {
                    match self.definitions.get(subject) {
                        None => self.push(
                            Severity::Error,
                            "deck.subject_unknown",
                            format!("deck subject `{subject}` is not defined by this story"),
                            &path,
                            Some(subject_pointer.clone()),
                            locate_scalar(&source, subject),
                            Some(subject.clone()),
                        ),
                        Some(definition)
                            if !matches!(
                                definition.kind,
                                Kind::Setting | Kind::Character | Kind::Entity | Kind::Command
                            ) =>
                        {
                            self.push(
                                Severity::Error,
                                "deck.subject_unsupported",
                                format!("{} `{subject}` cannot be bound to a physical card; use a setting, character, entity, or command", definition.kind.name()),
                                &path,
                                Some(subject_pointer.clone()),
                                locate_scalar(&source, subject),
                                Some(subject.clone()),
                            );
                        }
                        Some(_) => {}
                    }
                }

                if let Some(tag_id) =
                    tag_id.filter(|id| (0..=TAG_STANDARD_41H12_MAX_ID).contains(id))
                {
                    let range = None;
                    if let Some((first_subject, first_pointer, first_range)) =
                        seen_tags.get(&tag_id)
                    {
                        self.diagnostics.push(Diagnostic {
                            severity: Severity::Error,
                            code: "deck.tag_id_duplicate".to_string(),
                            message: format!("tagStandard41h12 ID {tag_id} is assigned to both `{first_subject}` and `{}`", subject.as_deref().unwrap_or("an invalid subject")),
                            path: path.clone(),
                            pointer: Some(tag_pointer),
                            range,
                            subject_id: subject.clone(),
                            related: vec![RelatedLocation { message: format!("first assigned to `{first_subject}` here"), path: path.clone(), pointer: Some(first_pointer.clone()), range: *first_range }],
                        });
                    } else {
                        seen_tags.insert(
                            tag_id,
                            (subject.clone().unwrap_or_default(), tag_pointer, range),
                        );
                    }
                }
                if let Some(subject) = subject {
                    let range = locate_scalar(&source, &subject);
                    if let Some((first_path, first_pointer, first_range)) =
                        seen_subjects.get(&subject)
                    {
                        self.diagnostics.push(Diagnostic {
                            severity: Severity::Error,
                            code: "deck.subject_duplicate".to_string(),
                            message: format!("deck subject `{subject}` is bound more than once"),
                            path: path.clone(),
                            pointer: Some(subject_pointer),
                            range,
                            subject_id: Some(subject.clone()),
                            related: vec![RelatedLocation {
                                message: "first bound here".to_string(),
                                path: first_path.clone(),
                                pointer: Some(first_pointer.clone()),
                                range: *first_range,
                            }],
                        });
                    } else {
                        seen_subjects.insert(subject, (path.clone(), subject_pointer, range));
                    }
                }
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

    fn validate_point_awards(&mut self, owner_groups: &[&[Item]]) {
        for owner in owner_groups.iter().flat_map(|items| items.iter()) {
            let Some(value) = owner.mapping.get(Value::String("points".to_string())) else {
                continue;
            };
            let pointer = format!("{}/points", owner.pointer);
            let Some(points) = value.as_mapping() else {
                self.push(
                    Severity::Error,
                    "points.type",
                    "`points` must be a mapping".to_string(),
                    &owner.path,
                    Some(pointer),
                    None,
                    Some(owner.id.clone()),
                );
                continue;
            };
            for key in points.keys().filter_map(Value::as_str) {
                if !matches!(key, "value" | "max_claim_count" | "requires") {
                    self.push(
                        Severity::Error,
                        "points.unknown_field",
                        format!("unknown point-award field `{key}`"),
                        &owner.path,
                        Some(format!("{pointer}/{}", escape_pointer(key))),
                        None,
                        Some(owner.id.clone()),
                    );
                }
            }
            for (field, required) in [("value", true), ("max_claim_count", false)] {
                match points.get(Value::String(field.to_string())) {
                    Some(Value::Number(number))
                        if number.as_u64().is_some_and(|value| value > 0) => {}
                    None if !required => {}
                    _ => self.push(
                        Severity::Error,
                        if field == "value" {
                            "points.value"
                        } else {
                            "points.max_claim_count"
                        },
                        format!("point award `{field}` must be a positive whole number"),
                        &owner.path,
                        Some(format!("{pointer}/{field}")),
                        None,
                        Some(owner.id.clone()),
                    ),
                }
            }
            if let Some(requires) = points.get(Value::String("requires".to_string())) {
                if !is_string_sequence(requires) {
                    self.push(
                        Severity::Error,
                        "points.requires_type",
                        "point award `requires` must be a sequence of persistent requirement IDs"
                            .to_string(),
                        &owner.path,
                        Some(format!("{pointer}/requires")),
                        None,
                        Some(owner.id.clone()),
                    );
                }
            }
        }
    }

    fn validate_disallowed_point_owners(&mut self, owner_groups: &[&[Item]]) {
        for owner in owner_groups.iter().flat_map(|items| items.iter()) {
            if owner
                .mapping
                .contains_key(Value::String("points".to_string()))
            {
                self.push(
                    Severity::Error,
                    "points.owner",
                    "`points` is supported only on settings, entities, deductions, and commands"
                        .to_string(),
                    &owner.path,
                    Some(format!("{}/points", owner.pointer)),
                    None,
                    Some(owner.id.clone()),
                );
            }
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
            self.validate_character_voice_id(character);
            self.validate_character_portrayal(character);
            self.validate_character_testimony(character);
            if self.is_format_3_1_or_later() {
                self.validate_character_placement(character);
            }

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

    fn validate_character_placement(&mut self, character: &Item) {
        let initial_location = match character.mapping.get(Value::String("initial".to_string())) {
            None => None,
            Some(initial) => {
                let pointer = format!("{}/initial", character.pointer);
                let Some(initial) = initial.as_mapping() else {
                    self.push(
                        Severity::Error,
                        "character.initial_type",
                        "character `initial` must be a mapping containing `location`".to_string(),
                        &character.path,
                        Some(pointer),
                        None,
                        Some(character.id.clone()),
                    );
                    return;
                };
                match initial.get(Value::String("location".to_string())) {
                    Some(Value::String(location)) if !location.trim().is_empty() => {
                        Some(location.as_str())
                    }
                    Some(_) => {
                        self.push(
                            Severity::Error,
                            "character.location_type",
                            "character `initial.location` must be a non-empty setting ID"
                                .to_string(),
                            &character.path,
                            Some(format!("{pointer}/location")),
                            None,
                            Some(character.id.clone()),
                        );
                        None
                    }
                    None => {
                        self.push(
                            Severity::Error,
                            "character.location_missing",
                            "character `initial` must declare a setting `location`".to_string(),
                            &character.path,
                            Some(format!("{pointer}/location")),
                            None,
                            Some(character.id.clone()),
                        );
                        None
                    }
                }
            }
        };

        let Some(presence) = character.mapping.get(Value::String("presence".to_string())) else {
            return;
        };
        let pointer = format!("{}/presence", character.pointer);
        if initial_location.is_none() {
            self.push(
                Severity::Error,
                "character.presence_without_location",
                "character `presence` requires an authoritative `initial.location`; add a setting location or remove the presence gate"
                    .to_string(),
                &character.path,
                Some(pointer.clone()),
                None,
                Some(character.id.clone()),
            );
        }
        let Some(presence) = presence.as_mapping() else {
            self.push(
                Severity::Error,
                "character.presence_type",
                "character `presence` must be a mapping containing `requires`".to_string(),
                &character.path,
                Some(pointer),
                None,
                Some(character.id.clone()),
            );
            return;
        };
        let requires_pointer = format!("{pointer}/requires");
        match presence.get(Value::String("requires".to_string())) {
            Some(Value::String(id)) if !id.trim().is_empty() => {}
            Some(Value::Sequence(values)) if !values.is_empty() => {
                for (index, value) in values.iter().enumerate() {
                    if !value
                        .as_str()
                        .is_some_and(|requirement| !requirement.trim().is_empty())
                    {
                        self.push(
                            Severity::Error,
                            "character.presence_requirement_type",
                            "character presence requirements must be non-empty IDs".to_string(),
                            &character.path,
                            Some(format!("{requires_pointer}/{index}")),
                            None,
                            Some(character.id.clone()),
                        );
                    }
                }
            }
            _ => self.push(
                Severity::Error,
                "character.presence_requires_type",
                "character `presence.requires` must be one persistent requirement ID or a non-empty list of unique IDs"
                    .to_string(),
                &character.path,
                Some(requires_pointer),
                None,
                Some(character.id.clone()),
            ),
        }
    }

    fn validate_character_voice_id(&mut self, character: &Item) {
        let Some(voice_id) = character.mapping.get(Value::String("voice_id".to_string())) else {
            return;
        };
        let valid = voice_id.as_str().is_some_and(|voice_id| {
            !voice_id.is_empty()
                && voice_id.len() <= 128
                && voice_id.trim() == voice_id
                && voice_id.chars().all(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
                })
        });
        if !valid {
            self.push(
                Severity::Error,
                "character.voice_id",
                "character `voice_id` must be a 1–128 character ElevenLabs voice ID containing only ASCII letters, numbers, `-`, or `_`"
                    .to_string(),
                &character.path,
                Some(format!("{}/voice_id", character.pointer)),
                None,
                Some(character.id.clone()),
            );
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

    fn validate_fact_values(
        &mut self,
        facts: &[Item],
        commands: &[Item],
        fact_claims_enabled: bool,
    ) {
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
                        "format 3 facts enter the notebook automatically; omit `requires` to add a fact when the player joins".to_string(),
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
                if fact
                    .mapping
                    .contains_key(Value::String("requires".to_string()))
                {
                    self.push(
                        Severity::Error,
                        "fact.requires_removed",
                        "fact `requires` has been replaced by structured `on` action matching and `when.all` persistent conditions".to_string(),
                        &fact.path,
                        Some(format!("{}/requires", fact.pointer)),
                        None,
                        Some(fact.id.clone()),
                    );
                }
                let owner = self.fact_owner(fact);
                self.validate_action_match(fact, owner.as_ref(), false, commands);
                self.validate_persistent_when(
                    &fact.mapping,
                    &fact.path,
                    &fact.pointer,
                    &fact.id,
                    owner.as_ref(),
                );
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
                    "format 3 deductions use `inputs`; clue-based `supported_by` was removed"
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
            if self.is_format_3() {
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
        let parameter_shapes = command_parameter_types(command);
        let first_shape = parameter_shapes.first().and_then(Option::as_ref);
        if !matches!(first_shape, Some(shape)
            if shape.types == [CommandParameterType::Character]
                && shape.min == 1
                && shape.max == 1)
        {
            self.push(
                Severity::Error,
                "character.testimony_question_target_type",
                "the first `command.question` parameter must have type `character`".to_string(),
                &command.path,
                Some(format!("{first_pointer}/types")),
                None,
                Some(command.id.clone()),
            );
        }
        if !matches!(first_shape, Some(shape) if shape.min == 1 && shape.max == 1) {
            self.push(
                Severity::Error,
                "character.testimony_question_target_required",
                "the first `command.question` character parameter must be required".to_string(),
                &command.path,
                Some(format!("{first_pointer}/min")),
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
            if string_field(parameter, "name") != Some("topic") {
                self.push(
                    Severity::Error,
                    "character.testimony_question_topic_name",
                    "the optional `command.question` parameter must be named `topic`".to_string(),
                    &command.path,
                    Some(format!("{pointer}/name")),
                    None,
                    Some(command.id.clone()),
                );
                continue;
            }
            let shape = parameter_shapes.get(index).and_then(Option::as_ref);
            if !matches!(shape, Some(shape) if shape.types
            == [
                CommandParameterType::Character,
                CommandParameterType::Setting,
                CommandParameterType::Event,
                CommandParameterType::Entity,
                CommandParameterType::Deduction,
            ]) {
                self.push(
                    Severity::Error,
                    "character.testimony_question_topic_type",
                    "the canonical `topic` parameter must accept character, setting, event, entity, and deduction selections in that order".to_string(),
                    &command.path,
                    Some(format!("{pointer}/types")),
                    None,
                    Some(command.id.clone()),
                );
            }
            if !matches!(shape, Some(shape) if shape.min == 0) {
                self.push(
                    Severity::Error,
                    "character.testimony_question_topic_required",
                    "later `command.question` topic parameters must be optional".to_string(),
                    &command.path,
                    Some(format!("{pointer}/min")),
                    None,
                    Some(command.id.clone()),
                );
            }
        }
    }

    fn validate_command_parameters(
        &mut self,
        command: &Item,
    ) -> Vec<Option<CommandParameterShape>> {
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
                if self.is_format_3_1_or_later() {
                    self.validate_mapping_fields(
                        parameter,
                        &["name", "description", "types", "min", "max", "candidates"],
                        "command.parameter",
                        &command.path,
                        &command.source,
                        &pointer,
                        Some(&command.id),
                    );
                }
                if parameter.contains_key(Value::String("accepts".to_string())) {
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
                let legacy_type = string_field(parameter, "type")
                    .and_then(CommandParameterType::parse);
                if self.is_format_3()
                    && parameter.contains_key(Value::String("type".to_string()))
                {
                    self.push(
                        Severity::Error,
                        "command.parameter_type_removed",
                        "format 3 command parameters use ordered `types`; legacy singular `type` was removed"
                            .to_string(),
                        &command.path,
                        Some(format!("{pointer}/type")),
                        None,
                        Some(command.id.clone()),
                    );
                }
                let types_value = parameter.get(Value::String("types".to_string()));
                if legacy_type.is_some() && types_value.is_some() {
                    self.push(
                        Severity::Error,
                        "command.parameter_kind",
                        "command parameter must use either legacy `type` or canonical `types`, not both".to_string(),
                        &command.path,
                        Some(format!("{pointer}/types")),
                        None,
                        Some(command.id.clone()),
                    );
                }
                let mut types = if self.is_format_3() {
                    Vec::new()
                } else {
                    legacy_type.into_iter().collect::<Vec<_>>()
                };
                if let Some(types_value) = types_value {
                    let Some(values) = types_value.as_sequence() else {
                        self.push(
                            Severity::Error,
                            "command.parameter_types_type",
                            "command parameter `types` must be a non-empty sequence".to_string(),
                            &command.path,
                            Some(format!("{pointer}/types")),
                            None,
                            Some(command.id.clone()),
                        );
                        return None;
                    };
                    let mut seen_types = HashSet::new();
                    for (type_index, value) in values.iter().enumerate() {
                        let parsed = value.as_str().and_then(CommandParameterType::parse);
                        let Some(parsed) = parsed else {
                            self.push(
                                Severity::Error,
                                "command.parameter_kind",
                                "command parameter kinds must be `character`, `entity`, `setting`, `deduction`, or `event`".to_string(),
                                &command.path,
                                Some(format!("{pointer}/types/{type_index}")),
                                None,
                                Some(command.id.clone()),
                            );
                            continue;
                        };
                        if !seen_types.insert(parsed) {
                            self.push(
                                Severity::Error,
                                "command.parameter_kind_duplicate",
                                "command parameter `types` must not contain duplicate kinds".to_string(),
                                &command.path,
                                Some(format!("{pointer}/types/{type_index}")),
                                None,
                                Some(command.id.clone()),
                            );
                            continue;
                        }
                        types.push(parsed);
                    }
                    if values.is_empty() {
                        self.push(
                            Severity::Error,
                            "command.parameter_types_empty",
                            "command parameter `types` must contain at least one kind".to_string(),
                            &command.path,
                            Some(format!("{pointer}/types")),
                            None,
                            Some(command.id.clone()),
                        );
                    }
                }
                if types.is_empty() {
                    self.push(
                        Severity::Error,
                        "command.parameter_kind",
                        "command parameter must declare `types`".to_string(),
                        &command.path,
                        Some(format!("{pointer}/types")),
                        None,
                        Some(command.id.clone()),
                    );
                }
                if self.is_format_3()
                    && parameter.contains_key(Value::String("required".to_string()))
                {
                    self.push(
                        Severity::Error,
                        "command.parameter_required_removed",
                        "format 3 command parameters use `min` and `max`; legacy `required` was removed"
                            .to_string(),
                        &command.path,
                        Some(format!("{pointer}/required")),
                        None,
                        Some(command.id.clone()),
                    );
                } else if parameter
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
                let required = !self.is_format_3()
                    && bool_field(parameter, "required").unwrap_or(false);
                let min = integer_field(parameter, "min")
                    .and_then(|value| usize::try_from(value).ok())
                    .unwrap_or(usize::from(required));
                let max = integer_field(parameter, "max")
                    .and_then(|value| usize::try_from(value).ok())
                    .unwrap_or(1);
                for field in ["min", "max"] {
                    if parameter
                        .get(Value::String(field.to_string()))
                        .is_some_and(|value| value.as_i64().is_none())
                    {
                        self.push(
                            Severity::Error,
                            "command.parameter_cardinality_type",
                            format!("command parameter `{field}` must be a non-negative integer"),
                            &command.path,
                            Some(format!("{pointer}/{field}")),
                            None,
                            Some(command.id.clone()),
                        );
                    }
                }
                if max == 0 || min > max {
                    self.push(
                        Severity::Error,
                        "command.parameter_cardinality",
                        "command parameter cardinality must satisfy 0 <= min <= max and max >= 1".to_string(),
                        &command.path,
                        Some(format!("{pointer}/max")),
                        None,
                        Some(command.id.clone()),
                    );
                }
                if parameter.contains_key(Value::String("required".to_string()))
                    && (parameter.contains_key(Value::String("min".to_string()))
                        || parameter.contains_key(Value::String("max".to_string())))
                {
                    self.push(
                        Severity::Error,
                        "command.parameter_cardinality_legacy",
                        "legacy `required` cannot be combined with canonical `min` or `max`".to_string(),
                        &command.path,
                        Some(format!("{pointer}/required")),
                        None,
                        Some(command.id.clone()),
                    );
                }
                self.validate_command_parameter_candidates(command, parameter, &pointer, &types);
                (!types.is_empty()).then_some(CommandParameterShape { types, min, max })
            })
            .collect()
    }

    fn validate_command_parameter_candidates(
        &mut self,
        command: &Item,
        parameter: &Mapping,
        parameter_pointer: &str,
        parameter_types: &[CommandParameterType],
    ) {
        let Some(candidates) = parameter.get(Value::String("candidates".to_string())) else {
            return;
        };
        let pointer = format!("{parameter_pointer}/candidates");
        if !self.is_format_3_1_or_later() {
            self.push(
                Severity::Error,
                "command.parameter_candidates_version",
                "declarative command candidates require story format 3.1.0 or later".to_string(),
                &command.path,
                Some(pointer),
                None,
                Some(command.id.clone()),
            );
            return;
        }
        let Some(candidates) = candidates.as_mapping() else {
            self.push(
                Severity::Error,
                "command.candidates_type",
                "command parameter `candidates` must be a mapping".to_string(),
                &command.path,
                Some(pointer),
                None,
                Some(command.id.clone()),
            );
            return;
        };
        self.validate_mapping_fields(
            candidates,
            &["from", "capabilities"],
            "command.candidates",
            &command.path,
            &command.source,
            &pointer,
            Some(&command.id),
        );

        let from_pointer = format!("{pointer}/from");
        let mut parsed_sources = Vec::new();
        match candidates.get(Value::String("from".to_string())) {
            Some(Value::Sequence(sources)) if !sources.is_empty() => {
                let mut seen = HashSet::new();
                for (index, source) in sources.iter().enumerate() {
                    let item_pointer = format!("{from_pointer}/{index}");
                    let Some(source_name) = source.as_str() else {
                        self.push(
                            Severity::Error,
                            "command.candidates_source_unknown",
                            "candidate sources must be `all`, `current_location`, `inventory`, `reachable`, `known`, or `established`"
                                .to_string(),
                            &command.path,
                            Some(item_pointer),
                            None,
                            Some(command.id.clone()),
                        );
                        continue;
                    };
                    let Some(source) = CandidateSource::parse(source_name) else {
                        self.push(
                            Severity::Error,
                            "command.candidates_source_unknown",
                            format!("unknown candidate source `{source_name}`; use `all`, `current_location`, `inventory`, `reachable`, `known`, or `established`"),
                            &command.path,
                            Some(item_pointer),
                            None,
                            Some(command.id.clone()),
                        );
                        continue;
                    };
                    if !seen.insert(source) {
                        self.push(
                            Severity::Error,
                            "command.candidates_source_duplicate",
                            format!("candidate source `{source_name}` occurs more than once"),
                            &command.path,
                            Some(item_pointer),
                            None,
                            Some(command.id.clone()),
                        );
                        continue;
                    }
                    if !parameter_types.is_empty()
                        && !source
                            .produced_types()
                            .iter()
                            .any(|candidate_type| parameter_types.contains(candidate_type))
                    {
                        self.push(
                            Severity::Error,
                            "command.candidates_source_incompatible",
                            format!(
                                "candidate source `{source_name}` cannot produce any of this parameter's allowed types"
                            ),
                            &command.path,
                            Some(item_pointer),
                            None,
                            Some(command.id.clone()),
                        );
                    }
                    parsed_sources.push(source);
                }
            }
            Some(Value::Sequence(_)) | None => self.push(
                Severity::Error,
                "command.candidates_from_empty",
                "command parameter `candidates.from` must be a non-empty ordered set".to_string(),
                &command.path,
                Some(from_pointer),
                None,
                Some(command.id.clone()),
            ),
            Some(_) => self.push(
                Severity::Error,
                "command.candidates_from_type",
                "command parameter `candidates.from` must be a non-empty sequence".to_string(),
                &command.path,
                Some(from_pointer),
                None,
                Some(command.id.clone()),
            ),
        }

        let Some(capabilities) = candidates.get(Value::String("capabilities".to_string())) else {
            return;
        };
        let capabilities_pointer = format!("{pointer}/capabilities");
        let Value::Sequence(capabilities) = capabilities else {
            self.push(
                Severity::Error,
                "command.candidates_capabilities_type",
                "command parameter candidate `capabilities` must be a sequence".to_string(),
                &command.path,
                Some(capabilities_pointer),
                None,
                Some(command.id.clone()),
            );
            return;
        };
        if capabilities.is_empty() {
            self.push(
                Severity::Error,
                "command.candidates_capabilities_empty",
                "command parameter candidate `capabilities` must be omitted or non-empty"
                    .to_string(),
                &command.path,
                Some(capabilities_pointer),
                None,
                Some(command.id.clone()),
            );
            return;
        }
        let mut seen = HashSet::new();
        for (index, capability) in capabilities.iter().enumerate() {
            let item_pointer = format!("{capabilities_pointer}/{index}");
            let Some(capability) = capability.as_str() else {
                self.push(
                    Severity::Error,
                    "command.candidates_capability_unknown",
                    "candidate capabilities must be `portable`".to_string(),
                    &command.path,
                    Some(item_pointer),
                    None,
                    Some(command.id.clone()),
                );
                continue;
            };
            if capability != "portable" {
                self.push(
                    Severity::Error,
                    "command.candidates_capability_unknown",
                    format!("unknown candidate capability `{capability}`; format 3.1 supports only `portable`"),
                    &command.path,
                    Some(item_pointer),
                    None,
                    Some(command.id.clone()),
                );
                continue;
            }
            if !seen.insert(capability) {
                self.push(
                    Severity::Error,
                    "command.candidates_capability_duplicate",
                    "candidate capability `portable` occurs more than once".to_string(),
                    &command.path,
                    Some(item_pointer),
                    None,
                    Some(command.id.clone()),
                );
                continue;
            }
            let type_compatible = parameter_types.contains(&CommandParameterType::Entity);
            let source_compatible = parsed_sources.is_empty()
                || parsed_sources.iter().any(|source| {
                    source
                        .produced_types()
                        .contains(&CommandParameterType::Entity)
                });
            if !type_compatible || !source_compatible {
                self.push(
                    Severity::Error,
                    "command.candidates_capability_incompatible",
                    "candidate capability `portable` requires an entity parameter and a source that can produce entities"
                        .to_string(),
                    &command.path,
                    Some(item_pointer),
                    None,
                    Some(command.id.clone()),
                );
            }
        }
    }

    fn validate_runtime_command_signature(
        &mut self,
        command: &Item,
        parameter_types: &[Option<CommandParameterShape>],
    ) {
        if matches!(command.id.as_str(), "command.take" | "command.drop") {
            self.validate_inventory_command_signature(command, parameter_types);
            return;
        }
        let known = parameter_types.iter().flatten().collect::<Vec<_>>();
        let valid = match command.id.as_str() {
            "command.claim" | "command.deduce" => known.is_empty(),
            "command.move" => {
                matches!(known.as_slice(), [shape] if shape.types == [CommandParameterType::Setting] && shape.min == 1 && shape.max == 1)
            }
            "command.solve" => {
                if self.uses_question_solution_ruleset() {
                    known.is_empty()
                } else {
                    matches!(known.as_slice(), [suspect, theory]
                        if suspect.types == [CommandParameterType::Character] && suspect.min == 1 && suspect.max == 1
                            && theory.types == [CommandParameterType::Deduction] && theory.min == 1 && theory.max == 1)
                }
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
                        if self.uses_question_solution_ruleset() {
                            "ruleset.standard_mystery@3.0.0 `command.solve` must not declare parameters; answers come from `solution.questions`"
                                .to_string()
                        } else {
                            "`command.solve` must declare character then deduction parameters"
                                .to_string()
                        }
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
        parameter_types: &[Option<CommandParameterShape>],
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
        if !matches!(parameter_types, [Some(shape)] if shape.types == [CommandParameterType::Entity] && shape.min == 1 && shape.max == 1)
        {
            let field = parameter
                .as_mapping()
                .map(|mapping| {
                    if mapping.contains_key(Value::String("types".to_string())) {
                        if mapping
                            .get(Value::String("types".to_string()))
                            .and_then(Value::as_sequence)
                            .is_some_and(|types| {
                                types.len() == 1 && types[0].as_str() == Some("entity")
                            })
                        {
                            "min"
                        } else {
                            "types"
                        }
                    } else if string_field(mapping, "type") == Some("entity") {
                        "required"
                    } else {
                        "type"
                    }
                })
                .unwrap_or("types");
            self.push_inventory_signature_error(command, format!("{parameters_pointer}/0/{field}"));
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
        parameter_types: &[Option<CommandParameterShape>],
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
        parameter_types: &[Option<CommandParameterShape>],
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
        parameter_types: &[Option<CommandParameterShape>],
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
                let Some(parameter) = parameter_types.get(index).and_then(Option::as_ref) else {
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
                if parameter.max != 1
                    || !parameter
                        .types
                        .iter()
                        .all(|parameter_type| expected.contains(&parameter_type.kind()))
                {
                    self.push(
                        Severity::Error,
                        "command.effect_parameter_type",
                        format!(
                            "`{reference}` accepts {} with cardinality {}..{}; expected one {}",
                            parameter
                                .types
                                .iter()
                                .map(|parameter_type| parameter_type.name())
                                .collect::<Vec<_>>()
                                .join(" or "),
                            parameter.min,
                            parameter.max,
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

    fn fact_owner(&self, fact: &Item) -> Option<Definition> {
        let (owner_pointer, _) = fact.pointer.rsplit_once("/facts/")?;
        self.definitions
            .values()
            .find(|definition| definition.pointer == owner_pointer)
            .cloned()
    }

    fn validate_action_match(
        &mut self,
        item: &Item,
        owner: Option<&Definition>,
        required: bool,
        commands: &[Item],
    ) {
        let Some(value) = item.mapping.get(Value::String("on".to_string())) else {
            if required {
                self.push(
                    Severity::Error,
                    "action_match.missing",
                    "`on` must identify the command this trigger matches".to_string(),
                    &item.path,
                    Some(format!("{}/on", item.pointer)),
                    None,
                    Some(item.id.clone()),
                );
            }
            return;
        };
        let pointer = format!("{}/on", item.pointer);
        let Some(on) = value.as_mapping() else {
            self.push(
                Severity::Error,
                "action_match.type",
                "`on` must be a mapping with `command` and optional `parameters` or `actor`"
                    .to_string(),
                &item.path,
                Some(pointer),
                None,
                Some(item.id.clone()),
            );
            return;
        };
        for key in on.keys().filter_map(Value::as_str) {
            if !matches!(key, "command" | "parameters" | "actor") {
                self.push(
                    Severity::Error,
                    "action_match.unknown_field",
                    format!("unknown action-match field `{key}`"),
                    &item.path,
                    Some(format!("{pointer}/{}", escape_pointer(key))),
                    None,
                    Some(item.id.clone()),
                );
            }
        }
        let command_id = string_field(on, "command").filter(|id| !id.trim().is_empty());
        let command = command_id.and_then(|id| {
            self.definitions
                .get(id)
                .filter(|definition| definition.kind == Kind::Command)
        });
        if command_id.is_none() {
            self.push(
                Severity::Error,
                "action_match.command",
                "`on.command` must be a command ID".to_string(),
                &item.path,
                Some(format!("{pointer}/command")),
                None,
                Some(item.id.clone()),
            );
        } else if command.is_none() {
            let id = command_id.unwrap();
            let code = if self.definitions.contains_key(id) {
                "reference.wrong_type"
            } else {
                "reference.unknown"
            };
            self.push(
                Severity::Error,
                code,
                format!("`{id}` must refer to a command"),
                &item.path,
                Some(format!("{pointer}/command")),
                locate_scalar(&item.source, id),
                Some(id.to_string()),
            );
        }
        if let Some(actor) = on.get(Value::String("actor".to_string())) {
            let actor_pointer = format!("{pointer}/actor");
            let Some(actor) = actor.as_str().filter(|id| !id.trim().is_empty()) else {
                self.push(
                    Severity::Error,
                    "action_match.actor",
                    "`on.actor` must be an authored character ID".to_string(),
                    &item.path,
                    Some(actor_pointer),
                    None,
                    Some(item.id.clone()),
                );
                return;
            };
            match self.definitions.get(actor) {
                Some(definition) if definition.kind == Kind::Character => {}
                Some(_) => self.push(
                    Severity::Error,
                    "reference.wrong_type",
                    format!("`{actor}` must refer to a character actor"),
                    &item.path,
                    Some(actor_pointer),
                    locate_scalar(&item.source, actor),
                    Some(actor.to_string()),
                ),
                None => self.push(
                    Severity::Error,
                    "reference.unknown",
                    format!("reference `{actor}` is not defined"),
                    &item.path,
                    Some(actor_pointer),
                    locate_scalar(&item.source, actor),
                    Some(actor.to_string()),
                ),
            }
        }
        let Some(raw_bindings) = on.get(Value::String("parameters".to_string())) else {
            return;
        };
        let bindings_pointer = format!("{pointer}/parameters");
        let Some(bindings) = raw_bindings.as_mapping() else {
            self.push(
                Severity::Error,
                "action_match.parameters_type",
                "`on.parameters` must map semantic command parameter names to an ID, `owner`, or a non-empty ID list".to_string(),
                &item.path,
                Some(bindings_pointer),
                None,
                Some(item.id.clone()),
            );
            return;
        };
        let parameter_shapes = command_id
            .and_then(|id| commands.iter().find(|command| command.id == id))
            .map(|command| {
                let shapes = command_parameter_types(command);
                command
                    .mapping
                    .get(Value::String("parameters".to_string()))
                    .and_then(Value::as_sequence)
                    .into_iter()
                    .flatten()
                    .enumerate()
                    .filter_map(|(index, parameter)| {
                        let parameter = parameter.as_mapping()?;
                        Some((
                            string_field(parameter, "name")?.to_string(),
                            shapes.get(index)?.as_ref()?.clone(),
                        ))
                    })
                    .collect::<BTreeMap<_, _>>()
            })
            .unwrap_or_default();
        let mut owner_bound = false;
        for (raw_name, raw_binding) in bindings {
            let Some(name) = raw_name.as_str().filter(|name| !name.trim().is_empty()) else {
                self.push(
                    Severity::Error,
                    "action_match.parameter_name",
                    "action parameter binding names must be non-empty strings".to_string(),
                    &item.path,
                    Some(bindings_pointer.clone()),
                    None,
                    Some(item.id.clone()),
                );
                continue;
            };
            let binding_pointer = format!("{bindings_pointer}/{}", escape_pointer(name));
            let Some(shape) = parameter_shapes.get(name) else {
                self.push(
                    Severity::Error,
                    "action_match.parameter_unknown",
                    format!(
                        "`{name}` is not a parameter of `{}`",
                        command_id.unwrap_or("the command")
                    ),
                    &item.path,
                    Some(binding_pointer),
                    None,
                    Some(item.id.clone()),
                );
                continue;
            };
            let values: Vec<&str> = match raw_binding {
                Value::String(value) if !value.trim().is_empty() => vec![value],
                Value::Sequence(values)
                    if !values.is_empty()
                        && values.iter().all(|value| {
                            value.as_str().is_some_and(|value| !value.trim().is_empty())
                        }) =>
                {
                    values.iter().filter_map(Value::as_str).collect()
                }
                _ => {
                    self.push(
                        Severity::Error,
                        "action_match.parameter_binding",
                        format!("binding `{name}` must be an ID, `owner`, or a non-empty ID list"),
                        &item.path,
                        Some(binding_pointer),
                        None,
                        Some(item.id.clone()),
                    );
                    continue;
                }
            };
            if values.len() > shape.max {
                self.push(
                    Severity::Error,
                    "action_match.parameter_cardinality",
                    format!(
                        "binding `{name}` selects {} values, but the command accepts at most {}",
                        values.len(),
                        shape.max
                    ),
                    &item.path,
                    Some(binding_pointer.clone()),
                    None,
                    Some(item.id.clone()),
                );
            }
            let mut seen = HashSet::new();
            for (index, reference) in values.iter().enumerate() {
                owner_bound |= *reference == "owner";
                let value_pointer = if values.len() == 1 && raw_binding.as_str().is_some() {
                    binding_pointer.clone()
                } else {
                    format!("{binding_pointer}/{index}")
                };
                if !seen.insert(*reference) {
                    self.push(
                        Severity::Error,
                        "list.duplicate_reference",
                        format!("`{reference}` occurs more than once in this binding"),
                        &item.path,
                        Some(value_pointer),
                        locate_scalar(&item.source, reference),
                        Some((*reference).to_string()),
                    );
                    continue;
                }
                let actual_kind = if *reference == "owner" {
                    owner.map(|definition| definition.kind)
                } else {
                    self.definitions
                        .get(*reference)
                        .map(|definition| definition.kind)
                };
                match actual_kind {
                    Some(kind) if shape.types.iter().any(|expected| expected.kind() == kind) => {}
                    Some(kind) => self.push(
                        Severity::Error,
                        "reference.wrong_type",
                        format!(
                            "`{reference}` is a {}; parameter `{name}` accepts {}",
                            kind.name(),
                            shape
                                .types
                                .iter()
                                .map(|kind| kind.name())
                                .collect::<Vec<_>>()
                                .join(" or ")
                        ),
                        &item.path,
                        Some(value_pointer),
                        locate_scalar(&item.source, reference),
                        Some((*reference).to_string()),
                    ),
                    None => self.push(
                        Severity::Error,
                        if *reference == "owner" {
                            "action_match.owner_unavailable"
                        } else {
                            "reference.unknown"
                        },
                        if *reference == "owner" {
                            "`owner` is available only for owner-nested facts".to_string()
                        } else {
                            format!("reference `{reference}` is not defined")
                        },
                        &item.path,
                        Some(value_pointer),
                        locate_scalar(&item.source, reference),
                        Some((*reference).to_string()),
                    ),
                }
            }
        }
        if owner.is_some() && !owner_bound {
            self.push(
                Severity::Error,
                "fact.source_unbound",
                "an action-discovered fact must bind one semantic parameter to its nesting `owner`; move the fact to its actual discovery source or use `owner`"
                    .to_string(),
                &item.path,
                Some(bindings_pointer),
                None,
                Some(item.id.clone()),
            );
        }
    }

    fn validate_persistent_when(
        &mut self,
        mapping: &Mapping,
        path: &str,
        item_pointer: &str,
        subject: &str,
        owner: Option<&Definition>,
    ) {
        let Some(value) = mapping.get(Value::String("when".to_string())) else {
            return;
        };
        let pointer = format!("{item_pointer}/when");
        let Some(when) = value.as_mapping() else {
            self.push(
                Severity::Error,
                "condition.type",
                "`when` must be a mapping containing a non-empty `all` list".to_string(),
                path,
                Some(pointer),
                None,
                Some(subject.to_string()),
            );
            return;
        };
        for key in when.keys().filter_map(Value::as_str) {
            if key != "all" {
                self.push(
                    Severity::Error,
                    "condition.unknown_field",
                    format!("unknown persistent-condition field `{key}`; use `when.all`"),
                    path,
                    Some(format!("{pointer}/{}", escape_pointer(key))),
                    None,
                    Some(subject.to_string()),
                );
            }
        }
        let Some(predicates) = when
            .get(Value::String("all".to_string()))
            .and_then(Value::as_sequence)
            .filter(|values| !values.is_empty())
        else {
            self.push(
                Severity::Error,
                "condition.all_type",
                "`when.all` must be a non-empty sequence of persistent predicate mappings"
                    .to_string(),
                path,
                Some(format!("{pointer}/all")),
                None,
                Some(subject.to_string()),
            );
            return;
        };
        for (index, predicate) in predicates.iter().enumerate() {
            let predicate_pointer = format!("{pointer}/all/{index}");
            let Some(predicate) = predicate.as_mapping().filter(|mapping| mapping.len() == 1)
            else {
                self.push(
                    Severity::Error,
                    "condition.predicate_type",
                    "each persistent predicate must be a mapping with exactly one of `at`, `owns`, `knows`, `flag`, `completed`, or `time`".to_string(),
                    path,
                    Some(predicate_pointer),
                    None,
                    Some(subject.to_string()),
                );
                continue;
            };
            let (raw_kind, operand) = predicate.iter().next().unwrap();
            let Some(kind) = raw_kind.as_str() else {
                self.push(
                    Severity::Error,
                    "condition.predicate_kind",
                    "persistent predicate names must be strings".to_string(),
                    path,
                    Some(predicate_pointer),
                    None,
                    Some(subject.to_string()),
                );
                continue;
            };
            let operand_pointer = format!("{predicate_pointer}/{}", escape_pointer(kind));
            if kind == "time" {
                let Some(time) = operand.as_mapping() else {
                    self.push(
                        Severity::Error,
                        "condition.time_type",
                        "`time` must contain `relation` and quoted `value`".to_string(),
                        path,
                        Some(operand_pointer),
                        None,
                        Some(subject.to_string()),
                    );
                    continue;
                };
                if time.len() != 2
                    || !string_field(time, "relation")
                        .is_some_and(|value| matches!(value, "before" | "at" | "after"))
                    || !string_field(time, "value").is_some_and(valid_time)
                {
                    self.push(
                        Severity::Error,
                        "condition.time_value",
                        "`time` requires only `relation` (`before`, `at`, or `after`) and a quoted HH:MM `value`".to_string(),
                        path,
                        Some(operand_pointer),
                        None,
                        Some(subject.to_string()),
                    );
                }
                continue;
            }
            let expected: &[Kind] = match kind {
                "at" => &[Kind::Setting],
                "owns" => &[Kind::Entity],
                "knows" => &[Kind::Fact, Kind::Deduction],
                "flag" => &[Kind::Flag],
                "completed" => &[Kind::Trigger],
                _ => {
                    self.push(
                        Severity::Error,
                        "condition.predicate_kind",
                        format!("`{kind}` is not a supported persistent predicate kind"),
                        path,
                        Some(operand_pointer),
                        None,
                        Some(subject.to_string()),
                    );
                    continue;
                }
            };
            let Some(reference) = operand.as_str().filter(|id| !id.trim().is_empty()) else {
                self.push(
                    Severity::Error,
                    "condition.predicate_operand",
                    format!("`{kind}` must contain one authored ID"),
                    path,
                    Some(operand_pointer),
                    None,
                    Some(subject.to_string()),
                );
                continue;
            };
            let definition = if reference == "owner" {
                owner
            } else {
                self.definitions.get(reference)
            };
            match definition {
                Some(definition) if expected.contains(&definition.kind) => {}
                Some(definition) => self.push(
                    Severity::Error,
                    "reference.wrong_type",
                    format!(
                        "`{reference}` is a {}; `{kind}` expects {}",
                        definition.kind.name(),
                        expected
                            .iter()
                            .map(|kind| kind.name())
                            .collect::<Vec<_>>()
                            .join(" or ")
                    ),
                    path,
                    Some(operand_pointer),
                    None,
                    Some(reference.to_string()),
                ),
                None => self.push(
                    Severity::Error,
                    if reference == "owner" {
                        "condition.owner_unavailable"
                    } else {
                        "reference.unknown"
                    },
                    if reference == "owner" {
                        "`owner` is available only for owner-nested facts".to_string()
                    } else {
                        format!("reference `{reference}` is not defined")
                    },
                    path,
                    Some(operand_pointer),
                    None,
                    Some(reference.to_string()),
                ),
            }
        }
    }

    fn validate_trigger_values(
        &mut self,
        triggers: &[Item],
        commands: &[Item],
        fact_claims_enabled: bool,
    ) {
        if fact_claims_enabled {
            let referenced_completions = self
                .parsed
                .iter()
                .flat_map(|file| completed_trigger_references(&file.value))
                .map(str::to_string)
                .collect::<BTreeSet<_>>();
            for trigger in triggers {
                for field in [
                    "command",
                    "parameters",
                    "time",
                    "location",
                    "any_of",
                    "all_of",
                ] {
                    if trigger
                        .mapping
                        .contains_key(Value::String(field.to_string()))
                    {
                        self.push(
                            Severity::Error,
                            "trigger.legacy_match_field",
                            format!(
                                "trigger `{field}` has moved into structured `on` or `when.all`"
                            ),
                            &trigger.path,
                            Some(format!("{}/{}", trigger.pointer, escape_pointer(field))),
                            None,
                            Some(trigger.id.clone()),
                        );
                    }
                }
                self.validate_action_match(trigger, None, true, commands);
                self.validate_persistent_when(
                    &trigger.mapping,
                    &trigger.path,
                    &trigger.pointer,
                    &trigger.id,
                    None,
                );
                let after = trigger
                    .mapping
                    .get(Value::String("after".to_string()))
                    .and_then(Value::as_str);
                if trigger
                    .mapping
                    .contains_key(Value::String("after".to_string()))
                    && !after.is_some_and(valid_delay)
                {
                    self.push(
                        Severity::Error,
                        "trigger.after",
                        "trigger `after` must be a positive delay such as `20m`, `1h`, or `2turns`"
                            .to_string(),
                        &trigger.path,
                        Some(format!("{}/after", trigger.pointer)),
                        None,
                        Some(trigger.id.clone()),
                    );
                }
                let has_effect = trigger
                    .mapping
                    .get(Value::String("effects".to_string()))
                    .and_then(Value::as_sequence)
                    .is_some_and(|effects| !effects.is_empty());
                if after.is_some_and(valid_delay) && has_effect {
                    self.push(
                        Severity::Error,
                        "trigger.delayed_effects",
                        "trigger `after` currently delays nested result facts only; immediate `effects` must be omitted"
                            .to_string(),
                        &trigger.path,
                        Some(format!("{}/effects", trigger.pointer)),
                        None,
                        Some(trigger.id.clone()),
                    );
                }
                let has_result = trigger
                    .mapping
                    .get(Value::String("facts".to_string()))
                    .and_then(Value::as_sequence)
                    .is_some_and(|facts| !facts.is_empty());
                if !has_effect
                    && !has_result
                    && !referenced_completions.contains(trigger.id.as_str())
                {
                    self.push(
                        Severity::Error,
                        "trigger.no_observable_result",
                        "trigger must have an effect, a nested result fact, or a completion identity referenced by `completed`".to_string(),
                        &trigger.path,
                        Some(trigger.pointer.clone()),
                        None,
                        Some(trigger.id.clone()),
                    );
                }
                let command = trigger
                    .mapping
                    .get(Value::String("on".to_string()))
                    .and_then(Value::as_mapping)
                    .and_then(|on| string_field(on, "command"))
                    .and_then(|id| commands.iter().find(|command| command.id == id));
                let parameter_types = command.map(command_parameter_types).unwrap_or_default();
                self.validate_world_effects(trigger, &parameter_types);
            }
            return;
        }
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
            .map(|command| {
                let shapes = command_parameter_types(command);
                command
                    .mapping
                    .get(Value::String("parameters".to_string()))
                    .and_then(Value::as_sequence)
                    .into_iter()
                    .flatten()
                    .enumerate()
                    .filter_map(|(index, parameter)| {
                        let parameter = parameter.as_mapping()?;
                        Some((
                            string_field(parameter, "name")?.to_string(),
                            shapes.get(index)?.as_ref()?.types.clone(),
                        ))
                    })
                    .collect::<BTreeMap<_, _>>()
            })
            .unwrap_or_default();

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
            let Some(expected) = parameters.get(name) else {
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
                Some(definition)
                    if expected
                        .iter()
                        .any(|parameter_type| definition.kind == parameter_type.kind()) => {}
                Some(definition) => self.push(
                    Severity::Error,
                    "reference.wrong_type",
                    format!(
                        "`{reference}` refers to a {}; parameter `{name}` expects {}",
                        definition.kind.name(),
                        expected
                            .iter()
                            .map(|parameter_type| parameter_type.name())
                            .collect::<Vec<_>>()
                            .join(" or ")
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

    fn validate_graphs(&mut self, inputs: GraphInputs<'_>) {
        let GraphInputs {
            settings,
            entities,
            clues,
            facts,
            deductions,
            triggers,
            fact_claims_enabled,
        } = inputs;
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
            self.validate_cycles(
                trigger_dependency_graph(triggers),
                "trigger.reference_cycle",
                "trigger completion dependency",
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
    fn validate_reference_text(
        &mut self,
        cases: &[Item],
        settings: &[Item],
        characters: &[Item],
        entities: &[Item],
        events: &[Item],
        facts: &[Item],
        deductions: &[Item],
        flags: &[Item],
        commands: &[Item],
        triggers: &[Item],
        testimonies: &[Item],
        win_states: &[Item],
        end_states: &[Item],
    ) {
        let mut definitions = BTreeMap::new();
        for items in [
            cases,
            settings,
            characters,
            entities,
            events,
            facts,
            deductions,
            flags,
            commands,
            triggers,
            testimonies,
            win_states,
            end_states,
        ] {
            for item in items {
                if reference_kind(item.kind.name()).is_some() {
                    definitions.insert(item.id.clone(), TextDefinition::from(item));
                }
            }
        }

        let mut consumers = Vec::new();
        for items in [
            cases,
            settings,
            characters,
            entities,
            events,
            facts,
            deductions,
            flags,
            commands,
            triggers,
            testimonies,
            win_states,
            end_states,
        ] {
            for item in items {
                collect_item_text_consumers(item, &mut consumers);
            }
        }
        collect_nested_command_text(commands, &mut consumers);
        collect_nested_command_text(triggers, &mut consumers);
        for file in &self.parsed {
            let Some(solution) = file
                .value
                .as_mapping()
                .and_then(|root| root.get(Value::String("solution".to_string())))
                .and_then(Value::as_mapping)
            else {
                continue;
            };
            for field in CONSUMER_FIELDS
                .iter()
                .filter(|field| field.kind == "solution")
            {
                if let Some(text) = mapping_path(solution, field.path).and_then(Value::as_str) {
                    consumers.push(TextConsumer {
                        owner_id: None,
                        path: file.path.to_string(),
                        source: file.source.to_string(),
                        pointer: format!("/solution/{}", field.path.replace('.', "/")),
                        authored: text.to_string(),
                        disclosure: field.disclosure,
                    });
                }
            }
            if let Some(questions) = solution
                .get(Value::String("questions".to_string()))
                .and_then(Value::as_sequence)
            {
                let disclosure = CONSUMER_FIELDS
                    .iter()
                    .find(|field| field.kind == "solution_question" && field.path == "prompt")
                    .expect("solution question prompt contract is registered")
                    .disclosure;
                for (index, question) in questions.iter().enumerate() {
                    if let Some(text) = question
                        .as_mapping()
                        .and_then(|question| string_field(question, "prompt"))
                    {
                        consumers.push(TextConsumer {
                            owner_id: None,
                            path: file.path.to_string(),
                            source: file.source.to_string(),
                            pointer: format!("/solution/questions/{index}/prompt"),
                            authored: text.to_string(),
                            disclosure,
                        });
                    }
                }
            }
        }

        let enabled = self
            .features
            .iter()
            .any(|feature| feature == REFERENCE_TEXT_FEATURE);
        if !enabled {
            for consumer in consumers {
                if parse_reference_text(&consumer.authored).is_ok_and(|parsed| {
                    parsed
                        .segments
                        .iter()
                        .any(|segment| matches!(segment, ReferenceTextSegment::Reference { .. }))
                }) {
                    self.push(
                        Severity::Error,
                        "reference_text.feature_required",
                        "reference expression requires `case.features: [reference_text_v1]`"
                            .to_string(),
                        &consumer.path,
                        Some(consumer.pointer.clone()),
                        locate_yaml_scalar_token(
                            &consumer.source,
                            &consumer.pointer,
                            &consumer.authored,
                            "[[",
                            0,
                        ),
                        consumer.owner_id,
                    );
                }
            }
            return;
        }

        for consumer in consumers {
            let parsed = match parse_reference_text(&consumer.authored) {
                Ok(parsed) => parsed,
                Err(error) => {
                    let (start, end) = parse_error_offsets(&error, consumer.authored.len());
                    self.push(
                        Severity::Error,
                        "reference_text.malformed",
                        error.to_string(),
                        &consumer.path,
                        Some(consumer.pointer.clone()),
                        locate_yaml_scalar_offset(
                            &consumer.source,
                            &consumer.pointer,
                            &consumer.authored,
                            start,
                            end,
                        ),
                        consumer.owner_id,
                    );
                    continue;
                }
            };
            if !parsed
                .segments
                .iter()
                .any(|segment| matches!(segment, ReferenceTextSegment::Reference { .. }))
            {
                continue;
            }
            let mut resolver = TextResolver::new(&definitions);
            let location = TextLocation {
                path: &consumer.path,
                source: &consumer.source,
                pointer: &consumer.pointer,
                authored: &consumer.authored,
            };
            match resolver.resolve_parsed(&parsed, consumer.disclosure, &mut Vec::new(), location) {
                Ok(node) => self.reference_text.push(ResolvedReferenceText {
                    path: consumer.path,
                    pointer: consumer.pointer,
                    disclosure: consumer.disclosure,
                    authored: consumer.authored,
                    resolved: node.text,
                    provenance: node.provenance,
                }),
                Err(error) => {
                    let range = error.range.or_else(|| {
                        error.expression.as_ref().and_then(|expression| {
                            locate_reference_expression(
                                &error.source,
                                &error.pointer,
                                &error.authored,
                                expression,
                            )
                        })
                    });
                    self.diagnostics.push(Diagnostic {
                        severity: Severity::Error,
                        code: error.code.to_string(),
                        message: error.message,
                        path: error.path,
                        pointer: Some(error.pointer),
                        range,
                        subject_id: consumer.owner_id,
                        related: error.related,
                    });
                }
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

#[derive(Clone)]
struct TextDefinition {
    id: String,
    kind: &'static str,
    path: String,
    source: String,
    pointer: String,
    mapping: Mapping,
}

impl From<&Item> for TextDefinition {
    fn from(item: &Item) -> Self {
        Self {
            id: item.id.clone(),
            kind: item.kind.name(),
            path: item.path.clone(),
            source: item.source.clone(),
            pointer: item.pointer.clone(),
            mapping: item.mapping.clone(),
        }
    }
}

struct TextConsumer {
    owner_id: Option<String>,
    path: String,
    source: String,
    pointer: String,
    authored: String,
    disclosure: DisclosureClass,
}

#[derive(Clone)]
struct ResolvedNode {
    text: String,
    provenance: Vec<ReferenceProvenance>,
}

struct TextResolveError {
    code: &'static str,
    message: String,
    path: String,
    source: String,
    pointer: String,
    authored: String,
    expression: Option<crate::ReferenceExpression>,
    range: Option<SourceRange>,
    related: Vec<RelatedLocation>,
}

#[derive(Clone, Copy)]
struct TextLocation<'a> {
    path: &'a str,
    source: &'a str,
    pointer: &'a str,
    authored: &'a str,
}

#[derive(Clone)]
struct ResolveEdge {
    target: (String, String),
    expression: crate::ReferenceExpression,
    path: String,
    source: String,
    pointer: String,
    authored: String,
}

impl ResolveEdge {
    fn new(
        target: (String, String),
        expression: &crate::ReferenceExpression,
        location: TextLocation<'_>,
    ) -> Self {
        Self {
            target,
            expression: expression.clone(),
            path: location.path.to_string(),
            source: location.source.to_string(),
            pointer: location.pointer.to_string(),
            authored: location.authored.to_string(),
        }
    }

    fn related_location(&self) -> RelatedLocation {
        RelatedLocation {
            message: format!(
                "cycle edge references `{}.{}`",
                self.target.0, self.target.1
            ),
            path: self.path.clone(),
            pointer: Some(self.pointer.clone()),
            range: locate_reference_expression(
                &self.source,
                &self.pointer,
                &self.authored,
                &self.expression,
            ),
        }
    }
}

struct TextResolver<'a> {
    definitions: &'a BTreeMap<String, TextDefinition>,
    memo: HashMap<(String, String, DisclosureClass), ResolvedNode>,
}

impl<'a> TextResolver<'a> {
    fn new(definitions: &'a BTreeMap<String, TextDefinition>) -> Self {
        Self {
            definitions,
            memo: HashMap::new(),
        }
    }

    fn resolve_parsed(
        &mut self,
        parsed: &crate::ParsedReferenceText,
        disclosure: DisclosureClass,
        stack: &mut Vec<ResolveEdge>,
        location: TextLocation<'_>,
    ) -> Result<ResolvedNode, Box<TextResolveError>> {
        let mut text = String::new();
        let mut provenance = Vec::new();
        for segment in &parsed.segments {
            match segment {
                ReferenceTextSegment::Literal { text: literal } => text.push_str(literal),
                ReferenceTextSegment::Reference { expression } => {
                    let Some(definition) = self.definitions.get(&expression.target_id) else {
                        return Err(text_error(
                            "reference_text.unknown_id",
                            format!(
                                "reference expression names unknown ID `{}`",
                                expression.target_id
                            ),
                            parsed,
                            expression,
                            location,
                        ));
                    };
                    let Some(kind) = reference_kind(definition.kind) else {
                        return Err(text_error(
                            "reference_text.unsupported_kind",
                            format!(
                                "`{}` is not a referenceable definition kind",
                                definition.kind
                            ),
                            parsed,
                            expression,
                            location,
                        ));
                    };
                    let property_path = if expression.property_path.is_empty() {
                        kind.default_path
                            .ok_or_else(|| {
                                text_error(
                                    "reference_text.default_missing",
                                    format!(
                                        "{} references require an explicit property path",
                                        definition.kind
                                    ),
                                    parsed,
                                    expression,
                                    location,
                                )
                            })?
                            .to_string()
                    } else {
                        expression.property_path.join(".")
                    };
                    let Some(path_contract) =
                        kind.paths.iter().find(|path| path.path == property_path)
                    else {
                        return Err(text_error(
                            "reference_text.path_disallowed",
                            format!(
                                "path `{property_path}` is not an allowed narrative path for `{}`",
                                definition.id
                            ),
                            parsed,
                            expression,
                            location,
                        ));
                    };
                    if !crate::reference_text::disclosure_allows(
                        disclosure,
                        path_contract.disclosure,
                    ) {
                        return Err(text_error(
                            "reference_text.disclosure",
                            disclosure_mismatch_message(
                                disclosure,
                                path_contract.disclosure,
                                &format!("{}.{property_path}", definition.id),
                            ),
                            parsed,
                            expression,
                            location,
                        ));
                    }
                    let key = (definition.id.clone(), property_path.clone());
                    if let Some(cycle_start) = stack.iter().position(|edge| edge.target == key) {
                        // The edge that first entered the repeated target came
                        // from outside the cycle. Participating edges begin
                        // with the following frame and end with this expression.
                        let mut related = stack[cycle_start + 1..]
                            .iter()
                            .map(ResolveEdge::related_location)
                            .collect::<Vec<_>>();
                        related.push(
                            ResolveEdge::new(key.clone(), expression, location).related_location(),
                        );
                        let mut error = text_error(
                            "reference_text.cycle",
                            format!(
                                "reference cycle detected at `{}.{property_path}`",
                                definition.id
                            ),
                            parsed,
                            expression,
                            location,
                        );
                        error.related = related;
                        return Err(error);
                    }
                    let memo_key = (definition.id.clone(), property_path.clone(), disclosure);
                    let node = if let Some(node) = self.memo.get(&memo_key) {
                        node.clone()
                    } else {
                        let Some(value) = mapping_path(&definition.mapping, &property_path) else {
                            return Err(text_error(
                                "reference_text.unknown_path",
                                format!(
                                    "`{}` has no value at allowed path `{property_path}`",
                                    definition.id
                                ),
                                parsed,
                                expression,
                                location,
                            ));
                        };
                        let Some(value) = value.as_str() else {
                            return Err(text_error(
                                "reference_text.non_string",
                                format!(
                                    "`{}.{property_path}` must be a string to be referenced",
                                    definition.id
                                ),
                                parsed,
                                expression,
                                location,
                            ));
                        };
                        if value.trim().is_empty() {
                            return Err(text_error(
                                "reference_text.empty",
                                format!(
                                    "`{}.{property_path}` cannot be empty when referenced",
                                    definition.id
                                ),
                                parsed,
                                expression,
                                location,
                            ));
                        }
                        let target_parsed = parse_reference_text(value).map_err(|error| {
                            let (start, end) = parse_error_offsets(&error, value.len());
                            Box::new(TextResolveError {
                                code: "reference_text.malformed",
                                message: format!(
                                    "malformed reference in `{}.{property_path}`: {error}",
                                    definition.id
                                ),
                                path: definition.path.clone(),
                                source: definition.source.clone(),
                                pointer: format!(
                                    "{}/{}",
                                    definition.pointer,
                                    property_path.replace('.', "/")
                                ),
                                authored: value.to_string(),
                                expression: None,
                                range: locate_yaml_scalar_offset(
                                    &definition.source,
                                    &format!(
                                        "{}/{}",
                                        definition.pointer,
                                        property_path.replace('.', "/")
                                    ),
                                    value,
                                    start,
                                    end,
                                ),
                                related: Vec::new(),
                            })
                        })?;
                        stack.push(ResolveEdge::new(key.clone(), expression, location));
                        let target_pointer =
                            format!("{}/{}", definition.pointer, property_path.replace('.', "/"));
                        let target_location = TextLocation {
                            path: &definition.path,
                            source: &definition.source,
                            pointer: &target_pointer,
                            authored: value,
                        };
                        let result =
                            self.resolve_parsed(&target_parsed, disclosure, stack, target_location);
                        stack.pop();
                        let node = result?;
                        self.memo.insert(memo_key, node.clone());
                        node
                    };
                    text.push_str(&node.text);
                    provenance.push(ReferenceProvenance {
                        expression: expression.clone(),
                        path: location.path.to_string(),
                        pointer: location.pointer.to_string(),
                        range: locate_reference_expression(
                            location.source,
                            location.pointer,
                            location.authored,
                            expression,
                        ),
                        definition_pointer: format!(
                            "{}/{}",
                            definition.pointer,
                            property_path.replace('.', "/")
                        ),
                        resolved_path: property_path,
                        resolved_value: node.text.clone(),
                    });
                    provenance.extend(node.provenance);
                }
            }
        }
        Ok(ResolvedNode { text, provenance })
    }
}

fn disclosure_mismatch_message(
    consumer: DisclosureClass,
    target: DisclosureClass,
    target_path: &str,
) -> String {
    let consumer_name = match consumer {
        DisclosureClass::PlayerSafe => "baseline player-safe",
        DisclosureClass::GatedPlayerSafe => "gated player-safe",
        DisclosureClass::PrivateNarrator => "private narrator",
    };
    let target_name = match target {
        DisclosureClass::PlayerSafe => "baseline player-safe",
        DisclosureClass::GatedPlayerSafe => "gated player-safe",
        DisclosureClass::PrivateNarrator => "private narrator",
    };
    let mut message =
        format!("{consumer_name} prose cannot reference {target_name} path `{target_path}`");
    if target == DisclosureClass::GatedPlayerSafe {
        message.push_str("; this would bypass the target's disclosure gate");
    }
    message
}

fn text_error(
    code: &'static str,
    message: String,
    _parsed: &crate::ParsedReferenceText,
    expression: &crate::ReferenceExpression,
    location: TextLocation<'_>,
) -> Box<TextResolveError> {
    Box::new(TextResolveError {
        code,
        message,
        path: location.path.to_string(),
        source: location.source.to_string(),
        pointer: location.pointer.to_string(),
        authored: location.authored.to_string(),
        expression: Some(expression.clone()),
        range: None,
        related: Vec::new(),
    })
}

fn mapping_path<'a>(mapping: &'a Mapping, path: &str) -> Option<&'a Value> {
    let mut components = path.split('.');
    let first = components.next()?;
    let mut value = mapping.get(Value::String(first.to_string()))?;
    for component in components {
        value = value
            .as_mapping()?
            .get(Value::String(component.to_string()))?;
    }
    Some(value)
}

fn collect_item_text_consumers(item: &Item, consumers: &mut Vec<TextConsumer>) {
    for field in CONSUMER_FIELDS
        .iter()
        .filter(|field| field.kind == item.kind.name())
    {
        if let Some(text) = mapping_path(&item.mapping, field.path).and_then(Value::as_str) {
            consumers.push(TextConsumer {
                owner_id: Some(item.id.clone()),
                path: item.path.clone(),
                source: item.source.clone(),
                pointer: format!("{}/{}", item.pointer, field.path.replace('.', "/")),
                authored: text.to_string(),
                disclosure: field.disclosure,
            });
        }
    }
}

fn collect_nested_command_text(items: &[Item], consumers: &mut Vec<TextConsumer>) {
    for item in items {
        let parameter_disclosure = CONSUMER_FIELDS
            .iter()
            .find(|field| field.kind == "command_parameter" && field.path == "description")
            .expect("command parameter prose is registered")
            .disclosure;
        let effect_kind = if item.kind == Kind::Trigger {
            "trigger_effect"
        } else {
            "command_effect"
        };
        let effect_disclosure = CONSUMER_FIELDS
            .iter()
            .find(|field| field.kind == effect_kind && field.path == "text")
            .expect("effect prose is registered")
            .disclosure;
        if let Some(parameters) = item
            .mapping
            .get(Value::String("parameters".to_string()))
            .and_then(Value::as_sequence)
        {
            for (index, parameter) in parameters.iter().enumerate() {
                if let Some(text) = parameter
                    .as_mapping()
                    .and_then(|mapping| string_field(mapping, "description"))
                {
                    consumers.push(TextConsumer {
                        owner_id: Some(item.id.clone()),
                        path: item.path.clone(),
                        source: item.source.clone(),
                        pointer: format!("{}/parameters/{index}/description", item.pointer),
                        authored: text.to_string(),
                        disclosure: parameter_disclosure,
                    });
                }
            }
        }
        if let Some(effects) = item
            .mapping
            .get(Value::String("effects".to_string()))
            .and_then(Value::as_sequence)
        {
            for (index, effect) in effects.iter().enumerate() {
                if let Some(text) = effect
                    .as_mapping()
                    .and_then(|mapping| string_field(mapping, "text"))
                {
                    consumers.push(TextConsumer {
                        owner_id: Some(item.id.clone()),
                        path: item.path.clone(),
                        source: item.source.clone(),
                        pointer: format!("{}/effects/{index}/text", item.pointer),
                        authored: text.to_string(),
                        disclosure: effect_disclosure,
                    });
                }
            }
        }
    }
}

fn parse_error_offsets(error: &crate::ReferenceParseError, source_len: usize) -> (usize, usize) {
    match error {
        crate::ReferenceParseError::Unclosed { start } => (*start, source_len),
        crate::ReferenceParseError::Empty { start, end }
        | crate::ReferenceParseError::Invalid { start, end, .. } => (*start, *end),
        crate::ReferenceParseError::UnexpectedClose { start } => {
            (*start, (*start + 2).min(source_len))
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct YamlNodeSpan {
    start: usize,
    end: usize,
    indent: isize,
    value_start: usize,
}

#[derive(Debug, Clone, Copy)]
struct YamlLine<'a> {
    start: usize,
    indent: usize,
    text: &'a str,
}

fn yaml_lines(source: &str) -> Vec<YamlLine<'_>> {
    let mut offset = 0;
    source
        .split_inclusive('\n')
        .map(|raw| {
            let without_lf = raw.strip_suffix('\n').unwrap_or(raw);
            let text = without_lf.strip_suffix('\r').unwrap_or(without_lf);
            let line = YamlLine {
                start: offset,
                indent: text.bytes().take_while(|byte| *byte == b' ').count(),
                text,
            };
            offset += raw.len();
            line
        })
        .collect()
}

fn flow_skip_whitespace(source: &str, mut offset: usize, end: usize) -> usize {
    while offset < end && source.as_bytes()[offset].is_ascii_whitespace() {
        offset += 1;
    }
    offset
}

fn flow_top_level_delimiter(
    source: &str,
    start: usize,
    end: usize,
    delimiters: &[u8],
) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut offset = start;
    let mut depth = 0usize;
    let mut quote = None;
    while offset < end {
        let byte = bytes[offset];
        if let Some(active_quote) = quote {
            if byte == active_quote {
                if active_quote == b'\'' && bytes.get(offset + 1) == Some(&b'\'') {
                    offset += 2;
                    continue;
                }
                quote = None;
            } else if active_quote == b'"' && byte == b'\\' {
                offset += 2;
                continue;
            }
        } else {
            match byte {
                b'\'' | b'"' => quote = Some(byte),
                b'{' | b'[' => depth += 1,
                b'}' | b']' if depth > 0 => depth -= 1,
                _ if depth == 0 && delimiters.contains(&byte) => return Some(offset),
                _ => {}
            }
        }
        offset += 1;
    }
    None
}

fn flow_entries(
    source: &str,
    node: YamlNodeSpan,
    open: u8,
    close: u8,
) -> Option<Vec<(usize, usize)>> {
    let start = flow_skip_whitespace(source, node.value_start, node.end);
    if source.as_bytes().get(start) != Some(&open) {
        return None;
    }
    let mut entries = Vec::new();
    let mut entry_start = flow_skip_whitespace(source, start + 1, node.end);
    loop {
        if source.as_bytes().get(entry_start) == Some(&close) {
            break;
        }
        let entry_end = flow_top_level_delimiter(source, entry_start, node.end, &[b',', close])?;
        entries.push((entry_start, entry_end));
        if source.as_bytes()[entry_end] == close {
            break;
        }
        entry_start = flow_skip_whitespace(source, entry_end + 1, node.end);
    }
    Some(entries)
}

fn yaml_flow_child_span(source: &str, node: YamlNodeSpan, component: &str) -> Option<YamlNodeSpan> {
    let start = flow_skip_whitespace(source, node.value_start, node.end);
    match source.as_bytes().get(start)? {
        b'[' => {
            let index = component.parse::<usize>().ok()?;
            let (value_start, end) = *flow_entries(source, node, b'[', b']')?.get(index)?;
            Some(YamlNodeSpan {
                start: value_start,
                end,
                indent: node.indent,
                value_start,
            })
        }
        b'{' => {
            for (entry_start, entry_end) in flow_entries(source, node, b'{', b'}')? {
                let colon = flow_top_level_delimiter(source, entry_start, entry_end, &[b':'])?;
                let authored_key = source[entry_start..colon].trim();
                let authored_key = authored_key
                    .strip_prefix(['\'', '"'])
                    .and_then(|key| key.strip_suffix(['\'', '"']))
                    .unwrap_or(authored_key);
                if authored_key == component {
                    let value_start = flow_skip_whitespace(source, colon + 1, entry_end);
                    return Some(YamlNodeSpan {
                        start: entry_start,
                        end: entry_end,
                        indent: node.indent,
                        value_start,
                    });
                }
            }
            None
        }
        _ => None,
    }
}

fn yaml_pointer_component(component: &str) -> String {
    component.replace("~1", "/").replace("~0", "~")
}

fn yaml_line_key_value_start(line: YamlLine<'_>, key: &str) -> Option<usize> {
    let mut content = &line.text[line.indent..];
    let mut content_offset = line.start + line.indent;
    if let Some(rest) = content.strip_prefix("- ") {
        content = rest;
        content_offset += 2;
    }
    let rest = content.strip_prefix(key)?;
    let rest = rest.strip_prefix(':')?;
    let colon_offset = content_offset + key.len();
    let whitespace = rest.bytes().take_while(u8::is_ascii_whitespace).count();
    Some(colon_offset + 1 + whitespace)
}

fn yaml_node_end(
    lines: &[YamlLine<'_>],
    line_index: usize,
    indent: usize,
    fallback: usize,
    allow_indentless_sequence: bool,
) -> usize {
    lines[line_index + 1..]
        .iter()
        .find(|line| {
            let trimmed = line.text.trim();
            !trimmed.is_empty()
                && !trimmed.starts_with('#')
                && (line.indent < indent
                    || (line.indent == indent
                        && !(allow_indentless_sequence
                            && (trimmed == "-" || trimmed.starts_with("- ")))))
        })
        .map_or(fallback, |line| line.start)
}

fn yaml_scalar_span(source: &str, pointer: &str) -> Option<YamlNodeSpan> {
    let lines = yaml_lines(source);
    let mut node = YamlNodeSpan {
        start: 0,
        end: source.len(),
        indent: -1,
        value_start: 0,
    };
    for raw_component in pointer.split('/').skip(1) {
        let component = yaml_pointer_component(raw_component);
        if let Some(flow_node) = yaml_flow_child_span(source, node, &component) {
            node = flow_node;
            continue;
        }
        let sequence_index = component.parse::<usize>().ok();
        let candidates = lines
            .iter()
            .enumerate()
            .filter(|(_, line)| {
                line.start >= node.start
                    && line.start < node.end
                    && if sequence_index.is_some() {
                        line.indent as isize >= node.indent
                    } else {
                        line.indent as isize > node.indent
                    }
                    && !line.text.trim().is_empty()
                    && !line.text.trim_start().starts_with('#')
            })
            .collect::<Vec<_>>();
        if let Some(sequence_index) = sequence_index {
            let direct_indent = candidates
                .iter()
                .filter(|(_, line)| {
                    let trimmed = line.text.trim_start();
                    trimmed == "-" || trimmed.starts_with("- ")
                })
                .map(|(_, line)| line.indent)
                .min()?;
            let (line_index, line) = candidates
                .into_iter()
                .filter(|(_, line)| {
                    line.indent == direct_indent
                        && (line.text.trim_start() == "-"
                            || line.text.trim_start().starts_with("- "))
                })
                .nth(sequence_index)?;
            let dash = line.text.find('-')?;
            node = YamlNodeSpan {
                start: line.start,
                end: yaml_node_end(&lines, line_index, line.indent, node.end, false),
                indent: line.indent as isize,
                value_start: line.start + dash + 1,
            };
        } else {
            let matches = candidates
                .into_iter()
                .filter_map(|(line_index, line)| {
                    yaml_line_key_value_start(*line, &component)
                        .map(|value_start| (line_index, *line, value_start))
                })
                .collect::<Vec<_>>();
            let direct_indent = matches.iter().map(|(_, line, _)| line.indent).min()?;
            let (line_index, line, value_start) = matches
                .into_iter()
                .find(|(_, line, _)| line.indent == direct_indent)?;
            node = YamlNodeSpan {
                start: line.start,
                end: yaml_node_end(&lines, line_index, line.indent, node.end, true),
                indent: line.indent as isize,
                value_start,
            };
        }
    }
    Some(node)
}

fn source_range(source: &str, start: usize, end: usize) -> SourceRange {
    let position = |offset: usize| {
        let prefix = &source[..offset];
        Position {
            line: prefix.bytes().filter(|byte| *byte == b'\n').count() + 1,
            column: prefix
                .rsplit_once('\n')
                .map_or(prefix.len() + 1, |(_, tail)| tail.len() + 1),
        }
    };
    SourceRange {
        start: position(start),
        end: position(end),
    }
}

fn locate_yaml_scalar_token(
    source: &str,
    pointer: &str,
    authored: &str,
    token: &str,
    ordinal: usize,
) -> Option<SourceRange> {
    let span = yaml_scalar_span(source, pointer)?;
    let authored_ordinal = authored.match_indices(token).nth(ordinal)?.0;
    let same_token_ordinal = authored[..authored_ordinal].matches(token).count();
    let relative = source[span.value_start..span.end]
        .match_indices(token)
        .nth(same_token_ordinal)?
        .0;
    let start = span.value_start + relative;
    Some(source_range(source, start, start + token.len()))
}

fn locate_yaml_scalar_offset(
    source: &str,
    pointer: &str,
    authored: &str,
    start: usize,
    end: usize,
) -> Option<SourceRange> {
    let span = yaml_scalar_span(source, pointer)?;
    let fragment = authored.get(start..end)?;
    let ordinal = authored[..start].matches(fragment).count();
    if !fragment.is_empty() {
        if let Some(relative) = source[span.value_start..span.end]
            .match_indices(fragment)
            .nth(ordinal)
            .map(|(offset, _)| offset)
        {
            let absolute = span.value_start + relative;
            return Some(source_range(source, absolute, absolute + fragment.len()));
        }
    }
    // Folded block scalars change authored whitespace. Delimiters themselves
    // remain byte-for-byte in source, so retain a useful field-local range.
    for token in ["[[", "]]", "[[]]"] {
        if authored
            .get(start..)
            .is_some_and(|tail| tail.starts_with(token))
        {
            let token_ordinal = authored[..start].matches(token).count();
            let relative = source[span.value_start..span.end]
                .match_indices(token)
                .nth(token_ordinal)?
                .0;
            let absolute = span.value_start + relative;
            return Some(source_range(source, absolute, absolute + token.len()));
        }
    }
    None
}

fn locate_reference_expression(
    source: &str,
    pointer: &str,
    authored: &str,
    expression: &crate::ReferenceExpression,
) -> Option<SourceRange> {
    let needle = format!("[[{}]]", expression.authored);
    let ordinal = authored[..expression.start].matches(&needle).count();
    let span = yaml_scalar_span(source, pointer)?;
    let relative = source[span.value_start..span.end]
        .match_indices(&needle)
        .nth(ordinal)?
        .0;
    let start = span.value_start + relative;
    Some(source_range(source, start, start + needle.len()))
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

fn completed_trigger_references(value: &Value) -> Vec<&str> {
    fn visit<'a>(value: &'a Value, key: Option<&str>, result: &mut Vec<&'a str>) {
        match value {
            Value::String(id) if key == Some("completed") => result.push(id),
            Value::Sequence(values) => {
                for value in values {
                    visit(value, key, result);
                }
            }
            Value::Mapping(values) => {
                for (raw_key, value) in values {
                    visit(value, raw_key.as_str(), result);
                }
            }
            Value::Tagged(tagged) => visit(&tagged.value, key, result),
            _ => {}
        }
    }

    let mut result = Vec::new();
    visit(value, None, &mut result);
    result
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
        _ if is_character_presence_requirement_pointer(pointer) => Some(PERSISTENT_REQUIREMENTS),
        _ if is_point_requirement_pointer(pointer) => Some(PERSISTENT_REQUIREMENTS),
        _ if is_win_state_requirement_pointer(pointer) => Some(PERSISTENT_REQUIREMENTS),
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

fn is_point_requirement_pointer(pointer: &str) -> bool {
    let parts = pointer
        .trim_start_matches('/')
        .split('/')
        .collect::<Vec<_>>();
    matches!(
        parts.as_slice(),
        [section, owner_index, "points", "requires", requirement_index]
            if matches!(*section, "settings" | "entities" | "deductions" | "commands")
                && owner_index.parse::<usize>().is_ok()
                && requirement_index.parse::<usize>().is_ok()
    )
}

fn is_win_state_requirement_pointer(pointer: &str) -> bool {
    let parts = pointer
        .trim_start_matches('/')
        .split('/')
        .collect::<Vec<_>>();
    matches!(
        parts.as_slice(),
        [section, state_index, "requires", requirement_index]
            if matches!(*section, "win_states" | "end_states")
                && state_index.parse::<usize>().is_ok()
                && requirement_index.parse::<usize>().is_ok()
    )
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

fn is_character_presence_requirement_pointer(pointer: &str) -> bool {
    let parts = pointer
        .trim_start_matches('/')
        .split('/')
        .collect::<Vec<_>>();
    matches!(
        parts.as_slice(),
        ["characters", character_index, "presence", "requires"]
            if character_index.parse::<usize>().is_ok()
    ) || matches!(
        parts.as_slice(),
        ["characters", character_index, "presence", "requires", requirement_index]
            if character_index.parse::<usize>().is_ok()
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
            let dependencies = persistent_predicate_ids(&item.mapping, "knows")
                .into_iter()
                .filter(|id| id_prefix(id) == Some("fact"))
                .collect();
            (item.id.clone(), dependencies)
        })
        .collect()
}

fn trigger_dependency_graph(items: &[Item]) -> BTreeMap<String, Vec<String>> {
    items
        .iter()
        .map(|item| {
            (
                item.id.clone(),
                persistent_predicate_ids(&item.mapping, "completed"),
            )
        })
        .collect()
}

fn persistent_predicate_ids(mapping: &Mapping, predicate: &str) -> Vec<String> {
    mapping
        .get(Value::String("when".to_string()))
        .and_then(Value::as_mapping)
        .and_then(|when| when.get(Value::String("all".to_string())))
        .and_then(Value::as_sequence)
        .into_iter()
        .flatten()
        .filter_map(Value::as_mapping)
        .filter_map(|condition| condition.get(Value::String(predicate.to_string())))
        .filter_map(Value::as_str)
        .map(str::to_string)
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

fn command_parameter_types(command: &Item) -> Vec<Option<CommandParameterShape>> {
    command
        .mapping
        .get(Value::String("parameters".to_string()))
        .and_then(Value::as_sequence)
        .into_iter()
        .flatten()
        .map(|parameter| {
            let parameter = parameter.as_mapping()?;
            let types = if let Some(types) = parameter
                .get(Value::String("types".to_string()))
                .and_then(Value::as_sequence)
            {
                types
                    .iter()
                    .filter_map(Value::as_str)
                    .filter_map(CommandParameterType::parse)
                    .collect::<Vec<_>>()
            } else {
                string_field(parameter, "type")
                    .and_then(CommandParameterType::parse)
                    .into_iter()
                    .collect()
            };
            (!types.is_empty()).then(|| CommandParameterShape {
                types,
                min: integer_field(parameter, "min")
                    .and_then(|value| usize::try_from(value).ok())
                    .unwrap_or_else(|| {
                        usize::from(bool_field(parameter, "required").unwrap_or(false))
                    }),
                max: integer_field(parameter, "max")
                    .and_then(|value| usize::try_from(value).ok())
                    .unwrap_or(1),
            })
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

fn is_string_sequence(value: &Value) -> bool {
    value
        .as_sequence()
        .is_some_and(|values| values.iter().all(Value::is_string))
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

fn time_to_minutes(value: &str) -> u16 {
    let bytes = value.as_bytes();
    u16::from((bytes[0] - b'0') * 10 + bytes[1] - b'0') * 60
        + u16::from((bytes[3] - b'0') * 10 + bytes[4] - b'0')
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

fn locate_item_field(source: &str, id: &str, field: &str) -> Option<SourceRange> {
    let lines = source.lines().collect::<Vec<_>>();
    let item_line = lines.iter().position(|line| {
        let Some(key_index) = line.find("id:") else {
            return false;
        };
        let prefix = line[..key_index].trim();
        if !prefix.is_empty() && prefix != "-" {
            return false;
        }
        line[key_index + 3..]
            .trim_start()
            .trim_start_matches(['\'', '"'])
            .starts_with(id)
    })?;
    let item_key_column = lines[item_line].find("id:")?;

    for (offset, line) in lines.iter().enumerate().skip(item_line + 1) {
        let indentation = line.len() - line.trim_start().len();
        if !line.trim().is_empty() && indentation < item_key_column {
            break;
        }
        if indentation != item_key_column {
            continue;
        }
        let trimmed = line.trim_start();
        let Some(rest) = trimmed
            .strip_prefix(field)
            .and_then(|rest| rest.strip_prefix(':'))
        else {
            continue;
        };
        let value = rest.trim_start();
        let spacing = rest.len() - value.len();
        let quote_bytes = usize::from(value.starts_with(['\'', '"']));
        let value_len = value
            .trim_matches(['\'', '"'])
            .split_whitespace()
            .next()
            .map(str::len)
            .unwrap_or(0);
        let column = indentation + field.len() + 1 + spacing + quote_bytes;
        return Some(SourceRange {
            start: Position {
                line: offset + 1,
                column: column + 1,
            },
            end: Position {
                line: offset + 1,
                column: column + value_len + 1,
            },
        });
    }
    None
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

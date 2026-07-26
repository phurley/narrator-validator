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
    "clues",
    "deductions",
    "tags",
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
    Deduction,
    Tag,
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
            Self::Deduction => "deduction",
            Self::Tag => "tag",
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
        self.validate_sections();

        let cases = self.items("case", Kind::Case, false);
        let settings = self.items("settings", Kind::Setting, true);
        let routes = self.items("routes", Kind::Route, true);
        let characters = self.items("characters", Kind::Character, true);
        let entities = self.items("entities", Kind::Entity, true);
        let events = self.items("events", Kind::Event, true);
        let clues = self.items("clues", Kind::Clue, true);
        let deductions = self.items("deductions", Kind::Deduction, true);
        let tags = self.items("tags", Kind::Tag, true);

        self.validate_case(&cases);
        self.validate_solution();
        self.validate_references();
        self.validate_duplicate_lists();
        self.validate_event_values(&events);
        self.validate_route_values(&routes);
        self.validate_graphs(&cases, &settings, &routes, &entities, &clues, &deductions);
        self.validate_tags(&tags);

        // Keep these collections live as independently indexed definitions;
        // assigning them here makes that intent explicit.
        let _ = characters;
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
                    if version != 1 {
                        self.push(
                            Severity::Error,
                            "format.unsupported_version",
                            format!("format version {version} is not supported; expected 1"),
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
                Some(value) if value > 0 => {}
                _ => self.push(
                    Severity::Error,
                    "route.invalid_travel_minutes",
                    "`travel_minutes` must be a positive integer".to_string(),
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
        cases: &[Item],
        settings: &[Item],
        route_items: &[Item],
        entities: &[Item],
        clues: &[Item],
        deductions: &[Item],
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
        self.validate_cycles(
            entity_containment,
            "entity.containment_cycle",
            "entity containment",
        );

        self.validate_cycles(
            dependency_graph(clues, "requires"),
            "clue.dependency_cycle",
            "clue dependency",
        );
        self.validate_cycles(
            dependency_graph(deductions, "requires"),
            "deduction.dependency_cycle",
            "deduction dependency",
        );

        self.validate_navigation(cases, settings, route_items);
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

    fn validate_tags(&mut self, tags: &[Item]) {
        for tag in tags {
            match tag.mapping.get(Value::String("members".to_string())) {
                Some(Value::Sequence(members)) if members.is_empty() => self.push(
                    Severity::Warning,
                    "tag.empty",
                    format!("tag `{}` has no members", tag.id),
                    &tag.path,
                    Some(format!("{}/members", tag.pointer)),
                    None,
                    Some(tag.id.clone()),
                ),
                None => self.push(
                    Severity::Warning,
                    "tag.empty",
                    format!("tag `{}` has no members", tag.id),
                    &tag.path,
                    Some(format!("{}/members", tag.pointer)),
                    None,
                    Some(tag.id.clone()),
                ),
                Some(value) if !is_string_sequence(value) => self.push(
                    Severity::Error,
                    "tag.members_type",
                    "`members` must be a sequence of IDs".to_string(),
                    &tag.path,
                    Some(format!("{}/members", tag.pointer)),
                    None,
                    Some(tag.id.clone()),
                ),
                Some(_) => {}
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

fn collect_references(
    value: &Value,
    pointer: &str,
    key: Option<&str>,
    references: &mut Vec<Reference>,
) {
    match value {
        Value::String(id)
            if key != Some("id") && (looks_like_id(id) || is_reference_pointer(pointer)) =>
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

fn expected_kind(pointer: &str) -> Option<&'static [Kind]> {
    let field = pointer.rsplit('/').next().unwrap_or_default();
    let parent = pointer.rsplit('/').nth(1).unwrap_or_default();
    const SETTINGS: &[Kind] = &[Kind::Setting];
    const CHARACTERS: &[Kind] = &[Kind::Character];
    const ENTITIES: &[Kind] = &[Kind::Entity];
    const CLUES: &[Kind] = &[Kind::Clue];
    const DEDUCTIONS: &[Kind] = &[Kind::Deduction];
    const CONTAINERS: &[Kind] = &[Kind::Setting, Kind::Character, Kind::Entity];
    const CONTENT_SOURCES: &[Kind] = &[Kind::Setting, Kind::Character, Kind::Entity, Kind::Event];
    const TAGGABLE: &[Kind] = &[
        Kind::Setting,
        Kind::Character,
        Kind::Entity,
        Kind::Event,
        Kind::Clue,
        Kind::Deduction,
    ];

    match field {
        "victim" | "culprit" | "testimony_source" => Some(CHARACTERS),
        "weapon" => Some(ENTITIES),
        "parent" | "from" | "to" | "location" => Some(SETTINGS),
        "container" => Some(CONTAINERS),
        _ if parent == "participants" => Some(CHARACTERS),
        _ if parent == "supported_by" => Some(CLUES),
        _ if parent == "requires" && pointer.contains("/routes/") => Some(ENTITIES),
        _ if parent == "requires" && pointer.contains("/clues/") => Some(CLUES),
        _ if parent == "requires" && pointer.contains("/deductions/") => Some(DEDUCTIONS),
        _ if parent == "entry_settings" || parent == "exit_settings" => Some(SETTINGS),
        _ if field == "source" && pointer.contains("/characters/") => Some(CONTENT_SOURCES),
        _ if field == "target" && pointer.contains("/clues/") => Some(CONTENT_SOURCES),
        _ if parent == "targets" && pointer.contains("/clues/") => Some(CONTENT_SOURCES),
        _ if parent == "members" && pointer.contains("/tags/") => Some(TAGGABLE),
        _ => None,
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

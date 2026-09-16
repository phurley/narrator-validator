use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde_yaml::{Mapping, Value};

use narrator_validator::{
    is_story_test_path, merge_layers, validate, Diagnostic, PlayabilityReport, PlayabilityStatus,
    Severity, SourceFile, ValidationReport, VALIDATOR_VERSION,
};

#[derive(Clone, Copy)]
enum Format {
    Text,
    Json,
    Github,
}

fn main() -> ExitCode {
    match run(env::args().skip(1)) {
        Ok(valid) if valid => ExitCode::SUCCESS,
        Ok(_) => ExitCode::from(1),
        Err(message) => {
            eprintln!("narrator-validator: {message}");
            ExitCode::from(2)
        }
    }
}

fn run(args: impl Iterator<Item = String>) -> Result<bool, String> {
    let mut format = Format::Text;
    let mut root = None;
    let mut deck_dir = None;
    let mut strip_deck_only = false;
    let mut emit_effective = None;
    let mut args = args.peekable();
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "-h" | "--help" => {
                print_help();
                return Ok(true);
            }
            "-V" | "--version" => {
                println!("narrator-validator {}", env!("CARGO_PKG_VERSION"));
                return Ok(true);
            }
            "--format" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--format requires text, json, or github".to_string())?;
                format = match value.as_str() {
                    "text" => Format::Text,
                    "json" => Format::Json,
                    "github" => Format::Github,
                    _ => return Err(format!("unknown output format `{value}`")),
                };
            }
            "--deck-dir" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--deck-dir requires a path".to_string())?;
                deck_dir = Some(PathBuf::from(value));
            }
            "--strip-deck-only" => {
                strip_deck_only = true;
            }
            "--emit-effective" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--emit-effective requires a path".to_string())?;
                emit_effective = Some(PathBuf::from(value));
            }
            value if value.starts_with('-') => return Err(format!("unknown option `{value}`")),
            value => {
                if root.replace(PathBuf::from(value)).is_some() {
                    return Err("only one repository path may be supplied".to_string());
                }
            }
        }
    }

    let root = root.unwrap_or_else(|| PathBuf::from("."));
    let story_files = read_sources(&root)?;

    let files = if let Some(deck_dir) = &deck_dir {
        let mut deck_files = read_sources(deck_dir)?;
        let mut story_files = story_files;
        if strip_deck_only {
            strip_deck_only_fields(&mut deck_files, &mut story_files)?;
        }
        match merge_layers(&deck_files, &story_files) {
            Ok(merged) => merged,
            Err(diagnostics) => {
                let report = ValidationReport {
                    validator_version: VALIDATOR_VERSION.to_string(),
                    format_version: None,
                    valid: false,
                    diagnostics,
                    features: Vec::new(),
                    reference_text: Vec::new(),
                    playability: None,
                };
                emit_report(&format, &report)?;
                return Ok(false);
            }
        }
    } else {
        story_files
    };

    if let Some(effective_dir) = &emit_effective {
        write_effective_set(effective_dir, &files)?;
    }

    let report = validate(&files);
    emit_report(&format, &report)?;
    Ok(report.valid)
}

fn emit_report(format: &Format, report: &ValidationReport) -> Result<(), String> {
    match format {
        Format::Text => print_text(
            &report.diagnostics,
            report.playability.as_ref(),
            report.valid,
        ),
        Format::Json => println!(
            "{}",
            serde_json::to_string_pretty(report)
                .map_err(|error| format!("could not serialize report: {error}"))?
        ),
        Format::Github => print_github(&report.diagnostics),
    }
    Ok(())
}

/// Moves the deck-owned `case.format_version` / `case.ruleset` fields from
/// the story's `case.yaml` onto the deck layer's, mirroring the backend
/// import rule (ADR-022 §6): a deck missing the field inherits the story's
/// value, a deck already carrying the identical value is untouched, and a
/// conflicting value is rejected naming both. This lets a checked-out story
/// repository, which still carries both fields, validate against an empty
/// deck directory.
fn strip_deck_only_fields(
    deck: &mut Vec<SourceFile>,
    story: &mut [SourceFile],
) -> Result<(), String> {
    const DECK_ONLY_FIELDS: &[&str] = &["format_version", "ruleset"];

    let Some(story_case) = story.iter_mut().find(|file| file.path == "case.yaml") else {
        return Ok(());
    };
    let mut story_value: Value = serde_yaml::from_str(&story_case.source)
        .map_err(|error| format!("could not parse `case.yaml`: {error}"))?;
    let Some(story_case_mapping) = story_value
        .as_mapping_mut()
        .and_then(|root| root.get_mut("case"))
        .and_then(Value::as_mapping_mut)
    else {
        return Ok(());
    };

    let mut moved = Vec::new();
    for field in DECK_ONLY_FIELDS {
        if let Some(value) = story_case_mapping.remove(*field) {
            moved.push((*field, value));
        }
    }
    if moved.is_empty() {
        return Ok(());
    }
    story_case.source = serde_yaml::to_string(&story_value)
        .map_err(|error| format!("could not serialize stripped `case.yaml`: {error}"))?;

    let deck_case_index = deck.iter().position(|file| file.path == "case.yaml");
    let mut deck_value: Value = match &deck_case_index {
        Some(index) => serde_yaml::from_str(&deck[*index].source)
            .map_err(|error| format!("could not parse deck `case.yaml`: {error}"))?,
        None => Value::Mapping(Mapping::new()),
    };
    if deck_value.as_mapping().is_none() {
        deck_value = Value::Mapping(Mapping::new());
    }
    let deck_root = deck_value.as_mapping_mut().expect("just set to a mapping");
    if deck_root.get("case").and_then(Value::as_mapping).is_none() {
        deck_root.insert(
            Value::String("case".to_string()),
            Value::Mapping(Mapping::new()),
        );
    }
    let deck_case_mapping = deck_root
        .get_mut("case")
        .and_then(Value::as_mapping_mut)
        .expect("just inserted");

    for (field, value) in moved {
        match deck_case_mapping.get(field) {
            None => {
                deck_case_mapping.insert(Value::String(field.to_string()), value);
            }
            Some(existing) if existing == &value => {}
            Some(existing) => {
                return Err(format!(
                    "--strip-deck-only: `case.{field}` differs between the story (`{:?}`) and the deck (`{:?}`)",
                    value, existing
                ));
            }
        }
    }

    let deck_source = serde_yaml::to_string(&deck_value)
        .map_err(|error| format!("could not serialize deck `case.yaml`: {error}"))?;
    match deck_case_index {
        Some(index) => deck[index].source = deck_source,
        None => deck.push(SourceFile {
            path: "case.yaml".to_string(),
            source: deck_source,
        }),
    }
    Ok(())
}

fn write_effective_set(directory: &Path, files: &[SourceFile]) -> Result<(), String> {
    for file in files {
        let path = directory.join(&file.path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("could not create `{}`: {error}", parent.display()))?;
        }
        fs::write(&path, &file.source)
            .map_err(|error| format!("could not write `{}`: {error}", path.display()))?;
    }
    Ok(())
}

fn read_sources(root: &Path) -> Result<Vec<SourceFile>, String> {
    if !root.is_dir() {
        return Err(format!("`{}` is not a directory", root.display()));
    }
    let mut paths = Vec::new();
    visit(root, root, &mut paths)?;
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let relative = path
                .strip_prefix(root)
                .expect("visited path is under root")
                .to_string_lossy()
                .replace('\\', "/");
            let source = fs::read_to_string(&path)
                .map_err(|error| format!("could not read `{}`: {error}", path.display()))?;
            Ok(SourceFile {
                path: relative,
                source,
            })
        })
        .collect()
}

/// Story YAML anywhere in the repository, plus Format 3.8 map SVGs, which
/// live only in `maps/`, and ADR-021 story test scripts, which live only
/// under `scripts/`. All three are UTF-8 text, so `SourceFile` needs no
/// binary channel; a raster map would, which is why SVG is the only image
/// format the story contract admits.
fn is_story_file(relative: &Path) -> bool {
    match relative.extension().and_then(|value| value.to_str()) {
        Some("yaml" | "yml") => true,
        Some("svg") => relative
            .parent()
            .is_some_and(|parent| parent == Path::new("maps")),
        Some("json") => is_story_test_path(&relative.to_string_lossy().replace('\\', "/")),
        _ => false,
    }
}

fn visit(root: &Path, directory: &Path, paths: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(directory)
        .map_err(|error| format!("could not read `{}`: {error}", directory.display()))?
    {
        let entry = entry.map_err(|error| format!("could not read directory entry: {error}"))?;
        let path = entry.path();
        let relative = path.strip_prefix(root).expect("visited path is under root");
        if relative
            .components()
            .any(|component| component.as_os_str() == ".git")
        {
            continue;
        }
        let file_type = entry
            .file_type()
            .map_err(|error| format!("could not inspect `{}`: {error}", path.display()))?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            visit(root, &path, paths)?;
        } else if file_type.is_file() && is_story_file(relative) {
            paths.push(path);
        }
    }
    Ok(())
}

fn print_text(diagnostics: &[Diagnostic], playability: Option<&PlayabilityReport>, valid: bool) {
    for diagnostic in diagnostics {
        let position = diagnostic
            .range
            .map(|range| format!(":{}:{}", range.start.line, range.start.column))
            .unwrap_or_default();
        println!(
            "{}{}: {}[{}]: {}",
            diagnostic.path,
            position,
            severity_name(diagnostic.severity),
            diagnostic.code,
            diagnostic.message
        );
    }
    println!(
        "{} error(s), {} warning(s)",
        diagnostics
            .iter()
            .filter(|item| item.severity == Severity::Error)
            .count(),
        diagnostics
            .iter()
            .filter(|item| item.severity == Severity::Warning)
            .count()
    );
    if let Some(playability) = playability {
        println!(
            "deduction graph: maximum depth {}, largest automatic cascade {}{}",
            playability.deduction_graph.maximum_depth,
            playability.deduction_graph.largest_cascade_size,
            playability
                .deduction_graph
                .largest_cascade_root
                .as_deref()
                .map(|root| format!(" from {root}"))
                .unwrap_or_default()
        );
        for terminal in &playability.terminal_paths {
            let status = match terminal.status {
                PlayabilityStatus::Proved => "proved",
                PlayabilityStatus::NotProved => "not_proved",
                PlayabilityStatus::Inconclusive => "inconclusive",
            };
            if let Some(bound) = &terminal.lower_bound {
                println!("playability {}: {} ({} action(s), {} minute(s), {} route action(s), {} wait minute(s))", terminal.id, status, bound.action_count, bound.elapsed_minutes, bound.route_action_count, bound.wait_minutes);
            } else if let Some(blocker) = &terminal.blocker {
                println!(
                    "{}: playability[{}] {}: {}",
                    blocker.path, blocker.code, terminal.id, blocker.message
                );
            }
        }
        for policy in playability
            .notebook_policies
            .iter()
            .filter(|policy| !policy.auto_facts || !policy.auto_deductions)
        {
            let proved = policy
                .terminal_paths
                .iter()
                .filter(|path| path.status == PlayabilityStatus::Proved)
                .count();
            println!(
                "notebook policy auto_facts={} auto_deductions={}: {proved}/{} terminal path(s) proved",
                policy.auto_facts,
                policy.auto_deductions,
                policy.terminal_paths.len()
            );
        }
    }
    if valid {
        println!("valid");
    }
}

fn print_github(diagnostics: &[Diagnostic]) {
    for diagnostic in diagnostics {
        let command = severity_name(diagnostic.severity);
        let mut properties = format!("title={}", github_property(&diagnostic.code));
        if !diagnostic.path.is_empty() {
            properties.insert_str(0, &format!("file={},", github_property(&diagnostic.path)));
        }
        if let Some(range) = diagnostic.range {
            properties.push_str(&format!(
                ",line={},col={},endLine={},endColumn={}",
                range.start.line, range.start.column, range.end.line, range.end.column
            ));
        }
        println!(
            "::{command} {properties}::{}",
            github_message(&diagnostic.message)
        );
    }
}

fn severity_name(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
    }
}

fn github_property(value: &str) -> String {
    github_message(value)
        .replace(':', "%3A")
        .replace(',', "%2C")
}

fn github_message(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

fn print_help() {
    println!(
        "Validate a complete Narrator story repository.

Usage: narrator-validator [OPTIONS] [REPOSITORY]

Options:
      --format <FORMAT>       text (default), json, or github
      --deck-dir <DIR>        Overlay REPOSITORY as a story layer on top of a
                               deck layer read from DIR before validating
                               (ADR-022)
      --strip-deck-only       Move case.format_version and case.ruleset from
                               the story's case.yaml onto the deck layer
                               before merging (requires --deck-dir)
      --emit-effective <DIR>  Write the merged effective file set to DIR
  -h, --help                  Print help
  -V, --version               Print version"
    );
}

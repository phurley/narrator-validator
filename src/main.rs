use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use narrator_validator::{
    validate, Diagnostic, PlayabilityReport, PlayabilityStatus, Severity, SourceFile,
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
            value if value.starts_with('-') => return Err(format!("unknown option `{value}`")),
            value => {
                if root.replace(PathBuf::from(value)).is_some() {
                    return Err("only one repository path may be supplied".to_string());
                }
            }
        }
    }

    let root = root.unwrap_or_else(|| PathBuf::from("."));
    let files = read_sources(&root)?;
    let report = validate(&files);
    match format {
        Format::Text => print_text(
            &report.diagnostics,
            report.playability.as_ref(),
            report.valid,
        ),
        Format::Json => println!(
            "{}",
            serde_json::to_string_pretty(&report)
                .map_err(|error| format!("could not serialize report: {error}"))?
        ),
        Format::Github => print_github(&report.diagnostics),
    }
    Ok(report.valid)
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
        } else if file_type.is_file()
            && matches!(
                path.extension().and_then(|value| value.to_str()),
                Some("yaml" | "yml")
            )
        {
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
      --format <FORMAT>  text (default), json, or github
  -h, --help             Print help
  -V, --version          Print version"
    );
}

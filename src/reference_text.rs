//! Story Format 3.2+ reference-aware prose.
//!
//! The tables in this module are the authoritative disclosure and path
//! registry. Validation, Rust consumers, and the browser metadata export all
//! use these values rather than maintaining parallel allowlists.

use serde::{Deserialize, Serialize};

pub const REFERENCE_TEXT_FEATURE: &str = "reference_text_v1";
pub const SUPPORTED_FEATURES: &[&str] = &[REFERENCE_TEXT_FEATURE];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisclosureClass {
    PlayerSafe,
    GatedPlayerSafe,
    PrivateNarrator,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ConsumerField {
    pub kind: &'static str,
    pub path: &'static str,
    pub disclosure: DisclosureClass,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ReferencePath {
    pub path: &'static str,
    pub disclosure: DisclosureClass,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ReferenceKind {
    pub kind: &'static str,
    pub default_path: Option<&'static str>,
    pub paths: &'static [ReferencePath],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReferenceExpression {
    pub authored: String,
    pub target_id: String,
    pub property_path: Vec<String>,
    /// Zero-based UTF-8 byte offset of the opening delimiter.
    pub start: usize,
    /// Exclusive zero-based UTF-8 byte offset after the closing delimiter.
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ReferenceTextSegment {
    Literal { text: String },
    Reference { expression: ReferenceExpression },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedReferenceText {
    pub source: String,
    pub segments: Vec<ReferenceTextSegment>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReferenceProvenance {
    pub expression: ReferenceExpression,
    pub path: String,
    pub pointer: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<crate::SourceRange>,
    pub definition_pointer: String,
    pub resolved_path: String,
    pub resolved_value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedReferenceText {
    pub path: String,
    pub pointer: String,
    pub disclosure: DisclosureClass,
    pub authored: String,
    pub resolved: String,
    pub provenance: Vec<ReferenceProvenance>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ReferenceParseError {
    #[error("reference expression is not closed")]
    Unclosed { start: usize },
    #[error("reference expression is empty")]
    Empty { start: usize, end: usize },
    #[error("`{authored}` is not a reference ID followed by named mapping fields")]
    Invalid {
        authored: String,
        start: usize,
        end: usize,
    },
    #[error("unexpected closing reference delimiter")]
    UnexpectedClose { start: usize },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ReferenceTextParseResult {
    Parsed { value: ParsedReferenceText },
    Error { error: ReferenceParseError },
}

pub fn parse_reference_text_result(source: &str) -> ReferenceTextParseResult {
    match parse_reference_text(source) {
        Ok(value) => ReferenceTextParseResult::Parsed { value },
        Err(error) => ReferenceTextParseResult::Error { error },
    }
}

const PUBLIC: DisclosureClass = DisclosureClass::PlayerSafe;
const GATED: DisclosureClass = DisclosureClass::GatedPlayerSafe;
const PRIVATE: DisclosureClass = DisclosureClass::PrivateNarrator;

pub const CONSUMER_FIELDS: &[ConsumerField] = &[
    ConsumerField {
        kind: "case",
        path: "title",
        disclosure: PUBLIC,
    },
    ConsumerField {
        kind: "case",
        path: "premise",
        disclosure: PUBLIC,
    },
    ConsumerField {
        kind: "case",
        path: "opening",
        disclosure: PUBLIC,
    },
    ConsumerField {
        kind: "case",
        path: "players.description",
        disclosure: PUBLIC,
    },
    ConsumerField {
        kind: "persona",
        path: "description",
        disclosure: PUBLIC,
    },
    ConsumerField {
        kind: "persona",
        path: "narrator_guidance",
        disclosure: PRIVATE,
    },
    ConsumerField {
        kind: "setting",
        path: "name",
        disclosure: PUBLIC,
    },
    ConsumerField {
        kind: "setting",
        path: "description",
        disclosure: PUBLIC,
    },
    ConsumerField {
        kind: "character",
        path: "name",
        disclosure: PUBLIC,
    },
    ConsumerField {
        kind: "character",
        path: "role",
        disclosure: PUBLIC,
    },
    ConsumerField {
        kind: "character",
        path: "occupation",
        disclosure: PUBLIC,
    },
    ConsumerField {
        kind: "character",
        path: "description",
        disclosure: PUBLIC,
    },
    ConsumerField {
        kind: "character",
        path: "portrayal.demeanor",
        disclosure: PUBLIC,
    },
    ConsumerField {
        kind: "character",
        path: "portrayal.speech_style",
        disclosure: PUBLIC,
    },
    ConsumerField {
        kind: "character",
        path: "narrator_guidance.goal",
        disclosure: PRIVATE,
    },
    ConsumerField {
        kind: "character",
        path: "narrator_guidance.secret",
        disclosure: PRIVATE,
    },
    ConsumerField {
        kind: "character",
        path: "narrator_guidance.motive",
        disclosure: PRIVATE,
    },
    ConsumerField {
        kind: "character",
        path: "narrator_guidance.method",
        disclosure: PRIVATE,
    },
    ConsumerField {
        kind: "character",
        path: "narrator_guidance.cover_story",
        disclosure: PRIVATE,
    },
    ConsumerField {
        kind: "character",
        path: "narrator_guidance.testimony_guidance",
        disclosure: PRIVATE,
    },
    ConsumerField {
        kind: "entity",
        path: "name",
        disclosure: PUBLIC,
    },
    ConsumerField {
        kind: "entity",
        path: "description",
        disclosure: PUBLIC,
    },
    ConsumerField {
        kind: "event",
        path: "summary",
        disclosure: GATED,
    },
    ConsumerField {
        kind: "fact",
        path: "statement",
        disclosure: GATED,
    },
    ConsumerField {
        kind: "fact",
        path: "narrative_detail",
        disclosure: GATED,
    },
    ConsumerField {
        kind: "deduction",
        path: "conclusion",
        disclosure: GATED,
    },
    ConsumerField {
        kind: "command",
        path: "name",
        disclosure: PUBLIC,
    },
    ConsumerField {
        kind: "command",
        path: "description",
        disclosure: PUBLIC,
    },
    ConsumerField {
        kind: "command_parameter",
        path: "description",
        disclosure: PUBLIC,
    },
    ConsumerField {
        kind: "command_effect",
        path: "text",
        disclosure: PUBLIC,
    },
    ConsumerField {
        kind: "trigger",
        path: "name",
        disclosure: PUBLIC,
    },
    ConsumerField {
        kind: "trigger",
        path: "description",
        disclosure: PUBLIC,
    },
    ConsumerField {
        kind: "trigger_effect",
        path: "text",
        disclosure: PUBLIC,
    },
    ConsumerField {
        kind: "flag",
        path: "name",
        disclosure: PUBLIC,
    },
    ConsumerField {
        kind: "flag",
        path: "description",
        disclosure: PUBLIC,
    },
    ConsumerField {
        kind: "testimony",
        path: "text",
        disclosure: GATED,
    },
    ConsumerField {
        kind: "win",
        path: "name",
        disclosure: PUBLIC,
    },
    ConsumerField {
        kind: "win",
        path: "text",
        disclosure: PUBLIC,
    },
    ConsumerField {
        kind: "end",
        path: "name",
        disclosure: PUBLIC,
    },
    ConsumerField {
        kind: "end",
        path: "text",
        disclosure: PUBLIC,
    },
    ConsumerField {
        kind: "solution",
        path: "narrator_guidance.motive",
        disclosure: PRIVATE,
    },
    ConsumerField {
        kind: "solution",
        path: "narrator_guidance.method",
        disclosure: PRIVATE,
    },
    ConsumerField {
        kind: "solution",
        path: "narrator_guidance.proof_summary",
        disclosure: PRIVATE,
    },
    ConsumerField {
        kind: "solution_question",
        path: "prompt",
        disclosure: PUBLIC,
    },
    ConsumerField {
        kind: "solution_step",
        path: "prompt",
        disclosure: PUBLIC,
    },
    ConsumerField {
        kind: "solution_step",
        path: "on_success.notes",
        disclosure: PRIVATE,
    },
    ConsumerField {
        kind: "solution_step",
        path: "on_failure.notes",
        disclosure: PRIVATE,
    },
];

const CASE_PATHS: &[ReferencePath] = &[
    ReferencePath {
        path: "title",
        disclosure: PUBLIC,
    },
    ReferencePath {
        path: "premise",
        disclosure: PUBLIC,
    },
    ReferencePath {
        path: "opening",
        disclosure: PUBLIC,
    },
];
const ORDINARY_PATHS: &[ReferencePath] = &[
    ReferencePath {
        path: "name",
        disclosure: PUBLIC,
    },
    ReferencePath {
        path: "description",
        disclosure: PUBLIC,
    },
];
const CHARACTER_PATHS: &[ReferencePath] = &[
    ReferencePath {
        path: "name",
        disclosure: PUBLIC,
    },
    ReferencePath {
        path: "role",
        disclosure: PUBLIC,
    },
    ReferencePath {
        path: "occupation",
        disclosure: PUBLIC,
    },
    ReferencePath {
        path: "description",
        disclosure: PUBLIC,
    },
    ReferencePath {
        path: "portrayal.demeanor",
        disclosure: PUBLIC,
    },
    ReferencePath {
        path: "portrayal.speech_style",
        disclosure: PUBLIC,
    },
    ReferencePath {
        path: "narrator_guidance.goal",
        disclosure: PRIVATE,
    },
    ReferencePath {
        path: "narrator_guidance.secret",
        disclosure: PRIVATE,
    },
    ReferencePath {
        path: "narrator_guidance.motive",
        disclosure: PRIVATE,
    },
    ReferencePath {
        path: "narrator_guidance.method",
        disclosure: PRIVATE,
    },
    ReferencePath {
        path: "narrator_guidance.cover_story",
        disclosure: PRIVATE,
    },
    ReferencePath {
        path: "narrator_guidance.testimony_guidance",
        disclosure: PRIVATE,
    },
];
const EVENT_PATHS: &[ReferencePath] = &[ReferencePath {
    path: "summary",
    disclosure: GATED,
}];
const FACT_PATHS: &[ReferencePath] = &[
    ReferencePath {
        path: "statement",
        disclosure: GATED,
    },
    ReferencePath {
        path: "narrative_detail",
        disclosure: GATED,
    },
];
const DEDUCTION_PATHS: &[ReferencePath] = &[ReferencePath {
    path: "conclusion",
    disclosure: GATED,
}];
const TESTIMONY_PATHS: &[ReferencePath] = &[ReferencePath {
    path: "text",
    disclosure: GATED,
}];
const WIN_PATHS: &[ReferencePath] = &[
    ReferencePath {
        path: "name",
        disclosure: PUBLIC,
    },
    ReferencePath {
        path: "text",
        disclosure: PUBLIC,
    },
];

pub const REFERENCE_KINDS: &[ReferenceKind] = &[
    ReferenceKind {
        kind: "case",
        default_path: Some("title"),
        paths: CASE_PATHS,
    },
    ReferenceKind {
        kind: "setting",
        default_path: Some("name"),
        paths: ORDINARY_PATHS,
    },
    ReferenceKind {
        kind: "character",
        default_path: Some("name"),
        paths: CHARACTER_PATHS,
    },
    ReferenceKind {
        kind: "entity",
        default_path: Some("name"),
        paths: ORDINARY_PATHS,
    },
    ReferenceKind {
        kind: "event",
        default_path: Some("summary"),
        paths: EVENT_PATHS,
    },
    ReferenceKind {
        kind: "fact",
        default_path: Some("statement"),
        paths: FACT_PATHS,
    },
    ReferenceKind {
        kind: "deduction",
        default_path: Some("conclusion"),
        paths: DEDUCTION_PATHS,
    },
    ReferenceKind {
        kind: "flag",
        default_path: Some("name"),
        paths: ORDINARY_PATHS,
    },
    ReferenceKind {
        kind: "command",
        default_path: Some("name"),
        paths: ORDINARY_PATHS,
    },
    ReferenceKind {
        kind: "trigger",
        default_path: Some("name"),
        paths: ORDINARY_PATHS,
    },
    ReferenceKind {
        kind: "testimony",
        default_path: Some("text"),
        paths: TESTIMONY_PATHS,
    },
    ReferenceKind {
        kind: "win",
        default_path: Some("name"),
        paths: WIN_PATHS,
    },
    ReferenceKind {
        kind: "end",
        default_path: Some("name"),
        paths: WIN_PATHS,
    },
    ReferenceKind {
        kind: "answer",
        default_path: Some("name"),
        paths: ORDINARY_PATHS,
    },
];

pub fn reference_kind(kind: &str) -> Option<&'static ReferenceKind> {
    REFERENCE_KINDS.iter().find(|entry| entry.kind == kind)
}

pub fn reference_text_metadata_json() -> String {
    #[derive(Serialize)]
    struct Metadata {
        supported_features: &'static [&'static str],
        consumer_fields: &'static [ConsumerField],
        reference_kinds: &'static [ReferenceKind],
    }
    serde_json::to_string(&Metadata {
        supported_features: SUPPORTED_FEATURES,
        consumer_fields: CONSUMER_FIELDS,
        reference_kinds: REFERENCE_KINDS,
    })
    .expect("static reference metadata is serializable")
}

/// Parse reference-aware text while retaining literal and reference segments.
/// `\[[` emits literal `[[`; the escape slash is removed.
pub fn parse_reference_text(source: &str) -> Result<ParsedReferenceText, ReferenceParseError> {
    let bytes = source.as_bytes();
    let mut segments = Vec::new();
    let mut literal = String::new();
    let mut index = 0;
    let mut escaped_expression = false;
    while index < bytes.len() {
        if bytes[index] == b'\\' && bytes.get(index + 1..index + 3) == Some(b"[[") {
            literal.push_str("[[");
            index += 3;
            escaped_expression = true;
            continue;
        }
        if bytes.get(index..index + 2) == Some(b"]]") {
            if escaped_expression {
                literal.push_str("]]");
                index += 2;
                escaped_expression = false;
                continue;
            }
            return Err(ReferenceParseError::UnexpectedClose { start: index });
        }
        if bytes.get(index..index + 2) != Some(b"[[") {
            let character = source[index..]
                .chars()
                .next()
                .expect("valid character boundary");
            literal.push(character);
            index += character.len_utf8();
            continue;
        }
        if !literal.is_empty() {
            segments.push(ReferenceTextSegment::Literal {
                text: std::mem::take(&mut literal),
            });
        }
        let start = index;
        let Some(relative_end) = source[index + 2..].find("]]") else {
            return Err(ReferenceParseError::Unclosed { start });
        };
        let close = index + 2 + relative_end;
        let end = close + 2;
        let authored = &source[index + 2..close];
        if authored.is_empty() {
            return Err(ReferenceParseError::Empty { start, end });
        }
        let components = authored.split('.').collect::<Vec<_>>();
        let valid_component = |part: &&str| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
                && !part.starts_with('_')
                && !part.ends_with('_')
                && !part.contains("__")
        };
        if components.len() < 2 || !components.iter().all(valid_component) {
            return Err(ReferenceParseError::Invalid {
                authored: authored.to_string(),
                start,
                end,
            });
        }
        // `answer.*` IDs are three components by design
        // (`answer.<category>.<value>`, e.g. `answer.motive.jealousy`): the
        // whole three-part string is the target ID, and answer cards have
        // no addressable property path. Every other kind is `kind.id` with
        // any remaining components forming a trailing property path.
        let (target_id, property_path) = if components[0] == "answer" {
            if components.len() != 3 {
                return Err(ReferenceParseError::Invalid {
                    authored: authored.to_string(),
                    start,
                    end,
                });
            }
            (authored.to_string(), Vec::new())
        } else {
            (
                format!("{}.{}", components[0], components[1]),
                components[2..]
                    .iter()
                    .map(|part| (*part).to_string())
                    .collect(),
            )
        };
        segments.push(ReferenceTextSegment::Reference {
            expression: ReferenceExpression {
                authored: authored.to_string(),
                target_id,
                property_path,
                start,
                end,
            },
        });
        index = end;
    }
    if !literal.is_empty() {
        segments.push(ReferenceTextSegment::Literal { text: literal });
    }
    Ok(ParsedReferenceText {
        source: source.to_string(),
        segments,
    })
}

pub(crate) fn disclosure_allows(consumer: DisclosureClass, target: DisclosureClass) -> bool {
    match consumer {
        DisclosureClass::PrivateNarrator => true,
        DisclosureClass::GatedPlayerSafe => target != DisclosureClass::PrivateNarrator,
        DisclosureClass::PlayerSafe => target == DisclosureClass::PlayerSafe,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_preserves_order_and_unescapes_literal_opening_delimiters() {
        let parsed = parse_reference_text(
            r"See [[character.echo]], \[[character.literal]], then [[setting.clinic.name]].",
        )
        .unwrap();
        let references = parsed
            .segments
            .iter()
            .filter_map(|segment| match segment {
                ReferenceTextSegment::Reference { expression } => {
                    Some(expression.authored.as_str())
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(references, ["character.echo", "setting.clinic.name"]);
        assert!(parsed.segments.iter().any(|segment| matches!(
            segment,
            ReferenceTextSegment::Literal { text } if text.contains("[[character.literal]]")
        )));
    }

    #[test]
    fn parser_rejects_each_malformed_delimiter_and_component_shape() {
        assert!(matches!(
            parse_reference_text("[[character.echo"),
            Err(ReferenceParseError::Unclosed { .. })
        ));
        assert!(matches!(
            parse_reference_text("[[]]"),
            Err(ReferenceParseError::Empty { .. })
        ));
        assert!(matches!(
            parse_reference_text("[[character.echo.*]]"),
            Err(ReferenceParseError::Invalid { .. })
        ));
        assert!(matches!(
            parse_reference_text("character.echo]]"),
            Err(ReferenceParseError::UnexpectedClose { .. })
        ));
    }

    #[test]
    fn parser_treats_three_component_answer_ids_as_the_full_target() {
        for authored in [
            "answer.motive.jealousy",
            "answer.time.evening",
            "answer.method.struck",
        ] {
            let source = format!("[[{authored}]]");
            let parsed = parse_reference_text(&source).unwrap();
            let ReferenceTextSegment::Reference { expression } = &parsed.segments[0] else {
                panic!("expected reference segment for {authored}");
            };
            assert_eq!(expression.target_id, authored);
            assert!(expression.property_path.is_empty());
        }
    }

    #[test]
    fn parser_rejects_two_component_answer_id_without_a_value() {
        assert!(matches!(
            parse_reference_text("[[answer.motive]]"),
            Err(ReferenceParseError::Invalid { .. })
        ));
    }

    #[test]
    fn parser_rejects_four_component_answer_id() {
        assert!(matches!(
            parse_reference_text("[[answer.motive.jealousy.extra]]"),
            Err(ReferenceParseError::Invalid { .. })
        ));
    }

    #[test]
    fn parser_still_splits_non_answer_kind_into_id_and_property_path() {
        let parsed = parse_reference_text("[[deduction.foo.conclusion]]").unwrap();
        let ReferenceTextSegment::Reference { expression } = &parsed.segments[0] else {
            panic!("expected reference segment");
        };
        assert_eq!(expression.target_id, "deduction.foo");
        assert_eq!(expression.property_path, vec!["conclusion".to_string()]);
    }

    #[test]
    fn parser_offsets_are_utf8_bytes() {
        let parsed = parse_reference_text("é [[character.echo.name]]!").unwrap();
        let ReferenceTextSegment::Reference { expression } = &parsed.segments[1] else {
            panic!("expected reference segment");
        };
        assert_eq!((expression.start, expression.end), (3, 26));
        assert!(matches!(
            parse_reference_text("é [[character.echo"),
            Err(ReferenceParseError::Unclosed { start: 3 })
        ));
    }
}

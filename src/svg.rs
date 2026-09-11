//! Story Format 3.8 map-SVG safety checks.
//!
//! A `case.map` variant's `source` is handed to the player client verbatim
//! and rendered, so the validator — not the client — is the gate. Every
//! check here is a rejection: the authored SVG must parse as XML, be rooted
//! at `<svg>` with a `viewBox`, and contain no scripting, no embedded
//! foreign markup, no event handlers, and no reference that leaves the
//! document.
//!
//! The 256 KiB per-file cap Format 3.8 requires of a map is not repeated
//! here. `MAX_FILE_BYTES` in `validator.rs` already applies it to every file
//! in a snapshot, including `maps/*.svg`, and reports
//! `repository.file_too_large`; a second map-specific cap would be a second
//! mechanism saying the same thing, and would drift.

/// One rejected property of an authored map SVG, already carrying the
/// diagnostic code the validator reports it under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapSvgProblem {
    pub code: &'static str,
    pub message: String,
}

fn problem(code: &'static str, message: impl Into<String>) -> MapSvgProblem {
    MapSvgProblem {
        code,
        message: message.into(),
    }
}

/// Inspect one authored map SVG. An empty result means the document is safe
/// to hand to a renderer. Results are ordered deterministically: parse, then
/// document-level shape, then per-node findings in document order.
pub fn check_map_svg(source: &str) -> Vec<MapSvgProblem> {
    let mut problems = Vec::new();
    // `roxmltree`'s defaults reject a DTD outright, which is what keeps an
    // entity-expansion bomb out of the validator as well as the client.
    let document = match roxmltree::Document::parse(source) {
        Ok(document) => document,
        Err(error) => {
            problems.push(problem(
                "case.map_svg_invalid",
                format!("map SVG is not well-formed XML: {error}"),
            ));
            return problems;
        }
    };

    let root = document.root_element();
    if root.tag_name().name() != "svg" {
        problems.push(problem(
            "case.map_svg_root",
            format!(
                "map SVG root element must be `<svg>`, found `<{}>`",
                root.tag_name().name()
            ),
        ));
    } else if !root.has_attribute("viewBox") {
        problems.push(problem(
            "case.map_svg_view_box",
            "map SVG root `<svg>` must declare a `viewBox` so the client can scale and fit it"
                .to_string(),
        ));
    }

    for node in document.descendants().filter(roxmltree::Node::is_element) {
        let name = node.tag_name().name();
        if matches!(name, "script" | "foreignObject") {
            problems.push(problem(
                "case.map_svg_forbidden_element",
                format!("map SVG must not contain `<{name}>`"),
            ));
        }
        for attribute in node.attributes() {
            let attribute_name = attribute.name();
            // SVG event attributes are all `on*`; matching the prefix
            // case-insensitively rather than enumerating the ~40 names keeps
            // this correct as the SVG/HTML event vocabulary grows.
            if attribute_name.len() > 2 && attribute_name[..2].eq_ignore_ascii_case("on") {
                problems.push(problem(
                    "case.map_svg_event_attribute",
                    format!("map SVG must not declare the event handler `{attribute_name}` on `<{name}>`"),
                ));
            }
            if attribute_name == "href" && !attribute.value().starts_with('#') {
                problems.push(problem(
                    "case.map_svg_external_reference",
                    format!(
                        "map SVG `href` on `<{name}>` must be a same-document fragment such as `#shape`, found `{}`",
                        attribute.value()
                    ),
                ));
            }
        }
    }

    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codes(source: &str) -> Vec<&'static str> {
        check_map_svg(source)
            .into_iter()
            .map(|problem| problem.code)
            .collect()
    }

    const SAFE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 60">
  <rect id="parlor" x="0" y="0" width="40" height="30" />
  <use href="#parlor" x="50" />
</svg>
"##;

    #[test]
    fn accepts_a_safe_map() {
        assert!(check_map_svg(SAFE).is_empty());
    }

    #[test]
    fn rejects_malformed_xml_without_reporting_anything_else() {
        assert_eq!(codes("<svg viewBox=\"0 0 1 1\">"), ["case.map_svg_invalid"]);
    }

    #[test]
    fn rejects_a_dtd_so_an_entity_bomb_cannot_reach_a_renderer() {
        assert_eq!(
            codes(
                r#"<!DOCTYPE svg [<!ENTITY a "aaaaaaaaaa">]>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1"><title>&a;</title></svg>"#
            ),
            ["case.map_svg_invalid"]
        );
    }

    #[test]
    fn requires_an_svg_root_with_a_view_box() {
        assert_eq!(codes("<plan></plan>"), ["case.map_svg_root"]);
        assert_eq!(
            codes(r#"<svg xmlns="http://www.w3.org/2000/svg"><rect /></svg>"#),
            ["case.map_svg_view_box"]
        );
    }

    #[test]
    fn rejects_script_foreign_object_events_and_external_references() {
        assert_eq!(
            codes(
                SAFE.replace("<rect", "<script>alert(1)</script><rect")
                    .as_str()
            ),
            ["case.map_svg_forbidden_element"]
        );
        assert_eq!(
            codes(
                SAFE.replace("<rect", "<foreignObject><rect /></foreignObject><rect")
                    .as_str()
            ),
            ["case.map_svg_forbidden_element"]
        );
        assert_eq!(
            codes(
                SAFE.replace("<rect id", "<rect onclick=\"x()\" id")
                    .as_str()
            ),
            ["case.map_svg_event_attribute"]
        );
        assert_eq!(
            codes(SAFE.replace("<rect id", "<rect ONLOAD=\"x()\" id").as_str()),
            ["case.map_svg_event_attribute"]
        );
        assert_eq!(
            codes(
                SAFE.replace("href=\"#parlor\"", "href=\"https://example.com/a.svg\"")
                    .as_str()
            ),
            ["case.map_svg_external_reference"]
        );
    }

    #[test]
    fn rejects_a_namespaced_xlink_href_that_leaves_the_document() {
        let source = SAFE
            .replace(
                "xmlns=\"http://www.w3.org/2000/svg\"",
                "xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\"",
            )
            .replace(
                "href=\"#parlor\"",
                "xlink:href=\"http://example.com/a.svg\"",
            );
        assert_eq!(codes(&source), ["case.map_svg_external_reference"]);
    }
}

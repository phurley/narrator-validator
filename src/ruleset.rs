//! Immutable, versioned command catalogs supplied by Narrator.

use serde::{Deserialize, Serialize};

pub const STANDARD_MYSTERY_RULESET_ID: &str = "ruleset.standard_mystery";
pub const STANDARD_MYSTERY_RULESET_VERSION_1: &str = "1.0.0";
pub const STANDARD_MYSTERY_RULESET_VERSION_2: &str = "2.0.0";
/// Latest standard mystery ruleset authored by this validator release.
pub const STANDARD_MYSTERY_RULESET_VERSION: &str = STANDARD_MYSTERY_RULESET_VERSION_2;

/// A story's exact ruleset selection. Released versions are append-only: an
/// existing `(id, version)` pair must never be changed in place.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RulesetReference {
    pub id: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRuleset {
    pub reference: RulesetReference,
    pub commands_yaml: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RulesetError {
    #[error("unknown ruleset `{id}`; supported rulesets: ruleset.standard_mystery@1.0.0 and ruleset.standard_mystery@2.0.0")]
    Unknown { id: String },
    #[error("ruleset `{id}` does not support version `{version}`; use version 1.0.0 or 2.0.0")]
    IncompatibleVersion { id: String, version: String },
}

pub fn resolve_ruleset(reference: &RulesetReference) -> Result<ResolvedRuleset, RulesetError> {
    if reference.id != STANDARD_MYSTERY_RULESET_ID {
        return Err(RulesetError::Unknown {
            id: reference.id.clone(),
        });
    }
    let commands_yaml = match reference.version.as_str() {
        STANDARD_MYSTERY_RULESET_VERSION_1 => STANDARD_MYSTERY_COMMANDS_1_0_YAML,
        STANDARD_MYSTERY_RULESET_VERSION_2 => STANDARD_MYSTERY_COMMANDS_2_0_YAML,
        _ => {
            return Err(RulesetError::IncompatibleVersion {
                id: reference.id.clone(),
                version: reference.version.clone(),
            });
        }
    };
    Ok(ResolvedRuleset {
        reference: reference.clone(),
        commands_yaml,
    })
}

// This is the immutable 1.0.0 catalog. Add a new version instead of editing
// command semantics after release.
const STANDARD_MYSTERY_COMMANDS_1_0_YAML: &str = r#"commands:
  - id: command.move
    name: Move
    description: Travel to a setting connected to the player's current location.
    parameters:
      - name: destination
        description: The setting the player wants to reach.
        types: [setting]
        min: 1
        max: 1
    effects:
      - operation: move
        subjects: [player]
        setting: param1
      - operation: advance_time
        route: route

  - id: command.open
    name: Open
    description: Open a container or the player's current setting.
    parameters:
      - name: target
        description: The container or current setting to open.
        types: [entity, setting]
        min: 1
        max: 1

  - id: command.search
    name: Search
    description: Search a container or the player's current setting.
    parameters:
      - name: target
        description: The container or current setting to search.
        types: [entity, setting]
        min: 1
        max: 1

  - id: command.examine
    name: Examine
    description: Inspect a person, place, or object for useful details.
    parameters:
      - name: target
        description: The person, place, or object to inspect.
        types: [entity, character, setting]
        min: 1
        max: 1

  - id: command.take
    name: Take
    description: Pick up an entity that is present and portable.
    parameters:
      - name: item
        description: The entity the player wants to carry.
        types: [entity]
        min: 1
        max: 1

  - id: command.drop
    name: Drop
    description: Put a carried entity down in the player's current setting.
    parameters:
      - name: item
        description: The carried entity the player wants to put down.
        types: [entity]
        min: 1
        max: 1

  - id: command.use
    name: Use
    description: Use one entity on another entity or setting.
    parameters:
      - name: item
        description: The entity being used.
        types: [entity]
        min: 1
        max: 1
      - name: target
        description: The optional entity or setting the item will affect.
        types: [entity, setting]
        min: 0
        max: 1

  - id: command.question
    name: Question
    description: Ask a character about a known person, place, event, object, or deduction.
    parameters:
      - name: character
        description: The character being questioned.
        types: [character]
        min: 1
        max: 1
      - name: topic
        description: The optional subject being discussed.
        types: [character, setting, event, entity, deduction]
        min: 0
        max: 5

  - id: command.deduce
    name: Deduce
    description: Interpret one to three notebook facts or prior deductions as a new theory.

  - id: command.solve
    name: Solve
    description: Accuse a suspect using an established solution deduction.
    parameters:
      - name: suspect
        description: The character being accused.
        types: [character]
        min: 1
        max: 1
      - name: theory
        description: The established deduction offered as the solution.
        types: [deduction]
        min: 1
        max: 1
"#;

// This is the immutable 2.0.0 catalog. Its command shapes match 1.0.0 while
// candidate eligibility is declared on each gameplay parameter.
const STANDARD_MYSTERY_COMMANDS_2_0_YAML: &str = r#"commands:
  - id: command.move
    name: Move
    description: Travel to a setting connected to the player's current location.
    parameters:
      - name: destination
        description: The setting the player wants to reach.
        types: [setting]
        min: 1
        max: 1
        candidates:
          from: [reachable]
    effects:
      - operation: move
        subjects: [player]
        setting: param1
      - operation: advance_time
        route: route

  - id: command.open
    name: Open
    description: Open a container or the player's current setting.
    parameters:
      - name: target
        description: The container or current setting to open.
        types: [entity, setting]
        min: 1
        max: 1
        candidates:
          from: [current_location]

  - id: command.search
    name: Search
    description: Search a container or the player's current setting.
    parameters:
      - name: target
        description: The container or current setting to search.
        types: [entity, setting]
        min: 1
        max: 1
        candidates:
          from: [current_location]

  - id: command.examine
    name: Examine
    description: Inspect a person, place, or object for useful details.
    parameters:
      - name: target
        description: The person, place, or object to inspect.
        types: [entity, character, setting]
        min: 1
        max: 1
        candidates:
          from: [current_location, inventory]

  - id: command.take
    name: Take
    description: Pick up an entity that is present and portable.
    parameters:
      - name: item
        description: The entity the player wants to carry.
        types: [entity]
        min: 1
        max: 1
        candidates:
          from: [current_location]
          capabilities: [portable]

  - id: command.drop
    name: Drop
    description: Put a carried entity down in the player's current setting.
    parameters:
      - name: item
        description: The carried entity the player wants to put down.
        types: [entity]
        min: 1
        max: 1
        candidates:
          from: [inventory]
          capabilities: [portable]

  - id: command.use
    name: Use
    description: Use one entity on another entity or setting.
    parameters:
      - name: item
        description: The entity being used.
        types: [entity]
        min: 1
        max: 1
        candidates:
          from: [inventory]
      - name: target
        description: The optional entity or setting the item will affect.
        types: [entity, setting]
        min: 0
        max: 1
        candidates:
          from: [current_location, inventory]

  - id: command.question
    name: Question
    description: Ask a character about a known person, place, event, object, or deduction.
    parameters:
      - name: character
        description: The character being questioned.
        types: [character]
        min: 1
        max: 1
        candidates:
          from: [current_location]
      - name: topic
        description: The optional subject being discussed.
        types: [character, setting, event, entity, deduction]
        min: 0
        max: 5
        candidates:
          from: [known]

  - id: command.deduce
    name: Deduce
    description: Interpret one to three notebook facts or prior deductions as a new theory.

  - id: command.solve
    name: Solve
    description: Accuse a suspect using an established solution deduction.
    parameters:
      - name: suspect
        description: The character being accused.
        types: [character]
        min: 1
        max: 1
        candidates:
          from: [known]
      - name: theory
        description: The established deduction offered as the solution.
        types: [deduction]
        min: 1
        max: 1
        candidates:
          from: [established]
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn released_standard_catalog_is_stable_and_claim_free() {
        let resolved = resolve_ruleset(&RulesetReference {
            id: STANDARD_MYSTERY_RULESET_ID.to_string(),
            version: "1.0.0".to_string(),
        })
        .expect("standard ruleset");
        let document: serde_yaml::Value =
            serde_yaml::from_str(resolved.commands_yaml).expect("catalog YAML");
        let commands = document["commands"].as_sequence().expect("commands");
        let ids = commands
            .iter()
            .map(|command| command["id"].as_str().expect("command id"))
            .collect::<Vec<_>>();
        assert_eq!(
            ids,
            [
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
            ]
        );
        assert!(!resolved.commands_yaml.contains("command.claim"));
        assert!(!resolved.commands_yaml.contains("candidates:"));
    }

    #[test]
    fn standard_catalog_2_0_declares_exact_candidate_contracts() {
        let resolved = resolve_ruleset(&RulesetReference {
            id: STANDARD_MYSTERY_RULESET_ID.to_string(),
            version: "2.0.0".to_string(),
        })
        .expect("standard ruleset 2.0");
        let document: serde_yaml::Value =
            serde_yaml::from_str(resolved.commands_yaml).expect("catalog YAML");
        let commands = document["commands"].as_sequence().expect("commands");

        let contract = |command_id: &str, parameter_name: &str| {
            let command = commands
                .iter()
                .find(|command| command["id"].as_str() == Some(command_id))
                .unwrap_or_else(|| panic!("missing {command_id}"));
            let parameter = command["parameters"]
                .as_sequence()
                .expect("parameters")
                .iter()
                .find(|parameter| parameter["name"].as_str() == Some(parameter_name))
                .unwrap_or_else(|| panic!("missing {command_id}.{parameter_name}"));
            let from = parameter["candidates"]["from"]
                .as_sequence()
                .expect("candidate sources")
                .iter()
                .map(|source| source.as_str().expect("candidate source"))
                .collect::<Vec<_>>();
            let capabilities = parameter["candidates"]["capabilities"]
                .as_sequence()
                .map(|values| {
                    values
                        .iter()
                        .map(|capability| capability.as_str().expect("capability"))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            (from, capabilities)
        };

        for (command, parameter, from, capabilities) in [
            ("command.move", "destination", vec!["reachable"], vec![]),
            ("command.open", "target", vec!["current_location"], vec![]),
            ("command.search", "target", vec!["current_location"], vec![]),
            (
                "command.examine",
                "target",
                vec!["current_location", "inventory"],
                vec![],
            ),
            (
                "command.take",
                "item",
                vec!["current_location"],
                vec!["portable"],
            ),
            ("command.drop", "item", vec!["inventory"], vec!["portable"]),
            ("command.use", "item", vec!["inventory"], vec![]),
            (
                "command.use",
                "target",
                vec!["current_location", "inventory"],
                vec![],
            ),
            (
                "command.question",
                "character",
                vec!["current_location"],
                vec![],
            ),
            ("command.question", "topic", vec!["known"], vec![]),
            ("command.solve", "suspect", vec!["known"], vec![]),
            ("command.solve", "theory", vec!["established"], vec![]),
        ] {
            assert_eq!(contract(command, parameter), (from, capabilities));
        }
    }
}

//! Immutable, versioned command catalogs supplied by Narrator.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

pub const STANDARD_MYSTERY_RULESET_ID: &str = "ruleset.standard_mystery";
pub const STANDARD_MYSTERY_RULESET_VERSION_1: &str = "1.0.0";
pub const STANDARD_MYSTERY_RULESET_VERSION_2: &str = "2.0.0";
pub const STANDARD_MYSTERY_RULESET_VERSION_3: &str = "3.0.0";
pub const STANDARD_MYSTERY_RULESET_VERSION_4: &str = "4.0.0";
pub const STANDARD_MYSTERY_RULESET_VERSION_5: &str = "5.0.0";
pub const STANDARD_MYSTERY_RULESET_VERSION_6: &str = "6.0.0";
pub const STANDARD_MYSTERY_RULESET_VERSION_7: &str = "7.0.0";
/// Latest standard mystery ruleset authored by this validator release.
pub const STANDARD_MYSTERY_RULESET_VERSION: &str = STANDARD_MYSTERY_RULESET_VERSION_7;

/// tagStandard41h12 IDs 2000 through 2112 inclusive, immediately below the
/// scanner-control reservation at 2113/2114, are permanently reserved for
/// ruleset-owned answer decks (Story Format 3.7). Ruleset 7.0.0's 29 cards
/// occupy 2084-2112; 2000-2083 remain unassigned headroom for future
/// ruleset-owned decks.
pub const ANSWER_DECK_TAG_ID_MIN: i64 = 2000;
pub const ANSWER_DECK_TAG_ID_MAX: i64 = 2112;

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
    /// Stable semantic identities for mechanics whose availability is selected
    /// by game-instance policy. Consumers can filter by these capabilities
    /// without copying the ruleset's command definitions.
    pub command_capabilities: &'static [RulesetCommandCapability],
    /// The ruleset-owned answer-deck catalog (Story Format 3.7's `answer.*`
    /// subjects), merged into a story's definitions the same way
    /// `commands_yaml` is. `None` for rulesets published before answer decks
    /// existed.
    pub answers_yaml: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RulesetCommandCapability {
    pub command_id: &'static str,
    pub mechanic: &'static str,
    pub enabled_when: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RulesetError {
    #[error("unknown ruleset `{id}`; supported rulesets: ruleset.standard_mystery@1.0.0, @2.0.0, @3.0.0, @4.0.0, @5.0.0, @6.0.0, and @7.0.0")]
    Unknown { id: String },
    #[error(
        "ruleset `{id}` does not support version `{version}`; use version 1.0.0, 2.0.0, 3.0.0, 4.0.0, 5.0.0, 6.0.0, or 7.0.0"
    )]
    IncompatibleVersion { id: String, version: String },
}

pub fn resolve_ruleset(reference: &RulesetReference) -> Result<ResolvedRuleset, RulesetError> {
    if reference.id != STANDARD_MYSTERY_RULESET_ID {
        return Err(RulesetError::Unknown {
            id: reference.id.clone(),
        });
    }
    let (commands_yaml, command_capabilities, answers_yaml) = match reference.version.as_str() {
        STANDARD_MYSTERY_RULESET_VERSION_1 => (
            STANDARD_MYSTERY_COMMANDS_1_0_YAML,
            LEGACY_NOTEBOOK_COMMAND_CAPABILITIES,
            None,
        ),
        STANDARD_MYSTERY_RULESET_VERSION_2 => (
            STANDARD_MYSTERY_COMMANDS_2_0_YAML,
            LEGACY_NOTEBOOK_COMMAND_CAPABILITIES,
            None,
        ),
        STANDARD_MYSTERY_RULESET_VERSION_3 => (
            standard_mystery_commands_3_0_yaml(),
            LEGACY_NOTEBOOK_COMMAND_CAPABILITIES,
            None,
        ),
        STANDARD_MYSTERY_RULESET_VERSION_4 => (
            standard_mystery_commands_4_0_yaml(),
            NOTEBOOK_COMMAND_CAPABILITIES,
            None,
        ),
        STANDARD_MYSTERY_RULESET_VERSION_5 => (
            standard_mystery_commands_5_0_yaml(),
            RECONCILIATION_COMMAND_CAPABILITIES,
            None,
        ),
        STANDARD_MYSTERY_RULESET_VERSION_6 => (
            STANDARD_MYSTERY_COMMANDS_6_0_YAML,
            RECONCILIATION_COMMAND_CAPABILITIES,
            None,
        ),
        STANDARD_MYSTERY_RULESET_VERSION_7 => (
            // command.solve stays byte-for-byte parameterless: the multi-step
            // solve session is backend-owned state, not a re-asserted
            // command parameter. The 6.0.0 command catalog carries over
            // unchanged.
            STANDARD_MYSTERY_COMMANDS_6_0_YAML,
            RECONCILIATION_COMMAND_CAPABILITIES,
            Some(STANDARD_MYSTERY_ANSWERS_7_0_YAML),
        ),
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
        command_capabilities,
        answers_yaml,
    })
}

const LEGACY_NOTEBOOK_COMMAND_CAPABILITIES: &[RulesetCommandCapability] = &[
    RulesetCommandCapability {
        command_id: "command.deduce",
        mechanic: "establish_deduction",
        enabled_when: "manual_deductions",
    },
    RulesetCommandCapability {
        command_id: "command.solve",
        mechanic: "submit_solution",
        enabled_when: "always",
    },
];

const NOTEBOOK_COMMAND_CAPABILITIES: &[RulesetCommandCapability] = &[
    RulesetCommandCapability {
        command_id: "command.claim",
        mechanic: "claim_fact",
        enabled_when: "manual_facts",
    },
    RulesetCommandCapability {
        command_id: "command.deduce",
        mechanic: "establish_deduction",
        enabled_when: "manual_deductions",
    },
    RulesetCommandCapability {
        command_id: "command.solve",
        mechanic: "submit_solution",
        enabled_when: "always",
    },
];

const RECONCILIATION_COMMAND_CAPABILITIES: &[RulesetCommandCapability] = &[
    RulesetCommandCapability {
        command_id: "command.claim",
        mechanic: "claim_fact",
        enabled_when: "manual_facts",
    },
    RulesetCommandCapability {
        command_id: "command.deduce",
        mechanic: "establish_deduction",
        enabled_when: "manual_deductions",
    },
    RulesetCommandCapability {
        command_id: "command.reconcile",
        mechanic: "reconcile_notebooks",
        enabled_when: "multiple_players_with_unshared_facts",
    },
    RulesetCommandCapability {
        command_id: "command.solve",
        mechanic: "submit_solution",
        enabled_when: "always",
    },
];

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

fn standard_mystery_commands_3_0_yaml() -> &'static str {
    const LEGACY_SOLVE: &str = r#"  - id: command.solve
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
    const QUESTION_SOLVE: &str = r#"  - id: command.solve
    name: Solve
    description: Answer the story's authored solution questions with physical cards.
"#;
    static CATALOG: OnceLock<String> = OnceLock::new();
    CATALOG
        .get_or_init(|| {
            let resolved = STANDARD_MYSTERY_COMMANDS_2_0_YAML.replace(LEGACY_SOLVE, QUESTION_SOLVE);
            assert_ne!(resolved, STANDARD_MYSTERY_COMMANDS_2_0_YAML);
            resolved
        })
        .as_str()
}

fn standard_mystery_commands_4_0_yaml() -> &'static str {
    const DEDUCE: &str = r#"  - id: command.deduce
    name: Deduce
    description: Interpret one to three notebook facts or prior deductions as a new theory.
"#;
    const NOTEBOOK_COMMANDS: &str = r#"  - id: command.claim
    name: Claim
    description: Deliberately add one available fact to the player's notebook.

  - id: command.deduce
    name: Deduce
    description: Deliberately establish an authoritative notebook deduction from one to three known facts or prior deductions.
"#;
    static CATALOG: OnceLock<String> = OnceLock::new();
    CATALOG
        .get_or_init(|| {
            let resolved = standard_mystery_commands_3_0_yaml().replace(DEDUCE, NOTEBOOK_COMMANDS);
            assert_ne!(resolved, standard_mystery_commands_3_0_yaml());
            resolved
        })
        .as_str()
}

fn standard_mystery_commands_5_0_yaml() -> &'static str {
    const SOLVE: &str = r#"  - id: command.solve
    name: Solve
    description: Answer the story's authored solution questions with physical cards.
"#;
    const RECONCILE_AND_SOLVE: &str = r#"  - id: command.reconcile
    name: Reconcile
    description: Compare claimed notebook facts with every joined player.

  - id: command.solve
    name: Solve
    description: Answer the story's authored solution questions with physical cards.
"#;
    static CATALOG: OnceLock<String> = OnceLock::new();
    CATALOG
        .get_or_init(|| {
            let resolved = standard_mystery_commands_4_0_yaml().replace(SOLVE, RECONCILE_AND_SOLVE);
            assert_ne!(resolved, standard_mystery_commands_4_0_yaml());
            resolved
        })
        .as_str()
}

// This is the immutable 6.0.0 catalog. Identical to 5.0.0's reconciliation
// catalog with `default_cost_minutes` added to every command, per ADR-004.
// `command.move`'s default is unused (its real cost flows through the
// existing route mechanism) but is still authored, as is `0` for every
// notebook command (`command.claim`, `command.deduce`, `command.reconcile`,
// `command.solve`), since the field is required on every command.
const STANDARD_MYSTERY_COMMANDS_6_0_YAML: &str = r#"commands:
  - id: command.move
    name: Move
    description: Travel to a setting connected to the player's current location.
    default_cost_minutes: 0
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
    default_cost_minutes: 1
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
    default_cost_minutes: 5
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
    default_cost_minutes: 3
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
    default_cost_minutes: 1
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
    default_cost_minutes: 1
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
    default_cost_minutes: 2
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
    default_cost_minutes: 4
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

  - id: command.claim
    name: Claim
    description: Deliberately add one available fact to the player's notebook.
    default_cost_minutes: 0

  - id: command.deduce
    name: Deduce
    description: Deliberately establish an authoritative notebook deduction from one to three known facts or prior deductions.
    default_cost_minutes: 0

  - id: command.reconcile
    name: Reconcile
    description: Compare claimed notebook facts with every joined player.
    default_cost_minutes: 0

  - id: command.solve
    name: Solve
    description: Answer the story's authored solution questions with physical cards.
    default_cost_minutes: 0
"#;

// This is the immutable 7.0.0 answer-deck catalog: 29 cards (10 motive, 8
// time, 11 method), verbatim from docs/answer-deck-vocabulary.md, the
// authoritative source. `tag_id`s are assigned descending from 2112 and
// occupy 2084-2112 of the reserved 2000-2112 range; append-only and
// immutable once shipped, exactly like a commands catalog.
const STANDARD_MYSTERY_ANSWERS_7_0_YAML: &str = r#"answers:
  - id: answer.motive.greed
    tag_id: 2112
    name: Greed
    description: Done for money, property, a payout, an inheritance, or the value of the thing itself.

  - id: answer.motive.jealousy
    tag_id: 2111
    name: Jealousy
    description: Done because a rival had what the culprit wanted — a person, a place, a reputation, a prize.

  - id: answer.motive.revenge
    tag_id: 2110
    name: Revenge
    description: Done to settle a past wrong, real or only believed.

  - id: answer.motive.fear_of_exposure
    tag_id: 2109
    name: Fear of exposure
    description: Done to keep a secret buried — to silence a witness, destroy a record, or stop an investigation.

  - id: answer.motive.self_preservation
    tag_id: 2108
    name: Self-preservation
    description: Done to escape immediate danger to the culprit's own body; self-defence, panic in a struggle, a way out of a trap.

  - id: answer.motive.fear_for_another
    tag_id: 2107
    name: Fear for someone else
    description: Done to shield another person from harm, blame, or loss — a child, a partner, an accomplice.

  - id: answer.motive.love
    tag_id: 2106
    name: Love
    description: Done out of attachment to a person; to win them, keep them, or refuse to let them go.

  - id: answer.motive.ambition
    tag_id: 2105
    name: Ambition
    description: Done for position, standing, credit, or a legacy rather than for money.

  - id: answer.motive.desperation
    tag_id: 2104
    name: Desperation
    description: Done by someone cornered — debt, illness, eviction, a deadline — seeking relief rather than gain.

  - id: answer.motive.loyalty
    tag_id: 2103
    name: Loyalty
    description: Done on behalf of a person, family, employer, or cause the culprit felt bound to, including acting on orders.

  - id: answer.time.dawn
    tag_id: 2102
    name: At dawn
    description: First light until the sun is up.

  - id: answer.time.morning
    tag_id: 2101
    name: In the morning
    description: Sunrise until late morning.

  - id: answer.time.midday
    tag_id: 2100
    name: Around midday
    description: Late morning until early afternoon, across the midday meal.

  - id: answer.time.afternoon
    tag_id: 2099
    name: In the afternoon
    description: Early afternoon until the light starts to go.

  - id: answer.time.evening
    tag_id: 2098
    name: In the evening
    description: Sunset through the early part of the night.

  - id: answer.time.night
    tag_id: 2097
    name: At night
    description: Full dark, but still before midnight.

  - id: answer.time.after_midnight
    tag_id: 2096
    name: After midnight
    description: Midnight until the small hours.

  - id: answer.time.before_dawn
    tag_id: 2095
    name: Before dawn
    description: The small hours until first light.

  - id: answer.method.struck
    tag_id: 2094
    name: Struck with something
    description: Blunt force — a weapon, a tool, an ordinary heavy object.

  - id: answer.method.stabbed
    tag_id: 2093
    name: Stabbed or cut
    description: A blade, a point, or broken glass.

  - id: answer.method.shot
    tag_id: 2092
    name: Shot
    description: A firearm or other projectile.

  - id: answer.method.poisoned
    tag_id: 2091
    name: Poisoned or drugged
    description: A substance given, hidden in food or drink, or substituted — whether meant to kill or only to incapacitate.

  - id: answer.method.strangled
    tag_id: 2090
    name: Strangled or smothered
    description: Air cut off by hands, a ligature, or an obstruction.

  - id: answer.method.drowned
    tag_id: 2089
    name: Drowned
    description: Held under, or unable to get out of the water.

  - id: answer.method.fell
    tag_id: 2088
    name: Killed by a fall
    description: A fall from height, down stairs, or onto something hard — pushed, dropped, or lost footing.

  - id: answer.method.fire
    tag_id: 2087
    name: Burned in a fire
    description: Fire, smoke, or an explosion.

  - id: answer.method.crushed
    tag_id: 2086
    name: Crushed or run down
    description: A vehicle, machinery, or a collapsing structure or load.

  - id: answer.method.neglect
    tag_id: 2085
    name: Left without help
    description: Medicine withheld, an injury left untreated, an alarm ignored, someone abandoned somewhere they could not survive.

  - id: answer.method.not_killed
    tag_id: 2084
    name: Not killed by anyone
    description: Illness, a failing heart, or a death nothing external caused — including the case where nobody died at all.
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn released_standard_catalog_is_byte_for_byte_stable_and_claim_free() {
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

        // This length and FNV-1a fingerprint were recorded from the released
        // 1.0.0 catalog before the 2.0.0 catalog was added. Together they make
        // an accidental edit fail without duplicating the complete YAML in the
        // test. A new command contract belongs in a new ruleset version.
        let fingerprint = resolved
            .commands_yaml
            .as_bytes()
            .iter()
            .fold(0xcbf29ce484222325_u64, |hash, byte| {
                (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
            });
        assert_eq!(resolved.commands_yaml.len(), 3_164);
        assert_eq!(fingerprint, 0xbcd9c7cdae74ca72);
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

    #[test]
    fn standard_catalog_3_0_changes_only_solve_to_authored_questions() {
        let version_2 = resolve_ruleset(&RulesetReference {
            id: STANDARD_MYSTERY_RULESET_ID.to_string(),
            version: STANDARD_MYSTERY_RULESET_VERSION_2.to_string(),
        })
        .expect("standard ruleset 2.0");
        let version_3 = resolve_ruleset(&RulesetReference {
            id: STANDARD_MYSTERY_RULESET_ID.to_string(),
            version: STANDARD_MYSTERY_RULESET_VERSION_3.to_string(),
        })
        .expect("standard ruleset 3.0");
        let document: serde_yaml::Value =
            serde_yaml::from_str(version_3.commands_yaml).expect("catalog YAML");
        let commands = document["commands"].as_sequence().expect("commands");
        let solve = commands
            .iter()
            .find(|command| command["id"].as_str() == Some("command.solve"))
            .expect("Solve command");
        assert!(solve.get("parameters").is_none());
        assert!(solve["description"]
            .as_str()
            .is_some_and(|description| description.contains("authored solution questions")));

        let v2_without_solve = version_2
            .commands_yaml
            .split("  - id: command.solve\n")
            .next()
            .expect("v2 prefix");
        let v3_without_solve = version_3
            .commands_yaml
            .split("  - id: command.solve\n")
            .next()
            .expect("v3 prefix");
        assert_eq!(v3_without_solve, v2_without_solve);
    }

    #[test]
    fn standard_catalog_4_0_adds_manual_notebook_commands_and_semantic_capabilities() {
        let resolved = resolve_ruleset(&RulesetReference {
            id: STANDARD_MYSTERY_RULESET_ID.to_string(),
            version: STANDARD_MYSTERY_RULESET_VERSION_4.to_string(),
        })
        .expect("standard ruleset 4.0");
        let document: serde_yaml::Value =
            serde_yaml::from_str(resolved.commands_yaml).expect("catalog YAML");
        let ids = document["commands"]
            .as_sequence()
            .expect("commands")
            .iter()
            .map(|command| command["id"].as_str().expect("command id"))
            .collect::<Vec<_>>();
        assert!(ids.contains(&"command.claim"));
        assert!(ids.contains(&"command.deduce"));
        assert_eq!(ids.iter().filter(|id| **id == "command.solve").count(), 1);
        assert_eq!(
            resolved.command_capabilities,
            [
                RulesetCommandCapability {
                    command_id: "command.claim",
                    mechanic: "claim_fact",
                    enabled_when: "manual_facts",
                },
                RulesetCommandCapability {
                    command_id: "command.deduce",
                    mechanic: "establish_deduction",
                    enabled_when: "manual_deductions",
                },
                RulesetCommandCapability {
                    command_id: "command.solve",
                    mechanic: "submit_solution",
                    enabled_when: "always",
                },
            ]
        );
    }

    #[test]
    fn standard_catalog_5_0_adds_only_parameterless_reconcile_and_its_capability() {
        let version_4 = resolve_ruleset(&RulesetReference {
            id: STANDARD_MYSTERY_RULESET_ID.to_string(),
            version: STANDARD_MYSTERY_RULESET_VERSION_4.to_string(),
        })
        .expect("standard ruleset 4.0");
        let resolved = resolve_ruleset(&RulesetReference {
            id: STANDARD_MYSTERY_RULESET_ID.to_string(),
            version: STANDARD_MYSTERY_RULESET_VERSION_5.to_string(),
        })
        .expect("standard ruleset 5.0");
        let document: serde_yaml::Value =
            serde_yaml::from_str(resolved.commands_yaml).expect("catalog YAML");
        let commands = document["commands"].as_sequence().expect("commands");
        let reconcile = commands
            .iter()
            .filter(|command| command["id"].as_str() == Some("command.reconcile"))
            .collect::<Vec<_>>();
        assert_eq!(reconcile.len(), 1);
        assert!(reconcile[0].get("parameters").is_none());
        assert!(reconcile[0].get("effects").is_none());

        let version_5_without_reconcile = resolved
            .commands_yaml
            .replace(
                "  - id: command.reconcile\n    name: Reconcile\n    description: Compare claimed notebook facts with every joined player.\n\n",
                "",
            );
        assert_eq!(version_5_without_reconcile, version_4.commands_yaml);
        assert_eq!(
            resolved.command_capabilities,
            [
                RulesetCommandCapability {
                    command_id: "command.claim",
                    mechanic: "claim_fact",
                    enabled_when: "manual_facts",
                },
                RulesetCommandCapability {
                    command_id: "command.deduce",
                    mechanic: "establish_deduction",
                    enabled_when: "manual_deductions",
                },
                RulesetCommandCapability {
                    command_id: "command.reconcile",
                    mechanic: "reconcile_notebooks",
                    enabled_when: "multiple_players_with_unshared_facts",
                },
                RulesetCommandCapability {
                    command_id: "command.solve",
                    mechanic: "submit_solution",
                    enabled_when: "always",
                },
            ]
        );
    }

    fn fnv1a(text: &str) -> u64 {
        text.as_bytes()
            .iter()
            .fold(0xcbf29ce484222325_u64, |hash, byte| {
                (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
            })
    }

    #[test]
    fn released_catalogs_1_0_through_5_0_are_byte_for_byte_pinned() {
        // Length and FNV-1a fingerprint recorded from each released catalog.
        // Any accidental edit to an already-published version fails here
        // without duplicating the complete YAML in the test. A new command
        // contract belongs in a new ruleset version, never an edit in place.
        for (version, expected_len, expected_fingerprint) in [
            (
                STANDARD_MYSTERY_RULESET_VERSION_1,
                3_164,
                0xbcd9c7cdae74ca72,
            ),
            (
                STANDARD_MYSTERY_RULESET_VERSION_2,
                3_868,
                0x3c94f7819056250f,
            ),
            (
                STANDARD_MYSTERY_RULESET_VERSION_3,
                3_489,
                0x9e6c0afc9739bddf,
            ),
            (
                STANDARD_MYSTERY_RULESET_VERSION_4,
                3_642,
                0x5ee57ab388c05f5d,
            ),
            (
                STANDARD_MYSTERY_RULESET_VERSION_5,
                3_763,
                0x5a3515089d128df6,
            ),
        ] {
            let resolved = resolve_ruleset(&RulesetReference {
                id: STANDARD_MYSTERY_RULESET_ID.to_string(),
                version: version.to_string(),
            })
            .unwrap_or_else(|_| panic!("standard ruleset {version}"));
            assert_eq!(
                resolved.commands_yaml.len(),
                expected_len,
                "catalog length changed for {version}"
            );
            assert_eq!(
                fnv1a(resolved.commands_yaml),
                expected_fingerprint,
                "catalog contents changed for {version}"
            );
        }
    }

    #[test]
    fn standard_catalog_6_0_adds_only_default_cost_minutes() {
        let version_5 = resolve_ruleset(&RulesetReference {
            id: STANDARD_MYSTERY_RULESET_ID.to_string(),
            version: STANDARD_MYSTERY_RULESET_VERSION_5.to_string(),
        })
        .expect("standard ruleset 5.0");
        let resolved = resolve_ruleset(&RulesetReference {
            id: STANDARD_MYSTERY_RULESET_ID.to_string(),
            version: STANDARD_MYSTERY_RULESET_VERSION_6.to_string(),
        })
        .expect("standard ruleset 6.0");

        assert_eq!(
            resolved.command_capabilities,
            version_5.command_capabilities
        );

        // Stripping every `default_cost_minutes` line from 6.0.0 must yield
        // exactly 5.0.0's catalog: same commands, same shapes, same order.
        let stripped: String = resolved
            .commands_yaml
            .lines()
            .filter(|line| !line.trim_start().starts_with("default_cost_minutes:"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(stripped.trim_end(), version_5.commands_yaml.trim_end());

        let document: serde_yaml::Value =
            serde_yaml::from_str(resolved.commands_yaml).expect("catalog YAML");
        let commands = document["commands"].as_sequence().expect("commands");
        assert!(!commands.is_empty());
        for command in commands {
            let id = command["id"].as_str().expect("command id");
            let cost = command["default_cost_minutes"].as_i64();
            assert!(
                cost.is_some_and(|minutes| minutes >= 0),
                "{id} is missing a non-negative default_cost_minutes"
            );
        }
    }

    #[test]
    fn every_published_ruleset_version_declares_default_cost_minutes_where_present() {
        // Acceptance criterion: every command in every published ruleset
        // version declares `default_cost_minutes` wherever the field exists
        // at all (6.0.0 onward). Versions before 6.0.0 never mention the
        // field, so they are exempt rather than silently satisfied.
        for version in [
            STANDARD_MYSTERY_RULESET_VERSION_1,
            STANDARD_MYSTERY_RULESET_VERSION_2,
            STANDARD_MYSTERY_RULESET_VERSION_3,
            STANDARD_MYSTERY_RULESET_VERSION_4,
            STANDARD_MYSTERY_RULESET_VERSION_5,
            STANDARD_MYSTERY_RULESET_VERSION_6,
        ] {
            let resolved = resolve_ruleset(&RulesetReference {
                id: STANDARD_MYSTERY_RULESET_ID.to_string(),
                version: version.to_string(),
            })
            .unwrap_or_else(|_| panic!("standard ruleset {version}"));
            let declares_field = resolved.commands_yaml.contains("default_cost_minutes");
            if !declares_field {
                continue;
            }
            let document: serde_yaml::Value =
                serde_yaml::from_str(resolved.commands_yaml).expect("catalog YAML");
            let commands = document["commands"].as_sequence().expect("commands");
            for command in commands {
                let id = command["id"].as_str().expect("command id");
                assert!(
                    command["default_cost_minutes"].as_i64().is_some(),
                    "{version}: {id} has no explicit default_cost_minutes"
                );
            }
        }
    }

    #[test]
    fn unknown_version_error_names_7_0_0() {
        let error = resolve_ruleset(&RulesetReference {
            id: STANDARD_MYSTERY_RULESET_ID.to_string(),
            version: "8.0.0".to_string(),
        })
        .expect_err("unpublished version must error");
        assert!(matches!(error, RulesetError::IncompatibleVersion { .. }));
        assert!(error.to_string().contains("7.0.0"));

        let unknown_id_error = resolve_ruleset(&RulesetReference {
            id: "ruleset.unknown".to_string(),
            version: "1.0.0".to_string(),
        })
        .expect_err("unknown ruleset id must error");
        assert!(matches!(unknown_id_error, RulesetError::Unknown { .. }));
        assert!(unknown_id_error.to_string().contains("7.0.0"));
    }

    #[test]
    fn ruleset_version_constant_points_at_7_0_0() {
        assert_eq!(STANDARD_MYSTERY_RULESET_VERSION, "7.0.0");
        assert_eq!(
            STANDARD_MYSTERY_RULESET_VERSION,
            STANDARD_MYSTERY_RULESET_VERSION_7
        );
    }

    #[test]
    fn standard_catalog_7_0_carries_over_6_0_commands_and_adds_the_answer_deck() {
        let version_6 = resolve_ruleset(&RulesetReference {
            id: STANDARD_MYSTERY_RULESET_ID.to_string(),
            version: STANDARD_MYSTERY_RULESET_VERSION_6.to_string(),
        })
        .expect("standard ruleset 6.0");
        let resolved = resolve_ruleset(&RulesetReference {
            id: STANDARD_MYSTERY_RULESET_ID.to_string(),
            version: STANDARD_MYSTERY_RULESET_VERSION_7.to_string(),
        })
        .expect("standard ruleset 7.0");

        assert_eq!(resolved.commands_yaml, version_6.commands_yaml);
        assert_eq!(
            resolved.command_capabilities,
            version_6.command_capabilities
        );

        let answers_yaml = resolved.answers_yaml.expect("7.0.0 declares answer decks");
        assert!(version_6.answers_yaml.is_none());

        let document: serde_yaml::Value =
            serde_yaml::from_str(answers_yaml).expect("answer catalog YAML");
        let answers = document["answers"].as_sequence().expect("answers");
        assert_eq!(answers.len(), 29);

        let motive = answers
            .iter()
            .filter(|answer| {
                answer["id"]
                    .as_str()
                    .is_some_and(|id| id.starts_with("answer.motive."))
            })
            .count();
        let time = answers
            .iter()
            .filter(|answer| {
                answer["id"]
                    .as_str()
                    .is_some_and(|id| id.starts_with("answer.time."))
            })
            .count();
        let method = answers
            .iter()
            .filter(|answer| {
                answer["id"]
                    .as_str()
                    .is_some_and(|id| id.starts_with("answer.method."))
            })
            .count();
        assert_eq!((motive, time, method), (10, 8, 11));

        let mut seen_tags = std::collections::BTreeSet::new();
        for answer in answers {
            let tag_id = answer["tag_id"].as_i64().expect("tag_id");
            assert!((ANSWER_DECK_TAG_ID_MIN..=ANSWER_DECK_TAG_ID_MAX).contains(&tag_id));
            assert!(seen_tags.insert(tag_id), "duplicate tag_id {tag_id}");
            assert!(answer["name"].as_str().is_some_and(|name| !name.is_empty()));
            assert!(answer["description"]
                .as_str()
                .is_some_and(|description| !description.is_empty()));
        }
        assert_eq!(seen_tags.len(), 29);
        assert_eq!(*seen_tags.iter().min().unwrap(), 2084);
        assert_eq!(*seen_tags.iter().max().unwrap(), 2112);
    }
}

# Migrating story format 2 to format 3

## Migrating deductions for automatic notebook policies

Update validator, backend, authoring WASM, and story CI together. Current
Format 3.4 stories that support all four notebook policies should select
`ruleset.standard_mystery@4.0.0`. Remove copied Claim, Deduce, and Solve
definitions; the ruleset supplies them and exports their policy capabilities.

Audit every deduction as if it will appear without confirmation the instant
its inputs become known:

1. Remove or rewrite `truth: false`, contradicted, speculative, and provisional
   conclusions. They need a future hypothesis mechanic, not authoritative
   deduction state.
2. Replace final accusations and physical answer copies with intermediate
   insights. Keep final card commitment in `solution.questions`, grading in
   Solve, and outcome text/precedence in end states.
3. Collapse mechanical relay chains and split overwhelming fan-out. Review
   `playability.deduction_graph.maximum_depth` and `largest_cascade_size` in the
   JSON report.
4. Run both automatic and fully manual entries in
   `playability.notebook_policies`. An older ruleset without Claim makes the
   manual-fact path explicitly inconclusive.

Maintained story guidance:

- **Simple Mystery:** replace any single final “culprit + weapon + location”
  deduction with separate evidence-backed intermediate notes, then express the
  final answer only through authored solution questions.
- **Island Retreat:** audit branching alibi, method, and access chains for
  automatic fan-out; keep each note useful on its own and remove any terminal
  deduction that merely repeats the complete solution row.
- **Quiet Kennel:** preserve its useful non-murder intermediate deductions,
  but decide explicitly whether the final kennel conclusion is an ordinary
  generic ending or a graded Solve result. Do not model the same terminal truth
  in both a deduction and the solution contract.

See [Automatic deductions and notebook safety](docs/automatic-deductions.md)
for the normative notebook semantics and Case Health expectations.

## Moving from Format 3.3 to 3.4 authored end states

Update the validator, runtime, and editor consumers together, then set
`case.format_version: "3.4.0"`. Rename `win_states.yaml` and its root to
`end_states.yaml` / `end_states`, preserving every ID, condition, point gate,
name, text value, and sequence position. Add `outcome: won` and
`resolution: full` to preserve the exact legacy behavior. Existing `win.*` IDs
may remain stable during migration.

New entries may use `end.*` IDs and express `won`/`full`, `won`/`partial`, or
`lost`/`failure`. Add a quoted `at_or_after: "HH:MM"` threshold when clock time
is part of the condition. All conditions are conjunctive and the first
satisfied state wins authored precedence, so order specific full resolutions
before broader partial outcomes. See
[Story Format 3.4](docs/story-format-3.4.md) for the complete contract.

The transition validator still accepts `win_states` as ordered `won`/`full`
states and emits a migration warning. Do not define both roots.

## Moving from Format 3.2 to 3.3 authored Solve questions

Update every validator/runtime/editor/scanner consumer before changing the
story. Then select `case.format_version: "3.3.0"` and
`ruleset.standard_mystery@3.0.0`. Replace the complete legacy
victim/culprit/weapon/location/time/deduction solution with `win_state` and one
to four `questions` as documented in
[Story Format 3.3](docs/story-format-3.3.md). Do not mix the contracts.

Each answer contains one to five unique physical setting, character, or entity
IDs. Bind every answer subject in `deck.yaml`, and do not reuse a physical card
between questions. Remove `requires` and positive `minimum_points` from the
win state selected by `solution.win_state`; the exact question answers now own
that completion condition. Keep conditions on unrelated generic endings.

The 3.0 ruleset's Solve command has no authored parameters. Remove copied
legacy Solve definitions and any runtime assumption that the command submits a
fixed suspect plus deduction. Prompt references remain governed independently
by the Format 3.2 `reference_text_v1` negotiation.

## Moving from Format 3.1 to 3.2 reference-aware text

Format 3.1 stories require no changes. To use reference-aware prose, first
update every validating and executing consumer to validator 1.2-capable code.
Then set `case.format_version: "3.2.0"`, add the ordered unique feature list
`features: [reference_text_v1]`, and replace copied narrative names/details only
in fields and target paths listed in
[the Format 3.2 matrix](docs/story-format-3.2.md#authoritative-disclosure-and-path-matrix).

Do not add `features` before all consumers negotiate it: validator 1.1 rejects
the field by design, and a 1.2 consumer that does not advertise
`reference_text_v1` rejects the repository before interpreting prose. Escaped
`\[[...]]` remains literal. IDs, gates, truth, effects, author notes, voice data,
and other mechanical or private values cannot be exposed through player-safe
text.

Validator `1.0.0` authors story format `3.0.0`. Format 2 is a migration-only
input: the validator reports one `format.incompatible_version` diagnostic and
does not partially interpret the repository as format 3. Complete the changes
below as one repository migration, then set `case.format_version: "3.0.0"` and
validate the complete snapshot.

Keep the old repository on a migration branch until the native CLI, authoring
browser package, and backend all report validator `1.0.0`, story format
`3.0.0`, and zero diagnostics.

## 1. Use canonical root files

Move `case` and `solution` into `case.yaml`. Keep only `settings` and `routes`
in `settings.yaml`. Put each other known section in its matching file:

| File | Root section |
| --- | --- |
| `case.yaml` | `case`, `solution` |
| `settings.yaml` | `settings`, `routes` |
| `characters.yaml` | `characters` |
| `entities.yaml` | `entities` |
| `events.yaml` | `events` |
| `deductions.yaml` | `deductions` |
| `flags.yaml` | `flags` |
| `commands.yaml` | story-specific `commands` only |
| `triggers.yaml` | `triggers` |
| `deck.yaml` | `cards` |

Remove standalone `facts`, `tags`, and format-2 `clues` sections. Facts stay
nested under their character, entity, setting, event, or trigger owner. Extra
YAML files may still hold unrelated project metadata.

## 2. Make the case and answer explicit

Use semantic versions, typed player limits, a versioned ruleset, deterministic
time, and explicit entry and exit settings:

```yaml
case:
  id: case.example
  format_version: "3.0.0"
  ruleset:
    id: ruleset.standard_mystery
    version: "1.0.0"
  initial_time: "20:00"
  players: { min: 2, max: 6 }
  entry_settings: [setting.lounge]
  exit_settings: [setting.lounge]

solution:
  victim: character.victim
  culprit: character.culprit
  weapon: entity.weapon
  location: setting.study
  time: "20:15"
  deduction: deduction.solution
  narrator_guidance:
    motive: The specific reason for the crime.
    method: The authoritative sequence of actions.
    proof_summary: The evidence chain players must establish.
```

Delete duplicate answer data from case prose, characters, events, or custom
metadata. `solution` is the mechanical answer. Private explanation belongs in
its `narrator_guidance`; player-safe discoveries belong in facts.

## 3. Separate semantic objects from the physical deck

Remove every inline `tag_id` from settings, characters, entities, and commands.
Replace the removed `tags` catalog with a physical manifest:

```yaml
# deck.yaml
cards:
  - tag_id: 0
    subject: setting.lounge
  - tag_id: 1
    subject: entity.weapon
  - tag_id: 2
    subject: command.examine
```

Each integer is between 0 and 2114. Both `tag_id` and `subject` must be unique.
A semantic object does not need a card. Standard command cards point at the
ruleset command ID and do not copy command behavior into `commands.yaml`.

## 4. Select the maintained ruleset

Select `ruleset.standard_mystery` version `1.0.0`. Remove copied definitions of
`command.move`, `open`, `search`, `examine`, `take`, `drop`, `use`, `question`,
`deduce`, and `solve`. Keep only story extensions in `commands.yaml` and use
the canonical ordered parameter contract:

```yaml
commands:
  - id: command.investigate
    name: Investigate
    parameters:
      - name: target
        types: [entity, character, setting]
        min: 1
        max: 1
```

Replace legacy singular `type`, `required`, and `accepts` parameter fields with
`types`, `min`, and `max`.

## 5. Convert discovery to facts, testimony, and durable gates

A fact's nesting owner is its discovery source. Do not repeat an ambiguous
source ID or use an untyped `requires` bag. Use `on` for the action that reveals
the fact and `when.all` for durable state. Within an owner-nested fact, `owner`
means that character, entity, setting, or event:

```yaml
entities:
  - id: entity.weapon
    type: object
    name: Display knife
    description: A polished ceremonial knife.
    facts:
      - id: fact.weapon_washed
        statement: The knife was washed recently.
        narrative_detail: Cleaner remains beneath the collar.
        on:
          command: command.examine
          parameters: { target: owner }
      - id: fact.weapon_implicates_culprit
        statement: The residue matches the culprit's cleaner.
        when:
          all:
            - knows: fact.weapon_washed
            - flag: flag.cleaner_found
```

Supported durable predicates are `at`, `owns`, `knows`, `flag`, `completed`,
and typed `time`. Testimony remains ordered player-safe dialogue; use its
`requires` IDs to make a line available and `reveals` to learn nested facts.
An ordinary fact with neither `on` nor `when` is learned on the opening turn
unless testimony or trigger ownership gives it a more specific source.

## 6. Replace generic trigger gates with action matching

Every action-driven trigger uses `on.command` plus semantic parameters. Put
durable prerequisites in `when.all`; do not use a top-level `command` field or
a generic ID gate:

```yaml
triggers:
  - id: trigger.test_weapon
    name: Test the weapon
    on:
      command: command.investigate
      parameters: { target: entity.weapon }
    when:
      all:
        - at: setting.study
    once: true
    after: 20m
    facts:
      - id: fact.victim_blood_found
        statement: The victim's blood remains beneath the knife collar.
```

Trigger-owned facts are learned when the trigger completes. With `after`, the
facts and effects resolve only when the delayed result completes; do not also
add an ambiguous discovery gate to those facts.

## 7. Use the shared effect and win contract

Commands and triggers use only `set_flag`, `move`, `transform`, `reveal`,
`conceal`, `learn_fact`, `establish_deduction`, `describe`, `advance_time`,
`win`, and `lose`. Use authored IDs, `player`, positional `paramN` bindings, or
the matched `route`. Delayed flag updates use `set_flag.after`.

Deductions use one to three fact or deduction `inputs`. The final true
deduction must match `solution.deduction`; establishing it determines the
winner. Coordinate any explicit `win` effect with that same answer instead of
creating a second solution path.

## 8. Apply the strict disclosure boundary

- `description`, `portrayal`, and testimony are safe before discovery.
- Fact `statement` and `narrative_detail` are safe only after discovery.
- `narrator_guidance` is private narration input.
- `author_notes` is author-only and never runtime narration input.
- IDs, routes, gates, effects, times, truth, placement, and solution references
  are mechanical state, not prose to reveal.

Remove `examined`, `forensic`, and other duplicate discovery prose. Move safe
gated detail into `narrative_detail`; move secrets, motives, methods, and
performance direction into the appropriate `narrator_guidance` object.

## 9. Verify the complete migration

From a fresh checkout, run:

```sh
cargo run --manifest-path /path/to/narrator-validator/Cargo.toml -- /path/to/story
cargo run --manifest-path /path/to/narrator-validator/Cargo.toml -- --format json /path/to/story
```

Both commands must report validator `1.0.0`, format `3.0.0`, zero errors, and
zero warnings. Then open the same commit in the author, run Test Play, create a
backend game, exercise physical-card lookup and delayed results, establish the
solution deduction, and confirm the winner. Do not change the format version
until all file moves are complete: a format-2 snapshot is intentionally stopped
before schema interpretation, while a format-3 snapshot is validated strictly.

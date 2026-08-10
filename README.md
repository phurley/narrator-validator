# narrator-validator

The shared semantic validator for complete Narrator story repositories.

One Rust implementation is exposed in three forms:

- a library for `narrator-backend`;
- a WebAssembly function for the TypeScript editor;
- a CLI (and composite GitHub Action) for repository checks.

The validator always receives the complete repository snapshot. Diagnostics
have stable rule codes, JSON pointers, and best-effort source ranges so callers
can render the same result as an API response, an editor marker, or a CI
annotation.

Known top-level sections use canonical root filenames so the validator, editor,
and game engine agree on where content lives:

- `settings.yaml`: `case`, `solution`, `settings`, and `routes`
- `characters.yaml`, `entities.yaml`, `events.yaml`, `deductions.yaml`,
  `flags.yaml`, `commands.yaml`, and `triggers.yaml`: the matching section
- `clues.yaml`: legacy format-1 `clues`

Other top-level metadata may remain in additional YAML files. The former
`tags` section is removed; use flags for authored world state.

## CLI

```sh
cargo run -- /path/to/story
cargo run -- --format json /path/to/story
cargo run -- --format github /path/to/story
```

Exit status is `0` for a valid repository, `1` for validation errors, and `2`
for CLI or filesystem errors. Warnings do not make a repository invalid.

## Rust

```rust
use narrator_validator::{validate, SourceFile};

let report = validate(&[SourceFile {
    path: "settings.yaml".into(),
    source: "settings: []\n".into(),
}]);
```

The core has no filesystem, async-runtime, HTTP, or GitHub dependency.

## Browser / React

Install the CLI matching the crate's pinned `wasm-bindgen` version, then build
the browser package:

```sh
cargo install wasm-bindgen-cli --version 0.2.100 --locked
node scripts/build-web-package.mjs
```

The generated npm package is written to `pkg/`. Install that directory in the
React application during local development:

```sh
pnpm add ../narrator-validator/pkg
```

The package owns WASM initialization and exposes an asynchronous, typed API:

```ts
import { validateRepository, type SourceFile } from "narrator-validator";

const files: SourceFile[] = [
  { path: "settings.yaml", source: "settings: []\n" },
];
const report = await validateRepository(files);
```

`validateRepository` always validates the complete in-memory snapshot and
returns a typed `ValidationReport`. For live editor feedback, call it from a
Web Worker so parsing does not block React input:

```ts
// validator.worker.ts
import { validateRepository, type SourceFile } from "narrator-validator";

self.onmessage = async (event: MessageEvent<SourceFile[]>) => {
  self.postMessage(await validateRepository(event.data));
};
```

The low-level wasm-bindgen exports remain available from
`narrator-validator/raw`.

## GitHub Actions

This repository includes a composite action:

```yaml
- uses: actions/checkout@v4
- uses: phurley/narrator-validator@v0
```

It builds the pinned validator revision and emits native GitHub annotations.

## Facts and deductions

Validator 0.7 supports the format-2 notebook and action-effect model. Format 2
removes clues and the standalone `facts.yml` section. Facts are nested beneath
the character, entity, setting, event, or trigger they belong to. Each owner
may omit `facts`, use an empty list, or contain any number of fact objects.
Every fact has a stable `fact.*` ID and a non-empty `statement`; optional
`about` and `sources` fields retain additional typed authoring context.

A fact with no `requires` field becomes available on the opening player turn.
Otherwise, `requires` is one ID or a non-empty list of IDs, all of which must be
present:

```yaml
entities:
  - id: entity.generator_controller
    name: Generator controller
    facts:
      - id: fact.manual_cutoff
        statement: The generator was cut off manually.
        requires: [command.examine, entity.generator_controller]

      - id: fact.blackout_was_staged
        statement: Someone staged the blackout.
        requires: fact.manual_cutoff
```

A `fact.*` requirement means that fact has been learned, not merely made
available. The engine evaluates requirements when a player turn begins and
after an action resolves; the resolved command, arguments, and authored effects
participate in that check.

## Entity placement, visibility, and portability

Entity state keeps four independent concepts separate:

- An **active** entity is the current entity that exists in runtime state rather
  than a form replaced by a transformation.
- A **contained** entity has a current container. Author its starting placement
  with `initial.container`; a setting, character, or another entity may be the
  container, so containers can nest. Containment cycles are invalid.
- A **visible** entity has every optional `visibility.requires` gate satisfied.
  Omission means no additional visibility gate. One requirement ID or a
  non-empty unique list is accepted; a list means all requirements must hold.
- A **portable** entity has `physical.portable: true`. Omitting `physical`,
  omitting `portable`, or explicitly setting it to false means non-portable.

Visibility gates must be evaluable outside a turn: known facts or deductions,
satisfied flags or triggers, the player's setting, or an entity the player
owns. Command, character, event, route, clue, and testimony IDs are not
persistent visibility requirements.

```yaml
entities:
  - id: entity.display_box
    name: Locked display box
    initial:
      container: setting.study

  - id: entity.service_pistol
    name: Service pistol
    initial:
      container: entity.display_box
    physical:
      portable: true
    visibility:
      requires: flag.display_box_open
```

Containment, visibility, and portability do not imply one another. In
particular, an entity can be visible but non-portable, portable but hidden, or
nested in another entity. Actions determine what players can do, so the entity
contract intentionally has no `searchable`, `investigatable`, `takeable`, or
other verb-shaped booleans.

## Actions and effects

Actions remain in the `commands` section. Each action has an ID, name, optional
description, zero or more typed parameters, and zero or more effects. The old
`aliases` field is invalid, and each parameter's singular `type` replaces the
old `accepts` list:

```yaml
commands:
  - id: command.enter
    name: Enter
    description: Enter a selected room with a companion.
    parameters:
      - name: destination
        type: setting
        required: true
      - name: companion
        type: character
        required: false
    effects:
      - operation: move
        subjects: [player, param2]
        setting: param1
```

Parameter types are `character`, `entity`, `setting`, `deduction`, and `event`.
Within an effect, `param1`, `param2`, and so on refer to parameters by their
one-based position. A parameter reference is valid only where its declared
type matches the operand. Authored IDs are also checked for existence and kind.

Supported effects have these shapes:

```yaml
effects:
  - operation: advance_time
    minutes: 15
  - operation: move
    subjects: [player, character.guide, entity.lantern, param1]
    setting: setting.boathouse
  - operation: transform
    entity_from: entity.sealed_letter
    entity_to: entity.open_letter
  - operation: learn_fact
    fact_id: fact.letter_contents
  - operation: establish_deduction
    deduction_id: deduction.blackmail
  - operation: describe
    text: Thunder rolls across the island.
  - operation: trigger
    trigger_id: trigger.lockdown
  - operation: win
    text: The mystery is solved.
  - operation: lose
    text: The culprit escapes.
```

`move.subjects` accepts `player`, character/entity IDs, and compatible
parameters. Facts, triggers, narrative text, and durations are authored effect
values because they are not action parameter types. Trigger-file effects
continue to use their existing, separate `operation`/`target`/`value` contract.
Runtime durations such as `advance_time.minutes` and route travel times must be
positive whole minutes.
The former `add_tag` and `remove_tag` action effects are invalid.

## Flags and trigger gates

Authored boolean world state lives in the required `flags` section in
`flags.yaml`. Every flag has a globally unique `flag.*` ID, a player-facing name
and description, and an explicit initial state:

```yaml
flags:
  - id: flag.storm_started
    name: Storm started
    description: The storm has reached the island.
    initial_state: false
```

Triggers may restrict when and where they apply and which world subjects make
them eligible. Omitting `time` means anytime. Omitting `location`, using an
empty string, or using a blank YAML value means all locations. `any_of` and
`all_of` accept character, entity, and flag IDs:

```yaml
triggers:
  - id: trigger.boathouse_warning
    name: Boathouse warning
    description: Warn players once the storm reaches the occupied boathouse.
    command: command.enter
    once: true
    time:
      relation: after
      value: "21:00"
    location: setting.boathouse
    any_of: [character.guide, entity.weather_radio]
    all_of: [flag.storm_started]
    effects:
      - operation: give
        target: flag.boathouse_warning_heard
```

`time.relation` is `before`, `at`, or `after`, and `time.value` is a quoted
24-hour `HH:MM` value. The legacy free-form `conditions` list is invalid.
Trigger command, description, `once`, effects, and nested facts retain their
existing contracts.

Executable format-2 trigger effects are `move`, `advance_time_by_route`,
`claim`, `give`, `give_after`, `remove`, and `satisfy_requirement`. Their
`target` and `value` operands are checked against the referenced command's
named parameter types; `$actor` and `$fact` are available only to the
operations that define them. Unknown operations, extra fields, missing values,
and wrong-kind authored IDs are errors rather than runtime surprises.

Delayed work uses flags. `give`, `give_after`, and `remove` target authored
flags; `give_after` also needs a positive minute, hour, or turn delay:

```yaml
- operation: give_after
  target: flag.knife_forensics_complete
  value: 20m
```

The resulting fact can require `flag.knife_forensics_complete`. Format 2 rejects
clue sections, top-level fact sections, facts nested beneath unsupported owner
types, `initially_known`, clue-based deduction `supported_by`, and the legacy
`learn`/`discover` effects. Fact requirement cycles are also rejected.

Format 2 facts may include optional player-safe `narrative_detail`. When
present, it must be a non-empty string and inherits the fact's requirements;
it has no separate visibility gate:

```yaml
facts:
  - id: fact.knife_was_recently_washed
    statement: The diving knife was recently washed.
    narrative_detail: Fresh scratches mark the blade beneath its polished surface.
    requires: [command.examine, entity.diving_knife]
```

Facts may also carry an explicit occurrence time when the fact statement itself
asserts that the described occurrence happened then:

```yaml
facts:
  - id: fact.rowan_died_at_2118
    statement: Rowan's watch recorded his final heartbeat at 21:18.
    occurred_at:
      day: 0
      time: "21:18"
```

`occurred_at` is optional. When present, it is an exact mapping containing only
required `day` and `time` keys. `day` is a non-negative integer within the
runtime's signed 32-bit day range. `time` is an exact quoted `HH:MM` value from
`"00:00"` through `"23:59"`; whitespace and missing zero padding are rejected.
This metadata records the occurrence asserted by the fact, not when evidence
was discovered. The validator never infers chronology from the fact statement
or from `about`, which remains relationship metadata.

Characters may define explicitly player-safe portrayal and ordered testimony:

```yaml
characters:
  - id: character.mara_voss
    portrayal:
      demeanor: Controlled and professionally helpful.
      speech_style: Precise, restrained sentences.
    testimony:
      - id: testimony.mara_generator_alibi
        text: Mara says she was in the generator shed from 21:10 onward.
        requires: [command.question, character.mara_voss, event.blackout]
        reveals: [fact.mara_claimed_generator_alibi]
```

`portrayal` may be omitted. When present, it must be a non-empty mapping with
only `demeanor` and/or `speech_style`, and each present value must be a
non-empty string. Empty mappings and unknown fields are rejected so a declared
player-safe boundary cannot silently contain private or negative-list data.

`testimony` may be omitted or be an empty sequence with the same meaning. Each
entry must be a mapping with a globally unique `testimony.*` ID, non-empty
`text`, and a non-empty sequence of unique `requires` IDs. Requirements must
include both `command.question` and the owning character ID; additional real
fact, entity, event, setting, flag, deduction, route, or trigger gates may
follow. No other command ID may appear: a turn executes one command, so a
testimony gated by both `command.question` and another command could never be
selected. `reveals` may be omitted or be an empty sequence with the same
meaning. Its entries must be unique existing fact IDs. Entry fields other than
`id`, `text`, `requires`, and `reveals` are rejected.

When at least one testimony entry is authored, `command.question` must first
declare a parameter with exact `name: character`, `type: character`, and
`required: true`. That first parameter is the testimony owner target used by
the runtime. Any later parameters must be optional topics
named `topic_character`, `topic_entity`, `topic_setting`, `topic_event`, or
`topic_deduction`, with the matching parameter type. This makes owner binding
and testimony selection deterministic rather than dependent on an ambiguous
command shape.

The validator proves the structure, reference kinds, uniqueness, and explicit
question/target gates. It cannot prove that natural-language testimony is
semantically consistent with the statements of its revealed facts; authors and
story review remain responsible for that consistency. Legacy goals, motives,
secrets, methods, cover stories, and earlier behavior notes belong beneath
`private` and are never converted into player-safe portrayal or testimony.

Format 1 remains supported for existing repositories, including its required
`clues` section and optional 0.3 fact extensions.

Format 2 cases require a quoted `case.initial_time` in 24-hour `HH:MM` form.
This initializes the authoritative shared clock used by time gates, route
travel, and delayed effects:

```yaml
case:
  id: case.last_tide
  format_version: 2
  initial_time: "21:32"
```

Gameplay deductions may define a player-facing `conclusion`, one to three
fact/deduction `inputs`, hidden boolean `truth`, `contradicted_by` references,
and an optional structured `solves` answer. Deduction cycle detection follows
both `requires` and deduction-valued `inputs`.

## Initial rules

The first pass checks:

- YAML syntax, document count, aliases, and bounded complexity;
- required top-level story sections;
- globally unique, kind-prefixed IDs;
- known typed and untyped references;
- duplicate values in reference lists;
- required flag metadata and typed trigger time/location/subject gates;
- deterministic format-2 initial time and executable trigger-effect shapes;
- player-safe character portrayal and testimony shape, identity, gate, and
  reveal references;
- versioned clue or nested-fact knowledge models and two- or three-input deductions;
- setting-parent, entity-containment, fact/clue, and deduction cycles;
- route endpoints, travel duration, reachability, and exitability;
- explicit entry/exit settings when supplied, with a compatibility fallback
  that requires all navigable settings to be strongly connected;
- solution reference types and basic event time/duration values.

Repositories without an explicit version or navigation contract remain
accepted with migration warnings. Compatibility mode treats a setting with
`type: island` as non-navigable. New repositories should add:

```yaml
case:
  format_version: 2
  entry_settings:
    - setting.main_lodge
  exit_settings:
    - setting.main_lodge
```

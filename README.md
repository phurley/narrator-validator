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

- `case.yaml`: `case` and `solution`
- `settings.yaml`: `settings` and `routes`
- `characters.yaml`, `entities.yaml`, `events.yaml`, `deductions.yaml`,
  `flags.yaml`, `commands.yaml`, and `triggers.yaml`: the matching section
- `end_states.yaml`: ordered Format 3.4 `end_states` terminal outcomes
- `deck.yaml`: the physical `cards` bindings for one printed deck edition
- `clues.yaml`: legacy format-1 `clues`

Other top-level metadata may remain in additional YAML files. The former
`tags` section is removed; use flags for authored world state.

Physical cards are separate from semantic story objects. `deck.yaml` contains
one `cards` sequence whose entries bind a numeric `tag_id` to a canonical
setting, character, entity, or command `subject`:

```yaml
cards:
  - tag_id: 13
    subject: entity.diving_knife
```

IDs range from 0 through 2114 and must be unique within the deck. Subjects may
also be bound only once. Story objects do not require a card, and an alternate
printed edition can replace only `deck.yaml` while reusing every semantic story
file. Standard command cards bind their existing `command.*` definitions; the
deck never copies command behavior.

Commands and triggers share one ordered world-effect contract. Canonical
operations are `set_flag`, `move`, `transform`, `reveal`, `conceal`,
`learn_fact`, `establish_deduction`, `describe`, `advance_time`, `win`, and
`lose`. References use authored IDs, `player`, positional `paramN` bindings, or
the matched `route`; delayed flag assignment uses `set_flag.after` rather than
a second operation vocabulary.

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
- uses: phurley/narrator-validator@v1.7.0
```

It builds the pinned validator revision and, by default, emits native GitHub
annotations. Inputs:

- `path` (default `.`) — repository-relative path containing the story.
- `format` (default `github`) — `text`, `json`, or `github`, passed straight
  to the validator.
- `report-path` (default unset) — if set, the report is also written to this
  runner-relative path (in addition to stdout), so a later step can
  post-process it, e.g. a JSON report for a story-specific coverage check.

## Story format versions

Every story declares a quoted semantic version at `case.format_version`.
This validator authors format `3.4.0`. Format 3 minor and patch releases remain
structurally compatible within the major format, while capabilities added by a
minor release must be explicitly negotiated through `case.features`. The
format-1 validation path remains for legacy repositories, while format-2
repositories stop with focused migration
guidance before the strict format-3 schema runs.

Validator `1.7.0` adds append-only standard mystery ruleset `5.0.0`, allowing
the full Solve end state to require persistent world state and adding explicit
multiplayer notebook reconciliation. Validator `1.6.0` makes automatic deduction closure the default playability
policy, reports all four fact/deduction policy combinations, and adds
automatic-notebook Case Health findings plus the append-only standard mystery
ruleset `4.0.0`. Validator `1.5.0` introduced the separate bounded playability
report. Older supported formats remain valid. See
[Story Format 3.4](docs/story-format-3.4.md). Validator `1.1.0` remains the
coordinated release for the format-3.1 contract. See
[MIGRATION.md](MIGRATION.md) for the complete format-2 migration and the
[`v1.1.0` release](https://github.com/phurley/narrator-validator/releases/tag/v1.1.0)
for the exact consumer commit matrix.

The normative placement, presence, candidate-selection, compatibility, and
privacy decisions are recorded in
[Story Format 3.1](docs/story-format-3.1.md),
[Story Format 3.2](docs/story-format-3.2.md),
[Story Format 3.3](docs/story-format-3.3.md),
[Story Format 3.4](docs/story-format-3.4.md), and
[ADR 0001](docs/adr/0001-story-format-3.1-character-presence-and-command-candidates.md).
The [ADR index](docs/adr/README.md) is the discovery point for architecture
decisions shared by validator consumers.

Legacy integer versions, missing versions, and versions outside the supported
range stop validation with migration guidance before version-specific schema
rules run. This prevents an older story from producing a misleading cascade of
unrelated field errors.

## Format 3.2 reference-aware text

Format 3.2 stories may opt into stable prose references with
`case.features: [reference_text_v1]`. `[[character.echo]]` uses a per-kind
display default; `[[character.echo.portrayal.demeanor]]` selects an explicitly
allowed named narrative path. Resolution is recursive, disclosure-aware, and
retains ordered provenance. See the [Format 3.2 contract](docs/story-format-3.2.md)
for feature negotiation, grammar, escaping, the authoritative path matrix, and
consumer APIs.

## Format 3.3 authored Solve questions

Format 3.3 pairs `solution.questions` with
`ruleset.standard_mystery@3.0.0`. A story asks one to four authored questions;
each private expected answer contains one to five unique physical setting,
character, or entity cards. Unordered rows require exact set equality and
`ordered: true` rows require exact sequence equality. The solution points to
one terminal state for terminal name/text while other generic endings remain
available. Ruleset 5 may additionally require persistent world state before
that full Solve result is eligible. See the [Format 3.3 contract](docs/story-format-3.3.md) for
the schema, scanner bounds, migration, disclosure, and shared comparison APIs.

## Format 3.4 ordered end states

Format 3.4 generalizes win states into one authored-precedence sequence of
named full wins, partial wins, and failures. Conditions combine persistent IDs,
an optional score gate, and an optional game-clock threshold. The first
satisfied state after a resolved turn is terminal, and its final score is the
current score snapshot. See the [Format 3.4 contract](docs/story-format-3.4.md)
for legal outcome/tier pairs, deterministic shadowing diagnostics, and the
behavior-preserving `win_states` transition.

## Format 3 document and disclosure contract

Format 3 makes document placement, field ownership, and disclosure boundaries
explicit. `case.yaml` owns the case metadata and mystery answer; `settings.yaml`
owns only the world settings and routes. A case declares typed player limits:

```yaml
case:
  id: case.last_tide
  format_version: "3.1.0"
  ruleset:
    id: ruleset.standard_mystery
    version: "2.0.0"
  players:
    min: 2
    max: 6
```

Every character, entity, and setting has one baseline player-safe
`description`. Discoveries that are not safe at first sight belong in a gated
fact's `narrative_detail`; duplicate `examined` and `forensic` prose is not part
of the contract. A container setting uses `navigable: false` explicitly. Its
descriptive `type` (for example `island`) remains independent of navigation.

| Boundary | Namespace or field | Named consumer |
| --- | --- | --- |
| Player-safe baseline | `description`, `portrayal`, ordered `testimony` | runtime safe-story and narration projections; author cards |
| Gated player-safe detail | nested fact `statement` and `narrative_detail` | notebook and fact-aware narration projections |
| Private narrator guidance | `narrator_guidance` | private authoring/narrator source projection; never safe-story output |
| Mechanical state | IDs, tags, placement, routes, gates, effects, times, truth, and solution references | validator and deterministic game engine |
| Author-only material | `author_notes` | raw authoring projection only; never runtime or narration input |

`solution.narrator_guidance` accepts `motive`, `method`, and `proof_summary`.
Character `narrator_guidance` accepts `goal`, `secret`, `motive`, `method`,
`cover_story`, and `testimony_guidance`. The last name deliberately cannot be
confused with player-safe ordered `testimony`. `author_notes` is the explicit
open namespace for research, demographics, drafting reminders, and other
material with no runtime projection.

## Versioned mystery ruleset

`case.ruleset` selects an exact immutable command catalog.
`ruleset.standard_mystery@1.0.0`, `@2.0.0`, `@3.0.0`, `@4.0.0`, and `@5.0.0`
supply Move, Open, Search, Examine, Take, Drop, Use, Question, Deduce, and Solve
with canonical ordered semantic parameter groups. Version 2 adds explicit
candidate sources and portability filters; version 3 keeps that catalog but
makes Solve parameterless so Format 3.3 can supply authored card-set questions.
Version 4 adds the parameterless Claim command while retaining Deduce and
question-based Solve, allowing one validated story to run under automatic or
manual notebook policies. Earlier versions remain immutable. Version 5 adds
parameterless Reconcile so joined players can deliberately pool claimed facts,
and permits persistent `requires` on the Solve-selected canonical end state.

Each resolved ruleset exports `command_capabilities`. The stable
`claim_fact`/`manual_facts`, `establish_deduction`/`manual_deductions`,
`reconcile_notebooks`/`multiple_players_with_unshared_facts`, and
`submit_solution`/`always` identities let runtimes filter commands by game
policy without copying command definitions. Versions 1 through 3 omit Claim
and therefore report fully manual fact analysis as inconclusive.

Ruleset commands participate in global ID and reference validation, including
physical bindings in `deck.yaml`, without being copied into `commands.yaml`.
That file is optional and contains only story-specific extensions such as
`command.investigate`. Extension IDs must be distinct. Overrides are explicitly
deferred in version 1; redefining a ruleset command produces
`ruleset.command_conflict` instead of merging arbitrary YAML fields.

Released ruleset versions are append-only. A new command contract requires a
new ruleset version, so an immutable story snapshot continues resolving the
same catalog after reload. Unknown IDs and incompatible versions fail before
game creation with the supported ID and version in the diagnostic.

Known item mappings reject unknown fields with an exact JSON pointer. This
includes cases, solutions, settings, routes, characters, entities, events,
facts, deductions, flags, commands, triggers, and testimony entries. These
compact examples show every item kind and its canonical fields:

```yaml
# case.yaml
case: { id: case.example, format_version: "3.1.0", players: { min: 1, max: 4 }, initial_time: "20:00", entry_settings: [setting.study], exit_settings: [setting.study] }
solution: { culprit: character.suspect, weapon: entity.knife, location: setting.study, time: "20:15", deduction: deduction.solution }

# settings.yaml
settings:
  - { id: setting.world, type: island, navigable: false, name: The island, description: A storm-bound island. }
  - { id: setting.study, type: room, name: Study, description: A book-lined study., parent: setting.world }
routes: []

# characters.yaml
characters:
  - id: character.suspect
    name: Alex Vale
    description: A composed guest in a rain-dark coat.
    initial: { location: setting.study }
    presence: { requires: flag.power_out }
    narrator_guidance: { goal: Keep the missing hour private. }
    testimony: [{ id: testimony.alibi, text: Alex says they remained in the lounge., requires: [command.question, character.suspect], reveals: [fact.alibi] }]
    facts: [{ id: fact.alibi, statement: Alex claimed to remain in the lounge. }]

# entities.yaml / events.yaml / deductions.yaml / flags.yaml
entities: [{ id: entity.knife, type: object, name: Knife, description: A polished display knife., initial: { container: setting.study } }]
events: [{ id: event.murder, day: 0, time: "20:15", duration_minutes: 0, location: setting.study, participants: [character.suspect] }]
deductions: [{ id: deduction.solution, conclusion: Alex used the knife., inputs: [fact.alibi], truth: true }]
flags: [{ id: flag.power_out, name: Power out, description: The power has failed., initial_state: false }]

# optional commands.yaml / triggers.yaml / deck.yaml
commands: [{ id: command.investigate, name: Investigate, parameters: [{ name: target, types: [entity, setting], min: 1, max: 1 }] }]
triggers: [{ id: trigger.blackout, name: Blackout, on: { command: command.question, parameters: { character: character.suspect } }, effects: [{ operation: set_flag, flag: flag.power_out, value: true }] }]
cards: [{ tag_id: 0, subject: setting.study }, { tag_id: 1, subject: character.suspect }, { tag_id: 2, subject: entity.knife }]
```

## Format 3 character placement, presence, and command candidates

Format 3.1 keeps narrative kind, player knowledge, authoritative world
location, and player-scoped physical presence separate. A character may declare
`initial.location` as a setting and may gate local availability with persistent
`presence.requires`. Remote or gated characters stay absent from local options
and narration without hiding their public identity or leaking their location or
requirements.

Command parameters may declare ordered candidate sources from `all`,
`current_location`, `inventory`, `reachable`, `known`, and `established`, then
apply the closed `portable` capability filter. Source results are unioned and
deduplicated deterministically. The backend uses the same revision-bound
resolver for options and reducer authorization.

See the [Story Format 3.1 reference](docs/story-format-3.1.md) and
[accepted architecture decision](docs/adr/0001-story-format-3.1-character-presence-and-command-candidates.md)
for the normative semantics and privacy invariants.

## Facts and deductions

Format 3 retains the nested notebook and action-effect model introduced by
format 2. It
removes clues and the standalone `facts.yml` section. Facts are nested beneath
the character, entity, setting, event, or trigger they belong to. Each owner
may omit `facts`, use an empty list, or contain any number of fact objects.
Every fact has a stable `fact.*` ID and a non-empty `statement`; optional
`about` and `sources` fields retain additional typed authoring context.

A fact's nesting owner is its discovery source. `about` remains relationship
metadata and never changes discovery. An ordinary fact with neither `on` nor
`when` becomes available on the opening player turn. A fact referenced by
testimony `reveals` is learned only from that testimony, and a fact nested under
a trigger is learned when that trigger completes (or, when the trigger has
`after`, when its delayed result completes).

`on` matches the current action by command and semantic parameter name. Within
an owner-nested fact, `owner` means the enclosing character, entity, setting, or
event, so the source ID need not be repeated. `when.all` contains only durable
state predicates:

```yaml
entities:
  - id: entity.generator_controller
    name: Generator controller
    facts:
      - id: fact.manual_cutoff
        statement: The generator was cut off manually.
        on:
          command: command.examine
          parameters:
            target: owner

      - id: fact.blackout_was_staged
        statement: Someone staged the blackout.
        when:
          all:
            - knows: fact.manual_cutoff
```

Persistent predicates are explicit mappings: `at` takes a setting, `owns` an
entity, `knows` a fact or deduction, `flag` a flag, `completed` a trigger, and
`time` a `relation`/`value` mapping. The engine evaluates them when a player
joins and after actions or delayed work resolve. `on` and `when` may be combined;
both must match. Format 3 rejects the former fact `requires` ID bag.

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

## Character placement and physical presence

Format 3.1 can place a character at one authoritative setting and gate their
physical presence with durable, player-relative requirements:

```yaml
characters:
  - id: character.echo
    name: Echo
    description: A publicly known missing detection dog.
    initial:
      location: setting.hidden_run
    presence:
      requires: flag.echo_discovered
```

`initial.location` accepts only a setting. A character is physically present
when their current setting matches the player's and every `presence.requires`
reference is satisfied for that player. Omitted placement means the character
is unplaced; omitted presence means no additional gate. Placement and gates do
not conceal public identity or known notebook references, and characters never
become portable inventory or nested containers.

## Actions and effects

Actions remain in the `commands` section. Each action has an ID, name, optional
description, zero or more semantic parameters, and zero or more effects. Each
parameter declares its accepted definition kinds and selection cardinality:

```yaml
commands:
  - id: command.enter
    name: Enter
    description: Enter a selected room with a companion.
    parameters:
      - name: destination
        types: [setting]
        min: 1
        max: 1
      - name: companion
        types: [character]
        min: 0
        max: 1
        candidates:
          from: [current_location]
    effects:
      - operation: move
        subjects: [player, param2]
        setting: param1
```

Parameter types are `character`, `entity`, `setting`, `deduction`, and `event`.
`types` is ordered, non-empty, and unique. Cardinality must satisfy
`0 <= min <= max` and `max >= 1`. A parameter can therefore model one semantic
role that accepts alternative kinds or multiple selected cards without merging
distinct roles. Format 3 rejects the removed singular `type`/`required` shape;
use `types`/`min`/`max` for every command parameter.
Format 3.1 parameters may add `candidates.from`, an ordered non-empty set of
`all`, `current_location`, `inventory`, `reachable`, `known`, or `established`.
Sources are unioned and deduplicated. The optional `capabilities: [portable]`
filter is valid only for entity-capable parameters. The standard mystery 2.0
catalog declares these contracts explicitly; custom commands use the same
resolver and validation rules.
Within an effect, `param1`, `param2`, and so on refer to parameters by their
one-based position. An effect parameter reference is valid only for a
single-card parameter whose accepted kinds all match the operand. Authored IDs
are also checked for existence and kind.

Supported effects have these shapes:

```yaml
effects:
  - operation: set_flag
    flag: flag.storm_started
    value: true
  - operation: advance_time
    minutes: 15
  - operation: advance_time
    route: route
  - operation: move
    subjects: [player, character.guide, entity.lantern, param1]
    setting: setting.boathouse
  - operation: transform
    entity_from: entity.sealed_letter
    entity_to: entity.open_letter
  - operation: reveal
    entity: entity.letter_contents
  - operation: conceal
    entity: entity.letter_contents
  - operation: learn_fact
    fact_id: fact.letter_contents
  - operation: establish_deduction
    deduction_id: deduction.blackmail
  - operation: describe
    text: Thunder rolls across the island.
  - operation: win
    text: The mystery is solved.
  - operation: lose
    text: The culprit escapes.
```

Commands and triggers use these same effect shapes and positional parameter
references. `move.subjects` accepts `player`, character/entity IDs, and
compatible parameters. `advance_time` requires exactly one positive whole
`minutes` value or a route reference; `route` means the route matched for the
current action. Delayed trigger work uses `set_flag.after` and can only assign
true because its completion is player-scoped.

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

Triggers use the same boundary. `on` matches only the current action.
`when.all` evaluates only durable player/world state. An optional `on.actor`
names an authored character actor explicitly; a character ID is never inferred
from a selected-card bag. Parameter bindings accept one ID or a non-empty list
and are checked against the command parameter's semantic union and maximum
cardinality:

```yaml
triggers:
  - id: trigger.boathouse_warning
    name: Boathouse warning
    description: Warn players once the storm reaches the occupied boathouse.
    on:
      command: command.enter
      parameters:
        destination: setting.boathouse
    once: true
    when:
      all:
        - at: setting.boathouse
        - flag: flag.storm_started
        - time:
            relation: after
            value: "21:00"
    effects:
      - operation: set_flag
        flag: flag.boathouse_warning_heard
        value: true
```

`time.relation` is `before`, `at`, or `after`, and `time.value` is a quoted
24-hour `HH:MM` value. Legacy top-level `command`, `parameters`, `time`,
`location`, `any_of`, `all_of`, and `conditions` are invalid in format 3.
Unknown parameters, impossible owner/actor kinds, over-cardinality bindings,
wrong-kind predicates, and triggers without an effect, nested result fact, or
referenced completion identity are precise errors.

Delay a trigger's actor-scoped nested result facts with `after`:

```yaml
triggers:
  - id: trigger.knife_forensics
    name: Knife forensics
    on:
      command: command.investigate
      parameters:
        target: entity.diving_knife
    after: 20m
    facts:
      - id: fact.knife_forensics
        statement: The knife was cleaned with marine degreaser.
```

This replaces disposable completion flags whose only purpose was to unlock one
result fact. Delayed triggers cannot also declare immediate `effects`; model a
delayed observation as nested facts, and use a separate immediate trigger when
a world effect is required. Format 3 rejects clue sections, top-level fact
sections, facts nested beneath unsupported owner types, `initially_known`, and
clue-based deduction `supported_by`. Fact and trigger-completion dependency
cycles are reported deterministically.

Format 3 facts may include optional player-safe `narrative_detail`. When
present, it must be a non-empty string and inherits the fact's requirements;
it has no separate visibility gate:

```yaml
facts:
  - id: fact.knife_was_recently_washed
    statement: The diving knife was recently washed.
    narrative_detail: Fresh scratches mark the blade beneath its polished surface.
    on:
      command: command.examine
      parameters:
        target: owner
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
    voice_id: JBFqnCBsd6RMkjVDRZzb
    portrayal:
      demeanor: Controlled and professionally helpful.
      speech_style: Precise, restrained sentences.
    testimony:
      - id: testimony.mara_generator_alibi
        text: Mara says she was in the generator shed from 21:10 onward.
        requires: [command.question, character.mara_voss, event.blackout]
        reveals: [fact.mara_claimed_generator_alibi]
```

`voice_id` may be omitted. When present, it selects the character's
ElevenLabs voice for generated dialogue and must be a 1–128 character ID made
only from ASCII letters, numbers, `-`, or `_`. It is delivery metadata and is
not exposed as player-safe story content.

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
declare a required `character` parameter accepting exactly one character. The
canonical second parameter is an optional `topic` union accepting character,
entity, setting, event, and deduction selections; its authored maximum controls
how many topics may accompany the question. Legacy individually typed optional
topic parameters remain accepted during migration. The first parameter remains
the testimony owner target, making owner binding deterministic.

`reveals` is authoritative: a revealed fact does not repeat the question owner,
topic, or command gates in its own discovery fields. The validator proves the
structure, reference kinds, uniqueness, and explicit question/target gates. It
cannot prove that natural-language testimony is
semantically consistent with the statements of its revealed facts; authors and
story review remain responsible for that consistency. Goals, motives, secrets,
methods, cover stories, and private behavioral direction belong beneath
`narrator_guidance`; unordered private accounts belong beneath
`testimony_guidance`. They are never converted into player-safe portrayal or
ordered testimony.

Format 1 remains supported for existing repositories, including its required
`clues` section and optional 0.3 fact extensions.

Format 3 cases require a quoted `case.initial_time` in 24-hour `HH:MM` form.
This initializes the authoritative shared clock used by time gates, route
travel, and delayed effects:

```yaml
case:
  id: case.last_tide
  format_version: "3.1.0"
  initial_time: "21:32"
```

Gameplay deductions are concise, authoritative player-visible notebook notes,
not narrated world events or prompts for player role-play. They define a
player-facing `conclusion` and one to three fact/deduction `inputs`; dependency
closure is acyclic and deterministic. Every deduction may be established
automatically, so `truth: false` is a contradictory contract and speculative
wording receives a review warning. A future hypothesis mechanic should own
false or provisional accusations.

Stories are validated once and must remain coherent under automatic and manual
fact/deduction policies. Story YAML never declares `auto_facts` or
`auto_deductions`; those are immutable game-instance preferences. Automatic
deduction mode repeatedly establishes every satisfied deduction to a
player-scoped fixed point. Manual mode keeps Claim and Deduce available through
ruleset 4. Solve remains a distinct final commitment graded only by
`solution.questions` and end states. See
[Automatic deductions and notebook safety](docs/automatic-deductions.md).

## Points and terminal states

The validator also reports conservative action-level reachability separately
from structural validity. See [Static playability analysis](docs/playability-analysis.md)
for the deterministic supported subset, bounds, terminal statuses, lower-bound
route/action/time evidence, and conspicuous handling of unsupported mechanics.

Settings, entities, deductions, and commands may define an authoritative point
award:

```yaml
points:
  value: 10
  max_claim_count: 1
  requires: [setting.library, entity.brass_key]
```

`value` and `max_claim_count` are positive whole numbers; the claim count
defaults to one. Requirements are optional persistent setting, entity, fact,
deduction, flag, or trigger references. The runtime evaluates them against the
post-transition player state and tracks claims per player and authored source.
Point awards on routes, characters, events, or triggers are rejected.

Format 3.4 terminal outcomes live in canonical `end_states.yaml`:

```yaml
end_states:
  - id: end.escape
    name: Escaped the house
    outcome: won
    resolution: partial
    requires: [flag.front_door_unlocked]
    minimum_points: 50
    at_or_after: "21:30"
    text: You force the front door open and reach the road.
```

End-state sequence order is semantic: after each resolved turn the runtime
selects the first satisfied state. Each state needs a stable ID, player-facing
name and completion text, legal outcome/resolution pair, and at least one
persistent requirement, positive point threshold, clock threshold, or authored
Solve condition. `minimum_points` defaults to zero and only gates selection;
the selected state's final score is the current score snapshot.

A story may omit the murder-specific `solution` block when it defines at least
one generic end state. Legacy `win_states` remain readable as ordered
`won`/`full` states; migrate by preserving their IDs, order, conditions, names,
and text while adding the explicit outcome and resolution fields. See the
[Format 3.4 contract](docs/story-format-3.4.md) for precedence diagnostics and
the coordinated transition.

## Initial rules

The first pass checks:

- YAML syntax, document count, aliases, and bounded complexity;
- required top-level story sections;
- globally unique, kind-prefixed IDs;
- known typed and untyped references;
- duplicate values in reference lists;
- required flag metadata and typed trigger action/persistent-condition gates;
- deterministic format-3 initial time, trigger delay, and executable effect shapes;
- player-safe character portrayal and testimony shape, identity, gate, and
  reveal references;
- versioned clue or nested-fact knowledge models and two- or three-input deductions;
- point-award owners, positive values, claim limits, and persistent requirements;
- canonical ordered win states, thresholds, terminal configuration, and references;
- setting-parent, entity-containment, fact/clue, and deduction cycles;
- route endpoints, travel duration, reachability, and exitability;
- explicit entry/exit settings when supplied, with a compatibility fallback
  that requires all navigable settings to be strongly connected;
- solution reference types and basic event time/duration values.

Format 3 repositories require an explicit navigation contract and a semantic
format version. Container-only settings are explicitly non-navigable; their
descriptive category does not change navigation behavior:

```yaml
case:
  format_version: "3.1.0"
  entry_settings:
    - setting.main_lodge
  exit_settings:
    - setting.main_lodge

settings:
  - id: setting.larkspur_cay
    type: island
    navigable: false
    description: A storm-bound coral island.
```

## Discovery migration examples

- Lena testimony keeps its ordered question/topic gates on the testimony entry;
  its fact is listed only in `reveals` and has no duplicate discovery gate.
- Mara access-log facts use `on.command: command.examine`, bind the semantic
  target parameter to `owner`, and put durable controller visibility under
  `when.all: [{ flag: flag.controller_contents_revealed }]`.
- Tunnel unlock triggers bind `on.parameters.item` to the staff key and the
  target role to the tunnel entrance. Characters are not placed in an
  alternative selected-ID bag when `command.use` cannot select them.
- Delayed forensic triggers move their delay to trigger `after` and nest the
  result fact under that trigger; the one-use completion flag and fact
  requirement are removed.

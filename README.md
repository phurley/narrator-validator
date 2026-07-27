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
  `tags.yaml`, `commands.yaml`, and `triggers.yaml`: the matching section
- `clues.yaml`: legacy format-1 `clues`

Other top-level metadata may remain in additional YAML files.

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

Validator 0.5 supports the format-2 notebook and claim model. Format 2 removes
clues and the standalone `facts.yml` section. Facts are nested beneath the
character, entity, setting, event, or trigger they belong to. Each owner may
omit `facts`, use an empty list, or contain any number of fact objects. Every
fact has a stable `fact.*` ID and a non-empty `statement`; optional `about` and
`sources` fields retain additional typed authoring context.

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

A `fact.*` requirement means that fact has been claimed, not merely made
available. Format 2 therefore requires `command.claim` with a parameter that
accepts fact IDs. The engine evaluates requirements when a player turn begins
and after an action resolves; the resolved command and arguments participate
in that check.

Delayed work uses state tags. A state tag needs no static `members`, and
`give_after` must target a tag with a positive minute, hour, or turn delay:

```yaml
- operation: give_after
  target: tag.knife_forensics_complete
  value: 20m
```

The resulting fact can require `tag.knife_forensics_complete`. Format 2 rejects
clue sections, top-level fact sections, facts nested beneath unsupported owner
types, `initially_known`, clue-based deduction `supported_by`, and the legacy
`learn`/`discover` effects. Fact requirement cycles are also rejected.

Format 1 remains supported for existing repositories, including its required
`clues` section and optional 0.3 fact extensions.

Gameplay deductions may define a player-facing `conclusion`, two or three
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

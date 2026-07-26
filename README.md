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

## Initial rules

The first pass checks:

- YAML syntax, document count, aliases, and bounded complexity;
- required top-level story sections;
- globally unique, kind-prefixed IDs;
- known typed and untyped references;
- duplicate values in reference lists;
- setting-parent, entity-containment, clue, and deduction cycles;
- route endpoints, travel duration, reachability, and exitability;
- explicit entry/exit settings when supplied, with a compatibility fallback
  that requires all navigable settings to be strongly connected;
- solution reference types and basic event time/duration values.

The current format has no explicit version or navigation contract. The
validator accepts it, emits migration warnings, and treats a setting with
`type: island` as non-navigable. New repositories should add:

```yaml
case:
  format_version: 1
  entry_settings:
    - setting.main_lodge
  exit_settings:
    - setting.main_lodge
```

# Story Format 3.12: deck layer field ownership

Format 3.12 documents the deck layer introduced by
[ADR-022](https://github.com/phurley/narrator-system/blob/fb02269/ADR-022-deck-defaults-and-story-overrides.md):
a deck stores its own working-copy file set using the same canonical section
files as a story, and the effective file set a game, simulation, story test or
validation run consumes is the deck layer overlaid by the story layer. A 3.12
story with no deck layer (an empty deck, or `merge_layers(&[], &story)`)
behaves identically to a 3.11 story, except for the field-ownership rules
below: those apply unconditionally, because they name which layer a field may
ever come from, not what a deck contributes.

`merge_layers` is a pure function over two `SourceFile` lists shared by the
CLI, the backend and the WASM package. It does not validate content; call
`validate` on its output.

## Merge rules by section

- **Id-listed sections** (`settings`, `routes`, `characters`, `entities`,
  `events`, `flags`, `commands`, `triggers`, `deductions`, and the nested
  `facts`/`testimony` lists inside a character or entity, and `case.map`'s
  `variants`) merge by id. A story entry whose id matches a deck entry
  **replaces it whole**. Deck entries the story does not mention are
  inherited. Story entries with new ids are appended after the (deck-ordered)
  inherited/replaced entries.
- **Tombstones.** A story entry of the exact shape `{ id: <deck id>, remove:
  true }` and no other keys drops the matching deck entry instead of
  replacing it. `remove: true` is a reserved key on every id-listed entry from
  Format 3.12.
- **Map-keyed sections** (`command_costs`, a subject's `command_overrides`,
  `reference-literals.yaml`'s top-level keys) merge per key; the story's key
  wins. There is no tombstone for a map key.
- **`case` and `wait`** merge per top-level key; a key present in the story
  wins, and nested objects are replaced whole except `case.map`, whose
  `variants` merge by id and whose `preamble` follows the per-key rule.
- **`maps/<name>.svg`** in the story replaces the deck's file of the same
  name; other deck maps are inherited.
- Every other file (`deck.yaml`, and any file this module does not know as a
  canonical section) is a whole-file passthrough: the story's copy wins when
  both layers provide the same path.

## Deck-only and story-only fields

Per ADR-022 §3, some fields belong to exactly one layer:

**Deck-only** — a story `case.yaml` that sets one fails with
`layer.deck_only_field`:

- `case.format_version`
- `case.ruleset`

**Story-only** — a deck file that sets one fails with
`layer.story_only_field`:

- `case.id`, `case.premise`, `case.opening`
- `solution` (the `case.yaml` root key)
- `end_states.yaml`, `win_states.yaml` (whole files)
- any story test file under `scripts/` (ADR-021 story tests assert a story's
  end state, so they are story-only)

Everything else may be set by either layer, with the story winning on a
shared key — including `case.title`, `case.genre`, `case.tone`,
`case.features`, `case.entry_settings`, `case.exit_settings`,
`case.initial_time`, `case.players`, `case.map`, and every id-listed and
map-keyed section above.

These checks run against the raw deck and story inputs, not the merge output,
so a violation is reported even when the other layer omits the file entirely:
a story `case.yaml` that still sets `ruleset` fails merging against an empty
deck, not just a populated one. A bare story of this shape is therefore only
a valid *import candidate*, not a mergeable story layer, until whatever
imports it strips the deck-only fields onto the deck.

Both diagnostics are errors, but neither strips the offending value from the
merge: the merge still proceeds and story still wins on shared keys, so a
merged file that carries a layer-ownership violation still validates
meaningfully once the violation itself is fixed.

## Diagnostics

- `layer.deck_only_field` — a story file sets a field the deck layer owns.
- `layer.story_only_field` — a deck file sets a field or provides a whole
  file the story layer owns.
- `layer.tombstone_in_effective_set` — `validate` found a section entry of
  the exact tombstone shape `{ id, remove: true }`. Tombstones are consumed
  by `merge_layers`; one reaching `validate` means the merge step was
  skipped rather than that the content is actually invalid.
- `layer.remove_unknown_id` (unchanged from the merge's introduction) — a
  story tombstone names an id that is not present in the deck layer.
- `layer.invalid_yaml` (unchanged) — a canonical section file could not be
  parsed while merging.

## No other behaviour changes

Setting `case.format_version: "3.12.0"` on a story with no deck layer changes
nothing else: every Format 3.11 rule, including clock triggers and the
playability model, carries over unchanged.

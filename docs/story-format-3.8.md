# Story Format 3.8: `case.map`

Story Format 3.8 lets a story ship a floor plan. `case` gains an optional
`map` with a narratable `preamble` and an ordered list of `variants`, each
binding a committed SVG to a conjunctive `requires` condition, so the plan a
player sees can change when the story's world state does — a discovered
service stair appears on the plan.

```yaml
case:
  id: case.briar_house
  format_version: "3.8.0"
  map:
    preamble: >
      Briar House has three rooms off the parlor, and
      [[setting.study.description]]
    variants:
      - id: map.with_stair
        source: maps/briar-house-stair.svg
        requires: [flag.stair_found]
        preamble: The service stair now runs from the cellar to the study.
      - id: map.default
        source: maps/briar-house.svg
```

## A map is an illustration, not the route graph

"Map" already means two things in this project, and only one of them is
this one. The traversal graph is `settings.yaml`'s `routes`; that is what
decides where a player can go and what the bounded playability search
walks. `case.map` is **presentational only**. It never gates playability or
reachability, it is never an input to the deduction graph, and adding one to
a story cannot change what the search proves about it. The validator
enforces this by construction: map SVGs are excluded from the model the
playability analysis builds, and `case.map` contributes no nodes to it.

The player-facing name stays "map" because that is what a player calls the
thing on the screen.

## `case.map`

`map` is an optional mapping nested in `case.yaml` under `case`; there is no
new top-level section or file. It requires `format_version >= 3.8.0`.
Authoring it under an earlier format produces a versioned-feature diagnostic
naming 3.8.0 (`case.map_format_incompatible`) rather than a generic
unknown-field error, the same treatment Format 3.6 gave `case.players`.

Its fields:

- `preamble` (optional) — player-facing prose shown with the map the first
  time it is presented. A non-empty string (`case.map_preamble` otherwise).
- `variants` (required) — a non-empty, ordered sequence
  (`case.map_variants` otherwise).

Any other key is `case.map_unknown_field`.

## `case.map.variants[]`

Each entry is a mapping (`case.map_variant_type` otherwise) with:

- `id` (required) — a `map.<snake_case>` ID, unique within the list
  (`case.map_variant_id`, `case.map_variant_id_duplicate`). Unlike the other
  authored namespaces, `map.` IDs are local to `case.map`: a variant is a
  presentation choice, not a subject anything else may reference.
- `source` (required) — a repository-relative `maps/<name>.svg` path
  (`case.map_variant_source`) naming a file committed in the same snapshot
  (`case.map_variant_source_missing`).
- `requires` (optional) — a sequence of persistent requirement IDs
  (`case.map_variant_requires_type`). Absent or `[]` means unconditional.
- `preamble` (optional) — prose shown when this variant becomes the active
  one, in addition to `case.map.preamble`.

Any other key is `case.map_unknown_field`.

### Ordering and precedence

Variant semantics are identical to `end_states`: `requires` is conjunctive
over persistent IDs, authored order is the precedence contract, and the
first satisfied variant wins. The vocabulary is the same persistent
requirement set routes, entity visibility and end states already use —
settings, entities, facts, deductions, flags and triggers. A `requires`
entry naming anything else is `reference.wrong_type`; an entry naming
nothing at all is `reference.unknown`. Map variants deliberately do **not**
use the `when: {all: [...]}` predicate dialect, so no new predicate code
exists to diverge.

Because order decides, two rules follow, and the validator enforces both:

- **Exactly one variant is unconditional, and it is authored last.** A
  missing fallback (`case.map_unconditional_missing`) means some player
  reaches a state with no map at all; an unconditional variant that is not
  last (`case.map_unconditional_not_last`) makes every variant after it
  dead; a second unconditional variant (`case.map_unconditional_duplicate`)
  can never be selected.
- **A later variant may not be shadowed by an earlier one.** If an earlier
  variant's `requires` is a subset of a later one's, the earlier is
  satisfied whenever the later is, and the later is unreachable
  (`case.map_variant_shadowed`); if the two conditions are identical it is
  `case.map_variant_duplicate_condition`. Both diagnostics carry a related
  location pointing at the earlier variant, exactly as
  `end_states.unreachable_precedence` and `end_states.duplicate_precedence`
  do.

## SVG is the only map format

A map is `maps/<name>.svg` and nothing else. Every layer of the toolchain
models story files as UTF-8 text — `SourceFile { path, source }` here, the
backend's `RepositoryFile.source`, the author's `repoSchemas.ts` — and a
raster channel would have to be threaded through all of it. SVG passes
through unchanged as text, scales without blur when a player zooms, and
diffs in git. A raster map, if ever needed, is a separate epic that adds an
out-of-band URL channel; it is not a `case.map` variant.

The CLI therefore enumerates `.svg` files directly under `maps/` alongside
story YAML. Those files are **not** story sections: they never reach the
YAML schema rules, the canonical-filename check, or the playability model.
They are validated only by the `case.map` variant that references them.

### Safety checks

The player client renders whatever it is handed, so the validator is the
gate rather than the client. Every referenced map SVG must parse as XML and
is rejected if any of the following holds:

| diagnostic | rejected |
| --- | --- |
| `case.map_svg_invalid` | not well-formed XML, or declares a DTD (which is what keeps an entity-expansion bomb out) |
| `case.map_svg_root` | root element is not `<svg>` |
| `case.map_svg_view_box` | root `<svg>` has no `viewBox`, so a client cannot scale or fit it |
| `case.map_svg_forbidden_element` | contains `<script>` or `<foreignObject>` |
| `case.map_svg_event_attribute` | any attribute whose name begins `on` (matched case-insensitively rather than enumerated, so the check does not go stale as the event vocabulary grows) |
| `case.map_svg_external_reference` | an `href` or `xlink:href` that is not a same-document `#fragment` |

These are reported against the SVG's own path, with the referencing variant
named in the message.

The 256 KiB per-file cap a map must respect is **not** a map-specific check.
`MAX_FILE_BYTES` already applies it to every file in a snapshot, `maps/*.svg`
included, and reports `repository.file_too_large`; a second cap saying the
same thing is a second mechanism that would drift, so the existing one is
what enforces it.

## Disclosure boundary

Both preambles are baseline **player-safe** reference-text consumers under
`reference_text_v1`, registered in `CONSUMER_FIELDS` as `case:map.preamble`
and `map_variant:preamble`. Authored `[[...]]` expressions in them are
resolved at compile time and receive the same unknown-ID
(`reference_text.unknown_id`), disclosure, and cycle diagnostics as
`case.premise` or `case.opening`.

`case.map.preamble` is also a referenceable case path, so
`[[case.<id>.map.preamble]]` resolves, `PlayerSafe`. A variant preamble is
not addressable this way: `[[...]]` has no index syntax and variants are a
sequence.

The SVG itself carries no disclosure grading — it is authored art, and
everything in it is by definition something a player is meant to look at.
An author who does not want a room on the plan should not draw it; there is
no "gated layer" concept, and the validator has no way to enforce one.
Variant **selection**, by contrast, is private: it depends on flags, which
are never projected to a client, so the active variant is resolved
server-side per player.

## Out of scope for this document

Backend ingest of `.svg` story files, compiling `case.map`, and the
per-player active-variant resolver are `narrator-backend#647`. The
`GET /api/player/games/{id}/map` projection and prepending the map preamble
to the opening presentation are `narrator-backend#648`. The map viewer,
first-start map screen and in-game Map entry are `narrator-app#252`;
refreshing on a revision change and the "map updated" notice are
`narrator-app#253`. Editing `case.map` in the case editor and the
blank-case format bump are `narrator-author#518`. Authoring the actual
Briar House plans is `simple_mystery#80`. The epic is
`narrator-backend#643`.

Explicitly out of scope for the format itself: PNG or any raster map,
per-room highlighting of a player's position, author-side SVG upload or
drawing, voicing a variant's preamble separately from the opening beat, and
any map-driven playability or reachability.

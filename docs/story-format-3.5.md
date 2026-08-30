# Story Format 3.5: `command_costs` clock-cost overrides

Story Format 3.5 lets a story override, per `(command, target)` pair, how
many in-game clock minutes a command costs when it targets a specific
entity, setting, or character. This is the per-story half of ADR-004's
clock-cost resolution mechanism; the other half, every command's
**command default** cost, is a ruleset-side fact (`ruleset.standard_mystery
@6.0.0` adds `default_cost_minutes` to its command catalog) and requires no
format bump of its own.

```yaml
# costs.yaml
command_costs:
  - id: cost.examine_bell
    command: command.examine
    target: entity.brass_bell
    minutes: 5
```

`command_costs` requires `format_version >= 3.5.0`. Authoring a
`command_costs` list — or simply having a non-empty `costs.yaml` — under an
earlier format produces `command_costs.format_incompatible` rather than a
silent fallback to command defaults: an author who wrote an override and had
it silently discarded would get behavior different from what they authored
with no diagnostic, so an older format is rejected outright rather than
downgraded.

## Where it lives

`command_costs` is a new top-level list, authored in its own file,
`costs.yaml`, sibling to `routes.yaml` and `end_states.yaml`. There is no
inline `case.yaml` table and no per-entity map: `costs.yaml` follows the
established `authored_item!` pattern already used by `Route` and `EndState`
so a reviewer auditing "what does examining the bell cost" can read one
list rather than hunting per-command maps scattered across every entity
file.

## Fields

Each `command_costs` entry accepts exactly:

- `id` (required) — a globally unique ID, same convention as every other
  authored item (`id.invalid`, `id.wrong_prefix`, `id.duplicate` apply as
  usual).
- `command` (required) — a non-empty command ID that must resolve to a
  command the story's ruleset declares
  (`command_costs.command_missing` / `command_costs.command_unknown`
  otherwise).
  - It must not be `command.move` (`command_costs.command_move_disallowed`):
    `command.move`'s cost is determined entirely by the route taken
    (`Route.travel_minutes`), a strictly more precise mechanism than a
    `(command, target)` pair, since two different routes to the same
    setting can already cost differently. `command_costs` cannot express
    that and does not need to.
  - It must take a target parameter (`command_costs.command_parameterless`
    otherwise); a parameterless command like `command.deduce`,
    `command.solve`, or `command.reconcile` has no target to key an
    override on and can only use its command default.
- `target` (required) — a non-empty ID naming the value bound to the
  command's *first* parameter (`param1`) — for `command.question` this is
  the `character`, never a `topic`; for `command.use` this is the `item`,
  never the optional secondary `target` parameter of the effect template.
  It must resolve to an entity, setting, or character declared elsewhere in
  the story (`command_costs.target_missing` / `command_costs.target_unknown`
  otherwise), and must be of a kind the command's first parameter accepts
  (`command_costs.target_kind_mismatch` otherwise). The validator only
  checks that both halves of the pair exist and are kind-compatible — not
  that the pair is reachable or meaningful in play.
- `minutes` (required) — a non-negative whole number of minutes supported
  by the runtime (`command_costs.minutes_invalid` otherwise), the same
  `require_whole_minutes` rule already applied to `Route.travel_minutes`
  and to `advance_time` effect literals. `minutes: 0` is allowed and
  explicit — a missing cost is never silently zero, but an authored zero
  is a legitimate override.
- Any other key is rejected as `command_costs.unknown_field`.

At most one `command_costs` entry may exist per `(command, target)` pair; a
repeat is `command_costs.duplicate_pair`, pointing back at the entry that
defined the pair first.

## Resolution order (informative)

`command_costs` is one tier of a four-tier resolution table a compiled turn
uses to pick exactly one clock cost, evaluated top to bottom (full detail:
`ADR-004`):

1. `command.move` — always the route mechanism, `command_costs` never
   applies.
2. A parameterless command — always its ruleset command default.
3. A command with a target and a matching `command_costs` entry — that
   entry's `minutes`.
4. Otherwise — the command's ruleset command default.

The validator's role in this format revision is entirely static: it checks
that an authored `command_costs` entry is well-formed and its `(command,
target)` pair exists. Resolving and applying clock cost during play is
`narrator-backend`'s responsibility (`CompiledStory` exposes `command_costs`
keyed by `(command, target)` and each command's `default_cost_minutes`,
mirroring how `story.routes` is already exposed) and is out of scope for
this document.

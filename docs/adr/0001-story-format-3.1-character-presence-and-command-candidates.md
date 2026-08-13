# ADR 0001: Story Format 3.1 character presence and command candidates

- Status: Accepted
- Date: 2026-08-13
- Owners: Narrator validator, backend, authoring, and maintained-story consumers
- Proposal: [narrator-validator issue #20](https://github.com/phurley/narrator-validator/issues/20)
- Contract reference: [Story Format 3.1](../story-format-3.1.md)

## Context

Narrator must distinguish four concepts that earlier story contracts could
conflate:

1. A definition's narrative kind (`character`, `entity`, or `setting`).
2. Whether a player knows that definition exists.
3. Where a physical participant is located in authoritative world state.
4. Whether that participant is physically present to one requesting player.

A missing character can therefore remain publicly known and continue to appear
in facts, deductions, events, and portrayal while being unavailable for local
interaction. The game engine owns location, presence, and candidate eligibility;
the narrator receives only the player-safe result.

Commands also need an authored selection policy. Inferring eligible targets
from command IDs made custom commands inconsistent with standard commands and
risked letting the options projection disagree with reducer authorization.

## Decision

Story Format 3.1 adds optional character placement and presence gates, and adds
optional declarative candidate selection to command parameters. The additions
are compatible with Format 3.0 when omitted. Stories opt into the new standard
command behavior by selecting the exact immutable ruleset version
`ruleset.standard_mystery@2.0.0`.

### Narrative kind, knowledge, location, and presence remain separate

`character`, `entity`, and `setting` continue to describe what a definition is.
Knowledge controls which references are player-safe. Location is authoritative
world state. Presence is a derived, player-scoped answer to whether a placed
character can participate in local physical interaction.

Public character identity is not a presence secret. A player may know a missing
character's name, description, authored existence, event participation, and
already-known fact or deduction references without learning the character's
current location or the unmet condition that hides them locally.

### Character placement

A character may declare an initial setting location:

```yaml
characters:
  - id: character.echo
    name: Echo
    description: A champion detection dog known to every handler.
    initial:
      location: setting.hidden_rival_run
    presence:
      requires: flag.echo_discovered
```

`initial.location` must reference a setting. Initialization seeds that value
into authoritative character-location state. An existing `move` effect may
move the character to another setting; persistence, retry, reload, replay, and
world events preserve that transition deterministically.

Placement is setting-only in Format 3.1. Characters are not entities, cannot be
portable, and cannot be contained by another character, entity, or player
inventory. This intentionally avoids importing entity transformation,
containment, and inventory semantics into the character model.

A character without `initial.location` is unplaced and is never a
`current_location` candidate. Declaring `presence` without placement is an
authoring error because its gate could never make the character locally
available.

### Player-scoped physical presence

`presence.requires` uses the normalized one-or-many persistent requirement
contract. A placed character is physically present to a requesting player only
when both conditions hold:

- the character's current authoritative location equals that player's current
  setting; and
- every presence requirement is satisfied by that player's scoped state.

Presence is derived rather than separately mutable. Different players may see
different local candidate sets for the same character and world revision.

The following are implementation requirements:

- Never expose a remote character's authoritative location in player-safe
  state, options, events, narration prompts, or rejection messages.
- Never expose an unmet presence requirement or distinguish which private gate
  failed.
- Exclude unplaced, remote, and presence-gated characters from local candidate
  sets and relevant-character narration.
- Retain public identity and references already authorized through the
  player's safe facts, deductions, or events.
- Preserve historical event and narration projections at their original
  revision; later movement cannot rewrite what a player previously observed.

### Declarative command candidates

A command parameter may define the player-safe sets from which it selects:

```yaml
commands:
  - id: command.secure
    name: Secure
    parameters:
      - name: item
        types: [entity]
        min: 1
        max: 1
        candidates:
          from: [current_location]
          capabilities: [portable]
```

`candidates.from` is a non-empty ordered set. The engine resolves every source,
unions and deduplicates results, applies the parameter's allowed types, then
intersects capability filters. Ordering is deterministic. Candidate evaluation
is read-only and revision-bound.

The authoritative source meanings are:

| Source | Meaning |
| --- | --- |
| `all` | Every player-safe definition of an allowed type. Author-private, concealed, or otherwise unsafe definitions are excluded. |
| `current_location` | The current setting, active visible entities rooted there, and characters there whose player-scoped presence gates pass. |
| `inventory` | Active visible entities contained by the acting player, including reachable visible nested contents. |
| `reachable` | Settings reachable from the player's current setting through routes currently usable by that player. |
| `known` | Allowed references in safe state or the notebook: public characters; safely revealed entities and settings; events with a player-known owned fact; and established deductions. |
| `established` | Deductions authoritatively established by the acting player. |

Format 3.1 defines one capability filter:

| Capability | Meaning |
| --- | --- |
| `portable` | The candidate is an entity with `physical.portable: true`. |

Unknown sources and capabilities are invalid. A source that cannot yield any
allowed parameter type is invalid. `portable` is invalid for a parameter that
cannot select entities. Capabilities are a closed engine vocabulary, not
author-defined executable predicates.

The options API and turn reducer must call the same authoritative resolver. A
valid definition ID is insufficient authorization: forged, stale, remote,
hidden, presence-gated, or otherwise unavailable selections fail with a generic
player-safe rejection.

### Worked examples

Echo above is a known-but-missing character. Before `flag.echo_discovered` is
satisfied for a player, Echo remains selectable through a parameter using
`known` but is absent from `current_location`. Echo's hidden location and gate
remain absent from that player's safe projection.

A local portable entity uses physical capability rather than narrative kind:

```yaml
entities:
  - id: entity.lead_rope
    type: object
    name: Lead rope
    description: A coiled training lead.
    physical:
      portable: true
    initial:
      container: setting.kennel
```

At the kennel, a Take parameter using `from: [current_location]` and
`capabilities: [portable]` offers the lead rope. After it enters inventory, it
leaves that source and becomes eligible through `inventory` for Drop or Use.

### Versioned standard mystery ruleset

`ruleset.standard_mystery@1.0.0` remains immutable. Its command signatures and
legacy Format 3.0 fallback behavior cannot change after release.

`ruleset.standard_mystery@2.0.0` declares candidates explicitly:

| Parameter | Sources and filters |
| --- | --- |
| Move destination | `reachable` |
| Open/Search target | `current_location` |
| Examine target | `current_location`, `inventory` |
| Take item | `current_location` + `portable` |
| Drop item | `inventory` + `portable` |
| Use item | `inventory` |
| Use target | `current_location`, `inventory` |
| Question character | `current_location` |
| Question topic | `known` |
| Solve suspect | `known` |
| Solve theory | `established` |

Custom authored commands use the same candidate contract and resolver. No
Format 3.1 path may recover command-ID-specific target filtering.

### Compatibility

- `case.format_version: "3.1.0"` selects the additive contract.
- Format 3.0 stories without `initial`, `presence`, or `candidates` continue to
  validate and execute unchanged.
- A parameter without `candidates` retains its Format 3.0 fallback behavior.
- A consumer that does not support 3.1 rejects the declared format before
  partially interpreting it.
- No existing field changes meaning.
- Ruleset behavior changes only through an exact version selection.

### Release coordination

One validator source commit defines the contract for all consumers. A release
must synchronize:

1. the Rust validator version and source;
2. the backend Git dependency and lockfile;
3. the checked-in authoring WASM package built with the crate-pinned
   `wasm-bindgen-cli`; and
4. maintained-story validator action pins.

Native Rust, backend runtime/replay, browser WASM, author unit/browser, and
maintained-story validation gates must agree before the coordinated version is
published.

## Consequences

The engine gains explicit character locality without changing a character into
an inventory object. Authors can express local, inventory, reachable, known,
and established selection policies consistently for standard and custom
commands. Options become explanatory projections of reducer authorization
rather than a separate rule implementation.

The backend must retain private authoritative locations and per-player
requirements while producing safe, revision-bound projections. UI author views
may inspect those mechanics, but player-facing Test Play must use the same safe
boundary as production.

## Rejected alternatives

- Treating a missing character as an entity or proxy entity was rejected
  because it loses character portrayal, testimony, event, and fact semantics.
- Making characters portable or generally containable was rejected because it
  introduces inventory and nested-containment behavior without a story need.
- Hiding the complete character definition behind presence was rejected because
  knowledge and physical availability are independent.
- Hardcoding candidate rules by command ID was rejected because extensions and
  reducer validation would diverge.
- Allowing arbitrary authored predicates was rejected because executable author
  logic is not portable, auditable, or safe.
- Mutating presence as separate runtime state was rejected because persistent
  facts, flags, and triggers already provide the authoritative gate vocabulary.

## Explicit non-goals

Format 3.1 does not add player-relative aliases, disguises, health or death
state machines, autonomous NPC behavior, character inventory/containment,
per-character command allowlists, private resolution roles, structured fact
time intervals, deduction supersession, conditional replacement descriptions,
or arbitrary candidate predicates. It does not replace facts, flags,
deductions, triggers, effects, or generic win states.

## Implementation issues

- [Validator contract and ruleset](https://github.com/phurley/narrator-validator/issues/23)
- [Backend runtime and authorization](https://github.com/phurley/narrator-backend/issues/48)
- [Backend pre-3.1 saved-game purge](https://github.com/phurley/narrator-backend/issues/49)
- [Authoring editors and computed views](https://github.com/phurley/narrator-author/issues/31)
- [Simple Mystery migration](https://github.com/phurley/simple_mystery/issues/26)
- [Island Retreat migration](https://github.com/phurley/island_retreat/issues/28)

# Story Format 3.6: player descriptions, personas, and player condition tokens

Story Format 3.6 lets a story describe its players. `case.players` gains an
optional free-text `description` of the player role in general, and an
optional `personas` list of selectable player identities. Authored
conditions gain a matching `persona.<id>` / `player.<n>` vocabulary so a fact
or trigger can be gated on "the acting player is this persona" or "the
acting player is in this seat."

```yaml
case:
  id: case.last_tide
  format_version: "3.6.0"
  players:
    min: 1
    max: 4
    description: One of you is secretly the detective; the rest play witnesses.
    personas:
      - id: persona.detective
        name: The Detective
        description: Leads the investigation and asks the questions.
        narrator_guidance: Address this player directly when revealing deductions.
      - id: persona.witness
        name: A Witness
        description: Has firsthand knowledge of the night's events.
```

`case.players.description`, `case.players.personas`, and the `persona.`/
`player.` condition tokens all require `format_version >= 3.6.0`. Authoring
them under an earlier format produces a versioned-feature diagnostic naming
3.6.0 (`case.players_personas_format_incompatible` for the `case.players`
fields, `condition.player_format_incompatible` for the new persistent
predicate) rather than a generic unknown-field error.

## `case.players.personas`

`personas` is an optional, non-empty sequence nested in `case.yaml` under
`case.players`; there is no new top-level section or file. Each entry is a
mapping with:

- `id` (required) — a `persona.<snake_case>` ID. `persona.` is a namespace
  like every other kind prefix: `id.invalid` and `id.wrong_prefix` apply the
  usual canonical-ID rules, and `id.duplicate` applies if the same ID is
  reused by another persona *or* collides with any other authored
  namespace (a fact, character, command, etc. that happens to declare the
  same literal ID).
- `name` (required) — a non-empty display name
  (`case.players_persona_name` otherwise).
- `description` (optional) — player-facing prose describing the persona.
- `narrator_guidance` (optional) — private narrator-only guidance, following
  the same disclosure boundary as character `narrator_guidance`.

`personas` must not contain more entries than `case.players.max`
(`case.players_personas_max`); a story cannot offer more roles than it can
seat. There is no minimum: a story may declare zero, one, or many personas
independently of `min`.

## `persona.<id>` and `player.<n>` condition tokens

Format 3.6 adds a new `player` persistent-condition predicate, valid
anywhere `when.all` is (fact and trigger persistent conditions), and adds
`persona.`/`player.` IDs to the vocabulary accepted by testimony `requires`:

```yaml
entities:
  - id: entity.ledger
    facts:
      - id: fact.detective_hunch
        statement: The detective suspects the ledger is doctored.
        when:
          all:
            - player: persona.detective

triggers:
  - id: trigger.detective_briefing
    name: Detective briefing
    on:
      command: command.investigate
      parameters:
        target: entity.ledger
    when:
      all:
        - player: player.1
    effects:
      - operation: set_flag
        flag: flag.briefed
        value: true
```

`player: persona.<id>` means "the acting player has selected that declared
persona"; `player: player.<n>` means "the acting player is seated in slot
`n`." `n` must fall within `case.players.min..=case.players.max`; a
declared persona ID must appear in `case.players.personas`. Either an
undeclared persona or an out-of-range player slot fails with
`reference.unknown`, the same diagnostic used for any other dangling
authored reference. A `player` predicate combines with `at`, `owns`,
`knows`, `flag`, `completed`, and `time` predicates exactly like any other
entry in `when.all` (all conjunctive).

Testimony `requires` accepts `persona.<id>` and `player.<n>` alongside its
existing setting, route, character, entity, event, fact, deduction, flag,
command, and trigger IDs, with the same meaning: the testimony is only
available to a player who is that persona, or seated in that slot.

## Playability analysis of persona-conditioned facts

The bounded playability search proves reachability for one deterministic,
persona-less playthrough; it does not model which persona a player has
selected or which seat is "acting." A `player` predicate is therefore
treated as an unsupported, statically unsatisfiable condition
(`playability.unsupported_player_condition`), the same conservative
treatment already given to `owns` inventory-ownership predicates and other
mechanics outside the static subset:

- A fact or trigger effect gated on a `player` predicate is simply excluded
  from the reachable state space. If no terminal path's proof depends on
  it, this has no effect: the analysis still proves every other reachable
  end state exactly as before, so persona-gated content can never make an
  otherwise-winnable default playthrough report as unwinnable
  (`PlayabilityStatus::NotProved`).
- If an authored solution or end state genuinely depends on a persona-gated
  fact — for example a `requires` entry that resolves only under a
  `player` predicate — the search cannot find a supported path to prove it,
  and the presence of the unsupported condition also downgrades any
  apparently-proved path elsewhere in the story to `Inconclusive`, exactly
  as any other unsupported predicate already does. The terminal path's
  `blocker` names `playability.unsupported_player_condition` so the
  diagnostic is unambiguous rather than a generic "not proved."

Runtime evaluation of `player` conditions — resolving which persona or seat
is actually acting during play — is out of scope for this format revision
and is tracked as a separate narrator-backend ticket.

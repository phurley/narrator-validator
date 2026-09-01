# Story Format 3.7: multi-step Solve and answer decks

Story Format 3.7 replaces the single-commit `solution.questions` contract
with `solution.steps`, an ordered sequence of independently-committed Solve
steps. It is paired with `ruleset.standard_mystery@7.0.0`, which keeps
`command.solve` byte-for-byte parameterless but retires the narrow
persistent-`requires` carve-out Ruleset 5 added for the single selected win
state, folding it into the general `end_states` conjunction described below.
Rulesets 1.0 through 6.0 remain byte-for-byte immutable.

```yaml
case:
  format_version: "3.7.0"
  ruleset:
    id: ruleset.standard_mystery
    version: "7.0.0"

solution:
  max_attempts: 3
  steps:
    - id: step.name_the_culprit
      prompt: Who killed Rowan Vale?
      time_cost_minutes: 5
      rows:
        - match: n_of_m
          n: 1
          cards: [character.mara_voss]
      on_success:
        effects:
          - operation: set_flag
            flag: flag.culprit_named
            value: true
        points: 10
      on_failure:
        points: -5
        notes: An innocent name is spoken aloud, and the real culprit relaxes.

end_states:
  - id: end.solve_case
    name: Solved the case
    outcome: won
    resolution: full
    requires: [flag.culprit_named]
    text: You explain the complete solution.
```

`solution.steps`, per-step `rows`, `on_success`/`on_failure`, `max_attempts`,
and `session_timeout_minutes` all require `format_version >= 3.7.0`.
`solution.questions` and `solution.win_state` are format 3.3–3.6 fields;
Format 3.7 rejects them outright (`solution.legacy_contract`) rather than
accepting a mixed contract, matching the precedent set when Format 3.3
retired the culprit/weapon/location/deduction contract.

## Why steps instead of one multi-question commit

Formats 3.3–3.6 answered every question in a single physical-grid commit: one
command-only row plus up to four answer rows, submitted together with one
`command.solve` press. That model cannot express a sequence where an early
wrong answer should cost the player something before they see the rest of
the puzzle, or where different combinations of correct/incorrect steps should
lead to different endings.

Format 3.7 instead gives each *step* its own commit. Per step, the player
gets the full 5×5 physical card grid — there is no reserved command-only row,
because which command is running is now tracked by the backend-owned solve
session (see "Session state" below), not re-asserted by placing a
`command.solve` card each commit. `command.solve` itself is unchanged and
parameterless: pressing ENTER on a solve-session grid submits exactly one
step, in the existing enter-card fashion. Nothing about how a physical
commit is scanned or submitted changes for the player.

## `solution` fields

- `max_attempts` (optional) — a whole number `>= 1` bounding how many times
  the player may work through the full step sequence before Solve becomes
  permanently unavailable (`solution.max_attempts_invalid` if zero or
  negative). Omitted means unlimited attempts, which is the Format 3.3–3.6
  behavior and therefore the correct migration default: those formats had no
  attempts concept, i.e. effectively unlimited retries.
  - An attempt is consumed only when a full run through the steps ends in a
    restart (a wrong step, or a cancel after step 1 — see below). A
    successful run that reaches the last step does not consume an attempt,
    because the game is over at that point.
  - There is no distinct authored value for "no attempts allowed": a story
    that wants Solve unreachable should not grant the player
    `command.solve` capability at all (a `requires` on that mechanic, same
    as any other gated command), not encode it as `max_attempts: 0`.
- `session_timeout_minutes` (optional) — a whole number `>= 1` of real-world
  (wall-clock, not game-clock) minutes. If authored, the backend converts an
  abandoned session — no step commit received within that many minutes of
  the last one — into a cancel at the player's current step, under the same
  free-at-step-1/fail-afterward rule that governs an explicit blank commit.
  Session survival and resumption themselves (a player reconnecting to find
  themselves still on step 3) are runtime behavior, out of scope for this
  document; this field only bounds how long an *abandoned* session is kept
  alive before the backend gives up on it.
- `steps` (required, minimum one entry) — see below. There is no fixed
  maximum step count: unlike the single-commit contract, steps are
  submitted one at a time, so the only per-submission size limit is each
  step's own five-row grid.

## `solution.steps[]`

Each step is a mapping:

- `id` (required) — a globally unique `step.<snake_case>` ID
  (`id.invalid`, `id.wrong_prefix`, `id.duplicate` apply as usual). Stable
  across edits that don't change step content, since the backend-owned solve
  session and any replay/telemetry data reference the step by this ID, not
  by its position.
- `prompt` (required, non-empty) — player-facing prose asked before the step
  is committed. Baseline player-safe and a registered
  `solution_step:prompt` reference-text consumer, exactly like the Format
  3.3 question `prompt` it replaces; using `[[...]]` still requires
  negotiating `reference_text_v1`.
- `time_cost_minutes` (required) — a non-negative whole number of game-clock
  minutes charged when this step is committed, win or lose. This is the
  step-level time expense called for in the design; it is authored directly
  here rather than through `costs.yaml`'s `command_costs`, because
  `command.solve` remains parameterless and `command_costs` already
  requires a target parameter to key on
  (`command_costs.command_parameterless`, Format 3.5).
- `rows` (required, one to five entries) — positional; see below.
- `on_success` / `on_failure` (both optional) — see below.

Any other key is `solution.step_unknown_field`.

### Rows and strict judging

Each row is a mapping with a `match` discriminator:

```yaml
rows:
  - match: n_of_m
    n: 2
    cards: [entity.diving_knife, entity.degreaser_bottle, entity.tide_chart]
  - match: ordered
    cards: [setting.generator_shed, setting.tide_observatory]
```

- `n_of_m` — `cards` lists the `m` cards (one to five, unique) that are
  individually acceptable in this row; `n` (required, `1 <= n <= m`) is how
  many of them the player must place, in any order and in any of the row's
  physical cells. A submission is correct only when it has exactly `n`
  cards, every one drawn from the authored `cards` pool, with no repeats.
  This generalizes the Format 3.3 unordered-answer contract, which is the
  degenerate `n == m` case (see "Legacy compatibility" below).
- `ordered` — `cards` lists the exact required sequence (one to five,
  unique). A submission is correct only when it is that complete sequence,
  in that order, in that row's cells. There is no separate `n`; an authored
  `n` alongside `match: ordered` is `solution.row_ordered_has_n`.

Judging a step is strict over the *entire* committed grid, not just the
cells the story authored rows for:

- Every authored row must match as described above.
- Every physical grid position not covered by an authored row — a row
  beyond the step's `rows` list, or an unused cell within a row it did
  define more cards than needed for — must be empty. Any card anywhere the
  story didn't ask for fails the step
  (`solution.step_unexpected_card`, a runtime-evaluated condition; the
  validator's role is limited to the static well-formedness checks below).
- The same physical card ID must not appear twice within one step, whether
  repeated within one row's `cards` or reused across two rows of the same
  step (`solution.step_card_duplicate`). Unlike Format 3.3, this is scoped
  to the step, not the whole solution: because each step is a separate
  commit, the same card may legitimately reappear in a *later* step (for
  example, the same suspect card named again to establish means after being
  named for motive in an earlier step).

A **blank commit** — an entirely empty grid, no cards placed at all — is a
**cancel**, not a row-matching failure:

- On the first step of the current attempt, a cancel is free: it consumes
  no attempt, fires no step's `on_failure`, and leaves the session parked at
  step one awaiting another commit.
- On any later step, a cancel is treated exactly as a wrong answer on that
  step: that step's `on_failure` fires, and the attempt restarts (see
  below).

A **wrong step** — any non-blank commit that does not satisfy every row
exactly as specified — always restarts the full solve: that step's
`on_failure` fires, the attempt is consumed, and the next commit is
evaluated against step one again, regardless of how many steps had already
been answered correctly this attempt. Format 3.7 does not offer a
"retry just this step" outcome; the design deliberately keeps failure
uniform and total per attempt, while still letting an author make partial
*progress* durable across attempts through `on_success` flags (see "Graded
endings" below) — an attempt failing does not erase flags a step's
`on_success` already set earlier in that same attempt or a previous one.

### `on_success` / `on_failure`

Both are optional mappings, evaluated exactly once when their outcome
occurs:

- `effects` (optional) — a sequence in the same shape as command/trigger
  `effects`, restricted to `operation: set_flag` (`flag`, `value`); other
  operations are `solution.step_effect_operation_unsupported`. There is no
  `after` delay — a step outcome's effects apply immediately, not on a
  future trigger tick.
- `notes` (optional) — private narrator-only prose describing this outcome
  for the next narration turn, following the same disclosure boundary as
  `narrator_guidance` (registered `solution_step:on_success.notes` /
  `solution_step:on_failure.notes` reference-text consumers,
  `PrivateNarrator`). Never sent to a player client.
- `points` (optional) — a signed whole number added directly to the
  player's running score when this outcome occurs. This is a new,
  simpler mechanic distinct from the existing `points: {value,
  max_claim_count, requires}` claimable point award on settings, entities,
  deductions, and commands (`validate_point_awards`): that mechanism grants
  a fixed positive amount once, gated by `requires`; a step's `points` is an
  unconditional signed delta — positive on `on_success`, typically negative
  on `on_failure` — applied every time that outcome fires, supporting
  multi-player ranking by how cleanly each player solved the case. The two
  fields are disambiguated by context (this one is only valid nested under
  `on_success`/`on_failure`) and never share a schema.

Any other key is `solution.step_outcome_unknown_field`.

## Graded endings replace `win_state`

Format 3.3–3.6 named exactly one end state as the Solve target
(`solution.win_state`) and forbade that state from declaring `requires` or a
positive `minimum_points` — Ruleset 5 narrowly lifted the `requires` half of
that restriction for a persistent world-flag prerequisite (used today by
`quiet_kennel`'s `flag.echo_recovered`).

Format 3.7 removes `solution.win_state` and that carve-out entirely.
Instead, because every step outcome can set an ordinary flag, an end state's
existing `requires` (Format 3.4) is the graded-endings mechanism: author one
flag per step outcome that should count toward a given ending, and list
those flags — plus any unrelated persistent prerequisite, exactly as before
— on whichever `end_states` entries should be reachable for that
step-result subset. This is a strict generalization of the ruleset 5
carve-out, not a narrower replacement: an end state may now combine any
number of step-outcome flags with any other persistent condition, using the
same conjunctive, first-satisfied-wins evaluation Format 3.4 already
defines. No new end-state fields, disclosure rules, or evaluation timing are
introduced by this document.

```yaml
end_states:
  - id: end.full_solution
    name: The whole truth
    outcome: won
    resolution: full
    requires: [flag.culprit_named, flag.method_established, flag.timeline_ordered]
    text: You prove every material element of the case.

  - id: end.partial_solution
    name: A defensible theory
    outcome: won
    resolution: partial
    requires: [flag.culprit_named]
    text: You name the culprit but never nail down how or when.
```

Author more specific (larger flag-set) full resolutions before broader
partial ones, exactly as Format 3.4 already requires for any other
`requires` conjunction.

## `answer.*`: knowledge-eligible, no-world-state subjects

Format 3.7 adds a new subject namespace, `answer.*`, supplied by the
ruleset the same way `command.*` is: the resolved ruleset's answer-deck
catalog is merged into the story's definitions automatically (see
`merge_ruleset_commands`, whose `answer.*` counterpart Format 3.7's
implementation adds alongside it), never authored by the story itself.
Ruleset `7.0.0` defines three generic decks:

- `answer.motive.*` — 10 coarse motive buckets (`greed`, `jealousy`,
  `revenge`, `fear_of_exposure`, `self_preservation`, `fear_for_another`,
  `love`, `ambition`, `desperation`, `loyalty`).
- `answer.time.*` — 8 coarse time-of-incident buckets (`dawn`, `morning`,
  `midday`, `afternoon`, `evening`, `night`, `after_midnight`,
  `before_dawn`). `answer.time.*`
  cards may also answer ordinary mid-game `command.question` topics, not
  only solve rows — a player can ask a witness "was it before or after
  midnight?" the same way they ask about a character, setting, or event.
- `answer.method.*` — 11 coarse method buckets (`struck`, `stabbed`,
  `shot`, `poisoned`, `strangled`, `drowned`, `fell`, `fire`, `crushed`,
  `neglect`, `not_killed`).

The complete catalog — every card's display `name`, its description, its
assigned `tag_id`, and the sizing and coverage rationale behind each deck —
is [Answer-deck vocabulary](answer-deck-vocabulary.md), which is the
authoritative reference the ruleset `7.0.0` implementation is built from.

Every `answer.*` ID carries a ruleset-supplied canonical display `name`
(the same role `character.name`/`setting.name` play), so
`[[answer.motive.jealousy]]` in a fact, deduction, or prompt renders
"Jealousy" without the story authoring anything beyond the reference.

`answer.*` subjects are **knowledge, never world state**: they are eligible
anywhere an entity/setting/character ID already is as *knowledge* — a
fact's or deduction's statement text, a `command.question` `topic`
selection, and, centrally, a solve row's `cards` — but they are never valid
in a world-state position: never `at` a setting, never `owns`-eligible
inventory, never a `command.move`/`command.take`/`command.examine` target.
Authoring one in any of those positions is
`subject.answer_no_world_state`, mirroring the existing
`deck.subject_unsupported` treatment given to kinds that can't be bound to
a physical card at all, except here the subject *is* a physical card and
the rejection is about placement, not deck binding.

### `deck.yaml` binding and the reserved tag range

`answer.*` subjects bind to a physical card the same way `command.*`
subjects do: a story that uses one in a solve row or a fact must declare it
in `deck.yaml` with `{ tag_id, subject }` before the validator will accept
the reference, exactly like any other deck subject
(`deck.subject_unknown` otherwise).

Unlike settings, characters, entities, and commands, an `answer.*` card's
`tag_id` is **fixed by the ruleset, not chosen by the author**. These cards
model a small set of generic, physical accessory cards (a "motive deck," a
"time-of-night deck," a "method deck") meant to be printed once and reused
across every story that opts into them, the same way the two ENTER control
cards are fixed across every story rather than author-assigned
(`src/scanner_control.rs`). tagStandard41h12 IDs **2000 through 2112**
inclusive (113 IDs) are permanently reserved for ruleset-owned answer
decks, immediately below the existing scanner-control reservation at 2113
and 2114. No story may bind a setting, character, entity, or command
subject to a tag in that range
(`deck.tag_id_reserved_ruleset_answer_deck`), and an `answer.*` subject's
`deck.yaml` entry must use exactly its ruleset-assigned `tag_id`
(`deck.answer_tag_id_mismatch` if it names any other value). Ruleset
7.0.0's 29 cards occupy tag IDs 2084–2112; IDs 2000–2083 remain reserved,
unassigned headroom for future ruleset-owned answer decks, consistent with
the existing rule that a released ruleset version's assignments are
append-only and immutable once shipped.

```yaml
# deck.yaml
cards:
  - { tag_id: 2111, subject: answer.motive.jealousy }
  - { tag_id: 2091, subject: answer.method.poisoned }
```

## Legacy compatibility

Format 3.3–3.6's single commit — one command row plus up to four answer
rows, submitted together — maps onto Format 3.7 as **one step** containing
one row per legacy question, in the same order:

- A legacy unordered answer (`answer: [...]`, no `ordered: true`) becomes an
  `n_of_m` row with `n` equal to `m` equal to the answer's card count: the
  degenerate case where every listed card is required, which is exactly
  the old "missing, extra, or duplicate cards are wrong" rule.
- A legacy `ordered: true` answer becomes an `ordered` row with the same
  `cards` in the same sequence.
- The migrated step's `time_cost_minutes` should equal whatever
  `command.solve`'s ruleset command-default cost already was, so that
  authored clock behavior is unchanged; migration tooling can read that
  value directly from the resolved ruleset rather than the author guessing
  it.
- The migrated step's `on_success` sets one new flag (for example
  `flag.solve_case_answered`); `on_failure` is empty (no effects, no
  points, no notes) so an incorrect legacy submission behaves as it always
  did — nothing happens, and the player may try again immediately, with
  `max_attempts` left unset for unlimited retries.
- The end state formerly named by `solution.win_state` gets that new flag
  added to its `requires` list, preserving whatever `requires` it may
  already have carried from the Ruleset 5 carve-out (see
  `quiet_kennel` in the worked example below) — a plain conjunction, no
  special casing.

Because a legacy solution never had more than four questions and Format 3.7
allows up to five rows per step, this mapping never overflows the new
single-step's row budget.

## Worked example: `simple_mystery`

`simple_mystery`'s current Format 3.6 solution:

```yaml
solution:
  win_state: win.solve_briar_house
  questions:
    - prompt: Who killed [[character.adrian_bell]]?
      answer: [character.lena_ortiz]
    - prompt: Which object and room identify the murder method and location?
      answer: [entity.brass_service_bell, setting.study]
    - prompt: Which records establish opportunity first and motive second?
      answer: [entity.study_door_log, entity.cash_ledger]
      ordered: true
```

expressed under Format 3.7 (`ruleset.standard_mystery@7.0.0`), following the
migration mapping above:

```yaml
case:
  format_version: "3.7.0"
  ruleset:
    id: ruleset.standard_mystery
    version: "7.0.0"

solution:
  steps:
    - id: step.solve_briar_house
      prompt: >
        Who killed [[character.adrian_bell]]? Which object and room identify
        the murder method and location? And which records establish
        opportunity first and motive second?
      time_cost_minutes: 0
      rows:
        - match: n_of_m
          n: 1
          cards: [character.lena_ortiz]
        - match: n_of_m
          n: 2
          cards: [entity.brass_service_bell, setting.study]
        - match: ordered
          cards: [entity.study_door_log, entity.cash_ledger]
      on_success:
        effects:
          - operation: set_flag
            flag: flag.solve_briar_house_answered
            value: true

end_states:
  - id: win.solve_briar_house
    name: "Solved [[case.last_bell_at_briar_house]]"
    outcome: won
    resolution: full
    requires: [flag.solve_briar_house_answered]
    text: >
      [[setting.study.name]] is where [[character.lena_ortiz]] killed
      [[character.adrian_bell]] with the [[entity.brass_service_bell]], after he
      # ...unchanged tail of the existing end state text
```

`time_cost_minutes: 0` preserves `simple_mystery`'s current behavior exactly
(its `command.solve` carries no per-story `command_costs` override today, so
Solve's cost is whatever the ruleset command default already is at
`0` additional minutes beyond that default — migration tooling should
substitute the resolved default rather than a literal `0` if a future
ruleset ever ships a nonzero one). `max_attempts` and
`session_timeout_minutes` are both omitted, matching the unlimited-retry
behavior `simple_mystery` already has. `quiet_kennel`'s single question
migrates the same way, except its migrated end state's `requires` becomes
`[flag.echo_case_answered, flag.echo_recovered]` — the new step-outcome flag
joined, not replaced, by the persistent world flag Ruleset 5 already
required there.

## Disclosure boundary

`solution.steps[].prompt` is `solution_step:prompt`, `PlayerSafe` (or
`GatedPlayerSafe` once negotiated via `[[...]]`), replacing
`solution_question:prompt`. Row `cards` — expected answers — remain private
mechanical truth, exactly as Format 3.3's expected answers were:
`PrivateNarrator`, never exposed to a player client before a submission is
evaluated. `on_success.notes` and `on_failure.notes` are
`PrivateNarrator`, following `narrator_guidance`. A valid report and any
player-safe consumer expose only: the current step's prompt, its rows'
required card counts and `match` policy (so a client can render the right
number of empty cells and know whether order matters), and safe card
presentation — never the expected IDs, never which specific `n`-subset of an
`n_of_m` pool is correct.

The `solutionContractMetadata` browser API (replacing `SolutionContractMetadata`
in `src/solution.rs`) is extended with `min_steps` (`1`), `min_step_rows`
(`1`), `max_step_rows` (`5`), `min_row_cards`/`max_row_cards` (`1`/`5`,
carried over unchanged from `MIN_SOLUTION_ANSWER_CARDS`/
`MAX_SOLUTION_ANSWER_CARDS`), and the ruleset's reserved answer-deck tag
range (`2000`–`2112`).

## Out of scope for this document

Schema validation for `solution.steps` (rejecting malformed rows, enforcing
the reserved tag range, extending `Kind`/`CommandParameterType` with
`Answer`, generalizing `solution_answer_matches` to `n_of_m`) is
`narrator-validator#79`. Playability analysis of a multi-step, multi-attempt
solve — whether the bounded search can still prove a default playthrough
reaches a graded ending — is `narrator-validator#80`. Backend solve-session
state, resumption after disconnect, the step-sequence idempotency number,
and real-time session-timeout enforcement are `narrator-backend` tickets
(`#504`–`#507`). Client rendering of the full per-step grid and scanner
changes are `narrator-app` tickets (`#508`–`#509`, `#482`, `#218`). This
document defines only the story-format schema and ruleset contract those
tickets build against.

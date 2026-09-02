# ADR 0002: Static playability model — take-only portable-entity inventory

- Status: Accepted
- Date: 2026-09-02
- Owners: Narrator validator (static playability analysis only)
- Proposal: [narrator-validator issue #96](https://github.com/phurley/narrator-validator/issues/96)
- Scope note: this is a validator-internal modeling decision about the
  static reachability search's supported action subset. It introduces no
  new authored story fields, no runtime/backend behavior change, and no
  Story Format revision. It does not require the multi-repo release
  coordination Format ADRs need.

## Context

`src/playability.rs`'s static reachability search proves or disproves
end/step reachability over a conservative, monotonic subset of story
mechanics. Before this decision, the model had no concept of inventory at
all: `subject_locations` was fixed at initialization and never changed, so
any `on` binding pattern (a trigger's or fact's) that named an
`entity.*`/`character.*` subject required that subject to be co-located
with the player for the lifetime of the search.

This is unsound relative to what the runtime actually supports.
`command.take`/`command.drop` and `physical.portable` are real, ratified
Format 3.1 mechanics (see
[ADR-0001](0001-story-format-3.1-character-presence-and-command-candidates.md)),
and stories rely on them: quiet_kennel's
`trigger.test_jo_curry_against_sedative_audit` binds
`target: entity.jo_curry_bowl` (portable, starts in `setting.staff_canteen`)
and `comparison: entity.sedative_cabinet_audit` (fixed, in
`setting.veterinary_clinic`) — two different rooms. A player can genuinely
carry the bowl to the clinic and trigger the comparison; the static model
could never represent that play-through, so it reported the step as
unreachable (in practice, `inconclusive`/`search_bound`, since nothing else
in the story could saturate the search budget on its own either) even
though it is actually playable.

## Decision

The static model gains a monotone `command.take` action and a
`State.inventory: BTreeSet<String>`:

- An `entity.*` with authored `physical.portable: true` becomes a
  `command.take` candidate whenever it's co-located with the player and not
  already carried.
- `command.take` only ever inserts into `state.inventory`. There is no
  `command.drop` action and no removal path.
- A binding resolution (`action_available`, `subject_known`) for an
  `entity.*` subject now accepts EITHER co-location OR inventory
  membership. `character.*` binding resolution is untouched — characters
  are never carryable (ADR-0001 already rejected that).

### Take-only, not drop, and why that stays sound

The supported predicate subset (`Predicate`: `Has`, `At`,
`TimeAfter`/`Equal`/`Before`, `Never`, and the `has()` helper that backs
them) has no variant that can require a subject's *absence*, or that an
entity specifically NOT be carried. Every supported predicate only ever
asserts presence. Given that, once a portable entity has been picked up,
there is no supported predicate that could newly start failing because of
it — carrying is therefore safe to model as monotone, and `command.drop`
(were it modeled) could never make a previously unreachable end/step
reachable. Omitting drop is not a completeness gap in this subset; it is
provably inert. If the predicate subset ever grows an absence/negation
form, this argument — and this decision — need to be revisited.

### Take-candidate scope is restricted, not exhaustive

`actions()` does not generate a `command.take` candidate for every
`physical.portable: true` entity in a story. It restricts candidates to
`takeable_entities`: portable entities that actually appear in some
trigger's `on` binding, a `solution_answer_rows` row, or a `solve_steps`
row's pool/cards. An owned fact's own `on` pattern (`target: owner`) always
resolves to that fact's own entity and is satisfied by co-location with
itself regardless of portability, so fact `on` bindings are deliberately
excluded from this set — including them made every portable entity in
quiet_kennel (12) "takeable" for no reachability benefit and exhausted the
search budget before the genuinely blocked trigger could be found. This
scoping is a search-performance concern, not a soundness one: adding more
take candidates can only expand the reachable set (monotone under
expansion), never shrink it, so a narrower `takeable_entities` can only
ever produce a subset of witnesses the unrestricted set would find, never
a false one.

## Consequences

- `MODEL_VERSION` moves from 3 to 4.
- Cross-room trigger bindings that involve a portable entity on one side
  become provable where they were previously always `inconclusive` or
  `NotProved`, without any story authoring change.
- Stories with several portable entities gated behind cross-room triggers
  add an inventory dimension to the search's state space (up to 2^k for k
  `takeable_entities`). The restriction above keeps k proportional to
  actual cross-room need rather than a story's total portable-entity
  count, but does not eliminate the dimension. Deep, multi-step stories
  with several such triggers may need more than the current
  `MAX_EXPLORED_STATES` budget to converge; that is a search-budget
  question, not a modeling gap, and is out of scope for this decision.

## Rejected alternatives

- **Full portable-entity relocation (drop, generic "entity moved"
  effects).** Rejected for this increment as unnecessary complexity: no
  currently-maintained story authors a `command.drop`-dependent trigger,
  and the soundness argument above shows drop can never change a proof's
  outcome for the current predicate subset. Referred to in the originating
  investigation as "Option D" and explicitly deferred.
- **Unconstrained take-for-every-portable-entity.** Rejected because it
  reintroduces the exact state-space blowup narrator-validator#91/#94 were
  written to fix, for a different axis (inventory subsets instead of
  answer/witness combinations).
- **Raising `MAX_EXPLORED_STATES`** to force convergence on deep stories.
  Rejected: it papers over a genuine budget question with a bigger
  constant rather than answering it, and the ticket that produced this
  ADR was explicit that doing so is out of scope.

## Explicit non-goals

This decision does not add `command.drop` to the static model, does not
model container/nested-inventory transfer, does not change character
placement or presence semantics (ADR-0001 remains authoritative there),
and does not change any authored story field or Story Format version.

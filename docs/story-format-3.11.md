# Story Format 3.11: clock triggers

Format 3.11 lets a trigger match the shared clock instead of a player command.
The world can now act on its own: a clock trigger is due at a whole minute on a
game day, fires when the clock reaches that time, and delivers its narrative,
facts and effects to the players who witness it. This implements
[ADR-019](https://github.com/phurley/narrator-system/blob/main/ADR-019-timed-actions-and-clock-triggers.md).
Standard mystery ruleset 9 is retained — clock triggers are story content, not
command catalog — and no ruleset version is published for this change.

```yaml
triggers:
  - id: trigger.lena_leaves_parlor
    name: Lena leaves the parlor
    on:
      clock:
        day: 0
        time: "21:00"
    once: true
    location: setting.parlor
    narrative: Lena gathers her coat and leaves without a word.
    speaker: character.lena_ortiz
    when:
      all:
        - flag: flag.lena_confronted
    effects:
      - operation: move
        subjects: [character.lena_ortiz]
        setting: setting.kitchen
    facts:
      - id: fact.lena_left_early
        statement: Lena left the parlor at nine.
```

## The clock match

- `on.clock` is a mapping with a required quoted 24-hour HH:MM `time` and an
  optional non-negative integer `day`. `time` uses the same whole-minute
  `HH:MM` grammar as `case.initial_time`. `day` defaults to the case's initial
  day.
- `on.clock` and `on.command` are mutually exclusive, as are `on.parameters`
  and `on.actor`: a clock trigger matches the clock, not a command. A trigger
  with neither `on.clock` nor `on.command` remains the existing error.
- Every other Format 3.10 trigger rule carries over unchanged. Setting
  `case.format_version: "3.11.0"` on a 3.10 story with no clock triggers
  changes nothing else. A trigger with `on.clock` under a format below 3.11 is
  a hard error naming the field and the required version.

## One-shot evaluation

A clock trigger must declare `once: true`: it is evaluated exactly once, when
it first comes due. If its `when` predicates fail at that moment the trigger
is consumed and never fires — a failed predicate does not retry later. Because
that surprises authors, a clock trigger with `when` predicates produces a
**warning** diagnostic stating the consumption rule.

Predicates that reference the acting player — `at` and `knows` — are errors on
a clock trigger, because there is no actor. World-state predicates (`flag`,
`owns`, `completed`, `player`, `time`) are accepted.

## Location, narrative and speaker

- `location` is required and names a declared setting: the place where the
  event happens. It scopes witnessing — the players located there when the
  trigger fires receive its narration, facts and visible effects. It is not a
  condition.
- `narrative` is an optional non-empty string of player-safe prose delivered
  to the witnesses. A clock trigger whose only observable output is its
  narrative is valid.
- `speaker` is optional, requires `narrative`, and must name a declared
  character; the sentence is then dialogue by that character.
- `facts` follow the existing nested-fact rules and are revealed to witnesses.
- Effects use the existing operation vocabulary. An `after:` delay on a clock
  trigger's effect is rejected — the trigger already has a time.

## Duplicate due times

Multiple clock triggers may share the same `(day, time)`. They all fire when
the clock reaches that time, in **authored order**: `triggers.yaml` order is
the documented tiebreak.

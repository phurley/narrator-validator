# Story Format 3.4: ordered authored end states

Story Format 3.4 replaces the win-only terminal collection with one ordered
`end_states` section. A story can name and describe a full resolution, a
partial resolution, or a failure using the same persistent state, score, and
game-clock vocabulary.

```yaml
case:
  format_version: "3.4.0"

end_states:
  - id: end.complete_truth
    name: The whole truth
    outcome: won
    resolution: full
    requires: [deduction.complete_solution]
    minimum_points: 50
    text: You prove every material element of the case.

  - id: end.partial_truth
    name: A defensible theory
    outcome: won
    resolution: partial
    requires: [deduction.central_theory]
    text: You establish the central theory, but questions remain.

  - id: end.deadline_failure
    name: The trail goes cold
    outcome: lost
    resolution: failure
    requires: [flag.evidence_compromised]
    at_or_after: "23:00"
    text: The deadline passes before the evidence can be preserved.
```

`end_states` belongs in `end_states.yaml`. A canonical state has a globally
unique `end.*` ID, or may retain an existing `win.*` ID during migration. Its
`name`, `outcome`, `resolution`, and `text` are required. Legal pairs are
`won`/`full`, `won`/`partial`, and `lost`/`failure`.

## Conditions, score, and evaluation

All listed conditions are conjunctive:

- `requires` contains persistent setting, entity, fact, deduction, flag, or
  trigger IDs;
- `minimum_points` is an optional non-negative whole-number gate and defaults
  to zero;
- `at_or_after` is an optional quoted 24-hour `HH:MM` game-clock threshold.

The runtime evaluates the sequence after every resolved turn, once effects and
time advancement have settled. The first satisfied state is terminal. The
selected state does not award points: the player's current points become the
final score snapshot, and `minimum_points` is only a gate.

Authored order is therefore part of the story contract. Put a more specific
full resolution before a broader partial resolution. The validator rejects an
exact duplicate condition and any later state that is provably shadowed by a
broader earlier monotonic condition. It does not guess about reachability that
depends on future actions or mutually exclusive authored mechanics.

The Rust `end_state_contract_metadata` API and browser
`endStateContractMetadata` API expose canonical/legacy section names,
evaluation timing, first-satisfied precedence, legal outcome/tier pairs, score
semantics, and legacy defaults.

## Solve-selected states

Format 3.3 authored Solve questions continue to use `solution.win_state` so
existing stories and replay data retain their stable reference. It may point
to either a canonical end state or a legacy win state. Answering every authored
question is that state's sole condition, so the selected state must not also
declare `requires`, a positive `minimum_points`, or `at_or_after`.

## Automatic and manual notebooks

Format 3.4 stories may select `ruleset.standard_mystery@4.0.0` to retain Claim
and Deduce for manual game-instance policies while continuing to use
question-based Solve. Story files do not select a notebook policy. Every
deduction must be safe for deterministic automatic fixed-point establishment;
false/speculative conclusions and solution-equivalent terminal deductions are
reported by Case Health. See
[Automatic deductions and notebook safety](automatic-deductions.md).

## Legacy transition

`win_states` remains readable during the transition. Its IDs, authored order,
requirements, point gates, names, and text keep their existing meaning. Each
legacy entry is interpreted as `outcome: won` and `resolution: full`. Format
3.4 reports a migration warning rather than changing terminal behavior.

To migrate, rename the root file and section to `end_states.yaml` and
`end_states`, preserve sequence order and IDs, and add `outcome: won` plus
`resolution: full` to every entry. Do not retain both roots: two independent
precedence sequences are rejected.

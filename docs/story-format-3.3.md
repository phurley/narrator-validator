# Story Format 3.3: authored Solve card sets

Story Format 3.3 replaces the legacy fixed suspect/deduction Solve signature
with one to four authored questions answered using physical story cards. It is
paired with the immutable `ruleset.standard_mystery@3.0.0` catalog.

```yaml
case:
  format_version: "3.3.0"
  ruleset:
    id: ruleset.standard_mystery
    version: "3.0.0"

solution:
  win_state: win.solve_case
  questions:
    - prompt: Who killed Rowan Vale?
      answer: [character.mara_voss]
    - prompt: Which objects establish the method?
      answer: [entity.diving_knife, entity.degreaser_bottle]
    - prompt: In what order were these locations involved?
      answer: [setting.generator_shed, setting.tide_observatory]
      ordered: true

win_states:
  - id: win.solve_case
    name: Solved the case
    text: You explain the complete solution.
```

## Exact comparison and scanner limits

Questions retain authored order. Each answer contains one to five unique
setting, character, or entity IDs, and every answer ID must have a physical
binding in `deck.yaml`. A card may occur in only one question. There are at
most four questions, leaving one command-only row plus four answer rows in the
phone scanner's 5 × 5 grid.

`ordered` defaults to `false`. An unordered submission is correct only when it
has exactly the expected set: missing, extra, or duplicate cards are wrong.
With `ordered: true`, the complete submitted sequence must equal the authored
sequence. The Rust `solution_answer_matches` function and browser
`solutionAnswerMatches` wrapper expose that same comparison contract.
Only a trusted runtime may call the comparison with authored expected IDs;
player clients receive the result, never the expected-answer input.

## Win state and ruleset boundary

`solution.win_state` names a defined win state that supplies terminal name and
text. That selected win state has no `requires` or positive `minimum_points`:
answering every Solve question is its sole completion condition. Other generic
win states retain their normal requirements or point thresholds and remain
available for non-Solve endings.

Ruleset 3.0 keeps every 2.0 command unchanged except `command.solve`. Solve is
now parameterless because its answer rows come from the private authored
solution contract. Rulesets 1.0 and 2.0 remain byte-for-byte immutable.

Format 3.3 rejects legacy culprit/weapon/location/deduction solution fields,
mixed contracts, copied legacy Solve parameters, and a condition duplicated on
the selected win state. Migrate the format, ruleset, solution, and win state as
one coordinated change.

## Disclosure boundary

Question prompts are baseline player-safe prose and are registered
`solution_question:prompt` reference-text consumers. If a prompt uses
`[[...]]`, the story must also negotiate `reference_text_v1` as in Format 3.2.
Expected answer IDs are private mechanical truth: valid reports and player-safe
consumers expose only the prompt, required card count, `ordered` policy, and
safe card presentation. They never expose expected IDs or correctness before a
submission is evaluated.

The machine-readable `solutionContractMetadata` browser API publishes the
format/ruleset versions, limits, default ordering, and disclosure classes.

# Static playability analysis

Format 3 validation reports include a `playability` object in Rust, CLI JSON,
and WebAssembly output. This analysis is separate from structural validity: a
story can be structurally valid while an authored terminal path is not reachable
through the actions the static model can prove.

Each ordered terminal path has one status:

- `proved`: the analyzer found a concrete supported action sequence. Its
  `lower_bound` contains the entry setting, ordered route/command/deduction
  steps, action and route-action counts, elapsed and non-route wait minutes,
  required delayed-work waits, and pivotal facts, deductions, flags, triggers,
  and score awards.
- `not_proved`: exhaustive exploration inside the published bounds found no
  path. The blocker links to the terminal field and names the first missing
  requirement, score threshold, time constraint, or authored-precedence block.
- `inconclusive`: an unsupported dynamic mechanic or a deterministic search
  bound could affect the result. Unsupported behavior is never assumed to
  succeed.

Model version 2 starts at every `case.entry_settings` location with opening
facts and initial flags. The top-level compatibility summary uses the default
`auto_facts: true`, `auto_deductions: true` policy. `notebook_policies` contains
all four policy combinations; manual facts become explicit Claim steps and
manual deductions become explicit Deduce steps when the selected ruleset
provides those mechanics. It models routes and `travel_minutes`, exact `on`
command bindings, monotonic `when.all` predicates, fixed time advances,
positive flag assignment, fact and deduction establishment, delayed trigger
facts, setting/command/deduction point awards, and ordered end-state conditions.

`deduction_graph` reports deterministic `maximum_depth`,
`largest_cascade_size`, its root, and the ordered deduction IDs in that
cascade. Each policy also reports `solution_answerability`. It is `proved` only
when an established deduction's explicit structural `solves` mapping covers a
complete physical `solution.questions` answer row; ambiguous prose remains
`inconclusive` or `not_proved` rather than being guessed.
The search minimizes action count, then elapsed minutes, then stable state and
action identity. It explores at most 25,000 states, 96 actions per path, and
2,880 elapsed minutes.

Authored route requirements are checked before traversal. Format 3.3+ question
solutions contribute one exact `command.solve` action using the authored answer
rows, so the selected solution end state participates in normal authored
precedence. Fixed `advance_time` effects apply whether a command or trigger owns
them. Ordinary action triggers snapshot their action match and persistent
conditions before the action's mechanic and command effects, then settle their
effects against the authoritative post-action state.

Non-monotonic effects, nested entity inventory transitions, entity point
awards, entity/inventory route gates, duplicate deduction input sets, and
condition forms outside this subset are reported as `inconclusive`.
If any unsupported behavior is present, an otherwise supported candidate is
also downgraded rather than emitted as a false proof. The complete notebook and
engine state remain authoritative; this report is authoring analysis, not
gameplay state.

Text CLI output prints one compact line per terminal path. `--format json`
returns the full typed report used by browser consumers.

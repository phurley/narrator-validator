# Automatic deductions and notebook safety

Deductions are authoritative derived knowledge displayed in a player's
notebook. They are not world events, narration instructions, accusations the
player must role-play, or a second solution contract.

Every story must work under four game-instance policies:

| `auto_facts` | `auto_deductions` | Notebook behavior |
| --- | --- | --- |
| `true` | `true` | Facts enter immediately and every newly satisfied deduction closes automatically to a fixed point. |
| `true` | `false` | Facts enter immediately; the player deliberately uses Deduce. |
| `false` | `true` | The player deliberately Claims available facts; each claim may trigger automatic deduction closure. |
| `false` | `false` | The player deliberately Claims facts and establishes deductions. |

These booleans are game creation preferences and never appear in story YAML.
Closure is deterministic, acyclic, and player-scoped: only that player's
claimed facts and established deductions satisfy inputs. A deduction is added
once and its points are awarded once.

## Authoring rules

- Write each `conclusion` as a concise fact-like insight that is safe to show
  immediately when its inputs become known.
- Use one to three facts or prior deductions as `inputs`. Avoid long chains of
  relay nodes whose conclusions add no information.
- Do not author `truth: false` deductions. Automatic establishment would turn
  them into authoritative false knowledge. Retire them or wait for a future
  explicitly non-authoritative hypothesis mechanic.
- Avoid speculative phrasing such as “perhaps,” “might,” or “possibly.” The
  validator reports it for review because automatic mode cannot ask a player
  whether to endorse the theory.
- Do not copy a physical answer row, culprit/method/location combination, or
  final accusation into a deduction. `solution.questions` owns the private
  expected cards, Solve owns commitment and grading, and end states own the
  terminal result.

Mentioning a suspect, object, or place in a genuine intermediate insight is
valid. Semantic overlap checks are warnings with exact source pointers; only
explicitly contradictory shapes such as `truth: false` are errors. Prose that
cannot be proven equivalent is left inconclusive.

## Analysis

Playability model version 2 runs all four policies. Automatic policies settle
deductions to fixed point after every modeled fact transition; manual policies
include explicit Claim and/or Deduce actions. `deduction_graph` reports maximum
chain depth and the largest deterministic transitive cascade. Per-policy
`solution_answerability` is proved only from explicit structural overlap, never
from a guess about prose meaning.

Use `ruleset.standard_mystery@4.0.0` with story format 3.4 when all four policy
paths must be playable. Its exported semantic command capabilities tell the
runtime when Claim and Deduce should be visible while keeping Solve distinct.

# Playability validation performance

Issue #56 was measured with `scripts/profile-wasm-validation.mjs`. Each browser
loaded one WASM module, ran one unrecorded warm-up, then recorded three complete
`validateRepositoryWithFeatures` calls. The run held the workspace box claim;
one-minute load was 3.73 at start and stayed between 3.60 and 4.17 at each
reported boundary. Chromium and Firefox used Playwright 1.62.0.

The parent-equivalent package was the checked-in `narrator-author` validator
package. Its playability source is unchanged from parent `2217efe`. The
optimized package was built from `aea9a6a`. `island_retreat` was pinned at
`1351d55ab450332dc6df4749d7826597a707329e`.

## Exact author save fixture

The 11-file fixture is the exact YAML-only snapshot that
`narrator-author#337` passed to the worker after authoring two Solve questions
with three answer cards.

| engine | parent samples (ms) | parent median | optimized samples (ms) | optimized median |
|---|---:|---:|---:|---:|
| Chromium | 1579.8, 1598.8, 1656.4 | 1598.8 | 79.7, 74.2, 74.1 | 74.2 |
| Firefox | 12625, 12645, 12632 | 12632 | 566, 535, 520 | 535 |

Policy explored-state counts fell from `[1279, 1615, 25000, 25000]`
(52,894 total, all bounded) to `[32, 40, 1408, 1664]` (3,144 total, none
bounded). The optimized package's structural-only medians were 4.9ms in
Chromium and 16ms in Firefox, locating the remaining time in playability rather
than parsing, schema, references, or serialization.

An earlier harness setup run included two non-YAML metadata files. Those
numbers are invalid evidence and are intentionally omitted from the comparison.

## Maintained story

| engine | parent samples (ms) | parent median | optimized samples (ms) | optimized median |
|---|---:|---:|---:|---:|
| Chromium | 9537.7, 9468.3, 9469.6 | 9469.6 | 9713.2, 9652.5, 9655.0 | 9655.0 |
| Firefox | 80025, 79995, 80164 | 80025 | 77635, 78322, 77797 | 77797 |

Both versions explored `[25000, 25000, 25000, 25000]`: the maintained story's
100,000-state cost is genuine combinatorics unrelated to redundant clock loops.
The optimization is neutral within normal run variance there. Structural-only
medians were 26.4ms in Chromium and 159ms in Firefox, so the state-space search
accounts for more than 99.7% of total maintained-story time.

## Conclusion

The validator does perform an inherent bounded state-space search. The reported
small fixture exposed an addressable representation problem inside it: after
every authored absolute-time boundary had passed, reversible route loops made
otherwise identical states distinct solely because their raw elapsed minute
differed. The search now canonicalizes those elapsed values while preserving
pending triggers by remaining duration and retaining the Pareto frontier of
action/elapsed costs. It still evaluates all four notebook policies, all
actions, predicates, timers, terminal paths, and resource bounds.

The maintained story shows the limit of this change: it still reaches the
25,000-state cap in every policy and needs separate evidence before any further
optimization.

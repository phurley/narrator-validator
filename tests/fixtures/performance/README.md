# Validation performance fixtures

`author-save-two-question.json` is the exact in-memory repository snapshot from
`narrator-author`'s Firefox save incident (`narrator-author#337`): the shared
`TEST_FILES` fixture after the `solve-contract` route and authoring edits have
created two Solve questions with three answer cards. The fixture source was
unchanged between the measured fix commit `382030c` and extraction. It contains
the exact YAML-only projection passed by `workspaceEditing.tsx`; non-YAML
repository metadata is deliberately excluded before validation.

The browser profiler keeps wall-clock measurements out of the test suite while
still failing loudly if the validator stops doing the expected four notebook
policy searches or reports missing/zero explored-state counts:

```sh
node scripts/profile-wasm-validation.mjs \
  --fixture tests/fixtures/performance/author-save-two-question.json \
  --playwright ../narrator-author/node_modules/playwright
```

`--fixture` also accepts a maintained story directory; the profiler reads its
root YAML files in sorted order. Record that checkout's exact revision with the
timing result.

Build `pkg/` first with `scripts/build-web-package.mjs`. Use the workspace box
claim and record system load for comparable measurements.

Use `--allow-missing-structural` only to profile a historical package from
before the structural-baseline export existed.

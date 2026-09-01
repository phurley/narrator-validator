import assert from 'node:assert/strict'
import { readFile, readdir } from 'node:fs/promises'

import {
  STANDARD_MYSTERY_RULESET,
  STANDARD_MYSTERY_RULESETS,
  VALIDATOR_SOURCE_COMMIT,
  endStateContractMetadata,
  initializeNarratorValidator,
  parseReferenceText,
  referenceTextMetadata,
  solutionAnswerMatches,
  solutionContractMetadata,
  validateRepository,
  validateRepositoryWithFeatures,
  validateRepositoryWithoutPlayabilityWithFeatures,
} from '../pkg/index.js'

const wasm = await readFile(
  new URL('../pkg/narrator_validator_bg.wasm', import.meta.url),
)
const manifest = JSON.parse(
  await readFile(new URL('../pkg/package.json', import.meta.url), 'utf8'),
)
await initializeNarratorValidator(wasm)

assert.equal(manifest.narratorValidatorSource.commit, VALIDATOR_SOURCE_COMMIT)
assert.match(VALIDATOR_SOURCE_COMMIT, /^[0-9a-f]{40}$/)

assert.deepEqual(
  STANDARD_MYSTERY_RULESETS.map((ruleset) => ruleset.version),
  ['1.0.0', '2.0.0', '3.0.0', '4.0.0', '5.0.0', '6.0.0', '7.0.0'],
)
assert.equal(STANDARD_MYSTERY_RULESET.version, '7.0.0')

// Only ruleset.standard_mystery@7.0.0 defines an answer-deck catalog
// (Story Format 3.7); earlier versions carry no `answers` field at all.
for (const ruleset of STANDARD_MYSTERY_RULESETS.slice(0, -1)) {
  assert.equal(ruleset.answers, undefined)
}
assert.equal(STANDARD_MYSTERY_RULESET.answers.length, 29)
assert.deepEqual(STANDARD_MYSTERY_RULESET.answers[0], {
  id: 'answer.motive.greed',
  tag_id: 2112,
  name: 'Greed',
  description:
    'Done for money, property, a payout, an inheritance, or the value of the thing itself.',
})
assert.deepEqual(STANDARD_MYSTERY_RULESET.answers[28], {
  id: 'answer.method.not_killed',
  tag_id: 2084,
  name: 'Not killed by anyone',
  description:
    'Illness, a failing heart, or a death nothing external caused — including the case where nobody died at all.',
})
assert.ok(
  STANDARD_MYSTERY_RULESET.answers.every(
    (card) => card.tag_id >= 2084 && card.tag_id <= 2112,
  ),
)
assert.ok(
  STANDARD_MYSTERY_RULESET.commands.some(
    (command) => command.id === 'command.claim',
  ),
)
assert.deepEqual(STANDARD_MYSTERY_RULESET.command_capabilities, [
  {
    command_id: 'command.claim',
    mechanic: 'claim_fact',
    enabled_when: 'manual_facts',
  },
  {
    command_id: 'command.deduce',
    mechanic: 'establish_deduction',
    enabled_when: 'manual_deductions',
  },
  {
    command_id: 'command.reconcile',
    mechanic: 'reconcile_notebooks',
    enabled_when: 'multiple_players_with_unshared_facts',
  },
  {
    command_id: 'command.solve',
    mechanic: 'submit_solution',
    enabled_when: 'always',
  },
])
assert.deepEqual(
  STANDARD_MYSTERY_RULESET.commands.find(
    (command) => command.id === 'command.reconcile',
  ),
  {
    id: 'command.reconcile',
    name: 'Reconcile',
    description: 'Compare claimed notebook facts with every joined player.',
  },
)
assert.equal(
  STANDARD_MYSTERY_RULESET.commands.find(
    (command) => command.id === 'command.solve',
  ).parameters,
  undefined,
)
assert.deepEqual(
  STANDARD_MYSTERY_RULESETS[1].commands
    .find((command) => command.id === 'command.solve')
    .parameters.map((parameter) => parameter.name),
  ['suspect', 'theory'],
)

const report = await validateRepository([])

assert.equal(report.valid, false)
assert.equal(report.validator_version, manifest.version)
assert.ok(
  report.diagnostics.some(
    (diagnostic) => diagnostic.code === 'schema.missing_section',
  ),
)

const solutionContract = await solutionContractMetadata()
assert.deepEqual(solutionContract, {
  story_format_version: '3.3.0',
  ruleset_id: 'ruleset.standard_mystery',
  ruleset_version: '3.0.0',
  compatible_ruleset_versions: ['3.0.0', '4.0.0', '5.0.0', '6.0.0'],
  min_questions: 1,
  max_questions: 4,
  min_answer_cards: 1,
  max_answer_cards: 5,
  ordered_default: false,
  prompt_disclosure: 'player_safe',
  expected_answer_disclosure: 'private_narrator',
})
const endStateContract = await endStateContractMetadata()
assert.deepEqual(endStateContract, {
  story_format_version: '3.4.0',
  canonical_section: 'end_states',
  canonical_file: 'end_states.yaml',
  legacy_section: 'win_states',
  precedence: 'authored_order_first_satisfied',
  evaluation_timing: 'after_every_resolved_turn',
  score_semantics: 'snapshot_and_minimum_gate',
  legacy_outcome: 'won',
  legacy_resolution: 'full',
  legal_outcome_resolutions: [
    { outcome: 'won', resolutions: ['full', 'partial'] },
    { outcome: 'lost', resolutions: ['failure'] },
  ],
})
assert.equal(
  await solutionAnswerMatches(
    ['entity.knife', 'entity.bottle'],
    ['entity.bottle', 'entity.knife'],
    false,
  ),
  true,
)
assert.equal(
  await solutionAnswerMatches(
    ['setting.shed', 'setting.observatory'],
    ['setting.observatory', 'setting.shed'],
    true,
  ),
  false,
)

const solveFixtureRoot = new URL(
  '../tests/fixtures/format-3.3-solve-card-sets/',
  import.meta.url,
)
const solveFixtureFiles = await Promise.all(
  (await readdir(solveFixtureRoot)).map(async (path) => ({
    path,
    source: await readFile(new URL(path, solveFixtureRoot), 'utf8'),
  })),
)
const solveReport = await validateRepositoryWithFeatures(solveFixtureFiles, [
  'reference_text_v1',
])
const solveStructuralReport =
  await validateRepositoryWithoutPlayabilityWithFeatures(solveFixtureFiles, [
    'reference_text_v1',
  ])
assert.equal(solveStructuralReport.playability, undefined)
assert.deepEqual(solveStructuralReport.diagnostics, solveReport.diagnostics)
assert.deepEqual(solveStructuralReport.reference_text, solveReport.reference_text)
assert.equal(solveReport.valid, true)
assert.equal(solveReport.format_version, '3.3.0')
assert.equal(
  solveReport.reference_text.find(
    (field) => field.pointer === '/solution/questions/0/prompt',
  ).resolved,
  'Who planned the crime against Rowan Vale?',
)
assert.equal(
  solveReport.reference_text.some((field) =>
    field.pointer.includes('/answer'),
  ),
  false,
)
const solveTerminal = solveReport.playability.terminal_paths.find(
  ({ id }) => id === 'win.solve_case',
)
assert.equal(solveTerminal.status, 'proved')
assert.equal(solveTerminal.lower_bound.action_count, 1)
assert.equal(
  solveTerminal.lower_bound.ordered_steps[0].action,
  'command.solve [character.culprit] [setting.study entity.knife]',
)
assert.ok(solveTerminal.lower_bound.pivotal_unlocks.includes('solution.correct'))

const ruleset5Files = solveFixtureFiles.map((file) => {
  if (file.path === 'case.yaml') {
    return {
      ...file,
      source: file.source
        .replace('format_version: "3.3.0"', 'format_version: "3.4.0"')
        .replace('version: "3.0.0"', 'version: "5.0.0"'),
    }
  }
  if (file.path === 'flags.yaml') {
    return {
      ...file,
      source:
        'flags:\n  - id: flag.solution_ready\n    name: Solution ready\n    description: The prerequisites for the complete solution are met.\n    initial_state: false\n',
    }
  }
  if (file.path === 'win_states.yaml') {
    return {
      path: 'end_states.yaml',
      source: file.source
        .replace('win_states:', 'end_states:')
        .replace(
          '    text: You explain the complete solution.',
          '    outcome: won\n    resolution: full\n    requires: [flag.solution_ready]\n    text: You explain the complete solution.',
        ),
    }
  }
  return file
})
const ruleset5Report = await validateRepositoryWithFeatures(ruleset5Files, [
  'reference_text_v1',
])
assert.equal(ruleset5Report.valid, true)

const invalidRuleset5Report = await validateRepositoryWithFeatures(
  ruleset5Files.map((file) =>
    file.path === 'end_states.yaml'
      ? {
          ...file,
          source: file.source.replace(
            '    requires: [flag.solution_ready]\n',
            '    requires: [flag.solution_ready]\n    minimum_points: 0\n',
          ),
        }
      : file,
  ),
  ['reference_text_v1'],
)
assert.equal(invalidRuleset5Report.valid, false)
assert.ok(
  invalidRuleset5Report.diagnostics.some(
    ({ code, pointer }) =>
      code === 'end_states.solution_condition_conflict' &&
      pointer === '/end_states/0/minimum_points',
  ),
)

const playabilityFixtureRoot = new URL(
  '../tests/fixtures/playability-analysis/',
  import.meta.url,
)
const playabilityFiles = await Promise.all(
  (await readdir(playabilityFixtureRoot)).map(async (path) => ({
    path,
    source: await readFile(new URL(path, playabilityFixtureRoot), 'utf8'),
  })),
)
const playabilityReport = await validateRepository(playabilityFiles)
assert.equal(playabilityReport.valid, true)
assert.deepEqual(
  playabilityReport.playability.terminal_paths.map(({ id, status }) => ({
    id,
    status,
  })),
  [
    { id: 'end.delayed', status: 'proved' },
    { id: 'end.proved', status: 'proved' },
    { id: 'end.missing_action', status: 'not_proved' },
  ],
)
assert.equal(
  playabilityReport.playability.terminal_paths[0].lower_bound.required_waits[0]
    .delay_minutes,
  20,
)
assert.equal(playabilityReport.playability.notebook_policies.length, 4)
assert.deepEqual(playabilityReport.playability.deduction_graph, {
  maximum_depth: 1,
  largest_cascade_size: 1,
  largest_cascade_root: 'fact.sample_matches',
  largest_cascade: ['deduction.solution'],
})

const metadata = await referenceTextMetadata()
assert.deepEqual(metadata.supported_features, ['reference_text_v1'])
assert.ok(
  metadata.reference_kinds.some(
    (kind) => kind.kind === 'character' && kind.default_path === 'name',
  ),
)

const parsedText = await parseReferenceText('é [[character.echo.name]]!')
assert.equal(parsedText.status, 'parsed')
assert.deepEqual(parsedText.value.segments, [
  { type: 'literal', text: 'é ' },
  {
    type: 'reference',
    expression: {
      authored: 'character.echo.name',
      target_id: 'character.echo',
      property_path: ['name'],
      start: 3,
      end: 26,
    },
  },
  { type: 'literal', text: '!' },
])

const escapedText = await parseReferenceText(
  String.raw`literal \[[character.echo]]`,
)
assert.equal(escapedText.status, 'parsed')
assert.deepEqual(escapedText.value.segments, [
  { type: 'literal', text: 'literal [[character.echo]]' },
])

const parseError = await parseReferenceText('é [[character.echo')
assert.deepEqual(parseError, {
  status: 'error',
  error: { type: 'unclosed', start: 3 },
})

const featureFiles = [
  {
    path: 'case.yaml',
    source:
      'case:\n  id: case.smoke\n  format_version: "3.2.0"\n  features: [reference_text_v1]\n  players: { min: 1, max: 1 }\n  initial_time: "20:00"\n  opening: "Hello, [[character.echo]]."\n',
  },
  {
    path: 'characters.yaml',
    source:
      'characters:\n  - id: character.echo\n    name: Echo Vale\n    description: A composed witness.\n',
  },
]
const unsupportedFeatureReport = await validateRepositoryWithFeatures(
  featureFiles,
  [],
)
assert.equal(unsupportedFeatureReport.reference_text, undefined)
assert.deepEqual(unsupportedFeatureReport.features, ['reference_text_v1'])
assert.equal(
  unsupportedFeatureReport.diagnostics[0].code,
  'feature.consumer_unsupported',
)

const supportedFeatureReport = await validateRepositoryWithFeatures(
  featureFiles,
  ['reference_text_v1'],
)
assert.deepEqual(supportedFeatureReport.features, ['reference_text_v1'])
assert.equal(supportedFeatureReport.reference_text.length, 1)
assert.equal(supportedFeatureReport.reference_text[0].resolved, 'Hello, Echo Vale.')
assert.equal(
  supportedFeatureReport.reference_text[0].provenance[0].expression.target_id,
  'character.echo',
)
assert.ok(supportedFeatureReport.reference_text[0].provenance[0].range)

const entityReport = await validateRepository([
  {
    path: 'entities.yaml',
    source:
      'entities:\n  - id: entity.pistol\n    physical:\n      portable: sometimes\n',
  },
])
assert.ok(
  entityReport.diagnostics.some(
    (diagnostic) =>
      diagnostic.code === 'entity.portable_type' &&
      diagnostic.pointer === '/entities/0/physical/portable',
  ),
)

const dropReport = await validateRepository([
  {
    path: 'case.yaml',
    source:
      'case:\n  id: case.smoke\n  format_version: "3.0.0"\n  title: Smoke test\n  initial_time: "21:00"\n  players:\n    min: 1\n    max: 4\n',
  },
  {
    path: 'commands.yaml',
    source:
      'commands:\n  - id: command.drop\n    name: Drop\n    parameters:\n      - name: item\n        type: entity\n        required: false\n',
  },
])
assert.ok(
  dropReport.diagnostics.some(
    (diagnostic) =>
      diagnostic.code === 'command.runtime_signature' &&
      diagnostic.pointer === '/commands/0/parameters/0/required' &&
      diagnostic.subject_id === 'command.drop',
  ),
)

console.log('browser package smoke test passed')

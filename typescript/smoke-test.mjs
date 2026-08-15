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
  ['1.0.0', '2.0.0', '3.0.0'],
)
assert.equal(STANDARD_MYSTERY_RULESET.version, '3.0.0')
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

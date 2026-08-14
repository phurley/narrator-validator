import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'

import {
  initializeNarratorValidator,
  referenceTextMetadata,
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

const report = await validateRepository([])

assert.equal(report.valid, false)
assert.equal(report.validator_version, manifest.version)
assert.ok(
  report.diagnostics.some(
    (diagnostic) => diagnostic.code === 'schema.missing_section',
  ),
)

const metadata = await referenceTextMetadata()
assert.deepEqual(metadata.supported_features, ['reference_text_v1'])
assert.ok(
  metadata.reference_kinds.some(
    (kind) => kind.kind === 'character' && kind.default_path === 'name',
  ),
)

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

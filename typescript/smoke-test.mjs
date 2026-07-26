import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'

import {
  initializeNarratorValidator,
  validateRepository,
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

console.log('browser package smoke test passed')

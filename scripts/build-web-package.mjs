#!/usr/bin/env node

import { execFileSync } from 'node:child_process'
import { copyFile, mkdir, readFile, writeFile } from 'node:fs/promises'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = dirname(dirname(fileURLToPath(import.meta.url)))
const output = join(root, 'pkg')
const wasmBindgen =
  process.env.NARRATOR_WASM_BINDGEN ?? 'wasm-bindgen'
const explicitSourceCommit = process.env.NARRATOR_VALIDATOR_SOURCE_COMMIT
const dirty = execFileSync('git', ['status', '--porcelain', '--untracked-files=all'], {
  cwd: root,
  encoding: 'utf8',
}).trim()
if (explicitSourceCommit === undefined && dirty !== '') {
  throw new Error(
    'validator worktree is dirty; commit the source or set NARRATOR_VALIDATOR_SOURCE_COMMIT to the exact source commit',
  )
}
const sourceCommit =
  explicitSourceCommit ??
  execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).trim()
if (!/^[0-9a-f]{40}$/.test(sourceCommit)) {
  throw new Error('NARRATOR_VALIDATOR_SOURCE_COMMIT must be a full lowercase Git commit SHA')
}

execFileSync(
  'cargo',
  [
    'build',
    '--release',
    '--target',
    'wasm32-unknown-unknown',
    '--features',
    'wasm',
    '--lib',
  ],
  {
    cwd: root,
    stdio: 'inherit',
  },
)

await mkdir(output, { recursive: true })

execFileSync(
  wasmBindgen,
  [
    '--target',
    'web',
    '--out-dir',
    output,
    '--out-name',
    'narrator_validator',
    join(
      root,
      'target',
      'wasm32-unknown-unknown',
      'release',
      'narrator_validator.wasm',
    ),
  ],
  {
    cwd: root,
    stdio: 'inherit',
  },
)

const [indexJavaScript, indexTypes] = await Promise.all([
  readFile(join(root, 'typescript', 'index.js'), 'utf8'),
  readFile(join(root, 'typescript', 'index.d.ts'), 'utf8'),
])
await Promise.all([
  writeFile(
    join(output, 'index.js'),
    indexJavaScript.replace('__NARRATOR_VALIDATOR_SOURCE_COMMIT__', sourceCommit),
  ),
  writeFile(
    join(output, 'index.d.ts'),
    indexTypes.replace('__NARRATOR_VALIDATOR_SOURCE_COMMIT__', sourceCommit),
  ),
])

const standardMysteryRulesets = ['1.0.0', '2.0.0', '3.0.0', '4.0.0', '5.0.0'].map((version) =>
  JSON.parse(
    execFileSync(
      'cargo',
      ['run', '--quiet', '--bin', 'export_ruleset', '--', version],
      { cwd: root, encoding: 'utf8' },
    ).trim(),
  ),
)
await writeFile(
  join(output, 'rulesets.js'),
  `export const STANDARD_MYSTERY_RULESETS = Object.freeze(${JSON.stringify(standardMysteryRulesets)}.map(Object.freeze))\nexport const STANDARD_MYSTERY_RULESET = STANDARD_MYSTERY_RULESETS.at(-1)\n`,
)

const metadata = JSON.parse(
  execFileSync(
    'cargo',
    ['metadata', '--no-deps', '--format-version', '1'],
    {
      cwd: root,
      encoding: 'utf8',
    },
  ),
)
const rustPackage = metadata.packages.find(
  (candidate) => candidate.manifest_path === join(root, 'Cargo.toml'),
)
if (rustPackage === undefined) {
  throw new Error('could not find narrator-validator in cargo metadata')
}

const manifest = {
  name: rustPackage.name,
  version: rustPackage.version,
  description: rustPackage.description,
  license: rustPackage.license,
  repository: rustPackage.repository,
  narratorValidatorSource: {
    repository: rustPackage.repository,
    commit: sourceCommit,
  },
  type: 'module',
  main: 'index.js',
  module: 'index.js',
  types: 'index.d.ts',
  sideEffects: false,
  files: [
    'index.js',
    'index.d.ts',
    'rulesets.js',
    'narrator_validator.js',
    'narrator_validator.d.ts',
    'narrator_validator_bg.wasm',
  ],
  exports: {
    '.': {
      types: './index.d.ts',
      import: './index.js',
    },
    './raw': {
      types: './narrator_validator.d.ts',
      import: './narrator_validator.js',
    },
    './rulesets': {
      types: './index.d.ts',
      import: './rulesets.js',
    },
  },
}

await writeFile(
  join(output, 'package.json'),
  `${JSON.stringify(manifest, null, 2)}\n`,
)

console.log(`Built browser package at ${output}`)

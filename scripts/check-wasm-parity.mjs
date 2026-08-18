#!/usr/bin/env node

import { execFileSync } from 'node:child_process'
import { readFile, readdir } from 'node:fs/promises'
import { basename, dirname, join, resolve } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

const [storyArgument, packageArgument, nativeReportArgument] = process.argv.slice(2)
if (!storyArgument || !packageArgument || !nativeReportArgument) {
  throw new Error('usage: check-wasm-parity.mjs STORY PACKAGE NATIVE_REPORT')
}
const story = resolve(storyArgument)
const validatorPackage = resolve(packageArgument)
const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)))

const files = await Promise.all(
  (await readdir(story)).filter((path) => path.endsWith('.yaml')).sort().map(
    async (path) => ({ path, source: await readFile(join(story, path), 'utf8') }),
  ),
)
const validator = await import(pathToFileURL(join(validatorPackage, 'index.js')))
const manifest = JSON.parse(await readFile(join(validatorPackage, 'package.json'), 'utf8'))
await validator.initializeNarratorValidator(
  await readFile(join(validatorPackage, 'narrator_validator_bg.wasm')),
)

const cargoToml = await readFile(join(repoRoot, 'Cargo.toml'), 'utf8')
const expectedVersion = cargoToml.match(/^version\s*=\s*"([^"]+)"/m)?.[1]
if (!expectedVersion) throw new Error('could not determine expected version from Cargo.toml')
if (manifest.version !== expectedVersion) {
  throw new Error(`expected validator ${expectedVersion}, got ${manifest.version}`)
}
const expectedCommit =
  process.env.NARRATOR_VALIDATOR_SOURCE_COMMIT ??
  execFileSync('git', ['rev-parse', 'HEAD'], { cwd: repoRoot, encoding: 'utf8' }).trim()
if (validator.VALIDATOR_SOURCE_COMMIT !== expectedCommit) {
  throw new Error(
    `expected validator source ${expectedCommit}, got ${validator.VALIDATOR_SOURCE_COMMIT}`,
  )
}

const [wasmReport, nativeReport] = await Promise.all([
  validator.validateRepository(files),
  readFile(resolve(nativeReportArgument), 'utf8').then(JSON.parse),
])
if (JSON.stringify(wasmReport) !== JSON.stringify(nativeReport)) {
  throw new Error('native and WASM validation reports differ')
}
if (!wasmReport.valid || wasmReport.diagnostics.length !== 0) {
  throw new Error(`story ${basename(story)} is not clean under validator ${expectedVersion}`)
}
console.log(`native/WASM parity passed for ${basename(story)} at ${validator.VALIDATOR_SOURCE_COMMIT}`)

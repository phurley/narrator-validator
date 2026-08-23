#!/usr/bin/env node

import { createServer } from 'node:http'
import { readFile, readdir, stat } from 'node:fs/promises'
import { loadavg } from 'node:os'
import { basename, dirname, join, resolve, sep } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

const root = dirname(dirname(fileURLToPath(import.meta.url)))
const options = parseArguments(process.argv.slice(2))
const packageRoot = resolve(options.packageRoot ?? join(root, 'pkg'))
const fixturePath = resolve(options.fixturePath)
const playwrightRoot = resolve(options.playwrightRoot)
const fixture = await loadFixture(fixturePath)

if (!Array.isArray(fixture) || fixture.length === 0) {
  throw new Error('fixture must be a non-empty JSON array of source files')
}

const playwright = await import(pathToFileURL(join(playwrightRoot, 'index.mjs')).href)
const server = createServer(async (request, response) => {
  try {
    if (request.url === '/') {
      response.writeHead(200, { 'content-type': 'text/html; charset=utf-8' })
      response.end(profilePage())
      return
    }
    if (request.url === '/fixture.json') {
      response.writeHead(200, { 'content-type': 'application/json' })
      response.end(JSON.stringify(fixture))
      return
    }
    if (request.url?.startsWith('/validator/')) {
      const relative = request.url.slice('/validator/'.length)
      const path = resolve(packageRoot, relative)
      if (!path.startsWith(`${packageRoot}${sep}`)) {
        response.writeHead(403).end()
        return
      }
      const content = await readFile(path)
      response.writeHead(200, { 'content-type': contentType(path) })
      response.end(content)
      return
    }
    response.writeHead(404).end()
  } catch (error) {
    if (!response.headersSent) {
      response.writeHead(500, { 'content-type': 'text/plain; charset=utf-8' })
      response.end(error instanceof Error ? error.stack : String(error))
    } else {
      response.destroy(error instanceof Error ? error : new Error(String(error)))
    }
  }
})

await new Promise((resolveListening) => server.listen(0, '127.0.0.1', resolveListening))
const address = server.address()
if (address === null || typeof address === 'string') throw new Error('profile server did not bind')

const results = []
try {
  for (const engineName of options.engines) {
    const engine = playwright[engineName]
    if (engine === undefined) throw new Error(`Playwright does not provide engine ${engineName}`)
    const browser = await engine.launch({ headless: true })
    try {
      const page = await browser.newPage()
      const pageErrors = []
      page.on('pageerror', (error) => pageErrors.push(error.message))
      await page.goto(`http://127.0.0.1:${address.port}/`)
      await page.waitForFunction(() => typeof globalThis.runProfile === 'function').catch((error) => {
        throw new Error(`${error.message}; page errors: ${pageErrors.join('; ')}`)
      })
      const result = await page.evaluate(
        ({ iterations, requireStructural }) =>
          globalThis.runProfile(iterations, requireStructural),
        { iterations: options.iterations, requireStructural: !options.allowMissingStructural },
      )
      assertProfileResult(result, engineName)
      results.push({ engine: engineName, ...result })
    } finally {
      await browser.close()
    }
  }
} finally {
  await new Promise((resolveClosed, reject) =>
    server.close((error) => (error === undefined ? resolveClosed() : reject(error))),
  )
}

process.stdout.write(
  `${JSON.stringify(
    {
      fixture: basename(fixturePath),
      source_file_count: fixture.length,
      iterations: options.iterations,
      load_average: loadavg(),
      results,
    },
    null,
    2,
  )}\n`,
)

function parseArguments(arguments_) {
  const parsed = {
    engines: ['chromium', 'firefox'],
    iterations: 3,
    fixturePath: undefined,
    packageRoot: undefined,
    playwrightRoot: process.env.NARRATOR_PLAYWRIGHT_ROOT,
    allowMissingStructural: false,
  }
  for (let index = 0; index < arguments_.length; index += 1) {
    const argument = arguments_[index]
    const value = arguments_[index + 1]
    if (argument === '--allow-missing-structural') {
      parsed.allowMissingStructural = true
      continue
    }
    if (argument === '--fixture') parsed.fixturePath = value
    else if (argument === '--package') parsed.packageRoot = value
    else if (argument === '--playwright') parsed.playwrightRoot = value
    else if (argument === '--engines') parsed.engines = value.split(',')
    else if (argument === '--iterations') parsed.iterations = Number(value)
    else throw new Error(`unknown argument: ${argument}`)
    index += 1
  }
  if (parsed.fixturePath === undefined) throw new Error('--fixture is required')
  if (parsed.playwrightRoot === undefined) {
    throw new Error('--playwright or NARRATOR_PLAYWRIGHT_ROOT is required')
  }
  if (!Number.isInteger(parsed.iterations) || parsed.iterations < 1) {
    throw new Error('--iterations must be a positive integer')
  }
  if (parsed.engines.length === 0 || parsed.engines.some((engine) => engine === '')) {
    throw new Error('--engines must name at least one Playwright engine')
  }
  return parsed
}

function assertProfileResult(result, engine) {
  if (!Array.isArray(result.samples_ms) || result.samples_ms.length !== options.iterations) {
    throw new Error(`${engine} did not execute every requested sample`)
  }
  if (result.valid !== true) {
    throw new Error(`${engine} fixture is invalid (${result.diagnostic_count} diagnostics)`)
  }
  if (
    result.structural_samples_ms !== null &&
    (!Array.isArray(result.structural_samples_ms) ||
      result.structural_samples_ms.length !== options.iterations)
  ) {
    throw new Error(`${engine} did not execute every structural-baseline sample`)
  }
  if (result.policy_count !== 4) {
    throw new Error(`${engine} analyzed ${result.policy_count} notebook policies; expected 4`)
  }
  if (
    !Array.isArray(result.explored_states_by_policy) ||
    result.explored_states_by_policy.length !== 4 ||
    result.explored_states_by_policy.some((count) => !Number.isInteger(count) || count < 1)
  ) {
    throw new Error(`${engine} returned missing or zero playability work counts`)
  }
  const counted = result.explored_states_by_policy.reduce((sum, count) => sum + count, 0)
  if (counted !== result.total_explored_states) {
    throw new Error(`${engine} total explored-state count does not match its policy counts`)
  }
}

function contentType(path) {
  if (path.endsWith('.js')) return 'text/javascript; charset=utf-8'
  if (path.endsWith('.wasm')) return 'application/wasm'
  if (path.endsWith('.json')) return 'application/json'
  return 'application/octet-stream'
}

async function loadFixture(path) {
  if ((await stat(path)).isDirectory()) {
    const names = (await readdir(path))
      .filter((name) => name.endsWith('.yaml') || name.endsWith('.yml'))
      .sort()
    return Promise.all(
      names.map(async (name) => ({ path: name, source: await readFile(join(path, name), 'utf8') })),
    )
  }
  return JSON.parse(await readFile(path, 'utf8'))
}

function profilePage() {
  return `<!doctype html>
<meta charset="utf-8">
<script type="module">
  import * as validator from '/validator/index.js'

  const files = await fetch('/fixture.json').then((response) => response.json())
  const metadata = await validator.referenceTextMetadata()

  globalThis.runProfile = async (iterations, requireStructural) => {
    const structuralValidator =
      validator.validateRepositoryWithoutPlayabilityWithFeatures
    if (requireStructural && typeof structuralValidator !== 'function') {
      throw new Error('validator package does not expose the structural baseline')
    }
    let structural_samples_ms = null
    if (typeof structuralValidator === 'function') {
      await structuralValidator(files, metadata.supported_features)
      structural_samples_ms = []
      for (let index = 0; index < iterations; index += 1) {
        const started = performance.now()
        const structural = await structuralValidator(files, metadata.supported_features)
        if (structural.playability != null) throw new Error('structural baseline ran playability')
        structural_samples_ms.push(performance.now() - started)
      }
    }
    await validator.validateRepositoryWithFeatures(files, metadata.supported_features)
    const samples_ms = []
    let report
    for (let index = 0; index < iterations; index += 1) {
      const started = performance.now()
      report = await validator.validateRepositoryWithFeatures(files, metadata.supported_features)
      samples_ms.push(performance.now() - started)
    }
    const policies = report?.playability?.notebook_policies ?? []
    const explored_states_by_policy = policies.map((policy) => policy.explored_states)
    return {
      valid: report.valid,
      diagnostic_count: report.diagnostics.length,
      samples_ms,
      median_ms: [...samples_ms].sort((left, right) => left - right)[Math.floor(samples_ms.length / 2)],
      structural_samples_ms,
      structural_median_ms: structural_samples_ms === null
        ? null
        : [...structural_samples_ms].sort((left, right) => left - right)[Math.floor(structural_samples_ms.length / 2)],
      policy_count: policies.length,
      explored_states_by_policy,
      total_explored_states: explored_states_by_policy.reduce((sum, count) => sum + count, 0),
      bounded_by_policy: policies.map((policy) => policy.bounded),
    }
  }
</script>`
}

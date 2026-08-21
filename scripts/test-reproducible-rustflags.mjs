#!/usr/bin/env node

import assert from 'node:assert/strict'

import {
  REPRODUCIBLE_CARGO_GIT_PREFIX,
  REPRODUCIBLE_CARGO_REGISTRY_PREFIX,
  REPRODUCIBLE_SOURCE_PREFIX,
  buildReproducibleRustflags,
} from './reproducible-rustflags.mjs'

const separator = '\x1f'
const expectedCases = 7
let completedCases = 0

function check(name, assertion) {
  assertion()
  completedCases += 1
  process.stderr.write(`ok ${completedCases} - ${name}\n`)
}

function expectedRemaps(sourceRoot, cargoHome) {
  return [
    `--remap-path-prefix=${cargoHome}/registry/src=${REPRODUCIBLE_CARGO_REGISTRY_PREFIX}`,
    `--remap-path-prefix=${cargoHome}/git/checkouts=${REPRODUCIBLE_CARGO_GIT_PREFIX}`,
    `--remap-path-prefix=${sourceRoot}=${REPRODUCIBLE_SOURCE_PREFIX}`,
  ]
}

function applyRemaps(sourcePath, flags) {
  let remapped = sourcePath
  for (const flag of flags) {
    const mapping = flag.slice('--remap-path-prefix='.length)
    const separatorIndex = mapping.lastIndexOf('=')
    const from = mapping.slice(0, separatorIndex)
    const to = mapping.slice(separatorIndex + 1)
    if (sourcePath.startsWith(from)) {
      remapped = `${to}${sourcePath.slice(from.length)}`
    }
  }
  return remapped
}

check('different checkout and Cargo roots produce the same virtual prefixes', () => {
  assert.equal(REPRODUCIBLE_SOURCE_PREFIX, '/virtual/narrator-validator')
  assert.equal(REPRODUCIBLE_CARGO_REGISTRY_PREFIX, '/virtual/cargo/registry/src')
  assert.equal(REPRODUCIBLE_CARGO_GIT_PREFIX, '/virtual/cargo/git/checkouts')

  const first = buildReproducibleRustflags({
    sourceRoot: '/tmp/first/narrator-validator',
    cargoHome: '/tmp/first/cargo',
  }).split(separator)
  const second = buildReproducibleRustflags({
    sourceRoot: '/opt/second/narrator-validator',
    cargoHome: '/opt/second/cargo',
  }).split(separator)

  assert.deepEqual(first, expectedRemaps('/tmp/first/narrator-validator', '/tmp/first/cargo'))
  assert.deepEqual(second, expectedRemaps('/opt/second/narrator-validator', '/opt/second/cargo'))
  assert.deepEqual(
    first.map((flag) => flag.slice(flag.indexOf('=/', '--remap-path-prefix='.length) + 1)),
    second.map((flag) => flag.slice(flag.indexOf('=/', '--remap-path-prefix='.length) + 1)),
  )
})

check('relative CARGO_HOME follows the Cargo build cwd, not the Node caller cwd', () => {
  const originalCwd = process.cwd()
  process.chdir('/')
  try {
    const flags = buildReproducibleRustflags({
      sourceRoot: '/workspace/narrator-validator',
      cargoHome: '.cargo',
    }).split(separator)

    assert.deepEqual(
      flags,
      expectedRemaps(
        '/workspace/narrator-validator',
        '/workspace/narrator-validator/.cargo',
      ),
    )
  } finally {
    process.chdir(originalCwd)
  }
})

check('source remap wins when the checkout is nested under Cargo git checkouts', () => {
  const sourceRoot = '/cargo/git/checkouts/narrator-validator/checkout'
  const flags = buildReproducibleRustflags({
    sourceRoot,
    cargoHome: '/cargo',
  }).split(separator)

  assert.equal(
    applyRemaps(`${sourceRoot}/src/lib.rs`, flags),
    '/virtual/narrator-validator/src/lib.rs',
  )
  assert.equal(flags.at(-1), `--remap-path-prefix=${sourceRoot}=${REPRODUCIBLE_SOURCE_PREFIX}`)
})

check('encoded caller flags are preserved as individual rustc arguments', () => {
  const flags = buildReproducibleRustflags({
    sourceRoot: '/src/validator',
    cargoHome: '/cargo',
    environment: {
      CARGO_ENCODED_RUSTFLAGS: ['-C', 'debuginfo=0'].join(separator),
    },
  }).split(separator)

  assert.deepEqual(flags.slice(0, 2), ['-C', 'debuginfo=0'])
  assert.equal(flags.length, 5)
})

check('plain RUSTFLAGS whitespace semantics are preserved safely', () => {
  const flags = buildReproducibleRustflags({
    sourceRoot: '/src/validator',
    cargoHome: '/cargo',
    environment: { RUSTFLAGS: '  -C   opt-level=s  ' },
  }).split(separator)

  assert.deepEqual(flags.slice(0, 2), ['-C', 'opt-level=s'])
  assert.equal(flags.length, 5)
})

check('ambiguous encoded and plain caller flags are rejected', () => {
  assert.throws(
    () =>
      buildReproducibleRustflags({
        sourceRoot: '/src/validator',
        cargoHome: '/cargo',
        environment: {
          CARGO_ENCODED_RUSTFLAGS: '-C\x1fdebuginfo=0',
          RUSTFLAGS: '-C opt-level=s',
        },
      }),
    /set only one/,
  )
})

check('caller path remaps are rejected', () => {
  assert.throws(
    () =>
      buildReproducibleRustflags({
        sourceRoot: '/src/validator',
        cargoHome: '/cargo',
        environment: {
          CARGO_ENCODED_RUSTFLAGS:
            '--remap-path-prefix=/caller/source=/caller/virtual',
        },
      }),
    /conflicts with reproducible browser packaging/,
  )
})

assert.equal(
  completedCases,
  expectedCases,
  `expected ${expectedCases} reproducible-rustflags cases, completed ${completedCases}`,
)
process.stdout.write(
  `reproducible rustflags contract: ${completedCases}/${expectedCases} cases\n`,
)

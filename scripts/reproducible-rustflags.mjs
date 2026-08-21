import { resolve } from 'node:path'

const encodedFlagSeparator = '\x1f'

export const REPRODUCIBLE_SOURCE_PREFIX = '/virtual/narrator-validator'
export const REPRODUCIBLE_CARGO_REGISTRY_PREFIX = '/virtual/cargo/registry/src'
export const REPRODUCIBLE_CARGO_GIT_PREFIX = '/virtual/cargo/git/checkouts'

function callerFlags(environment) {
  const encoded = environment.CARGO_ENCODED_RUSTFLAGS ?? ''
  const plain = environment.RUSTFLAGS ?? ''

  if (encoded !== '' && plain.trim() !== '') {
    throw new Error(
      'set only one of CARGO_ENCODED_RUSTFLAGS or RUSTFLAGS when building the browser package',
    )
  }

  if (encoded !== '') {
    return encoded.split(encodedFlagSeparator)
  }

  return plain.trim() === '' ? [] : plain.trim().split(/\s+/)
}

export function buildReproducibleRustflags({
  sourceRoot,
  cargoHome,
  environment = {},
}) {
  const existing = callerFlags(environment)

  // Caller remaps would make the final source names depend on flag ordering.
  // Reject them instead of silently producing a package with ambiguous provenance.
  if (
    existing.some(
      (flag) =>
        flag === '--remap-path-prefix' ||
        flag.startsWith('--remap-path-prefix='),
    )
  ) {
    throw new Error(
      'caller-provided --remap-path-prefix conflicts with reproducible browser packaging',
    )
  }

  const absoluteSourceRoot = resolve(sourceRoot)
  const absoluteCargoHome = resolve(cargoHome)
  const reproducibleRemaps = [
    `--remap-path-prefix=${absoluteSourceRoot}=${REPRODUCIBLE_SOURCE_PREFIX}`,
    `--remap-path-prefix=${resolve(absoluteCargoHome, 'registry', 'src')}=${REPRODUCIBLE_CARGO_REGISTRY_PREFIX}`,
    `--remap-path-prefix=${resolve(absoluteCargoHome, 'git', 'checkouts')}=${REPRODUCIBLE_CARGO_GIT_PREFIX}`,
  ]

  return [...existing, ...reproducibleRemaps].join(encodedFlagSeparator)
}

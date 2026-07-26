import initWasm, { validate_json as validateJson } from './narrator_validator.js'

/** @type {Promise<void> | undefined} */
let initialization

/**
 * Load the validator WebAssembly module.
 *
 * Browser callers normally omit `input` and let wasm-bindgen resolve the
 * adjacent `.wasm` asset. Supplying bytes or a compiled module is useful in
 * non-browser runtimes and tests.
 *
 * @param {import('./narrator_validator.js').InitInput | Promise<import('./narrator_validator.js').InitInput>} [input]
 * @returns {Promise<void>}
 */
export function initializeNarratorValidator(input) {
  if (initialization === undefined) {
    const initOptions =
      input === undefined ? undefined : { module_or_path: input }
    const attempt = initWasm(initOptions).then(() => undefined)
    initialization = attempt.catch((error) => {
      initialization = undefined
      throw error
    })
  }
  return initialization
}

/**
 * Validate a complete, in-memory Narrator repository snapshot.
 *
 * The first call loads the WASM module. Subsequent calls reuse it.
 *
 * @param {readonly import('./index.js').SourceFile[]} files
 * @returns {Promise<import('./index.js').ValidationReport>}
 */
export async function validateRepository(files) {
  await initializeNarratorValidator()
  return JSON.parse(validateJson(JSON.stringify(files)))
}

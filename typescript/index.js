import initWasm, {
  parse_reference_text_json as parseReferenceTextJson,
  reference_text_metadata_json_export as referenceTextMetadataJson,
  solution_answer_matches_json as solutionAnswerMatchesJson,
  solution_contract_metadata_json_export as solutionContractMetadataJson,
  validate_json as validateJson,
  validate_json_with_features as validateJsonWithFeatures,
} from './narrator_validator.js'

export { STANDARD_MYSTERY_RULESET, STANDARD_MYSTERY_RULESETS } from './rulesets.js'

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

/**
 * Validate after negotiating the exact capabilities implemented by a consumer.
 * This is the required entry point for runtimes that may support fewer features
 * than the validator itself.
 *
 * @param {readonly import('./index.js').SourceFile[]} files
 * @param {readonly string[]} supportedFeatures
 * @returns {Promise<import('./index.js').ValidationReport>}
 */
export async function validateRepositoryWithFeatures(files, supportedFeatures) {
  await initializeNarratorValidator()
  return JSON.parse(
    validateJsonWithFeatures(
      JSON.stringify(files),
      JSON.stringify(supportedFeatures),
    ),
  )
}

/** @returns {Promise<import('./index.js').ReferenceTextMetadata>} */
export async function referenceTextMetadata() {
  await initializeNarratorValidator()
  return JSON.parse(referenceTextMetadataJson())
}

/** @returns {Promise<import('./index.js').SolutionContractMetadata>} */
export async function solutionContractMetadata() {
  await initializeNarratorValidator()
  return JSON.parse(solutionContractMetadataJson())
}

/**
 * Compare one submitted row with an authored expected answer. Unordered rows
 * require exact set equality; ordered rows require exact sequence equality.
 *
 * @param {readonly string[]} expected
 * @param {readonly string[]} submitted
 * @param {boolean} ordered
 * @returns {Promise<boolean>}
 */
export async function solutionAnswerMatches(expected, submitted, ordered) {
  await initializeNarratorValidator()
  return solutionAnswerMatchesJson(
    JSON.stringify(expected),
    JSON.stringify(submitted),
    ordered,
  )
}

/**
 * Parse reference-aware prose without validating a repository. Expression
 * offsets are zero-based UTF-8 byte offsets, matching the Rust API.
 *
 * @param {string} source
 * @returns {Promise<import('./index.js').ReferenceTextParseResult>}
 */
export async function parseReferenceText(source) {
  await initializeNarratorValidator()
  return JSON.parse(parseReferenceTextJson(source))
}

import type { InitInput } from './narrator_validator.js'

export type { InitInput } from './narrator_validator.js'

export interface SourceFile {
  path: string
  source: string
}

export type Severity = 'error' | 'warning'

export interface Position {
  /** One-based line number. */
  line: number
  /** One-based UTF-8 byte column. */
  column: number
}

export interface SourceRange {
  start: Position
  end: Position
}

export interface RelatedLocation {
  message: string
  path: string
  pointer?: string
  range?: SourceRange
}

export interface Diagnostic {
  severity: Severity
  code: string
  message: string
  path: string
  pointer?: string
  range?: SourceRange
  subject_id?: string
  related?: RelatedLocation[]
}

export interface ValidationReport {
  validator_version: string
  format_version?: number
  valid: boolean
  diagnostics: Diagnostic[]
}

/**
 * Load the validator WebAssembly module.
 *
 * Browser callers normally omit `input`. Passing bytes or a compiled module
 * is useful in non-browser runtimes and tests.
 */
export function initializeNarratorValidator(
  input?: InitInput | Promise<InitInput>,
): Promise<void>

/**
 * Validate a complete, in-memory Narrator repository snapshot.
 *
 * The first call loads the WASM module. Subsequent calls reuse it.
 */
export function validateRepository(
  files: readonly SourceFile[],
): Promise<ValidationReport>

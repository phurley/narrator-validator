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
  format_version?: string
  valid: boolean
  diagnostics: Diagnostic[]
}

export interface RulesetCommandParameter {
  name: string
  description?: string
  types: Array<'character' | 'entity' | 'setting' | 'deduction' | 'event'>
  min: number
  max: number
}

export interface RulesetCommand {
  id: string
  name: string
  description?: string
  parameters?: RulesetCommandParameter[]
  effects?: Array<Record<string, unknown>>
}

export interface StandardMysteryRuleset {
  id: 'ruleset.standard_mystery'
  version: '1.0.0'
  commands: RulesetCommand[]
}

export const STANDARD_MYSTERY_RULESET: StandardMysteryRuleset

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

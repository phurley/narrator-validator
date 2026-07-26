/** Stable application-facing types for narrator-validator's WASM JSON API. */
export interface SourceFile {
  path: string;
  source: string;
}

export type Severity = "error" | "warning";

export interface Position {
  /** One-based line number. */
  line: number;
  /** One-based UTF-8 byte column. */
  column: number;
}

export interface SourceRange {
  start: Position;
  end: Position;
}

export interface RelatedLocation {
  message: string;
  path: string;
  pointer?: string;
  range?: SourceRange;
}

export interface Diagnostic {
  severity: Severity;
  code: string;
  message: string;
  path: string;
  pointer?: string;
  range?: SourceRange;
  subject_id?: string;
  related?: RelatedLocation[];
}

export interface ValidationReport {
  validator_version: string;
  format_version?: number;
  valid: boolean;
  diagnostics: Diagnostic[];
}

export type WasmValidateJson = (filesJson: string) => string;

/**
 * Typed wrapper around the generated WASM module's `validate_json` export.
 *
 * Keep validation in a Web Worker: parsing always covers the complete
 * in-memory repository snapshot and should not block editor input.
 */
export function validateRepository(
  validateJson: WasmValidateJson,
  files: SourceFile[],
): ValidationReport {
  return JSON.parse(validateJson(JSON.stringify(files))) as ValidationReport;
}

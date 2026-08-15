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
  features?: string[]
  reference_text?: ResolvedReferenceText[]
  playability?: PlayabilityReport
}

export type PlayabilityStatus = 'proved' | 'not_proved' | 'inconclusive'

export interface PlayabilityReport {
  model_version: number
  explored_states: number
  bounded: boolean
  terminal_paths: TerminalPathAnalysis[]
  notebook_policies: NotebookPolicyAnalysis[]
  deduction_graph: DeductionGraphAnalysis
}

export interface NotebookPolicyAnalysis {
  auto_facts: boolean
  auto_deductions: boolean
  explored_states: number
  bounded: boolean
  terminal_paths: TerminalPathAnalysis[]
  solution_answerability: SolutionAnswerability
}

export interface SolutionAnswerability {
  status: PlayabilityStatus
  action_count?: number
  solution_equivalent_deductions?: string[]
}

export interface DeductionGraphAnalysis {
  maximum_depth: number
  largest_cascade_size: number
  largest_cascade_root?: string
  largest_cascade?: string[]
}

export interface TerminalPathAnalysis {
  id: string
  outcome: string
  status: PlayabilityStatus
  path: string
  pointer: string
  range?: SourceRange
  lower_bound?: PlayabilityLowerBound
  blocker?: PlayabilityBlocker
}

export interface PlayabilityLowerBound {
  entry_setting: string
  action_count: number
  route_action_count: number
  elapsed_minutes: number
  wait_minutes: number
  required_waits: PlayabilityRequiredWait[]
  ordered_steps: PlayabilityStep[]
  pivotal_unlocks: string[]
}

export interface PlayabilityRequiredWait {
  trigger: string
  delay_minutes: number
}

export interface PlayabilityStep {
  kind: string
  action: string
  from?: string
  to?: string
  elapsed_minutes: number
  unlocks?: string[]
}

export interface PlayabilityBlocker {
  code: string
  message: string
  path: string
  pointer: string
  range?: SourceRange
  chain?: string[]
}

export type DisclosureClass =
  | 'player_safe'
  | 'gated_player_safe'
  | 'private_narrator'

export interface ReferenceExpression {
  authored: string
  target_id: string
  property_path: string[]
  /** Zero-based UTF-8 byte offset of the opening delimiter. */
  start: number
  /** Exclusive zero-based UTF-8 byte offset after the closing delimiter. */
  end: number
}

export type ReferenceTextSegment =
  | { type: 'literal'; text: string }
  | { type: 'reference'; expression: ReferenceExpression }

export interface ParsedReferenceText {
  source: string
  segments: ReferenceTextSegment[]
}

export type ReferenceParseError =
  | { type: 'unclosed'; start: number }
  | { type: 'empty'; start: number; end: number }
  | { type: 'invalid'; authored: string; start: number; end: number }
  | { type: 'unexpected_close'; start: number }

export type ReferenceTextParseResult =
  | { status: 'parsed'; value: ParsedReferenceText }
  | { status: 'error'; error: ReferenceParseError }

export interface ReferenceProvenance {
  expression: ReferenceExpression
  path: string
  pointer: string
  range?: SourceRange
  definition_pointer: string
  resolved_path: string
  resolved_value: string
}

export interface ResolvedReferenceText {
  path: string
  pointer: string
  disclosure: DisclosureClass
  authored: string
  resolved: string
  provenance: ReferenceProvenance[]
}

export interface ReferenceTextMetadata {
  supported_features: string[]
  consumer_fields: Array<{
    kind: string
    path: string
    disclosure: DisclosureClass
  }>
  reference_kinds: Array<{
    kind: string
    default_path: string | null
    paths: Array<{ path: string; disclosure: DisclosureClass }>
  }>
}

export interface RulesetCommandParameter {
  name: string
  description?: string
  types: Array<'character' | 'entity' | 'setting' | 'deduction' | 'event'>
  min: number
  max: number
  candidates?: {
    from: Array<'all' | 'current_location' | 'inventory' | 'reachable' | 'known' | 'established'>
    capabilities?: Array<'portable'>
  }
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
  version: '1.0.0' | '2.0.0' | '3.0.0' | '4.0.0' | '5.0.0'
  commands: RulesetCommand[]
  command_capabilities: Array<{
    command_id: string
    mechanic:
      | 'claim_fact'
      | 'establish_deduction'
      | 'reconcile_notebooks'
      | 'submit_solution'
    enabled_when:
      | 'manual_facts'
      | 'manual_deductions'
      | 'multiple_players_with_unshared_facts'
      | 'always'
  }>
}

export interface SolutionContractMetadata {
  story_format_version: '3.3.0'
  ruleset_id: 'ruleset.standard_mystery'
  ruleset_version: '3.0.0'
  compatible_ruleset_versions: ['3.0.0', '4.0.0', '5.0.0']
  min_questions: 1
  max_questions: 4
  min_answer_cards: 1
  max_answer_cards: 5
  ordered_default: false
  prompt_disclosure: 'player_safe'
  expected_answer_disclosure: 'private_narrator'
}

export interface EndStateContractMetadata {
  story_format_version: '3.4.0'
  canonical_section: 'end_states'
  canonical_file: 'end_states.yaml'
  legacy_section: 'win_states'
  precedence: 'authored_order_first_satisfied'
  evaluation_timing: 'after_every_resolved_turn'
  score_semantics: 'snapshot_and_minimum_gate'
  legacy_outcome: 'won'
  legacy_resolution: 'full'
  legal_outcome_resolutions: readonly [
    { outcome: 'won'; resolutions: readonly ['full', 'partial'] },
    { outcome: 'lost'; resolutions: readonly ['failure'] },
  ]
}

export const STANDARD_MYSTERY_RULESET: StandardMysteryRuleset
export const STANDARD_MYSTERY_RULESETS: readonly StandardMysteryRuleset[]
export const VALIDATOR_SOURCE_COMMIT: '__NARRATOR_VALIDATOR_SOURCE_COMMIT__'

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

export function validateRepositoryWithFeatures(
  files: readonly SourceFile[],
  supportedFeatures: readonly string[],
): Promise<ValidationReport>

export function referenceTextMetadata(): Promise<ReferenceTextMetadata>

export function solutionContractMetadata(): Promise<SolutionContractMetadata>

export function endStateContractMetadata(): Promise<EndStateContractMetadata>

export function solutionAnswerMatches(
  expected: readonly string[],
  submitted: readonly string[],
  ordered: boolean,
): Promise<boolean>

/** Parse prose; expression offsets are zero-based UTF-8 byte offsets. */
export function parseReferenceText(
  source: string,
): Promise<ReferenceTextParseResult>

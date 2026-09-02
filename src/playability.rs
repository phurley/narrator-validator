//! Conservative static playability analysis for the supported monotonic story subset.
//!
//! This is deliberately separate from structural validation. A proof is emitted
//! only for a concrete sequence of supported actions. Unsupported mechanics can
//! never turn a path into `proved`; they make an otherwise unresolved path
//! `inconclusive`.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap};

use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value};

use crate::{Position, SourceFile, SourceRange};

const MODEL_VERSION: u32 = 4;
const MAX_EXPLORED_STATES: usize = 25_000;
// `search_from`'s bounded-recovery legs are independently budgeted, seeded
// only from a genuine playthrough witness rather than re-deriving the whole
// reachable graph from the story's beginning -- see `search_from`'s doc
// comment. That narrower starting point means a larger budget here is cheap
// where the same bump on the main `search` would not be (narrator-validator#99).
const LEG_MAX_EXPLORED_STATES: usize = 60_000;
const MAX_ACTIONS: u32 = 96;
const MAX_ELAPSED_MINUTES: u32 = 2 * 24 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayabilityStatus {
    Proved,
    NotProved,
    Inconclusive,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayabilityReport {
    pub model_version: u32,
    /// Compatibility summary for the default automatic-facts/automatic-
    /// deductions policy. New consumers should use `notebook_policies`.
    pub explored_states: usize,
    pub bounded: bool,
    pub terminal_paths: Vec<TerminalPathAnalysis>,
    pub notebook_policies: Vec<NotebookPolicyAnalysis>,
    pub deduction_graph: DeductionGraphAnalysis,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotebookPolicyAnalysis {
    pub auto_facts: bool,
    pub auto_deductions: bool,
    pub explored_states: usize,
    pub bounded: bool,
    pub terminal_paths: Vec<TerminalPathAnalysis>,
    pub solution_answerability: SolutionAnswerability,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub step_answerability: Vec<StepAnswerability>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SolutionAnswerability {
    pub status: PlayabilityStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub solution_equivalent_deductions: Vec<String>,
}

/// Per-step answer reachability for a Format 3.7 `solution.steps` story
/// (Format 3.3-3.6's single-commit `solution_answerability` has nothing
/// equivalent to prove per-step; this is the multi-step generalization).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepAnswerability {
    pub id: String,
    pub status: PlayabilityStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocker: Option<PlayabilityBlocker>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeductionGraphAnalysis {
    pub maximum_depth: usize,
    pub largest_cascade_size: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub largest_cascade_root: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub largest_cascade: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalPathAnalysis {
    pub id: String,
    pub outcome: String,
    pub status: PlayabilityStatus,
    pub path: String,
    pub pointer: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<SourceRange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lower_bound: Option<PlayabilityLowerBound>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocker: Option<PlayabilityBlocker>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayabilityLowerBound {
    pub entry_setting: String,
    pub action_count: u32,
    pub route_action_count: u32,
    pub elapsed_minutes: u32,
    pub wait_minutes: u32,
    pub required_waits: Vec<PlayabilityRequiredWait>,
    pub ordered_steps: Vec<PlayabilityStep>,
    pub pivotal_unlocks: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayabilityRequiredWait {
    pub trigger: String,
    pub delay_minutes: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayabilityStep {
    pub kind: String,
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    pub elapsed_minutes: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unlocks: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayabilityBlocker {
    pub code: String,
    pub message: String,
    pub path: String,
    pub pointer: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<SourceRange>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chain: Vec<String>,
}

#[derive(Clone)]
struct LocatedItem {
    id: String,
    path: String,
    pointer: String,
    range: Option<SourceRange>,
    map: Mapping,
    owner: Option<String>,
}

#[derive(Clone)]
struct Route {
    id: String,
    from: String,
    to: String,
    minutes: u32,
    bidirectional: bool,
    requirements: Vec<String>,
}

#[derive(Clone, Default)]
struct ActionPattern {
    command: String,
    bindings: BTreeMap<String, Vec<String>>,
}

#[derive(Clone)]
struct FactRule {
    item: LocatedItem,
    on: Option<ActionPattern>,
    when: Vec<Predicate>,
    opening: bool,
    /// Raw `about` list: subject IDs this fact establishes knowledge of.
    about: Vec<String>,
    /// Raw `statement` prose, scanned for `[[ref]]` roots that also count
    /// as established subjects.
    statement: String,
}

/// A gate synthesized from a character's `testimony[].reveals` entry: the
/// fact is only available once `command.question` has been asked of the
/// testimony's owning character, subject to whatever else the testimony's
/// own `requires` implies.
#[derive(Clone)]
struct TestimonyGate {
    owner: String,
    requires: Vec<String>,
}

#[derive(Clone)]
struct DeductionRule {
    item: LocatedItem,
    inputs: Vec<String>,
    inputs_range: Option<SourceRange>,
    dependencies: Vec<String>,
    solves: BTreeSet<String>,
    /// Raw `conclusion` prose, scanned for `[[ref]]` roots the deduction
    /// establishes knowledge of.
    conclusion: String,
}

/// A Format 3.7 `solution.steps[]` entry.
#[derive(Clone)]
struct SolveStep {
    item: LocatedItem,
    time_cost: u32,
    rows: Vec<SolveRow>,
    on_success: StepOutcome,
    on_failure: StepOutcome,
}

#[derive(Clone)]
enum SolveRow {
    NOfM { n: usize, pool: Vec<String> },
    Ordered { cards: Vec<String> },
}

#[derive(Clone, Default)]
struct StepOutcome {
    set_flags: Vec<String>,
    points: i64,
}

#[derive(Clone)]
struct TriggerRule {
    item: LocatedItem,
    on: Option<ActionPattern>,
    when: Vec<Predicate>,
    after: u32,
    effects: Vec<Effect>,
    facts: Vec<String>,
    once: bool,
}

#[derive(Clone)]
struct CommandRule {
    id: String,
    effects: Vec<Effect>,
    requires_binding: bool,
}

#[derive(Clone)]
struct EndRule {
    item: LocatedItem,
    outcome: String,
    requirements: Vec<String>,
    minimum_points: u64,
    at_or_after: Option<u32>,
    solution_condition: bool,
}

#[derive(Clone)]
struct PointAward {
    source: String,
    kind: &'static str,
    value: u64,
    max_claim_count: u64,
    requirements: Vec<String>,
}

/// The supported predicate subset -- deliberately has no variant that can
/// require a subject's *absence*, or that an entity NOT be carried. `Has`
/// only ever asserts presence (a fact/deduction/flag/trigger-completion, or
/// the current location); nothing here or in `has()` can express "does not
/// have". This is why `State::inventory` is safe to model as monotone
/// (`command.take` only ever inserts, nothing ever removes): once a
/// portable entity is picked up, there is no supported predicate that could
/// possibly start failing because of it, so `command.drop` -- were it
/// modeled -- could never make a previously-unreachable end/step reachable.
/// Omitting it is therefore not a completeness gap in the take-only
/// increment; it is provably inert for this predicate subset. See
/// narrator-validator#96.
#[derive(Clone)]
enum Predicate {
    Has(String),
    At(String),
    TimeAfter(u32),
    TimeEqual(u32),
    TimeBefore(u32),
    Never,
}

#[derive(Clone)]
enum Effect {
    SetFlag(String),
    AdvanceTime(u32),
    LearnFact(String),
    EstablishDeduction(String),
    Move(String),
}

#[derive(Clone)]
struct Unsupported {
    code: String,
    message: String,
    path: String,
    pointer: String,
    range: Option<SourceRange>,
    /// True when the flagged construct is structurally excluded from the
    /// search itself: an unsatisfiable predicate (`Predicate::Never`), a
    /// route/visibility gate that can never hold (`has()` never returns
    /// true for an `entity.*` id), or a command/trigger filtered out of
    /// the candidate set entirely (`unsupported_commands`/
    /// `unsupported_triggers`). No witness the search finds can ever
    /// depend on a search-excluded construct, so it can never demote a
    /// genuinely-proved end/step -- see narrator-validator#88.
    ///
    /// False for the rarer constructs that stay *live* in the model
    /// despite being unsupported (an authored actor/owner that can't be
    /// resolved makes a fact/trigger's `on` gate silently drop to
    /// "ambient", not disappear; an ambiguous deduction remains fully
    /// establishable). For those, `witness_subject` names the id whose
    /// presence in a witness's reached state marks the construct as
    /// actually on that witness's path.
    search_excluded: bool,
    witness_subject: Option<String>,
}

/// An authored `entities[].initial.container` value, captured verbatim so
/// nested containers (an entity whose container is itself another entity,
/// rather than a `setting.*`) can be resolved transitively once every file
/// has been read -- the owning entity's own container may live in a file
/// processed later in `from_files`'s loop, or later in the same file.
#[derive(Clone)]
struct RawContainerRef {
    container: String,
    path: String,
    pointer: String,
    range: Option<SourceRange>,
}

#[derive(Clone, Default)]
struct Model {
    entries: Vec<String>,
    initial_minutes: u32,
    routes: Vec<Route>,
    commands: BTreeMap<String, CommandRule>,
    facts: BTreeMap<String, FactRule>,
    deductions: BTreeMap<String, DeductionRule>,
    triggers: BTreeMap<String, TriggerRule>,
    ends: Vec<EndRule>,
    initial_flags: BTreeSet<String>,
    unsupported: Vec<Unsupported>,
    solution_target: Option<String>,
    point_awards: BTreeMap<String, PointAward>,
    subject_locations: BTreeMap<String, String>,
    entity_containers: BTreeMap<String, RawContainerRef>,
    subject_requirements: BTreeMap<String, Vec<String>>,
    unsupported_commands: BTreeSet<String>,
    unsupported_triggers: BTreeSet<String>,
    solve_action: Option<String>,
    solution_answer_rows: Vec<BTreeSet<String>>,
    precomputed_patterns: Vec<ActionPattern>,
    elapsed_equivalence_horizon: u32,
    testimony_gates: BTreeMap<String, TestimonyGate>,
    /// Format 3.7 `solution.steps` (parallel to the legacy
    /// `solve_action`/`solution_target` path, which stays untouched).
    solve_steps: Vec<SolveStep>,
    max_attempts: Option<u32>,
    /// Card subject ID (including `answer.*`) -> the IDs of facts/
    /// deductions that establish knowledge of it.
    subject_witnesses: BTreeMap<String, BTreeSet<String>>,
    /// Requirement ID (a flag or trigger-established fact) -> the single
    /// `setting.*` location a trigger's `when: [{at: ...}, ...]` pins as
    /// necessary before that trigger can fire and produce the requirement
    /// (via `SetFlag` or its `facts` list). Only populated when exactly one
    /// producing trigger exists and its `when` names exactly one `at`, so
    /// this is never used to claim more than the model actually says.
    /// Consulted by `end_shortfall` as provenance credit -- see its doc
    /// comment.
    requirement_locations: BTreeMap<String, String>,
    /// Fact ID -> the single `setting.*` location that fact's own gate
    /// (trigger-produced, or an `on` action pattern whose entity/character
    /// bindings all resolve to one place) pins as necessary before the fact
    /// can ever be learned. Seeded from `requirement_locations`'s `fact.*`
    /// entries (trigger-produced facts), then extended per-fact for an
    /// examine/question-style `on` pattern: every binding value is resolved
    /// (`setting.*` to itself, `entity.*`/`character.*` via
    /// `subject_locations`) and recorded only if every binding resolves to
    /// the *same* location -- if any binding doesn't resolve, or two
    /// disagree, nothing is recorded, so this can never overclaim. Consulted
    /// by `subject_shortfall` the same way `requirement_locations` is
    /// consulted by `end_shortfall` -- see narrator-validator#94.
    fact_acquisition_locations: BTreeMap<String, String>,
    /// All-pairs BFS hop counts over `self.routes`, ignoring route
    /// `requirements` (a heuristic distance signal doesn't need to be
    /// admissible or even reachable-aware -- `search_from`'s soundness
    /// never depends on it). Consulted by `hop_count`, which both
    /// `end_shortfall`'s `away_from_producer` term and
    /// `subject_shortfall`/`witness_extra_cost`'s location-match terms use
    /// in place of a flat binary "elsewhere" penalty -- see
    /// narrator-validator#94's Option B.
    hop_counts: BTreeMap<(String, String), u32>,
    /// `entity.*` IDs authored with `physical.portable: true`. Populated
    /// from the `entities` branch of `read_owned_facts`. Every portable
    /// entity is a candidate for `command.take`, but not every one is
    /// actually generated as a take action -- see `takeable_entities`.
    portable_entities: BTreeSet<String>,
    /// The subset of `portable_entities` that actually appears in some
    /// *trigger's* `on` binding, a `solution_answer_rows` row, or a
    /// `solve_steps` row's pool/cards -- i.e. an entity some trigger or
    /// solve gate could actually care about the player carrying.
    /// Deliberately NOT sourced from the full `precomputed_patterns` set:
    /// an owned fact's `on` pattern binds `owner`, which always resolves to
    /// that fact's own entity, so it's satisfied by co-location with itself
    /// wherever it currently is and never needs carrying -- see
    /// `precompute_takeable_entities`'s doc comment for why that
    /// distinction matters in practice. `actions()` only ever generates a
    /// `command.take` candidate for a member of this set, never for every
    /// portable entity in the story: an unconstrained take-for-everything
    /// model reintroduces the exact 2^k inventory-subset state-space
    /// blowup narrator-validator#91/#94 just fixed for a different axis.
    /// Built once in `normalize()`, after `precompute_patterns` and the
    /// solve/solution rows are available.
    takeable_entities: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Pending {
    due: u32,
    trigger: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct State {
    entry: String,
    location: String,
    elapsed: u32,
    facts: BTreeSet<String>,
    available_facts: BTreeSet<String>,
    deductions: BTreeSet<String>,
    flags: BTreeSet<String>,
    completed: BTreeSet<String>,
    pending: Vec<Pending>,
    score: u64,
    point_claims: BTreeMap<String, u64>,
    solution_solved: bool,
    /// Index of the next Format 3.7 `solution.steps` entry to commit.
    /// Unused (stays 0) for the legacy single-commit path.
    next_step: u8,
    /// Failed full-sequence attempts consumed so far. Only meaningful when
    /// `Model::max_attempts` is authored.
    attempts_used: u32,
    /// Portable entities the player currently carries, via `command.take`.
    /// Monotone -- nothing ever removes from this set; there is no
    /// `command.drop` effect in the static model. See the doc comment on
    /// `actions()`'s take-generation block for why that omission is sound.
    inventory: BTreeSet<String>,
}

#[derive(Clone)]
struct Node {
    state: State,
    actions: u32,
    route_actions: u32,
    wait_minutes: u32,
    steps: Vec<PlayabilityStep>,
    unlocks: BTreeSet<String>,
    /// IDs of `unsupported_triggers` whose `on` pattern actually matched
    /// (and whose `when` held) at some action along this witness's path.
    /// The trigger's real effect was never applied -- it's excluded from
    /// firing -- but an unsupported trigger piggybacks on an otherwise
    /// ordinary, still-available command, so this witness *did* take the
    /// action that would genuinely have set it off. Used to scope
    /// narrator-validator#88's demotion to witnesses this can actually
    /// affect.
    shadowed_triggers: BTreeSet<String>,
}

#[derive(Clone)]
struct QueueNode(Node);

impl PartialEq for QueueNode {
    fn eq(&self, other: &Self) -> bool {
        queue_key(&self.0) == queue_key(&other.0)
    }
}
impl Eq for QueueNode {}
impl PartialOrd for QueueNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for QueueNode {
    fn cmp(&self, other: &Self) -> Ordering {
        queue_key(&other.0).cmp(&queue_key(&self.0))
    }
}

fn queue_key(node: &Node) -> (u32, u32, &State) {
    (node.actions, node.state.elapsed, &node.state)
}

/// Queue wrapper used only by `search_from`'s per-step chaining legs. Orders
/// the heap by `(h, actions, elapsed, state)` instead of `search`'s plain
/// `(actions, elapsed, state)` -- `h` is a goal-distance estimate computed
/// once at push time (see `Model::step_shortfall`). Deliberately a separate
/// type from `QueueNode` / `queue_key`, which the main `search` function
/// relies on for its early-exit "first pop is minimal" invariant and must
/// stay untouched: `search_from`'s own soundness doesn't depend on
/// exploration order (see the comment on `search_from` itself), so a
/// goal-aware order is safe here without being admissible.
#[derive(Clone)]
struct HeuristicQueueNode(Node, u32);

impl PartialEq for HeuristicQueueNode {
    fn eq(&self, other: &Self) -> bool {
        heuristic_queue_key(&self.0, self.1) == heuristic_queue_key(&other.0, other.1)
    }
}
impl Eq for HeuristicQueueNode {}
impl PartialOrd for HeuristicQueueNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for HeuristicQueueNode {
    fn cmp(&self, other: &Self) -> Ordering {
        heuristic_queue_key(&other.0, other.1).cmp(&heuristic_queue_key(&self.0, self.1))
    }
}

fn heuristic_queue_key(node: &Node, h: u32) -> (u32, u32, u32, &State) {
    (h, node.actions, node.state.elapsed, &node.state)
}

#[derive(Clone)]
struct CandidateAction {
    kind: &'static str,
    id: String,
    pattern: ActionPattern,
    from: Option<String>,
    to: Option<String>,
    minutes: u32,
}

pub(crate) fn analyze(
    files: &[SourceFile],
    format_version: Option<&str>,
) -> Option<PlayabilityReport> {
    if !format_version.is_some_and(|version| version.starts_with("3.")) {
        return None;
    }
    let mut model = Model::from_files(files);
    model.normalize();
    let mut notebook_policies = Vec::new();
    for auto_facts in [true, false] {
        for auto_deductions in [true, false] {
            notebook_policies.push(model.search(auto_facts, auto_deductions));
        }
    }
    let default = notebook_policies
        .first()
        .expect("the default notebook policy is always analyzed");
    Some(PlayabilityReport {
        model_version: MODEL_VERSION,
        explored_states: default.explored_states,
        bounded: default.bounded,
        terminal_paths: default.terminal_paths.clone(),
        notebook_policies,
        deduction_graph: model.deduction_graph_analysis(),
    })
}

impl Model {
    fn from_files(files: &[SourceFile]) -> Self {
        let mut model = Self::default();
        for file in files {
            let Ok(root) = serde_yaml::from_str::<Value>(&file.source) else {
                continue;
            };
            let Some(root) = root.as_mapping() else {
                continue;
            };
            if let Some(case) = map(root, "case") {
                model.entries.extend(strings(field(case, "entry_settings")));
                model.initial_minutes = string(case, "initial_time")
                    .and_then(parse_clock)
                    .unwrap_or(0);
                if let Some(solution) = map(root, "solution") {
                    model.solution_target = string(solution, "win_state").map(str::to_string);
                    if let Some(questions) =
                        field(solution, "questions").and_then(Value::as_sequence)
                    {
                        model
                            .solution_answer_rows
                            .extend(questions.iter().filter_map(|question| {
                                let question = question.as_mapping()?;
                                let answers = strings(field(question, "answer"))
                                    .into_iter()
                                    .collect::<BTreeSet<_>>();
                                (!answers.is_empty()).then_some(answers)
                            }));
                    } else {
                        let answers = ["culprit", "weapon", "location"]
                            .iter()
                            .filter_map(|field| string(solution, field))
                            .map(str::to_string)
                            .collect::<BTreeSet<_>>();
                        if !answers.is_empty() {
                            model.solution_answer_rows.push(answers);
                        }
                    }
                    model.solve_action = field(solution, "questions")
                        .and_then(Value::as_sequence)
                        .filter(|questions| !questions.is_empty())
                        .and_then(|questions| {
                            questions
                                .iter()
                                .map(|question| {
                                    question
                                        .as_mapping()
                                        .map(|question| strings(field(question, "answer")))
                                        .filter(|answer| !answer.is_empty())
                                })
                                .collect::<Option<Vec<_>>>()
                        })
                        .map(|answers| {
                            let mut action = "command.solve".to_string();
                            for answer in answers {
                                action.push_str(" [");
                                action.push_str(&answer.join(" "));
                                action.push(']');
                            }
                            action
                        });
                    model.max_attempts =
                        u64_field(solution, "max_attempts").map(|value| value as u32);
                    model.read_solution_steps(file, solution);
                }
                if let Some(ruleset) = map(case, "ruleset") {
                    if let (Some(id), Some(version)) =
                        (string(ruleset, "id"), string(ruleset, "version"))
                    {
                        if let Ok(resolved) = crate::resolve_ruleset(&crate::RulesetReference {
                            id: id.to_string(),
                            version: version.to_string(),
                        }) {
                            if let Ok(Value::Mapping(catalog)) =
                                serde_yaml::from_str::<Value>(resolved.commands_yaml)
                            {
                                model.read_commands(
                                    &SourceFile {
                                        path: "<ruleset>".to_string(),
                                        source: resolved.commands_yaml.to_string(),
                                    },
                                    sequence(&catalog, "commands"),
                                );
                            }
                        }
                    }
                }
            }
            model.read_routes(file, sequence(root, "routes"));
            model.read_flags(sequence(root, "flags"));
            model.read_commands(file, sequence(root, "commands"));
            model.read_triggers(file, sequence(root, "triggers"));
            model.read_deductions(file, sequence(root, "deductions"));
            for section in ["settings", "characters", "entities", "events"] {
                model.read_owned_facts(file, section, sequence(root, section));
            }
            model.read_ends(file, "end_states", sequence(root, "end_states"), false);
            model.read_ends(file, "win_states", sequence(root, "win_states"), true);
        }
        model.resolve_nested_containers();
        model.apply_testimony_gates();
        if model.entries.is_empty() {
            if let Some(first) = model.routes.first() {
                model.entries.push(first.from.clone());
            }
        }
        model.entries.sort();
        model.entries.dedup();
        model
    }

    fn read_routes(&mut self, file: &SourceFile, values: &[Value]) {
        for (index, value) in values.iter().enumerate() {
            let Some(item) = located(file, "routes", index, value, None) else {
                continue;
            };
            let (Some(from), Some(to)) = (string(&item.map, "from"), string(&item.map, "to"))
            else {
                continue;
            };
            let minutes = u64_field(&item.map, "travel_minutes").unwrap_or(0) as u32;
            let requirements = strings(field(&item.map, "requires"));
            for (requirement_index, requirement) in requirements.iter().enumerate() {
                if requirement.starts_with("entity.") {
                    self.unsupported(
                        file,
                        &format!("{}/requires/{requirement_index}", item.pointer),
                        "playability.unsupported_inventory_condition",
                        "entity route gates depend on reachable selection or inventory ownership, which is outside the static subset",
                    );
                }
            }
            self.routes.push(Route {
                id: item.id,
                from: from.to_string(),
                to: to.to_string(),
                minutes,
                bidirectional: bool_field(&item.map, "bidirectional").unwrap_or(false),
                requirements,
            });
        }
    }

    fn read_flags(&mut self, values: &[Value]) {
        for value in values {
            let Some(map) = value.as_mapping() else {
                continue;
            };
            if bool_field(map, "initial_state") == Some(true) {
                if let Some(id) = string(map, "id") {
                    self.initial_flags.insert(id.to_string());
                }
            }
        }
    }

    fn read_commands(&mut self, file: &SourceFile, values: &[Value]) {
        for (index, value) in values.iter().enumerate() {
            let Some(item) = located(file, "commands", index, value, None) else {
                continue;
            };
            let unsupported_before = self.unsupported.len();
            let effects = self.effects(file, &item.pointer, &item.map, item.id == "command.move");
            if self.unsupported.len() > unsupported_before {
                self.unsupported_commands.insert(item.id.clone());
            }
            self.read_points(file, &item, "command");
            let requires_binding = field(&item.map, "parameters")
                .and_then(Value::as_sequence)
                .is_some_and(|parameters| {
                    parameters.iter().any(|parameter| {
                        parameter
                            .as_mapping()
                            .and_then(|parameter| u64_field(parameter, "min"))
                            .unwrap_or(0)
                            > 0
                    })
                });
            self.commands.insert(
                item.id.clone(),
                CommandRule {
                    id: item.id,
                    effects,
                    requires_binding,
                },
            );
        }
    }

    fn read_owned_facts(&mut self, file: &SourceFile, section: &str, owners: &[Value]) {
        for (owner_index, owner_value) in owners.iter().enumerate() {
            let Some(owner) = owner_value.as_mapping() else {
                continue;
            };
            let owner_id = string(owner, "id").map(str::to_string);
            if let Some(owner_id) = owner_id.as_ref() {
                if section == "characters" {
                    if let Some(location) =
                        map(owner, "initial").and_then(|initial| string(initial, "location"))
                    {
                        self.subject_locations
                            .insert(owner_id.clone(), location.to_string());
                    }
                    if let Some(presence) = map(owner, "presence") {
                        self.subject_requirements
                            .insert(owner_id.clone(), strings(field(presence, "requires")));
                    }
                    if let Some(testimony) = field(owner, "testimony").and_then(Value::as_sequence)
                    {
                        for entry in testimony {
                            let Some(entry) = entry.as_mapping() else {
                                continue;
                            };
                            let requires = strings(field(entry, "requires"));
                            for reveal in strings(field(entry, "reveals")) {
                                self.testimony_gates.entry(reveal).or_insert_with(|| {
                                    TestimonyGate {
                                        owner: owner_id.clone(),
                                        requires: requires.clone(),
                                    }
                                });
                            }
                        }
                    }
                } else if section == "entities" {
                    if map(owner, "physical").and_then(|physical| bool_field(physical, "portable"))
                        == Some(true)
                    {
                        self.portable_entities.insert(owner_id.clone());
                    }
                    if let Some(container) =
                        map(owner, "initial").and_then(|initial| string(initial, "container"))
                    {
                        let pointer = format!("/{section}/{owner_index}/initial/container");
                        self.entity_containers.insert(
                            owner_id.clone(),
                            RawContainerRef {
                                container: container.to_string(),
                                path: file.path.clone(),
                                pointer: pointer.clone(),
                                range: locate_pointer(file, &pointer),
                            },
                        );
                    }
                    if field(owner, "points").is_some() {
                        self.unsupported(file, &format!("/{section}/{owner_index}/points"), "playability.unsupported_entity_points", "entity point awards require inventory transitions and are outside this static subset");
                    }
                    if let Some(visibility) = map(owner, "visibility") {
                        let requirements = strings(field(visibility, "requires"));
                        if requirements.iter().any(|id| id.starts_with("entity.")) {
                            self.unsupported(
                                file,
                                &format!("/{section}/{owner_index}/visibility/requires"),
                                "playability.unsupported_inventory_condition",
                                "entity ownership visibility gates are outside the static subset",
                            );
                        }
                        self.subject_requirements
                            .insert(owner_id.clone(), requirements);
                    }
                } else if section == "settings" {
                    if let Some(item) = located(file, section, owner_index, owner_value, None) {
                        self.read_points(file, &item, "setting");
                    }
                }
            }
            let Some(facts) = field(owner, "facts").and_then(Value::as_sequence) else {
                continue;
            };
            for (fact_index, value) in facts.iter().enumerate() {
                let pointer = format!("/{section}/{owner_index}/facts/{fact_index}");
                let Some(item) = located_at(file, value, pointer, owner_id.clone()) else {
                    continue;
                };
                let unsupported_before = self.unsupported.len();
                let on = field(&item.map, "on")
                    .and_then(Value::as_mapping)
                    .and_then(|map| self.pattern(file, &item.pointer, map, item.owner.as_deref()));
                // `pattern()` failing here (authored actor / unresolved
                // `owner`) doesn't drop the fact -- it drops the `on` gate,
                // which falls through to the "ambient, fires once `when`
                // holds" path below, i.e. this fact stays live in the
                // search. Mark the note(s) it just raised as such, keyed by
                // the fact that could actually end up on a witness.
                if on.is_none() {
                    for note in &mut self.unsupported[unsupported_before..] {
                        note.search_excluded = false;
                        note.witness_subject = Some(item.id.clone());
                    }
                }
                let when = self.predicates(file, &item.pointer, &item.map);
                let opening =
                    field(&item.map, "on").is_none() && field(&item.map, "when").is_none();
                let about = strings(field(&item.map, "about"));
                let statement = string(&item.map, "statement").unwrap_or("").to_string();
                self.facts.insert(
                    item.id.clone(),
                    FactRule {
                        item,
                        on,
                        when,
                        opening,
                        about,
                        statement,
                    },
                );
            }
        }
    }

    /// Resolve every authored `entities[].initial.container` transitively
    /// into a `setting.*` for `subject_locations`: a container may itself be
    /// another entity (an item in a bag in a room), any number of levels
    /// deep, and its own container may be authored in a different file or
    /// later in the same one, so this runs once after every file's owned
    /// facts have been read rather than inline in `read_owned_facts`.
    ///
    /// A container cycle (A contains B contains A) is guarded here rather
    /// than assumed to be caught elsewhere -- nothing in `read_owned_facts`
    /// or the schema validates that `initial.container` chains terminate --
    /// and is reported the same way as any other unresolved container: the
    /// entity keeps no `subject_locations` entry and gets an
    /// `unsupported_nested_container` note at its own authored pointer.
    ///
    /// `subject_locations` is a fixed *initial* fact consumed by
    /// `subject_known`/`action_available` to gate static reachability --
    /// nothing in this model tracks an entity moving after play starts, so
    /// there is no "container relocates during play" case to handle beyond
    /// resolving the authored initial nesting.
    fn resolve_nested_containers(&mut self) {
        let mut resolved: BTreeMap<String, Option<String>> = BTreeMap::new();
        let owners: Vec<String> = self.entity_containers.keys().cloned().collect();
        for owner_id in &owners {
            let mut visiting = BTreeSet::new();
            let setting = Self::resolve_container_setting(
                owner_id,
                &self.entity_containers,
                &mut resolved,
                &mut visiting,
            );
            match setting {
                Some(setting) => {
                    self.subject_locations.insert(owner_id.clone(), setting);
                }
                None => {
                    let raw = &self.entity_containers[owner_id];
                    self.unsupported.push(Unsupported {
                        code: "playability.unsupported_nested_container".to_string(),
                        message: "nested entity reachability is outside the static action model"
                            .to_string(),
                        path: raw.path.clone(),
                        pointer: raw.pointer.clone(),
                        range: raw.range,
                        search_excluded: true,
                        witness_subject: None,
                    });
                }
            }
        }
    }

    /// Resolve `id` (an entity id or a `setting.*` literal) to the
    /// `setting.*` it ultimately sits in, following `initial.container`
    /// chains through any number of intermediate entities. Returns `None`
    /// for a container cycle, a container pointing at something other than
    /// a `setting.*`/`entity.*` id, or an entity with no known container at
    /// all.
    fn resolve_container_setting(
        id: &str,
        containers: &BTreeMap<String, RawContainerRef>,
        resolved: &mut BTreeMap<String, Option<String>>,
        visiting: &mut BTreeSet<String>,
    ) -> Option<String> {
        if id.starts_with("setting.") {
            return Some(id.to_string());
        }
        if let Some(cached) = resolved.get(id) {
            return cached.clone();
        }
        if !visiting.insert(id.to_string()) {
            // Container cycle: A (transitively) contains itself.
            return None;
        }
        let result = match containers.get(id) {
            Some(raw)
                if raw.container.starts_with("setting.")
                    || raw.container.starts_with("entity.") =>
            {
                Self::resolve_container_setting(&raw.container, containers, resolved, visiting)
            }
            _ => None,
        };
        visiting.remove(id);
        resolved.insert(id.to_string(), result.clone());
        result
    }

    fn read_deductions(&mut self, file: &SourceFile, values: &[Value]) {
        for (index, value) in values.iter().enumerate() {
            let Some(item) = located(file, "deductions", index, value, None) else {
                continue;
            };
            let inputs = strings(field(&item.map, "inputs"));
            let inputs_range = locate_pointer(file, &format!("{}/inputs", item.pointer));
            let requirements = strings(field(&item.map, "requires"));
            let mut dependencies = inputs.clone();
            dependencies.extend(requirements.clone());
            let solves = field(&item.map, "solves")
                .and_then(Value::as_mapping)
                .map(|solves| {
                    ["culprit", "weapon", "location"]
                        .iter()
                        .filter_map(|field| string(solves, field))
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            self.read_points(file, &item, "deduction");
            let conclusion = string(&item.map, "conclusion").unwrap_or("").to_string();
            self.deductions.insert(
                item.id.clone(),
                DeductionRule {
                    item,
                    inputs,
                    inputs_range,
                    dependencies,
                    solves,
                    conclusion,
                },
            );
        }
    }

    /// Format 3.7's `solution.steps` -- a parallel path to the legacy
    /// `solve_action`/`solution_target` fields above, which stay untouched.
    fn read_solution_steps(&mut self, file: &SourceFile, solution: &Mapping) {
        let Some(steps) = field(solution, "steps").and_then(Value::as_sequence) else {
            return;
        };
        for (index, step) in steps.iter().enumerate() {
            let Some(item) = located_at(file, step, format!("/solution/steps/{index}"), None)
            else {
                continue;
            };
            let time_cost = u64_field(&item.map, "time_cost_minutes").unwrap_or(0) as u32;
            let rows = self.read_solve_rows(&item.map);
            let on_success = read_solve_step_outcome(&item.map, "on_success");
            let on_failure = read_solve_step_outcome(&item.map, "on_failure");
            self.solve_steps.push(SolveStep {
                item,
                time_cost,
                rows,
                on_success,
                on_failure,
            });
        }
    }

    fn read_solve_rows(&mut self, step: &Mapping) -> Vec<SolveRow> {
        let mut rows = Vec::new();
        let Some(raw_rows) = field(step, "rows").and_then(Value::as_sequence) else {
            return rows;
        };
        for row in raw_rows {
            let Some(row) = row.as_mapping() else {
                continue;
            };
            let cards = strings(field(row, "cards"));
            match string(row, "match") {
                Some("n_of_m") => {
                    let n = u64_field(row, "n").unwrap_or(cards.len() as u64) as usize;
                    rows.push(SolveRow::NOfM { n, pool: cards });
                }
                Some("ordered") => rows.push(SolveRow::Ordered { cards }),
                _ => {}
            }
        }
        rows
    }

    fn read_triggers(&mut self, file: &SourceFile, values: &[Value]) {
        for (index, value) in values.iter().enumerate() {
            let Some(item) = located(file, "triggers", index, value, None) else {
                continue;
            };
            let unsupported_before = self.unsupported.len();
            let on = field(&item.map, "on")
                .and_then(Value::as_mapping)
                .and_then(|map| self.pattern(file, &item.pointer, map, None));
            let when = self.predicates(file, &item.pointer, &item.map);
            let after = string(&item.map, "after")
                .and_then(parse_duration)
                .unwrap_or(0);
            if field(&item.map, "after").is_some() && after == 0 {
                self.unsupported(
                    file,
                    &format!("{}/after", item.pointer),
                    "playability.unsupported_delay",
                    "unsupported delayed-work duration",
                );
            }
            let effects = self.effects(file, &item.pointer, &item.map, false);
            if self.unsupported.len() > unsupported_before {
                self.unsupported_triggers.insert(item.id.clone());
                // Unlike an unsupported command (whose entire action
                // becomes unavailable, so it can never be part of any
                // witness) an unsupported trigger piggybacks on whatever
                // command matches its `on` pattern -- that command stays
                // perfectly ordinary and available. A witness can
                // therefore genuinely take the action that would have set
                // this trigger off in the real game, even though the
                // model silently drops its effect. Tag every note this
                // trigger just raised (regardless of which specific
                // sub-reason) so `witness_reached_by` can check whether
                // this witness actually shadowed it.
                for note in &mut self.unsupported[unsupported_before..] {
                    note.search_excluded = false;
                    note.witness_subject = Some(item.id.clone());
                }
            }
            let mut facts = Vec::new();
            if let Some(values) = field(&item.map, "facts").and_then(Value::as_sequence) {
                for (fact_index, value) in values.iter().enumerate() {
                    let pointer = format!("{}/facts/{fact_index}", item.pointer);
                    if let Some(fact) = located_at(file, value, pointer, Some(item.id.clone())) {
                        facts.push(fact.id.clone());
                        let about = strings(field(&fact.map, "about"));
                        let statement = string(&fact.map, "statement").unwrap_or("").to_string();
                        self.facts.insert(
                            fact.id.clone(),
                            FactRule {
                                item: fact,
                                on: None,
                                when: Vec::new(),
                                opening: false,
                                about,
                                statement,
                            },
                        );
                    }
                }
            }
            self.triggers.insert(
                item.id.clone(),
                TriggerRule {
                    once: bool_field(&item.map, "once").unwrap_or(true),
                    item,
                    on,
                    when,
                    after,
                    effects,
                    facts,
                },
            );
        }
    }

    fn read_ends(&mut self, file: &SourceFile, section: &str, values: &[Value], legacy: bool) {
        for (index, value) in values.iter().enumerate() {
            let Some(item) = located(file, section, index, value, None) else {
                continue;
            };
            let outcome = if legacy {
                "won"
            } else {
                string(&item.map, "outcome").unwrap_or("unknown")
            }
            .to_string();
            let requirements = strings(field(&item.map, "requires"));
            let minimum_points = u64_field(&item.map, "minimum_points").unwrap_or(0);
            let at_or_after = string(&item.map, "at_or_after").and_then(parse_clock);
            let solution_condition = self.solution_target.as_deref() == Some(&item.id);
            self.ends.push(EndRule {
                item,
                outcome,
                requirements,
                minimum_points,
                at_or_after,
                solution_condition,
            });
        }
    }

    fn read_points(&mut self, file: &SourceFile, item: &LocatedItem, kind: &'static str) {
        let Some(points) = field(&item.map, "points").and_then(Value::as_mapping) else {
            return;
        };
        let Some(value) = u64_field(points, "value") else {
            return;
        };
        let max_claim_count = u64_field(points, "max_claim_count").unwrap_or(1);
        if max_claim_count == 0 {
            self.unsupported(
                file,
                &format!("{}/points/max_claim_count", item.pointer),
                "playability.unsupported_points",
                "zero-claim point award cannot contribute to a proof",
            );
            return;
        }
        self.point_awards.insert(
            item.id.clone(),
            PointAward {
                source: item.id.clone(),
                kind,
                value,
                max_claim_count,
                requirements: strings(field(points, "requires")),
            },
        );
    }

    fn pattern(
        &mut self,
        file: &SourceFile,
        pointer: &str,
        map: &Mapping,
        owner: Option<&str>,
    ) -> Option<ActionPattern> {
        if field(map, "actor").is_some() {
            self.unsupported(
                file,
                &format!("{pointer}/actor"),
                "playability.unsupported_authored_actor",
                "authored non-player actors are outside the static action model",
            );
            return None;
        }
        let command = string(map, "command")?.to_string();
        let mut bindings = BTreeMap::new();
        if let Some(parameters) = field(map, "parameters").and_then(Value::as_mapping) {
            for (name, values) in parameters {
                let Some(name) = name.as_str() else { continue };
                let mut ids = strings(Some(values));
                for id in &mut ids {
                    if id == "owner" {
                        if let Some(owner) = owner {
                            *id = owner.to_string();
                        } else {
                            self.unsupported(
                                file,
                                pointer,
                                "playability.unsupported_owner",
                                "`owner` cannot be resolved outside an owned fact",
                            );
                            return None;
                        }
                    }
                }
                ids.sort();
                bindings.insert(name.to_string(), ids);
            }
        }
        Some(ActionPattern { command, bindings })
    }

    fn predicates(&mut self, file: &SourceFile, pointer: &str, item: &Mapping) -> Vec<Predicate> {
        let Some(when) = field(item, "when").and_then(Value::as_mapping) else {
            return Vec::new();
        };
        let Some(all) = field(when, "all").and_then(Value::as_sequence) else {
            self.unsupported(
                file,
                &format!("{pointer}/when"),
                "playability.unsupported_condition",
                "only deterministic `when.all` conditions are supported",
            );
            return vec![Predicate::Never];
        };
        let mut result = Vec::new();
        for (index, value) in all.iter().enumerate() {
            let Some(map) = value.as_mapping() else {
                continue;
            };
            if let Some(id) = string(map, "knows")
                .or_else(|| string(map, "flag"))
                .or_else(|| string(map, "completed"))
            {
                result.push(Predicate::Has(id.to_string()));
            } else if string(map, "owns").is_some() {
                self.unsupported(
                    file,
                    &format!("{pointer}/when/all/{index}/owns"),
                    "playability.unsupported_inventory_condition",
                    "inventory ownership predicates are outside the static subset",
                );
                result.push(Predicate::Never);
            } else if string(map, "player").is_some() {
                // The static search proves reachability for a single,
                // persona-less playthrough and never models which persona
                // or player slot is "acting". A `player` predicate is
                // therefore treated as never satisfied here: content gated
                // on it cannot make an otherwise-winnable default
                // playthrough unreachable (the fact is simply excluded from
                // the reachable set), but if the authored solution depends
                // on such a fact, the missing producer surfaces as an
                // inconclusive/unsupported terminal path rather than a
                // silent false pass.
                self.unsupported(
                    file,
                    &format!("{pointer}/when/all/{index}/player"),
                    "playability.unsupported_player_condition",
                    "persona/player-slot conditions are outside the static, persona-less playability subset",
                );
                result.push(Predicate::Never);
            } else if let Some(id) = string(map, "at") {
                result.push(Predicate::At(id.to_string()));
            } else if let Some(time) = field(map, "time").and_then(Value::as_mapping) {
                let relation = string(time, "relation");
                let minutes = string(time, "value").and_then(parse_clock);
                match (relation, minutes) {
                    (Some("after"), Some(minutes)) => result.push(Predicate::TimeAfter(minutes)),
                    (Some("at"), Some(minutes)) => result.push(Predicate::TimeEqual(minutes)),
                    (Some("before"), Some(minutes)) => result.push(Predicate::TimeBefore(minutes)),
                    _ => {
                        self.unsupported(
                            file,
                            &format!("{pointer}/when/all/{index}"),
                            "playability.unsupported_time_condition",
                            "unsupported time predicate",
                        );
                        result.push(Predicate::Never);
                    }
                }
            } else {
                self.unsupported(
                    file,
                    &format!("{pointer}/when/all/{index}"),
                    "playability.unsupported_condition",
                    "unsupported dynamic condition",
                );
                result.push(Predicate::Never);
            }
        }
        result
    }

    fn effects(
        &mut self,
        file: &SourceFile,
        pointer: &str,
        item: &Mapping,
        allow_route_effects: bool,
    ) -> Vec<Effect> {
        let mut result = Vec::new();
        let Some(values) = field(item, "effects").and_then(Value::as_sequence) else {
            return result;
        };
        for (index, value) in values.iter().enumerate() {
            let Some(map) = value.as_mapping() else {
                continue;
            };
            match string(map, "operation") {
                Some("set_flag") if bool_field(map, "value") == Some(true) => {
                    if let Some(id) = string(map, "flag") {
                        result.push(Effect::SetFlag(id.to_string()));
                    }
                }
                Some("advance_time") => {
                    if let Some(minutes) = u64_field(map, "minutes") {
                        result.push(Effect::AdvanceTime(minutes as u32));
                    } else if field(map, "route").is_none() || !allow_route_effects {
                        self.unsupported(
                            file,
                            &format!("{pointer}/effects/{index}"),
                            "playability.unsupported_effect",
                            "advance_time must use fixed minutes or a matched route",
                        );
                    }
                }
                Some("learn_fact") => {
                    if let Some(id) = string(map, "fact_id") {
                        result.push(Effect::LearnFact(id.to_string()));
                    }
                }
                Some("establish_deduction") => {
                    if let Some(id) = string(map, "deduction_id") {
                        result.push(Effect::EstablishDeduction(id.to_string()));
                    }
                }
                Some("move")
                    if allow_route_effects
                        && string(map, "setting")
                            .is_some_and(|setting| setting.starts_with("param")) => {}
                Some("move")
                    if string(map, "setting")
                        .is_some_and(|setting| setting.starts_with("param")) =>
                {
                    self.unsupported(
                        file,
                        &format!("{pointer}/effects/{index}"),
                        "playability.unsupported_effect",
                        "parameterized movement is only modeled for the built-in route command",
                    );
                }
                // A `move` effect naming a literal (non-parameterized)
                // setting id whose `subjects` include `player` moves the
                // one location `State` tracks (`state.location`), exactly
                // like the route-move machinery does.
                Some("move")
                    if string(map, "setting")
                        .is_some_and(|setting| !setting.starts_with("param"))
                        && strings(field(map, "subjects"))
                            .iter()
                            .any(|s| s == "player") =>
                {
                    if let Some(setting) = string(map, "setting") {
                        result.push(Effect::Move(setting.to_string()));
                    }
                }
                // A fixed-destination `move` whose subjects are *only*
                // non-player entities (e.g. an NPC) has no effect on
                // anything the search tracks: `subject_locations` is a
                // fixed *initial* fact populated once at model-build time,
                // never updated dynamically, and confirmed (see #89) to be
                // the only per-entity location the search consults. Since
                // no fact/deduction/predicate depends on an NPC's
                // post-move location specifically, this is a genuine no-op
                // rather than an unsupported construct -- flagging it would
                // incorrectly poison the whole trigger (and its other,
                // perfectly modeled effects) as unsupported.
                Some("move")
                    if string(map, "setting")
                        .is_some_and(|setting| !setting.starts_with("param")) => {}
                Some("describe") => {}
                Some(operation) => self.unsupported(
                    file,
                    &format!("{pointer}/effects/{index}"),
                    "playability.unsupported_effect",
                    &format!("effect `{operation}` is outside static analysis"),
                ),
                None => {}
            }
        }
        result
    }

    fn unsupported(&mut self, file: &SourceFile, pointer: &str, code: &str, message: &str) {
        self.unsupported.push(Unsupported {
            code: code.to_string(),
            message: message.to_string(),
            path: file.path.clone(),
            pointer: pointer.to_string(),
            range: locate_pointer(file, pointer),
            // Every call site that reaches this helper turns the flagged
            // construct into something the search can never satisfy: a
            // `Predicate::Never`, an unsatisfiable `entity.*` requirement,
            // or (via the `unsupported_commands`/`unsupported_triggers`
            // tracking around the calls in `read_commands`/`read_triggers`)
            // a fully filtered-out command/trigger. The two call sites that
            // instead leave a live, ambient construct behind (`pattern()`'s
            // authored-actor/owner failures) patch `search_excluded` back
            // to `false` immediately after, once the owning fact/trigger id
            // is known.
            search_excluded: true,
            witness_subject: None,
        });
    }

    /// Rewrite facts that are revealed exclusively through a character
    /// testimony's `reveals:` entry so they gate on the `command.question`
    /// action that actually reveals them, instead of being free at action 0.
    /// This mirrors the game-engine reducer, which excludes
    /// `story.testimony_reveals` facts from opening availability.
    fn apply_testimony_gates(&mut self) {
        let gates = std::mem::take(&mut self.testimony_gates);
        for (fact_id, gate) in gates {
            let Some(fact) = self.facts.get_mut(&fact_id) else {
                continue;
            };
            // A fact with its own authored `on`/`when` is already correctly
            // gated; don't override an explicit condition with a synthetic
            // one.
            if fact.on.is_some() {
                continue;
            }
            fact.opening = false;
            let mut bindings = BTreeMap::new();
            bindings.insert("character".to_string(), vec![gate.owner.clone()]);
            let mut topics = Vec::new();
            for requirement in &gate.requires {
                if requirement == "command.question" || requirement == &gate.owner {
                    continue;
                }
                if requirement.starts_with("fact.")
                    || requirement.starts_with("deduction.")
                    || requirement.starts_with("flag.")
                    || requirement.starts_with("trigger.")
                {
                    fact.when.push(Predicate::Has(requirement.clone()));
                } else if requirement.starts_with("setting.")
                    || requirement.starts_with("entity.")
                    || requirement.starts_with("character.")
                    || requirement.starts_with("event.")
                    || requirement.starts_with("answer.")
                {
                    // The testimony's own `requires` uses these as topic
                    // candidates (`command.question`'s `topic` parameter
                    // accepts character/setting/event/entity/deduction
                    // types, and `answer.*` per Format 3.7's
                    // question-topic-eligible knowledge subjects); bind them
                    // the same way a route or parameter pattern binds a
                    // subject so `action_available` applies its existing
                    // reachability/knowledge check.
                    topics.push(requirement.clone());
                } else {
                    fact.when.push(Predicate::Never);
                    self.unsupported.push(Unsupported {
                        code: "playability.unsupported_testimony_requirement".to_string(),
                        message: format!(
                            "testimony requirement `{requirement}` is outside the static playability subset"
                        ),
                        path: fact.item.path.clone(),
                        pointer: fact.item.pointer.clone(),
                        range: fact.item.range,
                        // A `Predicate::Never` gate, same as `predicates()`'s
                        // fallbacks: the fact can never be learned by the
                        // search, so no witness can ever depend on it.
                        search_excluded: true,
                        witness_subject: None,
                    });
                }
            }
            if !topics.is_empty() {
                topics.sort();
                topics.dedup();
                bindings.insert("topic".to_string(), topics);
            }
            fact.on = Some(ActionPattern {
                command: "command.question".to_string(),
                bindings,
            });
        }
    }

    fn normalize(&mut self) {
        let mut input_sets = BTreeMap::<Vec<String>, Vec<String>>::new();
        for deduction in self
            .deductions
            .values()
            .filter(|deduction| !deduction.inputs.is_empty())
        {
            let mut inputs = deduction.inputs.clone();
            inputs.sort();
            inputs.dedup();
            input_sets
                .entry(inputs)
                .or_default()
                .push(deduction.item.id.clone());
        }
        for ids in input_sets.values().filter(|ids| ids.len() > 1) {
            for id in ids {
                let deduction = &self.deductions[id];
                self.unsupported.push(Unsupported {
                    code: "playability.unsupported_ambiguous_deduction".to_string(),
                    message: format!(
                        "deduction `{id}` shares its input set with {}; runtime deduction selection may be ambiguous",
                        ids.iter()
                            .filter(|candidate| *candidate != id)
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    path: deduction.item.path.clone(),
                    pointer: format!("{}/inputs", deduction.item.pointer),
                    range: deduction.inputs_range,
                    // Unlike every other unsupported construct, an
                    // ambiguous deduction stays fully live in the search --
                    // it is a genuine `command.deduce`/auto-established
                    // deduction the model can and does establish, just one
                    // whose runtime selection isn't guaranteed to match.
                    // Only relevant to a witness that actually establishes
                    // it.
                    search_excluded: false,
                    witness_subject: Some(id.clone()),
                });
            }
        }
        self.routes
            .sort_by(|a, b| (&a.id, &a.from, &a.to).cmp(&(&b.id, &b.from, &b.to)));
        self.unsupported
            .sort_by(|a, b| (&a.path, &a.pointer, &a.code).cmp(&(&b.path, &b.pointer, &b.code)));
        self.precompute_elapsed_equivalence_horizon();
        self.precompute_patterns();
        self.precompute_takeable_entities();
        self.build_subject_witnesses();
        self.build_requirement_locations();
        self.build_fact_acquisition_locations();
        self.build_hop_counts();
    }

    /// Build `takeable_entities` -- see its field doc comment. Must run
    /// after `precompute_patterns` (consumes its bindings) but has no
    /// ordering dependency on the `build_*` steps that follow.
    fn precompute_takeable_entities(&mut self) {
        let mut referenced = BTreeSet::new();
        // Deliberately trigger `on` bindings only, not every fact `on`
        // binding (i.e. not the full `precomputed_patterns` set): an owned
        // fact's `on` pattern binds `owner`, which resolves to the fact's
        // own entity -- co-location with itself is automatic wherever that
        // entity currently is, portable or not, so it never needs carrying.
        // In quiet_kennel this distinction is the whole ballgame: every one
        // of its 12 portable entities has a self-referential examine fact
        // and so would appear in `precomputed_patterns`, but only the 4
        // actually gated behind a cross-location trigger binding
        // (`trigger.test_jo_curry_against_sedative_audit` and its
        // siblings -- see narrator-validator#96) ever benefit from being
        // carried. Restricting to trigger bindings keeps the take-action
        // fan-out proportional to the real cross-room need instead of the
        // story's total portable-entity count.
        for pattern in self
            .triggers
            .values()
            .filter_map(|trigger| trigger.on.as_ref())
        {
            referenced.extend(pattern.bindings.values().flatten().cloned());
        }
        for row in &self.solution_answer_rows {
            referenced.extend(row.iter().cloned());
        }
        for step in &self.solve_steps {
            for row in &step.rows {
                match row {
                    SolveRow::NOfM { pool, .. } => referenced.extend(pool.iter().cloned()),
                    SolveRow::Ordered { cards } => referenced.extend(cards.iter().cloned()),
                }
            }
        }
        self.takeable_entities = self
            .portable_entities
            .intersection(&referenced)
            .cloned()
            .collect();
    }

    /// Build `hop_counts` -- see its field doc comment.
    fn build_hop_counts(&mut self) {
        let mut settings = BTreeSet::new();
        for route in &self.routes {
            settings.insert(route.from.clone());
            settings.insert(route.to.clone());
        }
        let mut hops = BTreeMap::new();
        for start in &settings {
            let mut distance = BTreeMap::new();
            distance.insert(start.clone(), 0u32);
            let mut queue = std::collections::VecDeque::new();
            queue.push_back(start.clone());
            while let Some(current) = queue.pop_front() {
                let current_distance = distance[&current];
                for route in &self.routes {
                    let neighbor = if route.from == current {
                        Some(route.to.as_str())
                    } else if route.bidirectional && route.to == current {
                        Some(route.from.as_str())
                    } else {
                        None
                    };
                    if let Some(neighbor) = neighbor {
                        if !distance.contains_key(neighbor) {
                            distance.insert(neighbor.to_string(), current_distance + 1);
                            queue.push_back(neighbor.to_string());
                        }
                    }
                }
            }
            for (setting, steps) in distance {
                hops.insert((start.clone(), setting), steps);
            }
        }
        self.hop_counts = hops;
    }

    /// The BFS hop count from `from` to `to` over `self.routes`, or `1` if
    /// no path is known (unconnected, or one side isn't a route-graph
    /// setting at all) -- never `0` unless `from == to`, so this always
    /// carries at least the old flat "elsewhere" penalty even when it can't
    /// say anything more precise.
    fn hop_count(&self, from: &str, to: &str) -> u32 {
        if from == to {
            return 0;
        }
        self.hop_counts
            .get(&(from.to_string(), to.to_string()))
            .copied()
            .unwrap_or(1)
    }

    /// Build `fact_acquisition_locations` -- see its field doc comment.
    fn build_fact_acquisition_locations(&mut self) {
        let mut locations: BTreeMap<String, String> = self
            .requirement_locations
            .iter()
            .filter(|(id, _)| id.starts_with("fact."))
            .map(|(id, location)| (id.clone(), location.clone()))
            .collect();
        for fact in self.facts.values() {
            if locations.contains_key(&fact.item.id) {
                continue;
            }
            let Some(pattern) = &fact.on else {
                continue;
            };
            let mut resolved: Option<String> = None;
            let mut consistent = true;
            for id in pattern.bindings.values().flatten() {
                let binding_location = if id.starts_with("setting.") {
                    Some(id.clone())
                } else if id.starts_with("entity.") || id.starts_with("character.") {
                    self.subject_locations.get(id).cloned()
                } else {
                    None
                };
                let Some(binding_location) = binding_location else {
                    consistent = false;
                    break;
                };
                match &resolved {
                    None => resolved = Some(binding_location),
                    Some(existing) if existing == &binding_location => {}
                    Some(_) => {
                        consistent = false;
                        break;
                    }
                }
            }
            if consistent {
                if let Some(location) = resolved {
                    locations.insert(fact.item.id.clone(), location);
                }
            }
        }
        self.fact_acquisition_locations = locations;
    }

    /// Build `requirement_locations`: for every requirement ID produced by
    /// exactly one trigger (via `SetFlag` or that trigger's `facts` list)
    /// whose `when` names exactly one `at: X` predicate, record `X`. Used
    /// by `end_shortfall` for provenance credit -- a zero/near-zero
    /// requirement shortfall gives the heuristic nothing to climb down
    /// until the requirement is already met, so being at the one place
    /// that can ever produce it is a cheap, always-correct partial signal
    /// (never wrong: the trigger genuinely cannot fire from anywhere
    /// else).
    fn build_requirement_locations(&mut self) {
        let mut producers = BTreeMap::<String, BTreeSet<String>>::new();
        for trigger in self.triggers.values() {
            let mut produced: Vec<&str> = trigger
                .effects
                .iter()
                .filter_map(|effect| match effect {
                    Effect::SetFlag(id) => Some(id.as_str()),
                    _ => None,
                })
                .collect();
            produced.extend(trigger.facts.iter().map(String::as_str));
            for id in produced {
                producers
                    .entry(id.to_string())
                    .or_default()
                    .insert(trigger.item.id.clone());
            }
        }
        let mut locations = BTreeMap::new();
        for (id, trigger_ids) in &producers {
            if trigger_ids.len() != 1 {
                continue;
            }
            let trigger_id = trigger_ids.iter().next().unwrap();
            let Some(trigger) = self.triggers.get(trigger_id) else {
                continue;
            };
            let at_locations: Vec<&str> = trigger
                .when
                .iter()
                .filter_map(|predicate| match predicate {
                    Predicate::At(location) => Some(location.as_str()),
                    _ => None,
                })
                .collect();
            if let [location] = at_locations[..] {
                locations.insert(id.clone(), location.to_string());
            }
        }
        self.requirement_locations = locations;
    }

    /// Build `subject_witnesses`: for every card subject ID (including
    /// `answer.*`), the facts/deductions that establish knowledge of it.
    /// Mirrors #79's "known via facts/deductions that reference them"
    /// semantics uniformly across card kinds.
    fn build_subject_witnesses(&mut self) {
        let mut witnesses = BTreeMap::<String, BTreeSet<String>>::new();
        for fact in self.facts.values() {
            let mut subjects = fact.about.clone();
            if let Some(owner) = &fact.item.owner {
                subjects.push(owner.clone());
            }
            if let Ok(parsed) = crate::parse_reference_text(&fact.statement) {
                for segment in &parsed.segments {
                    if let crate::ReferenceTextSegment::Reference { expression } = segment {
                        subjects.push(expression.target_id.clone());
                    }
                }
            }
            subjects.sort();
            subjects.dedup();
            for subject in subjects {
                witnesses
                    .entry(subject)
                    .or_default()
                    .insert(fact.item.id.clone());
            }
        }
        for deduction in self.deductions.values() {
            let mut subjects = Vec::new();
            if let Ok(parsed) = crate::parse_reference_text(&deduction.conclusion) {
                for segment in &parsed.segments {
                    if let crate::ReferenceTextSegment::Reference { expression } = segment {
                        subjects.push(expression.target_id.clone());
                    }
                }
            }
            subjects.sort();
            subjects.dedup();
            for subject in subjects {
                witnesses
                    .entry(subject)
                    .or_default()
                    .insert(deduction.item.id.clone());
            }
        }
        self.subject_witnesses = witnesses;
    }

    /// True if `subject` (a card ID, including `answer.*`) can be
    /// established as known in `state`.
    ///
    /// `answer.*` subjects are the simple case: they have no world-state
    /// channel, so knowability is purely derived from the facts/deductions
    /// that reference them (`subject_witnesses`).
    ///
    /// Entity/character/setting subjects are handled by owner-witness plus
    /// location credit where the model tracks a physical position for the
    /// subject at all (`subject_locations`, populated from an authored
    /// `initial.location`/`initial.container`): being co-located with the
    /// subject, or holding an explicit witness, establishes it. A portable
    /// entity carried in `state.inventory` also counts, even away from its
    /// authored starting location -- see `State::inventory`'s doc comment.
    /// Most characters (and every setting, which is never itself a
    /// `subject_locations` key) have no authored physical position in this
    /// static model at all; those are treated as ambient background
    /// knowledge -- the named cast and the map are known from the outset,
    /// the same as before this ticket, when the legacy path never gated
    /// entity/character/setting subject knowledge in the first place.
    fn subject_known(&self, state: &State, subject: &str) -> bool {
        if subject.starts_with("answer.") {
            return self
                .subject_witnesses
                .get(subject)
                .is_some_and(|witnesses| witnesses.iter().any(|witness| has(state, witness)));
        }
        if state.location == subject {
            return true;
        }
        if state.inventory.contains(subject) {
            return true;
        }
        if let Some(location) = self.subject_locations.get(subject) {
            return location == &state.location
                || self
                    .subject_witnesses
                    .get(subject)
                    .is_some_and(|witnesses| witnesses.iter().any(|witness| has(state, witness)));
        }
        true
    }

    /// True if a row's knowledge gate is currently satisfied.
    fn row_satisfied(&self, row: &SolveRow, state: &State) -> bool {
        match row {
            SolveRow::NOfM { n, pool } => {
                pool.iter()
                    .filter(|subject| self.subject_known(state, subject))
                    .count()
                    >= *n
            }
            SolveRow::Ordered { cards } => cards
                .iter()
                .all(|subject| self.subject_known(state, subject)),
        }
    }

    /// Goal-distance estimate for `search_from`'s per-step chaining legs:
    /// the sum, over `solve_steps[index].rows`, of each row's shortfall --
    /// how far that row still is from `row_satisfied`. Built from the exact
    /// same `subject_known` calls `row_satisfied` uses, so the heuristic and
    /// the actual goal check can never disagree about what "satisfied"
    /// means. Not admissible (a claim can satisfy more than one row's
    /// shortfall at once, or a deduction can cost more than one action), but
    /// `search_from` doesn't require admissibility -- see its own doc
    /// comment.
    fn step_shortfall(&self, state: &State, index: usize) -> u32 {
        let Some(step) = self.solve_steps.get(index) else {
            return 0;
        };
        step.rows
            .iter()
            .map(|row| self.row_shortfall(row, state))
            .sum()
    }

    /// Goal-distance estimate for `search_from`'s per-end bounded-recovery
    /// legs: the sum of every component `end_satisfied` checks, each
    /// reduced to a cheap shortfall so the heap can favor states that are
    /// actually closer to the end rather than merely cheaper so far. Built
    /// from the same fields `end_satisfied` reads, so the heuristic and the
    /// actual goal check can never disagree about what "satisfied" means.
    /// Not admissible (see `step_shortfall`'s doc comment for why that's
    /// fine -- `search_from`'s soundness never depends on exploration
    /// order, only on `goal` itself, which this heuristic never touches).
    fn end_shortfall(&self, end: &EndRule, state: &State) -> u32 {
        let missing_requirements = end
            .requirements
            .iter()
            .filter(|id| !has(state, id))
            .map(|id| {
                // Provenance credit: if this requirement can only ever be
                // produced by a trigger pinned to one location, being
                // somewhere else is itself part of the shortfall -- see
                // `requirement_locations`'s doc comment for why this can
                // never overclaim.
                let away_from_producer = self
                    .requirement_locations
                    .get(id)
                    .map_or(0, |location| self.hop_count(&state.location, location));
                1 + away_from_producer
            })
            .sum::<u32>();
        let solution_shortfall = u32::from(end.solution_condition && !state.solution_solved);
        let points_shortfall = end.minimum_points.saturating_sub(state.score) as u32;
        let time_shortfall = end.at_or_after.map_or(0, |threshold| {
            threshold.saturating_sub(self.initial_minutes.saturating_add(state.elapsed))
        });
        missing_requirements + solution_shortfall + points_shortfall + time_shortfall
    }

    /// Goal-distance estimate for a single still-unknown card subject,
    /// feeding `row_shortfall` -- see narrator-validator#94. `row_shortfall`
    /// previously reduced every card to a flat, binary "known or not"
    /// signal, which degrades to near-uniform-cost search over a large
    /// branching factor. This gives partial credit toward whichever witness
    /// is cheapest to reach: being at the one place a witnessing fact's gate
    /// can be triggered, or having already met some of that fact's
    /// testimony/question prerequisites, or a deduction's dependencies.
    ///
    /// MUST return exactly 0 iff `subject_known` is true -- `row_shortfall`
    /// (and thus `step_shortfall`/`search_from`'s soundness argument) relies
    /// on that invariant to keep "0 iff satisfied" true all the way up.
    fn subject_shortfall(&self, state: &State, subject: &str) -> u32 {
        if self.subject_known(state, subject) {
            return 0;
        }
        // Entity/character subjects (step 2's cards, e.g. `entity.culvert_corner`)
        // aren't established via `subject_witnesses` the way `answer.*`/
        // `fact.*` subjects are -- `subject_known` gates them on
        // co-location (or an owning witness) directly against
        // `subject_locations`. Mirror that here instead of falling through
        // to the witness-based path below, which would find no witnesses
        // for most of these and collapse back to a flat 1.
        if subject.starts_with("entity.") || subject.starts_with("character.") {
            let away = self
                .subject_locations
                .get(subject)
                .map_or(0, |location| self.hop_count(&state.location, location));
            return 1 + away;
        }
        let extra = self
            .subject_witnesses
            .get(subject)
            .into_iter()
            .flatten()
            .map(|witness| self.witness_extra_cost(state, witness))
            .min()
            .unwrap_or(0);
        1 + extra
    }

    /// The extra-cost term `subject_shortfall` adds on top of its flat 1 for
    /// a specific witness (a fact or deduction id) that would establish the
    /// subject. See `subject_shortfall`'s doc comment.
    fn witness_extra_cost(&self, state: &State, witness: &str) -> u32 {
        if let Some(fact) = self.facts.get(witness) {
            let away_from_acquisition = self
                .fact_acquisition_locations
                .get(witness)
                .map_or(0, |location| self.hop_count(&state.location, location));
            let unmet_prerequisites = fact
                .when
                .iter()
                .filter(|predicate| matches!(predicate, Predicate::Has(id) if !has(state, id)))
                .count() as u32;
            return away_from_acquisition + unmet_prerequisites;
        }
        if let Some(deduction) = self.deductions.get(witness) {
            return deduction
                .dependencies
                .iter()
                .filter(|id| !has(state, id))
                .count() as u32;
        }
        0
    }

    /// How far `row` still is from `row_satisfied`: for `Ordered`, the sum
    /// of `subject_shortfall` over every card; for `NOfM`, the sum of the
    /// `n - known` smallest shortfalls among the currently-unknown pool
    /// members (the cheapest members left to acquire, since any `n` of the
    /// pool satisfy the row). Zero iff `row_satisfied`: every unknown
    /// subject's shortfall is `subject_shortfall`, which is >=1 whenever
    /// `subject_known` is false, so a nonzero `needed` count always sums to
    /// something positive, and `needed == 0` exactly tracks
    /// `row_satisfied`'s own `known >= n` check.
    fn row_shortfall(&self, row: &SolveRow, state: &State) -> u32 {
        match row {
            SolveRow::NOfM { n, pool } => {
                let mut shortfalls: Vec<u32> = pool
                    .iter()
                    .filter(|subject| !self.subject_known(state, subject))
                    .map(|subject| self.subject_shortfall(state, subject))
                    .collect();
                let known = pool.len() - shortfalls.len();
                let needed = n.saturating_sub(known);
                if needed == 0 {
                    return 0;
                }
                shortfalls.sort_unstable();
                shortfalls.into_iter().take(needed).sum()
            }
            SolveRow::Ordered { cards } => cards
                .iter()
                .map(|subject| self.subject_shortfall(state, subject))
                .sum(),
        }
    }

    /// True if `subject` could ever be established as known by *some*
    /// play-through, independent of the current state -- the static tier
    /// used to detect a hard-unlearnable answer (`subject_witnesses` has no
    /// entry at all, so no fact or deduction in the entire catalog
    /// references it).
    fn subject_learnable(&self, subject: &str) -> bool {
        if subject.starts_with("answer.") {
            return self
                .subject_witnesses
                .get(subject)
                .is_some_and(|witnesses| !witnesses.is_empty());
        }
        true
    }

    fn precompute_elapsed_equivalence_horizon(&mut self) {
        // Absolute clock values can affect the model only while an authored
        // predicate or terminal threshold can still change truth value. One
        // minute beyond the latest boundary, elapsed values are observationally
        // equivalent except for delayed work, which search_state_key preserves
        // as time remaining.
        let predicate_thresholds = self
            .facts
            .values()
            .flat_map(|fact| &fact.when)
            .chain(self.triggers.values().flat_map(|trigger| &trigger.when))
            .filter_map(|predicate| match predicate {
                Predicate::TimeAfter(value)
                | Predicate::TimeEqual(value)
                | Predicate::TimeBefore(value) => Some(*value),
                _ => None,
            });
        let latest = predicate_thresholds
            .chain(self.ends.iter().filter_map(|end| end.at_or_after))
            .max();
        self.elapsed_equivalence_horizon = latest.map_or(0, |threshold| {
            threshold
                .saturating_sub(self.initial_minutes)
                .saturating_add(1)
        });
    }

    fn search_state_key(&self, state: &State) -> State {
        let mut key = state.clone();
        let canonical_elapsed = state.elapsed.min(self.elapsed_equivalence_horizon);
        key.elapsed = canonical_elapsed;
        for pending in &mut key.pending {
            let remaining = pending.due.saturating_sub(state.elapsed);
            pending.due = canonical_elapsed.saturating_add(remaining);
        }
        key
    }

    fn precompute_patterns(&mut self) {
        let mut patterns = BTreeMap::<String, ActionPattern>::new();
        for pattern in self
            .facts
            .values()
            .filter_map(|fact| fact.on.as_ref())
            .chain(
                self.triggers
                    .values()
                    .filter_map(|trigger| trigger.on.as_ref()),
            )
        {
            patterns.insert(pattern_key(pattern), pattern.clone());
        }
        let uncovered_commands = self
            .commands
            .values()
            .filter(|command| {
                !(command.requires_binding
                    || self.unsupported_commands.contains(&command.id)
                    || matches!(command.id.as_str(), "command.claim" | "command.deduce")
                    || (command.id == "command.solve" && self.solve_action.is_some())
                    || patterns
                        .values()
                        .any(|pattern| pattern.command == command.id))
            })
            .map(|command| command.id.clone())
            .collect::<Vec<_>>();
        for command_id in uncovered_commands {
            patterns.insert(
                command_id.clone(),
                ActionPattern {
                    command: command_id,
                    bindings: BTreeMap::new(),
                },
            );
        }
        self.precomputed_patterns = patterns.into_values().collect();
    }

    fn solution_equivalent_deductions(&self) -> BTreeSet<String> {
        self.deductions
            .values()
            .filter(|deduction| {
                !deduction.solves.is_empty()
                    && self.solution_answer_rows.iter().any(|row| {
                        !row.is_empty()
                            && row.iter().all(|answer| deduction.solves.contains(answer))
                    })
            })
            .map(|deduction| deduction.item.id.clone())
            .collect()
    }

    fn deduction_graph_analysis(&self) -> DeductionGraphAnalysis {
        fn depth(
            model: &Model,
            id: &str,
            visiting: &mut BTreeSet<String>,
            memo: &mut BTreeMap<String, usize>,
        ) -> usize {
            if let Some(depth) = memo.get(id) {
                return *depth;
            }
            if !visiting.insert(id.to_string()) {
                return 0;
            }
            let result = model.deductions.get(id).map_or(0, |deduction| {
                1 + deduction
                    .dependencies
                    .iter()
                    .filter(|dependency| model.deductions.contains_key(*dependency))
                    .map(|dependency| depth(model, dependency, visiting, memo))
                    .max()
                    .unwrap_or(0)
            });
            visiting.remove(id);
            memo.insert(id.to_string(), result);
            result
        }

        let mut memo = BTreeMap::new();
        let maximum_depth = self
            .deductions
            .keys()
            .map(|id| depth(self, id, &mut BTreeSet::new(), &mut memo))
            .max()
            .unwrap_or(0);
        let roots = self
            .deductions
            .values()
            .flat_map(|deduction| deduction.dependencies.iter().cloned())
            .collect::<BTreeSet<_>>();
        let mut largest_cascade_root = None;
        let mut largest_cascade = Vec::new();
        for root in roots {
            let mut reached = BTreeSet::new();
            loop {
                let round = self
                    .deductions
                    .values()
                    .filter(|deduction| {
                        !reached.contains(&deduction.item.id)
                            && deduction.dependencies.iter().any(|dependency| {
                                dependency == &root || reached.contains(dependency)
                            })
                    })
                    .map(|deduction| deduction.item.id.clone())
                    .collect::<Vec<_>>();
                if round.is_empty() {
                    break;
                }
                reached.extend(round);
            }
            let candidate = reached.into_iter().collect::<Vec<_>>();
            if candidate.len() > largest_cascade.len() {
                largest_cascade_root = Some(root);
                largest_cascade = candidate;
            }
        }
        DeductionGraphAnalysis {
            maximum_depth,
            largest_cascade_size: largest_cascade.len(),
            largest_cascade_root,
            largest_cascade,
        }
    }

    /// True once every question this search could ever answer with
    /// `Proved` has already been answered that way, so continuing to pop
    /// the queue can only spend budget re-deriving states that no
    /// unresolved question still needs. The queue is a min-heap on
    /// `(actions, elapsed)` (see `QueueNode`/`queue_key`), so the first
    /// time any end or solve step is proved is already its minimal-action
    /// witness; nothing popped later could improve on it. This must NOT
    /// short-circuit a genuine `NotProved`/`Inconclusive` outcome -- those
    /// still require draining the queue (or hitting a bound) so the
    /// `bounded`/`reached_states` diagnostics stay accurate, which is why
    /// every branch here requires the corresponding question to already be
    /// `Proved`, never merely "no longer worth pursuing".
    fn search_fully_settled(
        &self,
        proofs: &BTreeMap<String, Node>,
        step_progress: &[Option<u32>],
        answerable: &Option<(u32, Vec<String>)>,
    ) -> bool {
        if !self
            .ends
            .iter()
            .all(|end| proofs.contains_key(&end.item.id))
        {
            return false;
        }
        if self.solve_steps.is_empty() {
            answerable.is_some()
        } else {
            step_progress.iter().all(Option::is_some)
        }
    }

    /// Applies one candidate action to `node`, producing the successor
    /// `Node` (or `None` if it overshoots `MAX_ELAPSED_MINUTES`). Shared by
    /// the main search and `search_from` so both expand a state in exactly
    /// the same way.
    fn expand(
        &self,
        node: &Node,
        action: CandidateAction,
        auto_facts: bool,
        auto_deductions: bool,
    ) -> Option<Node> {
        let mut next = node.clone();
        next.actions += 1;
        let before_elapsed = next.state.elapsed;
        let before_unlocks = next.unlocks.clone();
        let before_deductions = next.state.deductions.clone();
        if action.kind == "route" {
            next.route_actions += 1;
        }
        // Record any `unsupported_triggers` this action's pattern would
        // genuinely have fired (its `on` matches and `when` holds against
        // the state *before* this action, same snapshot `apply_action`
        // uses for its own real trigger matching below) -- narrator-
        // validator#88 needs to know this witness actually took the action
        // that shadows the trigger's un-modeled effect, not merely that
        // the trigger exists somewhere in the story.
        next.shadowed_triggers.extend(
            self.triggers
                .values()
                .filter(|trigger| {
                    self.unsupported_triggers.contains(&trigger.item.id)
                        && trigger
                            .on
                            .as_ref()
                            .is_some_and(|pattern| pattern_matches(pattern, &action.pattern))
                        && predicates_hold(&trigger.when, &node.state, self.initial_minutes)
                })
                .map(|trigger| trigger.item.id.clone()),
        );
        self.apply_action(&mut next.state, &action, &mut next.unlocks, auto_facts);
        self.settle(
            &mut next.state,
            Some(&action),
            &mut next.unlocks,
            auto_facts,
            auto_deductions,
        );
        let newly_established = next
            .state
            .deductions
            .difference(&before_deductions)
            .cloned()
            .collect::<BTreeSet<_>>();
        self.apply_point_awards(
            &mut next.state,
            &action,
            &newly_established,
            &mut next.unlocks,
        );
        if next.state.elapsed > MAX_ELAPSED_MINUTES {
            return None;
        }
        let gained = next
            .unlocks
            .difference(&before_unlocks)
            .cloned()
            .collect::<Vec<_>>();
        let elapsed = next.state.elapsed - before_elapsed;
        if action.kind != "route" {
            next.wait_minutes += elapsed;
        }
        next.steps.push(PlayabilityStep {
            kind: action.kind.to_string(),
            action: action.id,
            from: action.from,
            to: action.to,
            elapsed_minutes: elapsed,
            unlocks: gained,
        });
        Some(next)
    }

    /// A fresh, independently `budget`-bounded search for the first
    /// (minimal-action) node reachable from `seed` satisfying `goal`.
    /// `seed` must be a node genuinely reached by real actions (a main
    /// search's own checkpoint, or an earlier leg's result), which is what
    /// makes any `Some` result a genuine playthrough witness rather than a
    /// shortcut that could manufacture a false proof.
    fn search_from(
        &self,
        seed: Node,
        auto_facts: bool,
        auto_deductions: bool,
        budget: usize,
        heuristic: impl Fn(&Node) -> u32,
        goal: impl Fn(&Node) -> bool,
    ) -> Option<Node> {
        if goal(&seed) {
            return Some(seed);
        }
        let mut queue = BinaryHeap::new();
        let mut best = BTreeMap::<State, Vec<(u32, u32)>>::new();
        best.entry(self.search_state_key(&seed.state))
            .or_default()
            .push((seed.actions, seed.state.elapsed));
        let seed_h = heuristic(&seed);
        queue.push(HeuristicQueueNode(seed, seed_h));
        let mut explored = 0usize;
        while let Some(HeuristicQueueNode(node, _)) = queue.pop() {
            if explored >= budget {
                break;
            }
            explored += 1;
            if node.actions >= MAX_ACTIONS || node.state.elapsed >= MAX_ELAPSED_MINUTES {
                continue;
            }
            for action in self.actions(&node.state, auto_facts, auto_deductions) {
                let Some(next) = self.expand(&node, action, auto_facts, auto_deductions) else {
                    continue;
                };
                let cost = (next.actions, next.state.elapsed);
                let costs = best.entry(self.search_state_key(&next.state)).or_default();
                if costs
                    .iter()
                    .any(|known| known.0 <= cost.0 && known.1 <= cost.1)
                {
                    continue;
                }
                costs.retain(|known| !(cost.0 <= known.0 && cost.1 <= known.1));
                costs.push(cost);
                if goal(&next) {
                    return Some(next);
                }
                let next_h = heuristic(&next);
                queue.push(HeuristicQueueNode(next, next_h));
            }
        }
        None
    }

    /// The per-end acceptance check shared by `search_from_multi_end` (and,
    /// before narrator-validator#99, by the single-goal `search_from` leg it
    /// replaced): `candidate` is accepted as a witness for `end` only if the
    /// solve session it was reached in has actually concluded.
    /// `concluded_via_failure`'s `next_step == 0` reset can re-enter a solve
    /// attempt (the model doesn't forbid retrying after a reset while
    /// attempts remain), so a later node reached from that seed can be
    /// genuinely mid-sequence again -- this mirrors the main search's
    /// `solve_session_concluded` gate so such an intermediate coincidence
    /// (flags that happen to satisfy `end` before the runtime would actually
    /// let the game conclude) is never accepted as a witness.
    fn end_recovery_satisfied(&self, end: &EndRule, candidate: &Node) -> bool {
        let solve_session_concluded = self.solve_steps.is_empty()
            || candidate.state.next_step == 0
            || candidate.state.next_step as usize >= self.solve_steps.len();
        candidate.actions > 0
            && solve_session_concluded
            && self.end_satisfied(end, &candidate.state)
    }

    /// Like `search_from`, but resolves every still-unresolved end in
    /// `ends` from one `seed`, instead of one fixed goal at a time.
    ///
    /// Each end gets its own independent best-first priority queue --
    /// exactly the ordering a dedicated `search_from` call for that end
    /// alone would use -- run in ROUND-ROBIN lockstep (one pop per
    /// still-active end per round) so no single end's queue can starve
    /// another's turn. A single shared `min(shortfall)` priority, and a
    /// single shared queue fanning out per-end priorities into one combined
    /// pop order, were both tried and reverted during narrator-validator#99's
    /// development: either way, letting different ends' priority *values*
    /// compete directly for the front of one frontier lets a permanently
    /// unreachable end -- one gated behind a `Predicate::Never` unsupported
    /// construct still reports a small, never-quite-zero shortfall that
    /// looks perpetually "almost there" -- dominate essentially every pop
    /// and starve a genuinely reachable end's own convergence;
    /// wrong_floor's `end.elias_moves_sam` regressed from Proved to
    /// Inconclusive under both. Round-robin turn-taking never compares one
    /// end's priority number against another's, so it can't be misled that
    /// way, and it reproduces a dedicated `search_from`'s exact behavior
    /// whenever only one end remains (the common case).
    ///
    /// The dominance-pruning `best` map is shared across every end (a
    /// state's (actions, elapsed) cost is objectively the same regardless
    /// of which end is being sought, so pruning by it is end-independent),
    /// and `expanded` ensures a given (state, cost) is only ever run
    /// through `actions`/`expand` once, no matter how many ends' queues
    /// independently discover it -- when it IS expanded, every successor is
    /// pushed into every remaining end's own queue at that same moment
    /// (with that end's own shortfall as priority), so a later duplicate
    /// pop of an already-expanded state costs nothing beyond a cheap set
    /// lookup. This is what shares the actually-expensive part
    /// (`actions`/`expand`) across ends without sharing exploration order --
    /// where the real per-(seed, end) redundancy the architect identified
    /// lives. `explored` (states actually run through `actions`/`expand`)
    /// is bounded by `budget` in total across every end, same spirit as
    /// `search_from`. Soundness is unchanged: every entry in the returned
    /// map is a node genuinely reached from `seed` via real actions
    /// (`expand`) that independently passes `end_recovery_satisfied` for
    /// that end.
    fn search_from_multi_end(
        &self,
        seed: Node,
        auto_facts: bool,
        auto_deductions: bool,
        budget: usize,
        ends: &[&EndRule],
    ) -> BTreeMap<String, Node> {
        let mut found = BTreeMap::<String, Node>::new();
        let mut remaining = Vec::new();
        for end in ends {
            if self.end_recovery_satisfied(end, &seed) {
                found.insert(end.item.id.clone(), seed.clone());
            } else {
                remaining.push(*end);
            }
        }
        if remaining.is_empty() {
            return found;
        }
        let mut queues = Vec::with_capacity(remaining.len());
        for end in &remaining {
            let mut queue = BinaryHeap::new();
            let h = self.end_shortfall(end, &seed.state);
            queue.push(HeuristicQueueNode(seed.clone(), h));
            queues.push(queue);
        }
        let mut best = BTreeMap::<State, Vec<(u32, u32)>>::new();
        best.entry(self.search_state_key(&seed.state))
            .or_default()
            .push((seed.actions, seed.state.elapsed));
        let mut expanded = BTreeSet::<(State, u32, u32)>::new();
        let mut explored = 0usize;
        // A round-robin round: one pop attempt per still-unresolved end.
        // An end's queue coming up empty on a given round does NOT retire
        // it -- another end's turn later in this same round (or a later
        // round) can still push fresh entries into it via the multi-push
        // below, so only a round in which *no* end anywhere made any
        // progress (`any_progress` stays false) proves the frontier is
        // truly, permanently exhausted.
        loop {
            if explored >= budget || remaining.iter().all(|end| found.contains_key(&end.item.id))
            {
                break;
            }
            let mut any_progress = false;
            for index in 0..remaining.len() {
                if found.contains_key(&remaining[index].item.id) {
                    continue;
                }
                if explored >= budget {
                    break;
                }
                let Some(HeuristicQueueNode(node, _)) = queues[index].pop() else {
                    continue;
                };
                any_progress = true;
                let expand_key = (
                    self.search_state_key(&node.state),
                    node.actions,
                    node.state.elapsed,
                );
                if !expanded.insert(expand_key) {
                    // Already expanded via some end's earlier turn -- its
                    // successors were already pushed into every remaining
                    // end's own queue at that moment, so this duplicate pop
                    // needs no further work.
                    continue;
                }
                explored += 1;
                if node.actions >= MAX_ACTIONS || node.state.elapsed >= MAX_ELAPSED_MINUTES {
                    continue;
                }
                for action in self.actions(&node.state, auto_facts, auto_deductions) {
                    let Some(next) = self.expand(&node, action, auto_facts, auto_deductions)
                    else {
                        continue;
                    };
                    let cost = (next.actions, next.state.elapsed);
                    let costs = best.entry(self.search_state_key(&next.state)).or_default();
                    if costs
                        .iter()
                        .any(|known| known.0 <= cost.0 && known.1 <= cost.1)
                    {
                        continue;
                    }
                    costs.retain(|known| !(cost.0 <= known.0 && cost.1 <= known.1));
                    costs.push(cost);
                    for (other_index, end) in remaining.iter().enumerate() {
                        if found.contains_key(&end.item.id) {
                            continue;
                        }
                        if self.end_recovery_satisfied(end, &next) {
                            found.insert(end.item.id.clone(), next.clone());
                            continue;
                        }
                        let h = self.end_shortfall(end, &next.state);
                        queues[other_index].push(HeuristicQueueNode(next.clone(), h));
                    }
                }
            }
            if !any_progress {
                break;
            }
        }
        found
    }

    /// The zero-action starting `Node` for `entry`: opening facts settled
    /// (auto- or claimable, per `auto_facts`), initial flags applied, and
    /// any opening deductions/point-awards already resolved. Shared by
    /// `search`'s per-entry seeding and by whitebox tests that need a
    /// genuine seed for `search_from` without re-deriving this setup.
    fn opening_node(&self, entry: &str, auto_facts: bool, auto_deductions: bool) -> Node {
        let opening_facts = self
            .facts
            .values()
            .filter(|fact| fact.opening)
            .map(|fact| fact.item.id.clone())
            .collect::<BTreeSet<_>>();
        let mut state = State {
            entry: entry.to_string(),
            location: entry.to_string(),
            elapsed: 0,
            facts: auto_facts
                .then_some(opening_facts.clone())
                .unwrap_or_default(),
            available_facts: (!auto_facts).then_some(opening_facts).unwrap_or_default(),
            deductions: BTreeSet::new(),
            flags: self.initial_flags.clone(),
            completed: BTreeSet::new(),
            pending: Vec::new(),
            score: 0,
            point_claims: BTreeMap::new(),
            solution_solved: false,
            next_step: 0,
            attempts_used: 0,
            inventory: BTreeSet::new(),
        };
        let mut unlocks = state.facts.union(&state.available_facts).cloned().collect();
        self.settle(&mut state, None, &mut unlocks, auto_facts, auto_deductions);
        let opening_deductions = state.deductions.clone();
        self.apply_deduction_point_awards(&mut state, &opening_deductions, &mut unlocks);
        Node {
            state,
            actions: 0,
            route_actions: 0,
            wait_minutes: 0,
            steps: Vec::new(),
            unlocks,
            shadowed_triggers: BTreeSet::new(),
        }
    }

    fn search(&self, auto_facts: bool, auto_deductions: bool) -> NotebookPolicyAnalysis {
        let mut queue = BinaryHeap::new();
        // A canonical state can be reached with fewer actions but more elapsed
        // time, or vice versa. Neither dominates the other under both search
        // caps, so retain the Pareto frontier rather than choosing one scalar
        // cost and accidentally weakening a bounded proof.
        let mut best = BTreeMap::<State, Vec<(u32, u32)>>::new();
        let mut reached_states = Vec::new();
        for entry in &self.entries {
            let node = self.opening_node(entry, auto_facts, auto_deductions);
            best.entry(self.search_state_key(&node.state))
                .or_default()
                .push((0, 0));
            reached_states.push(node.state.clone());
            queue.push(QueueNode(node));
        }
        let mut proofs = BTreeMap::<String, Node>::new();
        let mut explored = 0usize;
        let mut bounded = false;
        let solution_equivalent = self.solution_equivalent_deductions();
        let mut answerable: Option<(u32, Vec<String>)> = None;
        // For each Format 3.7 step, the fewest actions in which some
        // reached state has *committed* it (`state.next_step > index`),
        // i.e. some play-through actually took the `solve_step` action for
        // it, not merely that its row cards are individually reachable
        // (see the ticket's conjunctive/simultaneous/sequenced note).
        let mut step_progress = vec![None::<u32>; self.solve_steps.len()];
        // Checkpoint for each Format 3.7 step index: the first (minimal-
        // action) reached node with `next_step == index`, i.e. genuinely
        // ready to attempt that step. Index `solve_steps.len()` holds the
        // checkpoint for "the whole session is concluded" (ready to check
        // ends). Used after the main search to seed a fresh, narrower
        // search per unresolved step instead of re-deriving the whole
        // reachable graph from the story's beginning for each one -- see
        // the chaining block below.
        let mut step_nodes = vec![None::<Node>; self.solve_steps.len() + 1];
        // A Format 3.7 solve session can also conclude by *failing* a step
        // (see `solve_step_fail`'s `next_step = 0` reset in `apply_action`),
        // which is how a graded "botched it" ending like island_retreat's
        // `end.mistaken_accusation` becomes reachable. That reset collides
        // on `next_step == 0` with the story's very first, pre-any-action
        // state, which `step_nodes[0]` already records -- so a
        // failure-concluded state is never captured there. Derived (not
        // recorded opportunistically like `step_nodes`) after both search
        // phases below, from each step's own `step_nodes[index]` readiness
        // checkpoint -- see the derivation loop after the per-step
        // chaining block for why a passive main-loop tracker can't do this
        // reliably.
        let step_fail_action_ids = self
            .solve_steps
            .iter()
            .map(|step| format!("command.solve {} fail", step.item.id))
            .collect::<Vec<_>>();
        let mut step_fail_nodes = vec![None::<Node>; self.solve_steps.len()];
        let unsupported_policy = if !auto_facts
            && !self.facts.is_empty()
            && !self.commands.contains_key("command.claim")
        {
            Some((
                "playability.unsupported_manual_facts",
                "manual fact acquisition requires a supported Claim command",
            ))
        } else if !auto_deductions
            && !self.deductions.is_empty()
            && !self.commands.contains_key("command.deduce")
        {
            Some((
                "playability.unsupported_manual_deductions",
                "manual deduction establishment requires a supported Deduce command",
            ))
        } else {
            None
        };
        while let Some(QueueNode(node)) = queue.pop() {
            if explored >= MAX_EXPLORED_STATES {
                bounded = true;
                break;
            }
            explored += 1;
            for (index, progress) in step_progress.iter_mut().enumerate() {
                if progress.is_none() && (node.state.next_step as usize) > index {
                    *progress = Some(node.actions);
                }
            }
            if let Some(checkpoint) = step_nodes.get_mut(node.state.next_step as usize) {
                if checkpoint.is_none() {
                    *checkpoint = Some(node.clone());
                }
            }
            let established_solution_notes = node
                .state
                .deductions
                .intersection(&solution_equivalent)
                .cloned()
                .collect::<Vec<_>>();
            if !established_solution_notes.is_empty()
                && answerable
                    .as_ref()
                    .map_or(true, |(actions, _)| node.actions < *actions)
            {
                answerable = Some((node.actions, established_solution_notes));
            }
            // While a Format 3.7 solve session is mid-sequence (some but not
            // all steps committed), the runtime hasn't concluded the
            // session yet, so authored end states -- even ones whose
            // `requires` an earlier step's flags already satisfy -- cannot
            // yet terminate the game. This is what lets a later step reach
            // a fuller graded ending instead of the search stopping dead at
            // the first, less-complete one an intermediate flag happens to
            // satisfy.
            let solve_session_concluded = self.solve_steps.is_empty()
                || node.state.next_step == 0
                || node.state.next_step as usize >= self.solve_steps.len();
            if node.actions > 0 && solve_session_concluded {
                if let Some(end) = self
                    .ends
                    .iter()
                    .find(|end| self.end_satisfied(end, &node.state))
                {
                    proofs.entry(end.item.id.clone()).or_insert(node);
                    if self.search_fully_settled(&proofs, &step_progress, &answerable) {
                        break;
                    }
                    continue;
                }
            }
            if node.actions >= MAX_ACTIONS || node.state.elapsed >= MAX_ELAPSED_MINUTES {
                bounded = true;
                continue;
            }
            if self.search_fully_settled(&proofs, &step_progress, &answerable) {
                break;
            }
            for action in self.actions(&node.state, auto_facts, auto_deductions) {
                let Some(next) = self.expand(&node, action, auto_facts, auto_deductions) else {
                    bounded = true;
                    continue;
                };
                let cost = (next.actions, next.state.elapsed);
                let costs = best.entry(self.search_state_key(&next.state)).or_default();
                if costs
                    .iter()
                    .any(|known| known.0 <= cost.0 && known.1 <= cost.1)
                {
                    continue;
                }
                costs.retain(|known| !(cost.0 <= known.0 && cost.1 <= known.1));
                costs.push(cost);
                reached_states.push(next.state.clone());
                queue.push(QueueNode(next));
            }
        }
        // Format 3.7 multi-step Solve: the main search above shares one
        // `MAX_EXPLORED_STATES` budget across every step and end, so a
        // story with many mutually-irrelevant claimable facts can exhaust
        // it long before reaching a step that's actually only a few
        // actions past the last one it proved. Rather than re-deriving
        // the whole reachable graph from the story's beginning again (and
        // hitting the exact same wall), chain a fresh, independently-
        // bounded search per unresolved step, seeded only from a
        // checkpoint the main search (or an earlier leg of this chain)
        // already reached through real actions. Every seed is therefore a
        // genuine playthrough witness, so this can only turn an
        // `Inconclusive` into a `Proved`, never manufacture a false one --
        // and it's only attempted when `bounded` is already true, i.e. the
        // main search's negative result was a budget truncation, not a
        // confirmed exhaustive `NotProved`.
        if bounded && !self.solve_steps.is_empty() {
            let mut carry: Option<Node> = None;
            for index in 0..self.solve_steps.len() {
                if step_progress[index].is_some() {
                    carry = None;
                    continue;
                }
                let Some(seed) = carry.take().or_else(|| step_nodes[index].clone()) else {
                    break;
                };
                let Some(found) = self.search_from(
                    seed,
                    auto_facts,
                    auto_deductions,
                    LEG_MAX_EXPLORED_STATES,
                    |node| self.step_shortfall(&node.state, index),
                    |candidate| candidate.state.next_step as usize > index,
                ) else {
                    break;
                };
                step_progress[index] = Some(found.actions);
                if step_nodes[index + 1].is_none() {
                    step_nodes[index + 1] = Some(found.clone());
                }
                carry = Some(found);
            }
        }
        // Derive `step_fail_nodes`: for each step whose `on_failure` can
        // actually produce a witness-worth effect (mirrors `actions()`'s
        // own `has_failure_effects` gate -- a penalty-only failure can
        // never be the *sole* route to a proof, so skip it rather than
        // waste a leg), a single cheap `search_from` starting at that
        // step's own readiness checkpoint (`step_nodes[index]`, populated
        // by the main search above or the chaining block just above) with
        // a one-hop goal: "the most recent action was this step's
        // `solve_step_fail`". Deriving it this way -- rather than
        // recording it opportunistically while draining the main queue --
        // is necessary because the main search can pop a step's readiness
        // checkpoint without ever popping its fail-successor before
        // hitting `MAX_EXPLORED_STATES` (the successor is pushed, not
        // popped), and because `step_nodes[index]` itself may only exist
        // thanks to the chaining block above, in which case the main
        // search never visited that state at all.
        if bounded && !self.solve_steps.is_empty() {
            for (index, step) in self.solve_steps.iter().enumerate() {
                let has_failure_effects =
                    !step.on_failure.set_flags.is_empty() || step.on_failure.points > 0;
                if !has_failure_effects {
                    continue;
                }
                let Some(seed) = step_nodes[index].clone() else {
                    continue;
                };
                let fail_id = &step_fail_action_ids[index];
                if let Some(found) = self.search_from(
                    seed,
                    auto_facts,
                    auto_deductions,
                    MAX_EXPLORED_STATES,
                    |_node| 0,
                    |candidate| {
                        candidate
                            .steps
                            .last()
                            .is_some_and(|last| &last.action == fail_id)
                    },
                ) {
                    step_fail_nodes[index] = Some(found);
                }
            }
        }
        // An end that's still unresolved after the step-chaining above might
        // conclude the solve session a different way than the one checkpoint
        // originally tried here: `step_nodes[solve_steps.len()]` only ever
        // captures a session concluded by *succeeding* the final step, but
        // `solve_step_fail` on any step (see its `next_step = 0` reset in
        // `apply_action`) concludes it too, and island_retreat's
        // `end.mistaken_accusation` requires exactly that "botched it"
        // path. `step_fail_nodes` (tracked above) are the seeds for that
        // case, one per step. Every seed here is a genuine
        // `MAX_EXPLORED_STATES`-bounded leg from a real playthrough
        // witness, so trying more of them can only turn an `Inconclusive`
        // into a `Proved`, never manufacture a false one -- and capping
        // this at the success checkpoint plus one per-step failure
        // checkpoint (rather than every intermediate step-readiness
        // checkpoint tried previously) bounds the added cost to at most
        // `solve_steps.len() + 1` extra legs per unresolved end.
        // narrator-validator#99: rather than a separate `search_from` leg
        // per (seed, end) pair -- which reruns the identical reachable-state
        // walk once per unresolved end, saturating the budget on every
        // repetition when the ends are blocked for a budget-independent
        // reason -- `search_from_multi_end` shares ONE walk per seed across
        // every still-unresolved end, checking each end's own
        // `end_recovery_satisfied` at every node reached and recording a
        // witness the first time each becomes true. This changes the cost
        // from `ends x seeds x budget` to `seeds x budget` without weakening
        // any individual end's proof: each recorded witness still is a node
        // genuinely reached from `seed` via real actions that independently
        // satisfies that end's own acceptance check.
        if bounded {
            let seeds = std::iter::once(step_nodes[self.solve_steps.len()].clone())
                .chain(step_fail_nodes.iter().cloned())
                .flatten();
            for seed in seeds {
                let unresolved = self
                    .ends
                    .iter()
                    .filter(|end| !proofs.contains_key(&end.item.id))
                    .collect::<Vec<_>>();
                if unresolved.is_empty() {
                    break;
                }
                let found = self.search_from_multi_end(
                    seed,
                    auto_facts,
                    auto_deductions,
                    LEG_MAX_EXPLORED_STATES,
                    &unresolved,
                );
                proofs.extend(found);
            }
        }
        let step_answerability = self
            .solve_steps
            .iter()
            .enumerate()
            .map(|(index, step)| {
                if let Some(action_count) = step_progress[index] {
                    return StepAnswerability {
                        id: step.item.id.clone(),
                        status: PlayabilityStatus::Proved,
                        action_count: Some(action_count),
                        blocker: None,
                    };
                }
                let hard_missing = step.rows.iter().enumerate().find_map(|(row_index, row)| {
                    match row {
                        SolveRow::NOfM { n, pool } => {
                            let learnable = pool.iter().filter(|s| self.subject_learnable(s)).count();
                            (learnable < *n).then(|| {
                                let card_index = pool
                                    .iter()
                                    .position(|s| !self.subject_learnable(s))
                                    .unwrap_or(0);
                                (row_index, card_index, pool[card_index].clone())
                            })
                        }
                        SolveRow::Ordered { cards } => cards
                            .iter()
                            .position(|s| !self.subject_learnable(s))
                            .map(|card_index| (row_index, card_index, cards[card_index].clone())),
                    }
                });
                if let Some((row_index, card_index, subject)) = hard_missing {
                    return StepAnswerability {
                        id: step.item.id.clone(),
                        status: PlayabilityStatus::NotProved,
                        action_count: None,
                        blocker: Some(PlayabilityBlocker {
                            code: "playability.unlearnable_answer".to_string(),
                            message: format!(
                                "no fact or deduction in the story ever establishes `{subject}`, which `{}` requires",
                                step.item.id
                            ),
                            path: step.item.path.clone(),
                            pointer: format!(
                                "{}/rows/{row_index}/cards/{card_index}",
                                step.item.pointer
                            ),
                            range: None,
                            chain: vec![step.item.id.clone(), subject],
                        }),
                    };
                }
                let inconclusive =
                    bounded || !self.unsupported.is_empty() || unsupported_policy.is_some();
                StepAnswerability {
                    id: step.item.id.clone(),
                    status: if inconclusive {
                        PlayabilityStatus::Inconclusive
                    } else {
                        PlayabilityStatus::NotProved
                    },
                    action_count: None,
                    blocker: if let Some(reason) = self.unsupported.first() {
                        Some(PlayabilityBlocker {
                            code: reason.code.clone(),
                            message: reason.message.clone(),
                            path: reason.path.clone(),
                            pointer: reason.pointer.clone(),
                            range: reason.range,
                            chain: vec![step.item.id.clone()],
                        })
                    } else if let Some((code, message)) = unsupported_policy {
                        Some(PlayabilityBlocker {
                            code: code.to_string(),
                            message: message.to_string(),
                            path: step.item.path.clone(),
                            pointer: step.item.pointer.clone(),
                            range: step.item.range,
                            chain: vec![step.item.id.clone()],
                        })
                    } else if bounded {
                        Some(PlayabilityBlocker {
                            code: "playability.search_bound".to_string(),
                            message: format!(
                                "analysis reached its deterministic bound of {MAX_EXPLORED_STATES} states, {MAX_ACTIONS} actions, or {MAX_ELAPSED_MINUTES} elapsed minutes"
                            ),
                            path: step.item.path.clone(),
                            pointer: step.item.pointer.clone(),
                            range: step.item.range,
                            chain: vec![step.item.id.clone()],
                        })
                    } else {
                        None
                    },
                }
            })
            .collect::<Vec<_>>();
        let terminal_paths = self.ends.iter().map(|end| {
            if let Some(node) = proofs.get(&end.item.id) {
                // A witness was actually found. Only demote it to
                // Inconclusive if some unsupported construct is both
                // (a) still live in the search model (`search_excluded ==
                // false`) and (b) actually reached by this specific
                // witness (its `witness_subject` shows up in the state the
                // witness settles into). Every other unsupported note
                // describes something the search structurally could never
                // have used to build this witness in the first place --
                // see narrator-validator#88.
                if let Some(reason) = self
                    .unsupported
                    .iter()
                    .find(|reason| self.witness_reached_by(reason, node))
                {
                    TerminalPathAnalysis { id: end.item.id.clone(), outcome: end.outcome.clone(), status: PlayabilityStatus::Inconclusive, path: end.item.path.clone(), pointer: end.item.pointer.clone(), range: end.item.range, lower_bound: None, blocker: Some(PlayabilityBlocker { code: reason.code.clone(), message: format!("a supported path was found, but `{}` may change its result; the path is not reported as proved", reason.message), path: reason.path.clone(), pointer: reason.pointer.clone(), range: reason.range, chain: vec![end.item.id.clone()] }) }
                } else {
                    TerminalPathAnalysis { id: end.item.id.clone(), outcome: end.outcome.clone(), status: PlayabilityStatus::Proved, path: end.item.path.clone(), pointer: end.item.pointer.clone(), range: end.item.range, lower_bound: Some(PlayabilityLowerBound { entry_setting: node.state.entry.clone(), action_count: node.actions, route_action_count: node.route_actions, elapsed_minutes: node.state.elapsed, wait_minutes: node.wait_minutes, required_waits: self.triggers.values().filter(|trigger| trigger.after > 0 && node.state.completed.contains(&trigger.item.id)).map(|trigger| PlayabilityRequiredWait { trigger: trigger.item.id.clone(), delay_minutes: trigger.after }).collect(), ordered_steps: node.steps.clone(), pivotal_unlocks: node.unlocks.iter().cloned().collect() }), blocker: None }
                }
            } else {
                let unlearnable = end
                    .requirements
                    .iter()
                    .find_map(|requirement| self.unlearnable_answer_blocker(end, requirement));
                let hard_missing = end
                    .requirements
                    .iter()
                    .any(|requirement| !self.has_possible_producer(requirement));
                let unsupported = self.unsupported.first();
                // The static "hard unlearnable" tier survives unsupported
                // constructs and the search bound, same as today's
                // hard-missing-producer tier.
                let inconclusive = unlearnable.is_none()
                    && !hard_missing
                    && (bounded || unsupported.is_some() || unsupported_policy.is_some() || end.solution_condition);
                let blocker = if let Some(blocker) = unlearnable {
                    blocker
                } else if hard_missing {
                    self.blocker(end, &reached_states)
                } else if let Some(reason) = unsupported {
                    PlayabilityBlocker { code: reason.code.clone(), message: reason.message.clone(), path: reason.path.clone(), pointer: reason.pointer.clone(), range: reason.range, chain: vec![end.item.id.clone()] }
                } else if let Some((code, message)) = unsupported_policy {
                    PlayabilityBlocker { code: code.to_string(), message: message.to_string(), path: end.item.path.clone(), pointer: end.item.pointer.clone(), range: end.item.range, chain: vec![end.item.id.clone()] }
                } else if end.solution_condition {
                    PlayabilityBlocker { code: "playability.unsupported_solution_selection".to_string(), message: "the authored Solve contract could not be represented as one exact supported action".to_string(), path: end.item.path.clone(), pointer: end.item.pointer.clone(), range: end.item.range, chain: vec![end.item.id.clone()] }
                } else if bounded {
                    PlayabilityBlocker { code: "playability.search_bound".to_string(), message: format!("analysis reached its deterministic bound of {MAX_EXPLORED_STATES} states, {MAX_ACTIONS} actions, or {MAX_ELAPSED_MINUTES} elapsed minutes"), path: end.item.path.clone(), pointer: end.item.pointer.clone(), range: end.item.range, chain: vec![end.item.id.clone()] }
                } else {
                    self.blocker(end, &reached_states)
                };
                TerminalPathAnalysis { id: end.item.id.clone(), outcome: end.outcome.clone(), status: if inconclusive { PlayabilityStatus::Inconclusive } else { PlayabilityStatus::NotProved }, path: end.item.path.clone(), pointer: end.item.pointer.clone(), range: end.item.range, lower_bound: None, blocker: Some(blocker) }
            }
        }).collect();
        let solution_answerability = if !self.solve_steps.is_empty() {
            // Format 3.7: aggregate the per-step results so existing
            // consumers of the legacy single-commit field don't see a
            // misleading permanent `NotProved` on a step-based story.
            // Proved iff every step is Proved; `action_count` is the final
            // step's, matching "how many actions to answer everything".
            if step_answerability
                .iter()
                .all(|step| step.status == PlayabilityStatus::Proved)
            {
                SolutionAnswerability {
                    status: PlayabilityStatus::Proved,
                    action_count: step_answerability.last().and_then(|step| step.action_count),
                    solution_equivalent_deductions: Vec::new(),
                }
            } else if step_answerability
                .iter()
                .any(|step| step.status == PlayabilityStatus::NotProved)
            {
                SolutionAnswerability {
                    status: PlayabilityStatus::NotProved,
                    action_count: None,
                    solution_equivalent_deductions: Vec::new(),
                }
            } else {
                SolutionAnswerability {
                    status: PlayabilityStatus::Inconclusive,
                    action_count: None,
                    solution_equivalent_deductions: Vec::new(),
                }
            }
        } else if let Some((action_count, deductions)) = answerable {
            SolutionAnswerability {
                status: PlayabilityStatus::Proved,
                action_count: Some(action_count),
                solution_equivalent_deductions: deductions,
            }
        } else {
            SolutionAnswerability {
                status: if self.unsupported.is_empty() && unsupported_policy.is_none() && !bounded {
                    PlayabilityStatus::NotProved
                } else {
                    PlayabilityStatus::Inconclusive
                },
                action_count: None,
                solution_equivalent_deductions: Vec::new(),
            }
        };
        NotebookPolicyAnalysis {
            auto_facts,
            auto_deductions,
            explored_states: explored,
            bounded,
            terminal_paths,
            solution_answerability,
            step_answerability,
        }
    }

    /// True if `reason` is both still live in the search model and its
    /// flagged construct is actually part of the state `node`'s witness
    /// settles into -- i.e. this specific proof genuinely depends on it,
    /// rather than the construct merely existing somewhere else in the
    /// story. See narrator-validator#88.
    fn witness_reached_by(&self, reason: &Unsupported, node: &Node) -> bool {
        if reason.search_excluded {
            return false;
        }
        let Some(subject) = &reason.witness_subject else {
            // A live-but-unattributed construct: we can't rule out
            // relevance, so stay conservative and demote.
            return true;
        };
        if node.state.facts.contains(subject)
            || node.state.available_facts.contains(subject)
            || node.state.deductions.contains(subject)
            || node.state.flags.contains(subject)
            || node.state.completed.contains(subject)
            || node.shadowed_triggers.contains(subject)
        {
            return true;
        }
        // An unsupported trigger whose own `on` gate couldn't be resolved
        // (rather than a supported gate this witness did or didn't
        // shadow) can't be pinned to any specific action the witness took.
        // Its precise firing point along the path also isn't observable
        // from the final state alone (e.g. an `at`/`time before` predicate
        // isn't monotonic). Stay conservative rather than guess.
        if self
            .triggers
            .get(subject)
            .is_some_and(|trigger| trigger.on.is_none())
        {
            return true;
        }
        false
    }

    fn has_possible_producer(&self, requirement: &str) -> bool {
        if has_id_in_initial_or_catalog(self, requirement) {
            return true;
        }
        if self
            .commands
            .values()
            .flat_map(|command| &command.effects)
            .chain(self.triggers.values().flat_map(|trigger| &trigger.effects))
            .any(|effect| match effect {
                Effect::SetFlag(id) | Effect::LearnFact(id) | Effect::EstablishDeduction(id) => {
                    id == requirement
                }
                Effect::AdvanceTime(_) | Effect::Move(_) => false,
            })
        {
            return true;
        }
        // Format 3.7's `solution.steps[].on_success`/`on_failure` are the
        // only producer of their flags: without this, every graded ending
        // that requires a step-outcome flag would be falsely `NotProved`
        // for any story on this path.
        self.solve_steps.iter().any(|step| {
            step.on_success
                .set_flags
                .iter()
                .any(|flag| flag == requirement)
                || step
                    .on_failure
                    .set_flags
                    .iter()
                    .any(|flag| flag == requirement)
        })
    }

    fn actions(
        &self,
        state: &State,
        auto_facts: bool,
        auto_deductions: bool,
    ) -> Vec<CandidateAction> {
        let mut actions = Vec::new();
        for route in &self.routes {
            if route.from == state.location && route.requirements.iter().all(|id| has(state, id)) {
                actions.push(CandidateAction {
                    kind: "route",
                    id: route.id.clone(),
                    pattern: ActionPattern {
                        command: "command.move".to_string(),
                        bindings: BTreeMap::from([(
                            "destination".to_string(),
                            vec![route.to.clone()],
                        )]),
                    },
                    from: Some(route.from.clone()),
                    to: Some(route.to.clone()),
                    minutes: route.minutes,
                });
            }
            if route.bidirectional
                && route.to == state.location
                && route.requirements.iter().all(|id| has(state, id))
            {
                actions.push(CandidateAction {
                    kind: "route",
                    id: route.id.clone(),
                    pattern: ActionPattern {
                        command: "command.move".to_string(),
                        bindings: BTreeMap::from([(
                            "destination".to_string(),
                            vec![route.from.clone()],
                        )]),
                    },
                    from: Some(route.to.clone()),
                    to: Some(route.from.clone()),
                    minutes: route.minutes,
                });
            }
        }
        // `command.take` isn't in `precomputed_patterns` (its `item`
        // parameter has `min: 1`, so `read_commands` marks it
        // `requires_binding` and `precompute_patterns` skips generating a
        // no-binding pattern for it) -- it needs one candidate per
        // currently-reachable portable entity instead of a single fixed
        // pattern. Restricted to `takeable_entities`, not every portable
        // entity in the story: see that field's doc comment for why.
        if self.commands.contains_key("command.take")
            && !self.unsupported_commands.contains("command.take")
        {
            for item in &self.takeable_entities {
                if state.inventory.contains(item) {
                    continue;
                }
                if self.subject_locations.get(item) != Some(&state.location) {
                    continue;
                }
                if !self
                    .subject_requirements
                    .get(item)
                    .map_or(true, |requirements| {
                        requirements.iter().all(|id| has(state, id))
                    })
                {
                    continue;
                }
                actions.push(CandidateAction {
                    kind: "take",
                    id: format!("command.take {item}"),
                    pattern: ActionPattern {
                        command: "command.take".to_string(),
                        bindings: BTreeMap::from([("item".to_string(), vec![item.clone()])]),
                    },
                    from: None,
                    to: None,
                    minutes: 0,
                });
            }
        }
        for pattern in &self.precomputed_patterns {
            if self.action_available(pattern, state) {
                let advances_time = self
                    .commands
                    .get(&pattern.command)
                    .map(|command| {
                        command
                            .effects
                            .iter()
                            .any(|effect| matches!(effect, Effect::AdvanceTime(_)))
                    })
                    .unwrap_or(false);
                actions.push(CandidateAction {
                    kind: if advances_time { "wait" } else { "command" },
                    id: pattern_key(pattern),
                    pattern: pattern.clone(),
                    from: None,
                    to: None,
                    minutes: 0,
                });
            }
        }
        if !state.solution_solved
            && self.commands.contains_key("command.solve")
            && !self.unsupported_commands.contains("command.solve")
            && self.solution_requirements_satisfied(state)
        {
            if let Some(action) = &self.solve_action {
                actions.push(CandidateAction {
                    kind: "solve",
                    id: action.clone(),
                    pattern: ActionPattern {
                        command: "command.solve".to_string(),
                        bindings: BTreeMap::new(),
                    },
                    from: None,
                    to: None,
                    minutes: 0,
                });
            }
        }
        if !self.solve_steps.is_empty()
            && self.commands.contains_key("command.solve")
            && !self.unsupported_commands.contains("command.solve")
        {
            if let Some(step) = self.solve_steps.get(state.next_step as usize) {
                if step.rows.iter().all(|row| self.row_satisfied(row, state)) {
                    actions.push(CandidateAction {
                        kind: "solve_step",
                        id: format!("command.solve {}", step.item.id),
                        pattern: ActionPattern {
                            command: "command.solve".to_string(),
                            bindings: BTreeMap::new(),
                        },
                        from: None,
                        to: None,
                        minutes: step.time_cost,
                    });
                }
                // Attempt/restart semantics are only modeled for the
                // (rare) case where an end state can only be reached
                // through an `on_failure` effect: a prover's play-through
                // never needs to actually fail, so this action exists
                // solely to reach that flag, not to simulate every wrong
                // answer. A negative `points` penalty alone can never be
                // the *only* route to a proof -- any end/step reachable
                // after taking a penalty is also reachable (with a
                // strictly better score) by never failing at all -- so
                // only a genuine failure flag or a positive failure bonus
                // makes this action worth generating. Real stories author
                // only negative failure points on most steps, and without
                // this narrowing the search wastes budget on a branch that
                // resets progress, burns an attempt, and can only ever
                // score worse than not failing.
                let has_failure_effects =
                    !step.on_failure.set_flags.is_empty() || step.on_failure.points > 0;
                let attempts_available = self
                    .max_attempts
                    .map_or(true, |max| state.attempts_used < max);
                if has_failure_effects && attempts_available {
                    actions.push(CandidateAction {
                        kind: "solve_step_fail",
                        id: format!("command.solve {} fail", step.item.id),
                        pattern: ActionPattern {
                            command: "command.solve".to_string(),
                            bindings: BTreeMap::new(),
                        },
                        from: None,
                        to: None,
                        minutes: step.time_cost,
                    });
                }
            }
        }
        if !auto_facts && self.commands.contains_key("command.claim") {
            for fact_id in &state.available_facts {
                actions.push(CandidateAction {
                    kind: "fact_claim",
                    id: format!("command.claim {fact_id}"),
                    pattern: ActionPattern {
                        command: "command.claim".to_string(),
                        bindings: BTreeMap::new(),
                    },
                    from: None,
                    to: None,
                    minutes: 0,
                });
            }
        }
        if !auto_deductions {
            for deduction in self.deductions.values() {
                if !state.deductions.contains(&deduction.item.id)
                    && deduction.dependencies.iter().all(|id| has(state, id))
                {
                    actions.push(CandidateAction {
                        kind: "deduction",
                        id: format!("command.deduce {}", deduction.item.id),
                        pattern: ActionPattern {
                            command: "command.deduce".to_string(),
                            bindings: BTreeMap::new(),
                        },
                        from: None,
                        to: None,
                        minutes: 0,
                    });
                }
            }
        }
        actions.sort_by(|a, b| (&a.kind, &a.id, &a.to).cmp(&(&b.kind, &b.id, &b.to)));
        actions
    }

    fn solution_requirements_satisfied(&self, state: &State) -> bool {
        self.solution_target
            .as_ref()
            .and_then(|target| self.ends.iter().find(|end| &end.item.id == target))
            .map_or(true, |target| {
                target.requirements.iter().all(|id| has(state, id))
            })
    }

    fn action_available(&self, pattern: &ActionPattern, state: &State) -> bool {
        if !self.commands.contains_key(&pattern.command)
            || self.unsupported_commands.contains(&pattern.command)
        {
            return false;
        }
        pattern.bindings.values().flatten().all(|id| {
            if id.starts_with("setting.") {
                id == &state.location
            } else if id.starts_with("fact.")
                || id.starts_with("deduction.")
                || id.starts_with("flag.")
                || id.starts_with("trigger.")
            {
                has(state, id)
            } else if id.starts_with("entity.") {
                // A carried portable entity satisfies a binding wherever the
                // player is, not only where it started -- see the doc
                // comment on `State::inventory`. Characters are never
                // carryable, so that branch below stays co-location-only.
                (state.inventory.contains(id)
                    || self.subject_locations.get(id) == Some(&state.location))
                    && self
                        .subject_requirements
                        .get(id)
                        .map_or(true, |requirements| {
                            requirements.iter().all(|id| has(state, id))
                        })
            } else if id.starts_with("character.") {
                self.subject_locations.get(id) == Some(&state.location)
                    && self
                        .subject_requirements
                        .get(id)
                        .map_or(true, |requirements| {
                            requirements.iter().all(|id| has(state, id))
                        })
            } else if id.starts_with("answer.") {
                self.subject_known(state, id)
            } else {
                true
            }
        })
    }

    fn apply_point_awards(
        &self,
        state: &mut State,
        action: &CandidateAction,
        newly_established: &BTreeSet<String>,
        unlocks: &mut BTreeSet<String>,
    ) {
        let mut sources = vec![action.pattern.command.clone()];
        if action.kind == "route" {
            if let Some(to) = &action.to {
                sources.push(to.clone());
            }
        }
        sources.extend(newly_established.iter().cloned());
        if action.kind == "solve" {
            state.solution_solved = true;
            unlocks.insert("solution.correct".to_string());
        }
        sources.sort();
        sources.dedup();
        for source in sources {
            let Some(award) = self.point_awards.get(&source) else {
                continue;
            };
            let claims = state.point_claims.get(&award.source).copied().unwrap_or(0);
            if claims >= award.max_claim_count
                || !award.requirements.iter().all(|id| has(state, id))
            {
                continue;
            }
            state.point_claims.insert(award.source.clone(), claims + 1);
            state.score = state.score.saturating_add(award.value);
            unlocks.insert(format!("score:{}:{}", award.kind, award.source));
        }
    }

    fn apply_deduction_point_awards(
        &self,
        state: &mut State,
        deductions: &BTreeSet<String>,
        unlocks: &mut BTreeSet<String>,
    ) {
        for source in deductions {
            let Some(award) = self.point_awards.get(source) else {
                continue;
            };
            if !award.requirements.iter().all(|id| has(state, id)) {
                continue;
            }
            state.point_claims.insert(award.source.clone(), 1);
            state.score = state.score.saturating_add(award.value);
            unlocks.insert(format!("score:{}:{}", award.kind, award.source));
        }
    }

    fn apply_action(
        &self,
        state: &mut State,
        action: &CandidateAction,
        unlocks: &mut BTreeSet<String>,
        auto_facts: bool,
    ) {
        // The runtime decides which ordinary action triggers match before it
        // applies the mechanic or command effects. Preserve that snapshot so
        // this action cannot satisfy its own flag/time/location predicate.
        let matching_triggers = self
            .triggers
            .values()
            .filter(|trigger| {
                !self.unsupported_triggers.contains(&trigger.item.id)
                    && (!trigger.once
                        || (!state.completed.contains(&trigger.item.id)
                            && !state
                                .pending
                                .iter()
                                .any(|pending| pending.trigger == trigger.item.id)))
                    && trigger
                        .on
                        .as_ref()
                        .is_some_and(|pattern| pattern_matches(pattern, &action.pattern))
                    && predicates_hold(&trigger.when, state, self.initial_minutes)
            })
            .map(|trigger| trigger.item.id.clone())
            .collect::<Vec<_>>();

        if action.kind == "deduction" {
            if let Some(id) = action.id.strip_prefix("command.deduce ") {
                state.deductions.insert(id.to_string());
                unlocks.insert(id.to_string());
            }
        }
        if action.kind == "fact_claim" {
            if let Some(id) = action.id.strip_prefix("command.claim ") {
                state.available_facts.remove(id);
                state.facts.insert(id.to_string());
                unlocks.insert(id.to_string());
            }
        }
        if action.kind == "take" {
            if let Some(item) = action
                .pattern
                .bindings
                .get("item")
                .and_then(|ids| ids.first())
            {
                state.inventory.insert(item.clone());
                unlocks.insert(item.clone());
            }
        }
        if action.kind == "route" {
            state.location = action.to.clone().unwrap_or_else(|| state.location.clone());
            state.elapsed = state.elapsed.saturating_add(action.minutes);
        }
        if action.kind == "solve_step" || action.kind == "solve_step_fail" {
            if let Some(step) = self.solve_steps.get(state.next_step as usize) {
                state.elapsed = state.elapsed.saturating_add(step.time_cost);
                let outcome = if action.kind == "solve_step" {
                    &step.on_success
                } else {
                    &step.on_failure
                };
                for flag in &outcome.set_flags {
                    state.flags.insert(flag.clone());
                    unlocks.insert(flag.clone());
                }
                if outcome.points >= 0 {
                    state.score = state.score.saturating_add(outcome.points as u64);
                } else {
                    state.score = state.score.saturating_sub(outcome.points.unsigned_abs());
                }
                if action.kind == "solve_step" {
                    state.next_step = state.next_step.saturating_add(1);
                    if state.next_step as usize >= self.solve_steps.len() {
                        state.solution_solved = true;
                        unlocks.insert("solution.correct".to_string());
                    }
                } else {
                    state.next_step = 0;
                    state.attempts_used = state.attempts_used.saturating_add(1);
                }
            }
        }
        if let Some(command) = self.commands.get(&action.pattern.command) {
            for effect in &command.effects {
                apply_effect(state, effect, unlocks, auto_facts);
            }
        }
        for trigger_id in matching_triggers {
            let trigger = &self.triggers[&trigger_id];
            if trigger.after > 0 {
                state.pending.push(Pending {
                    due: state.elapsed.saturating_add(trigger.after),
                    trigger: trigger.item.id.clone(),
                });
                state.pending.sort();
            } else {
                complete_trigger(state, trigger, unlocks, auto_facts);
            }
        }
        // The runtime discovers action-gated facts after the already-matched
        // triggers settle, so their persistent conditions observe immediate
        // trigger effects without allowing those effects to change matching.
        for fact in self.facts.values() {
            if fact
                .on
                .as_ref()
                .is_some_and(|pattern| pattern_matches(pattern, &action.pattern))
                && predicates_hold(&fact.when, state, self.initial_minutes)
            {
                acquire_fact(state, &fact.item.id, unlocks, auto_facts);
            }
        }
    }

    fn settle(
        &self,
        state: &mut State,
        _action: Option<&CandidateAction>,
        unlocks: &mut BTreeSet<String>,
        auto_facts: bool,
        auto_deductions: bool,
    ) {
        loop {
            let before = (
                state.facts.len(),
                state.available_facts.len(),
                state.deductions.len(),
                state.flags.len(),
                state.completed.len(),
                state.pending.len(),
            );
            let due = state
                .pending
                .iter()
                .filter(|pending| pending.due <= state.elapsed)
                .cloned()
                .collect::<Vec<_>>();
            state.pending.retain(|pending| pending.due > state.elapsed);
            for pending in due {
                if let Some(trigger) = self.triggers.get(&pending.trigger) {
                    complete_trigger(state, trigger, unlocks, auto_facts);
                }
            }
            for fact in self.facts.values() {
                if fact.on.is_none()
                    && !fact.opening
                    && predicates_hold(&fact.when, state, self.initial_minutes)
                    && fact
                        .item
                        .owner
                        .as_ref()
                        .map_or(true, |owner| !owner.starts_with("trigger."))
                {
                    acquire_fact(state, &fact.item.id, unlocks, auto_facts);
                }
            }
            for trigger in self.triggers.values() {
                if self.unsupported_triggers.contains(&trigger.item.id) {
                    continue;
                }
                if trigger.on.is_none()
                    && !state.completed.contains(&trigger.item.id)
                    && !state
                        .pending
                        .iter()
                        .any(|pending| pending.trigger == trigger.item.id)
                    && predicates_hold(&trigger.when, state, self.initial_minutes)
                {
                    if trigger.after > 0 {
                        state.pending.push(Pending {
                            due: state.elapsed.saturating_add(trigger.after),
                            trigger: trigger.item.id.clone(),
                        });
                        state.pending.sort();
                    } else {
                        complete_trigger(state, trigger, unlocks, auto_facts);
                    }
                }
            }
            if auto_deductions {
                for deduction in self.deductions.values() {
                    if !state.deductions.contains(&deduction.item.id)
                        && deduction.dependencies.iter().all(|id| has(state, id))
                    {
                        state.deductions.insert(deduction.item.id.clone());
                        unlocks.insert(deduction.item.id.clone());
                    }
                }
            }
            let after = (
                state.facts.len(),
                state.available_facts.len(),
                state.deductions.len(),
                state.flags.len(),
                state.completed.len(),
                state.pending.len(),
            );
            if before == after {
                break;
            }
        }
    }

    fn end_satisfied(&self, end: &EndRule, state: &State) -> bool {
        (!end.solution_condition || state.solution_solved)
            && end.requirements.iter().all(|id| has(state, id))
            && state.score >= end.minimum_points
            && end.at_or_after.map_or(true, |threshold| {
                self.initial_minutes.saturating_add(state.elapsed) >= threshold
            })
    }

    /// If `requirement` traces to a Format 3.7 step-outcome flag whose step
    /// demands a card subject with zero possible witnesses anywhere in the
    /// catalog, the static "hard unlearnable" tier: report
    /// `playability.unlearnable_answer` pointing at the offending row card
    /// instead of the generic `playability.missing_requirement`.
    fn unlearnable_answer_blocker(
        &self,
        end: &EndRule,
        requirement: &str,
    ) -> Option<PlayabilityBlocker> {
        let step = self.solve_steps.iter().find(|step| {
            step.on_success
                .set_flags
                .iter()
                .any(|flag| flag == requirement)
                || step
                    .on_failure
                    .set_flags
                    .iter()
                    .any(|flag| flag == requirement)
        })?;
        for (row_index, row) in step.rows.iter().enumerate() {
            let missing = match row {
                SolveRow::NOfM { n, pool } => {
                    let learnable = pool
                        .iter()
                        .filter(|subject| self.subject_learnable(subject))
                        .count();
                    (learnable < *n)
                        .then(|| {
                            pool.iter()
                                .position(|subject| !self.subject_learnable(subject))
                                .map(|card_index| (card_index, pool[card_index].clone()))
                        })
                        .flatten()
                }
                SolveRow::Ordered { cards } => cards
                    .iter()
                    .position(|subject| !self.subject_learnable(subject))
                    .map(|card_index| (card_index, cards[card_index].clone())),
            };
            if let Some((card_index, subject)) = missing {
                return Some(PlayabilityBlocker {
                    code: "playability.unlearnable_answer".to_string(),
                    message: format!(
                        "no fact or deduction in the story ever establishes `{subject}`, which `{}` requires",
                        step.item.id
                    ),
                    path: step.item.path.clone(),
                    pointer: format!("{}/rows/{row_index}/cards/{card_index}", step.item.pointer),
                    range: None,
                    chain: vec![
                        end.item.id.clone(),
                        requirement.to_string(),
                        step.item.id.clone(),
                        subject,
                    ],
                });
            }
        }
        None
    }

    fn blocker(&self, end: &EndRule, states: &[State]) -> PlayabilityBlocker {
        let mut missing = end
            .requirements
            .iter()
            .filter(|id| !states.iter().any(|state| has(state, id)))
            .cloned()
            .collect::<Vec<_>>();
        missing.sort();
        let (code, message, chain, pointer) = if let Some(id) = missing.first() {
            let requirement_index = end
                .requirements
                .iter()
                .position(|requirement| requirement == id)
                .unwrap_or(0);
            let mut chain = vec![end.item.id.clone()];
            chain.extend(self.missing_chain(id, states, &mut BTreeSet::new()));
            (
                "playability.missing_requirement",
                format!(
                    "no supported action can establish required `{id}`; blocked chain: {}",
                    chain.join(" -> ")
                ),
                chain,
                format!("{}/requires/{requirement_index}", end.item.pointer),
            )
        } else if end.minimum_points > states.iter().map(|state| state.score).max().unwrap_or(0) {
            (
                "playability.insufficient_score",
                format!(
                    "supported actions reach at most {} points, below required {}",
                    states.iter().map(|state| state.score).max().unwrap_or(0),
                    end.minimum_points
                ),
                vec![end.item.id.clone(), "minimum_points".to_string()],
                format!("{}/minimum_points", end.item.pointer),
            )
        } else if end.at_or_after.is_some()
            && !states.iter().any(|state| self.end_satisfied(end, state))
        {
            (
                "playability.route_time_blocked",
                "no supported route or wait action advances the clock to this terminal threshold"
                    .to_string(),
                vec![end.item.id.clone(), "at_or_after".to_string()],
                format!("{}/at_or_after", end.item.pointer),
            )
        } else {
            (
                "playability.precedence_blocked",
                "the path is never the first satisfied authored terminal state".to_string(),
                vec![end.item.id.clone()],
                end.item.pointer.clone(),
            )
        };
        PlayabilityBlocker {
            code: code.to_string(),
            message,
            path: end.item.path.clone(),
            pointer,
            range: None,
            chain,
        }
    }

    fn missing_chain(
        &self,
        id: &str,
        states: &[State],
        visiting: &mut BTreeSet<String>,
    ) -> Vec<String> {
        if !visiting.insert(id.to_string()) {
            return vec![id.to_string()];
        }
        let next = if let Some(deduction) = self.deductions.get(id) {
            deduction
                .dependencies
                .iter()
                .find(|dependency| !states.iter().any(|state| has(state, dependency)))
                .cloned()
        } else if let Some(fact) = self.facts.get(id) {
            fact.when
                .iter()
                .find_map(|predicate| match predicate {
                    Predicate::Has(dependency)
                        if !states.iter().any(|state| has(state, dependency)) =>
                    {
                        Some(dependency.clone())
                    }
                    _ => None,
                })
                .or_else(|| fact.on.as_ref().map(|on| on.command.clone()))
        } else if let Some(trigger) = self.triggers.get(id) {
            trigger.on.as_ref().map(|on| on.command.clone())
        } else if let Some(step) = self.solve_steps.iter().find(|step| {
            step.on_success.set_flags.iter().any(|flag| flag == id)
                || step.on_failure.set_flags.iter().any(|flag| flag == id)
        }) {
            // A step-outcome flag: descend into the step itself, then the
            // first row card that isn't known in any reached state.
            let mut chain = vec![id.to_string(), step.item.id.clone()];
            if let Some(subject) = step.rows.iter().find_map(|row| {
                let subjects: Vec<&String> = match row {
                    SolveRow::NOfM { pool, .. } => pool.iter().collect(),
                    SolveRow::Ordered { cards } => cards.iter().collect(),
                };
                subjects.into_iter().find(|subject| {
                    !states
                        .iter()
                        .any(|state| self.subject_known(state, subject))
                })
            }) {
                chain.push(subject.clone());
            }
            return chain;
        } else {
            None
        };
        let mut chain = vec![id.to_string()];
        if let Some(next) = next {
            chain.extend(self.missing_chain(&next, states, visiting));
        }
        chain
    }
}

fn complete_trigger(
    state: &mut State,
    trigger: &TriggerRule,
    unlocks: &mut BTreeSet<String>,
    auto_facts: bool,
) {
    state.completed.insert(trigger.item.id.clone());
    unlocks.insert(trigger.item.id.clone());
    for fact in &trigger.facts {
        acquire_fact(state, fact, unlocks, auto_facts);
    }
    for effect in &trigger.effects {
        apply_effect(state, effect, unlocks, auto_facts);
    }
}

fn apply_effect(
    state: &mut State,
    effect: &Effect,
    unlocks: &mut BTreeSet<String>,
    auto_facts: bool,
) {
    match effect {
        Effect::SetFlag(id) => {
            state.flags.insert(id.clone());
            unlocks.insert(id.clone());
        }
        Effect::AdvanceTime(minutes) => {
            state.elapsed = state.elapsed.saturating_add(*minutes);
        }
        Effect::LearnFact(id) => {
            acquire_fact(state, id, unlocks, auto_facts);
        }
        Effect::Move(setting) => {
            state.location = setting.clone();
        }
        Effect::EstablishDeduction(id) => {
            state.deductions.insert(id.clone());
            unlocks.insert(id.clone());
        }
    }
}

fn acquire_fact(state: &mut State, id: &str, unlocks: &mut BTreeSet<String>, auto_facts: bool) {
    if state.facts.contains(id) || state.available_facts.contains(id) {
        return;
    }
    if auto_facts {
        state.facts.insert(id.to_string());
    } else {
        state.available_facts.insert(id.to_string());
    }
    unlocks.insert(id.to_string());
}

fn predicates_hold(predicates: &[Predicate], state: &State, initial: u32) -> bool {
    predicates.iter().all(|predicate| match predicate {
        Predicate::Has(id) => has(state, id),
        Predicate::At(id) => &state.location == id,
        Predicate::TimeAfter(value) => initial.saturating_add(state.elapsed) > *value,
        Predicate::TimeEqual(value) => initial.saturating_add(state.elapsed) == *value,
        Predicate::TimeBefore(value) => initial.saturating_add(state.elapsed) < *value,
        Predicate::Never => false,
    })
}

fn has(state: &State, id: &str) -> bool {
    state.facts.contains(id)
        || state.deductions.contains(id)
        || state.flags.contains(id)
        || state.completed.contains(id)
        || id == state.location
}

fn has_id_in_initial_or_catalog(model: &Model, id: &str) -> bool {
    model.initial_flags.contains(id)
        || model.facts.contains_key(id)
        || model.deductions.contains_key(id)
        || model.triggers.contains_key(id)
        || model.entries.iter().any(|entry| entry == id)
        || model
            .routes
            .iter()
            .any(|route| route.from == id || route.to == id)
        || id.starts_with("entity.")
}

#[cfg(test)]
mod elapsed_equivalence_tests {
    use super::*;

    fn item(id: &str) -> LocatedItem {
        LocatedItem {
            id: id.to_string(),
            path: "fixture.yaml".to_string(),
            pointer: format!("/{id}"),
            range: None,
            map: Mapping::new(),
            owner: None,
        }
    }

    fn state(elapsed: u32, pending_due: Option<u32>) -> State {
        State {
            entry: "setting.entry".to_string(),
            location: "setting.entry".to_string(),
            elapsed,
            facts: BTreeSet::new(),
            available_facts: BTreeSet::new(),
            deductions: BTreeSet::new(),
            flags: BTreeSet::new(),
            completed: BTreeSet::new(),
            pending: pending_due
                .map(|due| {
                    vec![Pending {
                        due,
                        trigger: "trigger.delayed".to_string(),
                    }]
                })
                .unwrap_or_default(),
            score: 0,
            point_claims: BTreeMap::new(),
            solution_solved: false,
            next_step: 0,
            attempts_used: 0,
            inventory: BTreeSet::new(),
        }
    }

    #[test]
    fn elapsed_equivalence_starts_only_after_every_authored_time_boundary() {
        let mut model = Model {
            initial_minutes: 60,
            ..Model::default()
        };
        model.facts.insert(
            "fact.after".to_string(),
            FactRule {
                item: item("fact.after"),
                on: None,
                when: vec![Predicate::TimeAfter(70)],
                opening: false,
                about: Vec::new(),
                statement: String::new(),
            },
        );
        model.triggers.insert(
            "trigger.at".to_string(),
            TriggerRule {
                item: item("trigger.at"),
                on: None,
                when: vec![Predicate::TimeEqual(75), Predicate::TimeBefore(78)],
                after: 0,
                effects: Vec::new(),
                facts: Vec::new(),
                once: true,
            },
        );
        model.ends.push(EndRule {
            item: item("end.deadline"),
            outcome: "lost".to_string(),
            requirements: Vec::new(),
            minimum_points: 0,
            at_or_after: Some(80),
            solution_condition: false,
        });

        model.precompute_elapsed_equivalence_horizon();

        assert_eq!(model.elapsed_equivalence_horizon, 21);
        assert_ne!(
            model.search_state_key(&state(20, None)),
            model.search_state_key(&state(21, None))
        );
        assert_eq!(
            model.search_state_key(&state(21, None)),
            model.search_state_key(&state(2_000, None))
        );
    }

    #[test]
    fn elapsed_equivalence_preserves_delayed_trigger_remaining_time() {
        let model = Model {
            elapsed_equivalence_horizon: 10,
            ..Model::default()
        };

        assert_eq!(
            model.search_state_key(&state(20, Some(25))),
            model.search_state_key(&state(2_000, Some(2_005)))
        );
        assert_ne!(
            model.search_state_key(&state(20, Some(25))),
            model.search_state_key(&state(2_000, Some(2_006)))
        );
    }
}

fn read_solve_step_outcome(step: &Mapping, key: &str) -> StepOutcome {
    let Some(outcome) = field(step, key).and_then(Value::as_mapping) else {
        return StepOutcome::default();
    };
    let mut set_flags = Vec::new();
    if let Some(effects) = field(outcome, "effects").and_then(Value::as_sequence) {
        for effect in effects {
            let Some(effect) = effect.as_mapping() else {
                continue;
            };
            if string(effect, "operation") == Some("set_flag")
                && bool_field(effect, "value") == Some(true)
            {
                if let Some(flag) = string(effect, "flag") {
                    set_flags.push(flag.to_string());
                }
            }
        }
    }
    let points = field(outcome, "points")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    StepOutcome { set_flags, points }
}

fn pattern_matches(expected: &ActionPattern, actual: &ActionPattern) -> bool {
    expected.command == actual.command
        && expected
            .bindings
            .iter()
            .all(|(name, ids)| actual.bindings.get(name) == Some(ids))
}

fn pattern_key(pattern: &ActionPattern) -> String {
    let suffix = pattern
        .bindings
        .iter()
        .flat_map(|(name, ids)| ids.iter().map(move |id| format!(" {name}={id}")))
        .collect::<String>();
    format!("{}{suffix}", pattern.command)
}

fn located(
    file: &SourceFile,
    section: &str,
    index: usize,
    value: &Value,
    owner: Option<String>,
) -> Option<LocatedItem> {
    located_at(file, value, format!("/{section}/{index}"), owner)
}
fn located_at(
    file: &SourceFile,
    value: &Value,
    pointer: String,
    owner: Option<String>,
) -> Option<LocatedItem> {
    let map = value.as_mapping()?.clone();
    let id = string(&map, "id")?.to_string();
    Some(LocatedItem {
        id: id.clone(),
        path: file.path.clone(),
        pointer: pointer.clone(),
        range: locate_id(&file.source, &id),
        map,
        owner,
    })
}
fn field<'a>(map: &'a Mapping, name: &str) -> Option<&'a Value> {
    map.get(Value::String(name.to_string()))
}
fn map<'a>(map: &'a Mapping, name: &str) -> Option<&'a Mapping> {
    field(map, name).and_then(Value::as_mapping)
}
fn sequence<'a>(map: &'a Mapping, name: &str) -> &'a [Value] {
    field(map, name)
        .and_then(Value::as_sequence)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}
fn string<'a>(map: &'a Mapping, name: &str) -> Option<&'a str> {
    field(map, name).and_then(Value::as_str)
}
fn bool_field(map: &Mapping, name: &str) -> Option<bool> {
    field(map, name).and_then(Value::as_bool)
}
fn u64_field(map: &Mapping, name: &str) -> Option<u64> {
    field(map, name).and_then(Value::as_u64)
}
fn strings(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::String(value)) => vec![value.clone()],
        Some(Value::Sequence(values)) => values
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}
fn parse_clock(value: &str) -> Option<u32> {
    let (hour, minute) = value.split_once(':')?;
    Some(hour.parse::<u32>().ok()? * 60 + minute.parse::<u32>().ok()?)
}
fn parse_duration(value: &str) -> Option<u32> {
    let (number, unit) = value.split_at(value.len().checked_sub(1)?);
    let amount = number.parse::<u32>().ok()?;
    match unit {
        "m" => Some(amount),
        "h" => amount.checked_mul(60),
        _ => None,
    }
}
fn locate_id(source: &str, id: &str) -> Option<SourceRange> {
    locate_scalar(source, id)
}
fn locate_pointer(file: &SourceFile, pointer: &str) -> Option<SourceRange> {
    let field = pointer.rsplit('/').next()?;
    (!field.chars().all(|character| character.is_ascii_digit()))
        .then(|| locate_scalar(&file.source, field))
        .flatten()
}
fn locate_scalar(source: &str, scalar: &str) -> Option<SourceRange> {
    source.lines().enumerate().find_map(|(line, text)| {
        text.find(scalar).map(|column| SourceRange {
            start: Position {
                line: line + 1,
                column: column + 1,
            },
            end: Position {
                line: line + 1,
                column: column + scalar.len() + 1,
            },
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solve_action_is_gated_by_solution_target_requirements() {
        let target_id = "end.full".to_string();
        let mut model = Model {
            solution_target: Some(target_id.clone()),
            solve_action: Some("command.solve [character.answer]".to_string()),
            ..Model::default()
        };
        model.commands.insert(
            "command.solve".to_string(),
            CommandRule {
                id: "command.solve".to_string(),
                effects: Vec::new(),
                requires_binding: false,
            },
        );
        model.ends.push(EndRule {
            item: LocatedItem {
                id: target_id,
                path: "end_states.yaml".to_string(),
                pointer: "/end_states/0".to_string(),
                range: None,
                map: Mapping::new(),
                owner: None,
            },
            outcome: "won".to_string(),
            requirements: vec!["flag.rescue_complete".to_string()],
            minimum_points: 0,
            at_or_after: None,
            solution_condition: true,
        });
        let mut state = State {
            entry: "setting.start".to_string(),
            location: "setting.start".to_string(),
            elapsed: 0,
            facts: BTreeSet::new(),
            available_facts: BTreeSet::new(),
            deductions: BTreeSet::new(),
            flags: BTreeSet::new(),
            completed: BTreeSet::new(),
            pending: Vec::new(),
            score: 0,
            point_claims: BTreeMap::new(),
            solution_solved: false,
            next_step: 0,
            attempts_used: 0,
            inventory: BTreeSet::new(),
        };

        assert!(model
            .actions(&state, true, true)
            .iter()
            .all(|action| action.kind != "solve"));
        state.flags.insert("flag.rescue_complete".to_string());
        assert_eq!(
            model
                .actions(&state, true, true)
                .iter()
                .filter(|action| action.kind == "solve")
                .count(),
            1
        );
    }

    fn empty_node(entry: &str) -> Node {
        Node {
            state: State {
                entry: entry.to_string(),
                location: entry.to_string(),
                elapsed: 0,
                facts: BTreeSet::new(),
                available_facts: BTreeSet::new(),
                deductions: BTreeSet::new(),
                flags: BTreeSet::new(),
                completed: BTreeSet::new(),
                pending: Vec::new(),
                score: 0,
                point_claims: BTreeMap::new(),
                solution_solved: false,
                next_step: 0,
                attempts_used: 0,
                inventory: BTreeSet::new(),
            },
            actions: 0,
            route_actions: 0,
            wait_minutes: 0,
            steps: Vec::new(),
            unlocks: BTreeSet::new(),
            shadowed_triggers: BTreeSet::new(),
        }
    }

    #[test]
    fn search_from_finds_a_goal_reachable_by_real_actions_from_the_seed() {
        let mut model = Model {
            entries: vec!["setting.a".to_string()],
            ..Model::default()
        };
        model.routes.push(Route {
            id: "route.a_b".to_string(),
            from: "setting.a".to_string(),
            to: "setting.b".to_string(),
            minutes: 5,
            bidirectional: true,
            requirements: Vec::new(),
        });
        let seed = empty_node("setting.a");
        let found = model
            .search_from(
                seed,
                true,
                true,
                MAX_EXPLORED_STATES,
                |_node| 0,
                |node| node.state.location == "setting.b",
            )
            .expect("setting.b is one route hop away");
        assert_eq!(found.actions, 1);
        assert_eq!(found.state.elapsed, 5);
    }

    /// Builds a minimal `at_or_after`-bearing `EndRule` (narrator-
    /// validator#91's `end_shortfall` heuristic exists precisely to give
    /// this kind of goal a gradient signal instead of the flat zero the
    /// per-end bounded-recovery leg used to search with).
    fn timed_end(threshold: u32) -> EndRule {
        EndRule {
            item: LocatedItem {
                id: "end.timed".to_string(),
                path: "end_states.yaml".to_string(),
                pointer: "/end_states/0".to_string(),
                range: None,
                map: Mapping::new(),
                owner: None,
            },
            outcome: "won".to_string(),
            requirements: Vec::new(),
            minimum_points: 0,
            at_or_after: Some(threshold),
            solution_condition: false,
        }
    }

    #[test]
    fn end_shortfall_heuristic_does_not_manufacture_a_false_proof_for_an_unreachable_at_or_after() {
        // No routes and no wait-capable commands: elapsed can never move
        // past 0, so a threshold beyond that is genuinely unreachable no
        // matter how the heuristic orders exploration.
        let model = Model {
            entries: vec!["setting.a".to_string()],
            elapsed_equivalence_horizon: 50,
            ..Model::default()
        };
        let end = timed_end(40);
        let seed = empty_node("setting.a");
        let found = model.search_from(
            seed,
            true,
            true,
            MAX_EXPLORED_STATES,
            |node| model.end_shortfall(&end, &node.state),
            |candidate| model.end_satisfied(&end, &candidate.state),
        );
        assert!(
            found.is_none(),
            "elapsed can never advance in this model; the heuristic must not \
             hallucinate a witness for a threshold that's never reached: {:#?}",
            found.map(|node| node.state)
        );
    }

    #[test]
    fn end_shortfall_heuristic_proves_and_replays_a_genuine_at_or_after_witness() {
        let mut model = Model {
            entries: vec!["setting.a".to_string()],
            // Comfortably above every elapsed value this test reaches, so
            // `search_state_key` never folds two genuinely different
            // elapsed values into the same canonical state.
            elapsed_equivalence_horizon: 100,
            ..Model::default()
        };
        model.routes.push(Route {
            id: "route.a_b".to_string(),
            from: "setting.a".to_string(),
            to: "setting.b".to_string(),
            minutes: 13,
            bidirectional: true,
            requirements: Vec::new(),
        });
        let end = timed_end(40);
        let seed = empty_node("setting.a");
        let found = model
            .search_from(
                seed,
                true,
                true,
                MAX_EXPLORED_STATES,
                |node| model.end_shortfall(&end, &node.state),
                |candidate| model.end_satisfied(&end, &candidate.state),
            )
            .expect("shuttling the 13-minute route four times clears the 40-minute threshold");
        assert!(model.end_satisfied(&end, &found.state));
        assert_eq!(found.state.elapsed, 52);
        assert_eq!(found.actions, 4);
    }

    #[test]
    fn search_from_returns_none_for_a_genuinely_unreachable_goal() {
        let model = Model {
            entries: vec!["setting.a".to_string()],
            ..Model::default()
        };
        let seed = empty_node("setting.a");
        let found = model.search_from(
            seed,
            true,
            true,
            MAX_EXPLORED_STATES,
            |_node| 0,
            |node| node.state.location == "setting.nowhere",
        );
        assert!(
            found.is_none(),
            "no route exists to setting.nowhere; a checkpointed sub-search must not \
             hallucinate one"
        );
    }

    #[test]
    fn search_fully_settled_requires_every_end_and_step_to_already_be_proved() {
        let mut model = Model {
            solve_steps: vec![SolveStep {
                item: LocatedItem {
                    id: "step.only".to_string(),
                    path: "case.yaml".to_string(),
                    pointer: "/solution/steps/0".to_string(),
                    range: None,
                    map: Mapping::new(),
                    owner: None,
                },
                rows: Vec::new(),
                time_cost: 0,
                on_success: StepOutcome::default(),
                on_failure: StepOutcome::default(),
            }],
            ..Model::default()
        };
        model.ends.push(EndRule {
            item: LocatedItem {
                id: "end.only".to_string(),
                path: "end_states.yaml".to_string(),
                pointer: "/end_states/0".to_string(),
                range: None,
                map: Mapping::new(),
                owner: None,
            },
            outcome: "won".to_string(),
            requirements: Vec::new(),
            minimum_points: 0,
            at_or_after: None,
            solution_condition: false,
        });

        let proofs = BTreeMap::<String, Node>::new();
        let step_progress = vec![None::<u32>];
        let answerable: Option<(u32, Vec<String>)> = None;
        assert!(!model.search_fully_settled(&proofs, &step_progress, &answerable));

        let mut proofs_with_end = BTreeMap::new();
        proofs_with_end.insert("end.only".to_string(), empty_node("setting.a"));
        // The end is proved but the step isn't -- still not settled.
        assert!(!model.search_fully_settled(&proofs_with_end, &step_progress, &answerable));

        let step_progress_done = vec![Some(1u32)];
        // Now both the end and the only step are proved.
        assert!(model.search_fully_settled(&proofs_with_end, &step_progress_done, &answerable));
    }

    /// Clone of `tests/step_playability.rs`'s
    /// `mutually_exclusive_time_windows_falsify_independent_reachability`
    /// fixture, but with BOTH exact-time-window cards as two `n_of_m` rows
    /// of a *single* step instead of split across two steps -- unlike that
    /// integration test (whose two-step composition never actually drives
    /// a state count anywhere near `MAX_EXPLORED_STATES`, so its `bounded`
    /// stays false and `search_from`'s chaining leg is never exercised),
    /// this calls `search_from` with the new heuristic directly so the
    /// heuristic's row-shortfall balancing across two simultaneous rows in
    /// one step is what's actually under test. `second_route_minutes` and
    /// `second_fact_time` let the same builder produce both the negative
    /// (mutually exclusive) and positive (compatible, round-trip
    /// reachable) fixtures below.
    fn one_step_two_windows_story(second_route_minutes: u32, second_fact_time: &str) -> String {
        format!(
            r#"
case:
  id: case.example
  format_version: "3.7.0"
  ruleset:
    id: ruleset.standard_mystery
    version: "7.0.0"
  players:
    min: 1
    max: 4
  initial_time: "21:00"
  entry_settings: [setting.foyer]
  exit_settings: [setting.foyer]
solution:
  max_attempts: 2
  steps:
    - id: step.name_both
      prompt: What was the motive, and when did it happen?
      time_cost_minutes: 0
      rows:
        - match: n_of_m
          n: 1
          cards: [answer.motive.jealousy]
        - match: n_of_m
          n: 1
          cards: [answer.time.night]
      on_success:
        effects:
          - operation: set_flag
            flag: flag.both_named
            value: true
      on_failure:
        points: -1
end_states:
  - id: end.solved
    name: Solved
    outcome: won
    resolution: full
    requires: [flag.both_named]
    text: You name the motive and the time.
settings:
  - id: setting.world
    type: island
    navigable: false
    description: The world containing the playable rooms.
  - id: setting.foyer
    type: room
    description: The entry foyer.
    parent: setting.world
  - id: setting.den
    type: room
    description: A den seven minutes from the foyer.
    parent: setting.world
    facts:
      - id: fact.motive_hint
        statement: A jealous rage seems to explain everything.
        about: [answer.motive.jealousy]
        when:
          all:
            - time:
                relation: at
                value: "21:07"
  - id: setting.attic
    type: room
    description: An attic away from the foyer.
    parent: setting.world
    facts:
      - id: fact.time_hint
        statement: The clock in the attic stopped at the moment of the crime.
        about: [answer.time.night]
        when:
          all:
            - time:
                relation: at
                value: "{second_fact_time}"
routes:
  - id: route.foyer_den
    from: setting.foyer
    to: setting.den
    bidirectional: true
    travel_minutes: 7
  - id: route.foyer_attic
    from: setting.foyer
    to: setting.attic
    bidirectional: true
    travel_minutes: {second_route_minutes}
characters: []
entities: []
events: []
deductions: []
flags:
  - id: flag.both_named
    name: Both named
    description: Whether the motive and time have both been named.
    initial_state: false
"#
        )
    }

    /// Builds the search_from seed and the target step's heuristic/goal
    /// closures for `one_step_two_windows_story`, mirroring the real
    /// per-step chaining call site in `search`.
    fn windows_story_model_and_seed(source: &str) -> (Model, Node) {
        let mut model = Model::from_files(&[SourceFile {
            path: "story.yaml".to_string(),
            source: source.to_string(),
        }]);
        model.normalize();
        let entry = model.entries[0].clone();
        let seed = model.opening_node(&entry, true, true);
        (model, seed)
    }

    #[test]
    fn search_from_heuristic_does_not_manufacture_a_false_proof_for_simultaneous_exclusive_windows()
    {
        let source = one_step_two_windows_story(11, "21:11");
        let (model, seed) = windows_story_model_and_seed(&source);
        let found = model.search_from(
            seed,
            true,
            true,
            MAX_EXPLORED_STATES,
            |node| model.step_shortfall(&node.state, 0),
            |candidate| candidate.state.next_step as usize > 0,
        );
        assert!(
            found.is_none(),
            "the den's and attic's exact-time windows can never both be hit in one \
             play-through, even with both rows in a single step: {:#?}",
            found.map(|node| node.state)
        );
    }

    #[test]
    fn search_from_heuristic_proves_and_replays_a_genuine_witness_for_compatible_windows() {
        let source = one_step_two_windows_story(7, "21:21");
        let (model, seed) = windows_story_model_and_seed(&source);
        let found = model
            .search_from(
                seed.clone(),
                true,
                true,
                MAX_EXPLORED_STATES,
                |node| model.step_shortfall(&node.state, 0),
                |candidate| candidate.state.next_step as usize > 0,
            )
            .expect(
                "21:07 and 21:21 are seven minutes apart each way, reachable by a round \
                 trip through the foyer",
            );

        // Witness-replay: re-derive the goal by actually executing the
        // found path's own recorded actions from the seed through
        // `actions`/`expand`, rather than trusting the search's bookkeeping.
        // This converts "sound by argument" (search_from only ever expands
        // real playthrough actions) into "sound by execution" for this
        // heuristic-ordered leg specifically.
        let mut replay = seed;
        for step in &found.steps {
            let action = model
                .actions(&replay.state, true, true)
                .into_iter()
                .find(|candidate| {
                    candidate.id == step.action
                        && candidate.kind == step.kind
                        && candidate.to == step.to
                })
                .unwrap_or_else(|| {
                    panic!("witness action {} still available for replay", step.action)
                });
            replay = model
                .expand(&replay, action, true, true)
                .expect("replayed action stays within the elapsed/action bounds");
        }
        assert!(
            replay.state.next_step as usize > 0,
            "replaying the witness's own recorded actions must re-derive the step-committed \
             goal state: {:#?}",
            replay.state
        );
        assert_eq!(replay.state, found.state);
    }

    /// A one-setting, one-row story whose only witness fact is gated on a
    /// `fact.*` id nothing in the model ever produces -- genuinely
    /// unlearnable, independent of search order. Exercises
    /// `subject_shortfall`/`witness_extra_cost`'s `unmet_prerequisites` term
    /// specifically: it stays fixed at 1 forever (the `Has` predicate never
    /// holds), so the row's shortfall can never reach 0 and the step's
    /// `solve_step` action can never become available -- see
    /// narrator-validator#94.
    fn unlearnable_motive_story() -> String {
        r#"
case:
  id: case.example
  format_version: "3.7.0"
  ruleset:
    id: ruleset.standard_mystery
    version: "7.0.0"
  players:
    min: 1
    max: 4
  initial_time: "21:00"
  entry_settings: [setting.foyer]
  exit_settings: [setting.foyer]
solution:
  max_attempts: 2
  steps:
    - id: step.name_motive
      prompt: What was the motive?
      time_cost_minutes: 0
      rows:
        - match: n_of_m
          n: 1
          cards: [answer.motive.jealousy]
      on_success:
        effects:
          - operation: set_flag
            flag: flag.motive_named
            value: true
      on_failure:
        points: -1
end_states:
  - id: end.solved
    name: Solved
    outcome: won
    resolution: full
    requires: [flag.motive_named]
    text: You name the motive.
settings:
  - id: setting.world
    type: island
    navigable: false
    description: The world containing the playable room.
  - id: setting.foyer
    type: room
    description: The entry foyer.
    parent: setting.world
    facts:
      - id: fact.motive_hint
        statement: A jealous rage seems to explain everything, if only it could be confirmed.
        about: [answer.motive.jealousy]
        when:
          all:
            - knows: fact.never_learnable
routes: []
characters: []
entities: []
events: []
deductions: []
flags:
  - id: flag.motive_named
    name: Motive named
    description: Whether the motive has been named.
    initial_state: false
"#
        .to_string()
    }

    #[test]
    fn subject_shortfall_heuristic_does_not_manufacture_a_false_proof_for_an_unlearnable_witness() {
        let source = unlearnable_motive_story();
        let (model, seed) = windows_story_model_and_seed(&source);
        let found = model.search_from(
            seed,
            true,
            true,
            MAX_EXPLORED_STATES,
            |node| model.step_shortfall(&node.state, 0),
            |candidate| candidate.state.next_step as usize > 0,
        );
        assert!(
            found.is_none(),
            "fact.motive_hint's `knows: fact.never_learnable` gate can never hold, so \
             answer.motive.jealousy can never be known and the row can never be \
             satisfied -- the heuristic must not hallucinate a witness: {:#?}",
            found.map(|node| node.state)
        );
    }

    /// A two-setting story whose single card is witnessed by a fact that is
    /// both location-gated (`fact_acquisition_locations`, via an `on`
    /// pattern resolving to `setting.den`) and prerequisite-gated (a `knows`
    /// `when` on a second, itself-examinable fact at the starting room).
    /// Exercises both `witness_extra_cost` terms together and gives
    /// `subject_shortfall` room to show genuine, monotonically-decreasing
    /// partial credit as the real witness is assembled -- see
    /// narrator-validator#94.
    fn witnessed_motive_story() -> String {
        r#"
case:
  id: case.example
  format_version: "3.7.0"
  ruleset:
    id: ruleset.standard_mystery
    version: "7.0.0"
  players:
    min: 1
    max: 4
  initial_time: "21:00"
  entry_settings: [setting.foyer]
  exit_settings: [setting.foyer]
solution:
  max_attempts: 2
  steps:
    - id: step.name_motive
      prompt: What was the motive?
      time_cost_minutes: 0
      rows:
        - match: n_of_m
          n: 1
          cards: [answer.motive.jealousy]
      on_success:
        effects:
          - operation: set_flag
            flag: flag.motive_named
            value: true
      on_failure:
        points: -1
end_states:
  - id: end.solved
    name: Solved
    outcome: won
    resolution: full
    requires: [flag.motive_named]
    text: You name the motive.
settings:
  - id: setting.world
    type: island
    navigable: false
    description: The world containing the playable rooms.
  - id: setting.foyer
    type: room
    description: The entry foyer.
    parent: setting.world
  - id: setting.den
    type: room
    description: A den seven minutes from the foyer.
    parent: setting.world
routes:
  - id: route.foyer_den
    from: setting.foyer
    to: setting.den
    bidirectional: true
    travel_minutes: 7
characters:
  - id: character.confidant
    name: Confidant
    initial:
      location: setting.foyer
    facts:
      - id: fact.confidant_shares_hint
        statement: The confidant admits a private grudge.
        about: []
        on:
          command: command.examine
          parameters:
            target: owner
  - id: character.witness
    name: Witness
    initial:
      location: setting.den
    facts:
      - id: fact.motive_evidence
        statement: The witness saw the true motive in action.
        about: [answer.motive.jealousy]
        on:
          command: command.examine
          parameters:
            target: owner
        when:
          all:
            - knows: fact.confidant_shares_hint
entities: []
events: []
deductions: []
flags:
  - id: flag.motive_named
    name: Motive named
    description: Whether the motive has been named.
    initial_state: false
"#
        .to_string()
    }

    #[test]
    fn subject_shortfall_heuristic_proves_and_replays_a_genuine_two_hop_witness() {
        let source = witnessed_motive_story();
        let (model, seed) = windows_story_model_and_seed(&source);
        assert_eq!(
            model.step_shortfall(&seed.state, 0),
            3,
            "seed is away from the witness's acquisition location (1) and missing its \
             prerequisite fact (1), on top of the row's own flat 1 for not-yet-known"
        );
        let found = model
            .search_from(
                seed.clone(),
                true,
                true,
                MAX_EXPLORED_STATES,
                |node| model.step_shortfall(&node.state, 0),
                |candidate| candidate.state.next_step as usize > 0,
            )
            .expect(
                "examining the confidant, traveling to the den, and examining the witness \
                 is a genuine three-action path to the card",
            );

        // Witness-replay: re-derive the goal by actually executing the
        // found path's own recorded actions from the seed through
        // `actions`/`expand`, rather than trusting the search's bookkeeping.
        let mut replay = seed;
        for step in &found.steps {
            let action = model
                .actions(&replay.state, true, true)
                .into_iter()
                .find(|candidate| {
                    candidate.id == step.action
                        && candidate.kind == step.kind
                        && candidate.to == step.to
                })
                .unwrap_or_else(|| {
                    panic!("witness action {} still available for replay", step.action)
                });
            replay = model
                .expand(&replay, action, true, true)
                .expect("replayed action stays within the elapsed/action bounds");
        }
        assert!(
            replay.state.next_step as usize > 0,
            "replaying the witness's own recorded actions must re-derive the step-committed \
             goal state: {:#?}",
            replay.state
        );
        assert_eq!(replay.state, found.state);
    }

    /// Two rooms, a `command.investigate` trigger whose `on` pattern binds
    /// one entity present in each room (`entity.item` at `setting.room_a`,
    /// `entity.device` fixed at `setting.room_b`), gated `when: at
    /// setting.room_b` -- the exact shape narrator-validator#96 exists for
    /// (quiet_kennel's `trigger.test_jo_curry_against_sedative_audit`).
    /// `portable` toggles `entity.item`'s `physical.portable`, so the same
    /// builder produces both the take-only-inventory fixture and its
    /// portable:false control.
    fn cross_room_investigate_story(portable: bool) -> String {
        format!(
            r#"
case:
  id: case.example
  format_version: "3.7.0"
  players:
    min: 1
    max: 4
  initial_time: "09:00"
  entry_settings: [setting.room_a]
  exit_settings: [setting.room_a]
end_states:
  - id: end.cross_room_verified
    name: Cross-room verified
    outcome: won
    resolution: full
    requires: [flag.cross_room_verified]
    text: The item is checked against the device in the other room.
settings:
  - id: setting.world
    type: island
    navigable: false
    description: The world containing the playable rooms.
  - id: setting.room_a
    type: room
    description: The room holding the portable item.
    parent: setting.world
  - id: setting.room_b
    type: room
    description: The room holding the fixed device.
    parent: setting.world
routes:
  - id: route.a_b
    from: setting.room_a
    to: setting.room_b
    bidirectional: true
    travel_minutes: 5
characters: []
entities:
  - id: entity.item
    description: A small portable item.
    physical:
      portable: {portable}
    initial:
      container: setting.room_a
  - id: entity.device
    description: A fixed device bolted to the wall.
    initial:
      container: setting.room_b
events: []
deductions: []
commands:
  - id: command.take
    name: Take
    description: Pick up an entity that is present and portable.
    parameters:
      - name: item
        types: [entity]
        min: 1
        max: 1
  - id: command.investigate
    name: Investigate
    description: Compare the item against the device.
    parameters:
      - name: target
        types: [entity]
        min: 1
        max: 1
      - name: comparison
        types: [entity]
        min: 1
        max: 1
triggers:
  - id: trigger.cross_room_check
    name: Cross-room check
    on:
      command: command.investigate
      parameters:
        target: entity.item
        comparison: entity.device
    when:
      all:
        - at: setting.room_b
    effects:
      - operation: set_flag
        flag: flag.cross_room_verified
        value: true
flags:
  - id: flag.cross_room_verified
    name: Cross-room verified
    description: Whether the item has been checked against the device.
    initial_state: false
"#,
            portable = portable
        )
    }

    #[test]
    fn portable_entity_can_be_carried_across_rooms_to_satisfy_a_cross_room_trigger() {
        let source = cross_room_investigate_story(true);
        let mut model = Model::from_files(&[SourceFile {
            path: "story.yaml".to_string(),
            source,
        }]);
        model.normalize();
        assert!(
            model.takeable_entities.contains("entity.item"),
            "entity.item is portable and referenced in the trigger's own `on` binding, so it \
             must be a take candidate: {:?}",
            model.takeable_entities
        );
        let analysis = model.search(true, true);
        let end = analysis
            .terminal_paths
            .iter()
            .find(|end| end.id == "end.cross_room_verified")
            .expect("end.cross_room_verified terminal path");
        assert_eq!(
            end.status,
            PlayabilityStatus::Proved,
            "carrying the portable item into room_b must let the cross-room trigger fire: \
             {end:#?}"
        );
        let ordered_steps = &end
            .lower_bound
            .as_ref()
            .expect("a proved end carries a lower-bound witness")
            .ordered_steps;
        assert!(
            ordered_steps.iter().any(|step| step.kind == "take"),
            "the witness must actually take the portable entity, not merely reach the room: \
             {ordered_steps:#?}"
        );
    }

    /// Soundness control for the fixture above: flip `physical.portable` to
    /// `false` on the exact same setup. A non-portable entity must never
    /// become carryable, so the cross-room trigger -- and the end it gates
    /// -- must stay unreached.
    #[test]
    fn non_portable_entity_cannot_be_carried_and_the_cross_room_trigger_stays_unreached() {
        let source = cross_room_investigate_story(false);
        let mut model = Model::from_files(&[SourceFile {
            path: "story.yaml".to_string(),
            source,
        }]);
        model.normalize();
        assert!(
            model.takeable_entities.is_empty(),
            "a non-portable entity must never be a take candidate: {:?}",
            model.takeable_entities
        );
        let analysis = model.search(true, true);
        let end = analysis
            .terminal_paths
            .iter()
            .find(|end| end.id == "end.cross_room_verified")
            .expect("end.cross_room_verified terminal path");
        assert_ne!(
            end.status,
            PlayabilityStatus::Proved,
            "a non-portable entity must not let the cross-room trigger fire: {end:#?}"
        );
    }
}

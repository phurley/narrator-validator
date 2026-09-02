# Story Format 3.2: reference-aware text

Story Format 3.2 introduces negotiated story capabilities and the first such
capability, `reference_text_v1`. It lets selected narrative fields refer to
stable story definitions while retaining the authored expression and ordered,
typed provenance.

## Capability negotiation

Opt in explicitly:

```yaml
case:
  id: case.quiet_kennel
  format_version: "3.2.0"
  features:
    - reference_text_v1
```

`features` is an ordered sequence of unique, non-empty feature names. It is not
valid before Format 3.2. Every consumer must advertise support for every name
before interpreting the story. Unknown features and features absent from a
consumer's advertised set stop validation immediately. This is the fail-closed
minor-version boundary: Format 3 consumers can continue structurally reading
compatible minor releases without claiming they implement newly negotiated
semantics.

The released 1.1 validator rejects `features` as an unknown Format 3.1 case
field. In a 3.2 story without `reference_text_v1`, a valid-looking `[[...]]` in
a reference-aware field receives a focused opt-in diagnostic. Format 3.1 and
earlier never interpret brackets.

## Grammar and resolution

```yaml
opening: >
  [[character.echo]] waits beside [[setting.clinic.name]].
narrative_detail: >
  Her posture remains [[character.echo.portrayal.demeanor]].
```

An expression is `[[kind.authored_id]]` followed by zero or more named mapping
fields. Authored IDs contain exactly one dot, so the first two components are
always the definition ID. Components use the same lowercase snake-case shape
as IDs. Indexes, wildcards, functions, filters, conditionals, and general YAML
traversal are not supported. `\[[character.echo]]` renders the literal text
`[[character.echo]]`.

Bare references use the kind's default path. Resolution is recursive and
deterministic. The shared result retains the consuming path, authored and
resolved strings, disclosure class, and one provenance entry per expression in
encounter order. Unknown IDs, missing or disallowed paths, mapping/list/number/
boolean targets, empty strings, malformed delimiters, and cycles are errors.

## Authoritative disclosure and path matrix

The Rust `CONSUMER_FIELDS` and `REFERENCE_KINDS` constants are the normative
machine-readable registry. `reference_text_metadata_json()` and the browser
`referenceTextMetadata()` function export the same data.

| Kind | Default | Allowed target paths |
| --- | --- | --- |
| `case` | `title` | `title`, `premise`, `opening` |
| `setting`, `entity`, `flag`, `command`, `trigger` | `name` | `name`, `description` |
| `character` | `name` | `name`, `role`, `occupation`, `description`, `portrayal.demeanor`, `portrayal.speech_style`; private `narrator_guidance.goal`, `secret`, `motive`, `method`, `cover_story`, `testimony_guidance` |
| `event` | `summary` | `summary` |
| `fact` | `statement` | `statement`, `narrative_detail` |
| `deduction` | `conclusion` | `conclusion` |
| `testimony` | `text` | `text` |
| `win` | `name` | `name`, `text` |

Reference-aware consumers are case `title`, `premise`, `opening`, and
`players.description`; setting,
character, entity, command, trigger, and flag narrative fields; event `summary`;
fact prose; deduction `conclusion`; character portrayal and testimony text;
command parameter descriptions; narrative command/trigger effect text; win
state name/text; and private character/solution narrator guidance. The exact
list and disclosure class is exported by the registry.
Format 3.3 additionally registers each `solution.questions[*].prompt` as a
baseline player-safe `solution_question:prompt` consumer; expected answer IDs
remain private mechanical state and are never reference-text output.

Baseline player-safe consumers can reach only baseline player-safe paths;
gated player-safe consumers can reach baseline or gated paths. Neither can
reach private narrator guidance. Private narrator guidance may reach public, gated, or private
narrative paths but remains private. References never change the consumer's
existing visibility or fact/testimony gating. IDs, `voice_id`, author notes,
credentials, requirements, effects, truth/status, placement, time, points,
solution references, and other mechanical state are never interpolation
targets. Strict unknown-field validation is unchanged.

## Consumer API

Rust runtimes call `validate_with_supported_features`; browser runtimes call
`validateRepositoryWithFeatures`. Authoring tools that implement every feature
known to this validator may use the existing `validate`/`validateRepository`
entry point. Parsing-only consumers can call `parse_reference_text` and retain
its typed literal/reference segments without inventing a regular expression.
Browser consumers use the async `parseReferenceText` wrapper for the same
parsing-only contract. Expression `start` and `end` positions are zero-based
UTF-8 byte offsets in both APIs; parse failures are returned as a typed result
with the same byte offsets rather than thrown as JavaScript exceptions.

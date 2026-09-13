# Story Format 3.9: subject command overrides

Format 3.9 adds optional `command_overrides` to entities, characters, settings,
events and deductions, plus optional room anchors and embedded PNG/JPEG artwork
for existing `case.map` variants. Command overrides are a mapping of canonical
command IDs to booleans:

```yaml
command_overrides:
  command.open: false
  command.search: true
```

## Map room anchors and artwork

A map variant may add `rooms`, omitted or an empty list when no locations are
placed. Each entry names a `setting` whose type is `room` and an SVG-root-viewBox
coordinate anchor. A room may occur once per variant and at different anchors in
other variants.

```yaml
map:
  variants:
    - id: map.default
      source: maps/house.svg
      rooms:
        - setting: setting.parlor
          anchor: { x: 320, y: 180 }
```

Anchors use finite user units within the root `viewBox`, including negative
origins and all boundaries. Anchored SVGs must use centered `meet` fitting (or
omit `preserveAspectRatio`) and either omit both dimensions or provide matching
unitless/px dimensions. SVG images may use only canonical base64
`data:image/png` or `data:image/jpeg` URLs; external assets are not allowed.

An omitted entry inherits the command defaults; `false` disallows the subject;
`true` permits consideration under those same defaults. Reset removes the entry.
An allow never bypasses types, presence, visibility, inventory, portability,
requirements or command-specific rules. Overrides apply only when the subject
occupies the command's first declared parameter. A character that disallows
Question can remain a topic while questioning someone else. Standard and custom
commands use the same convention. Parameterless and special notebook/Solve flows
retain their behavior and cannot be assigned artificial subject overrides.

The validator rejects unknown IDs, nonboolean values, duplicate YAML keys and
subject/primary-parameter type mismatches. Earlier story formats reject this field.
This implements the authored contract in workspace ADR-013; backend eligibility
remains authoritative for options and execution.

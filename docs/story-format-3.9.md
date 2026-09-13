# Story Format 3.9: subject command overrides

Format 3.9 adds optional `command_overrides` to entities, characters, settings,
events and deductions. It is a mapping of canonical command IDs to booleans:

```yaml
command_overrides:
  command.open: false
  command.search: true
```

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

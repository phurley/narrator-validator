# Story Format 3.1

Story Format 3.1 adds authoritative setting placement and player-scoped
physical presence for characters. It also lets command parameters declare
candidate sources and the `portable` capability filter. Standard mystery
stories opt into the explicit selection policies through
`ruleset.standard_mystery@2.0.0`; version 1 remains immutable.

The normative architecture decision is
[ADR 0001: Story Format 3.1 character presence and command candidates](adr/0001-story-format-3.1-character-presence-and-command-candidates.md).
It defines field semantics, candidate source meanings, compatibility, privacy
invariants, rejected alternatives, and consumer release coordination.

The [repository README](../README.md#format-3-character-placement-presence-and-command-candidates)
contains the compact authoring reference. The complete proposal and acceptance
criteria remain available in
[narrator-validator issue #20](https://github.com/phurley/narrator-validator/issues/20).

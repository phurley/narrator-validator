//! Reserved tagStandard41h12 identities for the physical ENTER scanner
//! control cards.
//!
//! ENTER-1 and ENTER-2 are recognised by the camera before any story deck is
//! parsed, so they must never be assignable to a story `subject` in
//! `deck.yaml`. Consumers (the app's scanner and the printable-fixture
//! generator) need a cheap, unambiguous way to recognise a scanned tag as a
//! scanner-control card *before* clustering it against any resolved deck, so
//! the reservation is exposed as a plain integer comparison rather than a
//! lookup that depends on a resolved card.

/// Reserved tagStandard41h12 ID for the ENTER-1 scanner control card.
///
/// Chosen from the top of the tagStandard41h12 range (0 through 2114) so
/// reservations are unlikely to collide with low, densely-authored IDs in
/// existing decks.
pub const ENTER_1_TAG_ID: i64 = 2114;

/// Reserved tagStandard41h12 ID for the ENTER-2 scanner control card.
pub const ENTER_2_TAG_ID: i64 = 2113;

/// The two tagStandard41h12 IDs permanently reserved for scanner control
/// cards, in a stable, iterable form so guards can assert their count.
pub const RESERVED_SCANNER_CONTROL_TAG_IDS: [(ScannerControlRole, i64); 2] = [
    (ScannerControlRole::Enter1, ENTER_1_TAG_ID),
    (ScannerControlRole::Enter2, ENTER_2_TAG_ID),
];

/// A scanner-control role permanently excluded from story deck assignment.
///
/// These are not `command.*` items, deck subjects, candidates, or
/// game-engine action cards; the app removes them before creating an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScannerControlRole {
    Enter1,
    Enter2,
}

impl ScannerControlRole {
    /// The reserved tagStandard41h12 ID for this role.
    pub const fn tag_id(self) -> i64 {
        match self {
            ScannerControlRole::Enter1 => ENTER_1_TAG_ID,
            ScannerControlRole::Enter2 => ENTER_2_TAG_ID,
        }
    }

    /// The stable, human-readable name for this role, e.g. in diagnostics.
    pub const fn name(self) -> &'static str {
        match self {
            ScannerControlRole::Enter1 => "ENTER-1",
            ScannerControlRole::Enter2 => "ENTER-2",
        }
    }
}

/// Returns the scanner-control role reserving `tag_id`, if any.
///
/// This is the cheap, unambiguous check consumers should use to recognise a
/// scanned tag as a scanner control before resolving it against any deck.
pub fn scanner_control_role_for_tag_id(tag_id: i64) -> Option<ScannerControlRole> {
    RESERVED_SCANNER_CONTROL_TAG_IDS
        .iter()
        .find(|(_, id)| *id == tag_id)
        .map(|(role, _)| *role)
}

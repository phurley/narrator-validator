// GENERATED FILE. DO NOT EDIT BY HAND.
//
// Generated from narrator-validator's src/scanner_control.rs, the single
// authority for the reserved ENTER scanner-control tag IDs. Regenerate with:
//
//   cargo run --example generate_scanner_controls_dart > docs/scanner_control.dart
//
// in narrator-validator, then copy the file into narrator-app at
// lib/core/apriltag/scanner_control.dart.

/// Reserved tagStandard41h12 identities for the physical ENTER scanner
/// control cards. These are never assignable to a story `subject` and must
/// be recognised before any scanned tag is clustered against a resolved deck.
enum ScannerControlRole {
  enter1,
  enter2,
}

extension ScannerControlRoleTagId on ScannerControlRole {
  /// The reserved tagStandard41h12 ID for this role.
  int get tagId {
    switch (this) {
      case ScannerControlRole.enter1:
        return 2114;
      case ScannerControlRole.enter2:
        return 2113;
    }
  }

  /// The stable, human-readable name for this role, e.g. in diagnostics.
  String get label {
    switch (this) {
      case ScannerControlRole.enter1:
        return 'ENTER-1';
      case ScannerControlRole.enter2:
        return 'ENTER-2';
    }
  }
}

/// Returns the scanner-control role reserving [tagId], if any.
ScannerControlRole? scannerControlRoleForTagId(int tagId) {
  for (final role in ScannerControlRole.values) {
    if (role.tagId == tagId) return role;
  }
  return null;
}

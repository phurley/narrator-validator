#!/usr/bin/env bash
# Mirrors .github/workflows/ci.yml. CI pins toolchain 1.80.0.
#
# NOTE: this crate has four downstream consumers (backend Cargo dependency,
# the checked-in WASM package in narrator-author, and the action pinned by
# island_retreat). Passing here is necessary but NOT sufficient for a
# validator change — see "Validator release coordination" in ~/narrator/AGENTS.md.
#
# Usage: scripts/verify.sh [--quick]   (--quick skips wasm + web package)
set -uo pipefail
cd "$(dirname "$0")/.."

QUICK=0
for a in "$@"; do case "$a" in --quick) QUICK=1 ;; *) echo "unknown option: $a" >&2; exit 2 ;; esac; done

LOGDIR=".verify-logs"; rm -rf "$LOGDIR"; mkdir -p "$LOGDIR"
FAILED=0; SUMMARY=""

MSRV="1.80.0"
if rustup toolchain list >/dev/null 2>&1 && rustup toolchain list | grep -q "^$MSRV"; then
  CARGO=(rustup run "$MSRV" cargo); echo "toolchain: pinned $MSRV (matches CI)"
else
  CARGO=(cargo); echo "toolchain: local — CI pins $MSRV; lint differences may be drift."
fi

run() {
  local name="$1"; shift; local log="$LOGDIR/$name.log"
  printf '%-16s ... ' "$name"; local s; s=$(date +%s)
  if "$@" >"$log" 2>&1; then printf 'PASS (%ss)\n' "$(( $(date +%s) - s ))"; SUMMARY+=$(printf '\n  PASS  %-16s %s' "$name" "$log")
  else printf 'FAIL (%ss)  -> %s\n' "$(( $(date +%s) - s ))" "$log"; SUMMARY+=$(printf '\n  FAIL  %-16s %s' "$name" "$log"); FAILED=$((FAILED+1)); fi
}

run fmt    "${CARGO[@]}" fmt --check
run clippy "${CARGO[@]}" clippy --all-targets --all-features -- -D warnings
run test   "${CARGO[@]}" test --all-features

if [ "$QUICK" -eq 0 ]; then
  run wasm-check  "${CARGO[@]}" check --release --target wasm32-unknown-unknown --features wasm --lib
  run web-package node scripts/build-web-package.mjs
  run smoke-test  node typescript/smoke-test.mjs
fi

echo; echo "==== verify summary (narrator-validator) ===="; printf '%s\n' "$SUMMARY"; echo
if [ "$FAILED" -eq 0 ]; then echo "RESULT: PASS"; else echo "RESULT: FAIL ($FAILED step(s)); logs in $LOGDIR/"; fi
exit "$FAILED"

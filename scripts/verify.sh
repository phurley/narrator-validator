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

# Keep in sync with ci.yml's MSRV and Cargo.toml's rust-version.
MSRV="1.80.0"
if rustup toolchain list >/dev/null 2>&1 && rustup toolchain list | grep -q "^$MSRV"; then
  # `rustup run $MSRV cargo` resolves the correct pinned cargo (rustup looks
  # it up itself rather than trusting PATH), but that cargo then shells out
  # to `cargo-clippy`/`clippy-driver`/`rustfmt` via a plain PATH search. If a
  # newer toolchain sits earlier on PATH (e.g. Homebrew's, ahead of rustup's
  # shims) it wins: a newer clippy-driver paired with 1.80's cargo produces
  # hundreds of phantom lint errors for lints that don't exist in 1.80, and a
  # newer rustfmt formats by different rules than CI, tempting a "fix" that
  # reformats the whole repo. Homebrew's toolchain also has no
  # wasm32-unknown-unknown std, so wasm-check/web-package fail outright.
  #
  # Fix: put the MSRV toolchain's own bin/ (which has matching clippy-driver,
  # rustfmt, and the wasm target) ahead of PATH for every invocation, and
  # export it so child processes (build-web-package.mjs shells out to a
  # plain `cargo` itself) inherit the same resolution.
  TOOLCHAIN_BIN="$(rustup run "$MSRV" rustc --print sysroot 2>/dev/null)/bin"
  export PATH="$TOOLCHAIN_BIN:$PATH"
  CARGO=(rustup run "$MSRV" cargo)
  echo "toolchain: pinned MSRV $MSRV (matches CI)"

  # Assert the driver clippy will actually use matches the toolchain we just
  # selected, rather than assuming it. Without this check, a PATH regression
  # here silently goes back to phantom errors (or worse, a real regression
  # masked by version-mismatch noise).
  DRIVER_VERSION=$("${CARGO[@]}" clippy --version 2>&1 | head -1)
  MSRV_MINOR=$(printf '%s' "$MSRV" | cut -d. -f2)
  case "$DRIVER_VERSION" in
    "clippy 0.1.${MSRV_MINOR}"*) : ;;
    *)
      echo "FATAL: clippy-driver mismatch." >&2
      echo "  expected: clippy 0.1.${MSRV_MINOR}.x (matching MSRV $MSRV)" >&2
      echo "  got:      $DRIVER_VERSION" >&2
      echo "  \$PATH puts a different clippy-driver ahead of $TOOLCHAIN_BIN." >&2
      exit 2
      ;;
  esac
  echo "clippy-driver: $DRIVER_VERSION (matches MSRV $MSRV)"

  # Same class of trap as the clippy-driver PATH issue above: a newer
  # rustfmt on PATH would format-check with different rules than the one
  # that pins CI, silently swapping drift for different drift instead of
  # fixing it. Assert the version rustfmt reports through CARGO matches the
  # pinned toolchain rather than assuming the PATH prepend above covers it.
  RUSTFMT_VERSION=$("${CARGO[@]}" fmt --version 2>&1 | head -1)
  case "$RUSTFMT_VERSION" in
    "rustfmt 1.7."*) : ;;
    *)
      echo "FATAL: rustfmt version mismatch." >&2
      echo "  expected: rustfmt 1.7.x (matching MSRV $MSRV)" >&2
      echo "  got:      $RUSTFMT_VERSION" >&2
      exit 2
      ;;
  esac
  echo "rustfmt: $RUSTFMT_VERSION (matches MSRV $MSRV)"
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

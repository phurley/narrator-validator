#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

expected="reproducible rustflags contract: 5/5 cases"
output="$(node scripts/test-reproducible-rustflags.mjs)"
printf '%s\n' "$output"

if [ "$output" != "$expected" ]; then
  echo "expected exact contract marker: $expected" >&2
  exit 1
fi

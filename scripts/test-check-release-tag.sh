#!/usr/bin/env bash
set -euo pipefail

root=$(mktemp -d)
trap 'rm -rf "$root"' EXIT
repo="$root/repo"
mkdir -p "$repo"
git -C "$repo" init -q
git -C "$repo" config user.name "Release Tag Gate Test"
git -C "$repo" config user.email "release-tag-gate@example.invalid"

write_manifest() {
  local version=$1
  printf '[package]\nname = "fixture"\nversion = "%s"\n' "$version" > "$repo/Cargo.toml"
}

commit_manifest() {
  local message=$1
  git -C "$repo" add Cargo.toml
  git -C "$repo" commit -qm "$message"
}

expect_pass() {
  local name=$1
  shift
  if "$@" >"$root/stdout" 2>"$root/stderr"; then
    printf 'PASS: %s\n' "$name"
  else
    printf 'FAIL: %s unexpectedly failed\n' "$name" >&2
    cat "$root/stdout" "$root/stderr" >&2
    exit 1
  fi
}

expect_fail() {
  local name=$1 expected=$2
  shift 2
  if "$@" >"$root/stdout" 2>"$root/stderr"; then
    printf 'FAIL: %s unexpectedly passed\n' "$name" >&2
    cat "$root/stdout" >&2
    exit 1
  fi
  if ! grep -Fq "$expected" "$root/stderr"; then
    printf 'FAIL: %s did not report %q\n' "$name" "$expected" >&2
    cat "$root/stderr" >&2
    exit 1
  fi
  printf 'PASS: %s\n' "$name"
}

write_manifest 1.0.0
commit_manifest initial
expect_fail "missing parent history fails closed" "fetch at least two commits" \
  bash scripts/check-release-tag.sh "$repo"

printf '\n# no version change\n' >> "$repo/Cargo.toml"
commit_manifest unchanged-version
expect_pass "ordinary commits do not require tags" bash scripts/check-release-tag.sh "$repo"

write_manifest 1.1.0
commit_manifest untagged-bump
bump_sha=$(git -C "$repo" rev-parse HEAD)
expect_fail "untagged version bump fails" "git tag v1.1.0 $bump_sha && git push origin v1.1.0" \
  bash scripts/check-release-tag.sh "$repo"

git -C "$repo" tag v1
expect_fail "moving major tag is insufficient" "exact release tag v1.1.0 does not exist" \
  bash scripts/check-release-tag.sh "$repo"

git -C "$repo" tag v1.1.0 HEAD^
expect_fail "off-commit exact tag fails" "points at" \
  bash scripts/check-release-tag.sh "$repo"
git -C "$repo" tag -d v1.1.0 >/dev/null

git -C "$repo" tag -a v1.1.0 -m "fixture release"
expect_pass "annotated exact tag at bump commit passes" bash scripts/check-release-tag.sh "$repo"

echo "PASS: 6 release-tag gate cases"

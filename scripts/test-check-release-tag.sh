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

# Use commit-tree to create precisely controlled integration graphs, independent
# of merge-strategy behavior and without moving the immutable release tag.
base_sha=$(git -C "$repo" rev-parse "$bump_sha^")
release_tree=$(git -C "$repo" rev-parse "$bump_sha^{tree}")
merge_sha=$(printf 'integrate tagged release\n' | git -C "$repo" commit-tree "$release_tree" -p "$base_sha" -p "$bump_sha")
expect_pass "identical-tree merge of tagged direct parent passes" \
  bash scripts/check-release-tag.sh "$repo" "$merge_sha"

# A merge that changes even unrelated tracked content is a different release.
printf 'changed after release\n' > "$repo/extra.txt"
git -C "$repo" add extra.txt
changed_tree=$(git -C "$repo" write-tree)
changed_merge=$(printf 'changed integration\n' | git -C "$repo" commit-tree "$changed_tree" -p "$base_sha" -p "$bump_sha")
expect_fail "changed-tree merge of tagged parent fails" "identical-tree direct parent" \
  bash scripts/check-release-tag.sh "$repo" "$changed_merge"

# Equal trees alone do not authorize an unrelated/reconstructed version bump.
unrelated=$(printf 'reconstructed release\n' | git -C "$repo" commit-tree "$release_tree" -p "$base_sha")
expect_fail "same-tree ordinary commit cannot borrow release tag" "points at" \
  bash scripts/check-release-tag.sh "$repo" "$unrelated"
descendant=$(printf 'release descendant\n' | git -C "$repo" commit-tree "$release_tree" -p "$bump_sha")
indirect_merge=$(printf 'tag only on ancestor\n' | git -C "$repo" commit-tree "$release_tree" -p "$base_sha" -p "$descendant")
expect_fail "same-tree merge requires directly tagged parent" "points at" \
  bash scripts/check-release-tag.sh "$repo" "$indirect_merge"

git -C "$repo" tag -d v1.1.0 >/dev/null
expect_fail "identical-tree merge without release tag fails" "exact release tag v1.1.0 does not exist" \
  bash scripts/check-release-tag.sh "$repo" "$merge_sha"

echo "PASS: 11 release-tag gate cases"

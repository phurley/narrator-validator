#!/usr/bin/env bash
# Fail when a commit changes the crate version without an exact release tag.
set -euo pipefail

repo="${1:-.}"
commit="${2:-HEAD}"

git_in_repo() {
  git -C "$repo" "$@"
}

commit_sha=$(git_in_repo rev-parse "$commit^{commit}")
parent_sha=$(git_in_repo rev-list --parents -n 1 "$commit_sha" | awk '{ print $2 }')

manifest_version() {
  local revision=$1
  git_in_repo show "$revision:Cargo.toml" | awk '
    /^\[package\][[:space:]]*$/ { in_package = 1; next }
    /^\[/ && in_package { exit }
    in_package && $1 == "version" && $2 == "=" {
      value = $3
      gsub(/^"|"$/, "", value)
      print value
      exit
    }
  '
}

version=$(manifest_version "$commit_sha")
if [[ -z "$version" ]]; then
  echo "FAIL: could not read the package version from Cargo.toml at $commit_sha" >&2
  exit 1
fi

if [[ -z "$parent_sha" ]]; then
  echo "FAIL: cannot inspect the parent of $commit_sha; fetch at least two commits before running the release-tag gate" >&2
  exit 2
fi

parent_version=$(manifest_version "$parent_sha")
if [[ "$version" == "$parent_version" ]]; then
  echo "OK: Cargo.toml version is unchanged at $version"
  exit 0
fi

tag="v$version"
tag_commit=$(git_in_repo rev-parse --verify "refs/tags/$tag^{}" 2>/dev/null || true)
if [[ -z "$tag_commit" ]]; then
  cat >&2 <<EOF
FAIL: Cargo.toml changed from $parent_version to $version at $commit_sha,
but the exact release tag $tag does not exist.

Create and push the release tag at this commit:
  git tag $tag $commit_sha && git push origin $tag

The moving v1 tag is not a release tag and does not satisfy this gate.
EOF
  exit 1
fi

if [[ "$tag_commit" != "$commit_sha" ]]; then
  cat >&2 <<EOF
FAIL: Cargo.toml changed from $parent_version to $version at $commit_sha,
but $tag points at $tag_commit instead of the version-bump commit.

Release tags are immutable. Correct the version on main and create a new exact
release tag at its version-bump commit; do not move or force-push $tag.
EOF
  exit 1
fi

echo "OK: Cargo.toml changed from $parent_version to $version and $tag points at $commit_sha"

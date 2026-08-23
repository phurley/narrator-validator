# Releasing narrator-validator

Every released crate version has an immutable exact tag at the commit that
changed `Cargo.toml`: version `X.Y.Z` requires `vX.Y.Z`. The moving `v1` action
tag is a consumer convenience and never substitutes for that release tag.

Pull-request CI exercises the gate's fixture harness but does not require a tag
for work-in-progress version changes. On a push to `main`, CI fetches tags and
runs `scripts/check-release-tag.sh`. If the version changed, the exact tag must
exist and peel to that same commit. A tag with the right name on an older commit
fails; release tags must not be moved or force-pushed.

Immediately after the version-bump commit lands on `main`, tag that exact commit
and push the tag:

```sh
git tag vX.Y.Z <version-bump-commit>
git push origin vX.Y.Z
```

Then complete the cross-repository release and story-validation procedure in
the workspace `AGENTS.md`. GitHub Free does not provide required-check branch
protection for the private consumers, so this main-branch check is a loud
detector, not a server-side prohibition: a delayed tag can briefly leave the
main workflow red. Push the tag as part of the same release operation and rerun
the failed main workflow if it began before the tag was visible.

`scripts/test-check-release-tag.sh` proves the gate fails for missing parent
history, an untagged bump, a moving-`v1`-only bump, and an exact tag on the wrong
commit. It also proves ordinary commits and a correctly tagged bump pass.

# CI and Release

CircleCI is the canonical CI and release system for linear-cli. The checked-in
configuration is `.circleci/config.yml`; it owns branch validation and tagged
release artifacts. GitHub Actions is retained only as an explicitly dispatched
fallback for CI while repository protection rules are migrated.

## Language

**CI**: The CircleCI `ci` workflow runs the locked secure-storage test suite,
formatting check, clippy with warnings denied, a default-feature build, and
release-target consistency checks. Tagged releases run this same test gate.

**Release**: A semver tag matching `vX.Y.Z` starts five parallel target builds.
The release gate requires that the tag version equals `Cargo.toml`, that the
exact five archives exist, that each archive has the expected target format and
root binary, and that native binaries report the tagged version. Cross-target
jobs carry an independently checked version proof from their tagged source.
Only then are the GitHub release assets uploaded. Publication re-validates the
workspace and adds only missing assets; it never overwrites an existing asset
with `--clobber`. Crates.io publication is a separate opt-in lane so its
missing credentials cannot prevent the verified GitHub release from completing.

**Target set**: `.circleci/release-targets.json` is the source of truth for
`x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`,
`x86_64-pc-windows-msvc`, `x86_64-apple-darwin`, and `aarch64-apple-darwin`.

**Release contexts**: The existing CircleCI context `gh-release-publisher` must
provide `gh_token` for GitHub release uploads. The optional
`cargo-release-publisher` context must provide `CARGO_REGISTRY_TOKEN` for
crates.io. Those credentials are used only by downstream release jobs after
verification; crates.io publication requires the explicit `publish_crate=true`
pipeline parameter.

**Manual fallback**: When CircleCI is unavailable, follow
`docs/manual-release.md` and preserve the same five-asset/version gate.

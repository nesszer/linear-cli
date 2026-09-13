# CI Operator Guide

CircleCI is the canonical CI and release path. See
`.circleci/README.md` and `.circleci/config.yml` for the checked-in pipeline.

## Branch and pull-request CI

The CircleCI `ci` workflow runs locked tests with `secure-storage`, formatting,
clippy with warnings denied, a default-feature build, and release-target
consistency checks on Linux. It runs for non-release refs.

## Tagged releases

Push a tag matching `vX.Y.Z`. CircleCI builds these five archives:

- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`
- `x86_64-pc-windows-msvc`
- `x86_64-apple-darwin`
- `aarch64-apple-darwin`

The release verifier fails closed unless the tag matches `Cargo.toml`, all
five archives are present, each archive contains only the expected root binary,
the target formats match, and native binaries report the tagged version.
Cross-target jobs carry an independently checked version proof from the tagged
source. The same locked test/format/clippy job runs on the tag before the build
matrix. The GitHub release upload re-validates the artifacts and only adds
missing assets; an unexpected or mismatched existing asset stops the job without
an overwrite. Crates.io publication is an explicit opt-in lane so a missing
registry credential cannot turn an otherwise valid GitHub release into an
opaque pre-start failure.

The repository-owned target matrix is `.circleci/release-targets.json`; CI
checks that each CircleCI release build job uses the corresponding manifest
target, while the verifier and release manifest consume the same file.

## Credentials and plan prerequisites

The existing restricted CircleCI context `gh-release-publisher` must contain:

- `gh_token`: permission to create or update releases in `nesszer/linear-cli`.

For crates.io publication, create a separate restricted context
`cargo-release-publisher` with:

- `CARGO_REGISTRY_TOKEN`: permission to publish `linear-cli` on crates.io.

The automatic tag trigger publishes the GitHub release. Once the Cargo context
exists, run the tagged pipeline with the `publish_crate=true` pipeline
parameter to publish the crate as a separate downstream lane.

The macOS and Windows executors also need to be enabled for the CircleCI
organization/plan. The pipeline cannot prove that external project wiring or
executor entitlements exist from this repository alone.

## Legacy fallback

The GitHub Actions CI file is manual-dispatch only during the migration. The
old GitHub Actions release workflow is disabled so it cannot publish a partial
or incorrectly tagged asset set.

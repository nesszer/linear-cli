# CircleCI CI and release pipeline

CircleCI is the canonical CI and release path for this repository.

## CI

The `ci` workflow runs the locked test suite, formatting check, clippy with
warnings denied, a default-feature build, and release-target consistency checks
on Linux for non-release refs. Tagged releases run the same `test` job before
the release build matrix.

## Release

Push an annotated or lightweight tag matching `vX.Y.Z`. The release workflow:

1. Builds the five supported targets in parallel:
   - `x86_64-unknown-linux-gnu`
   - `aarch64-unknown-linux-gnu`
   - `x86_64-pc-windows-msvc`
   - `x86_64-apple-darwin`
   - `aarch64-apple-darwin`
2. Runs the locked test, format, and clippy gate.
3. Verifies that the tag version matches `Cargo.toml`.
4. Requires exactly those five archives, checks their archive roots and target
   formats, verifies native Linux/Windows binaries with `--version`, and
   generates `SHA256SUMS` plus `release-manifest.json`.
5. Re-validates the workspace and uploads only missing, digest-matching assets
   to the GitHub release; existing mismatches or unexpected assets fail closed.
6. Optionally publishes the matching crate version to crates.io when the
   pipeline is explicitly triggered with `publish_crate=true`.

The five-target matrix is owned by `.circleci/release-targets.json`. The
checked-in CircleCI build jobs are validated against that manifest by
`.circleci/release_targets.py`.

The GitHub release step is downstream of the five-asset gate and uses the
existing CircleCI context `gh-release-publisher` containing:

- `gh_token`: a GitHub token allowed to create/update releases in this repo.

Crates.io publishing is a separate, opt-in lane. Configure a dedicated
CircleCI context named `cargo-release-publisher` containing:

- `CARGO_REGISTRY_TOKEN`: the crates.io publish token.

The automatic tag trigger publishes GitHub assets only. After configuring the
Cargo context, trigger the tagged pipeline with
`--param publish_crate=true` to publish the crate. The local CircleCI CLI can
validate the file with:

```bash
circleci config validate .circleci/config.yml
```

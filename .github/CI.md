# CI Operator Guide

linear-cli shares a Blacksmith free-tier runner pool with sibling repos. This
guide tells an operator how to control who runs.

## Budget mode

Set the org/repo variable `CI_BUDGET_MODE` (Settings → Secrets and variables →
Actions → Variables). Unset/empty is treated as `normal`.

| Mode     | PR Check | Release (dispatch) | When to use                                   |
|----------|----------|--------------------|-----------------------------------------------|
| `normal` | runs     | runs               | Default. A real gate on every PR.             |
| `thin`   | skipped  | runs               | Defer linear-cli so Win-CodexBar gets the pool.|
| `off`    | skipped  | skipped            | Pause all CI for this repo.                   |

- PR Check `if`: `vars.CI_BUDGET_MODE != 'off' && vars.CI_BUDGET_MODE != 'thin'`
- Release `if`: `vars.CI_BUDGET_MODE != 'off'`

## Intended split

The shared Blacksmith free tier is ~3000 runner-minutes/month. The intended
share of that pool across repos is roughly **60 / 30 / 10**:
- **~60%** → Win-CodexBar (the priority repo).
- **~30%** → linear-cli.
- **~10%** → buffer for spikes and overruns.

This is a share of pool minutes per repo, **not** calendar time spent in each
`CI_BUDGET_MODE`. The budget mode is the knob that holds linear-cli near its
~30%: `thin` skips the **entire** linear-cli PR Check job (not individual
matrix legs), and `off` pauses all of this repo's CI. It is a planning target,
not a hard cap — move to `thin` whenever Win-CodexBar has open PRs competing
for the pool.

## $0 spend alert

Blacksmith bills the free pool per runner-minute, and **Windows bills ~2x
Linux**. PR Check is Linux-only and macOS release builds stay on GitHub-hosted
`macos-latest` specifically to avoid burning the Blacksmith pool. If you see
free-tier minutes dropping faster than the 60/30/10 plan accounts for, set
`CI_BUDGET_MODE=off` on the non-priority repo first.

## Release

Releases are **local by default**. See `docs/manual-release.md` for the
`cargo publish` + `gh release` sequence. The `release.yml` workflow is
**dispatch-only** (Actions → Release → Run workflow) and never auto-runs on a
tag or GitHub release. When dispatched:

- Linux x86_64 / aarch64 → `blacksmith-4vcpu-ubuntu-2404`
- Windows x86_64 → `blacksmith-4vcpu-windows-2025`
- macOS x86_64 / aarch64 → `macos-latest` (GitHub-hosted, not Blacksmith)

A dispatched release still runs in `thin` mode (operator intent overrides the
defer), but is skipped in `off`.

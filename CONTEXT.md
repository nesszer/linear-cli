# Shared CI Budget

linear-cli shares a Blacksmith free-tier runner pool with other repos (notably Win-CodexBar). To keep one repo from starving the others, CI runs are gated by a single org/repo variable, `CI_BUDGET_MODE`, and PR Check is trimmed to a single Linux job. Releases are local by default and the release workflow is dispatch-only. This file is the shared vocabulary so operators and contributors mean the same thing across repos.

## Language

**PR Check**: The gate that runs on every push/PR. For linear-cli it is a single Linux job on `blacksmith-4vcpu-ubuntu-2404`: `cargo test`, `cargo fmt --check`, `cargo clippy` (all with `--features secure-storage`), plus one default-features `cargo build`. Windows/macOS are intentionally absent from PR Check to protect the shared pool.
_Avoid_: "the test matrix", "CI" (CI is the whole workflow, not just the gate).

**Release**: Cutting and publishing a version — `cargo publish` plus GitHub release assets. For linear-cli this is local by default; the `release.yml` workflow exists only as a dispatch fallback and never auto-runs on tag/release.
_Avoid_: "deploy" (nothing is deployed; binaries are uploaded to a GitHub release).

**Blacksmith Pool**: The shared free tier of Blacksmith runners (`blacksmith-4vcpu-ubuntu-2404`, `blacksmith-4vcpu-windows-2025`) that linear-cli and sibling repos draw from. Windows builds bill ~2x against this pool versus Linux, which is why PR Check stays Linux-only and macOS release builds stay on GitHub-hosted `macos-latest`.
_Avoid_: "Blacksmith runners" without naming the shared-pool constraint; "the cluster".

**Local Release**: The default way to release — an operator runs `cargo publish` and `gh release create/upload` by hand (see `docs/manual-release.md`). The dispatch-only `release.yml` is a fallback, not the primary path.
_Avoid_: "manual release" interchangeably with the workflow; the workflow is "dispatch release".

**Budget Mode**: The value of org/repo variable `CI_BUDGET_MODE` — `normal` (PR Check runs), `thin` (the **entire** linear-cli PR Check job is skipped — not individual matrix legs — so Win-CodexBar gets priority), `off` (all CI skips). Unset/empty is treated as `normal`. PR Check honors `thin`; a manually dispatched Release still runs in `thin` because the operator asked for it. The intended share of the ~3000 free pool minutes/month is roughly 60 / 30 / 10 (Win-CodexBar / linear-cli / buffer) — a per-repo minute share, not calendar time spent in each mode.
_Avoid_: "spend mode", "throttle".

# Manual Release Guide

Use this guide when GitHub Actions is unavailable or when release assets need to be backfilled by hand.

## Rules

1. Publish the crate to crates.io before creating or updating the matching GitHub release.
2. Only attach binaries built from the exact source for that version tag.
3. Keep Windows release assets on `x86_64-pc-windows-msvc` so `cargo-binstall` metadata stays correct.

## Version Order

- Repair a broken old release from its exact tagged source.
- Cut the next release from the current branch only after the old release is consistent again.

## Repairing `v0.3.16`

Build from commit `84f522199e1a5c9332fca76ccefeae924c92115e`.

### Linux x86_64

```bash
cargo build --release --target x86_64-unknown-linux-gnu
tar -C target/x86_64-unknown-linux-gnu/release -czf linear-cli-x86_64-unknown-linux-gnu.tar.gz linear-cli
```

### Linux aarch64

`v0.3.16` still uses OpenSSL-backed TLS, so build it with a temporary `Cross.toml` instead of editing the old tag:

```toml
[target.aarch64-unknown-linux-gnu]
pre-build = [
  "dpkg --add-architecture $CROSS_DEB_ARCH",
  "apt-get update && apt-get --assume-yes install libssl-dev:$CROSS_DEB_ARCH"
]
```

```bash
CROSS_CONFIG=/absolute/path/to/Cross.toml cross build --release --target aarch64-unknown-linux-gnu
tar -C target/aarch64-unknown-linux-gnu/release -czf linear-cli-aarch64-unknown-linux-gnu.tar.gz linear-cli
```

### Windows x86_64 (MSVC)

On Ubuntu, `cargo xwin` also needs LLVM's MSVC-compatible entrypoints available on `PATH`:

```bash
sudo apt-get update
sudo apt-get install -y clang lld
sudo ln -sf /usr/bin/clang-18 /usr/local/bin/clang-cl
sudo ln -sf /usr/bin/llvm-lib-18 /usr/local/bin/llvm-lib
sudo ln -sf /usr/bin/llvm-ar-18 /usr/local/bin/llvm-ar
```

```bash
rustup target add x86_64-pc-windows-msvc
cargo xwin build --release --target x86_64-pc-windows-msvc
cd target/x86_64-pc-windows-msvc/release && 7z a ../../../linear-cli-x86_64-pc-windows-msvc.zip linear-cli.exe
```

### macOS on a real Mac

```bash
rustup target add x86_64-apple-darwin aarch64-apple-darwin
cargo build --release --target x86_64-apple-darwin
tar -C target/x86_64-apple-darwin/release -czf linear-cli-x86_64-apple-darwin.tar.gz linear-cli

cargo build --release --target aarch64-apple-darwin
tar -C target/aarch64-apple-darwin/release -czf linear-cli-aarch64-apple-darwin.tar.gz linear-cli
```

### Publish and upload

```bash
cargo publish
gh release upload v0.3.16 \
  linear-cli-x86_64-unknown-linux-gnu.tar.gz \
  linear-cli-aarch64-unknown-linux-gnu.tar.gz \
  linear-cli-x86_64-pc-windows-msvc.zip \
  linear-cli-x86_64-apple-darwin.tar.gz \
  linear-cli-aarch64-apple-darwin.tar.gz
```

Confirm crates.io shows `0.3.16` before moving on.

## Releasing `v0.3.17` and newer

Current `master` uses `reqwest` with `rustls`. Official binaries should build with
`--features secure-storage` so Keychain / Credential Manager / Secret Service work.

Linux builds need `libdbus-1-dev` (and `pkg-config`). For aarch64 cross builds, use the
repo-root `Cross.toml` which installs `libdbus-1-dev:$CROSS_DEB_ARCH`.

### Local builds

```bash
sudo apt-get install -y libdbus-1-dev pkg-config   # Linux hosts only

cargo build --release --features secure-storage --target x86_64-unknown-linux-gnu
tar -C target/x86_64-unknown-linux-gnu/release -czf linear-cli-x86_64-unknown-linux-gnu.tar.gz linear-cli

cross build --release --features secure-storage --target aarch64-unknown-linux-gnu
tar -C target/aarch64-unknown-linux-gnu/release -czf linear-cli-aarch64-unknown-linux-gnu.tar.gz linear-cli

cargo xwin build --release --features secure-storage --target x86_64-pc-windows-msvc
cd target/x86_64-pc-windows-msvc/release && 7z a ../../../linear-cli-x86_64-pc-windows-msvc.zip linear-cli.exe
```

Build both Apple targets on a Mac with `--features secure-storage` as well.

### Publish first, then release

```bash
cargo publish
gh release create v0.3.17 --title v0.3.17 --notes "Manual release."
gh release upload v0.3.17 \
  linear-cli-x86_64-unknown-linux-gnu.tar.gz \
  linear-cli-aarch64-unknown-linux-gnu.tar.gz \
  linear-cli-x86_64-pc-windows-msvc.zip \
  linear-cli-x86_64-apple-darwin.tar.gz \
  linear-cli-aarch64-apple-darwin.tar.gz
```

## Final Checks

```bash
cargo search linear-cli --limit 1
gh release view v0.3.17
```

The crates.io version and the GitHub release tag should match before announcing the release.

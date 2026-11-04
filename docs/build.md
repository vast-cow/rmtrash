# Build and release configuration

[README](../README.md) · [Usage](usage.md) · [Storage](storage.md) · [Build](build.md)

Install a stable Rust toolchain, then run `cargo build --release`. Install with `cargo install --path .`.

## Automated GitHub releases

The [release workflow](../.github/workflows/release.yml) runs on pushes to `main`.
It can also be started from the Actions tab with **Run workflow**, selecting
`main`. Runs are serialized to prevent concurrent version selection.

Each run tests and builds with stable Rust and `Cargo.lock` on native GitHub
Ubuntu runners for `x86_64-unknown-linux-musl` and
`aarch64-unknown-linux-musl`. The ARM build uses `ubuntu-24.04-arm`, which is
available for public repositories; private repositories need an eligible ARM
runner. Both builds must succeed before publication.

The workflow fetches all tags and selects the numerically highest exact
`vMAJOR.MINOR.PATCH` tag, then increments its patch number. For example,
`v1.10.9` becomes `v1.10.10`. Prerelease tags, build metadata, leading-zero
versions, and other names are ignored. With no matching tags, it uses `v0.0.1`.
Release tags are independent of the package version in `Cargo.toml` and the
version printed by `rmtrash --version`.

The workflow creates the tag at the triggering commit and publishes a GitHub
release with generated notes and only the two executables:
`rmtrash-x86_64-unknown-linux-musl` and `rmtrash-aarch64-unknown-linux-musl`.
Download the executable matching your architecture, rename it to `rmtrash`,
mark it executable with `chmod +x rmtrash`, and place it on your `PATH`.

Publication uses the built-in `GITHUB_TOKEN` with `contents: write`; no personal
access token is required. Repository rules must allow this token to create
release tags and releases. Failed builds create neither tags nor releases.

## Release profile

The release profile favors a small executable: size optimization (`z`), fat LTO,
one codegen unit, aborting panics, and stripped symbols. LTO and a single codegen
unit increase build time. Unexpected panics terminate immediately without stack
unwinding or running destructors; normal filesystem and CLI errors still use the
existing error handling. Stripping limits symbol-based debugging. Development
builds retain the default settings.

## Binary size comparison

Size comparison using Rust 1.85.1 on `x86_64-unknown-linux-gnu`, the same
`Cargo.lock`, and `cargo build --release --locked`:

| Release configuration | Executable bytes |
| --- | ---: |
| Original Cargo defaults | 1,440,952 |
| Defaults, with symbols stripped | 1,165,824 |
| `s`, fat LTO, one codegen unit, abort, stripped | 756,224 |
| `z`, fat LTO, one codegen unit, abort, stripped (selected) | 739,840 |
| `3`, fat LTO, one codegen unit, abort, stripped | 883,200 |
| `z`, thin LTO, one codegen unit, abort, stripped | 883,200 |
| `s`, thin LTO, one codegen unit, abort, stripped | 887,296 |
| `s`, fat LTO, one codegen unit, unwind, stripped | 813,568 |
| `s`, fat LTO, 16 codegen units, abort, stripped | 776,704 |

The selected configuration reduces the executable by 701,112 bytes (48.7%).
Sizes vary with the toolchain and target. To compare alternatives locally,
override individual settings, for example
`CARGO_PROFILE_RELEASE_OPT_LEVEL=s cargo build --release --locked`, and measure
with `stat -c %s target/release/rmtrash`.

Dependency features are retained to preserve behavior: Clap's defaults provide
help, usage, colors, error context, and suggestions; its `derive` and `env`
features support the CLI implementation and `RMTRASH_DIR`. Chrono already
disables default features and enables `clock` for local deletion timestamps.

## Recorded validation and performance

The following results were recorded for the release-profile comparison.

Validation: all 12 integration tests passed with both `cargo test --locked` and
`cargo test --release --locked`. The actual release executable also matched the
baseline's output and exit status in eight CLI cases and passed a
trash/restore/force/empty smoke check. In 60 alternating, warm-cache runs listing
500 entries, median elapsed time was 2.474 ms before and 2.619 ms after (about
6% slower, or 0.145 ms). This local measurement is not a general performance
guarantee.


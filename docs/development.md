# Development Guide

This document is for contributors working from the source tree.

## Toolchain

Install the Rust toolchain declared in `mise.toml`:

```sh
mise install
```

## Build and Test

Run the same checks used by CI before opening a pull request:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
cargo build --locked
```

## Local CLI Checks

Run the CLI from source:

```sh
cargo run -- list --json
```

Install the local checkout as `git-ws`:

```sh
cargo install --path .
```

After installation, Git can invoke the binary as:

```sh
git ws
```

## Release

See [release.md](release.md) for the release procedure.

# Repository Guidelines

## Project Structure & Module Organization

This repository contains `git-ws`, a Rust CLI helper for Git worktrees. The entry point is `src/main.rs`, reusable logic is exported from `src/lib.rs`, and feature-specific code is split across modules such as `src/git.rs`, `src/worktree.rs`, `src/github.rs`, and `src/cleanup.rs`. Integration tests live in `tests/`, with shared test-repository setup in `tests/support/mod.rs`. Check `docs/release.md` for release procedures, `scripts/` for helper scripts, and `.github/workflows/` for CI.

## Build, Test, and Development Commands

- `mise install`: Install Rust 1.95.0 as specified by `mise.toml`.
- `cargo fmt --check`: Verify that rustfmt would not produce formatting changes.
- `cargo clippy --all-targets -- -D warnings`: Lint all targets with warnings treated as errors.
- `cargo test --locked`: Run the full test suite while respecting `Cargo.lock`.
- `cargo build --locked`: Confirm that the debug build compiles.
- `cargo run -- list --json`: Example command for checking CLI behavior locally.
- `cargo install --path .`: Install the binary locally so it can run as `git ws`.

## Coding Style & Naming Conventions

Follow Rust 2024 edition conventions and standard rustfmt output. Do not manually disturb indentation or import ordering, and run `cargo fmt` before submitting changes. Use `snake_case` for modules, functions, and variables; `PascalCase` for types and enums; and `SCREAMING_SNAKE_CASE` for constants. For fallible operations, follow the existing code by using `anyhow::Result` with contextual errors. Keep user-facing Git and CLI output short and specific.

## Testing Guidelines

Add new CLI behavior tests to `tests/cli.rs`, and add pure logic or parser tests to `tests/core.rs`. For behavior involving external commands such as GitHub CLI integration, prefer the helpers in `tests/github_cli.rs` and `tests/support/mod.rs`. Name tests after the expected behavior, such as `behavior_under_condition`. Tests that use worktrees or Git repositories must be isolated with `tempfile` and must not depend on a developer's real repositories.

## Commit & Pull Request Guidelines

The history uses Conventional Commits such as `feat: ...`, `fix: ...`, `ci: ...`, and `chore: ...`. PRs should describe the purpose of the change, user-visible CLI changes, and the verification commands that were run. For changes related to issue or PR worktree features, link the relevant number. If an interactive picker or output format changes, include before-and-after examples. For release changes, follow `scripts/version.sh X.Y.Z` and the procedure in `docs/release.md`.

## Security & Configuration Tips

The `[init].on_create` setting in `.git-ws.toml` can execute arbitrary commands, so do not weaken trust-confirmation behavior. Tests and implementation code must not depend on a user's global Git configuration, existing worktrees, or credentials.

## Agent-Specific Instructions

Use English for explanations, work reports, and review comments in this repository. If there are existing uncommitted changes, do not revert files outside the requested scope, and edit only the necessary diff.

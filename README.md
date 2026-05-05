# git-ws

Fast Git branch and worktree workspace helper.

## Install

```sh
cargo install --path .
```

The binary is named `git-ws`, so Git can invoke it as:

```sh
git ws
```

Optional compatibility aliases:

```sh
git config --global alias.co "ws open"
git config --global alias.cleanup "ws cleanup"
git config --global alias.main "ws main"
```

For automatic `cd` after selecting or creating a worktree:

```fish
git ws init-shell fish | source
```

```sh
eval "$(git ws init-shell zsh)"
```

## Commands

```sh
git ws [open] [query] [--type all|worktree|local|remote]
git ws list [--json] [--type all|worktree|local|remote]
git ws new <branch> [--from <ref>] [--path <path>] [--no-init]
git ws issue [number|url] [--base <ref>] [--branch <name>] [--no-init]
git ws pr [number|url] [--branch <name>] [--no-init]
git ws cleanup [--dry-run] [--yes] [--force] [--json]
git ws main
git ws init-shell fish|zsh|bash
git ws doctor
```

Run `git ws`, `git ws issue`, or `git ws pr` without a query/number to open an
interactive fuzzy picker. To pick only remote branches, use:

```sh
git ws open --type remote
```

## Repository Config

Create `.git-ws.toml` at the repository root:

```toml
[worktree]
base_dir = ".worktrees"

[init]
on_create = [
  "mise install",
  "pnpm install",
]
```

Init commands are shown and confirmed the first time per repository/config hash.

## Release

Release builds are produced by GitHub Actions when a `vX.Y.Z` tag is pushed.
See [docs/release.md](docs/release.md) for the full procedure.

## License

MIT. See [LICENSE](LICENSE).

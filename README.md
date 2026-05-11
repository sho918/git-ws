# git-ws

Fast Git branch and worktree workspace helper.

## Install

Install with Homebrew:

```sh
brew tap sho918/git-ws https://github.com/sho918/git-ws
brew install sho918/git-ws/git-ws
```

Alternatively, download the archive for your platform from
[GitHub Releases](https://github.com/sho918/git-ws/releases), extract it, and
place the `git-ws` binary somewhere on your `PATH`.

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

Interactive and TTY table views show dense columns for the selected workflow:

- `git ws` and `git ws list`: status, branch name, upstream, tracking state,
  head, path, and action.
- `git ws issue`: number, title, author, labels, updated date, and planned
  branch.
- `git ws pr`: number, title, author, head, base, review state, and updated
  date.
- `git ws cleanup`: deletion selection, branch, disposition, reasons, path, and
  action.

TTY views use color as a secondary signal: worktrees are green, local branches
yellow, remote refs cyan, stale or blocked states red, behind/diverged states
magenta, and ahead states blue. Non-TTY output and JSON never include ANSI color
codes.

`git ws list --json` keeps the existing fields and adds `tracking` and `action`.
`git ws cleanup --json` keeps the existing fields and adds `reasons`,
`eligible`, `requiresForce`, and `action`.

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

## Development

Contributor build and test commands are documented in
[docs/development.md](docs/development.md). Release steps are documented in
[docs/release.md](docs/release.md).

## License

MIT. See [LICENSE](LICENSE).

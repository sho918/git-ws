# Repository Guidelines

## Project Structure & Module Organization

このリポジトリは Rust 製の Git worktree 補助 CLI `git-ws` です。エントリポイントは `src/main.rs`、再利用可能な処理は `src/lib.rs` から公開し、`src/git.rs`、`src/worktree.rs`、`src/github.rs`、`src/cleanup.rs` などに機能別で分割します。統合テストは `tests/` に置き、共通のテストリポジトリ生成処理は `tests/support/mod.rs` を使います。リリース手順は `docs/release.md`、補助スクリプトは `scripts/`、CI は `.github/workflows/` を確認してください。

## Build, Test, and Development Commands

- `mise install`: `mise.toml` に従い Rust 1.95.0 を用意します。
- `cargo fmt --check`: rustfmt で整形差分がないことを確認します。
- `cargo clippy --all-targets -- -D warnings`: 全ターゲットを警告ゼロで lint します。
- `cargo test --locked`: `Cargo.lock` を尊重して全テストを実行します。
- `cargo build --locked`: デバッグビルドでコンパイルを確認します。
- `cargo run -- list --json`: ローカルで CLI 挙動を確認する例です。
- `cargo install --path .`: `git ws` として動作するバイナリをローカルに入れます。

## Coding Style & Naming Conventions

Rust 2024 edition と標準 rustfmt に従います。インデントや import 整理は手作業で崩さず、提出前に `cargo fmt` を実行してください。モジュール、関数、変数は `snake_case`、型と enum は `PascalCase`、定数は `SCREAMING_SNAKE_CASE` を使います。失敗可能な処理は既存コードに合わせて `anyhow::Result` と文脈付きエラーを使い、Git/CLI 出力のユーザー向け文言は短く具体的にします。

## Testing Guidelines

新しい CLI 挙動は `tests/cli.rs`、純粋なロジックやパーサーは `tests/core.rs` に追加します。GitHub CLI 連携など外部コマンドを伴う挙動は `tests/github_cli.rs` と `tests/support/mod.rs` のヘルパーを優先してください。テスト名は `behavior_under_condition` のように、期待する挙動が読める名前にします。worktree や Git リポジトリを使うテストは `tempfile` ベースで分離し、開発者の実リポジトリに依存しないようにします。

## Commit & Pull Request Guidelines

履歴では `feat: ...`、`fix: ...`、`ci: ...`、`chore: ...` 形式の Conventional Commits を使っています。PR には変更目的、ユーザー-visible な CLI 変更、実行した検証コマンドを記載してください。Issue/PR worktree 機能に関わる変更は関連番号をリンクし、対話ピッカーや出力形式を変える場合は変更前後の例を添えます。リリース変更では `scripts/version.sh X.Y.Z` と `docs/release.md` の手順に従ってください。

## Security & Configuration Tips

`.git-ws.toml` の `[init].on_create` は任意コマンドを実行するため、信頼確認の挙動を弱めないでください。テストや実装でユーザーのグローバル Git 設定、既存 worktree、認証情報に依存しないようにします。

## Agent-Specific Instructions

このリポジトリでの説明、作業報告、レビューコメントは日本語で行います。既存の未コミット変更がある場合は、依頼範囲外のファイルを巻き戻さず、必要な差分だけを編集してください。

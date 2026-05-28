mod support;

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use serde::Serialize;
use support::{
    TestRepo, assert_success, command_output_with_path, configure_git_user, git, last_stdout_line,
    prepend_path,
};

#[test]
fn issue_creates_slugged_worktree_without_network() {
    let repo = TestRepo::with_remote();
    let fake_bin = fake_gh(
        r#"
if [ "$1" = "issue" ]; then
  printf '{"number":42,"title":"Fix worktree cleanup"}'
  exit 0
fi
exit 1
"#,
    );

    let output =
        command_output_with_path(repo.path(), fake_bin.path(), ["issue", "42", "--no-init"]);

    assert_success(&output);
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert!(path.ends_with(".worktrees/issue-42-fix-worktree-cleanup"));
}

#[test]
fn issue_without_id_loads_open_issues_before_interactive_picker() {
    let repo = TestRepo::with_remote();
    let fake_bin = fake_gh(
        r##"
if [ "$1" = "issue" ] && [ "$2" = "list" ]; then
  printf '[{"number":42,"title":"Fix worktree cleanup"}]'
  exit 0
fi
printf 'unexpected gh args: %s %s\n' "$1" "$2" >&2
exit 1
"##,
    );

    let output = command_output_with_path(repo.path(), fake_bin.path(), ["issue", "--no-init"]);

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("interactive picker requires a terminal or query argument")
    );
}

#[test]
fn same_repo_pr_uses_remote_head_as_start_point() {
    let repo = TestRepo::with_remote();
    repo.create_remote_branch("feature/pr-head");
    let fake_bin = fake_gh(
        r#"
if [ "$1" = "pr" ]; then
  printf '{"number":7,"title":"Add PR worktree","headRefName":"feature/pr-head","isCrossRepository":false}'
  exit 0
fi
exit 1
"#,
    );

    let output = command_output_with_path(repo.path(), fake_bin.path(), ["pr", "7", "--no-init"]);

    assert_success(&output);
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert!(path.ends_with(".worktrees/feature-pr-head"));
}

#[test]
fn same_repo_pr_sets_upstream_when_creating_local_branch() {
    let repo = TestRepo::with_remote();
    repo.create_remote_branch("feature/pr-head");
    git(repo.path(), ["branch", "-D", "feature/pr-head"]);
    let fake_bin = fake_gh(
        r#"
if [ "$1" = "pr" ]; then
  printf '{"number":7,"title":"Add PR worktree","headRefName":"feature/pr-head","isCrossRepository":false}'
  exit 0
fi
exit 1
"#,
    );

    let output = command_output_with_path(repo.path(), fake_bin.path(), ["pr", "7", "--no-init"]);

    assert_success(&output);
    let path = last_stdout_line(&output);
    let upstream = git(
        Path::new(&path),
        ["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
    );
    assert_eq!(
        String::from_utf8_lossy(&upstream.stdout).trim(),
        "origin/feature/pr-head"
    );
}

#[test]
fn pr_view_does_not_request_unsupported_base_repository_field() {
    let repo = TestRepo::with_remote();
    repo.create_remote_branch("feature/pr-head");
    let fake_bin = fake_gh(
        r#"
case "$*" in
  *baseRepository*)
    printf 'Unknown JSON field: "baseRepository"' >&2
    exit 2
    ;;
esac

if [ "$1" = "pr" ]; then
  printf '{"number":7,"title":"Add PR worktree","headRefName":"feature/pr-head","isCrossRepository":false}'
  exit 0
fi
exit 1
"#,
    );

    let output = command_output_with_path(repo.path(), fake_bin.path(), ["pr", "7", "--no-init"]);

    assert_success(&output);
}

#[test]
fn same_repo_pr_fetches_updated_head_before_creating_worktree() {
    let repo = TestRepo::with_remote();
    repo.create_remote_branch("feature/pr-head");
    let clone_parent = tempfile::tempdir().expect("clone parent");
    let clone_path = clone_parent.path().join("clone");
    git(
        clone_parent.path(),
        [
            "clone",
            repo.remote_path().to_str().expect("remote path"),
            clone_path.to_str().expect("clone path"),
        ],
    );
    configure_git_user(&clone_path);
    git(&clone_path, ["switch", "feature/pr-head"]);
    fs::write(clone_path.join("new-pr-head.txt"), "new\n").expect("write new PR head");
    git(&clone_path, ["add", "new-pr-head.txt"]);
    git(&clone_path, ["commit", "-m", "new pr head"]);
    git(&clone_path, ["push", "origin", "feature/pr-head"]);
    let remote_head = git(
        repo.remote_path(),
        ["rev-parse", "refs/heads/feature/pr-head"],
    );
    let remote_head = String::from_utf8_lossy(&remote_head.stdout)
        .trim()
        .to_string();

    let fake_bin = fake_gh(
        r#"
if [ "$1" = "pr" ]; then
  printf '{"number":7,"title":"Add PR worktree","headRefName":"feature/pr-head","isCrossRepository":false}'
  exit 0
fi
exit 1
"#,
    );

    let output = command_output_with_path(repo.path(), fake_bin.path(), ["pr", "7", "--no-init"]);

    assert_success(&output);
    let path = last_stdout_line(&output);
    let worktree_head = git(Path::new(&path), ["rev-parse", "HEAD"]);
    assert_eq!(
        String::from_utf8_lossy(&worktree_head.stdout).trim(),
        remote_head
    );
}

#[test]
fn same_repo_pr_fetches_head_branch_when_tag_has_same_name() {
    let repo = TestRepo::with_remote();
    repo.create_remote_branch("feature/pr-head");
    let branch_head = git(
        repo.remote_path(),
        ["rev-parse", "refs/heads/feature/pr-head"],
    );
    let branch_head = String::from_utf8_lossy(&branch_head.stdout)
        .trim()
        .to_string();
    git(
        repo.remote_path(),
        ["update-ref", "refs/tags/feature/pr-head", "refs/heads/main"],
    );
    git(repo.path(), ["branch", "-D", "feature/pr-head"]);
    let fake_bin = fake_gh(
        r#"
if [ "$1" = "pr" ]; then
  printf '{"number":7,"title":"Add PR worktree","headRefName":"feature/pr-head","isCrossRepository":false}'
  exit 0
fi
exit 1
"#,
    );

    let output = command_output_with_path(repo.path(), fake_bin.path(), ["pr", "7", "--no-init"]);

    assert_success(&output);
    let path = last_stdout_line(&output);
    let worktree_head = git(Path::new(&path), ["rev-parse", "HEAD"]);
    assert_eq!(
        String::from_utf8_lossy(&worktree_head.stdout).trim(),
        branch_head
    );
}

#[test]
fn same_repo_pr_fetches_from_only_non_origin_remote() {
    let repo = TestRepo::new();
    let upstream = tempfile::tempdir().expect("upstream remote");
    git(upstream.path(), ["init", "--bare", "-b", "main"]);
    git(
        repo.path(),
        [
            "remote",
            "add",
            "upstream",
            upstream.path().to_str().expect("upstream path"),
        ],
    );
    git(repo.path(), ["push", "-u", "upstream", "main"]);
    git(repo.path(), ["switch", "-c", "feature/pr-head"]);
    fs::write(repo.path().join("upstream-pr-head.txt"), "upstream\n")
        .expect("write upstream PR head");
    git(repo.path(), ["add", "upstream-pr-head.txt"]);
    git(repo.path(), ["commit", "-m", "upstream pr head"]);
    git(repo.path(), ["push", "-u", "upstream", "feature/pr-head"]);
    let remote_head = git(upstream.path(), ["rev-parse", "refs/heads/feature/pr-head"]);
    let remote_head = String::from_utf8_lossy(&remote_head.stdout)
        .trim()
        .to_string();
    git(repo.path(), ["switch", "main"]);
    git(repo.path(), ["branch", "-D", "feature/pr-head"]);
    let fake_bin = fake_gh(
        r#"
if [ "$1" = "pr" ]; then
  printf '{"number":7,"title":"Add PR worktree","headRefName":"feature/pr-head","isCrossRepository":false}'
  exit 0
fi
exit 1
"#,
    );

    let output = command_output_with_path(repo.path(), fake_bin.path(), ["pr", "7", "--no-init"]);

    assert_success(&output);
    let path = last_stdout_line(&output);
    let worktree_head = git(Path::new(&path), ["rev-parse", "HEAD"]);
    assert_eq!(
        String::from_utf8_lossy(&worktree_head.stdout).trim(),
        remote_head
    );
}

#[test]
fn same_repo_pr_fetches_from_remote_matching_current_github_repo() {
    let repo = TestRepo::new();
    let remotes = tempfile::tempdir().expect("remotes");
    let fork = remotes.path().join("fork-owner").join("repo.git");
    let upstream = remotes.path().join("owner").join("repo.git");
    fs::create_dir_all(&fork).expect("create fork remote");
    fs::create_dir_all(&upstream).expect("create upstream remote");
    git(&fork, ["init", "--bare", "-b", "main"]);
    git(&upstream, ["init", "--bare", "-b", "main"]);
    let fork_url = format!("file://{}", fork.display());
    let upstream_url = format!("file://{}", upstream.display());
    git(repo.path(), ["remote", "add", "origin", &fork_url]);
    git(repo.path(), ["remote", "add", "upstream", &upstream_url]);
    git(repo.path(), ["push", "-u", "upstream", "main"]);
    git(repo.path(), ["switch", "-c", "feature/pr-head"]);
    fs::write(repo.path().join("upstream-pr-head.txt"), "upstream\n")
        .expect("write upstream PR head");
    git(repo.path(), ["add", "upstream-pr-head.txt"]);
    git(repo.path(), ["commit", "-m", "upstream pr head"]);
    git(repo.path(), ["push", "-u", "upstream", "feature/pr-head"]);
    let remote_head = git(&upstream, ["rev-parse", "refs/heads/feature/pr-head"]);
    let remote_head = String::from_utf8_lossy(&remote_head.stdout)
        .trim()
        .to_string();
    git(repo.path(), ["switch", "main"]);
    git(repo.path(), ["branch", "-D", "feature/pr-head"]);
    let fake_bin = fake_gh(
        r#"
case "$*" in
  *baseRepository*)
    printf 'Unknown JSON field: "baseRepository"' >&2
    exit 2
    ;;
esac

if [ "$1" = "pr" ]; then
  printf '{"number":7,"title":"Add PR worktree","headRefName":"feature/pr-head","isCrossRepository":false}'
  exit 0
fi
if [ "$1" = "repo" ]; then
  printf '{"nameWithOwner":"owner/repo"}'
  exit 0
fi
exit 1
"#,
    );

    let output = command_output_with_path(repo.path(), fake_bin.path(), ["pr", "7", "--no-init"]);

    assert_success(&output);
    let path = last_stdout_line(&output);
    let worktree_head = git(Path::new(&path), ["rev-parse", "HEAD"]);
    assert_eq!(
        String::from_utf8_lossy(&worktree_head.stdout).trim(),
        remote_head
    );
}

#[test]
fn fork_pr_url_fetches_pull_ref_from_base_repository_remote() {
    let repo = TestRepo::new();
    let remotes = tempfile::tempdir().expect("remotes");
    let fork = remotes.path().join("fork-owner").join("repo.git");
    let upstream = remotes.path().join("owner").join("repo.git");
    fs::create_dir_all(&fork).expect("create fork remote");
    fs::create_dir_all(&upstream).expect("create upstream remote");
    git(&fork, ["init", "--bare", "-b", "main"]);
    git(&upstream, ["init", "--bare", "-b", "main"]);
    let fork_url = format!("file://{}", fork.display());
    let upstream_url = format!("file://{}", upstream.display());
    git(repo.path(), ["remote", "add", "origin", &fork_url]);
    git(repo.path(), ["remote", "add", "upstream", &upstream_url]);
    git(repo.path(), ["push", "-u", "origin", "main"]);
    git(repo.path(), ["push", "upstream", "main"]);
    git(repo.path(), ["switch", "-c", "pull-source-7"]);
    fs::write(repo.path().join("upstream-pull-ref.txt"), "upstream\n")
        .expect("write upstream PR head");
    git(repo.path(), ["add", "upstream-pull-ref.txt"]);
    git(repo.path(), ["commit", "-m", "upstream pull ref"]);
    git(repo.path(), ["push", "upstream", "HEAD:pull-source-7"]);
    git(
        &upstream,
        ["update-ref", "refs/pull/7/head", "refs/heads/pull-source-7"],
    );
    let pull_head = git(&upstream, ["rev-parse", "refs/pull/7/head"]);
    let pull_head = String::from_utf8_lossy(&pull_head.stdout)
        .trim()
        .to_string();
    git(repo.path(), ["switch", "main"]);
    git(repo.path(), ["branch", "-D", "pull-source-7"]);
    let fake_bin = fake_gh(
        r#"
if [ "$1" = "pr" ]; then
  printf '{"number":7,"title":"External Contribution","headRefName":"contributor-branch","isCrossRepository":true}'
  exit 0
fi
if [ "$1" = "repo" ]; then
  printf '{"nameWithOwner":"fork-owner/repo"}'
  exit 0
fi
exit 1
"#,
    );

    let output = command_output_with_path(
        repo.path(),
        fake_bin.path(),
        ["pr", "https://github.com/owner/repo/pull/7", "--no-init"],
    );

    assert_success(&output);
    let path = last_stdout_line(&output);
    let worktree_head = git(Path::new(&path), ["rev-parse", "HEAD"]);
    assert_eq!(
        String::from_utf8_lossy(&worktree_head.stdout).trim(),
        pull_head
    );
}

#[test]
fn same_repo_pr_force_updates_local_head_when_worktree_is_absent() {
    let repo = TestRepo::with_remote();
    repo.create_remote_branch("feature/pr-head");
    let clone_parent = tempfile::tempdir().expect("clone parent");
    let clone_path = clone_parent.path().join("clone");
    git(
        clone_parent.path(),
        [
            "clone",
            repo.remote_path().to_str().expect("remote path"),
            clone_path.to_str().expect("clone path"),
        ],
    );
    configure_git_user(&clone_path);
    git(&clone_path, ["switch", "-c", "rewritten", "origin/main"]);
    fs::write(clone_path.join("rewritten-pr-head.txt"), "rewritten\n")
        .expect("write rewritten PR head");
    git(&clone_path, ["add", "rewritten-pr-head.txt"]);
    git(&clone_path, ["commit", "-m", "rewritten pr head"]);
    git(
        &clone_path,
        ["push", "--force", "origin", "HEAD:feature/pr-head"],
    );
    let remote_head = git(
        repo.remote_path(),
        ["rev-parse", "refs/heads/feature/pr-head"],
    );
    let remote_head = String::from_utf8_lossy(&remote_head.stdout)
        .trim()
        .to_string();

    let fake_bin = fake_gh(
        r#"
if [ "$1" = "pr" ]; then
  printf '{"number":7,"title":"Add PR worktree","headRefName":"feature/pr-head","isCrossRepository":false}'
  exit 0
fi
exit 1
"#,
    );

    let output = command_output_with_path(
        repo.path(),
        fake_bin.path(),
        ["pr", "7", "--no-init", "--force"],
    );

    assert_success(&output);
    let path = last_stdout_line(&output);
    let worktree_head = git(Path::new(&path), ["rev-parse", "HEAD"]);
    assert_eq!(
        String::from_utf8_lossy(&worktree_head.stdout).trim(),
        remote_head
    );
}

#[test]
fn same_repo_pr_refuses_non_fast_forward_local_head_without_force() {
    let repo = TestRepo::with_remote();
    repo.create_remote_branch("feature/pr-head");
    git(repo.path(), ["switch", "feature/pr-head"]);
    fs::write(repo.path().join("local-only-pr-head.txt"), "local\n").expect("write local PR head");
    git(repo.path(), ["add", "local-only-pr-head.txt"]);
    git(repo.path(), ["commit", "-m", "local pr head"]);
    let local_head = git(repo.path(), ["rev-parse", "HEAD"]);
    let local_head = String::from_utf8_lossy(&local_head.stdout)
        .trim()
        .to_string();
    git(repo.path(), ["switch", "main"]);

    let clone_parent = tempfile::tempdir().expect("clone parent");
    let clone_path = clone_parent.path().join("clone");
    git(
        clone_parent.path(),
        [
            "clone",
            repo.remote_path().to_str().expect("remote path"),
            clone_path.to_str().expect("clone path"),
        ],
    );
    configure_git_user(&clone_path);
    git(&clone_path, ["switch", "feature/pr-head"]);
    fs::write(clone_path.join("remote-only-pr-head.txt"), "remote\n")
        .expect("write remote PR head");
    git(&clone_path, ["add", "remote-only-pr-head.txt"]);
    git(&clone_path, ["commit", "-m", "remote pr head"]);
    git(&clone_path, ["push", "origin", "feature/pr-head"]);

    let fake_bin = fake_gh(
        r#"
if [ "$1" = "pr" ]; then
  printf '{"number":7,"title":"Add PR worktree","headRefName":"feature/pr-head","isCrossRepository":false}'
  exit 0
fi
exit 1
"#,
    );

    let output = command_output_with_path(repo.path(), fake_bin.path(), ["pr", "7", "--no-init"]);

    assert!(
        !output.status.success(),
        "non-fast-forward PR fetch should fail without --force"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("--force"),
        "failure should explain the explicit overwrite path"
    );
    let after = git(repo.path(), ["rev-parse", "feature/pr-head"]);
    assert_eq!(
        String::from_utf8_lossy(&after.stdout).trim(),
        local_head,
        "local PR branch should keep its unpushed commit"
    );
}

#[test]
fn pr_rejects_untrusted_init_before_fetching_branch() {
    let repo = TestRepo::with_remote();
    repo.create_remote_branch("feature/pr-needs-trust");
    git(repo.path(), ["branch", "-D", "feature/pr-needs-trust"]);
    fs::write(
        repo.path().join(".git-ws.toml"),
        r#"
[init]
on_create = ["echo init"]
"#,
    )
    .expect("write config");
    let fake_bin = fake_gh(
        r#"
if [ "$1" = "pr" ]; then
  printf '{"number":7,"title":"Needs Trust","headRefName":"feature/pr-needs-trust","isCrossRepository":false}'
  exit 0
fi
exit 1
"#,
    );

    let output = command_output_with_path(repo.path(), fake_bin.path(), ["pr", "7"]);

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("init commands are not trusted"),
        "expected trust failure, got:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let branches = git(repo.path(), ["branch", "--list", "feature/pr-needs-trust"]);
    assert_eq!(String::from_utf8_lossy(&branches.stdout).trim(), "");
    assert!(
        !repo
            .path()
            .join(".worktrees/feature-pr-needs-trust")
            .exists(),
        "worktree should not be created before init trust is accepted"
    );
}

#[test]
fn pr_without_id_loads_open_prs_before_interactive_picker() {
    let repo = TestRepo::with_remote();
    let fake_bin = fake_gh(
        r##"
if [ "$1" = "pr" ] && [ "$2" = "list" ]; then
  printf '[{"number":7,"title":"Add PR worktree","headRefName":"feature/pr-head","isCrossRepository":false}]'
  exit 0
fi
printf 'unexpected gh args: %s %s\n' "$1" "$2" >&2
exit 1
"##,
    );

    let output = command_output_with_path(repo.path(), fake_bin.path(), ["pr", "--no-init"]);

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("interactive picker requires a terminal or query argument")
    );
}

#[test]
fn list_prs_prints_status_without_url_for_local_branches() {
    let repo = TestRepo::with_remote();
    repo.create_remote_branch("feature/pr-head");
    git(
        repo.path(),
        [
            "remote",
            "add",
            "github",
            "https://github.com/owner/repo.git",
        ],
    );
    let cache_home = tempfile::tempdir().expect("cache home");
    let fake_bin = fake_gh(
        r#"
if [ "$1" = "pr" ] && [ "$2" = "list" ]; then
  printf '[{"number":7,"title":"Add PR worktree","headRefName":"feature/pr-head","baseRefName":"main","state":"OPEN","isDraft":false,"updatedAt":"2026-05-10T12:00:00Z","url":"https://github.com/owner/repo/pull/7"}]'
  exit 0
fi
printf 'unexpected gh args: %s\n' "$*" >&2
exit 1
"#,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_git-ws"))
        .current_dir(repo.path())
        .env("PATH", prepend_path(fake_bin.path()))
        .env("XDG_CACHE_HOME", cache_home.path())
        .args(["list", "--type", "local", "--prs"])
        .output()
        .expect("run git-ws");

    assert_success(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("feature/pr-head\torigin/feature/pr-head\tin_sync"),
        "{stdout}"
    );
    assert!(stdout.contains("#7 open"), "{stdout}");
    assert!(
        !stdout.contains("https://github.com/owner/repo/pull/7"),
        "{stdout}"
    );
    assert!(
        !stdout.contains("\u{1b}["),
        "non-TTY output should stay plain"
    );
}

#[test]
fn list_prs_prints_status_without_url_for_remote_only_branches() {
    let repo = TestRepo::with_remote();
    repo.create_remote_branch("feature/remote-only-pr");
    git(repo.path(), ["branch", "-D", "feature/remote-only-pr"]);
    git(
        repo.path(),
        [
            "remote",
            "add",
            "github",
            "https://github.com/owner/repo.git",
        ],
    );
    let cache_home = tempfile::tempdir().expect("cache home");
    let fake_bin = fake_gh(
        r#"
if [ "$1" = "pr" ] && [ "$2" = "list" ]; then
  printf '[{"number":11,"title":"Remote PR","headRefName":"feature/remote-only-pr","baseRefName":"main","state":"OPEN","isDraft":false,"updatedAt":"2026-05-13T12:00:00Z","url":"https://github.com/owner/repo/pull/11"}]'
  exit 0
fi
printf 'unexpected gh args: %s\n' "$*" >&2
exit 1
"#,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_git-ws"))
        .current_dir(repo.path())
        .env("PATH", prepend_path(fake_bin.path()))
        .env("XDG_CACHE_HOME", cache_home.path())
        .args(["list", "--type", "remote", "--prs"])
        .output()
        .expect("run git-ws");

    assert_success(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("feature/remote-only-pr\torigin/feature/remote-only-pr"),
        "{stdout}"
    );
    assert!(stdout.contains("#11 open"), "{stdout}");
    assert!(
        !stdout.contains("https://github.com/owner/repo/pull/11"),
        "{stdout}"
    );
}

#[test]
fn list_prs_queries_each_head_branch_instead_of_only_the_first_repo_page() {
    let repo = TestRepo::with_remote();
    repo.create_remote_branch("feature/pr-head");
    git(
        repo.path(),
        [
            "remote",
            "add",
            "github",
            "https://github.com/owner/repo.git",
        ],
    );
    let cache_home = tempfile::tempdir().expect("cache home");
    let fake_bin = fake_gh(
        r#"
if [ "$1" = "pr" ] && [ "$2" = "list" ] && [ "$6" = "feature/pr-head" ]; then
    printf '[{"number":12,"title":"Head PR","headRefName":"feature/pr-head","baseRefName":"main","state":"OPEN","isDraft":false,"updatedAt":"2026-05-14T12:00:00Z","url":"https://github.com/owner/repo/pull/12"}]'
    exit 0
fi
if [ "$1" = "pr" ] && [ "$2" = "list" ] && [ "$5" = "--head" ]; then
  printf '[]'
  exit 0
fi
printf 'expected branch-scoped gh lookup, got: %s\n' "$*" >&2
exit 1
"#,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_git-ws"))
        .current_dir(repo.path())
        .env("PATH", prepend_path(fake_bin.path()))
        .env("XDG_CACHE_HOME", cache_home.path())
        .args(["list", "--type", "local", "--prs", "--refresh-prs"])
        .output()
        .expect("run git-ws");

    assert_success(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("#12 open"), "{stdout}");
}

#[test]
fn list_prs_reuses_cached_pr_lookup() {
    let repo = TestRepo::with_remote();
    repo.create_remote_branch("feature/pr-cache");
    git(
        repo.path(),
        [
            "remote",
            "add",
            "github",
            "https://github.com/owner/repo.git",
        ],
    );
    let cache_home = tempfile::tempdir().expect("cache home");
    let calls_path = cache_home.path().join("gh-calls");
    let fake_bin = fake_gh(&format!(
        r#"
if [ "$1" = "pr" ] && [ "$2" = "list" ] && [ "$6" = "feature/pr-cache" ]; then
  printf call >> '{}'
  printf '[{{"number":8,"title":"Cache PR","headRefName":"feature/pr-cache","baseRefName":"main","state":"MERGED","isDraft":false,"mergedAt":"2026-05-11T12:00:00Z","updatedAt":"2026-05-11T12:00:00Z","url":"https://github.com/owner/repo/pull/8"}}]'
  exit 0
fi
if [ "$1" = "pr" ] && [ "$2" = "list" ] && [ "$5" = "--head" ]; then
  printf '[]'
  exit 0
fi
printf 'unexpected gh args: %s\n' "$*" >&2
exit 1
"#,
        calls_path.display()
    ));

    for _ in 0..2 {
        let output = Command::new(env!("CARGO_BIN_EXE_git-ws"))
            .current_dir(repo.path())
            .env("PATH", prepend_path(fake_bin.path()))
            .env("XDG_CACHE_HOME", cache_home.path())
            .args(["list", "--type", "local", "--prs"])
            .output()
            .expect("run git-ws");

        assert_success(&output);
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("#8 merged"),
            "stdout:\n{}",
            String::from_utf8_lossy(&output.stdout)
        );
    }

    assert_eq!(
        fs::read_to_string(calls_path).expect("read call count"),
        "call"
    );
}

#[test]
fn cleanup_yes_deletes_unmerged_gone_branch_when_merged_pr_matches_default() {
    let repo = TestRepo::with_remote();
    repo.create_remote_branch("feature/squash-merged");
    git(repo.path(), ["switch", "feature/squash-merged"]);
    fs::write(repo.path().join("local-only.txt"), "local\n").expect("write local change");
    git(repo.path(), ["add", "local-only.txt"]);
    git(repo.path(), ["commit", "-m", "local only"]);
    let local_head =
        String::from_utf8_lossy(&git(repo.path(), ["rev-parse", "feature/squash-merged"]).stdout)
            .trim()
            .to_string();
    git(repo.path(), ["switch", "main"]);
    git(
        repo.remote_path(),
        ["branch", "-D", "feature/squash-merged"],
    );
    git(repo.path(), ["fetch", "--prune", "origin"]);
    git(
        repo.path(),
        [
            "remote",
            "add",
            "github",
            "https://github.com/owner/repo.git",
        ],
    );
    let cache_home = tempfile::tempdir().expect("cache home");
    let fake_bin = fake_gh(&format!(
        r#"
if [ "$1" = "pr" ] && [ "$2" = "list" ]; then
  printf '[{{"number":9,"title":"Squash merged","headRefName":"feature/squash-merged","headRefOid":"{}","headRepository":{{"nameWithOwner":"owner/repo"}},"baseRefName":"main","state":"MERGED","isDraft":false,"mergedAt":"2026-05-12T12:00:00Z","updatedAt":"2026-05-12T12:00:00Z","url":"https://github.com/owner/repo/pull/9"}}]'
  exit 0
fi
printf 'unexpected gh args: %s\n' "$*" >&2
exit 1
"#,
        local_head
    ));

    let output = Command::new(env!("CARGO_BIN_EXE_git-ws"))
        .current_dir(repo.path())
        .env("PATH", prepend_path(fake_bin.path()))
        .env("XDG_CACHE_HOME", cache_home.path())
        .args(["cleanup", "--yes"])
        .output()
        .expect("run git-ws");

    assert_success(&output);
    let branches = git(repo.path(), ["branch", "--list", "feature/squash-merged"]);
    assert_eq!(String::from_utf8_lossy(&branches.stdout).trim(), "");
}

#[test]
fn cleanup_yes_keeps_gone_branch_when_merged_pr_head_does_not_match_local_branch() {
    let repo = TestRepo::with_remote();
    repo.create_remote_branch("feature/reused-pr-head");
    let pr_head =
        String::from_utf8_lossy(&git(repo.path(), ["rev-parse", "feature/reused-pr-head"]).stdout)
            .trim()
            .to_string();
    git(repo.path(), ["switch", "feature/reused-pr-head"]);
    fs::write(repo.path().join("local-only.txt"), "local\n").expect("write local change");
    git(repo.path(), ["add", "local-only.txt"]);
    git(repo.path(), ["commit", "-m", "local only"]);
    git(repo.path(), ["switch", "main"]);
    git(
        repo.remote_path(),
        ["branch", "-D", "feature/reused-pr-head"],
    );
    git(repo.path(), ["fetch", "--prune", "origin"]);
    git(
        repo.path(),
        [
            "remote",
            "add",
            "github",
            "https://github.com/owner/repo.git",
        ],
    );
    let cache_home = tempfile::tempdir().expect("cache home");
    let fake_bin = fake_gh(&format!(
        r#"
if [ "$1" = "pr" ] && [ "$2" = "list" ]; then
  printf '[{{"number":13,"title":"Reused branch","headRefName":"feature/reused-pr-head","headRefOid":"{}","headRepository":{{"nameWithOwner":"owner/repo"}},"baseRefName":"main","state":"MERGED","isDraft":false,"mergedAt":"2026-05-15T12:00:00Z","updatedAt":"2026-05-15T12:00:00Z","url":"https://github.com/owner/repo/pull/13"}}]'
  exit 0
fi
printf 'unexpected gh args: %s\n' "$*" >&2
exit 1
"#,
        pr_head
    ));

    let output = Command::new(env!("CARGO_BIN_EXE_git-ws"))
        .current_dir(repo.path())
        .env("PATH", prepend_path(fake_bin.path()))
        .env("XDG_CACHE_HOME", cache_home.path())
        .args(["cleanup", "--yes"])
        .output()
        .expect("run git-ws");

    assert_success(&output);
    let branches = git(repo.path(), ["branch", "--list", "feature/reused-pr-head"]);
    assert!(
        String::from_utf8_lossy(&branches.stdout).contains("feature/reused-pr-head"),
        "branch should be preserved when the merged PR head is not the local branch head"
    );
}

#[test]
fn cleanup_yes_keeps_gone_branch_when_merged_pr_head_repository_does_not_match() {
    let repo = TestRepo::with_remote();
    repo.create_remote_branch("feature/fork-name-collision");
    git(repo.path(), ["switch", "feature/fork-name-collision"]);
    fs::write(repo.path().join("local-only.txt"), "local\n").expect("write local change");
    git(repo.path(), ["add", "local-only.txt"]);
    git(repo.path(), ["commit", "-m", "local only"]);
    let local_head = String::from_utf8_lossy(
        &git(repo.path(), ["rev-parse", "feature/fork-name-collision"]).stdout,
    )
    .trim()
    .to_string();
    git(repo.path(), ["switch", "main"]);
    git(
        repo.remote_path(),
        ["branch", "-D", "feature/fork-name-collision"],
    );
    git(repo.path(), ["fetch", "--prune", "origin"]);
    git(
        repo.path(),
        [
            "remote",
            "add",
            "github",
            "https://github.com/owner/repo.git",
        ],
    );
    let cache_home = tempfile::tempdir().expect("cache home");
    let fake_bin = fake_gh(&format!(
        r#"
if [ "$1" = "pr" ] && [ "$2" = "list" ]; then
  printf '[{{"number":14,"title":"Fork collision","headRefName":"feature/fork-name-collision","headRefOid":"{}","headRepository":{{"nameWithOwner":"contributor/repo"}},"baseRefName":"main","state":"MERGED","isDraft":false,"mergedAt":"2026-05-16T12:00:00Z","updatedAt":"2026-05-16T12:00:00Z","url":"https://github.com/owner/repo/pull/14"}}]'
  exit 0
fi
printf 'unexpected gh args: %s\n' "$*" >&2
exit 1
"#,
        local_head
    ));

    let output = Command::new(env!("CARGO_BIN_EXE_git-ws"))
        .current_dir(repo.path())
        .env("PATH", prepend_path(fake_bin.path()))
        .env("XDG_CACHE_HOME", cache_home.path())
        .args(["cleanup", "--yes"])
        .output()
        .expect("run git-ws");

    assert_success(&output);
    let branches = git(
        repo.path(),
        ["branch", "--list", "feature/fork-name-collision"],
    );
    assert!(
        String::from_utf8_lossy(&branches.stdout).contains("feature/fork-name-collision"),
        "branch should be preserved when the merged PR head repository differs"
    );
}

#[test]
fn cleanup_yes_keeps_gone_branch_when_merged_pr_targets_other_base() {
    let repo = TestRepo::with_remote();
    repo.create_remote_branch("feature/other-base");
    git(repo.path(), ["switch", "feature/other-base"]);
    fs::write(repo.path().join("local-only.txt"), "local\n").expect("write local change");
    git(repo.path(), ["add", "local-only.txt"]);
    git(repo.path(), ["commit", "-m", "local only"]);
    git(repo.path(), ["switch", "main"]);
    git(repo.remote_path(), ["branch", "-D", "feature/other-base"]);
    git(repo.path(), ["fetch", "--prune", "origin"]);
    git(
        repo.path(),
        [
            "remote",
            "add",
            "github",
            "https://github.com/owner/repo.git",
        ],
    );
    let cache_home = tempfile::tempdir().expect("cache home");
    let fake_bin = fake_gh(
        r#"
if [ "$1" = "pr" ] && [ "$2" = "list" ]; then
  printf '[{"number":10,"title":"Other base","headRefName":"feature/other-base","baseRefName":"develop","state":"MERGED","isDraft":false,"mergedAt":"2026-05-12T12:00:00Z","updatedAt":"2026-05-12T12:00:00Z","url":"https://github.com/owner/repo/pull/10"}]'
  exit 0
fi
printf 'unexpected gh args: %s\n' "$*" >&2
exit 1
"#,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_git-ws"))
        .current_dir(repo.path())
        .env("PATH", prepend_path(fake_bin.path()))
        .env("XDG_CACHE_HOME", cache_home.path())
        .args(["cleanup", "--yes"])
        .output()
        .expect("run git-ws");

    assert_success(&output);
    let branches = git(repo.path(), ["branch", "--list", "feature/other-base"]);
    assert!(
        String::from_utf8_lossy(&branches.stdout).contains("feature/other-base"),
        "branch merged only to another base should be preserved"
    );
}

#[test]
fn fork_pr_fetches_pull_ref_before_creating_worktree() {
    let repo = TestRepo::with_remote();
    repo.create_pull_ref(9);
    let fake_bin = fake_gh(
        r#"
if [ "$1" = "pr" ]; then
  printf '{"number":9,"title":"External Contribution","headRefName":"fork-branch","isCrossRepository":true}'
  exit 0
fi
exit 1
"#,
    );

    let output = command_output_with_path(repo.path(), fake_bin.path(), ["pr", "9", "--no-init"]);

    assert_success(&output);
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert!(path.ends_with(".worktrees/pr-9-external-contribution"));
}

#[test]
fn fork_pr_does_not_run_trusted_init_commands() {
    let repo = TestRepo::with_remote();
    repo.create_pull_ref(13);
    let marker = repo.path().join("fork-init-ran");
    let init_command = format!("printf init > '{}'", marker.display());
    fs::write(
        repo.path().join(".git-ws.toml"),
        format!(
            r#"
[init]
on_create = ["{init_command}"]
"#
        ),
    )
    .expect("write config");
    let config_home = tempfile::tempdir().expect("config home");
    write_trusted_init_store(repo.path(), config_home.path(), &[init_command]);
    let fake_bin = fake_gh(
        r#"
if [ "$1" = "pr" ]; then
  printf '{"number":13,"title":"External Init","headRefName":"fork-branch","isCrossRepository":true}'
  exit 0
fi
exit 1
"#,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_git-ws"))
        .current_dir(repo.path())
        .env("PATH", prepend_path(fake_bin.path()))
        .env("XDG_CONFIG_HOME", config_home.path())
        .args(["pr", "13"])
        .output()
        .expect("run git-ws");

    assert_success(&output);
    assert!(
        !marker.exists(),
        "fork PR should not run init commands automatically"
    );
}

#[test]
fn fork_pr_reuses_existing_worktree_before_fetching_pull_ref() {
    let repo = TestRepo::with_remote();
    repo.create_pull_ref(11);
    let fake_bin = fake_gh(
        r#"
if [ "$1" = "pr" ]; then
  printf '{"number":11,"title":"External Reuse","headRefName":"fork-branch","isCrossRepository":true}'
  exit 0
fi
exit 1
"#,
    );

    let first = command_output_with_path(repo.path(), fake_bin.path(), ["pr", "11", "--no-init"]);
    assert_success(&first);
    let first_path = String::from_utf8_lossy(&first.stdout).trim().to_string();

    let second = command_output_with_path(repo.path(), fake_bin.path(), ["pr", "11", "--no-init"]);

    assert_success(&second);
    assert_eq!(String::from_utf8_lossy(&second.stdout).trim(), first_path);
}

#[test]
fn gh_failure_is_reported() {
    let repo = TestRepo::with_remote();
    let fake_bin = fake_gh(
        r#"
printf 'missing auth' >&2
exit 2
"#,
    );

    let output =
        command_output_with_path(repo.path(), fake_bin.path(), ["issue", "42", "--no-init"]);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("missing auth"));
}

fn fake_gh(script: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("gh");
    fs::write(&path, format!("#!/bin/sh\n{script}\n")).expect("write fake gh");
    let mut permissions = fs::metadata(&path).expect("metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).expect("chmod fake gh");
    dir
}

fn write_trusted_init_store(repo_path: &Path, config_home: &Path, init_commands: &[String]) {
    let trust_dir = config_home.join("git-ws");
    fs::create_dir_all(&trust_dir).expect("create trust dir");
    let trust_path = trust_dir.join("trust.toml");
    let repo_path = repo_path.canonicalize().expect("canonicalize repo path");
    let mut repos = BTreeMap::new();
    repos.insert(
        repo_path.display().to_string(),
        trusted_init_value(init_commands),
    );
    let raw = toml::to_string_pretty(&TestTrustStore { repos }).expect("serialize trust store");
    fs::write(trust_path, raw).expect("write trust store");
}

#[derive(Serialize)]
struct TestTrustStore {
    repos: BTreeMap<String, String>,
}

fn trusted_init_value(init_commands: &[String]) -> String {
    let mut value = String::from("git-ws-init-trust-v1\n");
    value.push_str("worktree_base_dir:none\n");
    value.push_str(&format!("init_commands:{}\n", init_commands.len()));
    for command in init_commands {
        push_trust_field(&mut value, "init_command", command);
    }
    value
}

fn push_trust_field(output: &mut String, label: &str, value: &str) {
    output.push_str(label);
    output.push(':');
    output.push_str(&value.len().to_string());
    output.push(':');
    output.push_str(value);
    output.push('\n');
}

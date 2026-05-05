mod support;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use support::{
    TestRepo, assert_success, command_output_with_path, configure_git_user, git, last_stdout_line,
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

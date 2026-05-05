mod support;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use support::{TestRepo, prepend_path};

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

    let output = Command::new(env!("CARGO_BIN_EXE_git-ws"))
        .current_dir(repo.path())
        .env("PATH", prepend_path(fake_bin.path()))
        .args(["issue", "42", "--no-init"])
        .output()
        .expect("run git-ws");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert!(path.ends_with(".worktrees/issue-42-fix-worktree-cleanup"));
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

    let output = Command::new(env!("CARGO_BIN_EXE_git-ws"))
        .current_dir(repo.path())
        .env("PATH", prepend_path(fake_bin.path()))
        .args(["pr", "7", "--no-init"])
        .output()
        .expect("run git-ws");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert!(path.ends_with(".worktrees/feature-pr-head"));
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

    let output = Command::new(env!("CARGO_BIN_EXE_git-ws"))
        .current_dir(repo.path())
        .env("PATH", prepend_path(fake_bin.path()))
        .args(["pr", "9", "--no-init"])
        .output()
        .expect("run git-ws");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert!(path.ends_with(".worktrees/pr-9-external-contribution"));
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

    let output = Command::new(env!("CARGO_BIN_EXE_git-ws"))
        .current_dir(repo.path())
        .env("PATH", prepend_path(fake_bin.path()))
        .args(["issue", "42", "--no-init"])
        .output()
        .expect("run git-ws");

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

#[allow(clippy::needless_pass_by_value)]
fn _assert_path(_: &Path) {}

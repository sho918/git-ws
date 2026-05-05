mod support;

use std::process::Command;

use support::{TestRepo, command_output, git, last_stdout_line};

#[test]
fn help_prints_usage() {
    let output = Command::new(env!("CARGO_BIN_EXE_git-ws"))
        .arg("--help")
        .output()
        .expect("run git-ws");

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("git ws"));
}

#[test]
fn doctor_reports_git() {
    let repo = TestRepo::new();

    let output = command_output(repo.path(), ["doctor"]);

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("git-ws: git git version"));
}

#[test]
fn init_shell_fish_contains_cd_wrapper() {
    let output = Command::new(env!("CARGO_BIN_EXE_git-ws"))
        .args(["init-shell", "fish"])
        .output()
        .expect("run git-ws");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("__git_ws_run_and_cd"));
    assert!(stdout.contains("cd \"$last_line\""));
}

#[test]
fn list_outputs_remote_branch_json() {
    let repo = TestRepo::with_remote();
    repo.create_remote_branch("feature/list-json");

    let output = command_output(repo.path(), ["list", "--json"]);

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("feature/list-json"));
    assert!(stdout.contains("origin/feature/list-json"));
}

#[test]
fn new_creates_worktree_from_requested_ref() {
    let repo = TestRepo::with_remote();

    let output = command_output(
        repo.path(),
        ["new", "feature/new", "--from", "HEAD", "--no-init"],
    );

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let path = last_stdout_line(&output);
    assert!(path.ends_with(".worktrees/feature-new"));
    assert!(std::path::Path::new(&path).join(".git").exists());
}

#[test]
fn open_query_prints_existing_worktree_path_as_last_line() {
    let repo = TestRepo::with_remote();
    let create = command_output(
        repo.path(),
        ["new", "feature/open-query", "--from", "HEAD", "--no-init"],
    );
    assert!(
        create.status.success(),
        "{}",
        String::from_utf8_lossy(&create.stderr)
    );
    let created_path = last_stdout_line(&create);

    let output = command_output(repo.path(), ["open", "open-query"]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let last_line = last_stdout_line(&output);
    assert_eq!(last_line, created_path);
}

#[test]
fn cleanup_dry_run_handles_unborn_repository() {
    let repo = TestRepo::unborn();

    let output = command_output(repo.path(), ["cleanup", "--dry-run"]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("nothing to clean"));
}

#[test]
fn cleanup_json_reports_gone_branch() {
    let repo = TestRepo::with_remote();
    repo.create_remote_branch("feature/gone");
    git(repo.path(), ["switch", "feature/gone"]);
    git(repo.path(), ["switch", "main"]);
    git(repo.remote_path(), ["branch", "-D", "feature/gone"]);
    git(repo.path(), ["fetch", "--prune", "origin"]);

    let output = command_output(repo.path(), ["cleanup", "--json"]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("feature/gone"));
    assert!(stdout.contains("SafeDelete"));
}

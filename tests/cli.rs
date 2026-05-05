mod support;

use std::process::Command;

use support::{TestRepo, assert_success, command_output, git, last_stdout_line};

#[test]
fn help_prints_usage() {
    let output = Command::new(env!("CARGO_BIN_EXE_git-ws"))
        .arg("--help")
        .output()
        .expect("run git-ws");

    assert_success(&output);
    assert!(String::from_utf8_lossy(&output.stdout).contains("git ws"));
}

#[test]
fn doctor_reports_git() {
    let repo = TestRepo::new();

    let output = command_output(repo.path(), ["doctor"]);

    assert_success(&output);
    assert!(String::from_utf8_lossy(&output.stdout).contains("git-ws: git "));
}

#[test]
fn init_shell_fish_contains_cd_wrapper() {
    let output = Command::new(env!("CARGO_BIN_EXE_git-ws"))
        .args(["init-shell", "fish"])
        .output()
        .expect("run git-ws");

    assert_success(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("__git_ws_run_and_cd"));
    assert!(stdout.contains("GIT_WS_CD_FILE"));
}

#[test]
fn list_outputs_remote_branch_json() {
    let repo = TestRepo::with_remote();
    repo.create_remote_branch("feature/list-json");

    let output = command_output(repo.path(), ["list", "--json"]);

    assert_success(&output);
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

    assert_success(&output);
    let path = last_stdout_line(&output);
    assert!(path.ends_with(".worktrees/feature-new"));
    assert!(std::path::Path::new(&path).join(".git").exists());
}

#[test]
fn new_without_from_does_not_leak_rev_parse_sha() {
    let repo = TestRepo::with_remote();

    let output = command_output(repo.path(), ["new", "feature/default", "--no-init"]);

    assert_success(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout
            .lines()
            .any(|line| line.len() == 40 && line.chars().all(|ch| ch.is_ascii_hexdigit())),
        "rev-parse SHA should not leak to stdout:\n{stdout}"
    );
    assert!(last_stdout_line(&output).ends_with(".worktrees/feature-default"));
}

#[test]
fn new_writes_cd_target_to_side_channel_without_stdout_path() {
    let repo = TestRepo::with_remote();
    let cd_file = tempfile::NamedTempFile::new().expect("cd file");

    let output = Command::new(env!("CARGO_BIN_EXE_git-ws"))
        .current_dir(repo.path())
        .env("GIT_WS_CD_FILE", cd_file.path())
        .args(["new", "feature/side-channel", "--from", "HEAD", "--no-init"])
        .output()
        .expect("run git-ws");

    assert_success(&output);
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains(".worktrees/feature-side-channel"),
        "cd target should be written only to the side channel"
    );
    let target = std::fs::read_to_string(cd_file.path()).expect("read cd file");
    assert!(
        target
            .trim_end()
            .ends_with(".worktrees/feature-side-channel")
    );
}

#[test]
fn new_rejects_untrusted_init_before_creating_worktree() {
    let repo = TestRepo::new();
    std::fs::write(
        repo.path().join(".git-ws.toml"),
        r#"
[init]
on_create = ["echo init"]
"#,
    )
    .expect("write config");

    let output = command_output(repo.path(), ["new", "feature/needs-trust"]);

    assert!(!output.status.success());
    assert!(
        !repo.path().join(".worktrees/feature-needs-trust").exists(),
        "worktree should not be created before init trust is accepted"
    );
    let branches = git(repo.path(), ["branch", "--list", "feature/needs-trust"]);
    assert_eq!(String::from_utf8_lossy(&branches.stdout).trim(), "");
}

#[test]
fn open_query_prints_existing_worktree_path_as_last_line() {
    let repo = TestRepo::with_remote();
    let create = command_output(
        repo.path(),
        ["new", "feature/open-query", "--from", "HEAD", "--no-init"],
    );
    assert_success(&create);
    let created_path = last_stdout_line(&create);

    let output = command_output(repo.path(), ["open", "open-query"]);

    assert_success(&output);
    let last_line = last_stdout_line(&output);
    assert_eq!(last_line, created_path);
}

#[test]
fn open_remote_query_tracks_remote_branch() {
    let repo = TestRepo::with_remote();
    repo.create_remote_branch("feature/remote-query");
    git(repo.path(), ["branch", "-D", "feature/remote-query"]);

    let output = command_output(
        repo.path(),
        ["open", "--type", "remote", "feature/remote-query"],
    );

    assert_success(&output);
    let current = git(repo.path(), ["branch", "--show-current"]);
    assert_eq!(
        String::from_utf8_lossy(&current.stdout).trim(),
        "feature/remote-query"
    );
    let upstream = git(
        repo.path(),
        ["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
    );
    assert_eq!(
        String::from_utf8_lossy(&upstream.stdout).trim(),
        "origin/feature/remote-query"
    );
}

#[test]
fn cleanup_dry_run_handles_unborn_repository() {
    let repo = TestRepo::unborn();

    let output = command_output(repo.path(), ["cleanup", "--dry-run"]);

    assert_success(&output);
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

    assert_success(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("feature/gone"));
    assert!(stdout.contains("SafeDelete"));
}

#[test]
fn cleanup_yes_does_not_force_delete_unmerged_gone_branch() {
    let repo = TestRepo::with_remote();
    repo.create_remote_branch("feature/unmerged-gone");
    git(repo.path(), ["switch", "feature/unmerged-gone"]);
    std::fs::write(repo.path().join("local-only.txt"), "local\n").expect("write local change");
    git(repo.path(), ["add", "local-only.txt"]);
    git(repo.path(), ["commit", "-m", "local only"]);
    git(repo.path(), ["switch", "main"]);
    git(
        repo.remote_path(),
        ["branch", "-D", "feature/unmerged-gone"],
    );
    git(repo.path(), ["fetch", "--prune", "origin"]);

    let output = command_output(repo.path(), ["cleanup", "--yes"]);

    assert!(
        !output.status.success(),
        "cleanup should not force-delete unmerged commits without --force"
    );
    let branches = git(repo.path(), ["branch", "--list", "feature/unmerged-gone"]);
    assert!(String::from_utf8_lossy(&branches.stdout).contains("feature/unmerged-gone"));
}

#[test]
fn cleanup_force_yes_skips_current_worktree() {
    let repo = TestRepo::with_remote();
    repo.create_remote_branch("feature/force-clean");
    git(repo.remote_path(), ["branch", "-D", "feature/force-clean"]);
    git(repo.path(), ["fetch", "--prune", "origin"]);

    let output = command_output(repo.path(), ["cleanup", "--force", "--yes"]);

    assert_success(&output);
    assert!(repo.path().join(".git").exists());
    let main_branches = git(repo.path(), ["branch", "--list", "main"]);
    assert!(String::from_utf8_lossy(&main_branches.stdout).contains("main"));
    let stale_branches = git(repo.path(), ["branch", "--list", "feature/force-clean"]);
    assert_eq!(
        String::from_utf8_lossy(&stale_branches.stdout).trim(),
        "",
        "feature/force-clean should have been deleted"
    );
}

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
fn new_falls_back_when_slug_path_collides_with_another_branch() {
    let repo = TestRepo::with_remote();

    let first = command_output(
        repo.path(),
        ["new", "feature/foo-bar", "--from", "HEAD", "--no-init"],
    );
    assert_success(&first);
    let first_path = last_stdout_line(&first);

    let second = command_output(
        repo.path(),
        ["new", "feature/foo/bar", "--from", "HEAD", "--no-init"],
    );

    assert_success(&second);
    let second_path = last_stdout_line(&second);
    let second_segment = std::path::Path::new(&second_path)
        .file_name()
        .and_then(|name| name.to_str())
        .expect("worktree path segment");
    assert_ne!(second_path, first_path);
    assert!(second_segment.starts_with("feature-foo-bar-"));
    assert!(std::path::Path::new(&second_path).join(".git").exists());
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
fn new_from_linked_worktree_uses_primary_worktree_base_dir() {
    let repo = TestRepo::with_remote();
    let parent = command_output(
        repo.path(),
        ["new", "feature/parent", "--from", "HEAD", "--no-init"],
    );
    assert_success(&parent);
    let parent_path = last_stdout_line(&parent);

    let child = command_output(
        std::path::Path::new(&parent_path),
        ["new", "feature/child", "--from", "HEAD", "--no-init"],
    );

    assert_success(&child);
    let child_path = last_stdout_line(&child);
    let child_canonical =
        std::fs::canonicalize(&child_path).expect("canonicalize child worktree path");
    let repo_canonical = std::fs::canonicalize(repo.path()).expect("canonicalize repo path");
    let parent_canonical =
        std::fs::canonicalize(&parent_path).expect("canonicalize parent worktree path");
    assert!(child_path.ends_with(".worktrees/feature-child"));
    assert!(
        child_canonical.starts_with(&repo_canonical),
        "child worktree should be anchored under primary repo: {child_path}"
    );
    assert!(
        !child_canonical.starts_with(&parent_canonical),
        "child worktree should not be nested under parent worktree: {child_path}"
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
fn main_switches_to_remote_only_default_branch() {
    let repo = TestRepo::with_remote();
    repo.create_remote_branch("feature/current");
    git(repo.path(), ["switch", "feature/current"]);
    git(repo.path(), ["branch", "-D", "main"]);

    let output = command_output(repo.path(), ["main"]);

    assert_success(&output);
    let current = git(repo.path(), ["branch", "--show-current"]);
    assert_eq!(String::from_utf8_lossy(&current.stdout).trim(), "main");
    let upstream = git(
        repo.path(),
        ["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
    );
    assert_eq!(
        String::from_utf8_lossy(&upstream.stdout).trim(),
        "origin/main"
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
fn cleanup_yes_keeps_gone_default_branch() {
    let repo = TestRepo::with_remote();
    repo.create_remote_branch("feature/current");
    git(repo.path(), ["switch", "feature/current"]);
    git(
        repo.remote_path(),
        ["symbolic-ref", "HEAD", "refs/heads/feature/current"],
    );
    git(repo.remote_path(), ["branch", "-D", "main"]);
    git(repo.path(), ["fetch", "--prune", "origin"]);

    let output = command_output(repo.path(), ["cleanup", "--yes"]);

    assert_success(&output);
    let main_branches = git(repo.path(), ["branch", "--list", "main"]);
    assert!(String::from_utf8_lossy(&main_branches.stdout).contains("main"));
}

#[test]
fn cleanup_uses_origin_head_default_instead_of_current_branch() {
    let repo = TestRepo::with_remote();
    repo.create_remote_branch("develop");
    git(
        repo.remote_path(),
        ["symbolic-ref", "HEAD", "refs/heads/develop"],
    );
    git(repo.remote_path(), ["branch", "-D", "main"]);
    git(repo.path(), ["fetch", "--prune", "origin"]);
    git(
        repo.path(),
        [
            "switch",
            "-c",
            "feature/not-default-merged",
            "origin/develop",
        ],
    );
    std::fs::write(repo.path().join("not-default.txt"), "not default\n")
        .expect("write branch file");
    git(repo.path(), ["add", "not-default.txt"]);
    git(repo.path(), ["commit", "-m", "not merged to default"]);
    git(
        repo.path(),
        ["push", "-u", "origin", "feature/not-default-merged"],
    );
    git(
        repo.path(),
        ["switch", "-c", "feature/current", "origin/develop"],
    );
    git(
        repo.path(),
        [
            "merge",
            "--no-ff",
            "feature/not-default-merged",
            "-m",
            "merge into current only",
        ],
    );
    git(repo.path(), ["branch", "-D", "main"]);

    let output = command_output(repo.path(), ["cleanup", "--yes"]);

    assert_success(&output);
    let branches = git(
        repo.path(),
        ["branch", "--list", "feature/not-default-merged"],
    );
    assert!(
        String::from_utf8_lossy(&branches.stdout).contains("feature/not-default-merged"),
        "branch not merged to origin/HEAD default should not be deleted"
    );
}

#[test]
fn cleanup_yes_deletes_default_merged_worktree_from_non_default_head() {
    let repo = TestRepo::with_remote();
    let worktree_parent = repo.path().join(".worktrees");
    std::fs::create_dir_all(&worktree_parent).expect("create worktree parent");
    let worktree = worktree_parent.join("default-merged");
    git(
        repo.path(),
        [
            "worktree",
            "add",
            "-b",
            "feature/default-merged",
            worktree.to_str().expect("worktree path"),
            "main",
        ],
    );
    std::fs::write(worktree.join("default-merged.txt"), "default\n")
        .expect("write default merged file");
    git(&worktree, ["add", "default-merged.txt"]);
    git(&worktree, ["commit", "-m", "default merged"]);
    git(
        repo.path(),
        [
            "merge",
            "--no-ff",
            "feature/default-merged",
            "-m",
            "merge into default",
        ],
    );
    git(repo.path(), ["push", "origin", "main"]);
    git(repo.path(), ["fetch", "origin"]);
    git(repo.path(), ["switch", "-c", "feature/current", "HEAD~1"]);

    let output = command_output(repo.path(), ["cleanup", "--yes"]);

    assert_success(&output);
    assert!(
        !worktree.exists(),
        "default-merged worktree should have been removed"
    );
    let branches = git(repo.path(), ["branch", "--list", "feature/default-merged"]);
    assert_eq!(
        String::from_utf8_lossy(&branches.stdout).trim(),
        "",
        "default-merged branch should have been deleted"
    );
}

#[test]
fn cleanup_yes_refuses_unknown_default_branch_instead_of_deleting_trunk() {
    let repo = TestRepo::with_remote();
    git(repo.path(), ["branch", "-m", "main", "trunk"]);
    git(repo.remote_path(), ["branch", "-m", "main", "trunk"]);
    git(
        repo.remote_path(),
        ["symbolic-ref", "HEAD", "refs/heads/trunk"],
    );
    git(repo.path(), ["fetch", "--prune", "origin"]);
    git(
        repo.path(),
        ["symbolic-ref", "--delete", "refs/remotes/origin/HEAD"],
    );
    git(
        repo.path(),
        ["update-ref", "-d", "refs/remotes/origin/main"],
    );
    git(
        repo.path(),
        ["update-ref", "-d", "refs/remotes/origin/master"],
    );
    git(repo.path(), ["switch", "-c", "feature/current"]);

    let output = command_output(repo.path(), ["cleanup", "--yes"]);

    assert!(
        !output.status.success(),
        "cleanup unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("default branch could not be determined"),
        "expected default branch failure, got:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let branches = git(repo.path(), ["branch", "--list", "trunk"]);
    assert!(
        String::from_utf8_lossy(&branches.stdout).contains("trunk"),
        "cleanup should not delete an unrecognized default branch"
    );
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
fn cleanup_yes_keeps_unmerged_gone_worktree() {
    let repo = TestRepo::with_remote();
    repo.create_remote_branch("feature/unmerged-gone-worktree");
    let create = command_output(
        repo.path(),
        ["new", "feature/unmerged-gone-worktree", "--no-init"],
    );
    assert_success(&create);
    let path = last_stdout_line(&create);
    let worktree = std::path::Path::new(&path);
    std::fs::write(worktree.join("local-only.txt"), "local\n").expect("write local change");
    git(worktree, ["add", "local-only.txt"]);
    git(worktree, ["commit", "-m", "local only"]);
    git(
        repo.remote_path(),
        ["branch", "-D", "feature/unmerged-gone-worktree"],
    );
    git(repo.path(), ["fetch", "--prune", "origin"]);

    let output = command_output(repo.path(), ["cleanup", "--yes"]);

    assert!(
        !output.status.success(),
        "cleanup should refuse unmerged gone worktree without --force"
    );
    assert!(worktree.exists(), "worktree should be preserved on failure");
    let branches = git(
        repo.path(),
        ["branch", "--list", "feature/unmerged-gone-worktree"],
    );
    assert!(String::from_utf8_lossy(&branches.stdout).contains("feature/unmerged-gone-worktree"));
}

#[test]
fn cleanup_force_yes_removes_dirty_gone_worktree() {
    let repo = TestRepo::with_remote();
    repo.create_remote_branch("feature/dirty-gone");
    let create = command_output(repo.path(), ["new", "feature/dirty-gone", "--no-init"]);
    assert_success(&create);
    let path = last_stdout_line(&create);
    std::fs::write(std::path::Path::new(&path).join("dirty.txt"), "dirty\n")
        .expect("write dirty worktree file");
    git(repo.remote_path(), ["branch", "-D", "feature/dirty-gone"]);
    git(repo.path(), ["fetch", "--prune", "origin"]);

    let output = command_output(repo.path(), ["cleanup", "--force", "--yes"]);

    assert_success(&output);
    assert!(
        !std::path::Path::new(&path).exists(),
        "dirty gone worktree should be removed with --force"
    );
    let stale_branches = git(repo.path(), ["branch", "--list", "feature/dirty-gone"]);
    assert_eq!(
        String::from_utf8_lossy(&stale_branches.stdout).trim(),
        "",
        "feature/dirty-gone should have been deleted"
    );
}

#[test]
fn cleanup_force_yes_keeps_active_unmerged_branches_and_worktrees() {
    let repo = TestRepo::with_remote();
    repo.create_remote_branch("feature/active-clean");
    repo.create_remote_branch("feature/active-worktree");
    let create = command_output(repo.path(), ["new", "feature/active-worktree", "--no-init"]);
    assert_success(&create);
    let path = last_stdout_line(&create);
    std::fs::write(std::path::Path::new(&path).join("dirty.txt"), "dirty\n")
        .expect("write dirty worktree file");

    let output = command_output(repo.path(), ["cleanup", "--force", "--yes"]);

    assert_success(&output);
    let clean_branches = git(repo.path(), ["branch", "--list", "feature/active-clean"]);
    assert!(
        String::from_utf8_lossy(&clean_branches.stdout).contains("feature/active-clean"),
        "active clean branch with an upstream should not be force deleted"
    );
    let worktree_branches = git(repo.path(), ["branch", "--list", "feature/active-worktree"]);
    assert!(
        String::from_utf8_lossy(&worktree_branches.stdout).contains("feature/active-worktree"),
        "active worktree branch with an upstream should not be force deleted"
    );
    assert!(
        std::path::Path::new(&path).exists(),
        "active worktree should be preserved"
    );
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

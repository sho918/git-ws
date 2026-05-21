mod support;

use std::collections::{BTreeMap, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::process::Command;

use serde::Serialize;
use serde_json::{Value, json};
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
fn list_json_reports_tracking_and_action() {
    let repo = TestRepo::with_remote();
    repo.create_remote_branch("feature/tracking-json");
    git(repo.path(), ["switch", "feature/tracking-json"]);
    std::fs::write(repo.path().join("local-ahead.txt"), "ahead\n").expect("write ahead file");
    git(repo.path(), ["add", "local-ahead.txt"]);
    git(repo.path(), ["commit", "-m", "local ahead"]);
    git(repo.path(), ["switch", "main"]);

    let output = command_output(repo.path(), ["list", "--json"]);

    assert_success(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"tracking\""), "{stdout}");
    assert!(stdout.contains("\"state\": \"ahead\""), "{stdout}");
    assert!(stdout.contains("\"summary\": \"ahead 1\""), "{stdout}");
    assert!(
        stdout.contains("\"action\": \"git switch feature/tracking-json\""),
        "{stdout}"
    );
}

#[test]
fn list_type_worktree_omits_prunable_missing_worktree() {
    let repo = TestRepo::with_remote();
    let create = command_output(
        repo.path(),
        [
            "new",
            "feature/prunable-worktree",
            "--from",
            "HEAD",
            "--no-init",
        ],
    );
    assert_success(&create);
    let path = last_stdout_line(&create);
    std::fs::remove_dir_all(&path).expect("remove linked worktree directory");

    let output = command_output(repo.path(), ["list", "--type", "worktree", "--json"]);

    assert_success(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("feature/prunable-worktree"),
        "prunable missing worktree should not be listed as usable: {stdout}"
    );
}

#[test]
fn new_prunes_missing_worktree_before_reusing_branch() {
    let repo = TestRepo::with_remote();
    let first = command_output(
        repo.path(),
        [
            "new",
            "feature/prunable-reuse",
            "--from",
            "HEAD",
            "--no-init",
        ],
    );
    assert_success(&first);
    let stale_path = last_stdout_line(&first);
    std::fs::remove_dir_all(&stale_path).expect("remove linked worktree directory");

    let second = command_output(repo.path(), ["new", "feature/prunable-reuse", "--no-init"]);

    assert_success(&second);
    let new_path = last_stdout_line(&second);
    assert!(
        std::path::Path::new(&new_path).join(".git").exists(),
        "worktree should be recreated after pruning stale registration"
    );
}

#[test]
fn cleanup_yes_prunes_missing_worktree_before_deleting_merged_branch() {
    let repo = TestRepo::with_remote();
    let create = command_output(
        repo.path(),
        [
            "new",
            "feature/prunable-cleanup",
            "--from",
            "main",
            "--no-init",
        ],
    );
    assert_success(&create);
    let path = last_stdout_line(&create);
    let worktree = std::path::Path::new(&path);
    std::fs::write(worktree.join("prunable-cleanup.txt"), "cleanup\n").expect("write cleanup file");
    git(worktree, ["add", "prunable-cleanup.txt"]);
    git(worktree, ["commit", "-m", "prunable cleanup"]);
    git(
        repo.path(),
        [
            "merge",
            "--no-ff",
            "feature/prunable-cleanup",
            "-m",
            "merge prunable cleanup",
        ],
    );
    git(repo.path(), ["push", "origin", "main"]);
    git(repo.path(), ["fetch", "origin"]);
    std::fs::remove_dir_all(worktree).expect("remove linked worktree directory");

    let output = command_output(repo.path(), ["cleanup", "--yes"]);

    assert_success(&output);
    let branches = git(
        repo.path(),
        ["branch", "--list", "feature/prunable-cleanup"],
    );
    assert_eq!(
        String::from_utf8_lossy(&branches.stdout).trim(),
        "",
        "merged branch should be deleted after stale worktree is pruned"
    );
}

#[test]
fn list_type_remote_includes_non_origin_remote_branch() {
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
    git(repo.path(), ["switch", "-c", "feature/upstream-only"]);
    std::fs::write(repo.path().join("upstream-only.txt"), "upstream\n")
        .expect("write upstream branch file");
    git(repo.path(), ["add", "upstream-only.txt"]);
    git(repo.path(), ["commit", "-m", "upstream branch"]);
    git(
        repo.path(),
        ["push", "-u", "upstream", "feature/upstream-only"],
    );
    git(repo.path(), ["switch", "main"]);
    git(repo.path(), ["branch", "-D", "feature/upstream-only"]);

    let output = command_output(repo.path(), ["list", "--type", "remote", "--json"]);

    assert_success(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("feature/upstream-only"),
        "remote branch from non-origin remote should be listed: {stdout}"
    );
    assert!(
        stdout.contains("upstream/feature/upstream-only"),
        "remote ref should preserve non-origin remote name: {stdout}"
    );
}

#[test]
fn list_type_remote_preserves_slash_remote_name() {
    let repo = TestRepo::new();
    let upstream = tempfile::tempdir().expect("slash remote");
    git(upstream.path(), ["init", "--bare", "-b", "main"]);
    git(
        repo.path(),
        [
            "remote",
            "add",
            "team/upstream",
            upstream.path().to_str().expect("upstream path"),
        ],
    );
    git(repo.path(), ["push", "-u", "team/upstream", "main"]);
    git(repo.path(), ["switch", "-c", "feature/slash-remote"]);
    std::fs::write(repo.path().join("slash-remote.txt"), "slash\n")
        .expect("write slash remote file");
    git(repo.path(), ["add", "slash-remote.txt"]);
    git(repo.path(), ["commit", "-m", "slash remote branch"]);
    git(
        repo.path(),
        ["push", "-u", "team/upstream", "feature/slash-remote"],
    );
    git(repo.path(), ["switch", "main"]);
    git(repo.path(), ["branch", "-D", "feature/slash-remote"]);

    let output = command_output(repo.path(), ["list", "--type", "remote", "--json"]);

    assert_success(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"name\": \"feature/slash-remote\""),
        "slash remote name should not leak into the branch name: {stdout}"
    );
    assert!(
        stdout.contains("\"remote_ref\": \"team/upstream/feature/slash-remote\""),
        "remote ref should preserve the slash remote name: {stdout}"
    );
}

#[test]
fn list_type_remote_preserves_same_branch_from_multiple_remotes() {
    let repo = TestRepo::with_remote();
    repo.create_remote_branch("feature/shared");
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
    git(repo.path(), ["switch", "feature/shared"]);
    std::fs::write(repo.path().join("upstream-shared.txt"), "upstream\n")
        .expect("write upstream shared file");
    git(repo.path(), ["add", "upstream-shared.txt"]);
    git(repo.path(), ["commit", "-m", "upstream shared"]);
    git(repo.path(), ["push", "upstream", "feature/shared"]);
    git(repo.path(), ["switch", "main"]);
    git(repo.path(), ["branch", "-D", "feature/shared"]);
    git(repo.path(), ["fetch", "origin"]);
    git(repo.path(), ["fetch", "upstream"]);

    let output = command_output(repo.path(), ["list", "--type", "remote", "--json"]);

    assert_success(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("origin/feature/shared"),
        "origin remote branch should be listed: {stdout}"
    );
    assert!(
        stdout.contains("upstream/feature/shared"),
        "upstream remote branch should be listed: {stdout}"
    );
}

#[test]
fn open_type_remote_can_select_remote_qualified_duplicate_branch() {
    let repo = TestRepo::with_remote();
    repo.create_remote_branch("feature/shared");
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
    git(repo.path(), ["switch", "feature/shared"]);
    std::fs::write(repo.path().join("upstream-shared.txt"), "upstream\n")
        .expect("write upstream shared file");
    git(repo.path(), ["add", "upstream-shared.txt"]);
    git(repo.path(), ["commit", "-m", "upstream shared"]);
    git(repo.path(), ["push", "upstream", "feature/shared"]);
    git(repo.path(), ["switch", "main"]);
    git(repo.path(), ["branch", "-D", "feature/shared"]);
    git(repo.path(), ["fetch", "origin"]);
    git(repo.path(), ["fetch", "upstream"]);

    let output = command_output(
        repo.path(),
        ["open", "--type", "remote", "upstream/feature/shared"],
    );

    assert_success(&output);
    let upstream_ref = git(
        repo.path(),
        ["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
    );
    assert_eq!(
        String::from_utf8_lossy(&upstream_ref.stdout).trim(),
        "upstream/feature/shared"
    );
}

#[test]
fn open_type_remote_tracks_slash_remote_without_prefixing_branch_name() {
    let repo = TestRepo::new();
    let upstream = tempfile::tempdir().expect("slash remote");
    git(upstream.path(), ["init", "--bare", "-b", "main"]);
    git(
        repo.path(),
        [
            "remote",
            "add",
            "team/upstream",
            upstream.path().to_str().expect("upstream path"),
        ],
    );
    git(repo.path(), ["push", "-u", "team/upstream", "main"]);
    git(repo.path(), ["switch", "-c", "feature/slash-open"]);
    std::fs::write(repo.path().join("slash-open.txt"), "slash\n").expect("write slash open file");
    git(repo.path(), ["add", "slash-open.txt"]);
    git(repo.path(), ["commit", "-m", "slash open branch"]);
    git(
        repo.path(),
        ["push", "-u", "team/upstream", "feature/slash-open"],
    );
    git(repo.path(), ["switch", "main"]);
    git(repo.path(), ["branch", "-D", "feature/slash-open"]);

    let output = command_output(
        repo.path(),
        [
            "open",
            "--type",
            "remote",
            "team/upstream/feature/slash-open",
        ],
    );

    assert_success(&output);
    let current = git(repo.path(), ["branch", "--show-current"]);
    assert_eq!(
        String::from_utf8_lossy(&current.stdout).trim(),
        "feature/slash-open"
    );
    let upstream = git(
        repo.path(),
        ["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
    );
    assert_eq!(
        String::from_utf8_lossy(&upstream.stdout).trim(),
        "team/upstream/feature/slash-open"
    );
}

#[test]
fn open_type_remote_switches_existing_local_branch_for_selected_remote_name() {
    let repo = TestRepo::with_remote();
    repo.create_remote_branch("feature/shared");
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
    git(repo.path(), ["switch", "feature/shared"]);
    std::fs::write(repo.path().join("upstream-shared.txt"), "upstream\n")
        .expect("write upstream shared file");
    git(repo.path(), ["add", "upstream-shared.txt"]);
    git(repo.path(), ["commit", "-m", "upstream shared"]);
    git(repo.path(), ["push", "upstream", "feature/shared"]);
    git(repo.path(), ["switch", "main"]);
    git(repo.path(), ["fetch", "origin"]);
    git(repo.path(), ["fetch", "upstream"]);

    let output = command_output(
        repo.path(),
        ["open", "--type", "remote", "upstream/feature/shared"],
    );

    assert_success(&output);
    let current = git(repo.path(), ["branch", "--show-current"]);
    assert_eq!(
        String::from_utf8_lossy(&current.stdout).trim(),
        "feature/shared"
    );
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
    let status = git(
        repo.path(),
        ["status", "--porcelain", "--untracked-files=normal"],
    );
    assert_eq!(
        String::from_utf8_lossy(&status.stdout).trim(),
        "",
        "default worktree directory should not dirty the parent repository"
    );
}

#[test]
fn new_preserves_non_utf8_local_exclude_when_ignoring_generated_base_dir() {
    let repo = TestRepo::with_remote();
    let raw_exclude_path = git(repo.path(), ["rev-parse", "--git-path", "info/exclude"]);
    let exclude_path = {
        let path =
            Path::new(String::from_utf8_lossy(&raw_exclude_path.stdout).trim()).to_path_buf();
        if path.is_absolute() {
            path
        } else {
            repo.path().join(path)
        }
    };
    let existing_exclude = b"local-only-pattern\nnon-utf8-\xff\n".to_vec();
    std::fs::write(&exclude_path, &existing_exclude).expect("write non-UTF-8 exclude");

    let output = command_output(
        repo.path(),
        [
            "new",
            "feature/non-utf8-exclude",
            "--from",
            "HEAD",
            "--no-init",
        ],
    );

    assert_success(&output);
    let updated = std::fs::read(&exclude_path).expect("read updated exclude");
    assert!(
        updated.starts_with(&existing_exclude),
        "existing non-UTF-8 exclude bytes should be preserved: {updated:?}"
    );
    assert!(
        updated
            .windows(b"/.worktrees/\n".len())
            .any(|window| window == b"/.worktrees/\n"),
        "generated worktree base dir should be appended: {updated:?}"
    );
}

#[test]
fn new_without_from_uses_non_origin_remote_only_default_branch() {
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
    git(repo.path(), ["remote", "set-head", "upstream", "-a"]);
    git(repo.path(), ["switch", "-c", "feature/current", "main"]);
    std::fs::write(repo.path().join("current-only.txt"), "current\n").expect("write current file");
    git(repo.path(), ["add", "current-only.txt"]);
    git(repo.path(), ["commit", "-m", "current only"]);
    git(repo.path(), ["branch", "-D", "main"]);

    let output = command_output(repo.path(), ["new", "feature/from-upstream", "--no-init"]);

    assert_success(&output);
    let path = last_stdout_line(&output);
    assert!(
        !Path::new(&path).join("current-only.txt").exists(),
        "new worktree should start from upstream default branch, not current HEAD"
    );
}

#[test]
fn new_without_from_uses_non_origin_remote_head_default_branch() {
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
    git(repo.path(), ["switch", "-c", "develop", "main"]);
    std::fs::write(repo.path().join("develop-only.txt"), "develop\n").expect("write develop file");
    git(repo.path(), ["add", "develop-only.txt"]);
    git(repo.path(), ["commit", "-m", "develop default"]);
    git(repo.path(), ["push", "-u", "upstream", "develop"]);
    git(
        upstream.path(),
        ["symbolic-ref", "HEAD", "refs/heads/develop"],
    );
    git(repo.path(), ["fetch", "upstream"]);
    git(repo.path(), ["remote", "set-head", "upstream", "-a"]);
    git(repo.path(), ["switch", "-c", "feature/current", "main"]);
    std::fs::write(repo.path().join("current-only.txt"), "current\n").expect("write current file");
    git(repo.path(), ["add", "current-only.txt"]);
    git(repo.path(), ["commit", "-m", "current only"]);
    git(repo.path(), ["branch", "-D", "main"]);
    git(repo.path(), ["branch", "-D", "develop"]);

    let output = command_output(repo.path(), ["new", "feature/from-develop", "--no-init"]);

    assert_success(&output);
    let path = last_stdout_line(&output);
    assert!(Path::new(&path).join("develop-only.txt").exists());
    assert!(
        !Path::new(&path).join("current-only.txt").exists(),
        "new worktree should start from upstream HEAD, not current HEAD"
    );
}

#[test]
fn new_without_from_uses_slash_remote_head_default_branch() {
    let repo = TestRepo::new();
    let upstream = tempfile::tempdir().expect("slash remote");
    git(upstream.path(), ["init", "--bare", "-b", "main"]);
    git(
        repo.path(),
        [
            "remote",
            "add",
            "team/upstream",
            upstream.path().to_str().expect("upstream path"),
        ],
    );
    git(repo.path(), ["push", "-u", "team/upstream", "main"]);
    git(repo.path(), ["switch", "-c", "develop", "main"]);
    std::fs::write(repo.path().join("develop-only.txt"), "develop\n").expect("write develop file");
    git(repo.path(), ["add", "develop-only.txt"]);
    git(repo.path(), ["commit", "-m", "slash remote default"]);
    git(repo.path(), ["push", "-u", "team/upstream", "develop"]);
    git(
        upstream.path(),
        ["symbolic-ref", "HEAD", "refs/heads/develop"],
    );
    git(repo.path(), ["fetch", "team/upstream"]);
    git(repo.path(), ["remote", "set-head", "team/upstream", "-a"]);
    git(repo.path(), ["switch", "-c", "feature/current", "main"]);
    std::fs::write(repo.path().join("current-only.txt"), "current\n").expect("write current file");
    git(repo.path(), ["add", "current-only.txt"]);
    git(repo.path(), ["commit", "-m", "current only"]);
    git(repo.path(), ["branch", "-D", "main"]);
    git(repo.path(), ["branch", "-D", "develop"]);

    let output = command_output(repo.path(), ["new", "feature/from-slash", "--no-init"]);

    assert_success(&output);
    let path = last_stdout_line(&output);
    assert!(Path::new(&path).join("develop-only.txt").exists());
    assert!(
        !Path::new(&path).join("current-only.txt").exists(),
        "new worktree should start from slash remote HEAD, not current HEAD"
    );
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
fn new_from_linked_worktree_uses_primary_init_trust() {
    let repo = TestRepo::with_remote();
    let marker = repo.path().join("linked-init-ran");
    let init_command = format!("printf init > {}", marker.display());
    std::fs::write(
        repo.path().join(".git-ws.toml"),
        format!(
            r#"
[init]
on_create = ["{init_command}"]
"#
        ),
    )
    .expect("write config");
    git(repo.path(), ["add", ".git-ws.toml"]);
    git(repo.path(), ["commit", "-m", "add git ws init config"]);
    let config_home = tempfile::tempdir().expect("config home");
    write_trusted_init_store(repo.path(), config_home.path(), &[init_command]);
    let parent = command_output(
        repo.path(),
        [
            "new",
            "feature/trusted-parent",
            "--from",
            "HEAD",
            "--no-init",
        ],
    );
    assert_success(&parent);
    let parent_path = last_stdout_line(&parent);

    let child = Command::new(env!("CARGO_BIN_EXE_git-ws"))
        .current_dir(Path::new(&parent_path))
        .env("XDG_CONFIG_HOME", config_home.path())
        .args(["new", "feature/trusted-child"])
        .output()
        .expect("run git-ws");

    assert_success(&child);
    assert!(
        marker.exists(),
        "linked worktree should share primary worktree init trust"
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
fn new_rejects_legacy_short_hash_init_trust() {
    let repo = TestRepo::new();
    let marker = repo.path().join("legacy-hash-init-ran");
    let init_command = format!("printf init > {}", marker.display());
    std::fs::write(
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
    write_legacy_hashed_init_store(repo.path(), config_home.path(), &[init_command]);

    let output = Command::new(env!("CARGO_BIN_EXE_git-ws"))
        .current_dir(repo.path())
        .env("XDG_CONFIG_HOME", config_home.path())
        .args(["new", "feature/legacy-hash-trust"])
        .output()
        .expect("run git-ws");

    assert!(
        !output.status.success(),
        "legacy 64-bit hash trust should not authorize init commands"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("init commands are not trusted"),
        "stderr should explain missing trust, got:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!marker.exists(), "init command should not run");
    assert!(
        !repo
            .path()
            .join(".worktrees/feature-legacy-hash-trust")
            .exists(),
        "worktree should not be created before current init trust is accepted"
    );
    let branches = git(
        repo.path(),
        ["branch", "--list", "feature/legacy-hash-trust"],
    );
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
fn open_query_without_match_returns_error() {
    let repo = TestRepo::with_remote();

    let output = command_output(repo.path(), ["open", "missing-branch"]);

    assert!(
        !output.status.success(),
        "missing query should not succeed as a no-op"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("no match for query"),
        "stderr should explain no match, got:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
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
fn open_remote_query_uses_full_ref_when_tag_matches_remote_ref() {
    let repo = TestRepo::with_remote();
    repo.create_remote_branch("feature/remote-tag");
    git(repo.path(), ["branch", "-D", "feature/remote-tag"]);
    git(
        repo.path(),
        [
            "update-ref",
            "refs/tags/origin/feature/remote-tag",
            "refs/heads/main",
        ],
    );

    let output = command_output(
        repo.path(),
        ["open", "--type", "remote", "feature/remote-tag"],
    );

    assert_success(&output);
    let current = git(repo.path(), ["branch", "--show-current"]);
    assert_eq!(
        String::from_utf8_lossy(&current.stdout).trim(),
        "feature/remote-tag"
    );
    let upstream_remote = git(repo.path(), ["config", "branch.feature/remote-tag.remote"]);
    let upstream_merge = git(repo.path(), ["config", "branch.feature/remote-tag.merge"]);
    assert_eq!(
        String::from_utf8_lossy(&upstream_remote.stdout).trim(),
        "origin"
    );
    assert_eq!(
        String::from_utf8_lossy(&upstream_merge.stdout).trim(),
        "refs/heads/feature/remote-tag"
    );
}

#[test]
fn open_local_branch_ignores_same_named_tag() {
    let repo = TestRepo::new();
    git(repo.path(), ["switch", "-c", "feature/tag-name", "main"]);
    std::fs::write(repo.path().join("tag-name.txt"), "branch\n").expect("write branch file");
    git(repo.path(), ["add", "tag-name.txt"]);
    git(repo.path(), ["commit", "-m", "tag name branch"]);
    git(
        repo.path(),
        [
            "update-ref",
            "refs/tags/feature/tag-name",
            "refs/heads/main",
        ],
    );
    git(repo.path(), ["switch", "main"]);

    let output = command_output(repo.path(), ["open", "feature/tag-name"]);

    assert_success(&output);
    let current = git(repo.path(), ["branch", "--show-current"]);
    assert_eq!(
        String::from_utf8_lossy(&current.stdout).trim(),
        "feature/tag-name"
    );
}

#[test]
fn open_prunes_missing_worktree_before_switching_to_local_branch() {
    let repo = TestRepo::with_remote();
    git(repo.path(), ["switch", "-c", "feature/stale", "main"]);
    git(repo.path(), ["switch", "main"]);
    let stale_path = repo.path().join(".worktrees/feature-stale");
    git(
        repo.path(),
        [
            "worktree",
            "add",
            stale_path.to_str().expect("stale path"),
            "feature/stale",
        ],
    );
    std::fs::remove_dir_all(&stale_path).expect("remove stale worktree");

    let output = command_output(repo.path(), ["open", "feature/stale"]);

    assert_success(&output);
    let current = git(repo.path(), ["branch", "--show-current"]);
    assert_eq!(
        String::from_utf8_lossy(&current.stdout).trim(),
        "feature/stale"
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
    let upstream_remote = git(repo.path(), ["config", "branch.main.remote"]);
    let upstream_merge = git(repo.path(), ["config", "branch.main.merge"]);
    assert_eq!(
        String::from_utf8_lossy(&upstream_remote.stdout).trim(),
        "origin"
    );
    assert_eq!(
        String::from_utf8_lossy(&upstream_merge.stdout).trim(),
        "refs/heads/main"
    );
}

#[test]
fn main_rejects_extra_arguments_without_switching() {
    let repo = TestRepo::with_remote();
    repo.create_remote_branch("feature/current");
    git(repo.path(), ["switch", "feature/current"]);

    let output = command_output(repo.path(), ["main", "--help"]);

    assert!(
        !output.status.success(),
        "extra main argument should be rejected"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("unexpected"),
        "stderr should explain the unexpected argument, got:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let current = git(repo.path(), ["branch", "--show-current"]);
    assert_eq!(
        String::from_utf8_lossy(&current.stdout).trim(),
        "feature/current"
    );
}

#[test]
fn main_uses_full_default_ref_when_tag_matches_remote_ref() {
    let repo = TestRepo::with_remote();
    repo.create_remote_branch("feature/current");
    git(repo.path(), ["switch", "feature/current"]);
    git(repo.path(), ["branch", "-D", "main"]);
    git(
        repo.path(),
        [
            "update-ref",
            "refs/tags/origin/main",
            "refs/heads/feature/current",
        ],
    );

    let output = command_output(repo.path(), ["main"]);

    assert_success(&output);
    let current = git(repo.path(), ["branch", "--show-current"]);
    assert_eq!(String::from_utf8_lossy(&current.stdout).trim(), "main");
    let upstream_remote = git(repo.path(), ["config", "branch.main.remote"]);
    let upstream_merge = git(repo.path(), ["config", "branch.main.merge"]);
    assert_eq!(
        String::from_utf8_lossy(&upstream_remote.stdout).trim(),
        "origin"
    );
    assert_eq!(
        String::from_utf8_lossy(&upstream_merge.stdout).trim(),
        "refs/heads/main"
    );
}

#[test]
fn new_without_from_uses_full_default_ref_when_tag_matches_remote_ref() {
    let repo = TestRepo::with_remote();
    git(repo.path(), ["switch", "-c", "feature/current", "main"]);
    std::fs::write(repo.path().join("current-only.txt"), "current\n").expect("write current file");
    git(repo.path(), ["add", "current-only.txt"]);
    git(repo.path(), ["commit", "-m", "current only"]);
    git(
        repo.path(),
        [
            "update-ref",
            "refs/tags/origin/main",
            "refs/heads/feature/current",
        ],
    );
    git(repo.path(), ["branch", "-D", "main"]);

    let output = command_output(repo.path(), ["new", "feature/from-default", "--no-init"]);

    assert_success(&output);
    let path = last_stdout_line(&output);
    assert!(
        !Path::new(&path).join("current-only.txt").exists(),
        "new worktree should start from remote default, not the matching tag or current HEAD"
    );
}

#[test]
fn main_switches_to_default_remote_when_multiple_remotes_have_main() {
    let repo = TestRepo::with_remote();
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
    git(repo.path(), ["push", "upstream", "main"]);
    git(repo.path(), ["remote", "set-head", "origin", "-a"]);
    git(repo.path(), ["fetch", "upstream"]);
    git(repo.path(), ["switch", "-c", "feature/current", "main"]);
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
fn main_switches_to_non_origin_remote_only_default_branch() {
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
    git(repo.path(), ["remote", "set-head", "upstream", "-a"]);
    git(repo.path(), ["switch", "-c", "feature/current", "main"]);
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
        "upstream/main"
    );
}

#[test]
fn main_switches_to_slash_remote_head_default_branch() {
    let repo = TestRepo::new();
    let upstream = tempfile::tempdir().expect("slash remote");
    git(upstream.path(), ["init", "--bare", "-b", "main"]);
    git(
        repo.path(),
        [
            "remote",
            "add",
            "team/upstream",
            upstream.path().to_str().expect("upstream path"),
        ],
    );
    git(repo.path(), ["push", "-u", "team/upstream", "main"]);
    git(repo.path(), ["switch", "-c", "develop", "main"]);
    std::fs::write(repo.path().join("develop-only.txt"), "develop\n").expect("write develop file");
    git(repo.path(), ["add", "develop-only.txt"]);
    git(repo.path(), ["commit", "-m", "slash remote default"]);
    git(repo.path(), ["push", "-u", "team/upstream", "develop"]);
    git(
        upstream.path(),
        ["symbolic-ref", "HEAD", "refs/heads/develop"],
    );
    git(repo.path(), ["fetch", "team/upstream"]);
    git(repo.path(), ["remote", "set-head", "team/upstream", "-a"]);
    git(repo.path(), ["switch", "-c", "feature/current", "main"]);
    git(repo.path(), ["branch", "-D", "main"]);
    git(repo.path(), ["branch", "-D", "develop"]);

    let output = command_output(repo.path(), ["main"]);

    assert_success(&output);
    let current = git(repo.path(), ["branch", "--show-current"]);
    assert_eq!(String::from_utf8_lossy(&current.stdout).trim(), "develop");
    let upstream = git(
        repo.path(),
        ["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
    );
    assert_eq!(
        String::from_utf8_lossy(&upstream.stdout).trim(),
        "team/upstream/develop"
    );
}

#[test]
fn main_prunes_missing_worktree_before_switching_to_default_branch() {
    let repo = TestRepo::with_remote();
    git(repo.path(), ["switch", "-c", "feature/current", "main"]);
    let stale_path = repo.path().join(".worktrees/main-stale");
    git(
        repo.path(),
        [
            "worktree",
            "add",
            stale_path.to_str().expect("stale path"),
            "main",
        ],
    );
    std::fs::remove_dir_all(&stale_path).expect("remove stale worktree");

    let output = command_output(repo.path(), ["main"]);

    assert_success(&output);
    let current = git(repo.path(), ["branch", "--show-current"]);
    assert_eq!(String::from_utf8_lossy(&current.stdout).trim(), "main");
}

#[test]
fn master_errors_when_target_branch_is_missing() {
    let repo = TestRepo::new();

    let output = command_output(repo.path(), ["master"]);

    assert!(
        !output.status.success(),
        "missing master branch should not succeed as a no-op"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("branch not found"),
        "stderr should explain missing target, got:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn master_uses_full_remote_ref_when_tag_has_same_name() {
    let repo = TestRepo::with_remote();
    git(repo.path(), ["push", "origin", "main:master"]);
    git(repo.path(), ["fetch", "origin"]);
    git(
        repo.path(),
        ["update-ref", "refs/tags/master", "refs/heads/main"],
    );

    let output = command_output(repo.path(), ["master"]);

    assert_success(&output);
    let current = git(repo.path(), ["branch", "--show-current"]);
    assert_eq!(String::from_utf8_lossy(&current.stdout).trim(), "master");
    let upstream_remote = git(repo.path(), ["config", "branch.master.remote"]);
    let upstream_merge = git(repo.path(), ["config", "branch.master.merge"]);
    assert_eq!(
        String::from_utf8_lossy(&upstream_remote.stdout).trim(),
        "origin"
    );
    assert_eq!(
        String::from_utf8_lossy(&upstream_merge.stdout).trim(),
        "refs/heads/master"
    );
}

#[test]
fn master_rejects_extra_arguments_without_switching() {
    let repo = TestRepo::with_remote();
    git(repo.path(), ["push", "origin", "main:master"]);
    git(repo.path(), ["fetch", "origin"]);
    repo.create_remote_branch("feature/current");
    git(repo.path(), ["switch", "feature/current"]);

    let output = command_output(repo.path(), ["master", "typo"]);

    assert!(
        !output.status.success(),
        "extra master argument should be rejected"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("unexpected"),
        "stderr should explain the unexpected argument, got:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let current = git(repo.path(), ["branch", "--show-current"]);
    assert_eq!(
        String::from_utf8_lossy(&current.stdout).trim(),
        "feature/current"
    );
    let master = git(repo.path(), ["branch", "--list", "master"]);
    assert_eq!(String::from_utf8_lossy(&master.stdout).trim(), "");
}

#[test]
fn main_errors_when_default_branch_is_unknown() {
    let repo = TestRepo::new();
    git(repo.path(), ["branch", "-m", "main", "trunk"]);
    git(repo.path(), ["switch", "-c", "feature/current", "trunk"]);

    let output = command_output(repo.path(), ["main"]);

    assert!(
        !output.status.success(),
        "unknown default branch should not succeed as a no-op"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("default branch could not be determined"),
        "stderr should explain missing default branch, got:\n{}",
        String::from_utf8_lossy(&output.stderr)
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
fn cleanup_yes_protects_default_branch_when_tag_has_same_name() {
    let repo = TestRepo::with_remote();
    git(repo.path(), ["update-ref", "refs/tags/main", "HEAD"]);

    let output = command_output(repo.path(), ["cleanup", "--yes"]);

    assert_success(&output);
    let branches = git(repo.path(), ["branch", "--list", "main"]);
    assert!(
        String::from_utf8_lossy(&branches.stdout).contains("main"),
        "cleanup should preserve the default branch"
    );
}

#[test]
fn cleanup_json_reports_unmerged_gone_branch_as_skipped() {
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
    assert!(stdout.contains("SkipUnmerged"));
    assert!(stdout.contains("\"eligible\": false"));
    assert!(stdout.contains("\"requiresForce\": true"));
    assert!(stdout.contains("\"action\": \"skip\""));
}

#[test]
fn cleanup_json_reports_fresh_branch_as_unchanged() {
    let repo = TestRepo::with_remote();
    git(repo.path(), ["switch", "-c", "feature/fresh"]);

    let output = command_output(repo.path(), ["cleanup", "--json"]);

    assert_success(&output);
    let record = cleanup_json_record(&output.stdout, "feature/fresh");
    assert_eq!(record["defaultRelation"], "unchanged");
    assert_eq!(record["mergedToDefault"], false);
    assert_eq!(record["reasons"], json!(["unchanged"]));
}

#[test]
fn cleanup_json_reports_branch_without_own_commits_as_unchanged_after_default_advances() {
    let repo = TestRepo::with_remote();
    git(repo.path(), ["switch", "-c", "feature/no-own-commits"]);
    git(repo.path(), ["switch", "main"]);
    std::fs::write(repo.path().join("main-only.txt"), "main\n").expect("write main file");
    git(repo.path(), ["add", "main-only.txt"]);
    git(repo.path(), ["commit", "-m", "main advances"]);
    git(repo.path(), ["push", "origin", "main"]);

    let output = command_output(repo.path(), ["cleanup", "--json"]);

    assert_success(&output);
    let record = cleanup_json_record(&output.stdout, "feature/no-own-commits");
    assert_eq!(record["defaultRelation"], "unchanged");
    assert_eq!(record["mergedToDefault"], false);
    assert_eq!(record["reasons"], json!(["unchanged"]));
    assert_eq!(record["disposition"], "SafeDelete");
    assert_eq!(record["eligible"], true);
    assert_eq!(record["action"], "git branch -D feature/no-own-commits");
}

#[test]
fn cleanup_json_reports_branch_with_own_commits_merged_to_default_as_merged() {
    let repo = TestRepo::with_remote();
    git(repo.path(), ["switch", "-c", "feature/merged-json"]);
    std::fs::write(repo.path().join("merged-json.txt"), "merged\n").expect("write branch file");
    git(repo.path(), ["add", "merged-json.txt"]);
    git(repo.path(), ["commit", "-m", "merged json"]);
    git(repo.path(), ["switch", "main"]);
    git(
        repo.path(),
        [
            "merge",
            "--no-ff",
            "feature/merged-json",
            "-m",
            "merge json branch",
        ],
    );
    git(repo.path(), ["push", "origin", "main"]);

    let output = command_output(repo.path(), ["cleanup", "--json"]);

    assert_success(&output);
    let record = cleanup_json_record(&output.stdout, "feature/merged-json");
    assert_eq!(record["defaultRelation"], "merged");
    assert_eq!(record["mergedToDefault"], true);
    assert_eq!(record["reasons"], json!(["merged"]));
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
    git(repo.path(), ["remote", "set-head", "origin", "-a"]);
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
fn cleanup_yes_uses_non_origin_remote_only_default_branch() {
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
    git(repo.path(), ["remote", "set-head", "upstream", "-a"]);
    git(
        repo.path(),
        ["switch", "-c", "feature/default-merged", "main"],
    );
    std::fs::write(repo.path().join("default-merged.txt"), "default\n")
        .expect("write default merged file");
    git(repo.path(), ["add", "default-merged.txt"]);
    git(repo.path(), ["commit", "-m", "default merged"]);
    git(repo.path(), ["switch", "main"]);
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
    git(repo.path(), ["push", "upstream", "main"]);
    git(repo.path(), ["fetch", "upstream"]);
    git(
        repo.path(),
        ["switch", "-c", "feature/current", "upstream/main"],
    );
    git(repo.path(), ["branch", "-D", "main"]);

    let output = command_output(repo.path(), ["cleanup", "--yes"]);

    assert_success(&output);
    let branches = git(repo.path(), ["branch", "--list", "feature/default-merged"]);
    assert_eq!(
        String::from_utf8_lossy(&branches.stdout).trim(),
        "",
        "branch merged to upstream default should be deleted"
    );
}

#[test]
fn cleanup_yes_uses_non_origin_remote_head_default_branch() {
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
    git(repo.path(), ["switch", "-c", "develop", "main"]);
    std::fs::write(repo.path().join("develop-only.txt"), "develop\n").expect("write develop file");
    git(repo.path(), ["add", "develop-only.txt"]);
    git(repo.path(), ["commit", "-m", "develop default"]);
    git(repo.path(), ["push", "-u", "upstream", "develop"]);
    git(
        upstream.path(),
        ["symbolic-ref", "HEAD", "refs/heads/develop"],
    );
    git(
        repo.path(),
        ["switch", "-c", "feature/default-merged", "develop"],
    );
    std::fs::write(repo.path().join("default-merged.txt"), "default\n")
        .expect("write default merged file");
    git(repo.path(), ["add", "default-merged.txt"]);
    git(repo.path(), ["commit", "-m", "default merged"]);
    git(repo.path(), ["switch", "develop"]);
    git(
        repo.path(),
        [
            "merge",
            "--no-ff",
            "feature/default-merged",
            "-m",
            "merge into develop",
        ],
    );
    git(repo.path(), ["push", "upstream", "develop"]);
    git(repo.path(), ["fetch", "upstream"]);
    git(repo.path(), ["remote", "set-head", "upstream", "-a"]);
    git(
        repo.path(),
        ["switch", "-c", "feature/current", "upstream/develop"],
    );
    git(repo.path(), ["branch", "-D", "main"]);
    git(repo.path(), ["branch", "-D", "develop"]);

    let output = command_output(repo.path(), ["cleanup", "--yes"]);

    assert_success(&output);
    let branches = git(repo.path(), ["branch", "--list", "feature/default-merged"]);
    assert_eq!(
        String::from_utf8_lossy(&branches.stdout).trim(),
        "",
        "branch merged to upstream HEAD default should be deleted"
    );
}

#[test]
fn cleanup_yes_keeps_gone_branch_merged_only_to_current_head() {
    let repo = TestRepo::with_remote();
    repo.create_remote_branch("feature/gone-current-only");
    git(repo.path(), ["switch", "-c", "feature/current", "main"]);
    git(
        repo.path(),
        [
            "merge",
            "--no-ff",
            "feature/gone-current-only",
            "-m",
            "merge into current only",
        ],
    );
    git(
        repo.remote_path(),
        ["branch", "-D", "feature/gone-current-only"],
    );
    git(repo.path(), ["fetch", "--prune", "origin"]);

    let output = command_output(repo.path(), ["cleanup", "--yes"]);

    assert_success(&output);
    let branches = git(
        repo.path(),
        ["branch", "--list", "feature/gone-current-only"],
    );
    assert!(
        String::from_utf8_lossy(&branches.stdout).contains("feature/gone-current-only"),
        "gone branch merged only to current HEAD should not be deleted"
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
fn cleanup_yes_skips_primary_worktree_branch_from_linked_worktree() {
    let repo = TestRepo::with_remote();
    git(
        repo.path(),
        ["switch", "-c", "feature/primary-merged", "main"],
    );
    std::fs::write(repo.path().join("primary-merged.txt"), "primary\n")
        .expect("write primary merged file");
    git(repo.path(), ["add", "primary-merged.txt"]);
    git(repo.path(), ["commit", "-m", "primary merged"]);
    git(repo.path(), ["switch", "main"]);
    git(
        repo.path(),
        [
            "merge",
            "--no-ff",
            "feature/primary-merged",
            "-m",
            "merge into default",
        ],
    );
    git(repo.path(), ["push", "origin", "main"]);
    git(repo.path(), ["fetch", "origin"]);
    git(repo.path(), ["switch", "feature/primary-merged"]);
    let linked_parent = tempfile::tempdir().expect("linked worktree parent");
    let current_worktree = linked_parent.path().join("current-linked");
    git(
        repo.path(),
        [
            "worktree",
            "add",
            "-b",
            "feature/current-linked",
            current_worktree.to_str().expect("worktree path"),
            "main",
        ],
    );

    let output = command_output(&current_worktree, ["cleanup", "--yes"]);

    assert_success(&output);
    assert!(
        repo.path().join(".git").exists(),
        "primary worktree should be preserved"
    );
    let branches = git(repo.path(), ["branch", "--list", "feature/primary-merged"]);
    assert!(
        String::from_utf8_lossy(&branches.stdout).contains("feature/primary-merged"),
        "primary worktree branch should be preserved"
    );
}

#[test]
fn cleanup_yes_deletes_multiple_default_merged_worktrees() {
    let repo = TestRepo::with_remote();
    let worktree_parent = repo.path().join(".worktrees");
    std::fs::create_dir_all(&worktree_parent).expect("create worktree parent");
    let worktrees: Vec<_> = (0..8)
        .map(|index| {
            let branch = format!("feature/default-merged-{index}");
            let worktree = worktree_parent.join(format!("default-merged-{index}"));
            git(
                repo.path(),
                [
                    "worktree",
                    "add",
                    "-b",
                    branch.as_str(),
                    worktree.to_str().expect("worktree path"),
                    "main",
                ],
            );
            std::fs::write(
                worktree.join(format!("default-merged-{index}.txt")),
                format!("default {index}\n"),
            )
            .expect("write default merged file");
            git(&worktree, ["add", "."]);
            git(&worktree, ["commit", "-m", "default merged"]);
            git(
                repo.path(),
                [
                    "merge",
                    "--no-ff",
                    branch.as_str(),
                    "-m",
                    "merge into default",
                ],
            );
            (branch, worktree)
        })
        .collect();
    git(repo.path(), ["push", "origin", "main"]);
    git(repo.path(), ["fetch", "origin"]);

    let output = command_output(repo.path(), ["cleanup", "--yes"]);

    assert_success(&output);
    for (branch, worktree) in worktrees {
        assert!(
            !worktree.exists(),
            "merged worktree should have been removed: {}",
            worktree.display()
        );
        let branches = git(repo.path(), ["branch", "--list", branch.as_str()]);
        assert_eq!(
            String::from_utf8_lossy(&branches.stdout).trim(),
            "",
            "{branch} should have been deleted"
        );
    }
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
        ["update-ref", "-d", "refs/remotes/origin/HEAD"],
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
fn cleanup_yes_ignores_main_tag_when_default_branch_is_unknown() {
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
        ["update-ref", "-d", "refs/remotes/origin/HEAD"],
    );
    git(
        repo.path(),
        ["update-ref", "-d", "refs/remotes/origin/main"],
    );
    git(
        repo.path(),
        ["update-ref", "-d", "refs/remotes/origin/master"],
    );
    git(repo.path(), ["switch", "-c", "feature/tag-merged", "trunk"]);
    std::fs::write(repo.path().join("tag-merged.txt"), "tag\n").expect("write tag-merged file");
    git(repo.path(), ["add", "tag-merged.txt"]);
    git(repo.path(), ["commit", "-m", "tag merged only"]);
    git(repo.path(), ["update-ref", "refs/tags/main", "HEAD"]);
    git(repo.path(), ["switch", "trunk"]);

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
    let branches = git(repo.path(), ["branch", "--list", "feature/tag-merged"]);
    assert!(
        String::from_utf8_lossy(&branches.stdout).contains("feature/tag-merged"),
        "tag-merged branch should be preserved"
    );
}

#[test]
fn cleanup_yes_skips_unmerged_gone_branch() {
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

    assert_success(&output);
    let branches = git(repo.path(), ["branch", "--list", "feature/unmerged-gone"]);
    assert!(String::from_utf8_lossy(&branches.stdout).contains("feature/unmerged-gone"));
}

#[test]
fn cleanup_yes_skips_unmerged_gone_worktree() {
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

    assert_success(&output);
    assert!(worktree.exists(), "worktree should be preserved");
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

fn cleanup_json_record(stdout: &[u8], branch: &str) -> Value {
    let records: Vec<Value> = serde_json::from_slice(stdout).expect("parse cleanup JSON");
    records
        .into_iter()
        .find(|record| record.get("branch").and_then(Value::as_str) == Some(branch))
        .unwrap_or_else(|| panic!("missing cleanup record for {branch}"))
}

fn write_trusted_init_store(repo_path: &Path, config_home: &Path, init_commands: &[String]) {
    let trust_dir = config_home.join("git-ws");
    std::fs::create_dir_all(&trust_dir).expect("create trust dir");
    let trust_path = trust_dir.join("trust.toml");
    let repo_path = repo_path.canonicalize().expect("canonicalize repo path");
    let mut repos = BTreeMap::new();
    repos.insert(
        repo_path.display().to_string(),
        trusted_init_value(init_commands),
    );
    let raw = toml::to_string_pretty(&TestTrustStore { repos }).expect("serialize trust store");
    std::fs::write(trust_path, raw).expect("write trust store");
}

fn write_legacy_hashed_init_store(repo_path: &Path, config_home: &Path, init_commands: &[String]) {
    let trust_dir = config_home.join("git-ws");
    std::fs::create_dir_all(&trust_dir).expect("create trust dir");
    let trust_path = trust_dir.join("trust.toml");
    let repo_path = repo_path.canonicalize().expect("canonicalize repo path");
    let hash = trusted_init_hash(init_commands);
    std::fs::write(
        trust_path,
        format!("[repos]\n\"{}\" = \"{}\"\n", repo_path.display(), hash),
    )
    .expect("write legacy trust store");
}

fn trusted_init_hash(init_commands: &[String]) -> String {
    let mut hasher = DefaultHasher::new();
    let worktree_base_dir: Option<String> = None;
    worktree_base_dir.hash(&mut hasher);
    init_commands.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
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

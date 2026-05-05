#![allow(dead_code)]

use std::ffi::OsString;
use std::path::Path;
use std::process::{Command, Output};

pub struct TestRepo {
    root: tempfile::TempDir,
    remote: Option<tempfile::TempDir>,
}

impl TestRepo {
    pub fn unborn() -> Self {
        let root = tempfile::tempdir().expect("tempdir");
        git(root.path(), ["init", "-b", "main"]);
        Self { root, remote: None }
    }

    pub fn new() -> Self {
        let repo = Self::unborn();
        repo.configure_user();
        std::fs::write(repo.path().join("README.md"), "test\n").expect("write readme");
        git(repo.path(), ["add", "README.md"]);
        git(repo.path(), ["commit", "-m", "initial"]);
        repo
    }

    pub fn with_remote() -> Self {
        let mut repo = Self::new();
        let remote = tempfile::tempdir().expect("remote tempdir");
        git(remote.path(), ["init", "--bare", "-b", "main"]);
        git(
            repo.path(),
            [
                "remote",
                "add",
                "origin",
                remote.path().to_str().expect("remote path"),
            ],
        );
        git(repo.path(), ["push", "-u", "origin", "main"]);
        repo.remote = Some(remote);
        repo
    }

    pub fn path(&self) -> &Path {
        self.root.path()
    }

    pub fn remote_path(&self) -> &Path {
        self.remote.as_ref().expect("remote").path()
    }

    pub fn create_remote_branch(&self, branch: &str) {
        git(self.path(), ["switch", "-c", branch]);
        std::fs::write(
            self.path()
                .join(format!("{}.txt", branch.replace('/', "-"))),
            branch,
        )
        .expect("write branch file");
        git(self.path(), ["add", "."]);
        git(self.path(), ["commit", "-m", "branch commit"]);
        git(self.path(), ["push", "-u", "origin", branch]);
        git(self.path(), ["switch", "main"]);
    }

    pub fn create_pull_ref(&self, number: u64) {
        self.create_remote_branch(&format!("pull-source-{number}"));
        git(
            self.remote_path(),
            [
                "update-ref",
                &format!("refs/pull/{number}/head"),
                &format!("refs/heads/pull-source-{number}"),
            ],
        );
        git(self.path(), ["switch", "main"]);
    }

    fn configure_user(&self) {
        configure_git_user(self.path());
    }
}

pub fn configure_git_user(cwd: &Path) {
    git(cwd, ["config", "user.name", "Test User"]);
    git(cwd, ["config", "user.email", "test@example.com"]);
    git(cwd, ["config", "commit.gpgsign", "false"]);
}

pub fn command_output<const N: usize>(cwd: &Path, args: [&str; N]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_git-ws"))
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("run git-ws")
}

pub fn command_output_with_path<const N: usize>(
    cwd: &Path,
    extra_path: &Path,
    args: [&str; N],
) -> Output {
    Command::new(env!("CARGO_BIN_EXE_git-ws"))
        .current_dir(cwd)
        .env("PATH", prepend_path(extra_path))
        .args(args)
        .output()
        .expect("run git-ws")
}

#[track_caller]
pub fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

pub fn git<const N: usize>(cwd: &Path, args: [&str; N]) -> Output {
    let output = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

pub fn last_stdout_line(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .last()
        .expect("last stdout line")
        .to_string()
}

pub fn prepend_path(path: &Path) -> OsString {
    let mut value = OsString::from(path);
    value.push(":");
    value.push(std::env::var_os("PATH").unwrap_or_default());
    value
}

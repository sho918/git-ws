use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result, anyhow};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Worktree {
    pub path: PathBuf,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub is_main: bool,
}

#[derive(Debug, Clone)]
pub struct Repo {
    pub root: PathBuf,
}

pub fn ensure_repo() -> Result<Repo> {
    let root = git_output(["rev-parse", "--show-toplevel"])?;
    Ok(Repo {
        root: PathBuf::from(root.trim()),
    })
}

pub fn git_output<I, S>(args: I) -> Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = Command::new("git")
        .args(args)
        .output()
        .context("failed to spawn git")?;
    if !output.status.success() {
        return Err(anyhow!(
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout).context("git output was not UTF-8")
}

pub fn git_output_bytes<I, S>(args: I) -> Result<Vec<u8>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = Command::new("git")
        .args(args)
        .output()
        .context("failed to spawn git")?;
    if !output.status.success() {
        return Err(anyhow!(
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(output.stdout)
}

pub fn git_status<I, S>(args: I) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let status = Command::new("git")
        .args(args)
        .status()
        .context("failed to spawn git")?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("git exited with status {status}"))
    }
}

pub fn list_worktrees() -> Result<Vec<Worktree>> {
    let output = git_output_bytes(["worktree", "list", "--porcelain", "-z"])?;
    parse_worktree_porcelain(&output)
}

pub fn parse_worktree_porcelain(input: &[u8]) -> Result<Vec<Worktree>> {
    let mut worktrees = Vec::new();
    let mut current = PartialWorktree::default();

    for raw in input.split(|byte| *byte == 0) {
        if raw.is_empty() {
            current.flush(&mut worktrees)?;
            continue;
        }

        let line = std::str::from_utf8(raw).context("worktree porcelain was not UTF-8")?;
        if let Some(path) = line.strip_prefix("worktree ") {
            current.flush(&mut worktrees)?;
            current.path = Some(PathBuf::from(path));
        } else if let Some(head) = line.strip_prefix("HEAD ") {
            current.head = Some(head.to_string());
        } else if let Some(branch) = line.strip_prefix("branch ") {
            current.branch = Some(short_branch(branch));
        }
    }

    current.flush(&mut worktrees)?;
    for (index, worktree) in worktrees.iter_mut().enumerate() {
        worktree.is_main = index == 0;
    }

    Ok(worktrees)
}

#[derive(Default)]
struct PartialWorktree {
    path: Option<PathBuf>,
    head: Option<String>,
    branch: Option<String>,
}

impl PartialWorktree {
    fn flush(&mut self, worktrees: &mut Vec<Worktree>) -> Result<()> {
        let Some(path) = self.path.take() else {
            return Ok(());
        };
        worktrees.push(Worktree {
            path,
            head: self.head.take(),
            branch: self.branch.take(),
            is_main: false,
        });
        Ok(())
    }
}

pub fn list_local_branches() -> Result<Vec<(String, Option<String>, String)>> {
    let output = git_output([
        "for-each-ref",
        "--format=%(refname:short)%09%(upstream:short)%09%(objectname:short)",
        "refs/heads",
    ])?;
    Ok(output
        .lines()
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let branch = parts.next()?.to_string();
            let upstream = parts
                .next()
                .filter(|value| !value.is_empty())
                .map(ToString::to_string);
            let head = parts.next().unwrap_or_default().to_string();
            Some((branch, upstream, head))
        })
        .collect())
}

pub fn list_remote_branches() -> Result<Vec<(String, String, String)>> {
    let output = git_output([
        "for-each-ref",
        "--format=%(refname)%09%(refname:short)%09%(objectname:short)",
        "refs/remotes/origin",
    ])?;
    Ok(output
        .lines()
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let full = parts.next()?;
            let short = parts.next()?;
            let head = parts.next().unwrap_or_default();
            if full == "refs/remotes/origin/HEAD" {
                return None;
            }
            let name = short.strip_prefix("origin/")?;
            if name.is_empty() || name == "HEAD" || name == "origin" {
                return None;
            }
            Some((name.to_string(), format!("origin/{name}"), head.to_string()))
        })
        .collect())
}

pub fn branch_exists(branch: &str) -> bool {
    Command::new("git")
        .args(["show-ref", "--verify", "--quiet"])
        .arg(format!("refs/heads/{branch}"))
        .status()
        .is_ok_and(|status| status.success())
}

pub fn default_start_point() -> String {
    for candidate in [
        "origin/HEAD",
        "origin/main",
        "origin/master",
        "main",
        "master",
        "HEAD",
    ] {
        if Command::new("git")
            .args(["rev-parse", "--verify", "--quiet", candidate])
            .status()
            .is_ok_and(|status| status.success())
        {
            return candidate.to_string();
        }
    }
    "HEAD".to_string()
}

pub fn current_worktree_root() -> Result<PathBuf> {
    Ok(PathBuf::from(
        git_output(["rev-parse", "--show-toplevel"])?.trim(),
    ))
}

pub fn current_branch() -> Result<String> {
    Ok(git_output(["branch", "--show-current"])?.trim().to_string())
}

pub fn default_branch() -> String {
    for candidate in ["origin/main", "origin/master", "main", "master"] {
        if Command::new("git")
            .args(["rev-parse", "--verify", "--quiet", candidate])
            .status()
            .is_ok_and(|status| status.success())
        {
            return candidate.to_string();
        }
    }
    "HEAD".to_string()
}

pub fn short_branch(refname: &str) -> String {
    refname
        .strip_prefix("refs/heads/")
        .unwrap_or(refname)
        .to_string()
}

use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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
    let bytes = git_output_bytes(args)?;
    String::from_utf8(bytes).context("git output was not UTF-8")
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
    git_output_bytes(args).map(drop)
}

pub fn emit_cd_path(path: &Path) -> Result<()> {
    if let Some(cd_file) = env::var_os("GIT_WS_CD_FILE") {
        fs::write(&cd_file, format!("{}\n", path.display())).with_context(|| {
            format!(
                "failed to write cd target to {}",
                PathBuf::from(cd_file).display()
            )
        })?;
    } else {
        println!("{}", path.display());
    }
    Ok(())
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
    ref_exists(&format!("refs/heads/{branch}"))
}

pub fn default_start_point() -> String {
    first_existing_ref(&[
        "origin/HEAD",
        "origin/main",
        "origin/master",
        "main",
        "master",
        "HEAD",
    ])
    .unwrap_or_else(|| "HEAD".to_string())
}

pub fn current_worktree_root() -> Result<PathBuf> {
    Ok(ensure_repo()?.root)
}

pub fn current_branch() -> Result<String> {
    Ok(git_output(["branch", "--show-current"])?.trim().to_string())
}

pub fn default_branch() -> String {
    first_existing_ref(&["origin/main", "origin/master", "main", "master"])
        .unwrap_or_else(|| "HEAD".to_string())
}

pub fn ref_exists(refname: &str) -> bool {
    Command::new("git")
        .args(["rev-parse", "--verify", "--quiet", refname])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn first_existing_ref(candidates: &[&str]) -> Option<String> {
    candidates
        .iter()
        .find(|candidate| ref_exists(candidate))
        .map(|candidate| (*candidate).to_string())
}

pub fn short_branch(refname: &str) -> String {
    refname
        .strip_prefix("refs/heads/")
        .unwrap_or(refname)
        .to_string()
}

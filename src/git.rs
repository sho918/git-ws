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
    Ok(list_worktrees_with_prunable()?.0)
}

pub fn list_worktrees_with_prunable() -> Result<(Vec<Worktree>, bool)> {
    let output = git_output_bytes(["worktree", "list", "--porcelain", "-z"])?;
    parse_worktree_porcelain(&output)
}

pub fn prune_worktrees() -> Result<()> {
    git_status(["worktree", "prune", "--expire", "now"])
}

pub fn list_worktrees_after_prune_if_stale() -> Result<Vec<Worktree>> {
    let (worktrees, prunable_seen) = list_worktrees_with_prunable()?;
    if !prunable_seen {
        return Ok(worktrees);
    }
    prune_worktrees()?;
    list_worktrees()
}

pub fn parse_worktree_porcelain(input: &[u8]) -> Result<(Vec<Worktree>, bool)> {
    let mut worktrees = Vec::new();
    let mut current = PartialWorktree::default();
    let mut prunable_seen = false;

    for raw in input.split(|byte| *byte == 0) {
        if raw.is_empty() {
            current.flush(&mut worktrees, &mut prunable_seen)?;
            continue;
        }

        let line = std::str::from_utf8(raw).context("worktree porcelain was not UTF-8")?;
        if let Some(path) = line.strip_prefix("worktree ") {
            current.flush(&mut worktrees, &mut prunable_seen)?;
            current.path = Some(PathBuf::from(path));
        } else if let Some(head) = line.strip_prefix("HEAD ") {
            current.head = Some(head.to_string());
        } else if let Some(branch) = line.strip_prefix("branch ") {
            current.branch = Some(short_branch(branch));
        } else if line.starts_with("prunable") {
            current.prunable = true;
        }
    }

    current.flush(&mut worktrees, &mut prunable_seen)?;
    for (index, worktree) in worktrees.iter_mut().enumerate() {
        worktree.is_main = index == 0;
    }

    Ok((worktrees, prunable_seen))
}

#[derive(Default)]
struct PartialWorktree {
    path: Option<PathBuf>,
    head: Option<String>,
    branch: Option<String>,
    prunable: bool,
}

impl PartialWorktree {
    fn flush(&mut self, worktrees: &mut Vec<Worktree>, prunable_seen: &mut bool) -> Result<()> {
        let entry = std::mem::take(self);
        let Some(path) = entry.path else {
            return Ok(());
        };
        if entry.prunable {
            *prunable_seen = true;
            return Ok(());
        }
        worktrees.push(Worktree {
            path,
            head: entry.head,
            branch: entry.branch,
            is_main: false,
        });
        Ok(())
    }
}

pub fn list_local_branches() -> Result<Vec<(String, Option<String>, String)>> {
    let output = git_output([
        "for-each-ref",
        "--format=%(refname)%09%(upstream:short)%09%(objectname:short)",
        "refs/heads",
    ])?;
    Ok(output
        .lines()
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let branch = parts.next()?.strip_prefix("refs/heads/")?.to_string();
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
    let remotes = remote_names_by_length();
    let output = git_output([
        "for-each-ref",
        "--format=%(refname)%09%(objectname:short)",
        "refs/remotes",
    ])?;
    Ok(output
        .lines()
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let remote_ref = parts.next()?.strip_prefix("refs/remotes/")?;
            let head = parts.next().unwrap_or_default();
            let (_, name) =
                split_remote_ref(remote_ref, &remotes).or_else(|| remote_ref.split_once('/'))?;
            if name.is_empty() || name == "HEAD" {
                return None;
            }
            Some((name.to_string(), remote_ref.to_string(), head.to_string()))
        })
        .collect())
}

pub fn branch_exists(branch: &str) -> bool {
    ref_exists(&format!("refs/heads/{branch}"))
}

pub fn switch_target_exists(branch: &str) -> bool {
    branch_exists(branch) || remote_tracking_ref_for_branch(branch).is_some()
}

pub fn default_start_point() -> String {
    default_branch().unwrap_or_else(|| "HEAD".to_string())
}

pub fn current_worktree_root() -> Result<PathBuf> {
    Ok(ensure_repo()?.root)
}

pub fn primary_worktree_root() -> Result<PathBuf> {
    Ok(list_worktrees()?
        .into_iter()
        .next()
        .map(|worktree| worktree.path)
        .unwrap_or(ensure_repo()?.root))
}

pub fn current_branch() -> Result<String> {
    Ok(git_output(["branch", "--show-current"])?.trim().to_string())
}

pub fn default_branch() -> Option<String> {
    let remotes = remote_names_by_length();
    let candidates = collect_default_branch_candidates(&remotes);
    pick_default_branch(&candidates, &remotes)
}

pub fn local_branch_name(refname: &str) -> &str {
    if let Some(branch) = refname.strip_prefix("refs/heads/") {
        return branch;
    }
    if let Some(remote_ref) = refname.strip_prefix("refs/remotes/") {
        let remotes = remote_names_by_length();
        if let Some((_, branch)) = split_remote_ref(remote_ref, &remotes) {
            return branch;
        }
        return remote_ref
            .split_once('/')
            .map_or(remote_ref, |(_, branch)| branch);
    }
    refname
        .split_once('/')
        .map_or(refname, |(_, branch)| branch)
}

pub fn ref_exists(refname: &str) -> bool {
    Command::new("git")
        .args(["rev-parse", "--verify", "--quiet", refname])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

pub(crate) fn remote_names() -> Vec<String> {
    git_output(["remote"])
        .map(|output| {
            output
                .lines()
                .filter(|remote| !remote.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn remote_names_by_length() -> Vec<String> {
    let mut names = remote_names();
    names.sort_by(|left, right| right.len().cmp(&left.len()).then_with(|| left.cmp(right)));
    names
}

fn split_remote_ref<'a>(remote_ref: &'a str, remotes: &[String]) -> Option<(&'a str, &'a str)> {
    remotes.iter().find_map(|remote| {
        let branch = remote_ref.strip_prefix(remote)?.strip_prefix('/')?;
        Some((&remote_ref[..remote.len()], branch))
    })
}

struct DefaultBranchCandidate {
    refname: String,
    symref_target: Option<String>,
}

fn collect_default_branch_candidates(remotes: &[String]) -> Vec<DefaultBranchCandidate> {
    git_output([
        "for-each-ref",
        "--format=%(refname)%09%(symref)",
        "refs/remotes",
        "refs/heads/main",
        "refs/heads/master",
    ])
    .map(|output| {
        output
            .lines()
            .filter_map(|line| {
                let mut parts = line.split('\t');
                let refname = parts.next()?.to_string();
                if !is_default_branch_candidate(&refname, remotes) {
                    return None;
                }
                let symref_target = parts
                    .next()
                    .filter(|value| !value.is_empty())
                    .map(ToString::to_string);
                Some(DefaultBranchCandidate {
                    refname,
                    symref_target,
                })
            })
            .collect()
    })
    .unwrap_or_default()
}

fn is_default_branch_candidate(refname: &str, remotes: &[String]) -> bool {
    if matches!(refname, "refs/heads/main" | "refs/heads/master") {
        return true;
    }
    let Some(remote_ref) = refname.strip_prefix("refs/remotes/") else {
        return false;
    };
    let name = split_remote_ref(remote_ref, remotes)
        .map(|(_, branch)| branch)
        .or_else(|| remote_ref.split_once('/').map(|(_, branch)| branch));
    matches!(name, Some("HEAD" | "main" | "master"))
}

fn pick_default_branch(
    candidates: &[DefaultBranchCandidate],
    remotes: &[String],
) -> Option<String> {
    let head_target = |remote: &str| -> Option<String> {
        candidates
            .iter()
            .find(|candidate| candidate.refname == format!("refs/remotes/{remote}/HEAD"))
            .and_then(|candidate| candidate.symref_target.as_deref())
            .map(ToString::to_string)
    };
    if let Some(target) = head_target("origin") {
        return Some(target);
    }

    let has = |refname: &str| {
        candidates
            .iter()
            .any(|candidate| candidate.refname == refname)
    };
    let remote_match = |branch: &str| {
        candidates.iter().find_map(|candidate| {
            let remote_ref = candidate.refname.strip_prefix("refs/remotes/")?;
            let (_, name) =
                split_remote_ref(remote_ref, remotes).or_else(|| remote_ref.split_once('/'))?;
            (name == branch).then(|| candidate.refname.clone())
        })
    };

    for branch in ["main", "master"] {
        let origin_ref = format!("refs/remotes/origin/{branch}");
        if has(&origin_ref) {
            return Some(origin_ref);
        }
    }

    if let Some(target) = candidates
        .iter()
        .find(|candidate| candidate.refname.ends_with("/HEAD"))
        .and_then(|candidate| candidate.symref_target.as_deref())
    {
        return Some(target.to_string());
    }

    for branch in ["main", "master"] {
        if let Some(value) = remote_match(branch) {
            return Some(value);
        }
    }

    for branch in ["main", "master"] {
        let local_ref = format!("refs/heads/{branch}");
        if has(&local_ref) {
            return Some(local_ref);
        }
    }

    None
}

pub fn remote_tracking_refname(refname: &str) -> Option<String> {
    let remotes = remote_names_by_length();
    if let Some(remote_ref) = refname.strip_prefix("refs/remotes/") {
        split_remote_ref(remote_ref, &remotes).or_else(|| remote_ref.split_once('/'))?;
        return Some(refname.to_string());
    }
    let (remote, branch) =
        split_remote_ref(refname, &remotes).or_else(|| refname.split_once('/'))?;
    let full_ref = format!("refs/remotes/{remote}/{branch}");
    ref_exists(&full_ref).then_some(full_ref)
}

pub fn remote_tracking_ref_for_branch(branch: &str) -> Option<String> {
    let remotes = remote_names();
    if remotes.iter().any(|remote| remote == "origin") {
        let origin_ref = format!("refs/remotes/origin/{branch}");
        if ref_exists(&origin_ref) {
            return Some(origin_ref);
        }
    }

    remotes
        .into_iter()
        .filter(|remote| remote != "origin")
        .map(|remote| format!("refs/remotes/{remote}/{branch}"))
        .find(|refname| ref_exists(refname))
}

fn short_branch(refname: &str) -> String {
    refname
        .strip_prefix("refs/heads/")
        .unwrap_or(refname)
        .to_string()
}

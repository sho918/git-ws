use std::collections::{HashMap, HashSet};
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;

use anyhow::{Context, Result, anyhow};
use serde::Serialize;

use crate::git::{current_worktree_root, default_branch, git_output, git_status, list_worktrees};
use crate::path_to_str;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupInput {
    pub branch: String,
    pub worktree_path: Option<PathBuf>,
    pub is_current_worktree: bool,
    pub is_dirty: bool,
    pub upstream_gone: bool,
    pub merged_to_default: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum CleanupDisposition {
    SafeDelete,
    SkipCurrent,
    SkipDirty,
    SkipUnmerged,
}

pub fn classify_cleanup_candidate(input: &CleanupInput) -> CleanupDisposition {
    if input.is_current_worktree {
        return CleanupDisposition::SkipCurrent;
    }
    if input.is_dirty {
        return CleanupDisposition::SkipDirty;
    }
    if input.upstream_gone || input.merged_to_default {
        CleanupDisposition::SafeDelete
    } else {
        CleanupDisposition::SkipUnmerged
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CleanupOptions {
    pub dry_run: bool,
    pub yes: bool,
    pub force: bool,
    pub json: bool,
}

pub fn run_cleanup(options: CleanupOptions) -> Result<()> {
    let candidates = discover_cleanup_candidates()?;

    if options.json {
        print_cleanup_json(&candidates)?;
        return Ok(());
    }

    let safe: Vec<&CleanupInput> = candidates
        .iter()
        .filter(|input| match classify_cleanup_candidate(input) {
            CleanupDisposition::SafeDelete => true,
            CleanupDisposition::SkipCurrent => false,
            CleanupDisposition::SkipDirty | CleanupDisposition::SkipUnmerged => options.force,
        })
        .collect();

    if safe.is_empty() {
        println!("git-ws: nothing to clean");
        return Ok(());
    }

    println!("git-ws: cleanup candidates");
    for (index, input) in safe.iter().enumerate() {
        let path = input
            .worktree_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "-".to_string());
        println!("  {}. {} {}", index + 1, input.branch, path);
    }

    if options.dry_run {
        return Ok(());
    }

    let selected = if options.yes {
        (0..safe.len()).collect()
    } else {
        prompt_cleanup_selection(safe.len())?
    };

    for index in selected {
        delete_cleanup_candidate(safe[index], options.force)?;
    }

    Ok(())
}

pub fn discover_cleanup_candidates() -> Result<Vec<CleanupInput>> {
    let (current_root, worktrees, merged, local) = thread::scope(|scope| -> Result<_> {
        let current_root = scope.spawn(current_worktree_root);
        let worktrees = scope.spawn(list_worktrees);
        let merged = scope.spawn(merged_branches);
        let local = scope.spawn(local_branches_with_track);
        Ok((
            current_root.join().expect("current_worktree_root thread")?,
            worktrees.join().expect("list_worktrees thread")?,
            merged.join().expect("merged_branches thread")?,
            local.join().expect("local_branches_with_track thread")?,
        ))
    })?;
    let merged: HashSet<String> = merged.into_iter().collect();

    let dirty_paths = check_worktrees_dirty(&worktrees);

    Ok(local
        .into_iter()
        .map(|(branch, upstream_gone)| {
            let worktree_path = worktrees
                .iter()
                .find(|worktree| worktree.branch.as_deref() == Some(branch.as_str()))
                .map(|worktree| worktree.path.clone());
            let is_current_worktree = worktree_path.as_ref() == Some(&current_root);
            let is_dirty = worktree_path
                .as_ref()
                .is_some_and(|path| dirty_paths.get(path).copied().unwrap_or(false));
            CleanupInput {
                upstream_gone,
                merged_to_default: merged.contains(&branch),
                branch,
                worktree_path,
                is_current_worktree,
                is_dirty,
            }
        })
        .collect())
}

fn check_worktrees_dirty(worktrees: &[crate::git::Worktree]) -> HashMap<PathBuf, bool> {
    let paths: Vec<PathBuf> = worktrees
        .iter()
        .map(|worktree| worktree.path.clone())
        .collect();
    thread::scope(|scope| {
        let handles: Vec<_> = paths
            .iter()
            .map(|path| {
                let path = path.clone();
                scope.spawn(move || {
                    let is_dirty = !worktree_clean(&path).unwrap_or(false);
                    (path, is_dirty)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("worktree_clean thread"))
            .collect()
    })
}

fn local_branches_with_track() -> Result<Vec<(String, bool)>> {
    Ok(git_output([
        "for-each-ref",
        "--format=%(refname:short)%09%(upstream:track)",
        "refs/heads",
    ])?
    .lines()
    .filter_map(|line| {
        let (branch, track) = line.split_once('\t')?;
        Some((branch.to_string(), track.contains("[gone]")))
    })
    .collect())
}

fn merged_branches() -> Result<Vec<String>> {
    let default = default_branch();
    let default_local = default
        .strip_prefix("origin/")
        .unwrap_or(&default)
        .to_string();
    let output = match git_output([
        "branch",
        "--format=%(refname:short)",
        "--merged",
        default.as_str(),
    ]) {
        Ok(output) => output,
        Err(_) => return Ok(Vec::new()),
    };
    let protected: HashSet<&str> = ["main", "master", "develop", default_local.as_str()].into();
    Ok(output
        .lines()
        .map(|line| line.trim_start_matches('*').trim().to_string())
        .filter(|branch| !branch.is_empty() && !protected.contains(branch.as_str()))
        .collect())
}

fn worktree_clean(path: &Path) -> Result<bool> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["status", "--porcelain", "--untracked-files=normal"])
        .output()
        .with_context(|| format!("failed to inspect {}", path.display()))?;
    Ok(output.status.success() && output.stdout.is_empty())
}

fn prompt_cleanup_selection(max: usize) -> Result<Vec<usize>> {
    if !io::stdin().is_terminal() {
        return Err(anyhow!("cleanup requires --yes in non-interactive mode"));
    }

    print!("Delete which candidates? Enter numbers separated by spaces, or 'all': ");
    io::stdout().flush().ok();
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let trimmed = input.trim();
    if trimmed.eq_ignore_ascii_case("all") {
        return Ok((0..max).collect());
    }
    let mut selected = Vec::new();
    for part in trimmed.split(|ch: char| ch.is_ascii_whitespace() || ch == ',') {
        if part.is_empty() {
            continue;
        }
        let number: usize = part
            .parse()
            .with_context(|| format!("invalid selection: {part}"))?;
        if number == 0 || number > max {
            return Err(anyhow!("selection out of range: {number}"));
        }
        selected.push(number - 1);
    }
    Ok(selected)
}

fn delete_cleanup_candidate(input: &CleanupInput, force: bool) -> Result<()> {
    if let Some(path) = &input.worktree_path {
        git_status(["worktree", "remove", path_to_str(path)?])?;
    }
    let flag = if force { "-D" } else { "-d" };
    git_status(["branch", flag, input.branch.as_str()])
}

#[derive(Serialize)]
struct CleanupRecord<'a> {
    branch: &'a str,
    #[serde(rename = "worktreePath")]
    worktree_path: Option<&'a Path>,
    disposition: CleanupDisposition,
    #[serde(rename = "upstreamGone")]
    upstream_gone: bool,
    #[serde(rename = "mergedToDefault")]
    merged_to_default: bool,
    dirty: bool,
    current: bool,
}

fn print_cleanup_json(inputs: &[CleanupInput]) -> Result<()> {
    let values: Vec<CleanupRecord> = inputs
        .iter()
        .map(|input| CleanupRecord {
            branch: input.branch.as_str(),
            worktree_path: input.worktree_path.as_deref(),
            disposition: classify_cleanup_candidate(input),
            upstream_gone: input.upstream_gone,
            merged_to_default: input.merged_to_default,
            dirty: input.is_dirty,
            current: input.is_current_worktree,
        })
        .collect();
    println!("{}", serde_json::to_string_pretty(&values)?);
    Ok(())
}

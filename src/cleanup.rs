use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result, anyhow};

use crate::git::{current_worktree_root, default_branch, git_output, git_status, list_worktrees};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupInput {
    pub branch: String,
    pub worktree_path: Option<PathBuf>,
    pub is_current_worktree: bool,
    pub is_dirty: bool,
    pub upstream_gone: bool,
    pub merged_to_default: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupDisposition {
    SafeDelete,
    SkipCurrent,
    SkipDirty,
    SkipUnmerged,
}

pub fn classify_cleanup_candidate(input: CleanupInput) -> CleanupDisposition {
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

#[derive(Debug, Clone)]
pub struct CleanupCandidate {
    pub input: CleanupInput,
    pub disposition: CleanupDisposition,
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
    let safe: Vec<_> = candidates
        .iter()
        .filter(|candidate| {
            candidate.disposition == CleanupDisposition::SafeDelete || options.force
        })
        .collect();

    if options.json {
        print_cleanup_json(&candidates)?;
        return Ok(());
    }

    if safe.is_empty() {
        println!("git-ws: nothing to clean");
        return Ok(());
    }

    println!("git-ws: cleanup candidates");
    for (index, candidate) in safe.iter().enumerate() {
        let path = candidate
            .input
            .worktree_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "-".to_string());
        println!("  {}. {} {}", index + 1, candidate.input.branch, path);
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
        let candidate = safe[index];
        delete_cleanup_candidate(candidate, options.force)?;
    }

    Ok(())
}

pub fn discover_cleanup_candidates() -> Result<Vec<CleanupCandidate>> {
    let current_root = current_worktree_root()?;
    let worktrees = list_worktrees()?;
    let merged = merged_branches()?;
    let gone = gone_branches()?;
    let mut candidates = Vec::new();

    for branch in local_branch_names()? {
        let worktree_path = worktrees
            .iter()
            .find(|worktree| worktree.branch.as_deref() == Some(branch.as_str()))
            .map(|worktree| worktree.path.clone());
        let is_current_worktree = worktree_path.as_ref() == Some(&current_root);
        let is_dirty = worktree_path
            .as_ref()
            .is_some_and(|path| !worktree_clean(path).unwrap_or(false));
        let input = CleanupInput {
            upstream_gone: gone.iter().any(|gone_branch| gone_branch == &branch),
            merged_to_default: merged.iter().any(|merged_branch| merged_branch == &branch),
            branch,
            worktree_path,
            is_current_worktree,
            is_dirty,
        };
        let disposition = classify_cleanup_candidate(input.clone());
        candidates.push(CleanupCandidate { input, disposition });
    }

    Ok(candidates)
}

fn local_branch_names() -> Result<Vec<String>> {
    Ok(
        git_output(["for-each-ref", "--format=%(refname:short)", "refs/heads"])?
            .lines()
            .map(ToString::to_string)
            .collect(),
    )
}

fn gone_branches() -> Result<Vec<String>> {
    Ok(git_output([
        "for-each-ref",
        "--format=%(refname:short)%09%(upstream:track)",
        "refs/heads",
    ])?
    .lines()
    .filter_map(|line| {
        let (branch, track) = line.split_once('\t')?;
        track.contains("[gone]").then(|| branch.to_string())
    })
    .collect())
}

fn merged_branches() -> Result<Vec<String>> {
    let default = default_branch();
    let output = match git_output([
        "branch",
        "--format=%(refname:short)",
        "--merged",
        default.as_str(),
    ]) {
        Ok(output) => output,
        Err(_) => return Ok(Vec::new()),
    };

    Ok(output
        .lines()
        .map(|line| line.trim_start_matches('*').trim().to_string())
        .filter(|branch| !matches!(branch.as_str(), "main" | "master" | "develop"))
        .collect())
}

fn worktree_clean(path: &PathBuf) -> Result<bool> {
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

fn delete_cleanup_candidate(candidate: &CleanupCandidate, force: bool) -> Result<()> {
    if let Some(path) = &candidate.input.worktree_path {
        git_status(["worktree", "remove", path.to_str().unwrap_or_default()])?;
    }
    if force {
        git_status(["branch", "-D", candidate.input.branch.as_str()])
    } else {
        git_status(["branch", "-d", candidate.input.branch.as_str()])
            .or_else(|_| git_status(["branch", "-D", candidate.input.branch.as_str()]))
    }
}

fn print_cleanup_json(candidates: &[CleanupCandidate]) -> Result<()> {
    let values: Vec<_> = candidates
        .iter()
        .map(|candidate| {
            serde_json::json!({
                "branch": candidate.input.branch,
                "worktreePath": candidate.input.worktree_path,
                "disposition": format!("{:?}", candidate.disposition),
                "upstreamGone": candidate.input.upstream_gone,
                "mergedToDefault": candidate.input.merged_to_default,
                "dirty": candidate.input.is_dirty,
                "current": candidate.input.is_current_worktree,
            })
        })
        .collect();
    println!("{}", serde_json::to_string_pretty(&values)?);
    Ok(())
}

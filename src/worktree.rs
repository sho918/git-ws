use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow};

use crate::config::{
    FileConfig, ensure_base_dir_ignored, ensure_init_trusted, load_file_config, load_git_config,
    resolve_base_dir,
};
use crate::git::{
    Worktree, branch_exists, default_start_point, emit_cd_path, ensure_repo, git_status,
    list_worktrees_after_prune_if_stale, primary_worktree_root,
};
use crate::path_to_str;

const MAX_PATH_FALLBACK_ATTEMPTS: usize = 1000;
const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

#[derive(Debug, Clone)]
pub struct CreateWorktreeOptions {
    pub branch: String,
    pub start_point: Option<String>,
    pub path: Option<PathBuf>,
    pub run_init: bool,
}

pub fn create_worktree(options: CreateWorktreeOptions) -> Result<PathBuf> {
    let worktrees = list_worktrees_after_prune_if_stale()?;
    if let Some(path) = worktree_path_for_existing_branch(&worktrees, &options.branch) {
        emit_cd_path(&path)?;
        return Ok(path);
    }
    create_worktree_unchecked(options, &worktrees)
}

/// Create a new worktree without first checking whether the branch already
/// has one. The caller must have invoked `prune_worktrees` and pass a fresh
/// `worktrees` snapshot — stale entries cause `git worktree add` to fail and
/// the primary path used for trust/anchor decisions is read from this slice.
pub(crate) fn create_worktree_unchecked(
    options: CreateWorktreeOptions,
    worktrees: &[Worktree],
) -> Result<PathBuf> {
    let repo = ensure_repo()?;
    let file_config = load_file_config(&repo.root)?;
    let git_config = load_git_config();
    let base_anchor = worktrees
        .first()
        .map(|worktree| worktree.path.clone())
        .unwrap_or_else(|| repo.root.clone());
    let base_dir = resolve_base_dir(&base_anchor, &file_config, &git_config);
    let uses_generated_path = options.path.is_none();
    let path = match options.path {
        Some(path) => path,
        None => worktree_path_for_branch(&base_dir, &options.branch)?,
    };

    let should_run_init = should_run_init(options.run_init, &file_config);
    if should_run_init {
        ensure_init_trusted(&base_anchor, &file_config)?;
    }
    if uses_generated_path {
        ensure_base_dir_ignored(&base_anchor, &base_dir)?;
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let path_str = path_to_str(&path)?;
    if branch_exists(&options.branch) {
        git_status(["worktree", "add", path_str, options.branch.as_str()])?;
    } else {
        let start = options.start_point.unwrap_or_else(default_start_point);
        git_status([
            "worktree",
            "add",
            "-b",
            options.branch.as_str(),
            path_str,
            start.as_str(),
        ])?;
    }

    if should_run_init {
        for command in &file_config.init_commands {
            let status = Command::new("sh")
                .arg("-c")
                .arg(command)
                .current_dir(&path)
                .status()
                .with_context(|| format!("failed to run init command: {command}"))?;
            if !status.success() {
                return Err(anyhow!("init command failed: {command}"));
            }
        }
    }

    emit_cd_path(&path)?;
    Ok(path)
}

pub fn ensure_worktree_init_trusted(run_init: bool) -> Result<()> {
    let repo = ensure_repo()?;
    let file_config = load_file_config(&repo.root)?;
    if should_run_init(run_init, &file_config) {
        let primary = primary_worktree_root()?;
        ensure_init_trusted(&primary, &file_config)?;
    }
    Ok(())
}

pub fn find_worktree_for_branch(branch: &str) -> Result<Option<PathBuf>> {
    let worktrees = list_worktrees_after_prune_if_stale()?;
    Ok(worktree_path_for_existing_branch(&worktrees, branch))
}

pub(crate) fn worktree_path_for_existing_branch(
    worktrees: &[Worktree],
    branch: &str,
) -> Option<PathBuf> {
    worktrees
        .iter()
        .find(|worktree| worktree.branch.as_deref() == Some(branch))
        .map(|worktree| worktree.path.clone())
}

fn should_run_init(run_init: bool, file_config: &FileConfig) -> bool {
    run_init && !file_config.init_commands.is_empty()
}

pub fn path_segment_for_branch(branch: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in branch.chars() {
        if ch.is_alphanumeric() {
            out.extend(ch.to_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    let segment = out.trim_matches('-').to_string();
    if segment.is_empty() {
        fallback_path_segment(branch)
    } else {
        segment
    }
}

fn worktree_path_for_branch(base_dir: &Path, branch: &str) -> Result<PathBuf> {
    let segment = path_segment_for_branch(branch);
    let path = base_dir.join(&segment);
    if !path.exists() {
        return Ok(path);
    }

    let fallback_segment = format!("{segment}-{}", stable_branch_hash(branch));
    let fallback = base_dir.join(&fallback_segment);
    if !fallback.exists() {
        return Ok(fallback);
    }

    for suffix in 2..=MAX_PATH_FALLBACK_ATTEMPTS {
        let candidate = base_dir.join(format!("{fallback_segment}-{suffix}"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }

    Err(anyhow!(
        "could not find unused worktree path for branch {branch}"
    ))
}

fn fallback_path_segment(branch: &str) -> String {
    format!("worktree-{}", stable_branch_hash(branch))
}

fn stable_branch_hash(branch: &str) -> String {
    let mut hash = FNV_OFFSET_BASIS;
    for byte in branch.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{hash:016x}")
}

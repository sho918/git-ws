use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result, anyhow};

use crate::config::{
    FileConfig, ensure_init_trusted, load_file_config, load_git_config, resolve_base_dir,
};
use crate::git::{
    branch_exists, default_start_point, emit_cd_path, ensure_repo, git_status, list_worktrees,
    primary_worktree_root,
};
use crate::path_to_str;

#[derive(Debug, Clone)]
pub struct CreateWorktreeOptions {
    pub branch: String,
    pub start_point: Option<String>,
    pub path: Option<PathBuf>,
    pub run_init: bool,
}

pub fn create_worktree(options: CreateWorktreeOptions) -> Result<PathBuf> {
    if let Some(path) = find_worktree_for_branch(&options.branch)? {
        emit_cd_path(&path)?;
        return Ok(path);
    }

    let repo = ensure_repo()?;
    let file_config = load_file_config(&repo.root)?;
    let git_config = load_git_config();
    let base_anchor = primary_worktree_root()?;
    let base_dir = resolve_base_dir(&base_anchor, &file_config, &git_config);
    let path = options
        .path
        .unwrap_or_else(|| base_dir.join(path_segment_for_branch(&options.branch)));

    let should_run_init = should_run_init(options.run_init, &file_config);
    if should_run_init {
        ensure_init_trusted(&repo.root, &file_config)?;
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
        ensure_init_trusted(&repo.root, &file_config)?;
    }
    Ok(())
}

pub fn find_worktree_for_branch(branch: &str) -> Result<Option<PathBuf>> {
    Ok(list_worktrees()?
        .into_iter()
        .find(|worktree| worktree.branch.as_deref() == Some(branch))
        .map(|worktree| worktree.path))
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

fn fallback_path_segment(branch: &str) -> String {
    let mut hasher = DefaultHasher::new();
    branch.hash(&mut hasher);
    format!("worktree-{:016x}", hasher.finish())
}

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result, anyhow};

use crate::config::{ensure_init_trusted, load_file_config, load_git_config, resolve_base_dir};
use crate::git::{branch_exists, default_start_point, ensure_repo, git_status, list_worktrees};

#[derive(Debug, Clone)]
pub struct CreateWorktreeOptions {
    pub branch: String,
    pub start_point: Option<String>,
    pub path: Option<PathBuf>,
    pub run_init: bool,
}

pub fn create_worktree(options: CreateWorktreeOptions) -> Result<PathBuf> {
    if let Some(path) = find_worktree_for_branch(&options.branch)? {
        println!("{}", path.display());
        return Ok(path);
    }

    let repo = ensure_repo()?;
    let file_config = load_file_config(&repo.root)?;
    let git_config = load_git_config();
    let base_dir = resolve_base_dir(&repo.root, &file_config, &git_config);
    let path = options
        .path
        .unwrap_or_else(|| base_dir.join(path_segment_for_branch(&options.branch)));

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    if branch_exists(&options.branch) {
        git_status([
            "worktree",
            "add",
            path.to_str()
                .ok_or_else(|| anyhow!("worktree path is not UTF-8"))?,
            options.branch.as_str(),
        ])?;
    } else {
        let start = options.start_point.unwrap_or_else(default_start_point);
        git_status([
            "worktree",
            "add",
            "-b",
            options.branch.as_str(),
            path.to_str()
                .ok_or_else(|| anyhow!("worktree path is not UTF-8"))?,
            start.as_str(),
        ])?;
    }

    if options.run_init && !file_config.init_commands.is_empty() {
        ensure_init_trusted(&repo.root, &file_config)?;
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

    println!("{}", path.display());
    Ok(path)
}

pub fn find_worktree_for_branch(branch: &str) -> Result<Option<PathBuf>> {
    Ok(list_worktrees()?
        .into_iter()
        .find(|worktree| worktree.branch.as_deref() == Some(branch))
        .map(|worktree| worktree.path))
}

pub fn path_segment_for_branch(branch: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in branch.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

use std::process::Command;

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;

use crate::git::{git_status, list_remote_branches};
use crate::worktree::{CreateWorktreeOptions, create_worktree};

#[derive(Debug, Deserialize)]
struct IssueView {
    number: u64,
    title: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PullRequestView {
    number: u64,
    title: String,
    head_ref_name: String,
    is_cross_repository: bool,
}

pub fn create_issue_worktree(
    id: &str,
    base: Option<String>,
    branch: Option<String>,
    run_init: bool,
) -> Result<()> {
    let issue: IssueView = gh_json(["issue", "view", id, "--json", "number,title"])?;
    let slug = slugify_title(&issue.title);
    let branch = branch.unwrap_or_else(|| format!("issue/{}-{slug}", issue.number));
    create_worktree(CreateWorktreeOptions {
        branch,
        start_point: base,
        path: None,
        run_init,
    })?;
    Ok(())
}

pub fn create_pr_worktree(id: &str, branch: Option<String>, run_init: bool) -> Result<()> {
    let pr: PullRequestView = gh_json([
        "pr",
        "view",
        id,
        "--json",
        "number,title,headRefName,isCrossRepository",
    ])?;
    let slug = slugify_title(&pr.title);
    let branch = branch.unwrap_or_else(|| {
        if pr.is_cross_repository {
            format!("pr/{}-{slug}", pr.number)
        } else {
            pr.head_ref_name.clone()
        }
    });

    let start_point = if pr.is_cross_repository {
        git_status([
            "fetch",
            "origin",
            format!("refs/pull/{}/head:refs/heads/{branch}", pr.number).as_str(),
        ])?;
        None
    } else if !local_remote_exists(&pr.head_ref_name)? {
        git_status([
            "fetch",
            "origin",
            format!("{}:refs/heads/{branch}", pr.head_ref_name).as_str(),
        ])?;
        None
    } else {
        Some(format!("origin/{}", pr.head_ref_name))
    };

    create_worktree(CreateWorktreeOptions {
        branch,
        start_point,
        path: None,
        run_init,
    })?;
    Ok(())
}

pub fn slugify_title(title: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    let slug = out.trim_matches('-').to_string();
    if slug.is_empty() {
        "work".to_string()
    } else {
        slug
    }
}

fn gh_json<const N: usize, T: for<'de> Deserialize<'de>>(args: [&str; N]) -> Result<T> {
    let output = Command::new("gh")
        .args(args)
        .output()
        .context("failed to spawn gh")?;
    if !output.status.success() {
        return Err(anyhow!(
            "gh failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    serde_json::from_slice(&output.stdout).context("failed to parse gh JSON")
}

fn local_remote_exists(branch: &str) -> Result<bool> {
    Ok(list_remote_branches()?
        .iter()
        .any(|(name, _remote, _head)| name == branch))
}

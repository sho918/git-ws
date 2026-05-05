use std::process::Command;

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;

use crate::git::git_status;
use crate::picker::PickerEntry;
use crate::worktree::{
    CreateWorktreeOptions, create_worktree, ensure_worktree_init_trusted, find_worktree_for_branch,
};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct IssueListItem {
    pub number: u64,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequestListItem {
    pub number: u64,
    pub title: String,
    pub head_ref_name: String,
    pub is_cross_repository: bool,
}

pub fn create_issue_worktree(
    id: &str,
    base: Option<String>,
    branch: Option<String>,
    run_init: bool,
) -> Result<()> {
    let issue: IssueListItem = gh_json(["issue", "view", id, "--json", "number,title"])?;
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
    let pr: PullRequestListItem = gh_json([
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

    if find_worktree_for_branch(&branch)?.is_some() {
        create_worktree(CreateWorktreeOptions {
            branch,
            start_point: None,
            path: None,
            run_init,
        })?;
        return Ok(());
    }

    let refspec = if pr.is_cross_repository {
        format!("refs/pull/{}/head", pr.number)
    } else {
        pr.head_ref_name.clone()
    };
    ensure_worktree_init_trusted(run_init)?;
    fetch_into_branch(&refspec, &branch)?;

    create_worktree(CreateWorktreeOptions {
        branch,
        start_point: None,
        path: None,
        run_init,
    })?;
    Ok(())
}

pub fn list_open_issues() -> Result<Vec<IssueListItem>> {
    gh_json([
        "issue",
        "list",
        "--state",
        "open",
        "--limit",
        "100",
        "--json",
        "number,title",
    ])
}

pub fn list_open_prs() -> Result<Vec<PullRequestListItem>> {
    gh_json([
        "pr",
        "list",
        "--state",
        "open",
        "--limit",
        "100",
        "--json",
        "number,title,headRefName,isCrossRepository",
    ])
}

pub fn issue_picker_entries(issues: &[IssueListItem]) -> Vec<PickerEntry<String>> {
    issues
        .iter()
        .map(|issue| {
            let number = issue.number.to_string();
            PickerEntry {
                marker: format!("#{number}"),
                name: issue.title.clone(),
                detail: "open issue".to_string(),
                action: format!("create worktree for issue #{number}"),
                search_text: format!("#{number} {}", issue.title),
                value: number,
            }
        })
        .collect()
}

pub fn pr_picker_entries(prs: &[PullRequestListItem]) -> Vec<PickerEntry<String>> {
    prs.iter()
        .map(|pr| {
            let number = pr.number.to_string();
            let detail = if pr.is_cross_repository {
                format!("fork:{}", pr.head_ref_name)
            } else {
                pr.head_ref_name.clone()
            };
            PickerEntry {
                marker: format!("#{number}"),
                name: pr.title.clone(),
                detail,
                action: format!("create worktree for PR #{number}"),
                search_text: format!("#{number} {} {}", pr.title, pr.head_ref_name),
                value: number,
            }
        })
        .collect()
}

pub fn slugify_title(title: &str) -> String {
    let slug = crate::slugify(title);
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

fn fetch_into_branch(refspec: &str, branch: &str) -> Result<()> {
    git_status([
        "fetch",
        "origin",
        format!("{refspec}:refs/heads/{branch}").as_str(),
    ])
}

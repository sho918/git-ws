use std::process::Command;

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;

use crate::git::{
    emit_cd_path, git_output, git_status, list_worktrees_after_prune_if_stale, remote_names,
};
use crate::picker::PickerEntry;
use crate::worktree::{
    CreateWorktreeOptions, create_worktree, create_worktree_unchecked,
    ensure_worktree_init_trusted, worktree_path_for_existing_branch,
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RepositoryViewItem {
    name_with_owner: String,
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

pub fn create_pr_worktree(
    id: &str,
    branch: Option<String>,
    run_init: bool,
    force: bool,
) -> Result<()> {
    let pr: PullRequestListItem = gh_json([
        "pr",
        "view",
        id,
        "--json",
        "number,title,headRefName,isCrossRepository",
    ])?;
    let run_init = run_init && !pr.is_cross_repository;
    let slug = slugify_title(&pr.title);
    let branch = branch.unwrap_or_else(|| {
        if pr.is_cross_repository {
            format!("pr/{}-{slug}", pr.number)
        } else {
            pr.head_ref_name.clone()
        }
    });

    let worktrees = list_worktrees_after_prune_if_stale()?;
    if let Some(path) = worktree_path_for_existing_branch(&worktrees, &branch) {
        emit_cd_path(&path)?;
        return Ok(());
    }

    let refspec = if pr.is_cross_repository {
        format!("refs/pull/{}/head", pr.number)
    } else {
        format!("refs/heads/{}", pr.head_ref_name)
    };
    ensure_worktree_init_trusted(run_init)?;
    let remote = fetch_remote_for_pr(id)?;
    let upstream_head = (!pr.is_cross_repository).then_some(pr.head_ref_name.as_str());
    fetch_pr_head_into_branch(&remote, &refspec, &branch, force, upstream_head)?;
    if let Some(head) = upstream_head {
        set_pr_branch_upstream(&remote, head, &branch)?;
    }

    create_worktree_unchecked(
        CreateWorktreeOptions {
            branch,
            start_point: None,
            path: None,
            run_init,
        },
        &worktrees,
    )?;
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

fn fetch_remote_for_current_repository() -> Result<String> {
    if let Some(repository) = current_repository_name_with_owner()
        && let Some(remote) = remote_for_repository(&repository)?
    {
        return Ok(remote);
    }
    default_fetch_remote()
}

fn fetch_remote_for_pr(id: &str) -> Result<String> {
    if let Some(repository) = repository_path_from_pr_url(id) {
        return remote_for_repository(&repository)?
            .ok_or_else(|| anyhow!("no git remote matches PR URL base repository {repository}"));
    }
    fetch_remote_for_current_repository()
}

fn current_repository_name_with_owner() -> Option<String> {
    gh_json::<4, RepositoryViewItem>(["repo", "view", "--json", "nameWithOwner"])
        .ok()
        .map(|repository| repository.name_with_owner)
}

fn remote_for_repository(repository: &str) -> Result<Option<String>> {
    let output = git_output(["remote", "-v"])?;
    Ok(output.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let remote = parts.next()?;
        let url = parts.next()?;
        let direction = parts.next()?;
        if direction != "(fetch)" {
            return None;
        }
        (repository_path_from_remote_url(url).as_deref() == Some(repository))
            .then(|| remote.to_string())
    }))
}

fn default_fetch_remote() -> Result<String> {
    let mut remotes = remote_names();
    if let Some(position) = remotes.iter().position(|remote| remote == "origin") {
        return Ok(remotes.swap_remove(position));
    }
    remotes
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("no git remote is configured"))
}

fn repository_path_from_remote_url(url: &str) -> Option<String> {
    let path = if let Some((_, path)) = url
        .strip_prefix("git@")
        .and_then(|value| value.split_once(':'))
    {
        path
    } else if let Some((_, rest)) = url.split_once("://") {
        rest.split_once('/')?.1
    } else {
        return None;
    };
    let path = path.trim_end_matches('/').trim_end_matches(".git");
    let mut parts = path.rsplit('/').filter(|part| !part.is_empty());
    let repo = parts.next()?;
    let owner = parts.next()?;
    Some(format!("{owner}/{repo}"))
}

fn repository_path_from_pr_url(url: &str) -> Option<String> {
    let (_, rest) = url.split_once("://")?;
    let path = rest.split_once('/')?.1;
    let path = path.split(['?', '#']).next()?.trim_end_matches('/');
    let mut parts = path.split('/').filter(|part| !part.is_empty());
    let owner = parts.next()?;
    let repo = parts.next()?.trim_end_matches(".git");
    let kind = parts.next()?;
    let number = parts.next()?;
    if kind != "pull" || number.parse::<u64>().is_err() {
        return None;
    }
    Some(format!("{owner}/{repo}"))
}

fn fetch_pr_head_into_branch(
    remote: &str,
    refspec: &str,
    branch: &str,
    force: bool,
    upstream_head: Option<&str>,
) -> Result<()> {
    let force_prefix = if force { "+" } else { "" };
    let branch_refspec = format!("{force_prefix}{refspec}:refs/heads/{branch}");
    let tracking_refspec =
        upstream_head.map(|head| format!("+refs/heads/{head}:refs/remotes/{remote}/{head}"));
    let mut args: Vec<&str> = vec!["fetch", remote, branch_refspec.as_str()];
    if let Some(refspec) = tracking_refspec.as_deref() {
        args.push(refspec);
    }
    git_status(args).with_context(|| {
        if force {
            format!("failed to force-fetch PR head into local branch {branch}")
        } else {
            format!(
                "failed to fast-forward PR head into local branch {branch}; use --force to overwrite a divergent local branch"
            )
        }
    })
}

fn set_pr_branch_upstream(remote: &str, head_ref: &str, branch: &str) -> Result<()> {
    let upstream = format!("{remote}/{head_ref}");
    git_status(["branch", "--set-upstream-to", upstream.as_str(), branch])
        .with_context(|| format!("failed to set upstream for PR branch {branch}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_path_from_remote_url_accepts_common_github_urls() {
        assert_eq!(
            repository_path_from_remote_url("git@github.com:owner/repo.git").as_deref(),
            Some("owner/repo")
        );
        assert_eq!(
            repository_path_from_remote_url("https://github.com/owner/repo.git").as_deref(),
            Some("owner/repo")
        );
        assert_eq!(
            repository_path_from_remote_url("ssh://git@github.com/owner/repo").as_deref(),
            Some("owner/repo")
        );
    }

    #[test]
    fn repository_path_from_pr_url_accepts_github_pr_urls() {
        assert_eq!(
            repository_path_from_pr_url("https://github.com/owner/repo/pull/123").as_deref(),
            Some("owner/repo")
        );
        assert_eq!(
            repository_path_from_pr_url("https://github.com/owner/repo/pull/123/files").as_deref(),
            Some("owner/repo")
        );
        assert_eq!(
            repository_path_from_pr_url("https://github.com/owner/repo/pull/not-a-number"),
            None
        );
    }
}

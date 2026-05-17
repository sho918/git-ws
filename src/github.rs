use std::collections::{BTreeMap, HashMap, HashSet};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::SystemTime;

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::git::{
    emit_cd_path, git_output, git_status, list_worktrees_after_prune_if_stale, remote_names,
};
use crate::picker::PickerEntry;
use crate::tui::Tone;
use crate::worktree::{
    CreateWorktreeOptions, create_worktree, create_worktree_unchecked,
    ensure_worktree_init_trusted, worktree_path_for_existing_branch,
};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct IssueListItem {
    pub number: u64,
    pub title: String,
    #[serde(default)]
    pub author: Option<GitHubUser>,
    #[serde(default)]
    pub labels: Vec<GitHubLabel>,
    #[serde(default, rename = "updatedAt")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequestListItem {
    pub number: u64,
    pub title: String,
    pub head_ref_name: String,
    pub is_cross_repository: bool,
    #[serde(default)]
    pub author: Option<GitHubUser>,
    #[serde(default, rename = "baseRefName")]
    pub base_ref_name: Option<String>,
    #[serde(default, rename = "isDraft")]
    pub is_draft: bool,
    #[serde(default, rename = "reviewDecision")]
    pub review_decision: Option<String>,
    #[serde(default, rename = "updatedAt")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct GitHubUser {
    pub login: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct GitHubLabel {
    pub name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RepositoryViewItem {
    name_with_owner: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchPullRequestInfo {
    pub number: u64,
    pub title: String,
    pub head_ref_name: String,
    pub head_ref_oid: Option<String>,
    pub head_repository: Option<String>,
    pub base_ref_name: Option<String>,
    pub state: String,
    pub is_draft: bool,
    pub merged_at: Option<String>,
    pub closed_at: Option<String>,
    pub updated_at: Option<String>,
    pub url: String,
}

impl BranchPullRequestInfo {
    pub fn label(&self) -> String {
        format!("#{} {}", self.number, self.state)
    }

    pub fn is_merged_into(&self, base: &str) -> bool {
        self.state == "merged" && self.base_ref_name.as_deref() == Some(base)
    }

    pub fn is_merged_into_head(&self, base: &str, repository: &str, head_oid: &str) -> bool {
        self.is_merged_into(base)
            && self.head_ref_oid.as_deref() == Some(head_oid)
            && self
                .head_repository
                .as_deref()
                .is_some_and(|head_repository| head_repository.eq_ignore_ascii_case(repository))
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct RawBranchPullRequest {
    number: u64,
    title: String,
    head_ref_name: String,
    #[serde(default)]
    head_ref_oid: Option<String>,
    #[serde(default)]
    head_repository: Option<RawRepository>,
    #[serde(default)]
    base_ref_name: Option<String>,
    state: String,
    #[serde(default)]
    is_draft: bool,
    #[serde(default)]
    merged_at: Option<String>,
    #[serde(default)]
    closed_at: Option<String>,
    #[serde(default)]
    updated_at: Option<String>,
    url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct RawRepository {
    name_with_owner: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PullRequestCache {
    branches: BTreeMap<String, Vec<BranchPullRequestInfo>>,
}

const PR_CACHE_TTL_SECS: u64 = 300;

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
        "number,title,author,labels,updatedAt",
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
        "number,title,headRefName,isCrossRepository,author,baseRefName,isDraft,reviewDecision,updatedAt",
    ])
}

pub fn load_branch_pull_requests(
    branches: &[&str],
    refresh: bool,
) -> Result<HashMap<String, BranchPullRequestInfo>> {
    let branches = requested_branch_names(branches);
    if branches.is_empty() {
        return Ok(HashMap::new());
    }
    let Some(repository) = current_git_remote_repository()? else {
        return Ok(HashMap::new());
    };
    let pull_requests =
        if !refresh && let Some(cached) = read_fresh_pr_cache(&repository, &branches) {
            cached?
        } else {
            let cache = fetch_branch_pull_requests(&repository, &branches)?;
            let values = flatten_cached_pull_requests(&cache, &branches);
            write_pr_cache(&repository, cache).ok();
            values
        };

    Ok(pull_requests_by_branch(
        &branches,
        &repository,
        pull_requests,
    ))
}

pub fn issue_picker_entries(issues: &[IssueListItem]) -> Vec<PickerEntry<String>> {
    issues
        .iter()
        .map(|issue| {
            let number = issue.number.to_string();
            let author = author_label(issue.author.as_ref());
            let labels = labels_label(&issue.labels);
            let updated = date_label(issue.updated_at.as_deref());
            let branch = format!("issue/{}-{}", issue.number, slugify_title(&issue.title));
            PickerEntry {
                marker: format!("#{number}"),
                name: issue.title.clone(),
                detail: author.clone(),
                extra_columns: vec![labels.clone(), updated.clone(), branch.clone()],
                tones: vec![
                    Tone::Info,
                    Tone::Default,
                    Tone::Local,
                    Tone::Warning,
                    Tone::Dim,
                    Tone::Info,
                ],
                action: format!("create worktree for issue #{number}"),
                search_text: format!(
                    "#{number} {} {author} {labels} {updated} {branch}",
                    issue.title
                ),
                value: number,
            }
        })
        .collect()
}

pub fn pr_picker_entries(prs: &[PullRequestListItem]) -> Vec<PickerEntry<String>> {
    prs.iter()
        .map(|pr| {
            let number = pr.number.to_string();
            let head = if pr.is_cross_repository {
                format!("fork:{}", pr.head_ref_name)
            } else {
                pr.head_ref_name.clone()
            };
            let author = author_label(pr.author.as_ref());
            let base = pr.base_ref_name.clone().unwrap_or_else(|| "-".to_string());
            let state = pr_state_label(pr);
            let updated = date_label(pr.updated_at.as_deref());
            PickerEntry {
                marker: format!("#{number}"),
                name: pr.title.clone(),
                detail: author.clone(),
                extra_columns: vec![head.clone(), base.clone(), state.clone(), updated.clone()],
                tones: vec![
                    Tone::Info,
                    Tone::Default,
                    Tone::Local,
                    Tone::Remote,
                    Tone::Dim,
                    pr_state_tone(pr),
                    Tone::Dim,
                ],
                action: format!("create worktree for PR #{number}"),
                search_text: format!(
                    "#{number} {} {author} {head} {base} {state} {updated}",
                    pr.title
                ),
                value: number,
            }
        })
        .collect()
}

fn author_label(author: Option<&GitHubUser>) -> String {
    author
        .map(|user| user.login.clone())
        .unwrap_or_else(|| "-".to_string())
}

fn labels_label(labels: &[GitHubLabel]) -> String {
    if labels.is_empty() {
        "-".to_string()
    } else {
        labels
            .iter()
            .map(|label| label.name.as_str())
            .collect::<Vec<_>>()
            .join(",")
    }
}

fn date_label(value: Option<&str>) -> String {
    value
        .and_then(|value| value.get(..10))
        .unwrap_or("-")
        .to_string()
}

fn pr_state_label(pr: &PullRequestListItem) -> String {
    if pr.is_draft {
        return "draft".to_string();
    }
    match pr.review_decision.as_deref() {
        Some("APPROVED") => "approved",
        Some("CHANGES_REQUESTED") => "changes_requested",
        Some("REVIEW_REQUIRED") => "review_required",
        _ => "open",
    }
    .to_string()
}

fn pr_state_tone(pr: &PullRequestListItem) -> Tone {
    if pr.is_draft {
        return Tone::Warning;
    }
    match pr.review_decision.as_deref() {
        Some("APPROVED") => Tone::Good,
        Some("CHANGES_REQUESTED") => Tone::Bad,
        Some("REVIEW_REQUIRED") => Tone::Behind,
        _ => Tone::Dim,
    }
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

fn requested_branch_names(branches: &[&str]) -> Vec<String> {
    let mut values: Vec<String> = branches
        .iter()
        .copied()
        .filter(|branch| !branch.is_empty())
        .map(ToString::to_string)
        .collect();
    values.sort();
    values.dedup();
    values
}

fn fetch_branch_pull_requests(
    repository: &str,
    branches: &[String],
) -> Result<BTreeMap<String, Vec<BranchPullRequestInfo>>> {
    let mut values = BTreeMap::new();
    for branch in branches {
        values.insert(
            branch.clone(),
            fetch_branch_pull_requests_for_head(repository, branch)?,
        );
    }
    Ok(values)
}

fn fetch_branch_pull_requests_for_head(
    repository: &str,
    branch: &str,
) -> Result<Vec<BranchPullRequestInfo>> {
    let raw: Vec<RawBranchPullRequest> = gh_json([
        "pr",
        "list",
        "-R",
        repository,
        "--head",
        branch,
        "--state",
        "all",
        "--limit",
        "100",
        "--json",
        "number,title,headRefName,headRefOid,headRepository,baseRefName,state,isDraft,mergedAt,closedAt,updatedAt,url",
    ])?;
    Ok(raw.into_iter().map(normalize_branch_pr).collect())
}

fn normalize_branch_pr(raw: RawBranchPullRequest) -> BranchPullRequestInfo {
    let state = if raw.is_draft && raw.state == "OPEN" {
        "draft".to_string()
    } else {
        raw.state.to_ascii_lowercase()
    };
    BranchPullRequestInfo {
        number: raw.number,
        title: raw.title,
        head_ref_name: raw.head_ref_name,
        head_ref_oid: raw.head_ref_oid,
        head_repository: raw
            .head_repository
            .map(|repository| repository.name_with_owner),
        base_ref_name: raw.base_ref_name,
        state,
        is_draft: raw.is_draft,
        merged_at: raw.merged_at,
        closed_at: raw.closed_at,
        updated_at: raw.updated_at,
        url: raw.url,
    }
}

fn pull_requests_by_branch(
    branches: &[String],
    repository: &str,
    pull_requests: Vec<BranchPullRequestInfo>,
) -> HashMap<String, BranchPullRequestInfo> {
    let wanted: HashSet<&str> = branches.iter().map(String::as_str).collect();
    let mut values: HashMap<String, BranchPullRequestInfo> = HashMap::new();
    for pull_request in pull_requests {
        if !wanted.contains(pull_request.head_ref_name.as_str()) {
            continue;
        }
        if let Some(current) = values.get(pull_request.head_ref_name.as_str())
            && !better_branch_pr(&pull_request, current, repository)
        {
            continue;
        }
        values.insert(pull_request.head_ref_name.clone(), pull_request);
    }
    values
}

fn better_branch_pr(
    left: &BranchPullRequestInfo,
    right: &BranchPullRequestInfo,
    repository: &str,
) -> bool {
    let left_same_repository = is_same_head_repository(left, repository);
    let right_same_repository = is_same_head_repository(right, repository);
    if left_same_repository != right_same_repository {
        return left_same_repository;
    }
    let left_rank = pr_state_rank(left);
    let right_rank = pr_state_rank(right);
    left_rank > right_rank || (left_rank == right_rank && left.updated_at > right.updated_at)
}

fn is_same_head_repository(value: &BranchPullRequestInfo, repository: &str) -> bool {
    value
        .head_repository
        .as_deref()
        .is_some_and(|head_repository| head_repository.eq_ignore_ascii_case(repository))
}

fn pr_state_rank(value: &BranchPullRequestInfo) -> u8 {
    match value.state.as_str() {
        "open" | "draft" => 3,
        "merged" => 2,
        "closed" => 1,
        _ => 0,
    }
}

pub fn current_git_remote_repository() -> Result<Option<String>> {
    let output = git_output(["remote", "-v"])?;
    Ok(output.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let _remote = parts.next()?;
        let url = parts.next()?;
        let direction = parts.next()?;
        (direction == "(fetch)").then(|| github_repository_path_from_remote_url(url))?
    }))
}

fn read_fresh_pr_cache(
    repository: &str,
    branches: &[String],
) -> Option<Result<Vec<BranchPullRequestInfo>>> {
    let path = pr_cache_path(repository)?;
    let modified = fs::metadata(&path)
        .and_then(|metadata| metadata.modified())
        .ok()?;
    let age = SystemTime::now().duration_since(modified).ok()?;
    if age.as_secs() > PR_CACHE_TTL_SECS {
        return None;
    }
    let raw = fs::read(&path).ok()?;
    match serde_json::from_slice::<PullRequestCache>(&raw) {
        Ok(cache) => branches
            .iter()
            .all(|branch| cache.branches.contains_key(branch))
            .then(|| Ok(flatten_cached_pull_requests(&cache.branches, branches))),
        Err(error) => Some(Err(error).context("failed to parse PR cache")),
    }
}

fn flatten_cached_pull_requests(
    cache: &BTreeMap<String, Vec<BranchPullRequestInfo>>,
    branches: &[String],
) -> Vec<BranchPullRequestInfo> {
    branches
        .iter()
        .filter_map(|branch| cache.get(branch))
        .flat_map(|values| values.iter().cloned())
        .collect()
}

fn write_pr_cache(
    repository: &str,
    branches: BTreeMap<String, Vec<BranchPullRequestInfo>>,
) -> Result<()> {
    let Some(path) = pr_cache_path(repository) else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let cache = PullRequestCache { branches };
    let raw = serde_json::to_vec_pretty(&cache).context("failed to serialize PR cache")?;
    fs::write(&path, raw).with_context(|| format!("failed to write {}", path.display()))
}

fn pr_cache_path(repository: &str) -> Option<PathBuf> {
    let base = env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))?;
    Some(
        base.join("git-ws")
            .join("pr-cache-v3")
            .join(format!("{}.json", cache_key(repository))),
    )
}

fn cache_key(repository: &str) -> String {
    repository.replace('/', "_")
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

fn github_repository_path_from_remote_url(url: &str) -> Option<String> {
    is_github_remote_url(url)
        .then(|| repository_path_from_remote_url(url))
        .flatten()
}

fn is_github_remote_url(url: &str) -> bool {
    if let Some(rest) = url.strip_prefix("git@")
        && let Some((host, _)) = rest.split_once(':')
    {
        return host == "github.com";
    }
    if let Some((_, rest)) = url.split_once("://")
        && let Some((host_part, _)) = rest.split_once('/')
    {
        let host = host_part.rsplit('@').next().unwrap_or("");
        return host == "github.com";
    }
    false
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
    fn github_repository_path_from_remote_url_rejects_non_github_file_urls() {
        assert_eq!(
            github_repository_path_from_remote_url("file:///tmp/owner/repo.git"),
            None
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

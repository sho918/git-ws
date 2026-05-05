use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Result;
use serde::Serialize;

use crate::git::{Worktree, list_local_branches, list_remote_branches, list_worktrees};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Candidate {
    pub name: String,
    pub worktree_path: Option<PathBuf>,
    pub local_ref: Option<String>,
    pub remote_ref: Option<String>,
    pub upstream: Option<String>,
    pub worktree_head: Option<String>,
    pub local_head: Option<String>,
    pub remote_head: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateFilter {
    All,
    Worktree,
    Local,
    Remote,
}

impl CandidateFilter {
    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "all" => Some(Self::All),
            "worktree" | "wt" => Some(Self::Worktree),
            "local" => Some(Self::Local),
            "remote" => Some(Self::Remote),
            _ => None,
        }
    }

    fn accepts(self, candidate: &Candidate) -> bool {
        match self {
            Self::All => true,
            Self::Worktree => candidate.worktree_path.is_some(),
            Self::Local => candidate.local_ref.is_some(),
            Self::Remote => candidate.remote_ref.is_some(),
        }
    }
}

impl Candidate {
    fn new(name: String) -> Self {
        Self {
            name,
            worktree_path: None,
            local_ref: None,
            remote_ref: None,
            upstream: None,
            worktree_head: None,
            local_head: None,
            remote_head: None,
        }
    }

    pub fn detail(&self) -> String {
        if let Some(path) = &self.worktree_path {
            return path.display().to_string();
        }
        if let Some(upstream) = &self.upstream {
            return format!("upstream={upstream}");
        }
        self.remote_ref.clone().unwrap_or_else(|| "-".to_string())
    }

    pub fn availability_label(&self) -> String {
        fn flag(set: bool, on: char) -> char {
            if set { on } else { ' ' }
        }
        format!(
            "[{}][{}][{}]",
            flag(self.worktree_path.is_some(), 'W'),
            flag(self.local_ref.is_some(), 'L'),
            flag(self.remote_ref.is_some(), 'R'),
        )
    }
}

pub fn load_candidates(filter: CandidateFilter) -> Result<Vec<Candidate>> {
    let (worktrees, local, remote) = std::thread::scope(|scope| -> Result<_> {
        let worktrees = scope.spawn(list_worktrees);
        let local = scope.spawn(list_local_branches);
        let remote = scope.spawn(list_remote_branches);
        Ok((
            worktrees.join().expect("list_worktrees thread")?,
            local.join().expect("list_local_branches thread")?,
            remote.join().expect("list_remote_branches thread")?,
        ))
    })?;
    Ok(merge_candidates(worktrees, local, remote)
        .into_iter()
        .filter(|candidate| filter.accepts(candidate))
        .collect())
}

pub fn merge_candidates(
    worktrees: Vec<Worktree>,
    local_branches: Vec<(String, Option<String>, String)>,
    remote_branches: Vec<(String, String, String)>,
) -> Vec<Candidate> {
    let mut candidates = BTreeMap::<String, Candidate>::new();

    for worktree in worktrees.into_iter().filter(|worktree| !worktree.is_main) {
        let Some(branch) = worktree.branch else {
            continue;
        };
        let candidate = candidates
            .entry(branch.clone())
            .or_insert_with(|| Candidate::new(branch));
        candidate.worktree_path = Some(worktree.path);
        candidate.worktree_head = worktree.head;
    }

    for (branch, upstream, head) in local_branches {
        let candidate = candidates
            .entry(branch.clone())
            .or_insert_with(|| Candidate::new(branch.clone()));
        candidate.local_ref = Some(branch);
        candidate.upstream = upstream;
        candidate.local_head = Some(head);
    }

    for (name, remote_ref, head) in remote_branches {
        let candidate = candidates
            .entry(name.clone())
            .or_insert_with(|| Candidate::new(name));
        candidate.remote_ref = Some(remote_ref);
        candidate.remote_head = Some(head);
    }

    candidates.into_values().collect()
}

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Result;
use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher};
use serde::Serialize;

use crate::git::{Worktree, list_local_branches, list_remote_branches, list_worktrees};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CandidateAvailability {
    pub worktree: bool,
    pub local: bool,
    pub remote: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Candidate {
    pub name: String,
    pub availability: CandidateAvailability,
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
            Self::Worktree => candidate.availability.worktree,
            Self::Local => candidate.availability.local,
            Self::Remote => candidate.availability.remote,
        }
    }
}

impl Candidate {
    fn new(name: String) -> Self {
        Self {
            name,
            availability: CandidateAvailability {
                worktree: false,
                local: false,
                remote: false,
            },
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
        format!(
            "[{}][{}][{}]",
            if self.availability.worktree { "W" } else { " " },
            if self.availability.local { "L" } else { " " },
            if self.availability.remote { "R" } else { " " },
        )
    }
}

pub fn load_candidates(filter: CandidateFilter) -> Result<Vec<Candidate>> {
    let candidates = merge_candidates(
        list_worktrees()?,
        list_local_branches()?,
        list_remote_branches()?,
    );
    Ok(candidates
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
        candidate.availability.worktree = true;
        candidate.worktree_path = Some(worktree.path);
        candidate.worktree_head = worktree.head;
    }

    for (branch, upstream, head) in local_branches {
        let candidate = candidates
            .entry(branch.clone())
            .or_insert_with(|| Candidate::new(branch.clone()));
        candidate.availability.local = true;
        candidate.local_ref = Some(branch);
        candidate.upstream = upstream;
        candidate.local_head = Some(head);
    }

    for (name, remote_ref, head) in remote_branches {
        let candidate = candidates
            .entry(name.clone())
            .or_insert_with(|| Candidate::new(name));
        candidate.availability.remote = true;
        candidate.remote_ref = Some(remote_ref);
        candidate.remote_head = Some(head);
    }

    candidates.into_values().collect()
}

pub fn rank_candidates<'a>(query: &str, candidates: &'a [Candidate]) -> Vec<&'a Candidate> {
    if query.trim().is_empty() {
        return candidates.iter().collect();
    }

    let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
    let pattern = Pattern::new(
        query,
        CaseMatching::Smart,
        Normalization::Smart,
        AtomKind::Fuzzy,
    );
    let names: Vec<&str> = candidates
        .iter()
        .map(|candidate| candidate.name.as_str())
        .collect();
    pattern
        .match_list(names, &mut matcher)
        .into_iter()
        .filter_map(|(name, _score)| {
            candidates
                .iter()
                .find(|candidate| candidate.name.as_str() == name)
        })
        .collect()
}

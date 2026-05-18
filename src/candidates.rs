use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use anyhow::Result;
use serde::Serialize;

use crate::git::{
    LocalBranch, Worktree, list_local_branches, list_remote_branches, list_worktrees,
};
use crate::progress::Progress;

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
    pub tracking: TrackingInfo,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackingState {
    InSync,
    Ahead,
    Behind,
    Diverged,
    Gone,
    NoUpstream,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TrackingInfo {
    pub state: TrackingState,
    pub ahead: u32,
    pub behind: u32,
    pub summary: String,
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
            tracking: TrackingInfo::no_upstream(),
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

    pub fn head_label(&self) -> &str {
        self.worktree_head
            .as_deref()
            .or(self.local_head.as_deref())
            .or(self.remote_head.as_deref())
            .unwrap_or("-")
    }

    pub fn path_label(&self) -> String {
        self.worktree_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "-".to_string())
    }

    pub fn upstream_label(&self) -> &str {
        self.upstream
            .as_deref()
            .or(self.remote_ref.as_deref())
            .unwrap_or("-")
    }

    pub fn action_label(&self) -> String {
        if let Some(path) = &self.worktree_path {
            format!("cd {}", path.display())
        } else if let Some(local) = &self.local_ref {
            format!("git switch {local}")
        } else if let Some(remote) = &self.remote_ref {
            format!("git switch -c {} --track {remote}", self.name)
        } else {
            "unavailable".to_string()
        }
    }
}

impl TrackingInfo {
    pub fn no_upstream() -> Self {
        Self {
            state: TrackingState::NoUpstream,
            ahead: 0,
            behind: 0,
            summary: "no_upstream".to_string(),
        }
    }

    pub fn from_git_track(track: Option<&str>, has_upstream: bool) -> Self {
        if !has_upstream {
            return Self::no_upstream();
        }
        let Some(track) = track.filter(|value| !value.is_empty()) else {
            return Self {
                state: TrackingState::InSync,
                ahead: 0,
                behind: 0,
                summary: "in_sync".to_string(),
            };
        };
        if track == "[gone]" {
            return Self {
                state: TrackingState::Gone,
                ahead: 0,
                behind: 0,
                summary: "gone".to_string(),
            };
        }

        let mut ahead = 0;
        let mut behind = 0;
        for part in track.trim_matches(['[', ']']).split(',') {
            let mut values = part.split_whitespace();
            match (values.next(), values.next()) {
                (Some("ahead"), Some(value)) => ahead = value.parse().unwrap_or(0),
                (Some("behind"), Some(value)) => behind = value.parse().unwrap_or(0),
                _ => {}
            }
        }

        let state = match (ahead > 0, behind > 0) {
            (true, true) => TrackingState::Diverged,
            (true, false) => TrackingState::Ahead,
            (false, true) => TrackingState::Behind,
            (false, false) => TrackingState::InSync,
        };
        let summary = match state {
            TrackingState::Ahead => format!("ahead {ahead}"),
            TrackingState::Behind => format!("behind {behind}"),
            TrackingState::Diverged => format!("ahead {ahead}, behind {behind}"),
            TrackingState::InSync => "in_sync".to_string(),
            TrackingState::Gone => "gone".to_string(),
            TrackingState::NoUpstream => "no_upstream".to_string(),
        };
        Self {
            state,
            ahead,
            behind,
            summary,
        }
    }
}

pub fn load_candidates(filter: CandidateFilter) -> Result<Vec<Candidate>> {
    load_candidates_with_progress(filter, Progress::disabled())
}

pub fn load_candidates_with_progress(
    filter: CandidateFilter,
    progress: Progress,
) -> Result<Vec<Candidate>> {
    let (worktrees, local, remote) = std::thread::scope(|scope| -> Result<_> {
        let worktrees = scope.spawn(|| progress.run_result("loading worktrees", list_worktrees));
        let local =
            scope.spawn(|| progress.run_result("loading local branches", list_local_branches));
        let remote =
            scope.spawn(|| progress.run_result("loading remote branches", list_remote_branches));
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
    local_branches: Vec<LocalBranch>,
    remote_branches: Vec<(String, String, String)>,
) -> Vec<Candidate> {
    let mut candidates = BTreeMap::<String, Candidate>::new();
    let mut remote_name_counts = HashMap::<String, usize>::new();
    for (name, _, _) in &remote_branches {
        *remote_name_counts.entry(name.clone()).or_default() += 1;
    }

    for worktree in worktrees.into_iter().filter(|worktree| !worktree.is_main) {
        let Some(branch) = worktree.branch else {
            continue;
        };
        let candidate = candidates
            .entry(local_candidate_key(&branch))
            .or_insert_with(|| Candidate::new(branch));
        candidate.worktree_path = Some(worktree.path);
        candidate.worktree_head = worktree.head;
    }

    for local_branch in local_branches {
        let candidate = candidates
            .entry(local_candidate_key(&local_branch.name))
            .or_insert_with(|| Candidate::new(local_branch.name.clone()));
        let has_upstream = local_branch.upstream.is_some();
        candidate.local_ref = Some(local_branch.name);
        candidate.upstream = local_branch.upstream;
        candidate.local_head = Some(local_branch.head);
        candidate.tracking =
            TrackingInfo::from_git_track(local_branch.track.as_deref(), has_upstream);
    }

    for (name, remote_ref, head) in remote_branches {
        if let Some(candidate) = candidates.get_mut(&local_candidate_key(&name))
            && should_attach_remote_to_local_candidate(
                candidate,
                &remote_ref,
                remote_name_counts
                    .get(name.as_str())
                    .copied()
                    .unwrap_or_default(),
            )
        {
            candidate.remote_ref = Some(remote_ref);
            candidate.remote_head = Some(head);
            continue;
        }

        let candidate = candidates
            .entry(remote_candidate_key(&remote_ref))
            .or_insert_with(|| Candidate::new(name));
        candidate.remote_ref = Some(remote_ref);
        candidate.remote_head = Some(head);
    }

    candidates.into_values().collect()
}

fn local_candidate_key(branch: &str) -> String {
    format!("local:{branch}")
}

fn remote_candidate_key(remote_ref: &str) -> String {
    format!("remote:{remote_ref}")
}

fn should_attach_remote_to_local_candidate(
    candidate: &Candidate,
    remote_ref: &str,
    remote_name_count: usize,
) -> bool {
    candidate.upstream.as_deref() == Some(remote_ref)
        || (candidate.upstream.is_none()
            && candidate.remote_ref.is_none()
            && remote_name_count <= 1)
}

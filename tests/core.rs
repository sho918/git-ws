use std::path::PathBuf;

use git_ws::candidates::{Candidate, CandidateFilter, merge_candidates, rank_candidates};
use git_ws::cleanup::{CleanupDisposition, CleanupInput, classify_cleanup_candidate};
use git_ws::config::{FileConfig, GitConfig, load_file_config, resolve_base_dir};
use git_ws::git::{Worktree, parse_worktree_porcelain};
use git_ws::github::{
    IssueListItem, PullRequestListItem, issue_picker_entries, parse_issue_list_json,
    parse_pr_list_json, pr_picker_entries, slugify_title,
};
use git_ws::picker::{PickerEntry, rank_entries};
use git_ws::shell::init_script;
use git_ws::worktree::path_segment_for_branch;

#[test]
fn parses_nul_terminated_worktree_porcelain() {
    let input = b"worktree /repo\0HEAD aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\0branch refs/heads/main\0\0worktree /repo/.worktrees/feature\0HEAD bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\0branch refs/heads/feature/demo\0\0";

    let worktrees = parse_worktree_porcelain(input).expect("porcelain should parse");

    assert_eq!(
        worktrees,
        vec![
            Worktree {
                path: PathBuf::from("/repo"),
                head: Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string()),
                branch: Some("main".to_string()),
                is_main: true,
            },
            Worktree {
                path: PathBuf::from("/repo/.worktrees/feature"),
                head: Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string()),
                branch: Some("feature/demo".to_string()),
                is_main: false,
            },
        ]
    );
}

#[test]
fn parses_detached_worktree_without_branch() {
    let input = b"worktree /repo\0HEAD aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\0detached\0\0";

    let worktrees = parse_worktree_porcelain(input).expect("porcelain should parse");

    assert_eq!(
        worktrees,
        vec![Worktree {
            path: PathBuf::from("/repo"),
            head: Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string()),
            branch: None,
            is_main: true,
        }]
    );
}

#[test]
fn rejects_non_utf8_worktree_porcelain() {
    let error = parse_worktree_porcelain(b"worktree /repo\xff\0\0").unwrap_err();

    assert!(
        error.to_string().contains("UTF-8"),
        "unexpected error: {error:#}"
    );
}

#[test]
fn merges_worktree_local_and_remote_candidates_by_branch_name() {
    let candidates = merge_candidates(
        vec![Worktree {
            path: PathBuf::from("/repo/.worktrees/feature"),
            head: Some("abc1234".to_string()),
            branch: Some("feature/demo".to_string()),
            is_main: false,
        }],
        vec![(
            "feature/demo".to_string(),
            Some("origin/feature/demo".to_string()),
            "abc1234".to_string(),
        )],
        vec![(
            "feature/demo".to_string(),
            "origin/feature/demo".to_string(),
            "abc1234".to_string(),
        )],
    );

    assert_eq!(
        candidates,
        vec![Candidate {
            name: "feature/demo".to_string(),
            worktree_path: Some(PathBuf::from("/repo/.worktrees/feature")),
            local_ref: Some("feature/demo".to_string()),
            remote_ref: Some("origin/feature/demo".to_string()),
            upstream: Some("origin/feature/demo".to_string()),
            worktree_head: Some("abc1234".to_string()),
            local_head: Some("abc1234".to_string()),
            remote_head: Some("abc1234".to_string()),
        }]
    );
}

#[test]
fn candidate_filter_accepts_documented_aliases() {
    assert_eq!(CandidateFilter::parse("all"), Some(CandidateFilter::All));
    assert_eq!(
        CandidateFilter::parse("worktree"),
        Some(CandidateFilter::Worktree)
    );
    assert_eq!(
        CandidateFilter::parse("wt"),
        Some(CandidateFilter::Worktree)
    );
    assert_eq!(
        CandidateFilter::parse("LOCAL"),
        Some(CandidateFilter::Local)
    );
    assert_eq!(
        CandidateFilter::parse("remote"),
        Some(CandidateFilter::Remote)
    );
    assert_eq!(CandidateFilter::parse("unknown"), None);
}

#[test]
fn candidate_display_prefers_worktree_then_upstream_then_remote() {
    let mut candidate = Candidate {
        name: "feature/demo".to_string(),
        worktree_path: Some(PathBuf::from("/repo/.worktrees/demo")),
        local_ref: Some("feature/demo".to_string()),
        remote_ref: Some("origin/feature/demo".to_string()),
        upstream: Some("origin/feature/demo".to_string()),
        worktree_head: None,
        local_head: None,
        remote_head: None,
    };

    assert_eq!(candidate.availability_label(), "[W][L][R]");
    assert_eq!(candidate.detail(), "/repo/.worktrees/demo");

    candidate.worktree_path = None;
    assert_eq!(candidate.detail(), "upstream=origin/feature/demo");

    candidate.upstream = None;
    assert_eq!(candidate.detail(), "origin/feature/demo");
}

#[test]
fn fuzzy_ranking_returns_matching_candidates_only() {
    let candidates = vec![
        candidate_named("feature/worktree-cleanup"),
        candidate_named("bug/auth-redirect"),
        candidate_named("docs/release"),
    ];

    let ranked = rank_candidates("cleanup", &candidates);

    assert_eq!(
        ranked
            .iter()
            .map(|candidate| candidate.name.as_str())
            .collect::<Vec<_>>(),
        vec!["feature/worktree-cleanup"]
    );
}

#[test]
fn picker_entries_match_search_text_beyond_display_name() {
    let entries = vec![
        PickerEntry::new(
            "7".to_string(),
            "#7".to_string(),
            "Add PR worktree".to_string(),
            "feature/pr-head".to_string(),
            "create PR worktree".to_string(),
            "#7 Add PR worktree feature/pr-head".to_string(),
        ),
        PickerEntry::new(
            "8".to_string(),
            "#8".to_string(),
            "Fix cleanup".to_string(),
            "feature/cleanup".to_string(),
            "create PR worktree".to_string(),
            "#8 Fix cleanup feature/cleanup".to_string(),
        ),
    ];

    let ranked = rank_entries("pr head", &entries);

    assert_eq!(ranked[0].value, "7");
}

#[test]
fn parses_github_issue_list_and_builds_picker_entries() {
    let issues = parse_issue_list_json(br#"[{"number":42,"title":"Fix worktree cleanup"}]"#)
        .expect("issue list should parse");

    assert_eq!(
        issues,
        vec![IssueListItem {
            number: 42,
            title: "Fix worktree cleanup".to_string(),
        }]
    );

    let entries = issue_picker_entries(&issues);
    assert_eq!(entries[0].value, "42");
    assert_eq!(entries[0].marker, "#42");
    assert!(entries[0].search_text.contains("Fix worktree cleanup"));
}

#[test]
fn parses_github_pr_list_and_builds_picker_entries() {
    let prs = parse_pr_list_json(
        br#"[{"number":7,"title":"Add PR worktree","headRefName":"feature/pr-head","isCrossRepository":false}]"#,
    )
    .expect("PR list should parse");

    assert_eq!(
        prs,
        vec![PullRequestListItem {
            number: 7,
            title: "Add PR worktree".to_string(),
            head_ref_name: "feature/pr-head".to_string(),
            is_cross_repository: false,
        }]
    );

    let entries = pr_picker_entries(&prs);
    assert_eq!(entries[0].value, "7");
    assert_eq!(entries[0].marker, "#7");
    assert!(entries[0].search_text.contains("feature/pr-head"));
}

#[test]
fn resolves_base_dir_with_file_config_then_git_config_then_default() {
    let repo = PathBuf::from("/repo");

    assert_eq!(
        resolve_base_dir(
            &repo,
            &FileConfig {
                worktree_base_dir: Some("../repo-wt".to_string()),
                init_commands: vec![],
            },
            &GitConfig {
                ws_base_dir: Some(".ws".to_string()),
                wt_base_dir: Some(".worktrees".to_string()),
            },
        ),
        PathBuf::from("/repo/../repo-wt")
    );

    assert_eq!(
        resolve_base_dir(
            &repo,
            &FileConfig::default(),
            &GitConfig {
                ws_base_dir: None,
                wt_base_dir: Some(".worktrees".to_string()),
            },
        ),
        PathBuf::from("/repo/.worktrees")
    );
}

#[test]
fn loads_repository_config_file() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        temp.path().join(".git-ws.toml"),
        r#"
[worktree]
base_dir = "../repo-wt"

[init]
on_create = ["mise install", "cargo test"]
"#,
    )
    .expect("write config");

    let config = load_file_config(temp.path()).expect("load config");

    assert_eq!(
        config,
        FileConfig {
            worktree_base_dir: Some("../repo-wt".to_string()),
            init_commands: vec!["mise install".to_string(), "cargo test".to_string()],
        }
    );
}

#[test]
fn path_segment_replaces_branch_separators() {
    assert_eq!(
        path_segment_for_branch("Feature/Worktree Cleanup"),
        "feature-worktree-cleanup"
    );
    assert_eq!(path_segment_for_branch("issue/123.fix"), "issue-123-fix");
    assert_eq!(path_segment_for_branch("機能/修正"), "機能-修正");
}

#[test]
fn slugifies_github_titles_for_branch_names() {
    assert_eq!(
        slugify_title("API で認証トークンが期限切れになった際にログインへ遷移する"),
        "api"
    );
    assert_eq!(
        slugify_title("Fix: Worktree cleanup skips dirty branches!"),
        "fix-worktree-cleanup-skips-dirty-branches"
    );
}

#[test]
fn cleanup_keeps_only_safe_default_candidates() {
    assert_eq!(
        classify_cleanup_candidate(&CleanupInput {
            branch: "feature/gone".to_string(),
            worktree_path: Some(PathBuf::from("/repo/.worktrees/gone")),
            is_current_worktree: false,
            is_dirty: false,
            upstream_gone: true,
            merged_to_default: false,
        }),
        CleanupDisposition::SafeDelete
    );

    assert_eq!(
        classify_cleanup_candidate(&CleanupInput {
            branch: "feature/dirty".to_string(),
            worktree_path: Some(PathBuf::from("/repo/.worktrees/dirty")),
            is_current_worktree: false,
            is_dirty: true,
            upstream_gone: true,
            merged_to_default: true,
        }),
        CleanupDisposition::SkipDirty
    );
}

#[test]
fn cleanup_classification_protects_current_before_dirty() {
    assert_eq!(
        classify_cleanup_candidate(&CleanupInput {
            branch: "feature/current".to_string(),
            worktree_path: Some(PathBuf::from("/repo")),
            is_current_worktree: true,
            is_dirty: true,
            upstream_gone: true,
            merged_to_default: true,
        }),
        CleanupDisposition::SkipCurrent
    );
}

#[test]
fn cleanup_classification_skips_unmerged_branches() {
    assert_eq!(
        classify_cleanup_candidate(&CleanupInput {
            branch: "feature/live".to_string(),
            worktree_path: None,
            is_current_worktree: false,
            is_dirty: false,
            upstream_gone: false,
            merged_to_default: false,
        }),
        CleanupDisposition::SkipUnmerged
    );
}

#[test]
fn shell_init_supports_documented_shells() {
    assert!(init_script("fish").expect("fish").contains("function git"));
    assert!(init_script("zsh").expect("zsh").contains("git()"));
    assert!(init_script("bash").expect("bash").contains("git()"));
    assert!(init_script("powershell").is_err());
}

#[test]
fn shell_init_uses_side_channel_for_cd_targets() {
    for shell in ["fish", "zsh", "bash"] {
        let script = init_script(shell).expect(shell);
        assert!(script.contains("GIT_WS_CD_FILE"), "{shell} missing env var");
        assert!(script.contains("mktemp"), "{shell} missing mktemp");
    }
}

fn candidate_named(name: &str) -> Candidate {
    Candidate {
        name: name.to_string(),
        worktree_path: None,
        local_ref: Some(name.to_string()),
        remote_ref: None,
        upstream: None,
        worktree_head: None,
        local_head: None,
        remote_head: None,
    }
}

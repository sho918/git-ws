use std::env;
use std::ffi::OsString;
use std::io::{self, IsTerminal};
use std::path::PathBuf;

use anyhow::{Result, anyhow};
use git_ws::candidates::{Candidate, CandidateFilter, TrackingState, load_candidates};
use git_ws::cleanup::{CleanupOptions, run_cleanup};
use git_ws::git::{
    branch_exists, current_branch, default_branch, emit_cd_path, git_status, local_branch_name,
    remote_tracking_ref_for_branch, remote_tracking_refname,
};
use git_ws::github::{
    create_issue_worktree, create_pr_worktree, issue_picker_entries, list_open_issues,
    list_open_prs, pr_picker_entries,
};
use git_ws::picker::{PickerView, pick_candidate, pick_entry};
use git_ws::shell::init_script;
use git_ws::worktree::{CreateWorktreeOptions, create_worktree, find_worktree_for_branch};
use lexopt::prelude::*;
use ratatui::layout::Constraint;
use serde::Serialize;

const ISSUE_PICKER_WIDTHS: &[Constraint] = &[
    Constraint::Length(12),
    Constraint::Percentage(28),
    Constraint::Percentage(16),
    Constraint::Length(18),
    Constraint::Length(11),
    Constraint::Min(18),
];

const PR_PICKER_WIDTHS: &[Constraint] = &[
    Constraint::Length(12),
    Constraint::Percentage(28),
    Constraint::Percentage(16),
    Constraint::Length(11),
    Constraint::Length(14),
    Constraint::Length(16),
    Constraint::Length(11),
];

fn main() {
    if let Err(error) = run() {
        eprintln!("git-ws: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let mut args: Vec<OsString> = env::args_os().collect();
    let program = args
        .first()
        .and_then(|arg| {
            PathBuf::from(arg)
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| "git-ws".to_string());
    args.remove(0);

    if program.ends_with("git-co") {
        return cmd_open(args);
    }
    if program.ends_with("git-cleanup") {
        return cmd_cleanup(args);
    }
    if program.ends_with("git-main") {
        let target = match args.first().and_then(|arg| arg.to_str()) {
            Some("master") => {
                args.remove(0);
                MainTarget::Master
            }
            _ => MainTarget::Default,
        };
        return cmd_main_args(target, args);
    }

    let Some(command) = args
        .first()
        .and_then(|arg| arg.to_str())
        .map(str::to_string)
    else {
        return cmd_open(Vec::new());
    };
    let mut rest = args;
    rest.remove(0);

    match command.as_str() {
        "-h" | "--help" | "help" => {
            print_help();
            Ok(())
        }
        "open" | "co" => cmd_open(rest),
        "list" => cmd_list(rest),
        "new" => cmd_new(rest),
        "issue" => cmd_issue(rest),
        "pr" => cmd_pr(rest),
        "cleanup" => cmd_cleanup(rest),
        "main" => cmd_main_args(MainTarget::Default, rest),
        "master" => cmd_main_args(MainTarget::Master, rest),
        "init-shell" => cmd_init_shell(rest),
        "doctor" => cmd_doctor(),
        other => {
            let mut open_args = Vec::with_capacity(rest.len() + 1);
            open_args.push(OsString::from(other));
            open_args.extend(rest);
            cmd_open(open_args)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MainTarget {
    Default,
    Master,
}

fn cmd_main_args(target: MainTarget, args: Vec<OsString>) -> Result<()> {
    if let Some(arg) = args.first() {
        return Err(anyhow!("unexpected argument: {}", arg.to_string_lossy()));
    }
    cmd_main(target)
}

fn cmd_open(args: Vec<OsString>) -> Result<()> {
    let OpenArgs { filter, query } = parse_open_args(args)?;
    let candidates = load_candidates(filter)?;
    let Some(candidate) = pick_candidate(&candidates, query.as_deref())? else {
        return Ok(());
    };
    run_candidate(candidate)
}

fn cmd_list(args: Vec<OsString>) -> Result<()> {
    let ListArgs { filter, json } = parse_list_args(args)?;
    let candidates = load_candidates(filter)?;
    if json {
        print_candidates_json(&candidates)?;
    } else if io::stdout().is_terminal() {
        print_candidate_table(&candidates);
    } else {
        for candidate in candidates {
            println!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}",
                candidate.availability_label(),
                candidate.name,
                candidate.upstream_label(),
                candidate.tracking.summary,
                candidate.head_label(),
                candidate.path_label(),
                candidate.action_label()
            );
        }
    }
    Ok(())
}

fn cmd_new(args: Vec<OsString>) -> Result<()> {
    let new = parse_new_args(args)?;
    create_worktree(CreateWorktreeOptions {
        branch: new.branch,
        start_point: new.start_point,
        path: new.path,
        run_init: new.run_init,
    })?;
    Ok(())
}

fn cmd_issue(args: Vec<OsString>) -> Result<()> {
    let issue = parse_issue_args(args)?;
    let id = if let Some(id) = issue.id {
        id
    } else if let Some(id) = pick_issue_id()? {
        id
    } else {
        return Ok(());
    };
    create_issue_worktree(&id, issue.base, issue.branch, issue.run_init)
}

fn cmd_pr(args: Vec<OsString>) -> Result<()> {
    let pr = parse_pr_args(args)?;
    let id = if let Some(id) = pr.id {
        id
    } else if let Some(id) = pick_pr_id()? {
        id
    } else {
        return Ok(());
    };
    create_pr_worktree(&id, pr.branch, pr.run_init, pr.force)
}

fn pick_issue_id() -> Result<Option<String>> {
    let issues = list_open_issues()?;
    let entries = issue_picker_entries(&issues);
    pick_entry(
        &entries,
        None,
        PickerView {
            prompt: "git ws issue>",
            marker_header: "Issue",
            name_header: "Title",
            detail_header: "Author",
            extra_headers: &["Labels", "Updated", "Planned"],
            widths: ISSUE_PICKER_WIDTHS,
        },
    )
}

fn pick_pr_id() -> Result<Option<String>> {
    let prs = list_open_prs()?;
    let entries = pr_picker_entries(&prs);
    pick_entry(
        &entries,
        None,
        PickerView {
            prompt: "git ws pr>",
            marker_header: "PR",
            name_header: "Title",
            detail_header: "Author",
            extra_headers: &["Head", "Base", "State", "Updated"],
            widths: PR_PICKER_WIDTHS,
        },
    )
}

fn cmd_cleanup(args: Vec<OsString>) -> Result<()> {
    let options = parse_cleanup_args(args)?;
    run_cleanup(options)
}

fn cmd_main(target: MainTarget) -> Result<()> {
    let target_ref = match target {
        MainTarget::Master => "master".to_string(),
        MainTarget::Default => default_branch().ok_or_else(|| {
            anyhow!("default branch could not be determined; set remote HEAD or use main/master")
        })?,
    };
    let branch = local_branch_name(&target_ref);
    let remote_ref = remote_tracking_refname(&target_ref).or_else(|| {
        (target == MainTarget::Master)
            .then(|| remote_tracking_ref_for_branch(branch))
            .flatten()
    });
    if let Some(path) = find_worktree_for_branch(branch)? {
        emit_cd_path(&path)?;
        return Ok(());
    }
    if current_branch().ok().as_deref() == Some(branch) {
        emit_cd_path(&std::env::current_dir()?)?;
        return Ok(());
    }
    if branch_exists(branch) {
        git_status(["switch", branch])?;
        emit_cd_path(&std::env::current_dir()?)?;
        return Ok(());
    }
    if let Some(remote_ref) = remote_ref {
        git_status(["switch", "-c", branch, "--track", remote_ref.as_str()])?;
        emit_cd_path(&std::env::current_dir()?)?;
        return Ok(());
    }
    Err(anyhow!("branch not found: {branch}"))
}

fn cmd_init_shell(args: Vec<OsString>) -> Result<()> {
    let mut iter = args.into_iter();
    let shell = iter
        .next()
        .ok_or_else(|| anyhow!("missing shell: fish, zsh, or bash"))?;
    if iter.next().is_some() {
        return Err(anyhow!("init-shell takes a single shell argument"));
    }
    let shell = shell
        .to_str()
        .ok_or_else(|| anyhow!("shell argument is not UTF-8"))?;
    print!("{}", init_script(shell)?);
    Ok(())
}

fn cmd_doctor() -> Result<()> {
    let git = git_ws::git::git_output(["--version"])?.trim().to_string();
    let gh = match std::process::Command::new("gh").arg("--version").output() {
        Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
            .unwrap_or("gh available")
            .to_string(),
        _ => "gh unavailable".to_string(),
    };
    if io::stdout().is_terminal() {
        println!("git-ws doctor");
        println!("{:<8} {}", "git", git);
        println!("{:<8} {}", "gh", gh);
    } else {
        println!("git-ws: git {git}");
        println!("git-ws: {gh}");
    }
    Ok(())
}

fn print_candidate_table(candidates: &[Candidate]) {
    println!("git-ws worktrees");
    println!(
        "{:<12} {:<34} {:<28} {:<16} {:<10} {:<28} Action",
        "Status", "Name", "Upstream", "Track", "Head", "Path"
    );
    println!(
        "{:-<12} {:-<34} {:-<28} {:-<16} {:-<10} {:-<28} {:-<1}",
        "", "", "", "", "", "", ""
    );
    for candidate in candidates {
        println!(
            "{} {:<34} {} {} {:<10} {} {}",
            color_candidate_status(candidate, 12),
            candidate.name,
            color_remote(candidate.upstream_label(), 28),
            color_tracking(candidate, 16),
            candidate.head_label(),
            color_path(candidate, 28),
            color_info(&candidate.action_label())
        );
    }
}

#[derive(Serialize)]
struct CandidateRecord<'a> {
    #[serde(flatten)]
    candidate: &'a Candidate,
    action: String,
}

fn print_candidates_json(candidates: &[Candidate]) -> Result<()> {
    let records: Vec<_> = candidates
        .iter()
        .map(|candidate| CandidateRecord {
            candidate,
            action: candidate.action_label(),
        })
        .collect();
    println!("{}", serde_json::to_string_pretty(&records)?);
    Ok(())
}

fn color_candidate_status(candidate: &Candidate, width: usize) -> String {
    let code = if candidate.worktree_path.is_some() {
        32
    } else if candidate.local_ref.is_some() {
        33
    } else if candidate.remote_ref.is_some() {
        36
    } else {
        2
    };
    color_padded(&candidate.availability_label(), code, width)
}

fn color_tracking(candidate: &Candidate, width: usize) -> String {
    let code = match candidate.tracking.state {
        TrackingState::InSync | TrackingState::NoUpstream => 2,
        TrackingState::Ahead => 34,
        TrackingState::Behind | TrackingState::Diverged => 35,
        TrackingState::Gone => 31,
    };
    color_padded(&candidate.tracking.summary, code, width)
}

fn color_remote(value: &str, width: usize) -> String {
    if value == "-" {
        color_padded(value, 2, width)
    } else {
        color_padded(value, 36, width)
    }
}

fn color_path(candidate: &Candidate, width: usize) -> String {
    if candidate.worktree_path.is_some() {
        color_padded(&candidate.path_label(), 32, width)
    } else {
        color_padded("-", 2, width)
    }
}

fn color_info(value: &str) -> String {
    color(value, 36)
}

fn color(value: &str, code: u8) -> String {
    format!("\x1b[{code}m{value}\x1b[0m")
}

fn color_padded(value: &str, code: u8, width: usize) -> String {
    format!("\x1b[{code}m{value:<width$}\x1b[0m")
}

fn run_candidate(candidate: Candidate) -> Result<()> {
    if let Some(path) = candidate.worktree_path {
        emit_cd_path(&path)?;
        return Ok(());
    }
    if let Some(local) = candidate.local_ref {
        if let Some(path) = find_worktree_for_branch(&local)? {
            emit_cd_path(&path)?;
            return Ok(());
        }
        git_status(["switch", local.as_str()])?;
        return Ok(());
    }
    if let Some(remote) = candidate.remote_ref {
        if let Some(path) = find_worktree_for_branch(&candidate.name)? {
            emit_cd_path(&path)?;
            return Ok(());
        }
        if branch_exists(&candidate.name) {
            git_status(["switch", candidate.name.as_str()])?;
            return Ok(());
        }
        let remote_ref = remote_tracking_refname(&remote).unwrap_or(remote);
        git_status([
            "switch",
            "-c",
            candidate.name.as_str(),
            "--track",
            remote_ref.as_str(),
        ])?;
        return Ok(());
    }
    Err(anyhow!("no actionable target for {}", candidate.name))
}

#[derive(Debug)]
struct OpenArgs {
    filter: CandidateFilter,
    query: Option<String>,
}

fn parse_open_args(args: Vec<OsString>) -> Result<OpenArgs> {
    let mut parser = lexopt::Parser::from_args(args);
    let mut filter = CandidateFilter::All;
    let mut query = None;
    while let Some(arg) = parser.next()? {
        match arg {
            Long("type") => {
                let value = parser.value()?.string()?;
                filter = CandidateFilter::parse(&value)
                    .ok_or_else(|| anyhow!("unknown type: {value}"))?;
            }
            Long("help") | Short('h') => {
                print_help();
                std::process::exit(0);
            }
            Value(value) if query.is_none() => query = Some(value.string()?),
            _ => return Err(arg.unexpected().into()),
        }
    }
    Ok(OpenArgs { filter, query })
}

#[derive(Debug)]
struct ListArgs {
    filter: CandidateFilter,
    json: bool,
}

fn parse_list_args(args: Vec<OsString>) -> Result<ListArgs> {
    let mut parser = lexopt::Parser::from_args(args);
    let mut filter = CandidateFilter::All;
    let mut json = false;
    while let Some(arg) = parser.next()? {
        match arg {
            Long("type") => {
                let value = parser.value()?.string()?;
                filter = CandidateFilter::parse(&value)
                    .ok_or_else(|| anyhow!("unknown type: {value}"))?;
            }
            Long("json") => json = true,
            _ => return Err(arg.unexpected().into()),
        }
    }
    Ok(ListArgs { filter, json })
}

#[derive(Debug)]
struct NewArgs {
    branch: String,
    start_point: Option<String>,
    path: Option<PathBuf>,
    run_init: bool,
}

fn parse_new_args(args: Vec<OsString>) -> Result<NewArgs> {
    let mut parser = lexopt::Parser::from_args(args);
    let mut branch = None;
    let mut start_point = None;
    let mut path = None;
    let mut run_init = true;
    while let Some(arg) = parser.next()? {
        match arg {
            Long("from") => start_point = Some(parser.value()?.string()?),
            Long("path") => path = Some(PathBuf::from(parser.value()?)),
            Long("no-init") => run_init = false,
            Value(value) if branch.is_none() => branch = Some(value.string()?),
            _ => return Err(arg.unexpected().into()),
        }
    }
    Ok(NewArgs {
        branch: branch.ok_or_else(|| anyhow!("missing branch"))?,
        start_point,
        path,
        run_init,
    })
}

#[derive(Debug)]
struct IssueArgs {
    id: Option<String>,
    base: Option<String>,
    branch: Option<String>,
    run_init: bool,
}

fn parse_issue_args(args: Vec<OsString>) -> Result<IssueArgs> {
    let mut parser = lexopt::Parser::from_args(args);
    let mut id = None;
    let mut base = None;
    let mut branch = None;
    let mut run_init = true;
    while let Some(arg) = parser.next()? {
        match arg {
            Long("base") => base = Some(parser.value()?.string()?),
            Long("branch") => branch = Some(parser.value()?.string()?),
            Long("no-init") => run_init = false,
            Value(value) if id.is_none() => id = Some(value.string()?),
            _ => return Err(arg.unexpected().into()),
        }
    }
    Ok(IssueArgs {
        id,
        base,
        branch,
        run_init,
    })
}

#[derive(Debug)]
struct PrArgs {
    id: Option<String>,
    branch: Option<String>,
    run_init: bool,
    force: bool,
}

fn parse_pr_args(args: Vec<OsString>) -> Result<PrArgs> {
    let mut parser = lexopt::Parser::from_args(args);
    let mut id = None;
    let mut branch = None;
    let mut run_init = true;
    let mut force = false;
    while let Some(arg) = parser.next()? {
        match arg {
            Long("branch") => branch = Some(parser.value()?.string()?),
            Long("no-init") => run_init = false,
            Long("force") => force = true,
            Value(value) if id.is_none() => id = Some(value.string()?),
            _ => return Err(arg.unexpected().into()),
        }
    }
    Ok(PrArgs {
        id,
        branch,
        run_init,
        force,
    })
}

fn parse_cleanup_args(args: Vec<OsString>) -> Result<CleanupOptions> {
    let mut parser = lexopt::Parser::from_args(args);
    let mut options = CleanupOptions {
        dry_run: false,
        yes: false,
        force: false,
        json: false,
    };
    while let Some(arg) = parser.next()? {
        match arg {
            Long("dry-run") => options.dry_run = true,
            Long("yes") | Short('y') => options.yes = true,
            Long("force") => options.force = true,
            Long("json") => options.json = true,
            _ => return Err(arg.unexpected().into()),
        }
    }
    Ok(options)
}

fn print_help() {
    println!(
        r#"git ws: fast Git branch/worktree workspace helper

Usage:
  git ws [open] [query] [--type all|worktree|local|remote]
  git ws list [--json] [--type all|worktree|local|remote]
  git ws new <branch> [--from <ref>] [--path <path>] [--no-init]
  git ws issue [number|url] [--base <ref>] [--branch <name>] [--no-init]
  git ws pr [number|url] [--branch <name>] [--no-init] [--force]
  git ws cleanup [--dry-run] [--yes] [--force] [--json]
  git ws main
  git ws init-shell fish|zsh|bash
  git ws doctor

Run open, issue, or pr without a target to use the interactive fuzzy picker.
TTY views show colored status columns; non-TTY and JSON output stay plain.
list --json adds tracking/action fields; cleanup --json adds eligibility/action fields.
Use `git ws open --type remote` to pick from remote branches only.
"#
    );
}

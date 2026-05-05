use std::env;
use std::ffi::OsString;
use std::path::PathBuf;

use anyhow::{Result, anyhow};
use git_ws::candidates::{Candidate, CandidateFilter, load_candidates};
use git_ws::cleanup::{CleanupOptions, run_cleanup};
use git_ws::git::{branch_exists, current_branch, default_branch, emit_cd_path, git_status};
use git_ws::github::{
    create_issue_worktree, create_pr_worktree, issue_picker_entries, list_open_issues,
    list_open_prs, pr_picker_entries,
};
use git_ws::picker::{PickerView, pick_candidate, pick_entry};
use git_ws::shell::init_script;
use git_ws::worktree::{CreateWorktreeOptions, create_worktree, find_worktree_for_branch};
use lexopt::prelude::*;

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
            Some("master") => MainTarget::Master,
            _ => MainTarget::Default,
        };
        return cmd_main(target);
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
        "main" => cmd_main(MainTarget::Default),
        "master" => cmd_main(MainTarget::Master),
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
        println!("{}", serde_json::to_string_pretty(&candidates)?);
    } else {
        for candidate in candidates {
            println!(
                "{}\t{}\t{}",
                candidate.availability_label(),
                candidate.name,
                candidate.detail()
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
    create_pr_worktree(&id, pr.branch, pr.run_init)
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
            detail_header: "Detail",
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
            detail_header: "Head",
        },
    )
}

fn cmd_cleanup(args: Vec<OsString>) -> Result<()> {
    let options = parse_cleanup_args(args)?;
    run_cleanup(options)
}

fn cmd_main(target: MainTarget) -> Result<()> {
    let branch = match target {
        MainTarget::Master => "master".to_string(),
        MainTarget::Default => {
            let default = default_branch();
            default
                .strip_prefix("origin/")
                .unwrap_or(default.as_str())
                .to_string()
        }
    };
    if let Some(path) = find_worktree_for_branch(&branch)? {
        emit_cd_path(&path)?;
        return Ok(());
    }
    if current_branch().ok().as_deref() != Some(branch.as_str()) && branch_exists(&branch) {
        git_status(["switch", branch.as_str()])?;
    }
    emit_cd_path(&std::env::current_dir()?)?;
    Ok(())
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
    println!(
        "git-ws: git {}",
        git_ws::git::git_output(["--version"])?.trim()
    );
    match std::process::Command::new("gh").arg("--version").output() {
        Ok(output) if output.status.success() => {
            let first = String::from_utf8_lossy(&output.stdout)
                .lines()
                .next()
                .unwrap_or("gh available")
                .to_string();
            println!("git-ws: {first}");
        }
        _ => println!("git-ws: gh unavailable"),
    }
    Ok(())
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
        git_status([
            "switch",
            "-c",
            candidate.name.as_str(),
            "--track",
            remote.as_str(),
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
}

fn parse_pr_args(args: Vec<OsString>) -> Result<PrArgs> {
    let mut parser = lexopt::Parser::from_args(args);
    let mut id = None;
    let mut branch = None;
    let mut run_init = true;
    while let Some(arg) = parser.next()? {
        match arg {
            Long("branch") => branch = Some(parser.value()?.string()?),
            Long("no-init") => run_init = false,
            Value(value) if id.is_none() => id = Some(value.string()?),
            _ => return Err(arg.unexpected().into()),
        }
    }
    Ok(PrArgs {
        id,
        branch,
        run_init,
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
  git ws pr [number|url] [--branch <name>] [--no-init]
  git ws cleanup [--dry-run] [--yes] [--force] [--json]
  git ws main
  git ws init-shell fish|zsh|bash
  git ws doctor

Run open, issue, or pr without a target to use the interactive fuzzy picker.
Use `git ws open --type remote` to pick from remote branches only.
"#
    );
}

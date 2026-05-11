use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::thread;

use anyhow::{Context, Result, anyhow};
use crossterm::event::{self, Event, KeyCode};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, Row, Table, TableState};
use serde::Serialize;

use crate::git::{
    current_worktree_root, default_branch, git_output, git_output_bytes, git_status,
    list_worktrees, list_worktrees_with_prunable, local_branch_name, prune_worktrees,
};
use crate::path_to_str;
use crate::tui::{
    self, HIGHLIGHT_SYMBOL, NavCommand, Outcome, Tone, TuiTerminal, header_style, label_line,
    panel, row_highlight_style, tone_style,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupInput {
    pub branch: String,
    pub worktree_path: Option<PathBuf>,
    pub is_current_worktree: bool,
    pub is_main_worktree: bool,
    pub is_dirty: bool,
    pub upstream_gone: bool,
    pub merged_to_default: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum CleanupDisposition {
    SafeDelete,
    SkipCurrent,
    SkipMain,
    SkipDirty,
    SkipUnmerged,
}

impl CleanupDisposition {
    pub fn as_str(self) -> &'static str {
        match self {
            CleanupDisposition::SafeDelete => "SafeDelete",
            CleanupDisposition::SkipCurrent => "SkipCurrent",
            CleanupDisposition::SkipMain => "SkipMain",
            CleanupDisposition::SkipDirty => "SkipDirty",
            CleanupDisposition::SkipUnmerged => "SkipUnmerged",
        }
    }
}

pub fn classify_cleanup_candidate(input: &CleanupInput) -> CleanupDisposition {
    if input.is_current_worktree {
        return CleanupDisposition::SkipCurrent;
    }
    if input.is_main_worktree {
        return CleanupDisposition::SkipMain;
    }
    if input.is_dirty {
        return CleanupDisposition::SkipDirty;
    }
    if input.merged_to_default {
        CleanupDisposition::SafeDelete
    } else {
        CleanupDisposition::SkipUnmerged
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CleanupOptions {
    pub dry_run: bool,
    pub yes: bool,
    pub force: bool,
    pub json: bool,
}

pub fn run_cleanup(options: CleanupOptions) -> Result<()> {
    let candidates = discover_cleanup_candidates()?;

    if options.json {
        print_cleanup_json(&candidates, options.force)?;
        return Ok(());
    }

    let safe: Vec<&CleanupInput> = candidates
        .iter()
        .filter(|input| should_cleanup_candidate(input, options.force))
        .collect();

    if safe.is_empty() {
        println!("git-ws: nothing to clean");
        return Ok(());
    }

    if options.dry_run {
        print_cleanup_candidates(&safe, options.force);
        return Ok(());
    }

    let selected = if options.yes {
        print_cleanup_candidates(&safe, options.force);
        (0..safe.len()).collect()
    } else {
        prompt_cleanup_selection(&safe, options.force)?
    };

    delete_cleanup_candidates_with(&safe, &selected, options.force, delete_cleanup_candidate)
}

fn delete_cleanup_candidates_with<F>(
    safe: &[&CleanupInput],
    selected: &[usize],
    force: bool,
    mut delete: F,
) -> Result<()>
where
    F: FnMut(&CleanupInput, bool) -> Result<()>,
{
    for &index in selected {
        delete(safe[index], force)?;
    }
    Ok(())
}

fn should_cleanup_candidate(input: &CleanupInput, force: bool) -> bool {
    eligible_for(classify_cleanup_candidate(input), input, force)
}

fn eligible_for(disposition: CleanupDisposition, input: &CleanupInput, force: bool) -> bool {
    match disposition {
        CleanupDisposition::SafeDelete => true,
        CleanupDisposition::SkipCurrent | CleanupDisposition::SkipMain => false,
        CleanupDisposition::SkipDirty => force && is_stale_or_merged(input),
        CleanupDisposition::SkipUnmerged => force && input.upstream_gone,
    }
}

fn is_stale_or_merged(input: &CleanupInput) -> bool {
    input.upstream_gone || input.merged_to_default
}

pub fn discover_cleanup_candidates() -> Result<Vec<CleanupInput>> {
    let (current_root, worktrees_pair, local, default) = thread::scope(|scope| -> Result<_> {
        let current_root = scope.spawn(current_worktree_root);
        let worktrees = scope.spawn(list_worktrees_with_prunable);
        let local = scope.spawn(local_branches_with_track);
        let default = scope.spawn(default_branch);
        Ok((
            current_root.join().expect("current_worktree_root thread")?,
            worktrees.join().expect("list_worktrees thread")?,
            local.join().expect("local_branches_with_track thread")?,
            default.join().expect("default_branch thread"),
        ))
    })?;
    let (worktrees_initial, prunable_seen) = worktrees_pair;
    let worktrees = if prunable_seen {
        prune_worktrees()?;
        list_worktrees()?
    } else {
        worktrees_initial
    };

    if local.is_empty() {
        return Ok(Vec::new());
    }

    let default = default.ok_or_else(|| {
        anyhow!("default branch could not be determined; set remote HEAD or use main/master")
    })?;
    let default_local = local_branch_name(&default);
    let protected = protected_branches_for_default(default_local);

    let (merged, dirty_paths) = thread::scope(|scope| {
        let merged = scope.spawn(|| merged_branches(&default));
        let dirty = scope.spawn(|| check_worktrees_dirty(&worktrees));
        (
            merged.join().expect("merged_branches thread"),
            dirty.join().expect("check_worktrees_dirty thread"),
        )
    });
    let merged: HashSet<String> = merged?.into_iter().collect();

    let worktree_by_branch: HashMap<&str, &crate::git::Worktree> = worktrees
        .iter()
        .filter_map(|worktree| worktree.branch.as_deref().map(|branch| (branch, worktree)))
        .collect();

    Ok(local
        .into_iter()
        .filter(|(branch, _upstream_gone)| !protected.contains(branch))
        .map(|(branch, upstream_gone)| {
            let branch_worktree = worktree_by_branch.get(branch.as_str()).copied();
            let worktree_path = branch_worktree.map(|worktree| worktree.path.clone());
            let is_current_worktree = worktree_path.as_ref() == Some(&current_root);
            let is_main_worktree = branch_worktree.is_some_and(|worktree| worktree.is_main);
            let is_dirty = worktree_path
                .as_ref()
                .is_some_and(|path| dirty_paths.get(path).copied().unwrap_or(false));
            CleanupInput {
                upstream_gone,
                merged_to_default: merged.contains(&branch),
                branch,
                worktree_path,
                is_current_worktree,
                is_main_worktree,
                is_dirty,
            }
        })
        .collect())
}

fn check_worktrees_dirty(worktrees: &[crate::git::Worktree]) -> HashMap<PathBuf, bool> {
    let paths: Vec<PathBuf> = worktrees
        .iter()
        .map(|worktree| worktree.path.clone())
        .collect();
    thread::scope(|scope| {
        let handles: Vec<_> = paths
            .iter()
            .map(|path| {
                let path = path.clone();
                scope.spawn(move || {
                    let is_dirty = !worktree_clean(&path).unwrap_or(false);
                    (path, is_dirty)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("worktree_clean thread"))
            .collect()
    })
}

fn local_branches_with_track() -> Result<Vec<(String, bool)>> {
    Ok(git_output([
        "for-each-ref",
        "--format=%(refname)%09%(upstream:track)",
        "refs/heads",
    ])?
    .lines()
    .filter_map(|line| {
        let (branch, track) = line.split_once('\t')?;
        let branch = branch.strip_prefix("refs/heads/")?;
        Some((branch.to_string(), track.contains("[gone]")))
    })
    .collect())
}

fn merged_branches(default: &str) -> Result<Vec<String>> {
    let merged = format!("--merged={default}");
    let output = match git_output([
        "for-each-ref",
        "--format=%(refname)",
        merged.as_str(),
        "refs/heads",
    ]) {
        Ok(output) => output,
        Err(_) => return Ok(Vec::new()),
    };
    Ok(output
        .lines()
        .filter_map(|line| line.strip_prefix("refs/heads/").map(str::to_string))
        .filter(|branch| !branch.is_empty())
        .collect())
}

fn protected_branches_for_default(default_local: &str) -> HashSet<String> {
    ["main", "master", "develop", default_local]
        .into_iter()
        .map(ToString::to_string)
        .collect()
}

fn worktree_clean(path: &Path) -> Result<bool> {
    let bytes = git_output_bytes([
        OsStr::new("-C"),
        path.as_os_str(),
        OsStr::new("status"),
        OsStr::new("--porcelain"),
        OsStr::new("--untracked-files=normal"),
    ])
    .with_context(|| format!("failed to inspect {}", path.display()))?;
    Ok(bytes.is_empty())
}

fn print_cleanup_candidates(inputs: &[&CleanupInput], force: bool) {
    if io::stdout().is_terminal() {
        println!("git-ws cleanup candidates");
        println!(
            "{:<4} {:<34} {:<16} {:<18} {:<28} Action",
            "No", "Branch", "Disposition", "Reasons", "Path"
        );
        println!(
            "{:-<4} {:-<34} {:-<16} {:-<18} {:-<28} {:-<1}",
            "", "", "", "", "", ""
        );
        for (index, input) in inputs.iter().enumerate() {
            let disposition = classify_cleanup_candidate(input);
            let tone = tone_for(disposition, requires_force_for(disposition, input));
            println!(
                "{:<4} {:<34} {} {:<18} {:<28} {}",
                index + 1,
                input.branch,
                color_disposition(disposition, tone, 16),
                cleanup_reason(input),
                cleanup_path(input),
                cleanup_action(input, force)
            );
        }
    } else {
        println!("git-ws: cleanup candidates");
        for (index, input) in inputs.iter().enumerate() {
            println!(
                "  {}. {}\t{}\t{}\t{}",
                index + 1,
                input.branch,
                classify_cleanup_candidate(input).as_str(),
                cleanup_reason(input),
                cleanup_action(input, force)
            );
        }
    }
}

fn cleanup_reason(input: &CleanupInput) -> String {
    let values = cleanup_reasons(input);
    if values.is_empty() {
        "-".to_string()
    } else {
        values.join(",")
    }
}

fn cleanup_reasons(input: &CleanupInput) -> Vec<&'static str> {
    let mut values = Vec::new();
    if input.upstream_gone {
        values.push("gone");
    }
    values.push(if input.merged_to_default {
        "merged"
    } else {
        "unmerged"
    });
    if input.is_dirty {
        values.push("dirty");
    }
    values
}

fn cleanup_path(input: &CleanupInput) -> String {
    input
        .worktree_path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "-".to_string())
}

fn prompt_cleanup_selection(inputs: &[&CleanupInput], force: bool) -> Result<Vec<usize>> {
    if !io::stdin().is_terminal() {
        return Err(anyhow!("cleanup requires --yes in non-interactive mode"));
    }
    if tui::is_interactive() {
        return run_cleanup_selector(inputs, force);
    }

    print_cleanup_candidates(inputs, force);
    prompt_cleanup_selection_text(inputs.len())
}

fn prompt_cleanup_selection_text(max: usize) -> Result<Vec<usize>> {
    print!("Delete which candidates? Enter numbers separated by spaces, or 'all': ");
    io::stdout().flush().ok();
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let trimmed = input.trim();
    if trimmed.eq_ignore_ascii_case("all") {
        return Ok((0..max).collect());
    }
    let mut selected = Vec::new();
    for part in trimmed.split(|ch: char| ch.is_ascii_whitespace() || ch == ',') {
        if part.is_empty() {
            continue;
        }
        let number: usize = part
            .parse()
            .with_context(|| format!("invalid selection: {part}"))?;
        if number == 0 || number > max {
            return Err(anyhow!("selection out of range: {number}"));
        }
        selected.push(number - 1);
    }
    Ok(selected)
}

fn run_cleanup_selector(inputs: &[&CleanupInput], force: bool) -> Result<Vec<usize>> {
    let mut terminal = TuiTerminal::new()?;
    let mut state = CleanupSelectorState::new(inputs, force);

    loop {
        if !event::poll(std::time::Duration::ZERO).unwrap_or(false) {
            terminal.draw(|frame| render_cleanup_selector(frame, &state))?;
        }
        loop {
            match event::read()? {
                Event::Key(key) => {
                    let command = cleanup_command(key);
                    match state.apply(command) {
                        Outcome::Continue => {
                            if matches!(command, CleanupCommand::Ignore) {
                                continue;
                            }
                            break;
                        }
                        Outcome::Cancel => return Ok(Vec::new()),
                        Outcome::Submit => return Ok(state.selected_indices()),
                    }
                }
                Event::Resize(..) => break,
                _ => {}
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CleanupRow {
    branch: String,
    disposition: &'static str,
    reason: String,
    path: String,
    action: String,
    tone: Tone,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CleanupSelectorState {
    selected: usize,
    checked: Vec<bool>,
    rows: Vec<CleanupRow>,
}

impl CleanupSelectorState {
    fn new(inputs: &[&CleanupInput], force: bool) -> Self {
        let rows: Vec<CleanupRow> = inputs
            .iter()
            .map(|input| {
                let disposition = classify_cleanup_candidate(input);
                let requires_force = requires_force_for(disposition, input);
                CleanupRow {
                    branch: input.branch.clone(),
                    disposition: disposition.as_str(),
                    reason: cleanup_reason(input),
                    path: cleanup_path(input),
                    action: cleanup_action(input, force),
                    tone: tone_for(disposition, requires_force),
                }
            })
            .collect();
        Self {
            selected: 0,
            checked: vec![false; rows.len()],
            rows,
        }
    }

    fn len(&self) -> usize {
        self.rows.len()
    }

    fn selected_count(&self) -> usize {
        self.checked.iter().filter(|&&value| value).count()
    }

    fn apply(&mut self, command: CleanupCommand) -> Outcome {
        match command {
            CleanupCommand::Cancel => Outcome::Cancel,
            CleanupCommand::Submit => Outcome::Submit,
            CleanupCommand::Up => {
                self.selected = self.selected.saturating_sub(1);
                Outcome::Continue
            }
            CleanupCommand::Down => {
                if self.selected + 1 < self.len() {
                    self.selected += 1;
                }
                Outcome::Continue
            }
            CleanupCommand::Toggle => {
                if let Some(value) = self.checked.get_mut(self.selected) {
                    *value = !*value;
                }
                Outcome::Continue
            }
            CleanupCommand::ToggleAll => {
                let all_checked = self.checked.iter().all(|&value| value);
                for value in &mut self.checked {
                    *value = !all_checked;
                }
                Outcome::Continue
            }
            CleanupCommand::Ignore => Outcome::Continue,
        }
    }

    fn selected_indices(&self) -> Vec<usize> {
        self.checked
            .iter()
            .enumerate()
            .filter_map(|(index, checked)| checked.then_some(index))
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CleanupCommand {
    Up,
    Down,
    Toggle,
    ToggleAll,
    Submit,
    Cancel,
    Ignore,
}

fn cleanup_command(key: event::KeyEvent) -> CleanupCommand {
    if let Some(nav) = tui::nav_command(key) {
        return match nav {
            NavCommand::Up => CleanupCommand::Up,
            NavCommand::Down => CleanupCommand::Down,
            NavCommand::Submit => CleanupCommand::Submit,
            NavCommand::Cancel => CleanupCommand::Cancel,
        };
    }
    match key.code {
        KeyCode::Char(' ') => CleanupCommand::Toggle,
        KeyCode::Char('a' | 'A') => CleanupCommand::ToggleAll,
        _ => CleanupCommand::Ignore,
    }
}

fn render_cleanup_selector(frame: &mut Frame<'_>, state: &CleanupSelectorState) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(7),
            Constraint::Length(1),
        ])
        .split(area);

    let title = Line::from(vec![
        Span::styled("git ws cleanup", Style::default().fg(Color::Cyan)),
        Span::raw(format!(
            "  {} candidate(s), {} selected",
            state.len(),
            state.selected_count()
        )),
    ]);
    frame.render_widget(Paragraph::new(title).block(panel(" cleanup ")), chunks[0]);

    let header = Row::new([
        Cell::from("Del"),
        Cell::from("Branch"),
        Cell::from("Disposition"),
        Cell::from("Reasons"),
        Cell::from("Path"),
        Cell::from("Action"),
    ])
    .style(header_style());
    let rows = state.rows.iter().enumerate().map(|(index, row)| {
        let checkbox = if state.checked.get(index).copied().unwrap_or(false) {
            "[x]"
        } else {
            "[ ]"
        };
        Row::new([
            Cell::from(checkbox),
            Cell::from(row.branch.as_str()).style(tone_style(row.tone)),
            Cell::from(row.disposition).style(tone_style(row.tone)),
            Cell::from(row.reason.as_str()).style(tone_style(row.tone)),
            Cell::from(row.path.as_str()).style(tone_style(if row.path == "-" {
                Tone::Dim
            } else {
                Tone::Worktree
            })),
            Cell::from(row.action.as_str()).style(tone_style(Tone::Info)),
        ])
    });
    let table = Table::new(
        rows,
        [
            Constraint::Length(5),
            Constraint::Percentage(27),
            Constraint::Length(16),
            Constraint::Length(18),
            Constraint::Percentage(24),
            Constraint::Min(18),
        ],
    )
    .header(header)
    .block(panel(" candidates "))
    .row_highlight_style(row_highlight_style())
    .highlight_symbol(HIGHLIGHT_SYMBOL);
    let mut table_state = TableState::default();
    table_state.select(Some(state.selected));
    frame.render_stateful_widget(table, chunks[1], &mut table_state);

    let detail = state
        .rows
        .get(state.selected)
        .map(|row| {
            vec![
                label_line("Branch  ", row.branch.as_str()),
                label_line("State   ", row.disposition),
                label_line("Reason  ", row.reason.as_str()),
                label_line("Path    ", row.path.as_str()),
                label_line("Action  ", row.action.as_str()),
            ]
        })
        .unwrap_or_else(|| vec![Line::from("No candidate")]);
    frame.render_widget(Paragraph::new(detail).block(panel(" detail ")), chunks[2]);

    frame.render_widget(
        Paragraph::new("space toggle  a all  ↑/↓ ctrl+n/p move  enter delete  esc cancel")
            .style(Style::default().fg(Color::DarkGray)),
        chunks[3],
    );
}

fn delete_cleanup_candidate(input: &CleanupInput, force: bool) -> Result<()> {
    if !force && !input.merged_to_default {
        return Err(anyhow!(
            "refusing to delete unmerged branch without --force: {}",
            input.branch
        ));
    }
    if let Some(path) = &input.worktree_path {
        let mut args: Vec<&str> = vec!["worktree", "remove"];
        if force {
            args.push("--force");
        }
        args.push(path_to_str(path)?);
        git_status(args)?;
    }
    // `git branch -d` re-checks against HEAD, so use -D only after the
    // default-merge or force validation above.
    git_status(["branch", "-D", input.branch.as_str()])
}

#[derive(Serialize)]
struct CleanupRecord<'a> {
    branch: &'a str,
    #[serde(rename = "worktreePath")]
    worktree_path: Option<&'a Path>,
    disposition: CleanupDisposition,
    #[serde(rename = "upstreamGone")]
    upstream_gone: bool,
    #[serde(rename = "mergedToDefault")]
    merged_to_default: bool,
    dirty: bool,
    current: bool,
    #[serde(rename = "mainWorktree")]
    main_worktree: bool,
    reasons: Vec<&'static str>,
    eligible: bool,
    #[serde(rename = "requiresForce")]
    requires_force: bool,
    action: String,
}

fn print_cleanup_json(inputs: &[CleanupInput], force: bool) -> Result<()> {
    let values: Vec<CleanupRecord> = inputs
        .iter()
        .map(|input| {
            let disposition = classify_cleanup_candidate(input);
            let eligible_no_force = eligible_for(disposition, input, false);
            let eligible_with_force = eligible_for(disposition, input, true);
            let eligible = if force {
                eligible_with_force
            } else {
                eligible_no_force
            };
            CleanupRecord {
                branch: input.branch.as_str(),
                worktree_path: input.worktree_path.as_deref(),
                disposition,
                upstream_gone: input.upstream_gone,
                merged_to_default: input.merged_to_default,
                dirty: input.is_dirty,
                current: input.is_current_worktree,
                main_worktree: input.is_main_worktree,
                reasons: cleanup_reasons(input),
                eligible,
                requires_force: !eligible_no_force && eligible_with_force,
                action: if eligible {
                    cleanup_action(input, force)
                } else {
                    "skip".to_string()
                },
            }
        })
        .collect();
    println!("{}", serde_json::to_string_pretty(&values)?);
    Ok(())
}

fn requires_force_for(disposition: CleanupDisposition, input: &CleanupInput) -> bool {
    !eligible_for(disposition, input, false) && eligible_for(disposition, input, true)
}

fn cleanup_action(input: &CleanupInput, force: bool) -> String {
    let branch_delete = format!("git branch -D {}", input.branch);
    if let Some(path) = &input.worktree_path {
        let remove = if force {
            format!("git worktree remove --force {}", path.display())
        } else {
            format!("git worktree remove {}", path.display())
        };
        format!("{remove} && {branch_delete}")
    } else {
        branch_delete
    }
}

fn tone_for(disposition: CleanupDisposition, requires_force: bool) -> Tone {
    match disposition {
        CleanupDisposition::SafeDelete => Tone::Good,
        CleanupDisposition::SkipDirty => Tone::Dirty,
        CleanupDisposition::SkipUnmerged if requires_force => Tone::Warning,
        CleanupDisposition::SkipCurrent
        | CleanupDisposition::SkipMain
        | CleanupDisposition::SkipUnmerged => Tone::Bad,
    }
}

fn color_disposition(disposition: CleanupDisposition, tone: Tone, width: usize) -> String {
    let code = match tone {
        Tone::Good => 32,
        Tone::Warning => 33,
        Tone::Dirty => 35,
        Tone::Bad => 31,
        _ => 2,
    };
    format!("\x1b[{code}m{:<width$}\x1b[0m", disposition.as_str())
}

#[cfg(test)]
mod tests {
    use crossterm::event::KeyModifiers;
    use insta::assert_snapshot;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    #[test]
    fn cleanup_selector_toggles_one_and_all_candidates() {
        let inputs = sample_inputs(3);
        let input_refs: Vec<_> = inputs.iter().collect();
        let mut state = CleanupSelectorState::new(&input_refs, false);

        assert_eq!(state.apply(CleanupCommand::Toggle), Outcome::Continue);
        assert_eq!(state.apply(CleanupCommand::Down), Outcome::Continue);
        assert_eq!(state.apply(CleanupCommand::ToggleAll), Outcome::Continue);

        assert_eq!(state.selected, 1);
        assert_eq!(state.selected_count(), 3);
        assert_eq!(state.selected_indices(), vec![0, 1, 2]);
    }

    #[test]
    fn cleanup_command_maps_control_n_and_p_to_movement() {
        assert_eq!(
            cleanup_command(event::KeyEvent::new(
                KeyCode::Char('n'),
                KeyModifiers::CONTROL,
            )),
            CleanupCommand::Down
        );
        assert_eq!(
            cleanup_command(event::KeyEvent::new(
                KeyCode::Char('p'),
                KeyModifiers::CONTROL,
            )),
            CleanupCommand::Up
        );
    }

    #[test]
    fn cleanup_deletes_selected_candidates_in_selection_order() {
        let inputs = sample_inputs(3);
        let input_refs: Vec<_> = inputs.iter().collect();
        let mut deleted = Vec::new();

        delete_cleanup_candidates_with(&input_refs, &[2, 0], false, |input, force| {
            assert!(!force);
            deleted.push(input.branch.clone());
            Ok(())
        })
        .expect("delete selected cleanup candidates");

        assert_eq!(deleted, ["branch-2", "branch-0"]);
    }

    #[test]
    fn renders_cleanup_selector_snapshot() {
        let inputs = [
            CleanupInput {
                branch: "feature/default-merged".to_string(),
                worktree_path: Some(PathBuf::from("/repo/.worktrees/default-merged")),
                is_current_worktree: false,
                is_main_worktree: false,
                is_dirty: false,
                upstream_gone: false,
                merged_to_default: true,
            },
            CleanupInput {
                branch: "feature/gone".to_string(),
                worktree_path: None,
                is_current_worktree: false,
                is_main_worktree: false,
                is_dirty: false,
                upstream_gone: true,
                merged_to_default: false,
            },
        ];
        let input_refs: Vec<_> = inputs.iter().collect();
        let mut state = CleanupSelectorState::new(&input_refs, false);
        state.apply(CleanupCommand::Toggle);
        let mut terminal = Terminal::new(TestBackend::new(150, 20)).expect("terminal");

        terminal
            .draw(|frame| render_cleanup_selector(frame, &state))
            .expect("draw");

        assert_snapshot!(terminal.backend());
    }

    fn sample_inputs(count: usize) -> Vec<CleanupInput> {
        (0..count)
            .map(|index| CleanupInput {
                branch: format!("branch-{index}"),
                worktree_path: None,
                is_current_worktree: false,
                is_main_worktree: false,
                is_dirty: false,
                upstream_gone: true,
                merged_to_default: false,
            })
            .collect()
    }
}

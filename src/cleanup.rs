use std::collections::{HashMap, HashSet};
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;

use anyhow::{Context, Result, anyhow};
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState};
use serde::Serialize;

use crate::git::{current_worktree_root, default_branch, git_output, git_status, list_worktrees};
use crate::path_to_str;
use crate::tui::TuiTerminal;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupInput {
    pub branch: String,
    pub worktree_path: Option<PathBuf>,
    pub is_current_worktree: bool,
    pub is_dirty: bool,
    pub upstream_gone: bool,
    pub merged_to_default: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum CleanupDisposition {
    SafeDelete,
    SkipCurrent,
    SkipDirty,
    SkipUnmerged,
}

pub fn classify_cleanup_candidate(input: &CleanupInput) -> CleanupDisposition {
    if input.is_current_worktree {
        return CleanupDisposition::SkipCurrent;
    }
    if input.is_dirty {
        return CleanupDisposition::SkipDirty;
    }
    if input.upstream_gone || input.merged_to_default {
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
        print_cleanup_json(&candidates)?;
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
        print_cleanup_candidates(&safe);
        return Ok(());
    }

    let selected = if options.yes {
        print_cleanup_candidates(&safe);
        (0..safe.len()).collect()
    } else {
        prompt_cleanup_selection(&safe)?
    };

    for index in selected {
        delete_cleanup_candidate(safe[index], options.force)?;
    }

    Ok(())
}

fn should_cleanup_candidate(input: &CleanupInput, force: bool) -> bool {
    match classify_cleanup_candidate(input) {
        CleanupDisposition::SafeDelete => true,
        CleanupDisposition::SkipCurrent => false,
        CleanupDisposition::SkipDirty => force && is_stale_or_merged(input),
        CleanupDisposition::SkipUnmerged => false,
    }
}

fn is_stale_or_merged(input: &CleanupInput) -> bool {
    input.upstream_gone || input.merged_to_default
}

pub fn discover_cleanup_candidates() -> Result<Vec<CleanupInput>> {
    let (current_root, worktrees, local) = thread::scope(|scope| -> Result<_> {
        let current_root = scope.spawn(current_worktree_root);
        let worktrees = scope.spawn(list_worktrees);
        let local = scope.spawn(local_branches_with_track);
        Ok((
            current_root.join().expect("current_worktree_root thread")?,
            worktrees.join().expect("list_worktrees thread")?,
            local.join().expect("local_branches_with_track thread")?,
        ))
    })?;

    if local.is_empty() {
        return Ok(Vec::new());
    }

    let default = default_branch().ok_or_else(|| {
        anyhow!("default branch could not be determined; set origin/HEAD or use main/master")
    })?;
    let default_local = default
        .strip_prefix("origin/")
        .unwrap_or(&default)
        .to_string();
    let protected = protected_branches_for_default(&default_local);
    let merged = merged_branches(&default)?;
    let merged: HashSet<String> = merged.into_iter().collect();

    let dirty_paths = check_worktrees_dirty(&worktrees);

    Ok(local
        .into_iter()
        .filter(|(branch, _upstream_gone)| !protected.contains(branch))
        .map(|(branch, upstream_gone)| {
            let worktree_path = worktrees
                .iter()
                .find(|worktree| worktree.branch.as_deref() == Some(branch.as_str()))
                .map(|worktree| worktree.path.clone());
            let is_current_worktree = worktree_path.as_ref() == Some(&current_root);
            let is_dirty = worktree_path
                .as_ref()
                .is_some_and(|path| dirty_paths.get(path).copied().unwrap_or(false));
            CleanupInput {
                upstream_gone,
                merged_to_default: merged.contains(&branch),
                branch,
                worktree_path,
                is_current_worktree,
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
        "--format=%(refname:short)%09%(upstream:track)",
        "refs/heads",
    ])?
    .lines()
    .filter_map(|line| {
        let (branch, track) = line.split_once('\t')?;
        Some((branch.to_string(), track.contains("[gone]")))
    })
    .collect())
}

fn merged_branches(default: &str) -> Result<Vec<String>> {
    let output = match git_output(["branch", "--format=%(refname:short)", "--merged", default]) {
        Ok(output) => output,
        Err(_) => return Ok(Vec::new()),
    };
    Ok(output
        .lines()
        .map(|line| line.trim_start_matches('*').trim().to_string())
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
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["status", "--porcelain", "--untracked-files=normal"])
        .output()
        .with_context(|| format!("failed to inspect {}", path.display()))?;
    Ok(output.status.success() && output.stdout.is_empty())
}

fn print_cleanup_candidates(inputs: &[&CleanupInput]) {
    if io::stdout().is_terminal() {
        println!("git-ws cleanup candidates");
        println!("{:<4} {:<40} {:<18} Path", "No", "Branch", "Reason");
        println!("{:-<4} {:-<40} {:-<18} {:-<1}", "", "", "", "");
        for (index, input) in inputs.iter().enumerate() {
            println!(
                "{:<4} {:<40} {:<18} {}",
                index + 1,
                input.branch,
                cleanup_reason(input),
                cleanup_path(input)
            );
        }
    } else {
        println!("git-ws: cleanup candidates");
        for (index, input) in inputs.iter().enumerate() {
            println!("  {}. {} {}", index + 1, input.branch, cleanup_path(input));
        }
    }
}

fn cleanup_reason(input: &CleanupInput) -> String {
    let mut values = Vec::new();
    if input.upstream_gone {
        values.push("gone");
    }
    if input.merged_to_default {
        values.push("merged");
    }
    if input.is_dirty {
        values.push("dirty");
    }
    if values.is_empty() {
        "-".to_string()
    } else {
        values.join(",")
    }
}

fn cleanup_path(input: &CleanupInput) -> String {
    input
        .worktree_path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "-".to_string())
}

fn prompt_cleanup_selection(inputs: &[&CleanupInput]) -> Result<Vec<usize>> {
    if !io::stdin().is_terminal() {
        return Err(anyhow!("cleanup requires --yes in non-interactive mode"));
    }
    if io::stderr().is_terminal() {
        return run_cleanup_selector(inputs);
    }

    print_cleanup_candidates(inputs);
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

fn run_cleanup_selector(inputs: &[&CleanupInput]) -> Result<Vec<usize>> {
    let mut terminal = TuiTerminal::new()?;
    let mut state = CleanupSelectorState::new(inputs.len());

    loop {
        terminal.draw(|frame| render_cleanup_selector(frame, &state, inputs))?;
        let Event::Key(key) = event::read()? else {
            continue;
        };
        match state.apply(cleanup_command(key), inputs.len()) {
            CleanupOutcome::Continue => {}
            CleanupOutcome::Cancel => return Ok(Vec::new()),
            CleanupOutcome::Submit => return Ok(state.selected_indices()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CleanupSelectorState {
    selected: usize,
    checked: Vec<bool>,
}

impl CleanupSelectorState {
    fn new(len: usize) -> Self {
        Self {
            selected: 0,
            checked: vec![false; len],
        }
    }

    fn apply(&mut self, command: CleanupCommand, len: usize) -> CleanupOutcome {
        match command {
            CleanupCommand::Cancel => CleanupOutcome::Cancel,
            CleanupCommand::Submit => CleanupOutcome::Submit,
            CleanupCommand::Up => {
                self.selected = self.selected.saturating_sub(1);
                CleanupOutcome::Continue
            }
            CleanupCommand::Down => {
                if self.selected + 1 < len {
                    self.selected += 1;
                }
                CleanupOutcome::Continue
            }
            CleanupCommand::Toggle => {
                if let Some(value) = self.checked.get_mut(self.selected) {
                    *value = !*value;
                }
                CleanupOutcome::Continue
            }
            CleanupCommand::ToggleAll => {
                let all_checked = self.checked.iter().all(|value| *value);
                for value in &mut self.checked {
                    *value = !all_checked;
                }
                CleanupOutcome::Continue
            }
            CleanupCommand::Ignore => CleanupOutcome::Continue,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CleanupOutcome {
    Continue,
    Submit,
    Cancel,
}

fn cleanup_command(key: event::KeyEvent) -> CleanupCommand {
    match key.code {
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            CleanupCommand::Cancel
        }
        KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => CleanupCommand::Down,
        KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => CleanupCommand::Up,
        KeyCode::Esc => CleanupCommand::Cancel,
        KeyCode::Enter => CleanupCommand::Submit,
        KeyCode::Up => CleanupCommand::Up,
        KeyCode::Down => CleanupCommand::Down,
        KeyCode::Char(' ') => CleanupCommand::Toggle,
        KeyCode::Char('a' | 'A') => CleanupCommand::ToggleAll,
        _ => CleanupCommand::Ignore,
    }
}

fn render_cleanup_selector(
    frame: &mut Frame<'_>,
    state: &CleanupSelectorState,
    inputs: &[&CleanupInput],
) {
    let area = frame.area();
    let selected_count = state.checked.iter().filter(|checked| **checked).count();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(4),
            Constraint::Length(1),
        ])
        .split(area);

    let title = Line::from(vec![
        Span::styled("git ws cleanup", Style::default().fg(Color::Cyan)),
        Span::raw(format!(
            "  {} candidate(s), {} selected",
            inputs.len(),
            selected_count
        )),
    ]);
    frame.render_widget(
        Paragraph::new(title).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(" cleanup "),
        ),
        chunks[0],
    );

    let header = Row::new([
        Cell::from("Del"),
        Cell::from("Branch"),
        Cell::from("Reason"),
        Cell::from("Path"),
    ])
    .style(
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    );
    let rows = inputs.iter().enumerate().map(|(index, input)| {
        let checkbox = if state.checked[index] { "[x]" } else { "[ ]" };
        Row::new([
            Cell::from(checkbox),
            Cell::from(input.branch.clone()),
            Cell::from(cleanup_reason(input)),
            Cell::from(cleanup_path(input)),
        ])
    });
    let table = Table::new(
        rows,
        [
            Constraint::Length(5),
            Constraint::Percentage(36),
            Constraint::Length(18),
            Constraint::Min(20),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(" candidates "),
    )
    .row_highlight_style(
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )
    .highlight_symbol("› ");
    let mut table_state = TableState::default();
    table_state.select(Some(state.selected));
    frame.render_stateful_widget(table, chunks[1], &mut table_state);

    let detail = inputs
        .get(state.selected)
        .map(|input| {
            vec![
                Line::from(vec![
                    Span::styled("Branch  ", Style::default().fg(Color::DarkGray)),
                    Span::raw(input.branch.as_str()),
                ]),
                Line::from(vec![
                    Span::styled("Reason  ", Style::default().fg(Color::DarkGray)),
                    Span::raw(cleanup_reason(input)),
                ]),
            ]
        })
        .unwrap_or_else(|| vec![Line::from("No candidate")]);
    frame.render_widget(
        Paragraph::new(detail).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(" detail "),
        ),
        chunks[2],
    );

    frame.render_widget(
        Paragraph::new("space toggle  a all  ↑/↓ ctrl+n/p move  enter delete  esc cancel")
            .style(Style::default().fg(Color::DarkGray)),
        chunks[3],
    );
}

fn delete_cleanup_candidate(input: &CleanupInput, force: bool) -> Result<()> {
    if input.worktree_path.is_some() && !force && !input.merged_to_default {
        return Err(anyhow!(
            "refusing to remove worktree for unmerged branch without --force: {}",
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
    // `git branch -d` falls back to HEAD when no upstream is set.
    // Candidates merged to default were already checked against that default ref.
    let flag = if force || input.merged_to_default {
        "-D"
    } else {
        "-d"
    };
    git_status(["branch", flag, input.branch.as_str()])
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
}

fn print_cleanup_json(inputs: &[CleanupInput]) -> Result<()> {
    let values: Vec<CleanupRecord> = inputs
        .iter()
        .map(|input| CleanupRecord {
            branch: input.branch.as_str(),
            worktree_path: input.worktree_path.as_deref(),
            disposition: classify_cleanup_candidate(input),
            upstream_gone: input.upstream_gone,
            merged_to_default: input.merged_to_default,
            dirty: input.is_dirty,
            current: input.is_current_worktree,
        })
        .collect();
    println!("{}", serde_json::to_string_pretty(&values)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use insta::assert_snapshot;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    #[test]
    fn cleanup_selector_toggles_one_and_all_candidates() {
        let mut state = CleanupSelectorState::new(3);

        assert_eq!(
            state.apply(CleanupCommand::Toggle, 3),
            CleanupOutcome::Continue
        );
        assert_eq!(
            state.apply(CleanupCommand::Down, 3),
            CleanupOutcome::Continue
        );
        assert_eq!(
            state.apply(CleanupCommand::ToggleAll, 3),
            CleanupOutcome::Continue
        );

        assert_eq!(state.selected, 1);
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
    fn renders_cleanup_selector_snapshot() {
        let inputs = [
            CleanupInput {
                branch: "feature/default-merged".to_string(),
                worktree_path: Some(PathBuf::from("/repo/.worktrees/default-merged")),
                is_current_worktree: false,
                is_dirty: false,
                upstream_gone: false,
                merged_to_default: true,
            },
            CleanupInput {
                branch: "feature/gone".to_string(),
                worktree_path: None,
                is_current_worktree: false,
                is_dirty: false,
                upstream_gone: true,
                merged_to_default: false,
            },
        ];
        let input_refs: Vec<_> = inputs.iter().collect();
        let mut state = CleanupSelectorState::new(input_refs.len());
        state.apply(CleanupCommand::Toggle, input_refs.len());
        let mut terminal = Terminal::new(TestBackend::new(96, 18)).expect("terminal");

        terminal
            .draw(|frame| render_cleanup_selector(frame, &state, &input_refs))
            .expect("draw");

        assert_snapshot!(terminal.backend());
    }
}

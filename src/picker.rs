use std::io::{self, IsTerminal};

use anyhow::{Result, anyhow};
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState, Wrap};

use crate::candidates::Candidate;
use crate::tui::TuiTerminal;

const VISIBLE_ROWS: usize = 15;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickerEntry<T> {
    pub value: T,
    pub marker: String,
    pub name: String,
    pub detail: String,
    pub action: String,
    pub search_text: String,
}

#[derive(Debug, Clone, Copy)]
pub struct PickerView<'a> {
    pub prompt: &'a str,
    pub marker_header: &'a str,
    pub name_header: &'a str,
    pub detail_header: &'a str,
}

pub fn pick_candidate(
    candidates: &[Candidate],
    initial_query: Option<&str>,
) -> Result<Option<Candidate>> {
    let entries: Vec<PickerEntry<Candidate>> =
        candidates.iter().cloned().map(candidate_entry).collect();
    pick_entry(
        &entries,
        initial_query,
        PickerView {
            prompt: "git ws>",
            marker_header: "Avail",
            name_header: "Name",
            detail_header: "Detail",
        },
    )
}

pub fn pick_entry<T: Clone>(
    entries: &[PickerEntry<T>],
    initial_query: Option<&str>,
    view: PickerView<'_>,
) -> Result<Option<T>> {
    if entries.is_empty() {
        return Ok(None);
    }

    if let Some(query) = initial_query {
        return Ok(rank_entries(query, entries)
            .first()
            .map(|entry| entry.value.clone()));
    }

    if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
        return Err(anyhow!(
            "interactive picker requires a terminal or query argument"
        ));
    }

    run_picker(entries, view)
}

pub fn rank_entries<'a, T>(query: &str, entries: &'a [PickerEntry<T>]) -> Vec<&'a PickerEntry<T>> {
    if query.trim().is_empty() {
        return entries.iter().collect();
    }

    let pattern = Pattern::new(
        query,
        CaseMatching::Smart,
        Normalization::Smart,
        AtomKind::Fuzzy,
    );
    let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
    let mut utf32_buffer = Vec::new();
    let mut scored: Vec<(usize, u32)> = entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            pattern
                .score(
                    Utf32Str::new(entry.search_text.as_str(), &mut utf32_buffer),
                    &mut matcher,
                )
                .map(|score| (index, score))
        })
        .collect();
    scored.sort_by_key(|(_, score)| std::cmp::Reverse(*score));
    scored
        .into_iter()
        .map(|(index, _score)| &entries[index])
        .collect()
}

fn run_picker<T: Clone>(entries: &[PickerEntry<T>], view: PickerView<'_>) -> Result<Option<T>> {
    let mut terminal = TuiTerminal::new()?;
    let mut state = PickerState::default();

    loop {
        let ranked = rank_entries(&state.query, entries);
        state.clamp_selection(ranked.len());
        terminal.draw(|frame| render_picker(frame, &state, &ranked, view))?;

        let Event::Key(key) = event::read()? else {
            continue;
        };
        match state.apply(picker_command(key), ranked.len()) {
            PickerOutcome::Continue => {}
            PickerOutcome::Cancel => return Ok(None),
            PickerOutcome::Submit => {
                return Ok(ranked.get(state.selected).map(|entry| entry.value.clone()));
            }
        }
    }
}

#[derive(Debug, Default)]
struct PickerState {
    query: String,
    selected: usize,
}

impl PickerState {
    fn apply(&mut self, command: PickerCommand, visible_len: usize) -> PickerOutcome {
        match command {
            PickerCommand::Cancel => PickerOutcome::Cancel,
            PickerCommand::Submit => PickerOutcome::Submit,
            PickerCommand::Up => {
                self.selected = self.selected.saturating_sub(1);
                PickerOutcome::Continue
            }
            PickerCommand::Down => {
                if self.selected + 1 < visible_len {
                    self.selected += 1;
                }
                PickerOutcome::Continue
            }
            PickerCommand::Backspace => {
                self.query.pop();
                self.selected = 0;
                PickerOutcome::Continue
            }
            PickerCommand::Insert(ch) => {
                self.query.push(ch);
                self.selected = 0;
                PickerOutcome::Continue
            }
            PickerCommand::Ignore => PickerOutcome::Continue,
        }
    }

    fn clamp_selection(&mut self, visible_len: usize) {
        if visible_len == 0 {
            self.selected = 0;
        } else if self.selected >= visible_len {
            self.selected = visible_len - 1;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PickerCommand {
    Up,
    Down,
    Backspace,
    Insert(char),
    Submit,
    Cancel,
    Ignore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PickerOutcome {
    Continue,
    Submit,
    Cancel,
}

fn picker_command(key: event::KeyEvent) -> PickerCommand {
    match key.code {
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            PickerCommand::Cancel
        }
        KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => PickerCommand::Down,
        KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => PickerCommand::Up,
        KeyCode::Esc => PickerCommand::Cancel,
        KeyCode::Enter => PickerCommand::Submit,
        KeyCode::Up => PickerCommand::Up,
        KeyCode::Down => PickerCommand::Down,
        KeyCode::Backspace => PickerCommand::Backspace,
        KeyCode::Char(ch) => PickerCommand::Insert(ch),
        _ => PickerCommand::Ignore,
    }
}

fn render_picker<T>(
    frame: &mut Frame<'_>,
    state: &PickerState,
    ranked: &[&PickerEntry<T>],
    view: PickerView<'_>,
) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(6),
            Constraint::Length(5),
            Constraint::Length(1),
        ])
        .split(area);

    let title = Line::from(vec![
        Span::styled(view.prompt, Style::default().fg(Color::Cyan)),
        Span::raw(" "),
        Span::styled(&state.query, Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(format!("  {} match(es)", ranked.len())),
    ]);
    frame.render_widget(
        Paragraph::new(title).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(" search "),
        ),
        chunks[0],
    );

    if ranked.is_empty() {
        frame.render_widget(
            Paragraph::new("No matches")
                .style(Style::default().fg(Color::DarkGray))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::DarkGray))
                        .title(" candidates "),
                ),
            chunks[1],
        );
    } else {
        let header = Row::new([
            Cell::from(view.marker_header),
            Cell::from(view.name_header),
            Cell::from(view.detail_header),
        ])
        .style(
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        );
        let rows = ranked.iter().take(VISIBLE_ROWS).map(|entry| {
            Row::new([
                Cell::from(entry.marker.clone()),
                Cell::from(entry.name.clone()),
                Cell::from(entry.detail.clone()),
            ])
        });
        let table = Table::new(
            rows,
            [
                Constraint::Length(12),
                Constraint::Percentage(44),
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
        table_state.select(Some(state.selected.min(VISIBLE_ROWS - 1)));
        frame.render_stateful_widget(table, chunks[1], &mut table_state);
    }

    let detail = ranked
        .get(state.selected)
        .map(|entry| {
            vec![
                Line::from(vec![
                    Span::styled("Selected ", Style::default().fg(Color::DarkGray)),
                    Span::styled(&entry.name, Style::default().fg(Color::White)),
                ]),
                Line::from(vec![
                    Span::styled("Detail   ", Style::default().fg(Color::DarkGray)),
                    Span::raw(&entry.detail),
                ]),
                Line::from(vec![
                    Span::styled("Action   ", Style::default().fg(Color::DarkGray)),
                    Span::raw(&entry.action),
                ]),
            ]
        })
        .unwrap_or_else(|| vec![Line::from("No selectable candidate")]);
    frame.render_widget(
        Paragraph::new(detail).wrap(Wrap { trim: true }).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(" detail "),
        ),
        chunks[2],
    );

    frame.render_widget(
        Paragraph::new("type to filter  ↑/↓ ctrl+n/p move  enter select  esc cancel")
            .style(Style::default().fg(Color::DarkGray)),
        chunks[3],
    );
}

fn candidate_entry(candidate: Candidate) -> PickerEntry<Candidate> {
    let name = candidate.name.clone();
    PickerEntry {
        marker: candidate.availability_label(),
        detail: candidate.detail(),
        action: action_label(&candidate),
        search_text: name.clone(),
        name,
        value: candidate,
    }
}

fn action_label(candidate: &Candidate) -> String {
    if let Some(path) = &candidate.worktree_path {
        format!("cd {}", path.display())
    } else if let Some(local) = &candidate.local_ref {
        format!("git switch {local}")
    } else if let Some(remote) = &candidate.remote_ref {
        format!("git switch -c {} --track {remote}", candidate.name)
    } else {
        "unavailable".to_string()
    }
}

#[cfg(test)]
mod tests {
    use insta::assert_snapshot;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    #[test]
    fn picker_state_updates_query_and_selection() {
        let mut state = PickerState::default();

        assert_eq!(
            state.apply(PickerCommand::Insert('p'), 3),
            PickerOutcome::Continue
        );
        assert_eq!(state.apply(PickerCommand::Down, 3), PickerOutcome::Continue);
        assert_eq!(
            state.apply(PickerCommand::Backspace, 3),
            PickerOutcome::Continue
        );

        assert_eq!(state.query, "");
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn picker_state_clamps_selection_to_visible_entries() {
        let mut state = PickerState {
            query: String::new(),
            selected: 5,
        };

        state.clamp_selection(2);

        assert_eq!(state.selected, 1);
    }

    #[test]
    fn picker_command_maps_control_n_and_p_to_movement() {
        assert_eq!(
            picker_command(event::KeyEvent::new(
                KeyCode::Char('n'),
                KeyModifiers::CONTROL,
            )),
            PickerCommand::Down
        );
        assert_eq!(
            picker_command(event::KeyEvent::new(
                KeyCode::Char('p'),
                KeyModifiers::CONTROL,
            )),
            PickerCommand::Up
        );
        assert_eq!(
            picker_command(event::KeyEvent::new(
                KeyCode::Char('c'),
                KeyModifiers::CONTROL,
            )),
            PickerCommand::Cancel
        );
    }

    #[test]
    fn renders_picker_snapshot() {
        let entries = [
            PickerEntry {
                value: "1",
                marker: "#1".to_string(),
                name: "feat: add git-ws CLI".to_string(),
                detail: "feat/implement-git-ws-cli".to_string(),
                action: "create worktree for PR #1".to_string(),
                search_text: "#1 feat: add git-ws CLI feat/implement-git-ws-cli".to_string(),
            },
            PickerEntry {
                value: "2",
                marker: "#2".to_string(),
                name: "fix: cleanup default branch".to_string(),
                detail: "fix/cleanup-default".to_string(),
                action: "create worktree for PR #2".to_string(),
                search_text: "#2 fix cleanup default branch fix/cleanup-default".to_string(),
            },
        ];
        let ranked: Vec<_> = entries.iter().collect();
        let state = PickerState {
            query: "git".to_string(),
            selected: 0,
        };
        let mut terminal = Terminal::new(TestBackend::new(88, 18)).expect("terminal");

        terminal
            .draw(|frame| {
                render_picker(
                    frame,
                    &state,
                    &ranked,
                    PickerView {
                        prompt: "git ws pr>",
                        marker_header: "PR",
                        name_header: "Title",
                        detail_header: "Head",
                    },
                );
            })
            .expect("draw");

        assert_snapshot!(terminal.backend());
    }
}

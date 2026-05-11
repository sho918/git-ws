use anyhow::{Result, anyhow};
use crossterm::event::{self, Event, KeyCode};
use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32String};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, Row, Table, TableState, Wrap};

use crate::candidates::{Candidate, TrackingState};
use crate::tui::{
    self, HIGHLIGHT_SYMBOL, NavCommand, Outcome, Tone, TuiTerminal, header_style, label_line,
    panel, row_highlight_style, tone_style,
};

const VISIBLE_ROWS: usize = 15;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickerEntry<T> {
    pub value: T,
    pub marker: String,
    pub name: String,
    pub detail: String,
    pub extra_columns: Vec<String>,
    pub tones: Vec<CellTone>,
    pub action: String,
    pub search_text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellTone {
    Default,
    Dim,
    Worktree,
    Local,
    Remote,
    Good,
    Warning,
    Bad,
    Info,
    Behind,
}

#[derive(Debug, Clone, Copy)]
pub struct PickerView<'a> {
    pub prompt: &'a str,
    pub marker_header: &'a str,
    pub name_header: &'a str,
    pub detail_header: &'a str,
    pub extra_headers: &'a [&'a str],
}

pub fn pick_candidate(
    candidates: &[Candidate],
    initial_query: Option<&str>,
) -> Result<Option<Candidate>> {
    if let Some(query) = initial_query {
        reject_ambiguous_remote_query(candidates, query)?;
    }
    let entries: Vec<PickerEntry<Candidate>> =
        candidates.iter().cloned().map(candidate_entry).collect();
    pick_entry(
        &entries,
        initial_query,
        PickerView {
            prompt: "git ws>",
            marker_header: "Avail",
            name_header: "Name",
            detail_header: "Upstream",
            extra_headers: &["Track", "Head", "Path", "Action"],
        },
    )
}

fn reject_ambiguous_remote_query(candidates: &[Candidate], query: &str) -> Result<()> {
    if candidates
        .iter()
        .any(|candidate| candidate.remote_ref.as_deref() == Some(query))
    {
        return Ok(());
    }
    if candidates.iter().any(|candidate| {
        candidate.name == query
            && (candidate.worktree_path.is_some() || candidate.local_ref.is_some())
    }) {
        return Ok(());
    }

    let remote_refs: Vec<&str> = candidates
        .iter()
        .filter(|candidate| candidate.name == query)
        .filter_map(|candidate| candidate.remote_ref.as_deref())
        .collect();
    if remote_refs.len() <= 1 {
        return Ok(());
    }

    Err(anyhow!(
        "ambiguous remote branch query '{query}'; use one of: {}",
        remote_refs.join(", ")
    ))
}

pub fn pick_entry<T: Clone>(
    entries: &[PickerEntry<T>],
    initial_query: Option<&str>,
    view: PickerView<'_>,
) -> Result<Option<T>> {
    if entries.is_empty() {
        if let Some(query) = initial_query {
            return Err(anyhow!("no match for query: {query}"));
        }
        return Ok(None);
    }

    if let Some(query) = initial_query {
        let ranked = rank_entries(query, entries);
        let Some(entry) = ranked.first() else {
            return Err(anyhow!("no match for query: {query}"));
        };
        return Ok(Some(entry.value.clone()));
    }

    if !tui::is_interactive() {
        return Err(anyhow!(
            "interactive picker requires a terminal or query argument"
        ));
    }

    run_picker(entries, view)
}

pub fn rank_entries<'a, T>(query: &str, entries: &'a [PickerEntry<T>]) -> Vec<&'a PickerEntry<T>> {
    PickerCache::new(entries).rank(query)
}

struct PickerCache<'a, T> {
    entries: &'a [PickerEntry<T>],
    haystacks: Vec<Utf32String>,
    matcher: Matcher,
}

impl<'a, T> PickerCache<'a, T> {
    fn new(entries: &'a [PickerEntry<T>]) -> Self {
        let haystacks = entries
            .iter()
            .map(|entry| Utf32String::from(entry.search_text.as_str()))
            .collect();
        Self {
            entries,
            haystacks,
            matcher: Matcher::new(Config::DEFAULT.match_paths()),
        }
    }

    fn rank(&mut self, query: &str) -> Vec<&'a PickerEntry<T>> {
        if query.trim().is_empty() {
            return self.entries.iter().collect();
        }
        let pattern = Pattern::new(
            query,
            CaseMatching::Smart,
            Normalization::Smart,
            AtomKind::Fuzzy,
        );
        let mut scored: Vec<(usize, u32)> = Vec::with_capacity(self.haystacks.len());
        for (index, haystack) in self.haystacks.iter().enumerate() {
            if let Some(score) = pattern.score(haystack.slice(..), &mut self.matcher) {
                scored.push((index, score));
            }
        }
        scored.sort_unstable_by_key(|(_, score)| std::cmp::Reverse(*score));
        scored
            .into_iter()
            .map(|(index, _)| &self.entries[index])
            .collect()
    }
}

fn run_picker<T: Clone>(entries: &[PickerEntry<T>], view: PickerView<'_>) -> Result<Option<T>> {
    let mut terminal = TuiTerminal::new()?;
    let mut state = PickerState::default();
    let mut cache = PickerCache::new(entries);
    let mut ranked = cache.rank(&state.query);

    loop {
        state.clamp_selection(ranked.len());
        if !event::poll(std::time::Duration::ZERO).unwrap_or(false) {
            terminal.draw(|frame| render_picker(frame, &state, &ranked, view))?;
        }

        loop {
            match event::read()? {
                Event::Key(key) => {
                    let command = picker_command(key);
                    match state.apply(command, ranked.len()) {
                        Outcome::Continue => {
                            if matches!(command, PickerCommand::Ignore) {
                                continue;
                            }
                            if matches!(
                                command,
                                PickerCommand::Insert(_) | PickerCommand::Backspace
                            ) {
                                ranked = cache.rank(&state.query);
                            }
                            break;
                        }
                        Outcome::Cancel => return Ok(None),
                        Outcome::Submit => {
                            return Ok(ranked.get(state.selected).map(|entry| entry.value.clone()));
                        }
                    }
                }
                Event::Resize(..) => break,
                _ => {}
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
    fn apply(&mut self, command: PickerCommand, visible_len: usize) -> Outcome {
        match command {
            PickerCommand::Cancel => Outcome::Cancel,
            PickerCommand::Submit => Outcome::Submit,
            PickerCommand::Up => {
                self.selected = self.selected.saturating_sub(1);
                Outcome::Continue
            }
            PickerCommand::Down => {
                if self.selected + 1 < visible_len {
                    self.selected += 1;
                }
                Outcome::Continue
            }
            PickerCommand::Backspace => {
                self.query.pop();
                self.selected = 0;
                Outcome::Continue
            }
            PickerCommand::Insert(ch) => {
                self.query.push(ch);
                self.selected = 0;
                Outcome::Continue
            }
            PickerCommand::Ignore => Outcome::Continue,
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

fn picker_command(key: event::KeyEvent) -> PickerCommand {
    if let Some(nav) = tui::nav_command(key) {
        return match nav {
            NavCommand::Up => PickerCommand::Up,
            NavCommand::Down => PickerCommand::Down,
            NavCommand::Submit => PickerCommand::Submit,
            NavCommand::Cancel => PickerCommand::Cancel,
        };
    }
    match key.code {
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
    frame.render_widget(Paragraph::new(title).block(panel(" search ")), chunks[0]);

    if ranked.is_empty() {
        frame.render_widget(
            Paragraph::new("No matches")
                .style(Style::default().fg(Color::DarkGray))
                .block(panel(" candidates ")),
            chunks[1],
        );
    } else {
        let headers = view.headers();
        let header =
            Row::new(headers.iter().map(|header| Cell::from(*header))).style(header_style());
        let rows = ranked.iter().map(|entry| {
            Row::new(
                entry
                    .display_columns()
                    .into_iter()
                    .enumerate()
                    .map(|(index, value)| {
                        let tone = entry.tones.get(index).copied().unwrap_or(CellTone::Default);
                        Cell::from(value).style(tone_style(tui_tone(tone)))
                    }),
            )
        });
        let table = Table::new(rows, view.widths())
            .header(header)
            .block(panel(" candidates "))
            .row_highlight_style(row_highlight_style())
            .highlight_symbol(HIGHLIGHT_SYMBOL);
        let scroll_offset = state
            .selected
            .saturating_sub(VISIBLE_ROWS.saturating_sub(1));
        let mut table_state = TableState::default().with_offset(scroll_offset);
        table_state.select(Some(state.selected));
        frame.render_stateful_widget(table, chunks[1], &mut table_state);
    }

    let detail = ranked
        .get(state.selected)
        .map(|entry| {
            let mut lines = vec![
                Line::from(vec![
                    Span::styled("Selected ", Style::default().fg(Color::DarkGray)),
                    Span::styled(&entry.name, Style::default().fg(Color::White)),
                ]),
                label_line("Detail   ", &entry.detail),
                label_line("Action   ", &entry.action),
            ];
            for (header, value) in view.extra_headers.iter().zip(entry.extra_columns.iter()) {
                lines.push(Line::from(vec![
                    Span::styled(format!("{header:<9}"), Style::default().fg(Color::DarkGray)),
                    Span::raw(value.clone()),
                ]));
            }
            lines
        })
        .unwrap_or_else(|| vec![Line::from("No selectable candidate")]);
    frame.render_widget(
        Paragraph::new(detail)
            .wrap(Wrap { trim: true })
            .block(panel(" detail ")),
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
    let upstream = candidate.upstream_label();
    let track = candidate.tracking.summary.clone();
    let head = candidate.head_label();
    let path = candidate.path_label();
    let action = candidate.action_label();
    PickerEntry {
        marker: candidate.availability_label(),
        detail: upstream.clone(),
        extra_columns: vec![track.clone(), head.clone(), path.clone(), action.clone()],
        tones: vec![
            availability_tone(&candidate),
            CellTone::Default,
            upstream_tone(&candidate),
            tracking_tone(&candidate.tracking.state),
            CellTone::Dim,
            if candidate.worktree_path.is_some() {
                CellTone::Worktree
            } else {
                CellTone::Dim
            },
            CellTone::Info,
        ],
        action,
        search_text: format!("{name} {upstream} {track} {head} {path}"),
        name,
        value: candidate,
    }
}

impl<'a> PickerView<'a> {
    fn headers(&self) -> Vec<&'a str> {
        let mut headers = vec![self.marker_header, self.name_header, self.detail_header];
        headers.extend_from_slice(self.extra_headers);
        headers
    }

    fn widths(&self) -> Vec<Constraint> {
        let headers = self.headers();
        if headers.len() == 3 {
            return vec![
                Constraint::Length(12),
                Constraint::Percentage(44),
                Constraint::Min(20),
            ];
        }
        headers
            .iter()
            .enumerate()
            .map(|(index, header)| match (index, *header) {
                (0, _) => Constraint::Length(12),
                (1, _) => Constraint::Percentage(28),
                (_, "Track" | "State") => Constraint::Length(16),
                (_, "Head" | "Updated") => Constraint::Length(11),
                (_, "Base") => Constraint::Length(14),
                (_, "Labels") => Constraint::Length(18),
                (_, "Path" | "Action" | "Planned") => Constraint::Min(18),
                _ => Constraint::Percentage(16),
            })
            .collect()
    }
}

impl<T> PickerEntry<T> {
    fn display_columns(&self) -> Vec<&str> {
        let mut columns = vec![
            self.marker.as_str(),
            self.name.as_str(),
            self.detail.as_str(),
        ];
        columns.extend(self.extra_columns.iter().map(String::as_str));
        columns
    }
}

fn availability_tone(candidate: &Candidate) -> CellTone {
    if candidate.worktree_path.is_some() {
        CellTone::Worktree
    } else if candidate.local_ref.is_some() {
        CellTone::Local
    } else if candidate.remote_ref.is_some() {
        CellTone::Remote
    } else {
        CellTone::Dim
    }
}

fn upstream_tone(candidate: &Candidate) -> CellTone {
    if candidate.upstream.is_some() || candidate.remote_ref.is_some() {
        CellTone::Remote
    } else {
        CellTone::Dim
    }
}

fn tracking_tone(state: &TrackingState) -> CellTone {
    match state {
        TrackingState::InSync => CellTone::Dim,
        TrackingState::Ahead => CellTone::Info,
        TrackingState::Behind | TrackingState::Diverged => CellTone::Behind,
        TrackingState::Gone => CellTone::Bad,
        TrackingState::NoUpstream => CellTone::Dim,
    }
}

fn tui_tone(tone: CellTone) -> Tone {
    match tone {
        CellTone::Default => Tone::Default,
        CellTone::Dim => Tone::Dim,
        CellTone::Worktree => Tone::Worktree,
        CellTone::Local => Tone::Local,
        CellTone::Remote => Tone::Remote,
        CellTone::Good => Tone::Good,
        CellTone::Warning => Tone::Warning,
        CellTone::Bad => Tone::Bad,
        CellTone::Info => Tone::Info,
        CellTone::Behind => Tone::Behind,
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::KeyModifiers;
    use insta::assert_snapshot;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    #[test]
    fn picker_state_updates_query_and_selection() {
        let mut state = PickerState::default();

        assert_eq!(
            state.apply(PickerCommand::Insert('p'), 3),
            Outcome::Continue
        );
        assert_eq!(state.apply(PickerCommand::Down, 3), Outcome::Continue);
        assert_eq!(state.apply(PickerCommand::Backspace, 3), Outcome::Continue);

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
                extra_columns: vec![],
                tones: vec![],
                action: "create worktree for PR #1".to_string(),
                search_text: "#1 feat: add git-ws CLI feat/implement-git-ws-cli".to_string(),
            },
            PickerEntry {
                value: "2",
                marker: "#2".to_string(),
                name: "fix: cleanup default branch".to_string(),
                detail: "fix/cleanup-default".to_string(),
                extra_columns: vec![],
                tones: vec![],
                action: "create worktree for PR #2".to_string(),
                search_text: "#2 fix cleanup default branch fix/cleanup-default".to_string(),
            },
        ];
        let ranked: Vec<_> = entries.iter().collect();
        let state = PickerState {
            query: "git".to_string(),
            selected: 0,
        };
        let mut terminal = Terminal::new(TestBackend::new(140, 18)).expect("terminal");

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
                        extra_headers: &[],
                    },
                );
            })
            .expect("draw");

        assert_snapshot!(terminal.backend());
    }

    #[test]
    fn render_picker_scrolls_candidates_to_selected_entry() {
        let entries = (1..=20)
            .map(|number| PickerEntry {
                value: number,
                marker: format!("#{number}"),
                name: format!("candidate-{number:02}"),
                detail: format!("branch-{number:02}"),
                extra_columns: vec![],
                tones: vec![],
                action: format!("create worktree {number:02}"),
                search_text: format!("candidate-{number:02} branch-{number:02}"),
            })
            .collect::<Vec<_>>();
        let ranked: Vec<_> = entries.iter().collect();
        let state = PickerState {
            query: String::new(),
            selected: 15,
        };
        let mut terminal = Terminal::new(TestBackend::new(88, 28)).expect("terminal");

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
                        extra_headers: &[],
                    },
                );
            })
            .expect("draw");

        let screen = format!("{:?}", terminal.backend());
        assert!(screen.contains("#16"), "{screen}");
    }
}

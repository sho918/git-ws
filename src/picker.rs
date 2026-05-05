use std::io::{self, IsTerminal, Write};

use anyhow::{Result, anyhow};
use crossterm::cursor;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{self, ClearType};
use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

use crate::candidates::Candidate;

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

impl<T> PickerEntry<T> {
    pub fn new(
        value: T,
        marker: String,
        name: String,
        detail: String,
        action: String,
        search_text: String,
    ) -> Self {
        Self {
            value,
            marker,
            name,
            detail,
            action,
            search_text,
        }
    }
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

    if !io::stdin().is_terminal() {
        return Err(anyhow!(
            "interactive picker requires a terminal or query argument"
        ));
    }

    let mut state = PickerState {
        query: String::new(),
        selected: 0,
    };
    let _guard = RawModeGuard::new()?;

    loop {
        let ranked = rank_entries(&state.query, entries);
        draw(&state, &ranked, view)?;
        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(None);
                }
                KeyCode::Esc => return Ok(None),
                KeyCode::Enter => {
                    return Ok(ranked.get(state.selected).map(|entry| entry.value.clone()));
                }
                KeyCode::Up => state.selected = state.selected.saturating_sub(1),
                KeyCode::Down if state.selected + 1 < ranked.len() => state.selected += 1,
                KeyCode::Backspace => {
                    state.query.pop();
                    state.selected = 0;
                }
                KeyCode::Char(ch) => {
                    state.query.push(ch);
                    state.selected = 0;
                }
                _ => {}
            }
        }
    }
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

struct PickerState {
    query: String,
    selected: usize,
}

struct RawModeGuard;

impl RawModeGuard {
    fn new() -> Result<Self> {
        terminal::enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        terminal::disable_raw_mode().ok();
        execute!(io::stdout(), cursor::Show).ok();
    }
}

fn draw<T>(state: &PickerState, ranked: &[&PickerEntry<T>], view: PickerView<'_>) -> Result<()> {
    let mut stdout = io::stdout();
    execute!(
        stdout,
        cursor::Hide,
        terminal::Clear(ClearType::All),
        cursor::MoveTo(0, 0)
    )?;
    writeln!(stdout, "{} {}", view.prompt, state.query)?;
    writeln!(
        stdout,
        "{:<9}  {:27}  {}",
        view.marker_header, view.name_header, view.detail_header
    )?;
    writeln!(stdout, "---------  ---------------------------  ------")?;
    for (index, entry) in ranked.iter().take(VISIBLE_ROWS).enumerate() {
        let marker = if index == state.selected { ">" } else { " " };
        writeln!(
            stdout,
            "{} {:9} {:27} {}",
            marker,
            entry.marker,
            fit(&entry.name, 27),
            entry.detail
        )?;
    }
    if let Some(entry) = ranked.get(state.selected) {
        writeln!(stdout)?;
        writeln!(stdout, "Selected: {}", entry.name)?;
        writeln!(stdout, "Action  : {}", entry.action)?;
    }
    stdout.flush()?;
    Ok(())
}

fn fit(value: &str, width: usize) -> String {
    let count = value.chars().count();
    if count <= width {
        format!("{value:<width$}")
    } else if width <= 3 {
        value.chars().take(width).collect()
    } else {
        let head: String = value.chars().take(width - 3).collect();
        format!("{head}...")
    }
}

fn candidate_entry(candidate: Candidate) -> PickerEntry<Candidate> {
    let marker = candidate.availability_label();
    let name = candidate.name.clone();
    let detail = candidate.detail();
    let action = action_label(&candidate);
    let search_text = candidate.name.clone();
    PickerEntry::new(candidate, marker, name, detail, action, search_text)
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

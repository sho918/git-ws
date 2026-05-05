use std::io::{self, IsTerminal, Write};

use anyhow::{Result, anyhow};
use crossterm::cursor;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{self, ClearType};

use crate::candidates::{Candidate, CandidateIndex};

const VISIBLE_ROWS: usize = 15;

pub fn pick_candidate(
    candidates: &[Candidate],
    initial_query: Option<&str>,
) -> Result<Option<Candidate>> {
    if candidates.is_empty() {
        return Ok(None);
    }

    let mut index = CandidateIndex::new(candidates);

    if let Some(query) = initial_query {
        return Ok(index
            .rank(query)
            .first()
            .map(|candidate| (*candidate).clone()));
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
        let ranked = index.rank(&state.query);
        draw(&state, &ranked)?;
        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(None);
                }
                KeyCode::Esc => return Ok(None),
                KeyCode::Enter => {
                    return Ok(ranked
                        .get(state.selected)
                        .map(|candidate| (*candidate).clone()));
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

fn draw(state: &PickerState, ranked: &[&Candidate]) -> Result<()> {
    let mut stdout = io::stdout();
    execute!(
        stdout,
        cursor::Hide,
        terminal::Clear(ClearType::All),
        cursor::MoveTo(0, 0)
    )?;
    writeln!(stdout, "git ws> {}", state.query)?;
    writeln!(stdout, "Avail      Name                         Detail")?;
    writeln!(stdout, "---------  ---------------------------  ------")?;
    for (index, candidate) in ranked.iter().take(VISIBLE_ROWS).enumerate() {
        let marker = if index == state.selected { ">" } else { " " };
        writeln!(
            stdout,
            "{} {:9} {:27} {}",
            marker,
            candidate.availability_label(),
            fit(&candidate.name, 27),
            candidate.detail()
        )?;
    }
    if let Some(candidate) = ranked.get(state.selected) {
        writeln!(stdout)?;
        writeln!(stdout, "Selected: {}", candidate.name)?;
        writeln!(stdout, "Action  : {}", action_label(candidate))?;
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

use std::io::{self, IsTerminal};
use std::sync::Once;

use anyhow::Result;
use crossterm::cursor;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::backend::CrosstermBackend;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders};
use ratatui::{Frame, Terminal};

pub(crate) const HIGHLIGHT_SYMBOL: &str = "› ";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    Continue,
    Submit,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NavCommand {
    Up,
    Down,
    Submit,
    Cancel,
}

pub(crate) fn nav_command(key: KeyEvent) -> Option<NavCommand> {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return match key.code {
            KeyCode::Char('c') => Some(NavCommand::Cancel),
            KeyCode::Char('n') => Some(NavCommand::Down),
            KeyCode::Char('p') => Some(NavCommand::Up),
            _ => None,
        };
    }
    match key.code {
        KeyCode::Esc => Some(NavCommand::Cancel),
        KeyCode::Enter => Some(NavCommand::Submit),
        KeyCode::Up => Some(NavCommand::Up),
        KeyCode::Down => Some(NavCommand::Down),
        _ => None,
    }
}

pub(crate) fn is_interactive() -> bool {
    io::stdin().is_terminal() && io::stderr().is_terminal()
}

pub(crate) fn panel(title: &str) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(title.to_string())
}

pub(crate) fn header_style() -> Style {
    Style::default()
        .fg(Color::DarkGray)
        .add_modifier(Modifier::BOLD)
}

pub(crate) fn row_highlight_style() -> Style {
    Style::default()
        .fg(Color::Black)
        .bg(Color::Cyan)
        .add_modifier(Modifier::BOLD)
}

pub(crate) fn label_line<'a>(label: &'a str, value: &'a str) -> Line<'a> {
    Line::from(vec![
        Span::styled(label, Style::default().fg(Color::DarkGray)),
        Span::raw(value),
    ])
}

pub(crate) struct TuiTerminal {
    terminal: Terminal<CrosstermBackend<io::Stderr>>,
}

impl TuiTerminal {
    pub(crate) fn new() -> Result<Self> {
        install_panic_hook();
        enable_raw_mode()?;
        match Self::init() {
            Ok(terminal) => Ok(Self { terminal }),
            Err(error) => {
                disable_raw_mode().ok();
                Err(error)
            }
        }
    }

    fn init() -> Result<Terminal<CrosstermBackend<io::Stderr>>> {
        let mut stderr = io::stderr();
        execute!(stderr, EnterAlternateScreen, cursor::Hide)?;
        let terminal = Terminal::new(CrosstermBackend::new(stderr))?;
        Ok(terminal)
    }

    pub(crate) fn draw<F>(&mut self, f: F) -> Result<()>
    where
        F: FnOnce(&mut Frame<'_>),
    {
        self.terminal.draw(f)?;
        Ok(())
    }
}

impl Drop for TuiTerminal {
    fn drop(&mut self) {
        restore_terminal();
    }
}

fn restore_terminal() {
    disable_raw_mode().ok();
    execute!(io::stderr(), cursor::Show, LeaveAlternateScreen).ok();
}

fn install_panic_hook() {
    static HOOK: Once = Once::new();
    HOOK.call_once(|| {
        let original = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore_terminal();
            original(info);
        }));
    });
}

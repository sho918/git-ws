use std::io;

use anyhow::Result;
use crossterm::cursor;
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::backend::CrosstermBackend;
use ratatui::{Frame, Terminal};

pub(crate) struct TuiTerminal {
    terminal: Terminal<CrosstermBackend<io::Stderr>>,
}

impl TuiTerminal {
    pub(crate) fn new() -> Result<Self> {
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
        let mut terminal = Terminal::new(CrosstermBackend::new(stderr))?;
        terminal.clear()?;
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
        disable_raw_mode().ok();
        execute!(
            self.terminal.backend_mut(),
            cursor::Show,
            LeaveAlternateScreen
        )
        .ok();
    }
}

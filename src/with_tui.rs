use std::io::{stdout, Stdout};

use crossterm::{
    event::DisableMouseCapture,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{backend::CrosstermBackend, Terminal};

pub trait WithTui {
    fn tui_setup(&self) -> Result<Terminal<CrosstermBackend<Stdout>>, Box<dyn std::error::Error>> {
        let backend = CrosstermBackend::new(stdout());
        let mut terminal = Terminal::new(backend)?;
        stdout().execute(EnterAlternateScreen)?;
        enable_raw_mode()?;
        terminal.clear()?;
        Ok(terminal)
    }

    fn tui_shutdown(
        &self,
        terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        terminal.show_cursor()?;
        disable_raw_mode()?;
        stdout()
            .execute(LeaveAlternateScreen)?
            .execute(DisableMouseCapture)?;
        Ok(())
    }
}

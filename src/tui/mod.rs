mod app;
mod event;
mod ui;

use anyhow::Result;
use crate::storage::paths::PortalPaths;

/// Launch the interactive TUI profile browser.
///
/// # Errors
///
/// Returns an error if terminal initialization fails or any I/O
/// operation during the event loop encounters a fatal error.
pub fn run(paths: &PortalPaths) -> Result<()> {
    let mut app = app::App::new(paths.clone())?;
    ratatui::run(|terminal| {
        loop {
            terminal.draw(|frame| ui::render(frame, &mut app))?;
            if event::handle(&mut app)? {
                break Ok(());
            }
        }
    })
}

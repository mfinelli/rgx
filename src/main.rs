use std::io;
use std::time::Duration;

use anyhow::{Context as _, Result};
use clap::Parser;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use rgx::cli::Cli;
use rgx::engine::RustEngine;
use rgx::tui::{handle_key, render, App};

fn main() -> Result<()> {
    let cli = Cli::parse();
    Cli::handle_subcommands(&cli);

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

    // Panic safety net: restores the terminal even if run() panics.
    // Drop::drop() can't return errors so they are silently swallowed here —
    // the explicit match below handles error propagation on the normal path.
    let _guard = scopeguard::defer_on_unwind! {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
    };

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run(&mut terminal);

    // Normal path cleanup — errors are captured and returned.
    let cleanup: Result<()> = (|| {
        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        terminal.show_cursor()?;
        Ok(())
    })();

    match (result, cleanup) {
        (Err(run_err), Err(cleanup_err)) => {
            Err(run_err.context(format!("cleanup also failed: {}", cleanup_err)))
        }
        (Err(run_err), Ok(_)) => Err(run_err),
        (Ok(_), Err(cleanup_err)) => Err(cleanup_err),
        (Ok(_), Ok(_)) => Ok(()),
    }
}

/// Run the application event loop until the user quits.
///
/// Owns the terminal for its entire duration. Returns `Ok(())` when the user
/// exits normally (e.g. presses `q`). Any error from rendering or event
/// reading is propagated up to `main()` for cleanup and reporting.
fn run(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    let engine = RustEngine::new();
    let mut app = App::new();

    // Force an initial render
    terminal.draw(|f| render(&app, f))?;

    loop {
        // Check if debounce has elapsed and evaluate if needed
        app.maybe_evaluate(&engine);

        // Poll for events with a short timeout so debounce fires promptly
        if event::poll(Duration::from_millis(30))? {
            if let Event::Key(key) = event::read()? {
                if handle_key(&mut app, key) {
                    break;
                }
            }
        }

        terminal.draw(|f| render(&app, f))?;
    }

    Ok(())
}

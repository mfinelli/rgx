use std::io;
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use cli::Cli;
use engine::RustEngine;
use tui::{handle_key, render, App};

fn main() -> Result<()> {
    let _cli = Cli::parse();

    // Set up terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Run the app, ensure cleanup even on panic
    let result = run(&mut terminal);

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }

    Ok(())
}

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

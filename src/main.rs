/* rgx: command line regexp tester
 * Copyright 2026 Mario Finelli
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */

use std::io;
use std::time::Duration;

use anyhow::{Context as _, Result};
use clap::Parser;
use clap_complete::generate;
use crossterm::{
    event::{self, Event},
    execute,
    terminal::{
        EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
        enable_raw_mode,
    },
};
use ratatui::{Terminal, backend::CrosstermBackend};

use rgx::cli::{Cli, Command};
use rgx::config::{Config, OnOpen};
use rgx::db::store::{Db, Session, default_db_path};
use rgx::engine::native::RustEngine;
use rgx::session::SessionManager;
use rgx::tui::app::{App, handle_key, render};

/// Restores the terminal to its original state when dropped.
///
/// This runs on all scope exits including panics, ensuring the terminal is
/// never left in raw mode. Errors are silently swallowed since Drop cannot
/// return a Result (the explicit cleanup in main() handles error propagation
/// on the normal path).
struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Handle subcommands before any TUI setup
    if let Some(Command::Completions { shell }) = cli.command {
        let mut cmd = <Cli as clap::CommandFactory>::command();
        let name = cmd.get_name().to_string();
        generate(shell, &mut cmd, name, &mut io::stdout());
        return Ok(());
    }

    let config = Config::load(cli.config.as_deref())
        .context("failed to load configuration")?;

    let db_path = config.db_path.clone().unwrap_or_else(default_db_path);
    let db = Db::open(&db_path).context("failed to open history database")?;

    // Determine session to open based on config and DB state.
    let manager = resolve_session(db, &config)?;

    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;

    // Panic safety net: restores the terminal even if run() panics.
    let _guard = TerminalGuard;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    let result = run(&mut terminal, config, manager);

    let cleanup: Result<()> = (|| {
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;
        Ok(())
    })();

    match (result, cleanup) {
        (Err(run_err), Err(cleanup_err)) => {
            Err(run_err
                .context(format!("cleanup also failed: {}", cleanup_err)))
        }
        (Err(run_err), Ok(_)) => Err(run_err),
        (Ok(_), Err(cleanup_err)) => Err(cleanup_err),
        (Ok(_), Ok(_)) => Ok(()),
    }
}

/// Decide which session to open based on config and existing DB state.
///
/// Runs the splash screen if `on_open` is `Ask` and a previous session exists.
/// The splash also shows available runtimes (currently only Rust is listed
/// since engine probing is deferred to a later phase).
fn resolve_session(db: Db, config: &Config) -> Result<SessionManager> {
    let most_recent = db.most_recent_session()?;

    match (&config.on_open, most_recent) {
        // No previous session regardless of config: start fresh, no prompt.
        (_, None) => SessionManager::new_session(db, "rust", Some("regex")),

        // Config says always continue.
        (OnOpen::Continue, Some(session)) => {
            SessionManager::resume(db, session)
        }

        // Config says always start new.
        (OnOpen::New, _) => {
            SessionManager::new_session(db, "rust", Some("regex"))
        }

        // Config says ask: show the splash and prompt.
        (OnOpen::Ask, Some(session)) => splash_prompt(db, session, config),
    }
}

/// Show the startup splash and prompt the user to continue or start new.
fn splash_prompt(
    db: Db,
    session: Session,
    _config: &Config,
) -> Result<SessionManager> {
    use crossterm::event::{KeyCode, KeyEvent};

    // Enable raw mode just for the duration of the prompt so we can read
    // single keypresses without waiting for Enter.
    enable_raw_mode()?;

    print!("\r\n  rgx v{}\r\n", env!("CARGO_PKG_VERSION"));
    print!("\r\n  \u{2713} Rust    built-in\r\n");
    print!(
        "  (additional runtimes shown here once engine probing is implemented)\r\n"
    );
    print!("\r\n");

    let session_label = session.name.as_deref().unwrap_or("unnamed session");
    print!("  Last session: {}\r\n", session_label);
    print!("\r\n  [C]ontinue   [N]ew session   [q]uit\r\n");

    let result = loop {
        if event::poll(Duration::from_millis(100))?
            && let Event::Key(KeyEvent { code, .. }) = event::read()?
        {
            match code {
                KeyCode::Char('c') | KeyCode::Char('C') | KeyCode::Enter => {
                    break SessionManager::resume(db, session);
                }
                KeyCode::Char('n') | KeyCode::Char('N') => {
                    break SessionManager::new_session(
                        db,
                        "rust",
                        Some("regex"),
                    );
                }
                KeyCode::Char('q') => {
                    disable_raw_mode()?;
                    std::process::exit(0);
                }
                _ => {}
            }
        }
    };

    // Disable raw mode before handing control back to main().
    disable_raw_mode()?;
    print!("\r\n");
    result
}

/// Run the application event loop until the user quits.
///
/// Owns the terminal for its entire duration. Returns `Ok(())` when the user
/// exits normally (e.g. presses `q`). Any error from rendering or event
/// reading is propagated up to `main()` for cleanup and reporting.
fn run(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    config: Config,
    manager: SessionManager,
) -> Result<()> {
    let engine = RustEngine::new();
    let mut app = App::new(
        config.nerd_fonts,
        config.default_results_view.to_results_view(),
        config.debounce_ms,
        config.fancy_regex_default,
    );
    app.attach_session(manager);

    // Force an initial render
    terminal.draw(|f| render(&app, f))?;

    loop {
        // Check if debounce has elapsed and evaluate if needed
        app.maybe_evaluate(&engine);

        // Poll for events with a short timeout so debounce fires promptly
        if event::poll(Duration::from_millis(30))?
            && let Event::Key(key) = event::read()?
            && handle_key(&mut app, key)
        {
            break;
        }

        terminal.draw(|f| render(&app, f))?;
    }

    Ok(())
}

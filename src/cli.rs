use std::path::PathBuf;

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};

#[derive(Parser, Debug)]
#[command(
    name = "rgx",
    version,
    about = "A terminal UI regex tester with multi-engine support",
    long_about = "rgx is a terminal UI regex tester supporting multiple engines \
                  (Rust, Python, JavaScript, Ruby, PHP, Go, grep, sed) with \
                  live evaluation, session history, and snippet export."
)]
pub struct Cli {
    /// Path to config file
    #[arg(long, value_name = "FILE", env = "RGX_CONFIG")]
    pub config: Option<PathBuf>,

    /// Path to history database
    #[arg(long, value_name = "FILE", env = "RGX_DB")]
    pub db: Option<PathBuf>,

    /// Enable Nerd Font icons in the UI
    #[arg(long, env = "RGX_NERD_FONTS")]
    pub nerd_fonts: bool,

    /// Skip the runtime probe splash screen
    #[arg(long)]
    pub no_splash: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Generate shell completions and print to stdout
    ///
    /// Example: rgx completions bash >> ~/.bash_completion
    Completions {
        /// The shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },
}

impl Cli {
    /// If a subcommand was given, handle it and exit.
    /// Returns the Cli if normal TUI startup should proceed.
    pub fn handle_subcommands(cli: &Cli) {
        if let Some(Command::Completions { shell }) = &cli.command {
            let mut cmd = Cli::command();
            let name = cmd.get_name().to_string();
            generate(*shell, &mut cmd, name, &mut std::io::stdout());
            std::process::exit(0);
        }
    }
}

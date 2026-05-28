use std::path::PathBuf;

/// Parsed CLI arguments for rgx.
/// Simple manual parser — clap will replace this once a Rust >= 1.85
/// toolchain is available in the build environment.
#[derive(Debug, Default)]
pub struct Cli {
    pub config: Option<PathBuf>,
    pub db: Option<PathBuf>,
    pub nerd_fonts: bool,
    pub no_splash: bool,
}

impl Cli {
    pub fn parse() -> Self {
        let mut cli = Cli::default();
        let args: Vec<String> = std::env::args().skip(1).collect();
        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "--help" | "-h" => {
                    print_help();
                    std::process::exit(0);
                }
                "--version" | "-V" => {
                    println!("rgx {}", env!("CARGO_PKG_VERSION"));
                    std::process::exit(0);
                }
                "--nerd-fonts" => cli.nerd_fonts = true,
                "--no-splash" => cli.no_splash = true,
                "--config" => {
                    i += 1;
                    if let Some(val) = args.get(i) {
                        cli.config = Some(PathBuf::from(val));
                    }
                }
                "--db" => {
                    i += 1;
                    if let Some(val) = args.get(i) {
                        cli.db = Some(PathBuf::from(val));
                    }
                }
                other => {
                    eprintln!("unknown argument: {}", other);
                    std::process::exit(1);
                }
            }
            i += 1;
        }
        cli
    }
}

fn print_help() {
    println!("rgx {} — A terminal UI regex tester with multi-engine support", env!("CARGO_PKG_VERSION"));
    println!();
    println!("USAGE:");
    println!("    rgx [OPTIONS]");
    println!();
    println!("OPTIONS:");
    println!("    --config <FILE>    Path to config file (default: ~/.config/rgx/config.toml)");
    println!("    --db <FILE>        Path to history database (default: ~/.local/share/rgx/history.db)");
    println!("    --nerd-fonts       Enable Nerd Font icons");
    println!("    --no-splash        Skip the runtime probe splash screen");
    println!("    -h, --help         Print help");
    println!("    -V, --version      Print version");
}

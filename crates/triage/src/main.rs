//! Triage daemon and CLI.
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use triage::config::{self, Config, Paths};

#[derive(Debug, Parser)]
#[command(
    name = "triage",
    version,
    about,
    help_template = "{name} {version}\n{about-with-newline}\n{usage-heading} {usage}\n\n{all-args}{after-help}"
)]
struct Cli {
    // No `env =`: config.rs reads TRIAGE_DATA_DIR so it can log when the flag wins.
    #[arg(
        long,
        global = true,
        value_name = "PATH",
        help = "Keep data, logs, and config under PATH. Overrides TRIAGE_DATA_DIR"
    )]
    data_dir: Option<PathBuf>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            // No tracing subscriber exists yet when config fails.
            let _ = writeln!(std::io::stderr(), "error: {err}");
            ExitCode::from(err.exit_code())
        }
    }
}

fn run(cli: Cli) -> Result<(), config::Error> {
    let _config = Config::load()?;
    let _paths = Paths::resolve(cli.data_dir)?;
    Ok(())
}

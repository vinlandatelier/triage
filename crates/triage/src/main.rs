use std::io::Write;
use std::process::ExitCode;

use clap::Parser;
use triage::cli::{self, Cli};

fn main() -> ExitCode {
    match cli::run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            // Config errors happen before the tracing subscriber exists.
            let _ = writeln!(std::io::stderr(), "error: {err}");
            ExitCode::from(err.exit_code())
        }
    }
}

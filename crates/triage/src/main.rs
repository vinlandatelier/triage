use std::io::Write;
use std::process::ExitCode;

use clap::Parser;
use triage::cli::{self, Cli};

fn main() -> ExitCode {
    match cli::run(Cli::parse()) {
        Ok(code) => code,
        Err(err) => {
            // No tracing subscriber exists yet when config fails.
            let _ = writeln!(std::io::stderr(), "error: {err}");
            ExitCode::from(err.exit_code())
        }
    }
}

//! Command-line surface: argument parsing and subcommands.

use std::fmt::Write as _;
use std::io;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;
use time::OffsetDateTime;

use crate::config::{self, Config, Paths};
use crate::{http, telemetry};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Config(#[from] config::Error),
    #[error(transparent)]
    Http(#[from] http::Error),
}

impl Error {
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Config(err) => err.exit_code(),
            Self::Http(err) => err.exit_code(),
        }
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "triage",
    version,
    about,
    help_template = "{name} {version}\n{about-with-newline}\n{usage-heading} {usage}\n\n{all-args}{after-help}"
)]
pub struct Cli {
    // No `env =`: config.rs reads TRIAGE_DATA_DIR so it can log when the flag wins.
    #[arg(
        long,
        global = true,
        value_name = "PATH",
        help = "Keep data, logs, and config under PATH. Overrides TRIAGE_DATA_DIR"
    )]
    data_dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run the daemon in the foreground on `127.0.0.1:$TRIAGE_PORT`
    Serve,
    /// Report whether the daemon is running; exits 1 when it is not
    Status,
    /// Inspect the resolved configuration
    #[command(subcommand)]
    Config(ConfigCommand),
    /// Locate log files
    #[command(subcommand)]
    Logs(LogsCommand),
    /// Print a shell completion script to stdout
    Completions { shell: CompletionShell },
}

#[derive(Debug, Subcommand)]
enum LogsCommand {
    /// Print the absolute path of today's JSONL log file, which may not exist yet
    Path,
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    /// Print every config value and path, secrets redacted
    Show,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum CompletionShell {
    Bash,
    Zsh,
    Fish,
}

impl From<CompletionShell> for Shell {
    fn from(shell: CompletionShell) -> Self {
        match shell {
            CompletionShell::Bash => Self::Bash,
            CompletionShell::Zsh => Self::Zsh,
            CompletionShell::Fish => Self::Fish,
        }
    }
}

pub fn run(cli: Cli) -> Result<ExitCode, Error> {
    // Completions must work before config exists, e.g. during install.
    let Cli { data_dir, command } = cli;
    if let Some(Command::Completions { shell }) = command {
        completions(shell, &mut io::stdout());
        return Ok(ExitCode::SUCCESS);
    }
    let config = Config::load()?;
    let paths = Paths::resolve(data_dir.clone())?;
    // Lives until `run` returns, which is when `main` exits.
    let _guard = telemetry::init(&config, &paths);
    config::log_data_dir_override(data_dir.as_deref());
    tracing::debug!(?config, ?paths, "startup");
    match command {
        Some(Command::Serve) => http::serve(config, paths)?,
        Some(Command::Status) => return Ok(status(config.port)?),
        Some(Command::Config(ConfigCommand::Show)) => show(&config, &paths),
        Some(Command::Logs(LogsCommand::Path)) => logs_path(&paths),
        Some(Command::Completions { .. }) | None => {}
    }
    Ok(ExitCode::SUCCESS)
}

#[expect(clippy::print_stdout, reason = "CLI output")]
fn status(port: u16) -> http::Result<ExitCode> {
    let Some(health) = http::probe(port)? else {
        println!("not running");
        return Ok(ExitCode::FAILURE);
    };
    println!(
        "running v{} on 127.0.0.1:{port}, up {}",
        health.version,
        uptime(health.uptime_s)
    );
    Ok(ExitCode::SUCCESS)
}

fn uptime(secs: u64) -> String {
    let (days, hours, mins) = (secs / 86_400, secs / 3600 % 24, secs / 60 % 60);
    match (days, hours, mins) {
        (0, 0, 0) => format!("{secs}s"),
        (0, 0, _) => format!("{mins}m"),
        (0, _, _) => format!("{hours}h{mins}m"),
        _ => format!("{days}d{hours}h"),
    }
}

#[expect(clippy::print_stdout, reason = "CLI output")]
fn logs_path(paths: &Paths) {
    println!(
        "{}",
        telemetry::log_file(&paths.logs, OffsetDateTime::now_utc().date()).display()
    );
}

#[expect(clippy::print_stdout, reason = "CLI output")]
fn show(config: &Config, paths: &Paths) {
    print!("{}", config_table(config, paths));
}

fn config_table(config: &Config, paths: &Paths) -> String {
    let Paths {
        data,
        logs,
        config: config_dir,
    } = paths;
    let mut rows = config.entries();
    rows.extend([
        ("data_dir".to_owned(), data.display().to_string()),
        ("logs_dir".to_owned(), logs.display().to_string()),
        ("config_dir".to_owned(), config_dir.display().to_string()),
    ]);
    let width = rows.iter().map(|(key, _)| key.len()).max().unwrap_or(0);
    rows.iter().fold(String::new(), |mut table, (key, value)| {
        let _ = writeln!(table, "{key:<width$}  {value}");
        table
    })
}

fn completions(shell: CompletionShell, out: &mut impl io::Write) {
    clap_complete::generate(Shell::from(shell), &mut Cli::command(), "triage", out);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths() -> Paths {
        Paths {
            data: "/d".into(),
            logs: "/d/logs".into(),
            config: "/d/config".into(),
        }
    }

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn table_aligns_values_and_lists_paths() {
        assert_eq!(
            config_table(&Config::default(), &paths()),
            "\
TRIAGE_PORT            7311
TRIAGE_LOG_LEVEL       \"info\"
TRIAGE_LOG_FILE_LEVEL  \"info,triage=debug\"
TRIAGE_LOG_FORMAT      Columns
TRIAGE_OTEL_ENDPOINT   None
data_dir               /d
logs_dir               /d/logs
config_dir             /d/config
"
        );
    }

    #[test]
    fn uptime_keeps_the_two_largest_units() {
        for (secs, want) in [
            (0, "0s"),
            (59, "59s"),
            (60, "1m"),
            (3599, "59m"),
            (3 * 3600 + 12 * 60 + 5, "3h12m"),
            (86_400 + 7200 + 60, "1d2h"),
        ] {
            assert_eq!(uptime(secs), want, "{secs}");
        }
    }

    #[test]
    fn completions_print_a_script_per_shell() -> Result<(), Box<dyn std::error::Error>> {
        for (shell, marker) in [
            (CompletionShell::Bash, "complete -F"),
            (CompletionShell::Zsh, "#compdef triage"),
            (CompletionShell::Fish, "complete -c triage"),
        ] {
            let mut out = Vec::new();
            completions(shell, &mut out);
            let script = String::from_utf8(out)?;
            assert!(script.contains(marker), "{shell:?}: {script}");
            assert!(script.contains("completions"), "{shell:?}");
        }
        Ok(())
    }
}

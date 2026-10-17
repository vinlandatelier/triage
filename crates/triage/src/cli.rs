//! Command-line surface: argument parsing and subcommands.

use std::fmt::Write as _;
use std::io;
use std::path::PathBuf;

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;

use crate::config::{self, Config, Paths};

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
    /// Inspect the resolved configuration
    #[command(subcommand)]
    Config(ConfigCommand),
    /// Print a shell completion script to stdout
    Completions { shell: CompletionShell },
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

pub fn run(cli: Cli) -> config::Result<()> {
    // Completions must work before config exists, e.g. during install.
    if let Some(Command::Completions { shell }) = cli.command {
        completions(shell, &mut io::stdout());
        return Ok(());
    }
    let config = Config::load()?;
    let paths = Paths::resolve(cli.data_dir)?;
    match cli.command {
        Some(Command::Config(ConfigCommand::Show)) => show(&config, &paths),
        Some(Command::Completions { .. }) | None => {}
    }
    Ok(())
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

//! Command-line surface: argument parsing and subcommands.

use std::fmt::Write as _;
use std::io;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;
use time::OffsetDateTime;

use crate::config::{self, Config, Paths};
use crate::{http, store, telemetry};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Config(#[from] config::Error),
    #[error(transparent)]
    Http(#[from] http::Error),
    #[error(transparent)]
    Store(#[from] store::Error),
    #[error("cannot start the async runtime: {0}")]
    Runtime(io::Error),
    #[error("cannot write to stdout: {0}")]
    Stdout(io::Error),
}

impl Error {
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Config(err) => err.exit_code(),
            Self::Http(err) => err.exit_code(),
            Self::Store(err) => err.exit_code(),
            Self::Runtime(_) | Self::Stdout(_) => 1,
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
    /// Inspect the database
    #[command(subcommand)]
    Db(DbCommand),
    /// Print a shell completion script to stdout
    Completions { shell: CompletionShell },
}

#[derive(Debug, Subcommand)]
enum LogsCommand {
    /// Print the absolute path of today's JSONL log file, which may not exist yet
    Path,
}

#[derive(Debug, Subcommand)]
enum DbCommand {
    /// Migrate the database, then print its schema as ordered `CREATE` statements
    Schema,
    /// Apply pending migrations and print how many ran
    Migrate,
    /// Compare the database schema to the `schema.sql` this build ships; exits 1 on drift
    Check,
    /// Run one query on a read-only connection and print the rows
    Sql {
        #[arg(allow_hyphen_values = true)]
        query: String,
        /// Print a JSON array of row objects instead of a table
        #[arg(long)]
        json: bool,
    },
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
        let mut script = Vec::new();
        completions(shell, &mut script);
        emit(&String::from_utf8_lossy(&script))?;
        return Ok(ExitCode::SUCCESS);
    }
    let config = Config::load()?;
    let paths = Paths::resolve(data_dir.clone())?;
    // Lives until `run` returns, which is when `main` exits.
    let _guard = telemetry::init(&config, &paths);
    config::log_data_dir_override(data_dir.as_deref());
    tracing::debug!(?config, ?paths, "startup");
    Ok(match command {
        Some(Command::Status) => status(config.port)?,
        Some(Command::Serve) => {
            http::serve(config, paths)?;
            ExitCode::SUCCESS
        }
        Some(Command::Config(ConfigCommand::Show)) => {
            emit(&config_table(&config, &paths))?;
            ExitCode::SUCCESS
        }
        Some(Command::Logs(LogsCommand::Path)) => {
            logs_path(&paths)?;
            ExitCode::SUCCESS
        }
        Some(Command::Db(DbCommand::Schema)) => {
            db_schema(&paths)?;
            ExitCode::SUCCESS
        }
        Some(Command::Db(DbCommand::Migrate)) => {
            db_migrate(&paths)?;
            ExitCode::SUCCESS
        }
        Some(Command::Db(DbCommand::Check)) => db_check(&paths)?,
        Some(Command::Db(DbCommand::Sql { query, json })) => {
            db_sql(&paths, &query, json)?;
            ExitCode::SUCCESS
        }
        Some(Command::Completions { .. }) | None => ExitCode::SUCCESS,
    })
}

/// Writes command output. A reader that closed the pipe early, as `head` does, is not an error.
fn emit(text: &str) -> Result<(), Error> {
    write_out(&mut io::stdout().lock(), text)
}

fn write_out(out: &mut impl io::Write, text: &str) -> Result<(), Error> {
    match out.write_all(text.as_bytes()).and_then(|()| out.flush()) {
        Err(err) if err.kind() != io::ErrorKind::BrokenPipe => Err(Error::Stdout(err)),
        _ => Ok(()),
    }
}

fn status(port: u16) -> Result<ExitCode, Error> {
    let Some(health) = http::probe(port)? else {
        emit("not running\n")?;
        return Ok(ExitCode::FAILURE);
    };
    emit(&format!(
        "running v{} on 127.0.0.1:{port}, up {}\n",
        health.version,
        uptime(health.uptime_s)
    ))?;
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

fn logs_path(paths: &Paths) -> Result<(), Error> {
    emit(&format!(
        "{}\n",
        telemetry::log_file(&paths.logs, OffsetDateTime::now_utc().date()).display()
    ))
}

fn block_on<T>(future: impl Future<Output = store::Result<T>>) -> Result<T, Error> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(Error::Runtime)?;
    Ok(runtime.block_on(future)?)
}

fn db_schema(paths: &Paths) -> Result<(), Error> {
    let schema = block_on(async {
        let store = store::open(paths).await?;
        store::dump_schema(&store.read).await
    })?;
    emit(&schema)
}

fn db_migrate(paths: &Paths) -> Result<(), Error> {
    let count = block_on(store::migrate(paths))?;
    let plural = if count == 1 { "" } else { "s" };
    emit(&format!("applied {count} migration{plural}\n"))
}

fn db_check(paths: &Paths) -> Result<ExitCode, Error> {
    let live = block_on(async { store::dump_schema(&store::open_read_only(paths).await?).await })?;
    match schema_drift(store::SCHEMA, &live) {
        None => {
            emit("schema matches schema.sql\n")?;
            Ok(ExitCode::SUCCESS)
        }
        Some(drift) => {
            emit(&drift)?;
            Ok(ExitCode::FAILURE)
        }
    }
}

fn schema_drift(want: &str, live: &str) -> Option<String> {
    let (mut want_lines, mut live_lines) = (want.lines(), live.lines());
    for line in 1.. {
        match (want_lines.next(), live_lines.next()) {
            (None, None) => return None,
            (a, b) if a == b => {}
            (a, b) => {
                return Some(format!(
                    "schema drift at line {line}\n  schema.sql  {}\n  database    {}\n",
                    shown(a),
                    shown(b),
                ));
            }
        }
    }
    None
}

fn shown(line: Option<&str>) -> &str {
    match line {
        None => "(end)",
        Some("") => "(blank line)",
        Some(text) => text,
    }
}

fn db_sql(paths: &Paths, query: &str, json: bool) -> Result<(), Error> {
    let rows = block_on(async { store::query(&store::open_read_only(paths).await?, query).await })?;
    if json {
        emit(&format!("{:#}\n", rows.to_json()?))
    } else {
        emit(&rows_table(&rows))
    }
}

fn rows_table(rows: &store::Rows) -> String {
    if rows.columns.is_empty() {
        return String::new();
    }
    let cell = |value: &serde_json::Value| match value {
        serde_json::Value::Null => "NULL".to_owned(),
        serde_json::Value::String(text) => telemetry::one_line(text),
        other => other.to_string(),
    };
    let header: Vec<String> = rows.columns.iter().map(telemetry::one_line).collect();
    let body: Vec<Vec<String>> = rows
        .rows
        .iter()
        .map(|row| row.iter().map(cell).collect())
        .collect();
    let widths: Vec<usize> = header
        .iter()
        .enumerate()
        .map(|(i, name)| {
            body.iter()
                .map(|line| line[i].chars().count())
                .fold(name.chars().count(), usize::max)
        })
        .collect();
    let rule = widths.iter().map(|&width| "-".repeat(width)).collect();
    [header, rule]
        .iter()
        .chain(&body)
        .fold(String::new(), |mut table, line| {
            let padded: Vec<String> = line
                .iter()
                .zip(&widths)
                .map(|(text, &width)| format!("{text:<width$}"))
                .collect();
            let _ = writeln!(table, "{}", padded.join("  ").trim_end());
            table
        })
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
    fn rows_table_pads_columns_and_marks_nulls_and_newlines() {
        let rows = store::Rows {
            columns: vec!["id".into(), "no\nte".into()],
            rows: vec![
                vec![serde_json::json!(1), serde_json::json!("a\nb\r\nc")],
                vec![serde_json::json!(200), serde_json::Value::Null],
                vec![serde_json::json!(3), serde_json::json!("\t\u{1b}[31m")],
            ],
        };
        assert_eq!(
            rows_table(&rows),
            "\
id   no⏎te
---  ------------
1    a⏎b⏎c
200  NULL
3    \\x09\\x1b[31m
"
        );
    }

    #[test]
    fn schema_drift_names_the_first_differing_line() {
        assert_eq!(schema_drift("a\nb\n", "a\nb\n"), None);
        assert_eq!(
            schema_drift("a\nb\n", "a\nc\nd\n").as_deref(),
            Some("schema drift at line 2\n  schema.sql  b\n  database    c\n")
        );
        assert_eq!(
            schema_drift("a\n", "a\nb\n").as_deref(),
            Some("schema drift at line 2\n  schema.sql  (end)\n  database    b\n")
        );
        assert_eq!(
            schema_drift("a\n", "a\n\nb\n").as_deref(),
            Some("schema drift at line 2\n  schema.sql  (end)\n  database    (blank line)\n")
        );
    }

    struct Failing(io::ErrorKind);

    impl io::Write for Failing {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(self.0.into())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn closed_stdout_is_success_and_other_errors_exit_1() {
        assert!(write_out(&mut Failing(io::ErrorKind::BrokenPipe), "x").is_ok());
        let err = write_out(&mut Failing(io::ErrorKind::StorageFull), "x").err();
        assert_eq!(err.map(|e| e.exit_code()), Some(1));
    }

    #[test]
    fn uptime_keeps_the_two_largest_units() {
        for (secs, want) in [
            (0, "0s"),
            (59, "59s"),
            (60, "1m"),
            (3599, "59m"),
            (3600, "1h0m"),
            (3 * 3600 + 12 * 60 + 5, "3h12m"),
            (86_400, "1d0h"),
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

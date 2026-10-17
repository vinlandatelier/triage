use std::io::{BufRead as _, BufReader};
use std::net::SocketAddr;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use axum_test::TestServer;
use tempfile::TempDir;
use triage::config::{Config, Paths};
use triage::http::{AppState, router};
use triage::store::{self, Store};

pub type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

pub const WAIT: Duration = Duration::from_secs(10);

pub struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// `triage --data-dir <dir>` with the developer's environment kept out.
pub fn triage(dir: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_triage"));
    command
        .arg("--data-dir")
        .arg(dir)
        // Keeps a developer's `.env` out of the run.
        .current_dir(dir)
        .env_remove("TRIAGE_OTEL_ENDPOINT")
        .env_remove("OTEL_EXPORTER_OTLP_ENDPOINT");
    command
}

/// Starts `serve` from `command` and waits for `listening`. The receiver yields the daemon's
/// later stderr lines.
pub fn serve(mut command: Command) -> TestResult<(Daemon, SocketAddr, mpsc::Receiver<String>)> {
    let child = command
        .arg("serve")
        // Port 0 avoids racing parallel tests for a free port; the `listening` line names it.
        .env("TRIAGE_PORT", "0")
        .env("TRIAGE_LOG_FORMAT", "columns")
        .env("TRIAGE_LOG_LEVEL", "info")
        .stderr(Stdio::piped())
        .spawn()?;
    let mut daemon = Daemon(child);
    let stderr = daemon.0.stderr.take().ok_or("no stderr")?;
    let (send, lines) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if send.send(line).is_err() {
                break;
            }
        }
    });
    let mut seen = Vec::new();
    let listening = wait_for_line(&lines, |line| {
        seen.push(line.to_owned());
        line.contains("listening")
    })
    .ok_or_else(|| format!("daemon never listened:\n{}", seen.join("\n")))?;
    let addr = listening
        .split_whitespace()
        .find_map(|part| part.strip_prefix("addr="))
        .ok_or_else(|| format!("no addr in {listening}"))?
        .parse()?;
    Ok((daemon, addr, lines))
}

pub fn wait_for_line(
    lines: &mpsc::Receiver<String>,
    mut found: impl FnMut(&str) -> bool,
) -> Option<String> {
    let deadline = Instant::now() + WAIT;
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match lines.recv_timeout(left) {
            Ok(line) if found(&line) => return Some(line),
            Ok(_) => {}
            Err(_) => return None,
        }
    }
    None
}

/// One test's app: an in-process server over a fresh, migrated database.
#[derive(Debug)]
pub struct TestApp {
    pub server: TestServer,
    pub store: Store,
    /// Dropping it deletes the database, so it lives as long as the app.
    #[allow(dead_code, reason = "read only by tests that need the data dir path")]
    pub dir: TempDir,
}

pub async fn spawn_app() -> TestResult<TestApp> {
    let dir = tempfile::tempdir()?;
    let paths = Paths {
        data: dir.path().to_path_buf(),
        logs: dir.path().join("logs"),
        config: dir.path().join("config"),
    };
    let store = store::open(&paths).await?;
    let server = TestServer::new(router(AppState::new(
        Config::default(),
        paths,
        store.clone(),
    )));
    Ok(TestApp { server, store, dir })
}

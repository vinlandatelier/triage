use std::io::{BufRead as _, BufReader};
use std::net::{Ipv4Addr, TcpListener};
use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn free_port() -> std::io::Result<u16> {
    Ok(TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?
        .local_addr()?
        .port())
}

fn triage(dir: &Path, port: u16) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_triage"));
    command
        .arg("--data-dir")
        .arg(dir)
        // Keeps a developer's `.env` out of the run.
        .current_dir(dir)
        .env("TRIAGE_PORT", port.to_string())
        .env_remove("TRIAGE_OTEL_ENDPOINT")
        .env_remove("OTEL_EXPORTER_OTLP_ENDPOINT");
    command
}

fn status(dir: &Path, port: u16) -> std::io::Result<Output> {
    triage(dir, port).arg("status").output()
}

#[test]
fn reports_not_running_and_exits_1() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let out = status(tmp.path(), free_port()?)?;
    assert_eq!(String::from_utf8(out.stdout)?, "not running\n");
    assert_eq!(out.status.code(), Some(1));
    Ok(())
}

#[test]
fn reports_version_and_port_when_running() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let port = free_port()?;
    let mut daemon = Daemon(
        triage(tmp.path(), port)
            .arg("serve")
            .env("TRIAGE_LOG_LEVEL", "info")
            .stderr(Stdio::piped())
            .spawn()?,
    );
    let stderr = daemon.0.stderr.take().ok_or("no stderr")?;
    let (send, lines) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if line.contains("listening") {
                let _ = send.send(());
            }
        }
    });
    lines
        .recv_timeout(Duration::from_secs(10))
        .map_err(|_| "daemon never listened")?;

    let out = status(tmp.path(), port)?;
    let stdout = String::from_utf8(out.stdout)?;
    let want = format!(
        "running v{} on 127.0.0.1:{port}, up ",
        env!("CARGO_PKG_VERSION")
    );
    assert!(stdout.starts_with(&want), "{stdout}");
    assert_eq!(out.status.code(), Some(0), "{stdout}");
    Ok(())
}

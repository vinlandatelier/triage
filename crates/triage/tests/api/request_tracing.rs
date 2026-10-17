use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::{SocketAddr, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use std::{fs, thread};

use serde_json::Value;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const WAIT: Duration = Duration::from_secs(10);

struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wait_for_line(lines: &mpsc::Receiver<String>, found: impl Fn(&str) -> bool) -> Option<String> {
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

#[test]
fn one_request_id_links_response_stderr_and_jsonl() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let child = Command::new(env!("CARGO_BIN_EXE_triage"))
        .arg("--data-dir")
        .arg(tmp.path())
        .arg("serve")
        // Keeps a developer's `.env` out of the run.
        .current_dir(tmp.path())
        // Port 0 avoids racing parallel tests for a free port; the `listening` line names it.
        .env("TRIAGE_PORT", "0")
        .env("TRIAGE_LOG_FORMAT", "columns")
        .env("TRIAGE_LOG_LEVEL", "info")
        .env("TRIAGE_LOG_FILE_LEVEL", "info,triage=debug")
        .env_remove("TRIAGE_OTEL_ENDPOINT")
        .env_remove("OTEL_EXPORTER_OTLP_ENDPOINT")
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
    let listening =
        wait_for_line(&lines, |line| line.contains("listening")).ok_or("daemon never listened")?;
    let addr: SocketAddr = listening
        .split_whitespace()
        .find_map(|part| part.strip_prefix("addr="))
        .ok_or_else(|| format!("no addr in {listening}"))?
        .parse()?;

    let mut stream = TcpStream::connect(addr)?;
    stream.set_read_timeout(Some(WAIT))?;
    stream.write_all(b"GET /health?x=1 HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    let id = response
        .lines()
        .find_map(|line| line.strip_prefix("x-request-id: "))
        .ok_or_else(|| format!("no x-request-id in {response}"))?;

    let req = format!("  req={id}");
    let line = wait_for_line(&lines, |line| line.ends_with(&req))
        .ok_or("no stderr line for the request")?;
    for part in [
        " INFO  middleware  http  ",
        "method=GET",
        "path=/health ",
        "status=200",
    ] {
        assert!(line.contains(part), "{part:?} missing from {line}");
    }
    assert!(!line.contains('?'), "query leaked into {line}");
    let duration = line
        .trim_end_matches(&req)
        .rsplit("  ")
        .next()
        .unwrap_or_default();
    assert!(duration.ends_with('s'), "no duration in {line}");

    let deadline = Instant::now() + WAIT;
    let jsonl = loop {
        let logs = tmp.path().join("logs");
        let found = fs::read_dir(&logs)?
            .filter_map(|entry| fs::read_to_string(entry.ok()?.path()).ok())
            .flat_map(|text| text.lines().map(str::to_owned).collect::<Vec<_>>())
            .filter_map(|line| serde_json::from_str::<Value>(&line).ok())
            .find(|line| line["request_id"] == id);
        if let Some(line) = found {
            break line;
        }
        if Instant::now() > deadline {
            return Err("no JSONL line for the request".into());
        }
        thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(jsonl["span"], "http", "{jsonl}");
    assert_eq!(jsonl["status"], 200, "{jsonl}");
    Ok(())
}

use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::time::{Duration, Instant};
use std::{fs, thread};

use serde_json::Value;

use crate::helpers::{TestResult, WAIT, serve, triage, wait_for_line};

#[test]
fn one_request_id_links_response_stderr_and_jsonl() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let mut command = triage(tmp.path());
    command.env("TRIAGE_LOG_FILE_LEVEL", "info,triage=debug");
    let (_daemon, addr, lines) = serve(command)?;

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

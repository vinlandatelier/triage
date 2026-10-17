use std::fs;
use std::net::{Ipv4Addr, TcpListener};
use std::path::Path;

use crate::helpers::{TestResult, serve, triage};

fn jsonl(dir: &Path) -> TestResult<String> {
    let mut all = String::new();
    for entry in fs::read_dir(dir.join("logs"))? {
        all.push_str(&fs::read_to_string(entry?.path())?);
    }
    Ok(all)
}

#[test]
fn second_instance_leaves_the_running_log_alone() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let (_daemon, _addr, _lines) = serve(triage(tmp.path()))?;
    let out = triage(tmp.path())
        .arg("serve")
        .env("TRIAGE_PORT", "0")
        .output()?;
    let stderr = String::from_utf8(out.stderr)?;
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("another triage instance"), "{stderr}");
    let log = jsonl(tmp.path())?;
    assert!(!log.contains("another triage instance"), "{log}");
    Ok(())
}

#[test]
fn startup_failure_is_logged_once() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let taken = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let port = taken.local_addr()?.port();
    let out = triage(tmp.path())
        .arg("serve")
        .env("TRIAGE_PORT", port.to_string())
        .env("TRIAGE_LOG_LEVEL", "info")
        .output()?;
    let stderr = String::from_utf8(out.stderr)?;
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert_eq!(stderr.matches("already in use").count(), 1, "{stderr}");
    let log = jsonl(tmp.path())?;
    assert!(
        log.lines()
            .any(|line| line.contains("serve failed") && line.contains("already in use")),
        "{log}"
    );
    Ok(())
}

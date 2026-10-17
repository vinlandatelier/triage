use std::io::{Read as _, Write as _};
use std::net::{Ipv4Addr, TcpListener};
use std::path::Path;
use std::process::Output;
use std::thread;

use crate::helpers::{TestResult, serve, triage};

fn status(dir: &Path, port: u16) -> std::io::Result<Output> {
    triage(dir)
        .arg("status")
        .env("TRIAGE_PORT", port.to_string())
        .output()
}

#[test]
fn reports_not_running_and_exits_1() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?
        .local_addr()?
        .port();
    let out = status(tmp.path(), port)?;
    assert_eq!(String::from_utf8(out.stdout)?, "not running\n");
    assert_eq!(out.status.code(), Some(1));
    Ok(())
}

#[test]
fn another_service_on_the_port_is_not_running() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let port = listener.local_addr()?.port();
    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let _ = stream.read(&mut [0; 1024]);
            let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\n\r\n");
        }
    });
    let out = status(tmp.path(), port)?;
    assert_eq!(String::from_utf8(out.stdout)?, "not running\n");
    assert_eq!(out.status.code(), Some(1));
    Ok(())
}

#[test]
fn reports_version_and_port_when_running() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let (_daemon, addr, _lines) = serve(triage(tmp.path()))?;

    let out = status(tmp.path(), addr.port())?;
    let stdout = String::from_utf8(out.stdout)?;
    let prefix = format!(
        "running v{} on 127.0.0.1:{}, up ",
        env!("CARGO_PKG_VERSION"),
        addr.port()
    );
    let uptime = stdout
        .strip_prefix(&prefix)
        .and_then(|rest| rest.strip_suffix("s\n"))
        .ok_or_else(|| format!("unexpected output: {stdout}"))?;
    assert!(uptime.parse::<u64>().is_ok(), "{stdout}");
    assert_eq!(out.status.code(), Some(0), "{stdout}");
    Ok(())
}

//! Client side of `GET /health`, for `triage status`.

use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use serde::Deserialize;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;

use super::{Error, Result};

const TIMEOUT: Duration = Duration::from_secs(2);
// A real health body is under 100 bytes; the cap stops a foreign listener from filling memory.
const MAX_RESPONSE: u64 = 16 * 1024;

#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct Health {
    status: String,
    pub version: String,
    pub uptime_s: u64,
}

/// Asks the daemon on `127.0.0.1:port` for its health. `None` means nothing healthy answered.
pub fn probe(port: u16) -> Result<Option<Health>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(Error::Runtime)?;
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    Ok(runtime.block_on(probe_addr(addr, TIMEOUT)))
}

async fn probe_addr(addr: SocketAddr, timeout: Duration) -> Option<Health> {
    let result = tokio::time::timeout(timeout, get_health(addr))
        .await
        .unwrap_or_else(|_| Err(io::Error::from(io::ErrorKind::TimedOut)));
    result
        .inspect_err(|err| {
            tracing::debug!(
                %addr,
                error = err as &dyn std::error::Error,
                "health probe failed"
            );
        })
        .ok()
}

// No HTTP client crate is allowlisted. axum sends `/health` with a content-length, never
// chunked, so reading to EOF under `Connection: close` yields the whole body.
async fn get_health(addr: SocketAddr) -> io::Result<Health> {
    let mut stream = TcpStream::connect(addr).await?;
    stream
        .write_all(
            format!("GET /health HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n").as_bytes(),
        )
        .await?;
    let mut response = Vec::new();
    (&mut stream)
        .take(MAX_RESPONSE + 1)
        .read_to_end(&mut response)
        .await?;
    if response.len() as u64 > MAX_RESPONSE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "response is too large",
        ));
    }
    parse(&response)
}

fn parse(response: &[u8]) -> io::Result<Health> {
    let invalid = |msg: &str| io::Error::new(io::ErrorKind::InvalidData, msg.to_owned());
    let text = std::str::from_utf8(response).map_err(|_| invalid("response is not UTF-8"))?;
    let (head, body) = text
        .split_once("\r\n\r\n")
        .ok_or_else(|| invalid("response has no body"))?;
    let status = head
        .strip_prefix("HTTP/1.")
        .and_then(|rest| rest.split(' ').nth(1))
        .unwrap_or_default();
    if status != "200" {
        return Err(invalid(&format!("status {status}")));
    }
    let health: Health = serde_json::from_str(body).map_err(io::Error::from)?;
    if health.status != "ok" {
        return Err(invalid(&format!("health status {}", health.status)));
    }
    Ok(health)
}

#[cfg(test)]
mod tests {
    use tokio::net::TcpListener;

    use super::*;
    use crate::config::{Config, Paths};
    use crate::http::{AppState, router};

    type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

    #[tokio::test]
    async fn reads_version_and_uptime_from_the_daemon() -> TestResult {
        let dir = tempfile::tempdir()?;
        let paths = Paths {
            data: dir.path().into(),
            logs: dir.path().join("logs"),
            config: dir.path().join("config"),
        };
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let addr = listener.local_addr()?;
        let app = router(AppState::new(Config::default(), paths));
        let server = tokio::spawn(axum::serve(listener, app).into_future());
        let health = probe_addr(addr, TIMEOUT).await.ok_or("no health")?;
        server.abort();
        assert_eq!(health.version, env!("CARGO_PKG_VERSION"));
        assert!(health.uptime_s < 60, "{health:?}");
        Ok(())
    }

    #[tokio::test]
    async fn closed_port_is_none() -> TestResult {
        let addr = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await?
            .local_addr()?;
        assert_eq!(probe_addr(addr, TIMEOUT).await, None);
        Ok(())
    }

    #[tokio::test]
    async fn silent_listener_times_out_as_none() -> TestResult {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let addr = listener.local_addr()?;
        assert_eq!(probe_addr(addr, Duration::from_millis(50)).await, None);
        Ok(())
    }

    #[tokio::test]
    async fn oversized_response_is_none() -> TestResult {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let addr = listener.local_addr()?;
        tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                let _ = stream.write_all(b"HTTP/1.1 200 OK\r\n\r\n").await;
                let chunk = vec![b' '; 4096];
                while stream.write_all(&chunk).await.is_ok() {}
            }
        });
        assert_eq!(probe_addr(addr, TIMEOUT).await, None);
        Ok(())
    }

    #[test]
    fn other_services_are_not_triage() {
        assert!(parse(b"HTTP/1.1 404 Not Found\r\n\r\n").is_err());
        assert!(parse(b"HTTP/1.1 200 OK\r\n\r\n<html>").is_err());
        assert!(parse(b"garbage").is_err());
        assert!(
            parse(b"SIP/2.0 200 OK\r\n\r\n{\"status\":\"ok\",\"version\":\"1\",\"uptime_s\":1}")
                .is_err()
        );
        assert!(
            parse(b"HTTP/1.1 200 OK\r\n\r\n{\"status\":\"down\",\"version\":\"1\",\"uptime_s\":1}")
                .is_err()
        );
    }

    #[test]
    fn parses_a_health_body() -> TestResult {
        let health = parse(
            b"HTTP/1.1 200 OK\r\ncontent-length: 46\r\n\r\n{\"status\":\"ok\",\"version\":\"1.2.3\",\"uptime_s\":9}",
        )?;
        assert_eq!(
            health,
            Health {
                status: "ok".into(),
                version: "1.2.3".into(),
                uptime_s: 9
            }
        );
        Ok(())
    }
}

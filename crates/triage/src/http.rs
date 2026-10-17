//! The daemon: a localhost HTTP server guarded by a lock on the data dir.

use std::fs::{File, OpenOptions, TryLockError};
use std::future::Future;
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::time::Duration;

use axum::Router;
use time::OffsetDateTime;
use tokio::net::TcpListener;

use crate::config::{Config, Paths};
use crate::telemetry;

mod router;
mod state;

pub use router::router;
pub use state::AppState;

const DRAIN: Duration = Duration::from_secs(10);

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("another triage instance is using {}", .0.display())]
    Locked(PathBuf),
    #[error("cannot lock {}: {source}", path.display())]
    Lock { path: PathBuf, source: io::Error },
    #[error("port {0} is already in use; set TRIAGE_PORT to a free port")]
    PortInUse(u16),
    #[error("cannot listen on {addr}: {source}")]
    Bind { addr: SocketAddr, source: io::Error },
    #[error("cannot start the async runtime: {0}")]
    Runtime(io::Error),
    #[error("server failed: {0}")]
    Serve(io::Error),
}

impl Error {
    pub fn exit_code(&self) -> u8 {
        1
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Runs in the foreground until SIGINT or SIGTERM, then drains for up to 10 s.
pub fn serve(config: Config, paths: Paths) -> Result<()> {
    // The OS drops the lock when the process dies, so a crash never leaves it stale.
    let _lock = lock(&paths.lock_file(), &paths.data)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(Error::Runtime)?;
    let result = runtime.block_on(async {
        let shutdown = shutdown_signal();
        let (listener, addr) = bind(config.port).await?;
        tracing::info!(
            version = env!("CARGO_PKG_VERSION"),
            %addr,
            data = %paths.data.display(),
            log = %telemetry::log_file(&paths.logs, OffsetDateTime::now_utc().date()).display(),
            "listening"
        );
        let state = AppState::new(config, paths);
        let started = state.started;
        let result = serve_until(listener, router(state), shutdown, DRAIN).await;
        tracing::info!(uptime_s = started.elapsed().as_secs(), "stopped");
        result
    });
    // A plain drop waits forever on blocking tasks, which would break the drain cap.
    runtime.shutdown_timeout(Duration::from_secs(1));
    result
}

fn lock(path: &Path, data: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
        .map_err(|source| Error::Lock {
            path: path.to_path_buf(),
            source,
        })?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(TryLockError::WouldBlock) => Err(Error::Locked(data.to_path_buf())),
        Err(TryLockError::Error(source)) => Err(Error::Lock {
            path: path.to_path_buf(),
            source,
        }),
    }
}

async fn bind(port: u16) -> Result<(TcpListener, SocketAddr)> {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let listener = TcpListener::bind(addr).await.map_err(|source| {
        if source.kind() == io::ErrorKind::AddrInUse {
            Error::PortInUse(port)
        } else {
            Error::Bind { addr, source }
        }
    })?;
    // Differs from `addr` when the port is 0.
    let bound = listener
        .local_addr()
        .map_err(|source| Error::Bind { addr, source })?;
    Ok((listener, bound))
}

/// Stops accepting once `shutdown` resolves, then waits up to `drain` for open requests.
async fn serve_until(
    listener: TcpListener,
    router: Router,
    shutdown: impl Future<Output = ()> + Send + 'static,
    drain: Duration,
) -> Result<()> {
    let (signalled, on_signal) = tokio::sync::oneshot::channel();
    let server = axum::serve(listener, router).with_graceful_shutdown(async move {
        shutdown.await;
        tracing::info!(drain_s = drain.as_secs(), "shutting down");
        let _ = signalled.send(());
    });
    let deadline = async move {
        // A dropped sender means the server ended on its own; the select takes that branch.
        if on_signal.await.is_ok() {
            tokio::time::sleep(drain).await;
        } else {
            std::future::pending::<()>().await;
        }
    };
    tokio::select! {
        result = server => result.map_err(Error::Serve),
        () = deadline => {
            tracing::warn!("open requests did not finish within the drain window");
            Ok(())
        }
    }
}

/// Registers the handlers before the future is polled: axum only polls it after it starts
/// accepting, and a SIGTERM in that gap would kill the process without a drain.
#[cfg(unix)]
fn shutdown_signal() -> impl Future<Output = ()> + Send + 'static {
    use tokio::signal::unix::{Signal, SignalKind, signal};

    fn listen(kind: SignalKind, name: &'static str) -> Option<Signal> {
        signal(kind)
            .inspect_err(|err| {
                tracing::warn!(
                    error = err as &dyn std::error::Error,
                    signal = name,
                    "cannot listen for signal"
                );
            })
            .ok()
    }

    async fn recv(signal: Option<Signal>) {
        match signal {
            Some(mut signal) => {
                signal.recv().await;
            }
            None => std::future::pending().await,
        }
    }

    let interrupt = listen(SignalKind::interrupt(), "SIGINT");
    let terminate = listen(SignalKind::terminate(), "SIGTERM");
    async move {
        tokio::select! {
            () = recv(interrupt) => {}
            () = recv(terminate) => {}
        }
    }
}

#[cfg(not(unix))]
fn shutdown_signal() -> impl Future<Output = ()> + Send + 'static {
    async {
        if let Err(err) = tokio::signal::ctrl_c().await {
            tracing::warn!(
                error = &err as &dyn std::error::Error,
                "cannot listen for ctrl-c"
            );
            std::future::pending::<()>().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::routing::get;
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    use tokio::net::TcpStream;

    use tokio::sync::{Notify, oneshot};

    use super::*;

    type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

    async fn get_path(addr: SocketAddr, path: &str) -> io::Result<String> {
        let mut stream = TcpStream::connect(addr).await?;
        stream
            .write_all(
                format!("GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").as_bytes(),
            )
            .await?;
        let mut response = String::new();
        stream.read_to_string(&mut response).await?;
        Ok(response)
    }

    async fn local_listener() -> io::Result<(TcpListener, SocketAddr)> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let addr = listener.local_addr()?;
        Ok((listener, addr))
    }

    fn state(dir: &Path) -> AppState {
        AppState::new(
            Config::default(),
            Paths {
                data: dir.to_path_buf(),
                logs: dir.join("logs"),
                config: dir.join("config"),
            },
        )
    }

    #[tokio::test]
    async fn health_reports_status_version_and_uptime() -> TestResult {
        let tmp = tempfile::tempdir()?;
        let (listener, addr) = local_listener().await?;
        let (stop, stopped) = oneshot::channel::<()>();
        let server = tokio::spawn(serve_until(
            listener,
            router(state(tmp.path())),
            async {
                let _ = stopped.await;
            },
            DRAIN,
        ));
        let response = get_path(addr, "/health").await?;
        let _ = stop.send(());
        server.await??;

        let (head, body) = response.split_once("\r\n\r\n").ok_or("no body")?;
        assert!(head.starts_with("HTTP/1.1 200"), "{head}");
        assert!(head.contains("content-type: application/json"), "{head}");
        let body: serde_json::Value = serde_json::from_str(body)?;
        assert_eq!(body["status"], "ok");
        assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
        assert!(body["uptime_s"].is_u64(), "{body}");
        Ok(())
    }

    #[tokio::test]
    async fn shutdown_lets_a_slow_request_finish() -> TestResult {
        let (listener, addr) = local_listener().await?;
        let entered = Arc::new(Notify::new());
        let notify = Arc::clone(&entered);
        let slow = Router::new().route(
            "/slow",
            get(move || {
                notify.notify_one();
                async {
                    tokio::time::sleep(Duration::from_millis(300)).await;
                    "done"
                }
            }),
        );
        let (stop, stopped) = oneshot::channel::<()>();
        let server = tokio::spawn(serve_until(
            listener,
            slow,
            async {
                let _ = stopped.await;
            },
            DRAIN,
        ));
        let request = tokio::spawn(get_path(addr, "/slow"));
        entered.notified().await;
        let _ = stop.send(());

        let response = request.await??;
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(response.ends_with("done"), "{response}");
        server.await??;
        Ok(())
    }

    #[tokio::test]
    async fn drain_window_caps_shutdown() -> TestResult {
        let (listener, addr) = local_listener().await?;
        let entered = Arc::new(Notify::new());
        let notify = Arc::clone(&entered);
        let hang = Router::new().route(
            "/hang",
            get(move || {
                notify.notify_one();
                std::future::pending::<&str>()
            }),
        );
        let drain = Duration::from_millis(50);
        let (stop, stopped) = oneshot::channel::<()>();
        let server = tokio::spawn(serve_until(
            listener,
            hang,
            async {
                let _ = stopped.await;
            },
            drain,
        ));
        let _request = tokio::spawn(get_path(addr, "/hang"));
        entered.notified().await;
        let stopping = std::time::Instant::now();
        let _ = stop.send(());
        tokio::time::timeout(Duration::from_secs(5), server).await???;
        assert!(stopping.elapsed() >= drain, "{:?}", stopping.elapsed());
        Ok(())
    }

    #[test]
    fn second_lock_names_the_data_dir() -> TestResult {
        let tmp = tempfile::tempdir()?;
        let path = tmp.path().join("triage.lock");
        let first = lock(&path, tmp.path())?;

        let Err(err) = lock(&path, tmp.path()) else {
            return Err("second lock must fail".into());
        };
        assert!(matches!(err, Error::Locked(_)), "{err:?}");
        assert_eq!(err.exit_code(), 1);
        assert_eq!(
            err.to_string(),
            format!("another triage instance is using {}", tmp.path().display())
        );

        drop(first);
        lock(&path, tmp.path())?;
        Ok(())
    }

    #[tokio::test]
    async fn port_in_use_names_the_port_and_var() -> TestResult {
        let (_held, addr) = local_listener().await?;
        let Err(err) = bind(addr.port()).await.map(|_| ()) else {
            return Err("second bind must fail".into());
        };
        assert_eq!(err.exit_code(), 1);
        let message = err.to_string();
        assert!(message.contains(&addr.port().to_string()), "{message}");
        assert!(message.contains("TRIAGE_PORT"), "{message}");
        Ok(())
    }
}

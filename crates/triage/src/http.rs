//! The daemon: a localhost HTTP server guarded by a lock on the data dir.

use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::time::Duration;

use axum::Router;
use time::OffsetDateTime;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};

use crate::config::{Config, Paths};
use crate::{store, telemetry};

mod assets;
mod json;
mod middleware;
mod openapi;
mod probe;
mod router;
mod state;

pub use json::Json;
pub use openapi::{api_doc, api_json};
pub use probe::{Health, probe};
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
    #[error(transparent)]
    Store(#[from] store::Error),
}

impl Error {
    pub fn exit_code(&self) -> u8 {
        1
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Runs in the foreground until SIGINT or SIGTERM, then drains for up to 10 s. A second
/// signal during the drain exits at once.
pub fn serve(config: Config, paths: Paths) -> Result<()> {
    // The OS drops the lock when the process dies, so a crash never leaves it stale.
    let _lock = lock(&paths.lock_file(), &paths.data)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(Error::Runtime)?;
    let result = runtime.block_on(async {
        let signals = shutdown_signals();
        let store = store::open(&paths).await?;
        let (listener, addr) = bind(config.port).await?;
        tracing::info!(
            version = env!("CARGO_PKG_VERSION"),
            %addr,
            data = %paths.data.display(),
            db = %paths.db_file().display(),
            log = %telemetry::log_file(&paths.logs, OffsetDateTime::now_utc().date()).display(),
            "listening"
        );
        let state = AppState::new(config, paths, store.clone());
        let started = state.started;
        let result = serve_until(listener, router(state), signals, DRAIN).await;
        // A cut drain leaves requests holding connections, and `close` would wait for them.
        if matches!(result, Ok(Stopped::Drained)) {
            store.close().await;
        }
        tracing::info!(uptime_s = started.elapsed().as_secs(), "stopped");
        result.map(drop)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stopped {
    /// Every open request finished.
    Drained,
    /// The drain window ran out or a second signal arrived.
    Cut,
}

/// Stops accepting on the first message from `signals`, then waits up to `drain` for open
/// requests, or until the next message.
async fn serve_until(
    listener: TcpListener,
    router: Router,
    mut signals: mpsc::UnboundedReceiver<()>,
    drain: Duration,
) -> Result<Stopped> {
    let (signalled, on_signal) = oneshot::channel();
    let server = axum::serve(listener, router).with_graceful_shutdown(async move {
        if signals.recv().await.is_none() {
            std::future::pending::<()>().await;
        }
        tracing::info!(drain_s = drain.as_secs(), "shutting down");
        let _ = signalled.send(signals);
    });
    let deadline = async move {
        // A dropped sender means the server ended on its own; the select takes that branch.
        let Ok(mut signals) = on_signal.await else {
            return std::future::pending::<()>().await;
        };
        tokio::select! {
            () = tokio::time::sleep(drain) => {
                tracing::warn!("open requests did not finish within the drain window");
            }
            Some(()) = signals.recv() => {
                tracing::warn!("second signal: exiting without waiting for open requests");
            }
        }
    };
    tokio::select! {
        result = server => result.map(|()| Stopped::Drained).map_err(Error::Serve),
        () = deadline => Ok(Stopped::Cut),
    }
}

/// Registers the handlers before axum polls anything: it only polls the shutdown future
/// after it starts accepting, and a SIGTERM in that gap would kill the process without a
/// drain. Each SIGINT or SIGTERM sends one message.
#[cfg(unix)]
fn shutdown_signals() -> mpsc::UnboundedReceiver<()> {
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

    async fn recv(signal: &mut Option<Signal>) {
        let received = match signal {
            Some(signal) => signal.recv().await.is_some(),
            None => false,
        };
        if !received {
            std::future::pending::<()>().await;
        }
    }

    let mut interrupt = listen(SignalKind::interrupt(), "SIGINT");
    let mut terminate = listen(SignalKind::terminate(), "SIGTERM");
    let (send, receive) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                () = recv(&mut interrupt) => {}
                () = recv(&mut terminate) => {}
            }
            if send.send(()).is_err() {
                break;
            }
        }
    });
    receive
}

#[cfg(not(unix))]
fn shutdown_signals() -> mpsc::UnboundedReceiver<()> {
    let (send, receive) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        loop {
            if let Err(err) = tokio::signal::ctrl_c().await {
                tracing::warn!(
                    error = &err as &dyn std::error::Error,
                    "cannot listen for ctrl-c"
                );
                break;
            }
            if send.send(()).is_err() {
                break;
            }
        }
    });
    receive
}

#[cfg(test)]
mod tests {
    use axum::routing::get;
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    use tokio::net::TcpStream;

    use tokio::sync::Notify;

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

    async fn state(dir: &Path) -> store::Result<AppState> {
        let paths = Paths {
            data: dir.to_path_buf(),
            logs: dir.join("logs"),
            config: dir.join("config"),
        };
        let store = store::open(&paths).await?;
        Ok(AppState::new(Config::default(), paths, store))
    }

    #[tokio::test]
    async fn health_reports_status_version_and_uptime() -> TestResult {
        let tmp = tempfile::tempdir()?;
        let (listener, addr) = local_listener().await?;
        let (stop, signals) = mpsc::unbounded_channel();
        let server = tokio::spawn(serve_until(
            listener,
            router(state(tmp.path()).await?),
            signals,
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
        let (stop, signals) = mpsc::unbounded_channel();
        let server = tokio::spawn(serve_until(listener, slow, signals, DRAIN));
        let request = tokio::spawn(get_path(addr, "/slow"));
        entered.notified().await;
        let _ = stop.send(());

        let response = request.await??;
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(response.ends_with("done"), "{response}");
        assert_eq!(server.await??, Stopped::Drained);
        Ok(())
    }

    fn hanging() -> (Router, Arc<Notify>) {
        let entered = Arc::new(Notify::new());
        let notify = Arc::clone(&entered);
        let hang = Router::new().route(
            "/hang",
            get(move || {
                notify.notify_one();
                std::future::pending::<&str>()
            }),
        );
        (hang, entered)
    }

    #[tokio::test]
    async fn drain_window_caps_shutdown() -> TestResult {
        let (listener, addr) = local_listener().await?;
        let (hang, entered) = hanging();
        let drain = Duration::from_millis(50);
        let (stop, signals) = mpsc::unbounded_channel();
        let server = tokio::spawn(serve_until(listener, hang, signals, drain));
        let _request = tokio::spawn(get_path(addr, "/hang"));
        entered.notified().await;
        let stopping = std::time::Instant::now();
        let _ = stop.send(());
        let stopped = tokio::time::timeout(Duration::from_secs(5), server).await???;
        assert_eq!(stopped, Stopped::Cut);
        assert!(stopping.elapsed() >= drain, "{:?}", stopping.elapsed());
        Ok(())
    }

    #[tokio::test]
    async fn second_signal_cuts_the_drain() -> TestResult {
        let (listener, addr) = local_listener().await?;
        let (hang, entered) = hanging();
        let (stop, signals) = mpsc::unbounded_channel();
        let server = tokio::spawn(serve_until(listener, hang, signals, DRAIN));
        let _request = tokio::spawn(get_path(addr, "/hang"));
        entered.notified().await;
        let _ = stop.send(());
        let _ = stop.send(());
        let stopped = tokio::time::timeout(Duration::from_secs(5), server).await???;
        assert_eq!(stopped, Stopped::Cut);
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

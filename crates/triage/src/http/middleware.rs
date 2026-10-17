//! Request ids and the per-request `http` span.

use std::time::Duration;

use axum::Router;
use axum::extract::Request;
use axum::http::{HeaderName, HeaderValue};
use axum::response::Response;
use tower_http::request_id::{
    MakeRequestId, PropagateRequestIdLayer, RequestId, SetRequestIdLayer,
};
use tower_http::trace::TraceLayer;
use tracing::Span;
use uuid::Uuid;

const X_REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");

/// Longest incoming id kept; anything longer is replaced.
const MAX_INCOMING_ID: usize = 128;

/// Gives every request an `x-request-id` (kept if the client sent a usable one), a span
/// carrying it, and the same header on the response.
pub fn trace(router: Router) -> Router {
    // The last `layer` call is outermost, so the id is set before the span reads it.
    router
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(make_span)
                .on_request(())
                .on_eos(())
                .on_response(on_response)
                // The span already records a 5xx `status`; the default adds a second ERROR line.
                .on_failure(()),
        )
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuidV7))
        .layer(axum::middleware::map_request(drop_unusable_request_id))
}

/// `SetRequestIdLayer` keeps any incoming header, so an empty, oversized, or non-ASCII id
/// would reach the logs and response. Spaces are rejected so an id cannot fake extra
/// columns in the stderr line.
async fn drop_unusable_request_id(mut request: Request) -> Request {
    let usable = request.headers().get(X_REQUEST_ID).is_none_or(|value| {
        let bytes = value.as_bytes();
        !bytes.is_empty()
            && bytes.len() <= MAX_INCOMING_ID
            && bytes.iter().all(u8::is_ascii_graphic)
    });
    if !usable {
        request.headers_mut().remove(X_REQUEST_ID);
    }
    request
}

/// Version 7 UUIDs, so ids sort by arrival.
#[derive(Debug, Clone, Copy, Default)]
pub struct MakeRequestUuidV7;

impl MakeRequestId for MakeRequestUuidV7 {
    fn make_request_id<B>(&mut self, _: &axum::http::Request<B>) -> Option<RequestId> {
        HeaderValue::try_from(Uuid::now_v7().to_string())
            .ok()
            .map(RequestId::new)
    }
}

fn make_span(request: &Request) -> Span {
    let request_id = request
        .extensions()
        .get::<RequestId>()
        .and_then(|id| id.header_value().to_str().ok())
        .unwrap_or_default();
    tracing::info_span!(
        "http",
        method = %request.method(),
        path = request.uri().path(),
        request_id,
        status = tracing::field::Empty,
    )
}

fn on_response(response: &Response, latency: Duration, span: &Span) {
    let status = response.status().as_u16();
    span.record("status", status);
    // Debug keeps stderr to the span's close line while the default file filter still gets one line per request.
    tracing::debug!(
        status,
        latency_ms = latency.as_secs_f64() * 1000.0,
        "response"
    );
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::net::{Ipv4Addr, SocketAddr};
    use std::sync::{Arc, Mutex};

    use axum::routing::get;
    use serde_json::Value;
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    use tokio::net::{TcpListener, TcpStream};
    use tracing_subscriber::layer::SubscriberExt as _;

    use super::*;
    use crate::telemetry;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<Vec<u8>>>);

    impl io::Write for Capture {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0
                .lock()
                .map_err(|_| io::Error::other("poisoned"))?
                .extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    async fn send(addr: SocketAddr, head: &str) -> io::Result<String> {
        let exchange = async {
            let mut stream = TcpStream::connect(addr).await?;
            stream
                .write_all(format!("{head}\r\nHost: x\r\nConnection: close\r\n\r\n").as_bytes())
                .await?;
            let mut response = String::new();
            stream.read_to_string(&mut response).await?;
            Ok(response)
        };
        tokio::time::timeout(Duration::from_secs(10), exchange).await?
    }

    fn header<'a>(response: &'a str, name: &str) -> Option<&'a str> {
        response
            .lines()
            .take_while(|line| !line.is_empty())
            .find_map(|line| line.strip_prefix(&format!("{name}: ")))
    }

    /// Serves `router` behind [`trace`] on a current-thread runtime, so the
    /// thread-local subscriber sees every request.
    async fn serve_one(
        router: Router,
        head: &str,
    ) -> Result<(String, Vec<Value>), Box<dyn std::error::Error>> {
        let capture = Capture::default();
        let writer = capture.clone();
        let subscriber =
            tracing_subscriber::registry().with(telemetry::file_layer(move || writer.clone()));
        let _default = tracing::subscriber::set_default(subscriber);

        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let addr = listener.local_addr()?;
        let server = tokio::spawn(axum::serve(listener, trace(router)).into_future());
        let response = send(addr, head).await?;
        server.abort();

        let bytes = capture.0.lock().map_err(|_| "poisoned")?.clone();
        let lines = String::from_utf8(bytes)?
            .lines()
            .map(serde_json::from_str)
            .collect::<Result<_, _>>()?;
        Ok((response, lines))
    }

    #[tokio::test(flavor = "current_thread")]
    async fn handler_events_carry_the_response_request_id() -> TestResult {
        let router = Router::new().route(
            "/work",
            get(|| async {
                tracing::info!("working");
                "ok"
            }),
        );
        let (response, lines) = serve_one(router, "GET /work?q=1 HTTP/1.1").await?;

        let id = header(&response, "x-request-id").ok_or(response.clone())?;
        assert_eq!(Uuid::parse_str(id)?.get_version_num(), 7);
        let [working, done] = lines.as_slice() else {
            return Err(format!("want two lines, got {lines:?}").into());
        };
        assert_eq!(working["message"], "working");
        assert_eq!(done["message"], "response");
        assert_eq!(done["status"], 200);
        assert!(done["latency_ms"].is_f64(), "{done}");
        for line in [working, done] {
            assert_eq!(line["request_id"], id, "{line}");
            assert_eq!(line["span"], "http", "{line}");
        }
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn incoming_request_id_is_kept() -> TestResult {
        let (response, lines) = serve_one(
            Router::new(),
            "GET /missing HTTP/1.1\r\nx-request-id: from-client",
        )
        .await?;
        assert_eq!(header(&response, "x-request-id"), Some("from-client"));
        let done = lines.last().ok_or("no line")?;
        assert_eq!(done["request_id"], "from-client");
        assert_eq!(done["status"], 404);
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn unusable_incoming_request_id_is_replaced() -> TestResult {
        let long = "a".repeat(MAX_INCOMING_ID + 1);
        for sent in ["", "x  status=500  req=y", "caf\u{e9}", long.as_str()] {
            let (response, lines) = serve_one(
                Router::new(),
                &format!("GET /missing HTTP/1.1\r\nx-request-id: {sent}"),
            )
            .await?;
            let id = header(&response, "x-request-id").ok_or(response.clone())?;
            assert_eq!(Uuid::parse_str(id)?.get_version_num(), 7, "{sent:?}");
            let done = lines.last().ok_or("no line")?;
            assert_eq!(done["request_id"], id, "{sent:?}");
        }
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn server_error_logs_one_line() -> TestResult {
        let router = Router::new().route(
            "/fail",
            get(|| async { axum::http::StatusCode::INTERNAL_SERVER_ERROR }),
        );
        let (_, lines) = serve_one(router, "GET /fail HTTP/1.1").await?;
        let [done] = lines.as_slice() else {
            return Err(format!("want one line, got {lines:?}").into());
        };
        assert_eq!(done["message"], "response");
        assert_eq!(done["status"], 500);
        Ok(())
    }
}

//! Request ids, the per-request `http` span, and problem+json error responses.

use std::any::Any;
use std::time::Duration;

use axum::Router;
use axum::extract::{DefaultBodyLimit, Request};
use axum::http::header::{CONTENT_LENGTH, CONTENT_TYPE};
use axum::http::{HeaderName, HeaderValue, StatusCode, Uri};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse as _, Response};
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::classify::ServerErrorsFailureClass;
use tower_http::request_id::{
    MakeRequestId, PropagateRequestIdLayer, RequestId, SetRequestIdLayer,
};
use tower_http::trace::TraceLayer;
use tracing::Span;
use uuid::Uuid;

use crate::error::{AppError, BODY_LIMIT, Problem};

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
                .on_failure(on_failure),
        )
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuidV7))
        .layer(middleware::map_request(drop_unusable_request_id))
}

/// `SetRequestIdLayer` keeps any incoming header, so an unusable id would reach the logs
/// and response. Spaces could fake extra columns in the stderr line, and anything outside
/// URI-unreserved characters could not round-trip as the problem `instance`.
async fn drop_unusable_request_id(mut request: Request) -> Request {
    let usable = request.headers().get(X_REQUEST_ID).is_none_or(|value| {
        let bytes = value.as_bytes();
        bytes.first().is_some_and(u8::is_ascii_alphanumeric)
            && bytes.len() <= MAX_INCOMING_ID
            && bytes
                .iter()
                .all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(b))
    });
    if !usable {
        request.headers_mut().remove(X_REQUEST_ID);
    }
    request
}

/// Caps extractor bodies, turns panics into 500s, and adds the request id as `instance` to
/// every problem. Must sit inside [`trace`], which sets the id.
pub fn problems(router: Router) -> Router {
    router
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .layer(CatchPanicLayer::custom(on_panic))
        .layer(middleware::from_fn(render_problem))
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "CatchPanicLayer hands over the payload by value"
)]
fn on_panic(payload: Box<dyn Any + Send + 'static>) -> Response {
    let message = payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("non-string panic payload");
    AppError::Internal(anyhow::anyhow!("handler panicked: {message}")).into_response()
}

async fn render_problem(request: Request, next: Next) -> Response {
    let instance = request
        .extensions()
        .get::<RequestId>()
        .and_then(|id| Uri::try_from(id.header_value().as_bytes()).ok());
    let mut response = next.run(request).await;
    let problem = match response.extensions_mut().remove::<Problem>() {
        Some(problem) => problem,
        None if is_body_limit_rejection(&response) => AppError::PayloadTooLarge.problem(),
        None => return response,
    };
    let problem = match instance {
        Some(instance) => problem.with_instance(instance),
        None => problem,
    };
    // Swap only the body so headers the handler set (`retry-after`, `allow`) survive.
    let (mut parts, _) = response.into_parts();
    let (rendered, body) = problem.into_response().into_parts();
    parts.headers.remove(CONTENT_LENGTH);
    parts.headers.extend(rendered.headers);
    Response::from_parts(parts, body)
}

/// axum answers an over-limit body with a plain-text 413; a handler's own 413 is left alone.
fn is_body_limit_rejection(response: &Response) -> bool {
    response.status() == StatusCode::PAYLOAD_TOO_LARGE
        && response
            .headers()
            .get(CONTENT_TYPE)
            .is_some_and(|value| value.as_bytes().starts_with(b"text/plain"))
}

/// Unknown `/api` paths answer with a problem; the rest stay free for the web UI.
pub async fn fallback(uri: Uri) -> Response {
    let path = uri.path();
    if path == "/api" || path.starts_with("/api/") {
        AppError::NotFound(format!("no route for {path}")).into_response()
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
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
    let latency_ms = latency.as_secs_f64() * 1000.0;
    // Debug keeps stderr to the span's close line while the default file filter still gets
    // one line per request. A 5xx is raised to warn so it survives a quieter filter.
    if response.status().is_server_error() {
        tracing::warn!(status, latency_ms, "response");
    } else {
        tracing::debug!(status, latency_ms, "response");
    }
}

/// `on_response` already covers 5xx statuses, so only body and stream errors log here.
fn on_failure(failure: ServerErrorsFailureClass, latency: Duration, _: &Span) {
    if let ServerErrorsFailureClass::Error(error) = failure {
        tracing::error!(
            error,
            latency_ms = latency.as_secs_f64() * 1000.0,
            "response failed"
        );
    }
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

    async fn send(addr: SocketAddr, head: &str, body: &[u8]) -> io::Result<String> {
        let exchange = async {
            let mut stream = TcpStream::connect(addr).await?;
            stream
                .write_all(format!("{head}\r\nHost: x\r\nConnection: close\r\n\r\n").as_bytes())
                .await?;
            stream.write_all(body).await?;
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
        let (mut responses, lines) = serve_all(router, &[(head, b"")]).await?;
        Ok((responses.pop().ok_or("no response")?, lines))
    }

    async fn serve_all(
        router: Router,
        requests: &[(&str, &[u8])],
    ) -> Result<(Vec<String>, Vec<Value>), Box<dyn std::error::Error>> {
        let capture = Capture::default();
        let writer = capture.clone();
        let subscriber =
            tracing_subscriber::registry().with(telemetry::file_layer(move || writer.clone()));
        let _default = tracing::subscriber::set_default(subscriber);

        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let addr = listener.local_addr()?;
        let server = tokio::spawn(axum::serve(listener, trace(router)).into_future());
        let mut responses = Vec::new();
        for (head, body) in requests {
            responses.push(send(addr, head, body).await?);
        }
        server.abort();

        let bytes = capture.0.lock().map_err(|_| "poisoned")?.clone();
        let lines = String::from_utf8(bytes)?
            .lines()
            .map(serde_json::from_str)
            .collect::<Result<_, _>>()?;
        Ok((responses, lines))
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
        for sent in [
            "",
            "x  status=500  req=y",
            "caf\u{e9}",
            "a\"b",
            "http://evil.example/x",
            "-lead",
            long.as_str(),
        ] {
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
        let router =
            Router::new().route("/fail", get(|| async { StatusCode::INTERNAL_SERVER_ERROR }));
        let (_, lines) = serve_one(router, "GET /fail HTTP/1.1").await?;
        let [done] = lines.as_slice() else {
            return Err(format!("want one line, got {lines:?}").into());
        };
        assert_eq!(done["message"], "response");
        assert_eq!(done["level"], "WARN");
        assert_eq!(done["status"], 500);
        Ok(())
    }

    fn problem_body(response: &str) -> Result<Value, Box<dyn std::error::Error>> {
        assert_eq!(
            header(response, "content-type"),
            Some("application/problem+json"),
            "{response}"
        );
        let (_, body) = response.split_once("\r\n\r\n").ok_or("no body")?;
        Ok(serde_json::from_str(body)?)
    }

    #[tokio::test(flavor = "current_thread")]
    async fn not_found_instance_is_the_request_id() -> TestResult {
        let router = Router::new().route(
            "/item",
            get(|| async { Err::<(), _>(AppError::NotFound("no item 7".into())) }),
        );
        let (response, _) = serve_one(problems(router), "GET /item HTTP/1.1").await?;
        assert!(response.starts_with("HTTP/1.1 404"), "{response}");
        let id = header(&response, "x-request-id").ok_or("no id")?;
        let body = problem_body(&response)?;
        assert_eq!(body["instance"], id, "{body}");
        assert_eq!(body["detail"], "no item 7", "{body}");
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn kept_incoming_id_is_the_instance() -> TestResult {
        let router = Router::new().fallback(fallback);
        for sent in [
            "from-client",
            "A1-b.c_d~e",
            "0190a0b1-7c2d-7e3f-8a4b-5c6d7e8f9a0b",
        ] {
            let (response, _) = serve_one(
                problems(router.clone()),
                &format!("GET /api/x HTTP/1.1\r\nx-request-id: {sent}"),
            )
            .await?;
            assert_eq!(header(&response, "x-request-id"), Some(sent), "{response}");
            assert_eq!(problem_body(&response)?["instance"], sent, "{response}");
        }
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn problem_keeps_handler_headers() -> TestResult {
        let router = Router::new().route(
            "/busy",
            get(|| async {
                Err::<(), _>((
                    [("retry-after", "5")],
                    AppError::Conflict("try later".into()),
                ))
            }),
        );
        let (response, _) = serve_one(problems(router), "GET /busy HTTP/1.1").await?;
        assert!(response.starts_with("HTTP/1.1 409"), "{response}");
        assert_eq!(header(&response, "retry-after"), Some("5"), "{response}");
        assert!(problem_body(&response)?["instance"].is_string());
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn handler_413_is_left_alone() -> TestResult {
        let router = Router::new().route(
            "/quota",
            get(|| async { (StatusCode::PAYLOAD_TOO_LARGE, axum::Json("quota exceeded")) }),
        );
        let (response, _) = serve_one(problems(router), "GET /quota HTTP/1.1").await?;
        assert!(response.starts_with("HTTP/1.1 413"), "{response}");
        assert!(response.ends_with("\"quota exceeded\""), "{response}");
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn internal_logs_the_chain_under_the_request_id() -> TestResult {
        let router = Router::new().route(
            "/boom",
            get(|| async {
                let err = anyhow::anyhow!("disk full").context("cannot save");
                Err::<(), _>(AppError::Internal(err))
            }),
        );
        let (response, lines) = serve_one(problems(router), "GET /boom HTTP/1.1").await?;
        assert!(response.starts_with("HTTP/1.1 500"), "{response}");
        let id = header(&response, "x-request-id").ok_or("no id")?;
        let body = problem_body(&response)?;
        assert_eq!(body["instance"], id, "{body}");
        assert!(body.get("detail").is_none(), "{body}");
        assert!(!response.contains("disk full"), "{response}");

        let logged = lines
            .iter()
            .find(|line| line["message"] == "internal error")
            .ok_or("no error line")?;
        assert_eq!(logged["level"], "ERROR", "{logged}");
        assert_eq!(logged["error"], "cannot save: disk full", "{logged}");
        assert_eq!(logged["request_id"], id, "{logged}");
        Ok(())
    }

    async fn kaboom() -> &'static str {
        panic!("kaboom")
    }

    #[tokio::test(flavor = "current_thread")]
    async fn panic_returns_a_problem_and_the_server_keeps_serving() -> TestResult {
        let router = Router::new()
            .route("/panic", get(kaboom))
            .route("/ok", get(|| async { "ok" }));
        let (responses, lines) = serve_all(
            problems(router),
            &[("GET /panic HTTP/1.1", b""), ("GET /ok HTTP/1.1", b"")],
        )
        .await?;
        let [panicked, ok] = responses.as_slice() else {
            return Err("want two responses".into());
        };
        assert!(panicked.starts_with("HTTP/1.1 500"), "{panicked}");
        let id = header(panicked, "x-request-id").ok_or("no id")?;
        assert_eq!(problem_body(panicked)?["instance"], id);
        assert!(ok.starts_with("HTTP/1.1 200"), "{ok}");
        assert!(
            lines
                .iter()
                .any(|line| line["error"] == "handler panicked: kaboom" && line["request_id"] == id),
            "{lines:?}"
        );
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn oversized_body_is_a_413_problem() -> TestResult {
        let router = Router::new().route(
            "/upload",
            axum::routing::post(|body: axum::body::Bytes| async move { body.len().to_string() }),
        );
        let big = vec![b'x'; BODY_LIMIT + 1];
        let head = format!("POST /upload HTTP/1.1\r\nContent-Length: {}", big.len());
        let (responses, _) = serve_all(problems(router), &[(&head, &big)]).await?;
        let response = responses.first().ok_or("no response")?;
        assert!(response.starts_with("HTTP/1.1 413"), "{response}");
        let body = problem_body(response)?;
        assert_eq!(
            body["type"],
            "https://triage.studio/problems/payload-too-large"
        );
        assert_eq!(
            body["instance"],
            header(response, "x-request-id").ok_or("no id")?
        );
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn unknown_api_path_is_a_404_problem() -> TestResult {
        let router = Router::new().fallback(fallback);
        let (api, _) = serve_one(problems(router.clone()), "GET /api/nope HTTP/1.1").await?;
        assert!(api.starts_with("HTTP/1.1 404"), "{api}");
        assert_eq!(problem_body(&api)?["detail"], "no route for /api/nope");

        let (bare, _) = serve_one(problems(router.clone()), "GET /api HTTP/1.1").await?;
        assert_eq!(problem_body(&bare)?["status"], 404);

        for path in ["/nope", "/apix"] {
            let (other, _) =
                serve_one(problems(router.clone()), &format!("GET {path} HTTP/1.1")).await?;
            assert!(other.starts_with("HTTP/1.1 404"), "{other}");
            assert_eq!(header(&other, "content-type"), None, "{other}");
        }
        Ok(())
    }
}

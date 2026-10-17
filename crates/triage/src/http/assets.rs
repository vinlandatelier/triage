//! The web UI from `web/dist/`: embedded in release builds, read from disk in debug builds.

use axum::http::{Method, Uri, header};
use axum::response::{IntoResponse as _, Response};
use rust_embed::{EmbeddedFile, RustEmbed};

use crate::error::AppError;

// No `allow_missing` here, so a release build without `web/dist/` fails to compile.
#[cfg(not(debug_assertions))]
#[derive(RustEmbed)]
#[folder = "../../web/dist/"]
struct Dist;

#[cfg(debug_assertions)]
#[derive(RustEmbed)]
#[folder = "../../web/dist/"]
#[allow_missing = true]
struct Dist;

const IMMUTABLE: &str = "public, max-age=31536000, immutable";
const NO_CACHE: &str = "no-cache";

/// Unknown `/api` paths answer with a problem; every other path belongs to the web UI.
pub async fn fallback(method: Method, uri: Uri) -> Response {
    let path = uri.path();
    if path == "/api" || path.starts_with("/api/") {
        return AppError::NotFound(format!("no route for {path}")).into_response();
    }
    if method != Method::GET && method != Method::HEAD {
        return (
            [(header::ALLOW, "GET, HEAD")],
            AppError::MethodNotAllowed(format!("{method} is not allowed on {path}")),
        )
            .into_response();
    }
    serve::<Dist>(path)
}

fn serve<E: RustEmbed>(path: &str) -> Response {
    let name = path.trim_start_matches('/');
    let asset = name.starts_with("assets/");
    if let Some(file) = (!name.is_empty()).then(|| E::get(name)).flatten() {
        return respond(file, if asset { IMMUTABLE } else { NO_CACHE });
    }
    // Only extensionless paths are client routes; a missing file must not come back as HTML.
    let route = !asset
        && !name
            .rsplit('/')
            .next()
            .is_some_and(|last| last.contains('.'));
    match E::get("index.html") {
        Some(index) if route => respond(index, NO_CACHE),
        _ => AppError::NotFound(format!("no file at {path}")).into_response(),
    }
}

fn respond(file: EmbeddedFile, cache: &'static str) -> Response {
    let mime = file.metadata.mimetype().to_owned();
    (
        [
            (header::CONTENT_TYPE, mime.as_str()),
            (header::CACHE_CONTROL, cache),
        ],
        file.data,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use axum::body::to_bytes;
    use axum::http::StatusCode;

    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[derive(RustEmbed)]
    #[folder = "src/http/assets/fixture/"]
    struct Fixture;

    fn header(response: &Response, name: header::HeaderName) -> Option<&str> {
        response.headers().get(name)?.to_str().ok()
    }

    async fn body(response: Response) -> Result<String, Box<dyn std::error::Error>> {
        Ok(String::from_utf8(
            to_bytes(response.into_body(), usize::MAX).await?.to_vec(),
        )?)
    }

    fn assert_problem(response: &Response) {
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            header(response, header::CONTENT_TYPE),
            Some("application/problem+json")
        );
    }

    #[tokio::test]
    async fn root_and_client_routes_serve_index_uncached() -> TestResult {
        for path in ["/", "/index.html", "/some/route", "/some/route/"] {
            let response = serve::<Fixture>(path);
            assert_eq!(response.status(), StatusCode::OK, "{path}");
            assert_eq!(header(&response, header::CACHE_CONTROL), Some(NO_CACHE));
            assert_eq!(header(&response, header::CONTENT_TYPE), Some("text/html"));
            assert!(body(response).await?.contains("<title>fixture</title>"));
        }
        Ok(())
    }

    #[test]
    fn hashed_assets_are_immutable() {
        let response = serve::<Fixture>("/assets/app-0123abcd.js");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(header(&response, header::CACHE_CONTROL), Some(IMMUTABLE));
        assert_eq!(
            header(&response, header::CONTENT_TYPE),
            Some("text/javascript")
        );
    }

    #[test]
    fn unhashed_files_are_revalidated() {
        let response = serve::<Fixture>("/favicon.svg");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(header(&response, header::CACHE_CONTROL), Some(NO_CACHE));
        assert_eq!(
            header(&response, header::CONTENT_TYPE),
            Some("image/svg+xml")
        );
    }

    #[test]
    fn missing_files_are_404_problems() {
        for path in ["/assets/missing.js", "/assets/route", "/missing.png"] {
            assert_problem(&serve::<Fixture>(path));
        }
    }

    #[tokio::test]
    async fn api_paths_never_fall_through() -> TestResult {
        for path in ["/api", "/api/nope"] {
            let response = fallback(Method::POST, Uri::from_static(path)).await;
            assert_problem(&response);
            assert!(
                body(response)
                    .await?
                    .contains(&format!("no route for {path}"))
            );
        }
        // A path that only starts with the letters `api` is a client route.
        let response = fallback(Method::GET, Uri::from_static("/apix")).await;
        assert!(!body(response).await?.contains("no route for"));
        Ok(())
    }

    #[tokio::test]
    async fn ui_paths_answer_only_get_and_head() -> TestResult {
        for method in [Method::POST, Method::PUT, Method::DELETE] {
            let response = fallback(method.clone(), Uri::from_static("/some/route")).await;
            assert_eq!(
                response.status(),
                StatusCode::METHOD_NOT_ALLOWED,
                "{method}"
            );
            assert_eq!(header(&response, header::ALLOW), Some("GET, HEAD"));
            assert_eq!(
                header(&response, header::CONTENT_TYPE),
                Some("application/problem+json")
            );
        }
        Ok(())
    }
}

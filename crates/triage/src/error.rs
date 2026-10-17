//! `AppError`, the one error type handlers return, rendered as RFC 9457 problem+json.

use axum::http::{StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use problem_details::ProblemDetails;
use serde::Serialize;

pub type Result<T, E = AppError> = std::result::Result<T, E>;

pub const BODY_LIMIT: usize = 1 << 20;

const PROBLEM_BASE: &str = "https://triage.studio/problems/";

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    NotFound(String),
    #[error("the request is invalid")]
    Validation(Vec<FieldError>),
    #[error("{0}")]
    Conflict(String),
    #[error("the request body is larger than {BODY_LIMIT} bytes")]
    PayloadTooLarge,
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FieldError {
    pub field: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ProblemExtensions {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<FieldError>,
}

pub type Problem = ProblemDetails<ProblemExtensions>;

impl AppError {
    pub fn status(&self) -> StatusCode {
        match self {
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::Validation(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn slug(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "not-found",
            Self::Validation(_) => "validation",
            Self::Conflict(_) => "conflict",
            Self::PayloadTooLarge => "payload-too-large",
            Self::Internal(_) => "internal",
        }
    }

    fn title(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "Not found",
            Self::Validation(_) => "Validation failed",
            Self::Conflict(_) => "Conflict",
            Self::PayloadTooLarge => "Payload too large",
            Self::Internal(_) => "Internal error",
        }
    }

    /// The problem body without `instance`, which only the request middleware knows.
    pub fn problem(&self) -> Problem {
        let status = self.status();
        let mut problem = ProblemDetails::new()
            .with_status(status)
            .with_title(self.title())
            .with_extensions(ProblemExtensions::default());
        if let Ok(uri) = Uri::try_from(format!("{PROBLEM_BASE}{}", self.slug())) {
            problem = problem.with_type(uri);
        }
        // 5xx details would leak internals; the log line carries them instead.
        if !status.is_server_error() {
            problem.detail = Some(self.to_string());
        }
        if let Self::Validation(errors) = self {
            problem.extensions.errors.clone_from(errors);
        }
        problem
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        if let Self::Internal(err) = &self {
            let err: &(dyn std::error::Error + 'static) = err.as_ref();
            tracing::error!(error = err, "internal error");
        }
        let problem = self.problem();
        let mut response = problem.clone().into_response();
        // `http::middleware::problems` re-renders from this to add `instance`.
        response.extensions_mut().insert(problem);
        response
    }
}

#[cfg(test)]
mod tests {
    use axum::body::to_bytes;
    use serde_json::{Value, json};

    use super::*;

    type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

    async fn body(
        err: AppError,
    ) -> std::result::Result<(StatusCode, String, Value), Box<dyn std::error::Error>> {
        let response = err.into_response();
        let status = response.status();
        let content_type = response
            .headers()
            .get("content-type")
            .ok_or("no content-type")?
            .to_str()?
            .to_owned();
        let bytes = to_bytes(response.into_body(), usize::MAX).await?;
        Ok((status, content_type, serde_json::from_slice(&bytes)?))
    }

    #[tokio::test]
    async fn not_found_has_detail() -> TestResult {
        let (status, content_type, body) = body(AppError::NotFound("no item 7".into())).await?;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(content_type, "application/problem+json");
        assert_eq!(
            body,
            json!({
                "type": "https://triage.studio/problems/not-found",
                "status": 404,
                "title": "Not found",
                "detail": "no item 7",
            })
        );
        Ok(())
    }

    #[tokio::test]
    async fn validation_lists_field_errors() -> TestResult {
        let err = AppError::Validation(vec![FieldError {
            field: "name".into(),
            message: "must not be empty".into(),
        }]);
        let (status, _, body) = body(err).await?;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["type"], "https://triage.studio/problems/validation");
        assert_eq!(
            body["errors"],
            json!([{"field": "name", "message": "must not be empty"}])
        );
        Ok(())
    }

    #[tokio::test]
    async fn internal_hides_detail() -> TestResult {
        let err = anyhow::anyhow!("password=hunter2").context("query failed");
        let (status, _, body) = body(AppError::Internal(err)).await?;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            body,
            json!({
                "type": "https://triage.studio/problems/internal",
                "status": 500,
                "title": "Internal error",
            })
        );
        Ok(())
    }
}

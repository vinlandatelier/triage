use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use utoipa::ToSchema;
use utoipa::openapi::OpenApi;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::middleware;
use super::openapi;
use super::state::AppState;
use crate::features;

pub fn router(state: AppState) -> Router {
    let (routes, doc) = routes().split_for_parts();
    let doc = openapi::finish(doc);
    let routes = scalar(routes, &doc);
    middleware::trace(middleware::problems(
        routes
            .route("/api/openapi.json", get(move || async move { Json(doc) }))
            .fallback(middleware::fallback)
            .with_state(state),
    ))
}

#[cfg(debug_assertions)]
fn scalar(routes: Router<AppState>, doc: &OpenApi) -> Router<AppState> {
    use utoipa_scalar::{Scalar, Servable as _};
    routes.merge(Scalar::with_url("/api/scalar", doc.clone()))
}

// Release builds leave `/api/scalar` to the `/api` fallback: the docs UI never ships.
#[cfg(not(debug_assertions))]
fn scalar(routes: Router<AppState>, _: &OpenApi) -> Router<AppState> {
    routes
}

/// Every documented route. [`openapi::api_doc`] reads the spec from the same value.
pub(super) fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::with_openapi(openapi::base())
        .routes(routes!(health))
        .merge(features::routes())
}

#[derive(Debug, Serialize, ToSchema)]
struct Health {
    status: &'static str,
    version: &'static str,
    uptime_s: u64,
}

/// Daemon health
///
/// The version and uptime of the running daemon; `triage status` reads it.
#[utoipa::path(
    get,
    path = "/health",
    operation_id = "health_get",
    responses((status = 200, description = "The daemon is up", body = Health)),
)]
async fn health(State(state): State<AppState>) -> Json<Health> {
    Json(Health {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
        uptime_s: state.started.elapsed().as_secs(),
    })
}

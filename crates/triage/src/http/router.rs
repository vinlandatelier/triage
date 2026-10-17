use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;

use super::middleware;
use super::state::AppState;

pub fn router(state: AppState) -> Router {
    middleware::trace(
        Router::new()
            .route("/health", get(health))
            .with_state(state),
    )
}

#[derive(Debug, Serialize)]
struct Health {
    status: &'static str,
    version: &'static str,
    uptime_s: u64,
}

async fn health(State(state): State<AppState>) -> Json<Health> {
    Json(Health {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
        uptime_s: state.started.elapsed().as_secs(),
    })
}

//! Product features, each in `features/<name>/` as `mod http service repo model`.

use utoipa_axum::router::OpenApiRouter;

use crate::http::AppState;

pub mod hello;

/// Every feature's routes, merged into `http::router::routes`.
pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().merge(hello::http::routes())
}

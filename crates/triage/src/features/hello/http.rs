use utoipa_axum::router::OpenApiRouter;

use crate::http::AppState;

pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
}

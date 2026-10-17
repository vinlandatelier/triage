use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::model::{Hello, HelloId, HelloName, NewHello};
use super::service;
use crate::error::{AppError, Result};
use crate::http::{AppState, Json};

const LIST_LIMIT: u32 = 20;

pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(create, list))
        .routes(routes!(get))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateHello {
    // A missing name gets the same 422 as an empty one, not axum's plain-text rejection.
    #[serde(default)]
    #[schema(required = true)]
    name: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[schema(as = Hello)]
pub struct HelloView {
    /// A ULID.
    id: String,
    name: String,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    created_at: OffsetDateTime,
}

impl From<Hello> for HelloView {
    fn from(hello: Hello) -> Self {
        Self {
            id: hello.id.to_string(),
            name: hello.name.as_str().to_owned(),
            created_at: hello.created_at,
        }
    }
}

/// Create a hello
///
/// Trims the name, which must then be 1 to 80 characters.
#[utoipa::path(
    post,
    path = "/api/hello",
    operation_id = "hello_create",
    tag = "hello",
    request_body = CreateHello,
    responses((status = 201, description = "The new hello", body = HelloView)),
)]
async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateHello>,
) -> Result<(StatusCode, Json<HelloView>)> {
    let name = HelloName::parse(&body.name)?;
    let hello = service::create(&state.store, NewHello { name }).await?;
    Ok((StatusCode::CREATED, Json(hello.into())))
}

/// List hellos
///
/// The 20 newest, newest first.
#[utoipa::path(
    get,
    path = "/api/hello",
    operation_id = "hello_list",
    tag = "hello",
    responses((status = 200, description = "The newest hellos", body = Vec<HelloView>)),
)]
async fn list(State(state): State<AppState>) -> Result<Json<Vec<HelloView>>> {
    let hellos = service::list(&state.store, LIST_LIMIT).await?;
    Ok(Json(hellos.into_iter().map(HelloView::from).collect()))
}

/// Get a hello
#[utoipa::path(
    get,
    path = "/api/hello/{id}",
    operation_id = "hello_get",
    tag = "hello",
    params(("id" = String, Path, description = "The hello's ULID")),
    responses((status = 200, description = "The hello", body = HelloView)),
)]
async fn get(State(state): State<AppState>, Path(id): Path<String>) -> Result<Json<HelloView>> {
    // A malformed id names no row, so it gets the same 404 as a missing one.
    let parsed = id
        .parse::<HelloId>()
        .map_err(|_| AppError::NotFound(format!("no hello {id}")))?;
    Ok(Json(service::get(&state.store, parsed).await?.into()))
}

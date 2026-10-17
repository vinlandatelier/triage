//! The `OpenAPI` document, generated from the routes registered in [`super::router`].

use utoipa::openapi::path::Operation;
use utoipa::openapi::{
    ComponentsBuilder, ContentBuilder, InfoBuilder, OpenApi, OpenApiBuilder, Ref, RefOr,
    ResponseBuilder, ServerBuilder,
};

use crate::error::{FieldError, ProblemSchema};

const PROBLEM: &str = "Problem";
const PROBLEM_CONTENT_TYPE: &str = "application/problem+json";

/// The spec the daemon serves at `/api/openapi.json` and `openapi.json` commits.
pub fn api_doc() -> OpenApi {
    finish(super::router::routes().split_for_parts().1)
}

/// [`api_doc`] as pretty JSON with sorted keys and a trailing newline, the committed form.
pub fn api_json() -> serde_json::Result<String> {
    // `Value` maps are `BTreeMap`s, so keys come out sorted.
    let value = serde_json::to_value(api_doc())?;
    Ok(serde_json::to_string_pretty(&value)? + "\n")
}

pub(super) fn base() -> OpenApi {
    OpenApiBuilder::new()
        .info(
            InfoBuilder::new()
                .title("Triage")
                .version(env!("CARGO_PKG_VERSION"))
                .build(),
        )
        .servers(Some([ServerBuilder::new()
            .url("http://127.0.0.1:7311")
            .build()]))
        .components(Some(
            ComponentsBuilder::new()
                .schema_from::<ProblemSchema>()
                .schema_from::<FieldError>()
                .response(
                    PROBLEM,
                    ResponseBuilder::new()
                        .description("An RFC 9457 problem")
                        .content(
                            PROBLEM_CONTENT_TYPE,
                            ContentBuilder::new()
                                .schema(Some(Ref::from_schema_name(PROBLEM)))
                                .build(),
                        )
                        .build(),
                )
                .build(),
        ))
        .build()
}

/// Gives every operation the problem response as its `default`, so no handler can forget it.
pub(super) fn finish(mut doc: OpenApi) -> OpenApi {
    for item in doc.paths.paths.values_mut() {
        let operations = [
            &mut item.get,
            &mut item.put,
            &mut item.post,
            &mut item.delete,
            &mut item.options,
            &mut item.head,
            &mut item.patch,
            &mut item.trace,
        ];
        for operation in operations.into_iter().flatten() {
            add_default_problem(operation);
        }
    }
    doc
}

fn add_default_problem(operation: &mut Operation) {
    operation
        .responses
        .responses
        .entry("default".to_owned())
        .or_insert_with(|| RefOr::Ref(Ref::from_response_name(PROBLEM)));
}

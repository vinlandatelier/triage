use serde_json::Value;
use triage::http::{api_doc, api_json};

use crate::helpers::{TestResult, spawn_app};

#[test]
fn openapi_spec_is_current() -> TestResult {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/openapi.json");
    let committed = std::fs::read_to_string(path).unwrap_or_default();
    pretty_assertions::assert_eq!(
        committed,
        api_json()?,
        "{path} is stale: run `just openapi` (left: committed, right: code)"
    );
    Ok(())
}

#[tokio::test]
async fn daemon_serves_the_spec() -> TestResult {
    let app = spawn_app().await?;
    let response = app.server.get("/api/openapi.json").await;
    response.assert_status_ok();
    assert_eq!(response.json::<Value>(), serde_json::to_value(api_doc())?);
    Ok(())
}

#[test]
fn every_operation_has_an_id_and_a_problem_default() -> TestResult {
    let doc = serde_json::to_value(api_doc())?;
    let paths = doc["paths"].as_object().ok_or("no paths")?;
    assert!(paths.contains_key("/health"), "{paths:?}");
    for (path, item) in paths {
        for (method, operation) in item.as_object().ok_or("path item is not an object")? {
            let id = operation["operationId"].as_str().unwrap_or_default();
            assert!(
                id.split_once('_')
                    .is_some_and(|(feature, verb)| !feature.is_empty() && !verb.is_empty()),
                "{method} {path}: operationId {id:?} is not `<feature>_<verb>`"
            );
            assert_eq!(
                operation["responses"]["default"]["$ref"], "#/components/responses/Problem",
                "{method} {path}"
            );
        }
    }
    Ok(())
}

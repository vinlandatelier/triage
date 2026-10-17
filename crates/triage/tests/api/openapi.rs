use std::collections::BTreeSet;

use serde_json::Value;
use triage::error::{AppError, FieldError};
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
    assert_keys_sorted(&serde_json::from_str(&committed)?, "$");
    Ok(())
}

/// `api_json` sorts only while no crate enables the `serde_json` feature `preserve_order`.
fn assert_keys_sorted(value: &Value, at: &str) {
    match value {
        Value::Object(map) => {
            let keys: Vec<_> = map.keys().collect();
            assert!(keys.is_sorted(), "{at}: keys are not sorted: {keys:?}");
            for (key, child) in map {
                assert_keys_sorted(child, &format!("{at}.{key}"));
            }
        }
        Value::Array(items) => {
            for (i, child) in items.iter().enumerate() {
                assert_keys_sorted(child, &format!("{at}[{i}]"));
            }
        }
        _ => {}
    }
}

#[test]
fn problem_schema_matches_the_body() -> TestResult {
    let doc = serde_json::to_value(api_doc())?;
    let schema = &doc["components"]["schemas"]["Problem"];
    let properties: BTreeSet<_> = schema["properties"]
        .as_object()
        .ok_or("Problem has no properties")?
        .keys()
        .cloned()
        .collect();

    let full = AppError::Validation(vec![FieldError {
        field: "name".to_owned(),
        message: "required".to_owned(),
    }])
    .problem()
    .with_instance(axum::http::Uri::from_static("req-1"));
    let full = serde_json::to_value(full)?;
    let keys: BTreeSet<_> = full
        .as_object()
        .ok_or("problem is not an object")?
        .keys()
        .cloned()
        .collect();
    assert_eq!(
        keys, properties,
        "error::ProblemSchema no longer matches the problem body"
    );

    let minimal = serde_json::to_value(AppError::Internal(anyhow::anyhow!("boom")).problem())?;
    for required in schema["required"]
        .as_array()
        .ok_or("Problem has no required")?
    {
        let key = required.as_str().ok_or("required entry is not a string")?;
        assert!(
            minimal.get(key).is_some(),
            "required `{key}` missing from {minimal}"
        );
    }
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

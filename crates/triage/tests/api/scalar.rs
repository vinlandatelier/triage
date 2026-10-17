use crate::helpers::{TestResult, spawn_app};

#[cfg(debug_assertions)]
#[tokio::test]
async fn debug_builds_serve_scalar_with_every_route() -> TestResult {
    let app = spawn_app().await?;
    let response = app.server.get("/api/scalar").await;
    response.assert_status_ok();
    assert_eq!(response.header("content-type"), "text/html; charset=utf-8");
    let html = response.text();
    assert!(html.contains("\"/health\""), "{html}");
    Ok(())
}

#[cfg(not(debug_assertions))]
#[tokio::test]
async fn release_builds_answer_scalar_with_a_not_found_problem() -> TestResult {
    let app = spawn_app().await?;
    let response = app.server.get("/api/scalar").await;
    response.assert_status_not_found();
    assert_eq!(response.header("content-type"), "application/problem+json");
    assert_eq!(
        response.json::<serde_json::Value>()["type"],
        "https://triage.studio/problems/not-found"
    );
    Ok(())
}

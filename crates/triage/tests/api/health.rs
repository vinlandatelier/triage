use serde_json::Value;

use crate::helpers::{TestResult, spawn_app};

#[tokio::test]
async fn health_reports_ok_and_version() -> TestResult {
    let app = spawn_app().await?;
    let response = app.server.get("/health").await;
    response.assert_status_ok();
    let body: Value = response.json();
    assert_eq!(body["status"], "ok");
    assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
    assert!(body["uptime_s"].is_u64(), "{body}");
    Ok(())
}

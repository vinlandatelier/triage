use std::time::Duration;

use insta::{Settings, assert_json_snapshot};
use serde_json::{Value, json};
use triage::config::{Config, Paths};
use triage::telemetry;

use crate::helpers::{TestResult, spawn_app};

/// Snapshot settings that blank what changes per run; `at` is `""` for one hello, `".*"` for a list.
fn redacted(at: &str) -> Settings {
    let mut settings = Settings::clone_current();
    settings.add_redaction(&format!("{at}.id"), "[id]");
    settings.add_redaction(&format!("{at}.created_at"), "[ts]");
    settings.add_redaction(&format!("{at}.updated_at"), "[ts]");
    settings.add_redaction(".instance", "[request id]");
    settings
}

#[tokio::test]
async fn create_returns_201_with_the_trimmed_name() -> TestResult {
    let app = spawn_app().await?;
    let response = app
        .server
        .post("/api/hello")
        .json(&json!({ "name": "  world  " }))
        .await;
    response.assert_status(axum::http::StatusCode::CREATED);
    let body: Value = response.json();
    redacted("").bind(|| assert_json_snapshot!(body));
    Ok(())
}

#[tokio::test]
async fn get_returns_the_created_row() -> TestResult {
    let app = spawn_app().await?;
    let created: Value = app
        .server
        .post("/api/hello")
        .json(&json!({ "name": "world" }))
        .await
        .json();
    let id = created["id"].as_str().ok_or("no id")?;
    let response = app.server.get(&format!("/api/hello/{id}")).await;
    response.assert_status_ok();
    assert_eq!(response.json::<Value>(), created);
    Ok(())
}

#[tokio::test]
async fn list_returns_the_20_newest_first() -> TestResult {
    let app = spawn_app().await?;
    for n in 0..25 {
        app.server
            .post("/api/hello")
            .json(&json!({ "name": format!("hello {n}") }))
            .await
            .assert_status(axum::http::StatusCode::CREATED);
        // ULIDs are random within one millisecond, so a tie would order by chance.
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    let response = app.server.get("/api/hello").await;
    response.assert_status_ok();
    let body: Value = response.json();
    let names: Vec<_> = body
        .as_array()
        .ok_or("not an array")?
        .iter()
        .map(|hello| hello["name"].clone())
        .collect();
    let want: Vec<_> = (5..25).rev().map(|n| json!(format!("hello {n}"))).collect();
    assert_eq!(names, want);
    redacted(".*").bind(|| assert_json_snapshot!(body));
    Ok(())
}

#[tokio::test]
async fn unknown_and_malformed_ids_are_404_problems() -> TestResult {
    let app = spawn_app().await?;
    for id in ["01ARZ3NDEKTSV4RRFFQ69G5FAV", "01JUNK"] {
        let response = app.server.get(&format!("/api/hello/{id}")).await;
        response.assert_status_not_found();
        assert_eq!(
            response.header("content-type").to_str()?,
            "application/problem+json"
        );
        let body: Value = response.json();
        assert!(body["instance"].is_string(), "{body}");
        redacted("").bind(|| assert_json_snapshot!(format!("not_found_{id}"), body));
    }
    Ok(())
}

#[tokio::test]
async fn blank_or_missing_name_is_a_422_naming_name() -> TestResult {
    let app = spawn_app().await?;
    for (case, body) in [("empty", json!({ "name": "   " })), ("missing", json!({}))] {
        let response = app.server.post("/api/hello").json(&body).await;
        response.assert_status_unprocessable_entity();
        let body: Value = response.json();
        assert_eq!(body["errors"][0]["field"], "name", "{body}");
        redacted("").bind(|| assert_json_snapshot!(format!("invalid_{case}"), body));
    }
    Ok(())
}

#[tokio::test]
async fn create_logs_the_new_id_in_the_hello_create_span() -> TestResult {
    let app = spawn_app().await?;
    let paths = Paths {
        data: app.dir.path().to_path_buf(),
        logs: app.dir.path().join("logs"),
        config: app.dir.path().join("config"),
    };
    // telemetry::init does not create the logs dir; startup does, in `Paths::create`.
    std::fs::create_dir_all(&paths.logs)?;
    let guard = telemetry::init(&Config::default(), &paths);
    let created: Value = app
        .server
        .post("/api/hello")
        .json(&json!({ "name": "world" }))
        .await
        .json();
    // Dropping the guard flushes the non-blocking file writer.
    drop(guard);

    let id = &created["id"];
    let found = std::fs::read_dir(&paths.logs)?
        .filter_map(|entry| std::fs::read_to_string(entry.ok()?.path()).ok())
        .flat_map(|text| text.lines().map(str::to_owned).collect::<Vec<_>>())
        .filter_map(|line| serde_json::from_str::<Value>(&line).ok())
        .any(|line| line["span"] == "hello.create" && line["hello.id"] == *id);
    assert!(found, "no hello.create line with hello.id {id}");
    Ok(())
}

use std::path::Path;
use std::process::{Command, Output};

use sqlx::SqlitePool;
use sqlx::sqlite::SqliteConnectOptions;

use crate::helpers::TestResult;

fn db(dir: &Path, args: &[&str]) -> std::io::Result<Output> {
    Command::new(env!("CARGO_BIN_EXE_triage"))
        .arg("--data-dir")
        .arg(dir)
        .arg("db")
        .args(args)
        // Keeps a developer's `.env` out of the run.
        .current_dir(dir)
        .output()
}

#[test]
fn check_passes_on_a_freshly_migrated_db() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let migrate = db(tmp.path(), &["migrate"])?;
    assert_eq!(migrate.status.code(), Some(0), "{migrate:?}");
    assert!(String::from_utf8(migrate.stdout)?.starts_with("applied "));
    let check = db(tmp.path(), &["check"])?;
    assert_eq!(check.status.code(), Some(0), "{check:?}");
    Ok(())
}

#[tokio::test]
async fn check_fails_on_drift() -> TestResult {
    let tmp = tempfile::tempdir()?;
    assert!(db(tmp.path(), &["migrate"])?.status.success());
    let options = SqliteConnectOptions::new().filename(tmp.path().join("triage.db"));
    let pool = SqlitePool::connect_with(options).await?;
    sqlx::query("CREATE TABLE stray (x INTEGER)")
        .execute(&pool)
        .await?;
    pool.close().await;
    let check = db(tmp.path(), &["check"])?;
    assert_eq!(check.status.code(), Some(1), "{check:?}");
    assert!(String::from_utf8(check.stdout)?.contains("schema drift"));
    Ok(())
}

#[test]
fn sql_rejects_writes() -> TestResult {
    let tmp = tempfile::tempdir()?;
    assert!(db(tmp.path(), &["migrate"])?.status.success());
    let out = db(tmp.path(), &["sql", "DELETE FROM _sqlx_migrations"])?;
    assert!(!out.status.success(), "{out:?}");
    let stderr = String::from_utf8(out.stderr)?;
    assert!(stderr.contains("readonly database"), "{stderr}");
    Ok(())
}

#[test]
fn sql_prints_a_table_or_json() -> TestResult {
    let tmp = tempfile::tempdir()?;
    assert!(db(tmp.path(), &["migrate"])?.status.success());
    let query = "SELECT 1 AS n, 'x' AS t";
    let table = db(tmp.path(), &["sql", query])?;
    assert_eq!(String::from_utf8(table.stdout)?, "n  t\n-  -\n1  x\n");
    let json = db(tmp.path(), &["sql", "--json", query])?;
    let value: serde_json::Value = serde_json::from_slice(&json.stdout)?;
    assert_eq!(value, serde_json::json!([{"n": 1, "t": "x"}]));
    Ok(())
}

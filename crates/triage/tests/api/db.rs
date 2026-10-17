use std::path::Path;
use std::process::Output;

use sqlx::SqlitePool;
use sqlx::sqlite::SqliteConnectOptions;

use crate::helpers::{TestResult, triage};

fn db(dir: &Path, args: &[&str]) -> std::io::Result<Output> {
    triage(dir).arg("db").args(args).output()
}

#[test]
fn check_passes_on_a_freshly_migrated_db() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let migrate = db(tmp.path(), &["migrate"])?;
    assert_eq!(migrate.status.code(), Some(0), "{migrate:?}");
    assert!(String::from_utf8(migrate.stdout)?.starts_with("applied "));
    let again = db(tmp.path(), &["migrate"])?;
    assert_eq!(String::from_utf8(again.stdout)?, "applied 0 migrations\n");
    let check = db(tmp.path(), &["check"])?;
    assert_eq!(check.status.code(), Some(0), "{check:?}");
    assert_eq!(
        String::from_utf8(check.stdout)?,
        "schema matches schema.sql\n"
    );
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
fn check_and_sql_need_an_existing_db() -> TestResult {
    let tmp = tempfile::tempdir()?;
    for args in [&["check"][..], &["sql", "SELECT 1"]] {
        let out = db(tmp.path(), args)?;
        assert_eq!(out.status.code(), Some(1), "{out:?}");
        let stderr = String::from_utf8(out.stderr)?;
        assert!(stderr.contains("run `triage db migrate`"), "{stderr}");
    }
    assert!(!tmp.path().join("triage.db").exists());
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
fn sql_rejects_more_than_one_statement() -> TestResult {
    let tmp = tempfile::tempdir()?;
    assert!(db(tmp.path(), &["migrate"])?.status.success());
    let out = db(tmp.path(), &["sql", "SELECT 1 AS a, 2 AS b; SELECT 3"])?;
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(String::from_utf8(out.stderr)?.contains("exactly one statement"));
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
    let empty = db(tmp.path(), &["sql", "-- no rows\nSELECT 1 AS n WHERE 0"])?;
    assert_eq!(String::from_utf8(empty.stdout)?, "n\n-\n");
    Ok(())
}

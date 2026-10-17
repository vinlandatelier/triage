use sqlx::SqlitePool;

use crate::helpers::{TestResult, spawn_app};

/// Every table is `STRICT`, keyed by `id TEXT PRIMARY KEY`, carries `created_at` and
/// `updated_at`, and uses `snake_case` names.
async fn violations(pool: &SqlitePool) -> sqlx::Result<Vec<String>> {
    let tables: Vec<(String, bool)> = sqlx::query_as(
        "SELECT name, strict FROM pragma_table_list
         WHERE schema = 'main' AND type = 'table'
           AND name NOT LIKE 'sqlite_%' AND name <> '_sqlx_migrations'
         ORDER BY name",
    )
    .fetch_all(pool)
    .await?;
    let mut found = Vec::new();
    for (table, strict) in tables {
        if !strict {
            found.push(format!("{table}: not STRICT"));
        }
        if !is_snake_case(&table) {
            found.push(format!("{table}: table name is not snake_case"));
        }
        let columns: Vec<(String, String, i64)> =
            sqlx::query_as("SELECT name, type, pk FROM pragma_table_info(?) ORDER BY cid")
                .bind(&table)
                .fetch_all(pool)
                .await?;
        let pk: Vec<_> = columns.iter().filter(|(_, _, pk)| *pk > 0).collect();
        if !matches!(pk.as_slice(), [(name, ty, _)] if name == "id" && ty == "TEXT") {
            found.push(format!("{table}: primary key is not `id TEXT`"));
        }
        for required in ["created_at", "updated_at"] {
            if !columns.iter().any(|(name, _, _)| name == required) {
                found.push(format!("{table}: missing {required}"));
            }
        }
        for (name, _, _) in &columns {
            if !is_snake_case(name) {
                found.push(format!("{table}.{name}: column name is not snake_case"));
            }
        }
    }
    Ok(found)
}

fn is_snake_case(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

#[tokio::test]
async fn schema_conventions() -> TestResult {
    let app = spawn_app().await?;
    let found = violations(&app.store.read).await?;
    assert!(found.is_empty(), "{found:#?}");
    Ok(())
}

#[tokio::test]
async fn violations_flags_each_broken_rule() -> TestResult {
    let app = spawn_app().await?;
    sqlx::query(
        "CREATE TABLE good (id TEXT PRIMARY KEY, created_at TEXT NOT NULL, updated_at TEXT NOT NULL) STRICT;
         CREATE TABLE loose (id TEXT PRIMARY KEY, created_at TEXT, updated_at TEXT);
         CREATE TABLE \"BadName\" (id INTEGER PRIMARY KEY, \"createdAt\" TEXT) STRICT;",
    )
    .execute(&app.store.write)
    .await?;
    let found = violations(&app.store.read).await?;
    assert_eq!(
        found,
        [
            "BadName: table name is not snake_case",
            "BadName: primary key is not `id TEXT`",
            "BadName: missing created_at",
            "BadName: missing updated_at",
            "BadName.createdAt: column name is not snake_case",
            "loose: not STRICT",
        ]
    );
    Ok(())
}

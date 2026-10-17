use serde_json::{Map, Value};
use sqlx::sqlite::{SqlitePool, SqliteRow};
use sqlx::{
    AssertSqlSafe, Column as _, Executor as _, Row as _, SqlSafeStr as _, Statement as _,
    TypeInfo as _, ValueRef as _,
};

use super::{Error, Result};

/// The result of an ad hoc query: column names in select order, then one cell per column per row.
#[derive(Debug, PartialEq)]
pub struct Rows {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
}

impl Rows {
    pub fn to_json(&self) -> Value {
        self.rows
            .iter()
            .map(|row| {
                let object: Map<String, Value> = self
                    .columns
                    .iter()
                    .cloned()
                    .zip(row.iter().cloned())
                    .collect();
                Value::Object(object)
            })
            .collect()
    }
}

/// Runs one statement and decodes every cell by its storage class, so any query works.
pub async fn run(pool: &SqlitePool, sql: &str) -> Result<Rows> {
    let mut conn = pool.acquire().await.map_err(Error::Query)?;
    // Preparing first keeps the column names when the query returns no rows.
    // Running arbitrary SQL is the point here; the read-only connection is the safety net.
    let statement = conn
        .prepare(AssertSqlSafe(sql).into_sql_str())
        .await
        .map_err(Error::Query)?;
    let columns = statement
        .columns()
        .iter()
        .map(|column| column.name().to_owned())
        .collect();
    let rows = statement
        .query()
        .fetch_all(&mut *conn)
        .await
        .map_err(Error::Query)?
        .iter()
        .map(cells)
        .collect::<Result<_, sqlx::Error>>()
        .map_err(Error::Query)?;
    Ok(Rows { columns, rows })
}

fn cells(row: &SqliteRow) -> sqlx::Result<Vec<Value>> {
    (0..row.len())
        .map(|i| {
            let raw = row.try_get_raw(i)?;
            if raw.is_null() {
                return Ok(Value::Null);
            }
            Ok(match raw.type_info().name() {
                "INTEGER" => row.try_get_unchecked::<i64, _>(i)?.into(),
                "REAL" => row.try_get_unchecked::<f64, _>(i)?.into(),
                "BLOB" => hex(&row.try_get_unchecked::<Vec<u8>, _>(i)?).into(),
                _ => row.try_get_unchecked::<String, _>(i)?.into(),
            })
        })
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::from("x'"), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    }) + "'"
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

    #[tokio::test]
    async fn decodes_each_storage_class() -> TestResult {
        let pool = SqlitePool::connect("sqlite::memory:").await?;
        let rows = run(
            &pool,
            "SELECT 1 AS i, 1.5 AS r, 'a' AS t, x'00ff' AS b, NULL AS n",
        )
        .await?;
        assert_eq!(rows.columns, ["i", "r", "t", "b", "n"]);
        assert_eq!(
            rows.rows,
            [vec![
                json!(1),
                json!(1.5),
                json!("a"),
                json!("x'00ff'"),
                json!(null)
            ]]
        );
        assert_eq!(
            rows.to_json(),
            json!([{"i": 1, "r": 1.5, "t": "a", "b": "x'00ff'", "n": null}])
        );
        Ok(())
    }

    #[tokio::test]
    async fn keeps_columns_when_no_rows_match() -> TestResult {
        let pool = SqlitePool::connect("sqlite::memory:").await?;
        let rows = run(&pool, "SELECT 1 AS a, 2 AS b WHERE 0").await?;
        assert_eq!(rows.columns, ["a", "b"]);
        assert_eq!(rows.rows, Vec::<Vec<Value>>::new());
        Ok(())
    }
}

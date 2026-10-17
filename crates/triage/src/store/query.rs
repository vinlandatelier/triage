use std::collections::HashSet;

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
    pub fn to_json(&self) -> Result<Value> {
        let mut seen = HashSet::new();
        if let Some(name) = self.columns.iter().find(|name| !seen.insert(*name)) {
            return Err(Error::DuplicateColumn(name.clone()));
        }
        Ok(self
            .rows
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
            .collect())
    }
}

/// Runs one statement and decodes every cell by its storage class, so any query works.
pub async fn run(pool: &SqlitePool, sql: &str) -> Result<Rows> {
    // sqlx runs every statement in the string but names columns after the first, so a
    // second statement would put its rows under the wrong header.
    match statements(sql) {
        0 => return Err(Error::Input("the query is empty")),
        1 => {}
        _ => return Err(Error::Input("pass exactly one statement")),
    }
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
                "REAL" => real(row.try_get_unchecked::<f64, _>(i)?),
                "BLOB" => hex(&row.try_get_unchecked::<Vec<u8>, _>(i)?).into(),
                // SQLite does not check that TEXT is valid UTF-8.
                _ => String::from_utf8_lossy(&row.try_get_unchecked::<Vec<u8>, _>(i)?).into(),
            })
        })
        .collect()
}

/// JSON has no infinity or NaN, and `null` would read as SQL `NULL`.
fn real(value: f64) -> Value {
    if value.is_finite() {
        value.into()
    } else {
        value.to_string().into()
    }
}

/// Counts the non-empty statements in `sql`, skipping `;` inside literals, quoted names,
/// and comments the way SQLite's tokenizer does.
fn statements(sql: &str) -> usize {
    let mut count = 0;
    let mut open = false;
    let mut chars = sql.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            ';' => open = false,
            c if c.is_whitespace() => {}
            '-' if chars.next_if_eq(&'-').is_some() => {
                chars.find(|&c| c == '\n');
            }
            '/' if chars.next_if_eq(&'*').is_some() => {
                while let Some(c) = chars.next() {
                    if c == '*' && chars.next_if_eq(&'/').is_some() {
                        break;
                    }
                }
            }
            c => {
                if !open {
                    open = true;
                    count += 1;
                }
                let close = match c {
                    '\'' | '"' | '`' => c,
                    '[' => ']',
                    _ => continue,
                };
                // An escaped `''` scans as two adjacent literals, which counts the same.
                chars.find(|&c| c == close);
            }
        }
    }
    count
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
            rows.to_json()?,
            json!([{"i": 1, "r": 1.5, "t": "a", "b": "x'00ff'", "n": null}])
        );
        Ok(())
    }

    #[tokio::test]
    async fn keeps_invalid_utf8_and_non_finite_reals() -> TestResult {
        let pool = SqlitePool::connect("sqlite::memory:").await?;
        let rows = run(&pool, "SELECT CAST(x'61ff' AS TEXT), 1e999, -1e999").await?;
        assert_eq!(
            rows.rows,
            [vec![json!("a\u{fffd}"), json!("inf"), json!("-inf")]]
        );
        Ok(())
    }

    #[tokio::test]
    async fn rejects_empty_and_multi_statement_input() -> TestResult {
        let pool = SqlitePool::connect("sqlite::memory:").await?;
        for sql in [
            "",
            " ; -- nothing",
            "SELECT 1; SELECT 2",
            "SELECT 1 AS a, 2 AS b; SELECT 3",
        ] {
            let Err(err) = run(&pool, sql).await else {
                return Err(format!("{sql:?} must be rejected").into());
            };
            assert!(matches!(err, Error::Input(_)), "{sql:?}: {err:?}");
        }
        Ok(())
    }

    #[test]
    fn counts_statements_outside_literals_and_comments() {
        for (sql, want) in [
            ("SELECT 1;", 1),
            ("SELECT ';' AS \"a;b\", [c;d], `e;f`, 'it''s;'", 1),
            ("SELECT 1 -- ; SELECT 2\n;", 1),
            ("SELECT 1 /* ; */ ; /* trailing */", 1),
            ("-- only a comment", 0),
            ("SELECT 1;SELECT 2", 2),
        ] {
            assert_eq!(statements(sql), want, "{sql:?}");
        }
    }

    #[test]
    fn json_rejects_duplicate_column_names() {
        let rows = Rows {
            columns: vec!["a".into(), "a".into()],
            rows: vec![vec![json!(1), json!(2)]],
        };
        assert!(matches!(rows.to_json(), Err(Error::DuplicateColumn(name)) if name == "a"));
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

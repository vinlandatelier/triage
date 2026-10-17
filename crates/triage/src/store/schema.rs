use sqlx::SqlitePool;

use super::{Error, Result};

/// `schema.sql` as of this build, so `triage db check` needs no source tree.
pub const SCHEMA: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/schema.sql"));

/// The schema as `;`-terminated `CREATE` statements separated by blank lines.
///
/// `.schema` order is not guaranteed, so this sorts by type then name for a stable diff.
pub async fn dump(pool: &SqlitePool) -> Result<String> {
    let statements: Vec<String> = sqlx::query_scalar(
        // `ANALYZE` and `PRAGMA optimize` create `sqlite_stat*` tables at runtime.
        "SELECT sql FROM sqlite_schema WHERE sql NOT NULL AND name NOT GLOB 'sqlite_stat*' \
         ORDER BY type DESC, name",
    )
    .fetch_all(pool)
    .await
    .map_err(Error::Schema)?;
    Ok(statements
        .iter()
        .map(|sql| format!("{sql};\n"))
        .collect::<Vec<_>>()
        .join("\n"))
}

use sqlx::SqlitePool;

use super::{Error, Result};

/// The schema as `;`-terminated `CREATE` statements separated by blank lines.
///
/// `.schema` order is not guaranteed, so this sorts by type then name for a stable diff.
pub async fn dump(pool: &SqlitePool) -> Result<String> {
    let statements: Vec<String> = sqlx::query_scalar(
        "SELECT sql FROM sqlite_schema WHERE sql NOT NULL ORDER BY type DESC, name",
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

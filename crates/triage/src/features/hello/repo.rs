use sqlx::{SqliteConnection, SqlitePool};
use time::format_description::BorrowedFormatItem;
use time::macros::format_description;
use time::{OffsetDateTime, UtcOffset};

use super::model::{Hello, HelloId, HelloName};
use crate::error::{AppError, Result};

// Fixed width so `ORDER BY created_at` sorts by time; RFC 3339 drops trailing subsecond
// zeros, and `...:00Z` would sort after `...:00.1Z`.
const TIMESTAMP: &[BorrowedFormatItem<'_>] =
    format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z");

fn timestamp(at: OffsetDateTime) -> Result<String> {
    at.to_offset(UtcOffset::UTC)
        .format(TIMESTAMP)
        .map_err(|err| AppError::Internal(err.into()))
}

pub async fn insert(conn: &mut SqliteConnection, hello: &Hello) -> Result<()> {
    let created_at = timestamp(hello.created_at)?;
    let updated_at = timestamp(hello.updated_at)?;
    sqlx::query!(
        "INSERT INTO hello (id, name, created_at, updated_at) VALUES (?, ?, ?, ?)",
        hello.id,
        hello.name,
        created_at,
        updated_at,
    )
    .execute(conn)
    .await?;
    Ok(())
}

pub async fn list_recent(pool: &SqlitePool, limit: u32) -> Result<Vec<Hello>> {
    let limit = i64::from(limit);
    Ok(sqlx::query_as!(
        Hello,
        r#"SELECT id AS "id: HelloId", name AS "name: HelloName",
                  created_at AS "created_at: OffsetDateTime",
                  updated_at AS "updated_at: OffsetDateTime"
           FROM hello ORDER BY created_at DESC, id DESC LIMIT ?"#,
        limit,
    )
    .fetch_all(pool)
    .await?)
}

pub async fn get(pool: &SqlitePool, id: HelloId) -> Result<Option<Hello>> {
    Ok(sqlx::query_as!(
        Hello,
        r#"SELECT id AS "id: HelloId", name AS "name: HelloName",
                  created_at AS "created_at: OffsetDateTime",
                  updated_at AS "updated_at: OffsetDateTime"
           FROM hello WHERE id = ?"#,
        id,
    )
    .fetch_optional(pool)
    .await?)
}

#[cfg(test)]
mod tests {
    use time::macros::datetime;

    use super::*;

    #[test]
    fn timestamp_keeps_trailing_zeros() -> Result<()> {
        assert_eq!(
            timestamp(datetime!(2026-01-02 03:04:00 UTC))?,
            "2026-01-02T03:04:00.000Z"
        );
        assert_eq!(
            timestamp(datetime!(2026-01-02 13:04:05.1 +10:00))?,
            "2026-01-02T03:04:05.100Z"
        );
        Ok(())
    }
}

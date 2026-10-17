use time::OffsetDateTime;
use tracing::{Span, field, instrument};

use super::model::{Hello, HelloId, NewHello};
use super::repo;
use crate::error::{AppError, Result};
use crate::store::Store;

// Truncated to the stored precision, so `create` returns what `get` later reads.
fn now() -> Result<OffsetDateTime> {
    let now = OffsetDateTime::now_utc();
    now.replace_millisecond(now.millisecond())
        .map_err(|err| AppError::Internal(err.into()))
}

#[instrument(name = "hello.create", skip_all, fields(hello.id = field::Empty))]
pub async fn create(store: &Store, new: NewHello) -> Result<Hello> {
    let now = now()?;
    let hello = Hello {
        id: HelloId::generate(),
        name: new.name,
        created_at: now,
        updated_at: now,
    };
    Span::current().record("hello.id", field::display(hello.id));
    let mut tx = store.begin().await?;
    repo::insert(&mut tx, &hello).await?;
    tx.commit().await?;
    Ok(hello)
}

#[instrument(name = "hello.list", skip(store))]
pub async fn list(store: &Store, limit: u32) -> Result<Vec<Hello>> {
    repo::list_recent(&store.read, limit).await
}

pub async fn get(store: &Store, id: HelloId) -> Result<Hello> {
    repo::get(&store.read, id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("no hello {id}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Paths;
    use crate::features::hello::model::HelloName;

    type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

    async fn store(dir: &tempfile::TempDir) -> crate::store::Result<Store> {
        crate::store::open(&Paths {
            data: dir.path().into(),
            logs: dir.path().join("logs"),
            config: dir.path().join("config"),
        })
        .await
    }

    async fn create_named(store: &Store, name: &str) -> Result<Hello> {
        let name = HelloName::parse(name)?;
        create(store, NewHello { name }).await
    }

    #[tokio::test]
    async fn create_then_get_round_trips() -> TestResult {
        let dir = tempfile::tempdir()?;
        let store = store(&dir).await?;
        let created = create_named(&store, " bob ").await?;
        assert_eq!(get(&store, created.id).await?, created);
        Ok(())
    }

    #[tokio::test]
    async fn list_is_newest_first_and_limited() -> TestResult {
        let dir = tempfile::tempdir()?;
        let store = store(&dir).await?;
        let mut ids = Vec::new();
        for name in ["a", "b", "c"] {
            ids.push(create_named(&store, name).await?.id);
            // ULIDs are random within one millisecond, so a tie would order by chance.
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
        let listed: Vec<_> = list(&store, 2).await?.into_iter().map(|h| h.id).collect();
        assert_eq!(listed, [ids[2], ids[1]]);
        Ok(())
    }

    #[tokio::test]
    async fn get_missing_is_not_found() -> TestResult {
        let dir = tempfile::tempdir()?;
        let store = store(&dir).await?;
        let result = get(&store, HelloId::generate()).await;
        assert!(matches!(result, Err(AppError::NotFound(_))), "{result:?}");
        Ok(())
    }
}

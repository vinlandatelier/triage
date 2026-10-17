use std::num::NonZeroUsize;
use std::path::Path;
use std::thread;
use std::time::Duration;

use sqlx::migrate::{MigrateError, Migrator};
use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteSynchronous,
};
use sqlx::{Sqlite, Transaction};

use super::{Error, Result};
use crate::config::Paths;

static MIGRATOR: Migrator = sqlx::migrate!();

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub struct Store {
    pub read: SqlitePool,
    /// One connection, so writers queue in the pool instead of contending for the file lock.
    pub write: SqlitePool,
}

impl Store {
    /// `BEGIN IMMEDIATE` takes the write lock up front; a deferred transaction that reads
    /// first fails with `SQLITE_BUSY` when it upgrades, and `busy_timeout` cannot retry that.
    pub async fn begin(&self) -> sqlx::Result<Transaction<'static, Sqlite>> {
        self.write.begin_with("BEGIN IMMEDIATE").await
    }
}

/// Opens `<data>/triage.db`, creating it if missing, and applies pending migrations.
pub async fn open(paths: &Paths) -> Result<Store> {
    open_with(&paths.db_file(), &MIGRATOR).await
}

async fn open_with(path: &Path, migrator: &Migrator) -> Result<Store> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(BUSY_TIMEOUT)
        .pragma("temp_store", "MEMORY");
    let open_err = |source| Error::Open {
        path: path.to_path_buf(),
        source,
    };
    // `connect_with` opens one connection eagerly, so a file that is not a database fails here.
    let write = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options.clone())
        .await
        .map_err(open_err)?;
    migrator
        .run(&write)
        .await
        .map_err(|err| migrate_error(migrator, err))?;
    let readers = thread::available_parallelism().map_or(4, NonZeroUsize::get);
    let read = SqlitePoolOptions::new()
        .max_connections(u32::try_from(readers).unwrap_or(u32::MAX))
        .connect_with(options.pragma("query_only", "ON"))
        .await
        .map_err(open_err)?;
    Ok(Store { read, write })
}

fn migrate_error(migrator: &Migrator, err: MigrateError) -> Error {
    let name = |version: i64| {
        migrator.iter().find(|m| m.version == version).map_or_else(
            || version.to_string(),
            |m| format!("{version}_{}.sql", m.description.replace(' ', "_")),
        )
    };
    match err {
        MigrateError::VersionMismatch(version) => Error::Edited {
            name: name(version),
        },
        MigrateError::ExecuteMigration(_, version)
        | MigrateError::VersionMissing(version)
        | MigrateError::VersionTooOld(version, _)
        | MigrateError::VersionTooNew(version, _) => Error::Migrate {
            name: name(version),
            source: err,
        },
        err => Error::Migrations(err),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

    async fn migrator(
        dir: &Path,
        files: &[(&str, &str)],
    ) -> std::result::Result<Migrator, Box<dyn std::error::Error>> {
        fs::create_dir_all(dir)?;
        for (name, sql) in files {
            fs::write(dir.join(name), sql)?;
        }
        Ok(Migrator::new(dir).await?)
    }

    #[tokio::test]
    async fn creates_the_file_in_wal_mode() -> TestResult {
        let tmp = tempfile::tempdir()?;
        let paths = Paths {
            data: tmp.path().into(),
            logs: tmp.path().join("logs"),
            config: tmp.path().join("config"),
        };
        let store = open(&paths).await?;
        assert!(paths.db_file().is_file());
        for pool in [&store.read, &store.write] {
            let mode: String = sqlx::query_scalar("PRAGMA journal_mode")
                .fetch_one(pool)
                .await?;
            assert_eq!(mode, "wal");
            let (sync, fks, temp_store): (i64, i64, i64) = (
                sqlx::query_scalar("PRAGMA synchronous")
                    .fetch_one(pool)
                    .await?,
                sqlx::query_scalar("PRAGMA foreign_keys")
                    .fetch_one(pool)
                    .await?,
                sqlx::query_scalar("PRAGMA temp_store")
                    .fetch_one(pool)
                    .await?,
            );
            assert_eq!((sync, fks, temp_store), (1, 1, 2), "NORMAL, ON, MEMORY");
        }
        Ok(())
    }

    #[tokio::test]
    async fn read_pool_rejects_writes() -> TestResult {
        let tmp = tempfile::tempdir()?;
        let store = open_with(&tmp.path().join("t.db"), &Migrator::DEFAULT).await?;
        let result = sqlx::query("CREATE TABLE t (x INTEGER)")
            .execute(&store.read)
            .await;
        assert!(result.is_err(), "{result:?}");
        Ok(())
    }

    #[tokio::test]
    async fn concurrent_writes_both_succeed() -> TestResult {
        let tmp = tempfile::tempdir()?;
        let migrations = migrator(
            &tmp.path().join("m"),
            &[(
                "1_counter.sql",
                "CREATE TABLE counter (n INTEGER NOT NULL) STRICT;",
            )],
        )
        .await?;
        let store = open_with(&tmp.path().join("t.db"), &migrations).await?;
        let write = |n: i64| {
            let store = store.clone();
            async move {
                let mut tx = store.begin().await?;
                sqlx::query("INSERT INTO counter (n) VALUES (?)")
                    .bind(n)
                    .execute(&mut *tx)
                    .await?;
                tokio::time::sleep(Duration::from_millis(20)).await;
                tx.commit().await
            }
        };
        let (a, b) = tokio::join!(tokio::spawn(write(1)), tokio::spawn(write(2)));
        a??;
        b??;
        let total: i64 = sqlx::query_scalar("SELECT sum(n) FROM counter")
            .fetch_one(&store.read)
            .await?;
        assert_eq!(total, 3);
        Ok(())
    }

    #[tokio::test]
    async fn not_a_database_names_the_path() -> TestResult {
        let tmp = tempfile::tempdir()?;
        let path = tmp.path().join("t.db");
        fs::write(
            &path,
            "this is not sqlite, just enough bytes to fill a header....",
        )?;
        let Err(err) = open_with(&path, &Migrator::DEFAULT).await else {
            return Err("a text file must not open".into());
        };
        assert!(matches!(err, Error::Open { .. }), "{err:?}");
        assert_eq!(err.exit_code(), 1);
        assert!(
            err.to_string().contains(&path.display().to_string()),
            "{err}"
        );
        Ok(())
    }

    #[tokio::test]
    async fn failed_migration_keeps_earlier_files_and_names_itself() -> TestResult {
        let tmp = tempfile::tempdir()?;
        let db = tmp.path().join("t.db");
        let migrations = migrator(
            &tmp.path().join("m"),
            &[
                ("1_first.sql", "CREATE TABLE first (x INTEGER) STRICT;"),
                (
                    "2_broken_one.sql",
                    "CREATE TABLE second (x INTEGER) STRICT; SELECT nope FROM nowhere;",
                ),
            ],
        )
        .await?;
        let Err(err) = open_with(&db, &migrations).await else {
            return Err("broken migration must fail".into());
        };
        assert!(err.to_string().contains("2_broken_one.sql"), "{err}");

        let pool = SqlitePool::connect_with(SqliteConnectOptions::new().filename(&db)).await?;
        let tables: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM sqlite_master WHERE name IN ('first', 'second') ORDER BY name",
        )
        .fetch_all(&pool)
        .await?;
        assert_eq!(tables, ["first"]);
        Ok(())
    }

    #[tokio::test]
    async fn edited_migration_names_itself_and_checksum() -> TestResult {
        let tmp = tempfile::tempdir()?;
        let db = tmp.path().join("t.db");
        let dir = tmp.path().join("m");
        let original =
            migrator(&dir, &[("1_first.sql", "CREATE TABLE first (x INTEGER);")]).await?;
        drop(open_with(&db, &original).await?);

        let edited = migrator(&dir, &[("1_first.sql", "CREATE TABLE first (y INTEGER);")]).await?;
        let Err(err) = open_with(&db, &edited).await else {
            return Err("edited migration must fail".into());
        };
        let message = err.to_string();
        assert!(message.contains("1_first.sql"), "{message}");
        assert!(message.contains("checksum"), "{message}");
        Ok(())
    }
}

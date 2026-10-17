//! SQLite storage: one database file, a read pool, and a one-connection write pool.

use std::path::PathBuf;

use sqlx::migrate::MigrateError;

mod pool;
mod schema;

pub use pool::{Store, open};
pub use schema::dump as dump_schema;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot open database {}: {source}", path.display())]
    Open { path: PathBuf, source: sqlx::Error },
    #[error("migration {name} was edited after it was applied: its checksum no longer matches")]
    Edited { name: String },
    #[error("migration {name} failed: {source}")]
    Migrate { name: String, source: MigrateError },
    #[error("cannot apply migrations: {0}")]
    Migrations(MigrateError),
    #[error("cannot read the schema: {0}")]
    Schema(sqlx::Error),
}

impl Error {
    pub fn exit_code(&self) -> u8 {
        1
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

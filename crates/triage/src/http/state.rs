use std::sync::Arc;
use std::time::Instant;

use crate::config::{Config, Paths};
use crate::store::Store;

#[derive(Debug, Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub paths: Arc<Paths>,
    pub store: Store,
    pub started: Instant,
}

impl AppState {
    pub fn new(config: Config, paths: Paths, store: Store) -> Self {
        Self {
            config: Arc::new(config),
            paths: Arc::new(paths),
            store,
            started: Instant::now(),
        }
    }
}

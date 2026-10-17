use std::sync::Arc;
use std::time::Instant;

use crate::config::{Config, Paths};

#[derive(Debug, Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub paths: Arc<Paths>,
    pub started: Instant,
}

impl AppState {
    pub fn new(config: Config, paths: Paths) -> Self {
        Self {
            config: Arc::new(config),
            paths: Arc::new(paths),
            started: Instant::now(),
        }
    }
}

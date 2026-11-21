use std::sync::Arc;
use tokio::sync::RwLock;

/// Shared in-memory server state tracking audit statistics.
#[derive(Debug, Clone)]
pub struct ServerState {
    audits_run: Arc<RwLock<u64>>,
}

impl Default for ServerState {
    fn default() -> Self {
        Self::new()
    }
}

impl ServerState {
    pub fn new() -> Self {
        Self {
            audits_run: Arc::new(RwLock::new(0)),
        }
    }

    pub async fn record_audit(&self) -> u64 {
        let mut guard = self.audits_run.write().await;
        *guard += 1;
        *guard
    }
}

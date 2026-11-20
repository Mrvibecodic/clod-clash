use arraydeque::{ArrayDeque, Wrapping};
use compact_str::CompactString;
use tokio::sync::RwLock;

const LOGS_QUEUE_LEN: usize = 100;

#[derive(Default)]
pub struct AsyncLogger {
    inner: RwLock<ArrayDeque<CompactString, LOGS_QUEUE_LEN, Wrapping>>,
}

impl AsyncLogger {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn append_log(&self, log: CompactString) {
        let mut guard = self.inner.write().await;
        guard.push_back(log);
    }

    pub async fn get_logs(&self) -> Vec<CompactString> {
        let guard = self.inner.read().await;
        guard.iter().cloned().collect()
    }

    pub async fn clear_logs(&self) {
        let mut guard = self.inner.write().await;
        guard.clear();
    }
}

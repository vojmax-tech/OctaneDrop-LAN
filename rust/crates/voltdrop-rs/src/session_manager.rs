use std::collections::{HashMap, VecDeque};
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::RwLock;
use voltdrop_p2p_core::session::{SessionStatus, TransferSession};

pub struct SessionManager {
    active: RwLock<HashMap<String, TransferSession>>,
    recent: RwLock<VecDeque<TransferSession>>,
    max_recent: usize,
}

impl SessionManager {
    pub fn new(max_recent: usize) -> Self {
        Self {
            active: RwLock::new(HashMap::new()),
            recent: RwLock::new(VecDeque::new()),
            max_recent,
        }
    }

    pub async fn register(&self, mut session: TransferSession) {
        let now = now_ts();
        if session.started_at <= 0.0 {
            session.started_at = now;
        }
        session.updated_at = now;
        self.active.write().await.insert(session.session_id.clone(), session);
    }

    pub async fn update(
        &self,
        session_id: &str,
        status: Option<SessionStatus>,
        transferred_bytes: Option<u64>,
        error: Option<String>,
    ) {
        let mut active = self.active.write().await;
        if let Some(session) = active.get_mut(session_id) {
            if let Some(next_status) = status {
                session.status = next_status;
            }
            if let Some(bytes) = transferred_bytes {
                session.set_transferred_bytes(bytes);
            }
            if let Some(err) = error {
                session.error = Some(err);
            }
            session.updated_at = now_ts();
        }
    }

    pub async fn finish(&self, session_id: &str, status: SessionStatus, error: Option<String>) {
        let mut active = self.active.write().await;
        let Some(mut session) = active.remove(session_id) else {
            return;
        };
        session.status = status;
        if status == SessionStatus::Completed && session.total_bytes > 0 {
            session.transferred_bytes = session.total_bytes;
        }
        session.error = error;
        session.updated_at = now_ts();
        drop(active);

        let mut recent = self.recent.write().await;
        recent.push_front(session);
        while recent.len() > self.max_recent {
            recent.pop_back();
        }
    }

    pub async fn list_all(&self) -> Vec<TransferSession> {
        let active = self.active.read().await;
        let recent = self.recent.read().await;
        let mut combined: Vec<TransferSession> = active.values().cloned().collect();
        combined.extend(recent.iter().cloned());
        combined.sort_by(|a, b| b.updated_at.total_cmp(&a.updated_at));
        combined
    }
}

fn now_ts() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}


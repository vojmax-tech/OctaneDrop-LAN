use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct Session {
    pub username: String,
    pub expires_at: i64,
}

#[derive(Clone)]
pub struct AuthService {
    sessions: Arc<RwLock<HashMap<String, Session>>>,
    ttl_seconds: i64,
}

impl AuthService {
    pub fn new(ttl_seconds: u64) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            ttl_seconds: ttl_seconds as i64,
        }
    }

    pub async fn issue_session(&self, username: &str) -> String {
        let token = Uuid::new_v4().simple().to_string();
        let expires_at = now_ts() + self.ttl_seconds.max(60);
        self.sessions.write().await.insert(
            token.clone(),
            Session {
                username: username.to_string(),
                expires_at,
            },
        );
        token
    }

    pub async fn verify(&self, token: &str) -> Option<String> {
        if token.trim().is_empty() {
            return None;
        }
        let now = now_ts();
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get(token) {
            if session.expires_at >= now {
                return Some(session.username.clone());
            }
        }
        sessions.remove(token);
        None
    }

    pub async fn revoke(&self, token: &str) {
        self.sessions.write().await.remove(token);
    }
}

fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}


use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionDirection {
    Send,
    Receive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    Queued,
    Connecting,
    Sending,
    Receiving,
    Persisting,
    Completed,
    Failed,
    Canceled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TransferSession {
    pub session_id: String,
    pub direction: SessionDirection,
    pub peer_host: String,
    pub peer_port: u16,
    pub file_name: String,
    pub total_bytes: u64,
    pub transferred_bytes: u64,
    pub status: SessionStatus,
    pub error: Option<String>,
    pub started_at: f64,
    pub updated_at: f64,
}

impl TransferSession {
    pub fn progress_percent(&self) -> u8 {
        if self.total_bytes == 0 {
            return 0;
        }
        let ratio = self.transferred_bytes as f64 / self.total_bytes as f64;
        (ratio.mul_add(100.0, 0.0).clamp(0.0, 100.0)) as u8
    }

    pub fn set_transferred_bytes(&mut self, value: u64) {
        if self.total_bytes == 0 {
            self.transferred_bytes = value;
            return;
        }
        self.transferred_bytes = value.min(self.total_bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_progress() {
        let session = TransferSession {
            session_id: "s1".to_string(),
            direction: SessionDirection::Send,
            peer_host: "192.168.1.20".to_string(),
            peer_port: 8766,
            file_name: "video.mp4".to_string(),
            total_bytes: 1000,
            transferred_bytes: 250,
            status: SessionStatus::Sending,
            error: None,
            started_at: 0.0,
            updated_at: 0.0,
        };
        assert_eq!(session.progress_percent(), 25);
    }

    #[test]
    fn allows_unknown_total() {
        let session = TransferSession {
            session_id: "s2".to_string(),
            direction: SessionDirection::Receive,
            peer_host: "192.168.1.21".to_string(),
            peer_port: 8766,
            file_name: "archive.zip".to_string(),
            total_bytes: 0,
            transferred_bytes: 42,
            status: SessionStatus::Receiving,
            error: None,
            started_at: 0.0,
            updated_at: 0.0,
        };
        assert_eq!(session.progress_percent(), 0);
    }

    #[test]
    fn clamps_transferred_bytes_to_total() {
        let mut session = TransferSession {
            session_id: "s3".to_string(),
            direction: SessionDirection::Send,
            peer_host: "192.168.1.22".to_string(),
            peer_port: 8766,
            file_name: "clip.mp4".to_string(),
            total_bytes: 10,
            transferred_bytes: 0,
            status: SessionStatus::Sending,
            error: None,
            started_at: 0.0,
            updated_at: 0.0,
        };
        session.set_transferred_bytes(99);
        assert_eq!(session.transferred_bytes, 10);
    }
}

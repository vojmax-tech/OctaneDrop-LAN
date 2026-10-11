use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

pub const DISCOVERY_MAX_BYTES: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryPayload {
    pub app_name: String,
    pub device_name: String,
    pub host: String,
    pub port: u16,
    pub transfer_port: u16,
    pub ts: i64,
}

impl DiscoveryPayload {
    pub fn peer_id(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    pub fn encode_compact(&self) -> Result<Vec<u8>, DiscoveryError> {
        let blob = serde_json::to_vec(self).map_err(|_| DiscoveryError::InvalidEncoding)?;
        if blob.len() > DISCOVERY_MAX_BYTES {
            return Err(DiscoveryError::PayloadTooLarge);
        }
        Ok(blob)
    }

    pub fn decode(blob: &[u8]) -> Result<Self, DiscoveryError> {
        if blob.len() > DISCOVERY_MAX_BYTES {
            return Err(DiscoveryError::PayloadTooLarge);
        }
        serde_json::from_slice(blob).map_err(|_| DiscoveryError::InvalidEncoding)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoveryError {
    InvalidEncoding,
    PayloadTooLarge,
}

impl Display for DiscoveryError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidEncoding => write!(f, "Invalid discovery payload encoding."),
            Self::PayloadTooLarge => write!(f, "Discovery payload exceeds allowed size."),
        }
    }
}

impl Error for DiscoveryError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_discovery_payload() {
        let payload = DiscoveryPayload {
            app_name: "VoltDrop".to_string(),
            device_name: "devbox".to_string(),
            host: "192.168.1.10".to_string(),
            port: 8765,
            transfer_port: 8766,
            ts: 1_776_000_000,
        };

        let encoded = payload.encode_compact().expect("discovery payload should encode");
        let decoded = DiscoveryPayload::decode(&encoded).expect("discovery payload should decode");

        assert_eq!(decoded, payload);
        assert_eq!(decoded.peer_id(), "192.168.1.10:8765");
    }

    #[test]
    fn rejects_oversized_payload() {
        let huge = vec![b'x'; DISCOVERY_MAX_BYTES + 1];
        let error = DiscoveryPayload::decode(&huge).expect_err("payload should be rejected");
        assert_eq!(error, DiscoveryError::PayloadTooLarge);
    }
}


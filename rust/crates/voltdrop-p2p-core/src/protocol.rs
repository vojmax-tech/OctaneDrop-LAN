use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const PROTOCOL_VERSION: u8 = 1;
pub const CONTROL_MAX_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum FrameKind {
    Hello,
    Offer,
    Accept,
    Reject,
    Chunk,
    Done,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ControlFrame {
    #[serde(rename = "v")]
    pub version: u8,
    pub kind: FrameKind,
    pub payload: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    InvalidEncoding,
    UnsupportedVersion,
    FrameTooLarge,
}

impl Display for ProtocolError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidEncoding => write!(f, "Invalid control frame encoding."),
            Self::UnsupportedVersion => write!(f, "Unsupported protocol version."),
            Self::FrameTooLarge => write!(f, "Control frame exceeds allowed size."),
        }
    }
}

impl Error for ProtocolError {}

pub fn make_control_frame(kind: FrameKind, payload: Map<String, Value>) -> ControlFrame {
    ControlFrame {
        version: PROTOCOL_VERSION,
        kind,
        payload,
    }
}

pub fn encode_control_frame(frame: &ControlFrame) -> Result<Vec<u8>, ProtocolError> {
    if frame.version != PROTOCOL_VERSION {
        return Err(ProtocolError::UnsupportedVersion);
    }
    let mut blob = serde_json::to_vec(frame).map_err(|_| ProtocolError::InvalidEncoding)?;
    if blob.len() > CONTROL_MAX_BYTES {
        return Err(ProtocolError::FrameTooLarge);
    }
    blob.push(b'\n');
    Ok(blob)
}

pub fn decode_control_frame(blob: &[u8]) -> Result<ControlFrame, ProtocolError> {
    let raw = if let Some(stripped) = blob.strip_suffix(b"\n") {
        stripped
    } else {
        blob
    };
    if raw.len() > CONTROL_MAX_BYTES {
        return Err(ProtocolError::FrameTooLarge);
    }
    let frame: ControlFrame =
        serde_json::from_slice(raw).map_err(|_| ProtocolError::InvalidEncoding)?;
    if frame.version != PROTOCOL_VERSION {
        return Err(ProtocolError::UnsupportedVersion);
    }
    Ok(frame)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_control_frame() {
        let mut payload = Map::new();
        payload.insert("session_id".to_string(), Value::String("abc123".to_string()));
        payload.insert("size".to_string(), Value::from(1024));

        let original = make_control_frame(FrameKind::Offer, payload);
        let encoded = encode_control_frame(&original).expect("frame should encode");
        let decoded = decode_control_frame(&encoded).expect("frame should decode");

        assert_eq!(decoded.version, PROTOCOL_VERSION);
        assert_eq!(decoded.kind, FrameKind::Offer);
        assert_eq!(decoded.payload.get("session_id"), Some(&Value::String("abc123".to_string())));
    }

    #[test]
    fn rejects_unknown_version() {
        let frame = ControlFrame {
            version: 99,
            kind: FrameKind::Hello,
            payload: Map::new(),
        };
        let error = encode_control_frame(&frame).expect_err("unsupported versions should fail");
        assert_eq!(error, ProtocolError::UnsupportedVersion);
    }

    #[test]
    fn rejects_oversized_frame() {
        let mut payload = Map::new();
        payload.insert("blob".to_string(), Value::String("x".repeat(CONTROL_MAX_BYTES)));
        let frame = make_control_frame(FrameKind::Chunk, payload);
        let error = encode_control_frame(&frame).expect_err("oversized frame should fail");
        assert_eq!(error, ProtocolError::FrameTooLarge);
    }
}


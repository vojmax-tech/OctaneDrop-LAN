use std::env;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub app_name: String,
    pub device_name: String,
    pub mode: RunMode,
    pub host: String,
    pub port: u16,
    pub transfer_port: u16,
    pub discovery_port: u16,
    pub discovery_interval_seconds: u64,
    pub discovery_ttl_seconds: u64,
    pub session_max_age_seconds: u64,
    pub max_upload_bytes: u64,
    pub storage_dir: PathBuf,
    pub data_dir: PathBuf,
    pub database_path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    Host,
    P2p,
}

impl RunMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Host => "host",
            Self::P2p => "p2p",
        }
    }
}

impl AppConfig {
    pub fn from_env() -> Self {
        let app_name = env_var("FILESHARE_APP_NAME").unwrap_or_else(|| "VoltDrop".to_string());
        let device_name = env_var("FILESHARE_DEVICE_NAME").unwrap_or_else(hostname_fallback);
        let host = env_var("FILESHARE_HOST").unwrap_or_else(|| "0.0.0.0".to_string());
        let port = env_u16("FILESHARE_PORT").unwrap_or(8765);
        let mode = match env_var("FILESHARE_MODE")
            .unwrap_or_else(|| "host".to_string())
            .to_lowercase()
            .as_str()
        {
            "p2p" => RunMode::P2p,
            _ => RunMode::Host,
        };
        let transfer_port = env_u16("FILESHARE_P2P_TRANSFER_PORT").unwrap_or(port.saturating_add(1));
        let discovery_port = env_u16("FILESHARE_DISCOVERY_PORT").unwrap_or(8766);
        let discovery_interval_seconds = env_u64("FILESHARE_DISCOVERY_INTERVAL_SECONDS").unwrap_or(5);
        let discovery_ttl_seconds = env_u64("FILESHARE_DISCOVERY_TTL_SECONDS").unwrap_or(15);
        let session_max_age_seconds = env_u64("FILESHARE_SESSION_MAX_AGE_SECONDS").unwrap_or(60 * 60 * 12);
        let max_upload_bytes = env_u64("FILESHARE_MAX_UPLOAD_BYTES").unwrap_or(1024_u64.pow(4));
        let data_dir = env_var("FILESHARE_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("data"));
        let storage_dir = env_var("FILESHARE_STORAGE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| data_dir.join("storage"));
        let database_path = data_dir.join("fileshare.db");

        Self {
            app_name,
            device_name,
            mode,
            host,
            port,
            transfer_port,
            discovery_port,
            discovery_interval_seconds,
            discovery_ttl_seconds,
            session_max_age_seconds,
            max_upload_bytes,
            storage_dir,
            data_dir,
            database_path,
        }
    }
}

fn env_var(key: &str) -> Option<String> {
    env::var(key).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

fn env_u16(key: &str) -> Option<u16> {
    env_var(key)?.parse::<u16>().ok()
}

fn env_u64(key: &str) -> Option<u64> {
    env_var(key)?.parse::<u64>().ok()
}

fn hostname_fallback() -> String {
    env::var("COMPUTERNAME")
        .or_else(|_| env::var("HOSTNAME"))
        .unwrap_or_else(|_| "Unknown Device".to_string())
}

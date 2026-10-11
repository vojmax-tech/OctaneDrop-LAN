use std::collections::HashMap;
use std::net::UdpSocket as StdUdpSocket;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tokio::net::UdpSocket;
use tokio::sync::{watch, RwLock};
use tokio::time::{sleep, timeout, Duration};
use tracing::warn;
use voltdrop_p2p_core::discovery::DiscoveryPayload;

use crate::config::AppConfig;

#[derive(Debug, Clone, Serialize)]
pub struct Peer {
    pub id: String,
    pub device_name: String,
    pub app_name: String,
    pub host: String,
    pub port: u16,
    pub transfer_port: u16,
    pub last_seen: f64,
}

pub struct DiscoveryService {
    config: AppConfig,
    peers: Arc<RwLock<HashMap<String, Peer>>>,
    stop_tx: watch::Sender<bool>,
}

impl DiscoveryService {
    pub fn new(config: AppConfig) -> Self {
        let (stop_tx, _) = watch::channel(false);
        Self {
            config,
            peers: Arc::new(RwLock::new(HashMap::new())),
            stop_tx,
        }
    }

    pub fn start(&self) {
        let mut stop_rx = self.stop_tx.subscribe();
        let config = self.config.clone();
        tokio::spawn(async move {
            let socket = match UdpSocket::bind("0.0.0.0:0").await {
                Ok(sock) => sock,
                Err(err) => {
                    warn!("discovery broadcast bind failed: {err}");
                    return;
                }
            };
            if let Err(err) = socket.set_broadcast(true) {
                warn!("discovery broadcast socket setup failed: {err}");
                return;
            }
            loop {
                if *stop_rx.borrow() {
                    break;
                }
                let payload = DiscoveryPayload {
                    app_name: config.app_name.clone(),
                    device_name: config.device_name.clone(),
                    host: advertised_host(&config),
                    port: config.port,
                    transfer_port: config.transfer_port,
                    ts: now_ts_i64(),
                };
                match payload.encode_compact() {
                    Ok(blob) => {
                        let target = format!("255.255.255.255:{}", config.discovery_port);
                        if let Err(err) = socket.send_to(&blob, &target).await {
                            warn!("discovery broadcast send failed: {err}");
                        }
                    }
                    Err(err) => warn!("discovery payload encode failed: {err}"),
                }
                sleep(Duration::from_secs(config.discovery_interval_seconds.max(1))).await;
            }
        });

        let mut stop_rx = self.stop_tx.subscribe();
        let peers = self.peers.clone();
        let config = self.config.clone();
        tokio::spawn(async move {
            let bind_addr = format!("0.0.0.0:{}", config.discovery_port);
            let socket = match UdpSocket::bind(&bind_addr).await {
                Ok(sock) => sock,
                Err(err) => {
                    warn!("discovery listener bind failed: {err}");
                    return;
                }
            };
            let mut buf = [0_u8; 4096];

            loop {
                if *stop_rx.borrow() {
                    break;
                }
                let recv = timeout(Duration::from_secs(1), socket.recv_from(&mut buf)).await;
                let Ok(Ok((size, sender))) = recv else {
                    expire_stale_peers(&peers, config.discovery_ttl_seconds).await;
                    continue;
                };
                let payload = match DiscoveryPayload::decode(&buf[..size]) {
                    Ok(next) => next,
                    Err(_) => continue,
                };
                let host = resolve_peer_host(&payload.host, sender);
                if host.is_empty() {
                    continue;
                }
                if payload.port == 0 || payload.transfer_port == 0 {
                    continue;
                }

                let peer_id = format!("{}:{}", host, payload.port);
                let local_id = format!("{}:{}", config.host, config.port);
                if peer_id == local_id {
                    continue;
                }

                let peer = Peer {
                    id: peer_id.clone(),
                    device_name: payload.device_name,
                    app_name: payload.app_name,
                    host,
                    port: payload.port,
                    transfer_port: payload.transfer_port,
                    last_seen: now_ts_f64(),
                };
                peers.write().await.insert(peer_id, peer);
                expire_stale_peers(&peers, config.discovery_ttl_seconds).await;
            }
        });
    }

    pub async fn stop(&self) {
        let _ = self.stop_tx.send(true);
    }

    pub async fn peers(&self) -> Vec<Peer> {
        expire_stale_peers(&self.peers, self.config.discovery_ttl_seconds).await;
        let peers = self.peers.read().await;
        let mut list: Vec<Peer> = peers.values().cloned().collect();
        list.sort_by(|a, b| b.last_seen.total_cmp(&a.last_seen));
        list
    }
}

fn resolve_peer_host(advertised_host: &str, sender: SocketAddr) -> String {
    let candidate = advertised_host.trim();
    if host_is_connectable(candidate) {
        return candidate.to_string();
    }
    sender.ip().to_string()
}

fn host_is_connectable(host: &str) -> bool {
    if host.is_empty() {
        return false;
    }
    let normalized = host.to_lowercase();
    if matches!(normalized.as_str(), "localhost" | "0.0.0.0" | "::" | "::1" | "127.0.0.1") {
        return false;
    }
    true
}

async fn expire_stale_peers(peers: &Arc<RwLock<HashMap<String, Peer>>>, ttl_seconds: u64) {
    let threshold = now_ts_f64() - ttl_seconds as f64;
    peers.write().await.retain(|_, peer| peer.last_seen >= threshold);
}

fn now_ts_f64() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn now_ts_i64() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn advertised_host(config: &AppConfig) -> String {
    if host_is_connectable(&config.host) {
        return config.host.clone();
    }
    detect_primary_private_ip().unwrap_or_else(|| config.host.clone())
}

fn detect_primary_private_ip() -> Option<String> {
    let socket = StdUdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("10.255.255.255:1").ok()?;
    let ip = socket.local_addr().ok()?.ip().to_string();
    if host_is_connectable(&ip) {
        Some(ip)
    } else {
        None
    }
}

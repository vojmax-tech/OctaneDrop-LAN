use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use tokio::fs::{self, File};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{watch, RwLock};
use tracing::warn;
use uuid::Uuid;
use voltdrop_p2p_core::protocol::{
    decode_control_frame, encode_control_frame, make_control_frame, ControlFrame, FrameKind,
};
use voltdrop_p2p_core::session::{SessionDirection, SessionStatus, TransferSession};

use crate::config::AppConfig;
use crate::db::{Database, NewFile};
use crate::session_manager::SessionManager;

const FRAME_CHUNK_BYTES: usize = 24 * 1024;

pub struct TransferService {
    config: AppConfig,
    sessions: Arc<SessionManager>,
    db: Arc<Database>,
    send_tasks: Arc<RwLock<HashMap<String, tokio::task::JoinHandle<()>>>>,
    stop_tx: watch::Sender<bool>,
}

#[derive(Clone)]
pub struct SendFileRequest {
    pub peer_host: String,
    pub peer_port: u16,
    pub file_path: PathBuf,
    pub file_name: String,
}

impl TransferService {
    pub fn new(config: AppConfig, sessions: Arc<SessionManager>, db: Arc<Database>) -> Self {
        let (stop_tx, _) = watch::channel(false);
        Self {
            config,
            sessions,
            db,
            send_tasks: Arc::new(RwLock::new(HashMap::new())),
            stop_tx,
        }
    }

    pub fn start_listener(self: &Arc<Self>) {
        let service = Arc::clone(self);
        let mut stop_rx = service.stop_tx.subscribe();
        tokio::spawn(async move {
            let bind_addr = format!("{}:{}", service.config.host, service.config.transfer_port);
            let listener = match TcpListener::bind(&bind_addr).await {
                Ok(listener) => listener,
                Err(err) => {
                    warn!("p2p listener bind failed: {err}");
                    return;
                }
            };
            loop {
                tokio::select! {
                    _ = stop_rx.changed() => {
                        if *stop_rx.borrow() {
                            break;
                        }
                    }
                    accept_result = listener.accept() => {
                        match accept_result {
                            Ok((stream, _addr)) => {
                                let svc = Arc::clone(&service);
                                tokio::spawn(async move {
                                    if let Err(err) = svc.handle_incoming(stream).await {
                                        warn!("incoming transfer failed: {err}");
                                    }
                                });
                            }
                            Err(err) => warn!("p2p accept failed: {err}"),
                        }
                    }
                }
            }
        });
    }

    pub async fn stop(&self) {
        let _ = self.stop_tx.send(true);
        let mut tasks = self.send_tasks.write().await;
        for (_, handle) in tasks.drain() {
            handle.abort();
        }
    }

    pub async fn begin_send(self: &Arc<Self>, request: SendFileRequest) -> Result<String> {
        let session_id = Uuid::new_v4().simple().to_string();
        let metadata = fs::metadata(&request.file_path)
            .await
            .with_context(|| format!("cannot read {}", request.file_path.display()))?;
        let total_bytes = metadata.len();
        self.sessions
            .register(TransferSession {
                session_id: session_id.clone(),
                direction: SessionDirection::Send,
                peer_host: request.peer_host.clone(),
                peer_port: request.peer_port,
                file_name: request.file_name.clone(),
                total_bytes,
                transferred_bytes: 0,
                status: SessionStatus::Queued,
                error: None,
                started_at: 0.0,
                updated_at: 0.0,
            })
            .await;

        let service = Arc::clone(self);
        let task_session_id = session_id.clone();
        let handle = tokio::spawn(async move {
            let result = service.send_file(task_session_id.clone(), request).await;
            if let Err(err) = result {
                service
                    .sessions
                    .finish(&task_session_id, SessionStatus::Failed, Some(err.to_string()))
                    .await;
            }
            service.send_tasks.write().await.remove(&task_session_id);
        });

        self.send_tasks.write().await.insert(session_id.clone(), handle);
        Ok(session_id)
    }

    pub async fn cancel_send(&self, session_id: &str) -> bool {
        let handle = self.send_tasks.write().await.remove(session_id);
        if let Some(task) = handle {
            task.abort();
            self.sessions
                .finish(session_id, SessionStatus::Canceled, Some("Canceled by user.".to_string()))
                .await;
            return true;
        }
        false
    }

    async fn send_file(&self, session_id: String, request: SendFileRequest) -> Result<()> {
        self.sessions
            .update(&session_id, Some(SessionStatus::Connecting), Some(0), None)
            .await;

        let peer_addr = format!("{}:{}", request.peer_host, request.peer_port);
        let stream = TcpStream::connect(&peer_addr)
            .await
            .with_context(|| format!("cannot connect to {}", peer_addr))?;
        let (reader_half, mut writer_half) = stream.into_split();
        let mut reader = BufReader::new(reader_half);

        let size = fs::metadata(&request.file_path).await?.len();
        let sha256_hex = compute_file_sha256(&request.file_path).await?;

        let mut offer_payload = Map::new();
        offer_payload.insert("session_id".to_string(), Value::String(session_id.clone()));
        offer_payload.insert("filename".to_string(), Value::String(request.file_name.clone()));
        offer_payload.insert("size".to_string(), Value::from(size));
        offer_payload.insert("sha256".to_string(), Value::String(sha256_hex.clone()));
        offer_payload.insert(
            "sender_name".to_string(),
            Value::String(self.config.device_name.clone()),
        );
        offer_payload.insert(
            "content_type".to_string(),
            Value::String("application/octet-stream".to_string()),
        );
        let frame = make_control_frame(FrameKind::Offer, offer_payload);
        write_frame(&mut writer_half, frame).await?;

        let response = read_frame(&mut reader).await?;
        if response.kind != FrameKind::Accept {
            let reason = response
                .payload
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or("peer rejected transfer");
            return Err(anyhow!(reason.to_string()));
        }

        self.sessions
            .update(&session_id, Some(SessionStatus::Sending), Some(0), None)
            .await;

        let mut file = File::open(&request.file_path)
            .await
            .with_context(|| format!("cannot open {}", request.file_path.display()))?;
        let mut buffer = vec![0_u8; FRAME_CHUNK_BYTES];
        let mut sent_bytes: u64 = 0;
        let mut chunk_index: u64 = 0;

        loop {
            let read = file.read(&mut buffer).await?;
            if read == 0 {
                break;
            }
            let encoded_chunk = BASE64_STANDARD.encode(&buffer[..read]);
            let mut payload = Map::new();
            payload.insert("session_id".to_string(), Value::String(session_id.clone()));
            payload.insert("index".to_string(), Value::from(chunk_index));
            payload.insert("data".to_string(), Value::String(encoded_chunk));
            let frame = make_control_frame(FrameKind::Chunk, payload);
            write_frame(&mut writer_half, frame).await?;
            sent_bytes += read as u64;
            chunk_index += 1;
            self.sessions
                .update(
                    &session_id,
                    Some(SessionStatus::Sending),
                    Some(sent_bytes),
                    None,
                )
                .await;
        }

        let mut done_payload = Map::new();
        done_payload.insert("session_id".to_string(), Value::String(session_id.clone()));
        done_payload.insert("total_bytes".to_string(), Value::from(sent_bytes));
        done_payload.insert("sha256".to_string(), Value::String(sha256_hex));
        write_frame(
            &mut writer_half,
            make_control_frame(FrameKind::Done, done_payload),
        )
        .await?;

        self.sessions
            .finish(&session_id, SessionStatus::Completed, None)
            .await;
        Ok(())
    }

    async fn handle_incoming(self: &Arc<Self>, stream: TcpStream) -> Result<()> {
        let peer_addr = stream
            .peer_addr()
            .map(|addr| addr.to_string())
            .unwrap_or_else(|_| "unknown:0".to_string());
        let peer_host = peer_addr
            .split(':')
            .next()
            .map(str::to_string)
            .unwrap_or_else(|| "unknown".to_string());
        let peer_port = peer_addr
            .split(':')
            .last()
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap_or(0);

        let (reader_half, mut writer_half) = stream.into_split();
        let mut reader = BufReader::new(reader_half);
        let offer = read_frame(&mut reader).await?;
        if offer.kind != FrameKind::Offer {
            return Err(anyhow!("expected OFFER frame"));
        }

        let session_id = offer
            .payload
            .get("session_id")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("missing session id"))?
            .to_string();
        let filename = offer
            .payload
            .get("filename")
            .and_then(Value::as_str)
            .unwrap_or("received.bin")
            .to_string();
        let expected_size = offer
            .payload
            .get("size")
            .and_then(Value::as_u64)
            .ok_or_else(|| anyhow!("missing size"))?;
        let expected_sha = offer
            .payload
            .get("sha256")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let content_type = offer
            .payload
            .get("content_type")
            .and_then(Value::as_str)
            .unwrap_or("application/octet-stream")
            .to_string();

        let peer_host_for_db = peer_host.clone();
        self.sessions
            .register(TransferSession {
                session_id: session_id.clone(),
                direction: SessionDirection::Receive,
                peer_host,
                peer_port,
                file_name: filename.clone(),
                total_bytes: expected_size,
                transferred_bytes: 0,
                status: SessionStatus::Receiving,
                error: None,
                started_at: 0.0,
                updated_at: 0.0,
            })
            .await;

        write_frame(
            &mut writer_half,
            make_control_frame(FrameKind::Accept, Map::new()),
        )
        .await?;

        fs::create_dir_all(&self.config.storage_dir).await?;
        let temp_path = self.config.storage_dir.join(format!(".p2p-{session_id}.part"));
        let final_path = self
            .config
            .storage_dir
            .join(format!("{session_id}-{}", sanitize_file_name(&filename)));
        let mut sink = File::create(&temp_path).await?;
        let mut received_bytes: u64 = 0;
        let mut digest = Sha256::new();
        let mut actual_sha_hex = String::new();

        loop {
            let frame = read_frame(&mut reader).await?;
            match frame.kind {
                FrameKind::Chunk => {
                    let chunk = frame
                        .payload
                        .get("data")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow!("invalid chunk payload"))?;
                    let bytes = BASE64_STANDARD
                        .decode(chunk.as_bytes())
                        .map_err(|_| anyhow!("invalid base64 chunk"))?;
                    if bytes.is_empty() {
                        continue;
                    }
                    sink.write_all(&bytes).await?;
                    received_bytes += bytes.len() as u64;
                    digest.update(&bytes);
                    self.sessions
                        .update(
                            &session_id,
                            Some(SessionStatus::Receiving),
                            Some(received_bytes),
                            None,
                        )
                        .await;
                }
                FrameKind::Done => {
                    let done_bytes = frame
                        .payload
                        .get("total_bytes")
                        .and_then(Value::as_u64)
                        .ok_or_else(|| anyhow!("missing total_bytes"))?;
                    let done_sha = frame
                        .payload
                        .get("sha256")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let actual_sha = format!("{:x}", digest.finalize());
                    actual_sha_hex = actual_sha.clone();
                    if done_bytes != received_bytes {
                        return Err(anyhow!("byte count mismatch"));
                    }
                    if expected_size != received_bytes {
                        return Err(anyhow!("offer size mismatch"));
                    }
                    if !expected_sha.is_empty() && expected_sha != actual_sha {
                        return Err(anyhow!("offer checksum mismatch"));
                    }
                    if !done_sha.is_empty() && done_sha != actual_sha {
                        return Err(anyhow!("done checksum mismatch"));
                    }
                    break;
                }
                FrameKind::Error => {
                    let reason = frame
                        .payload
                        .get("reason")
                        .and_then(Value::as_str)
                        .unwrap_or("peer sent error")
                        .to_string();
                    return Err(anyhow!(reason));
                }
                _ => return Err(anyhow!("unexpected frame kind while receiving")),
            }
        }

        sink.flush().await?;
        sink.sync_all().await?;
        drop(sink);
        fs::rename(&temp_path, &final_path).await?;

        let stored_name = final_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_string();
        self.db.insert_file(&NewFile {
            file_id: session_id.clone(),
            original_name: filename.clone(),
            stored_name,
            content_type,
            size_bytes: received_bytes as i64,
            sha256_hex: actual_sha_hex,
            uploaded_by: format!("peer:{peer_host_for_db}"),
        })?;

        self.sessions
            .finish(&session_id, SessionStatus::Completed, None)
            .await;
        Ok(())
    }
}

pub fn session_to_json(session: &TransferSession) -> Value {
    serde_json::json!({
        "session_id": session.session_id,
        "direction": session.direction,
        "peer_host": session.peer_host,
        "peer_port": session.peer_port,
        "file_name": session.file_name,
        "total_bytes": session.total_bytes,
        "transferred_bytes": session.transferred_bytes,
        "progress_percent": session.progress_percent(),
        "status": session.status,
        "error": session.error,
        "started_at": session.started_at,
        "updated_at": session.updated_at,
    })
}

async fn compute_file_sha256(path: &Path) -> Result<String> {
    let mut file = File::open(path)
        .await
        .with_context(|| format!("cannot open {}", path.display()))?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

async fn write_frame<W: AsyncWrite + Unpin>(writer: &mut W, frame: ControlFrame) -> Result<()> {
    let encoded = encode_control_frame(&frame).map_err(|err| anyhow!(err.to_string()))?;
    writer.write_all(&encoded).await?;
    writer.flush().await?;
    Ok(())
}

async fn read_frame<R: AsyncBufRead + Unpin>(reader: &mut R) -> Result<ControlFrame> {
    let mut line = Vec::new();
    let bytes_read = reader.read_until(b'\n', &mut line).await?;
    if bytes_read == 0 {
        return Err(anyhow!("peer closed connection"));
    }
    let frame = decode_control_frame(&line).map_err(|err| anyhow!(err.to_string()))?;
    Ok(frame)
}

fn sanitize_file_name(file_name: &str) -> String {
    let mut out = String::with_capacity(file_name.len());
    for ch in file_name.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "received.bin".to_string()
    } else {
        out
    }
}

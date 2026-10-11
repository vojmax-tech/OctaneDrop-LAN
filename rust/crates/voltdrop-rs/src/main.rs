mod config;
mod discovery_service;
mod auth;
mod db;
mod security;
mod session_manager;
mod transfer_service;

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use axum::extract::{Multipart, Path, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use sha2::{Digest, Sha256};
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::info;
use uuid::Uuid;

use auth::AuthService;
use config::{AppConfig, RunMode};
use db::{Database, NewFile};
use discovery_service::DiscoveryService;
use security::{hash_password, verify_password};
use session_manager::SessionManager;
use transfer_service::{session_to_json, SendFileRequest, TransferService};

#[derive(Clone)]
struct AppState {
    config: AppConfig,
    discovery: Arc<DiscoveryService>,
    sessions: Arc<SessionManager>,
    transfer: Arc<TransferService>,
    auth: Arc<AuthService>,
    db: Arc<Database>,
}

#[derive(Debug, Deserialize)]
struct PeerSendPayload {
    host: String,
    transfer_port: u16,
}

#[derive(Debug, Deserialize)]
struct LoginPayload {
    username: String,
    password: String,
}

#[derive(Debug, Deserialize)]
struct BootstrapPayload {
    username: String,
    password: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let config = AppConfig::from_env();
    tokio::fs::create_dir_all(&config.data_dir).await?;
    tokio::fs::create_dir_all(&config.storage_dir).await?;
    let db = Arc::new(Database::new(config.database_path.clone()));
    db.init()?;

    let discovery = Arc::new(DiscoveryService::new(config.clone()));
    let auth = Arc::new(AuthService::new(config.session_max_age_seconds));
    let sessions = Arc::new(SessionManager::new(40));
    let transfer = Arc::new(TransferService::new(config.clone(), sessions.clone(), db.clone()));
    transfer.start_listener();
    if config.mode == RunMode::P2p {
        discovery.start();
    }

    let state = AppState {
        config: config.clone(),
        discovery,
        sessions,
        transfer,
        auth,
        db,
    };

    let app = Router::new()
        .route("/", get(root))
        .route("/healthz", get(healthz))
        .route("/api/admin/bootstrap", post(bootstrap_admin))
        .route("/api/auth/login", post(login))
        .route("/api/auth/logout", post(logout))
        .route("/api/auth/me", get(auth_me))
        .route("/api/files", get(list_files))
        .route("/api/files/upload", post(upload_files))
        .route("/api/files/{file_id}/download", get(download_file))
        .route("/api/files/{file_id}/delete", post(delete_file))
        .route("/api/peers", get(peers))
        .route("/api/p2p/sessions", get(p2p_sessions))
        .route("/api/p2p/send/{file_id}", post(p2p_send))
        .route("/api/p2p/cancel/{session_id}", post(p2p_cancel))
        .with_state(state);

    let addr: SocketAddr = format!("{}:{}", config.host, config.port).parse()?;
    info!("Starting VoltDrop Rust server");
    info!("Mode: {}", config.mode.as_str());
    info!("HTTP: http://{}", addr);
    info!("P2P transfer listener: {}:{}", config.host, config.transfer_port);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn root(State(state): State<AppState>) -> impl IntoResponse {
    Json(json!({
        "app": state.config.app_name,
        "mode": state.config.mode.as_str(),
        "message": "VoltDrop Rust API is running.",
    }))
}

async fn healthz() -> impl IntoResponse {
    Json(json!({"status": "ok"}))
}

async fn bootstrap_admin(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<BootstrapPayload>,
) -> impl IntoResponse {
    if payload.username.trim().is_empty() || payload.password.len() < 10 {
        return error_json(StatusCode::BAD_REQUEST, "username is required and password must be at least 10 characters.");
    }

    match state.db.count_users() {
        Ok(count) if count > 0 => {
            if require_auth(&state, &headers).await.is_err() {
                return error_json(StatusCode::UNAUTHORIZED, "authentication required to reset existing admin.");
            }
        }
        Ok(_) => {}
        Err(err) => return error_json(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    }

    let password_hash = match hash_password(payload.password.trim()) {
        Ok(hash) => hash,
        Err(err) => return error_json(StatusCode::BAD_REQUEST, &err.to_string()),
    };
    if let Err(err) = state.db.upsert_user(payload.username.trim(), &password_hash) {
        return error_json(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string());
    }
    Json(json!({"status":"ok","username":payload.username.trim()})).into_response()
}

async fn login(State(state): State<AppState>, Json(payload): Json<LoginPayload>) -> impl IntoResponse {
    let username = payload.username.trim();
    if username.is_empty() || payload.password.is_empty() {
        return error_json(StatusCode::BAD_REQUEST, "username and password are required.");
    }
    let user = match state.db.get_user_by_username(username) {
        Ok(Some(user)) => user,
        Ok(None) => return error_json(StatusCode::UNAUTHORIZED, "invalid username or password."),
        Err(err) => return error_json(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    };
    if !verify_password(&payload.password, &user.password_hash) {
        return error_json(StatusCode::UNAUTHORIZED, "invalid username or password.");
    }
    let token = state.auth.issue_session(&user.username).await;
    Json(json!({"token": token, "username": user.username})).into_response()
}

async fn logout(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    let Some(token) = extract_bearer_token(&headers) else {
        return error_json(StatusCode::BAD_REQUEST, "Authorization header is required.");
    };
    state.auth.revoke(&token).await;
    Json(json!({"status":"ok"})).into_response()
}

async fn auth_me(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    match require_auth(&state, &headers).await {
        Ok(username) => Json(json!({"username": username})).into_response(),
        Err(resp) => resp,
    }
}

async fn list_files(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if let Err(resp) = require_auth(&state, &headers).await {
        return resp;
    }
    match state.db.list_files() {
        Ok(files) => Json(json!({"files": files})).into_response(),
        Err(err) => error_json(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    }
}

async fn upload_files(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> impl IntoResponse {
    let username = match require_auth(&state, &headers).await {
        Ok(username) => username,
        Err(resp) => return resp,
    };

    let mut uploaded: Vec<Value> = Vec::new();
    loop {
        let field = match multipart.next_field().await {
            Ok(Some(field)) => field,
            Ok(None) => break,
            Err(err) => return error_json(StatusCode::BAD_REQUEST, &format!("invalid multipart payload: {err}")),
        };
        let original_name = field
            .file_name()
            .map(sanitize_file_name)
            .unwrap_or_else(|| "upload.bin".to_string());
        let content_type = field
            .content_type()
            .map(|value| value.to_string())
            .unwrap_or_else(|| "application/octet-stream".to_string());
        let bytes = match field.bytes().await {
            Ok(bytes) => bytes,
            Err(err) => return error_json(StatusCode::BAD_REQUEST, &format!("could not read upload field: {err}")),
        };
        if bytes.is_empty() {
            continue;
        }
        if (bytes.len() as u64) > state.config.max_upload_bytes {
            return error_json(
                StatusCode::BAD_REQUEST,
                &format!("file exceeds max upload size of {} bytes", state.config.max_upload_bytes),
            );
        }

        let mut sha = Sha256::new();
        sha.update(&bytes);
        let sha_hex = format!("{:x}", sha.finalize());
        let file_id = Uuid::new_v4().simple().to_string();
        let stored_name = format!("{file_id}.bin");
        let disk_path = state.config.storage_dir.join(&stored_name);
        if let Err(err) = tokio::fs::write(&disk_path, &bytes).await {
            return error_json(StatusCode::INTERNAL_SERVER_ERROR, &format!("failed to store file: {err}"));
        }
        let record = NewFile {
            file_id: file_id.clone(),
            original_name: original_name.clone(),
            stored_name: stored_name.clone(),
            content_type: content_type.clone(),
            size_bytes: bytes.len() as i64,
            sha256_hex: sha_hex.clone(),
            uploaded_by: username.clone(),
        };
        if let Err(err) = state.db.insert_file(&record) {
            let _ = tokio::fs::remove_file(&disk_path).await;
            return error_json(StatusCode::INTERNAL_SERVER_ERROR, &format!("failed to save file metadata: {err}"));
        }
        uploaded.push(json!({
            "id": file_id,
            "name": original_name,
            "size_bytes": bytes.len(),
            "sha256": sha_hex,
        }));
    }

    if uploaded.is_empty() {
        return error_json(StatusCode::BAD_REQUEST, "no file fields were provided.");
    }
    Json(json!({"uploaded": uploaded})).into_response()
}

async fn download_file(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(file_id): Path<String>,
) -> impl IntoResponse {
    if let Err(resp) = require_auth(&state, &headers).await {
        return resp;
    }
    let file = match state.db.get_file(&file_id) {
        Ok(Some(file)) => file,
        Ok(None) => return error_json(StatusCode::NOT_FOUND, "file not found."),
        Err(err) => return error_json(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    };

    let disk_path = state.config.storage_dir.join(&file.stored_name);
    let bytes = match tokio::fs::read(&disk_path).await {
        Ok(bytes) => bytes,
        Err(_) => return error_json(StatusCode::GONE, "stored file is missing from disk."),
    };

    let mut response_headers = HeaderMap::new();
    let content_type = HeaderValue::from_str(&file.content_type)
        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));
    response_headers.insert(header::CONTENT_TYPE, content_type);
    response_headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{}\"", file.original_name.replace('"', "_")))
            .unwrap_or_else(|_| HeaderValue::from_static("attachment")),
    );
    response_headers.insert(
        "x-checksum-sha256",
        HeaderValue::from_str(&file.sha256_hex).unwrap_or_else(|_| HeaderValue::from_static("invalid")),
    );

    (response_headers, bytes).into_response()
}

async fn delete_file(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(file_id): Path<String>,
) -> impl IntoResponse {
    if let Err(resp) = require_auth(&state, &headers).await {
        return resp;
    }
    let file = match state.db.get_file(&file_id) {
        Ok(Some(file)) => file,
        Ok(None) => return error_json(StatusCode::NOT_FOUND, "file not found."),
        Err(err) => return error_json(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    };
    let disk_path = state.config.storage_dir.join(&file.stored_name);
    let _ = tokio::fs::remove_file(&disk_path).await;
    if let Err(err) = state.db.delete_file(&file_id) {
        return error_json(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string());
    }
    Json(json!({"status":"deleted","id":file_id})).into_response()
}

async fn peers(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if let Err(resp) = require_auth(&state, &headers).await {
        return resp;
    }
    if state.config.mode != RunMode::P2p {
        return Json(json!({"mode": state.config.mode.as_str(), "peers": []})).into_response();
    }
    let peers = state.discovery.peers().await;
    Json(json!({"mode": state.config.mode.as_str(), "peers": peers})).into_response()
}

async fn p2p_sessions(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if let Err(resp) = require_auth(&state, &headers).await {
        return resp;
    }
    let sessions = state.sessions.list_all().await;
    let serialized: Vec<Value> = sessions.iter().map(session_to_json).collect();
    Json(json!({"mode": state.config.mode.as_str(), "sessions": serialized})).into_response()
}

async fn p2p_send(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(file_id): Path<String>,
    Json(payload): Json<PeerSendPayload>,
) -> impl IntoResponse {
    if let Err(resp) = require_auth(&state, &headers).await {
        return resp;
    }
    if state.config.mode != RunMode::P2p {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": "P2P mode is not enabled."})))
            .into_response();
    }
    if payload.host.trim().is_empty() || payload.transfer_port == 0 {
        return error_json(StatusCode::BAD_REQUEST, "host and transfer_port are required.");
    }
    let file = match state.db.get_file(&file_id) {
        Ok(Some(file)) => file,
        Ok(None) => return error_json(StatusCode::NOT_FOUND, "file not found."),
        Err(err) => return error_json(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    };
    let file_path = state.config.storage_dir.join(&file.stored_name);
    if !file_path.exists() {
        return error_json(StatusCode::GONE, "stored file is missing from disk.");
    }
    let request = SendFileRequest {
        peer_host: payload.host.trim().to_string(),
        peer_port: payload.transfer_port,
        file_path,
        file_name: file.original_name,
    };

    match state.transfer.begin_send(request).await {
        Ok(session_id) => (
            StatusCode::OK,
            Json(json!({"status": "accepted", "session_id": session_id})),
        )
            .into_response(),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": err.to_string()})),
        )
            .into_response(),
    }
}

async fn p2p_cancel(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    if let Err(resp) = require_auth(&state, &headers).await {
        return resp;
    }
    if state.transfer.cancel_send(&session_id).await {
        return Json(json!({"status": "cancel_requested", "session_id": session_id})).into_response();
    }
    (StatusCode::NOT_FOUND, Json(json!({"error": "Session not active or not found."}))).into_response()
}

async fn require_auth(state: &AppState, headers: &HeaderMap) -> std::result::Result<String, axum::response::Response> {
    let Some(token) = extract_bearer_token(headers) else {
        return Err(error_json(StatusCode::UNAUTHORIZED, "Authorization: Bearer <token> is required."));
    };
    let Some(username) = state.auth.verify(&token).await else {
        return Err(error_json(StatusCode::UNAUTHORIZED, "invalid or expired token."));
    };
    Ok(username)
}

fn extract_bearer_token(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?.trim().to_string();
    let lower = value.to_lowercase();
    if !lower.starts_with("bearer ") {
        return None;
    }
    Some(value[7..].trim().to_string())
}

fn error_json(status: StatusCode, message: &str) -> axum::response::Response {
    (status, Json(json!({"error": message}))).into_response()
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
        "upload.bin".to_string()
    } else {
        out
    }
}

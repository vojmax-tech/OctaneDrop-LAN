# VoltDrop Rust Workspace

This workspace contains the Rust implementation of VoltDrop.

## Crates

- `voltdrop-p2p-core`: protocol, discovery, and session primitives
- `voltdrop-rs`: runnable server with auth, file APIs, and P2P services

## Run

From the repository root:

```bash
cd rust
cargo run -p voltdrop-rs
```

P2P mode:

```bash
cd rust
FILESHARE_MODE=p2p cargo run -p voltdrop-rs
```

## API

- `GET /healthz`
- `POST /api/admin/bootstrap`
- `POST /api/auth/login`
- `POST /api/auth/logout`
- `GET /api/auth/me`
- `GET /api/files`
- `POST /api/files/upload` (multipart form upload)
- `GET /api/files/{file_id}/download`
- `POST /api/files/{file_id}/delete`
- `GET /api/peers`
- `GET /api/p2p/sessions`
- `POST /api/p2p/send/{file_id}` with JSON: `{"host":"...","transfer_port":8766}`
- `POST /api/p2p/cancel/{session_id}`

Most routes require:

```text
Authorization: Bearer <token>
```

## Auth Bootstrap Example

Create first admin account:

```bash
curl -X POST http://127.0.0.1:8765/api/admin/bootstrap \
  -H "content-type: application/json" \
  -d '{"username":"admin","password":"your-long-password"}'
```

Login:

```bash
curl -X POST http://127.0.0.1:8765/api/auth/login \
  -H "content-type: application/json" \
  -d '{"username":"admin","password":"your-long-password"}'
```

Use token:

```bash
curl http://127.0.0.1:8765/api/files -H "Authorization: Bearer <token>"
```

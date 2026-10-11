# VoltDrop Rust Migration Plan

This document tracks the incremental migration from Python to Rust without breaking the current app.

## Current Status

Migration started with a new Rust workspace:

- `rust/Cargo.toml` (workspace)
- `rust/crates/voltdrop-p2p-core` (new crate)
- `rust/crates/voltdrop-rs` (runnable Rust server)

Implemented in Rust (phase 1 foundations):

- P2P control-frame model and encoder/decoder (`protocol.rs`)
- Discovery payload model and compact JSON codec (`discovery.rs`)
- Transfer session state model (`session.rs`)
- HTTP API server with P2P routes (`voltdrop-rs`)
- UDP discovery service + TCP transfer service (`voltdrop-rs`)
- SQLite persistence for users/files (`voltdrop-rs/src/db.rs`)
- Token-based auth/session flow (`voltdrop-rs/src/auth.rs`)
- Password hashing/verification compatible with Python `scrypt$...` format (`voltdrop-rs/src/security.rs`)

## Why This Order

P2P protocol and discovery are stable boundaries and do not require replacing the web UI first.
Migrating these first lowers risk and lets us compare Python vs Rust behavior with test fixtures.

## Python -> Rust Mapping

- `python/app/protocol.py` -> `rust/crates/voltdrop-p2p-core/src/protocol.rs`
- `python/app/discovery.py` (payload schema parts) -> `rust/crates/voltdrop-p2p-core/src/discovery.rs`
- `python/app/connection.py` (session state parts) -> `rust/crates/voltdrop-p2p-core/src/session.rs`

## Next Steps

1. Add browser-oriented session cookies + CSRF parity with Python UI model.
2. Add storage encryption-at-rest parity with Python implementation.
3. In Python `python/app/main.py`, introduce an adapter mode:
   - call Rust daemon endpoints when enabled
   - keep current Python implementation as fallback
4. Add parity tests with shared JSON fixtures for frame encode/decode and session states.
5. Migrate UI/backend integration endpoints one-by-one from Python to Rust-first execution.

## Local Build (once Rust toolchain is installed)

```powershell
cd rust
cargo test
```

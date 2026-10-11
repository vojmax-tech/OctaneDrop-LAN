# VoltDrop

VoltDrop is a LAN-first file sharing application.

- Local network transfer with no cloud dependency
- Browser-based Python app (`python/`)
- Rust implementation in progress (`rust/`)
- Peer discovery via UDP and direct transfer via TCP in P2P mode

## Features

- Password-protected access
- File upload/download and deletion
- Authenticated API access
- P2P peer discovery and direct device transfer
- Transfer progress, session tracking, and cancellation
- Windows executable build pipeline

## Repository Layout

```text
.
|-- .github/
|-- python/          # Current stable app (FastAPI + web UI)
|-- rust/            # Rust implementation (workspace + crates)
|-- RUST_MIGRATION.md
`-- README.md
```

## Quick Start (Python App)

1. Create and activate a virtual environment.

```bash
python -m venv .venv
```

PowerShell:

```powershell
.\.venv\Scripts\Activate.ps1
```

2. Install dependencies.

```bash
pip install -r python/requirements.txt
```

3. Create an admin account.

```bash
python python/run.py create-admin --username admin
```

4. Start the server.

Host mode:

```bash
python python/run.py serve --mode host --host 0.0.0.0 --port 8765 --open-browser
```

P2P mode:

```bash
python python/run.py serve --mode p2p --host 0.0.0.0 --port 8765 --open-browser
```

## Desktop App Mode (Windows EXE)

The packaged Windows app launches a native desktop window by default (embedded webview), not an external browser tab.

- Default EXE behavior: desktop window
- Optional fallback: `--no-desktop` (runs as local server and can open browser)
- Desktop window backend: `pywebview` (included by `python/requirements.txt`)
- Windows runtime: Microsoft Edge WebView2 Runtime

Source launcher commands:

```bash
python python/launcher.py --desktop
python python/launcher.py --no-desktop
```

## P2P Design

- Discovery: UDP broadcast/listen
- Transfer: TCP stream with framed control messages
- Session states: queued, connecting, sending, receiving, persisting, completed, failed, canceled

Primary Python modules:

- `python/app/discovery.py`
- `python/app/p2p_service.py`
- `python/app/connection.py`
- `python/app/protocol.py`

## Windows EXE Build (Python)

```powershell
cd python
./build_windows_exe.ps1
```

Output:

```text
python/dist/VoltDrop/VoltDrop.exe
```

## Build Troubleshooting (Windows)

If PowerShell blocks the script with an execution policy error:

```powershell
powershell -ExecutionPolicy Bypass -File .\build_windows_exe.ps1
```

If the build fails with `Failed building wheel for pythonnet` (or `nuget.exe update -self` during `pythonnet` install), use Python 3.12 for the desktop build:

```powershell
cd python
py -3.12 -m pip install --upgrade pip
py -3.12 -m pip install -r requirements-build.txt
py -3.12 -m PyInstaller --noconfirm --clean voltdrop_onedir.spec
```

Build scripts now auto-select `py -3.12` when it is installed and stop early with a clear message if the active Python is not 3.12.

If dependency installation still fails, check:

- Internet/firewall access to PyPI
- Proxy settings used by `pip`
- Python package install permissions

Desktop-window mode requires `pywebview`. If it is unavailable, VoltDrop falls back to browser mode.

## CI / Releases

GitHub Actions builds and publishes:

- `python/dist/VoltDrop-windows.zip`
- `python/dist/VoltDrop-windows.zip.sha256`

Workflow file:

- `.github/workflows/build-windows-release.yml`

## Rust Implementation

Rust code lives in `rust/`.

- `voltdrop-p2p-core`: protocol, discovery, and session primitives
- `voltdrop-rs`: runnable API server

Detailed status and migration plan:

- `RUST_MIGRATION.md`


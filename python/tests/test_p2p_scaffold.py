from __future__ import annotations

import secrets
from pathlib import Path

from app.protocol import (
    KIND_HELLO,
    ProtocolError,
    decode_control_frame,
    encode_control_frame,
    make_control_frame,
)
from app.transfer import iter_file_chunks, sha256_file


def test_protocol_frame_roundtrip() -> None:
    frame = make_control_frame(KIND_HELLO, device_name="dev-1", port=8765)
    blob = encode_control_frame(frame)
    decoded = decode_control_frame(blob.strip())
    assert decoded["kind"] == KIND_HELLO
    assert decoded["payload"]["device_name"] == "dev-1"


def test_protocol_rejects_unknown_kind() -> None:
    try:
        make_control_frame("NOPE")
    except ProtocolError:
        return
    raise AssertionError("Expected ProtocolError for unknown kind")


def test_transfer_chunking_and_hash() -> None:
    payload = (b"abc123" * 4096) + b"z"
    temp_root = Path(".test_tmp")
    temp_root.mkdir(parents=True, exist_ok=True)
    path = temp_root / f"payload-{secrets.token_hex(8)}.bin"
    try:
        path.write_bytes(payload)

        rebuilt = b"".join(iter_file_chunks(path, chunk_size=1024))
        digest = sha256_file(path, chunk_size=2048)

        assert rebuilt == payload
        assert len(digest) == 64
    finally:
        path.unlink(missing_ok=True)

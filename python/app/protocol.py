from __future__ import annotations

import json
from typing import Any

PROTOCOL_VERSION = 1
CONTROL_MAX_BYTES = 64 * 1024

KIND_HELLO = "HELLO"
KIND_OFFER = "OFFER"
KIND_ACCEPT = "ACCEPT"
KIND_REJECT = "REJECT"
KIND_CHUNK = "CHUNK"
KIND_DONE = "DONE"
KIND_ERROR = "ERROR"

VALID_KINDS = {
    KIND_HELLO,
    KIND_OFFER,
    KIND_ACCEPT,
    KIND_REJECT,
    KIND_CHUNK,
    KIND_DONE,
    KIND_ERROR,
}


class ProtocolError(ValueError):
    pass


def make_control_frame(kind: str, **payload: Any) -> dict[str, Any]:
    if kind not in VALID_KINDS:
        raise ProtocolError(f"Unsupported frame kind: {kind}")
    return {
        "v": PROTOCOL_VERSION,
        "kind": kind,
        "payload": payload,
    }


def encode_control_frame(frame: dict[str, Any]) -> bytes:
    blob = json.dumps(frame, separators=(",", ":"), ensure_ascii=True).encode("utf-8")
    if len(blob) > CONTROL_MAX_BYTES:
        raise ProtocolError("Control frame is too large.")
    return blob + b"\n"


def decode_control_frame(blob: bytes) -> dict[str, Any]:
    if len(blob) > CONTROL_MAX_BYTES:
        raise ProtocolError("Control frame exceeds allowed size.")
    try:
        frame = json.loads(blob.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise ProtocolError("Invalid control frame encoding.") from exc

    if not isinstance(frame, dict):
        raise ProtocolError("Control frame must be a JSON object.")
    if frame.get("v") != PROTOCOL_VERSION:
        raise ProtocolError("Unsupported protocol version.")
    kind = frame.get("kind")
    payload = frame.get("payload")
    if kind not in VALID_KINDS:
        raise ProtocolError("Unsupported frame kind.")
    if not isinstance(payload, dict):
        raise ProtocolError("Control frame payload must be an object.")
    return frame

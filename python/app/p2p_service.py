from __future__ import annotations

import asyncio
import base64
import contextlib
import hashlib
import secrets
from dataclasses import dataclass
from pathlib import Path
from typing import Awaitable, Callable

from .config import Settings
from .connection import ConnectionManager, TransferSession
from .db import Database, StoredFile
from .protocol import (
    KIND_ACCEPT,
    KIND_CHUNK,
    KIND_DONE,
    KIND_ERROR,
    KIND_OFFER,
    ProtocolError,
    decode_control_frame,
    encode_control_frame,
    make_control_frame,
)
from .security import FileCipher, StreamingDecryptError

MAX_FRAME_CHUNK_BYTES = 24 * 1024


class P2PTransferError(RuntimeError):
    pass


@dataclass(slots=True)
class P2PSendResult:
    session_id: str
    peer: str
    bytes_sent: int
    sha256_hex: str


class P2PTransferService:
    def __init__(
        self,
        *,
        settings: Settings,
        db: Database,
        file_cipher: FileCipher,
        connection_manager: ConnectionManager,
        on_files_changed: Callable[[], Awaitable[None]] | None = None,
    ) -> None:
        self._settings = settings
        self._db = db
        self._file_cipher = file_cipher
        self._connection_manager = connection_manager
        self._on_files_changed = on_files_changed
        self._server: asyncio.AbstractServer | None = None
        self._send_tasks: dict[str, asyncio.Task[None]] = {}

    async def start(self) -> None:
        if self._server:
            return
        self._server = await asyncio.start_server(
            self._handle_client,
            host=self._settings.host,
            port=self._settings.p2p_transfer_port,
        )

    async def stop(self) -> None:
        if self._server:
            self._server.close()
            await self._server.wait_closed()
            self._server = None
        tasks = list(self._send_tasks.values())
        for task in tasks:
            task.cancel()
        for task in tasks:
            with contextlib.suppress(asyncio.CancelledError):
                await task
        self._send_tasks.clear()

    async def begin_send(
        self,
        *,
        stored: StoredFile,
        disk_path: Path,
        peer_host: str,
        peer_port: int,
    ) -> str:
        session_id = secrets.token_urlsafe(10)
        task = asyncio.create_task(
            self._background_send(
                session_id=session_id,
                stored=stored,
                disk_path=disk_path,
                peer_host=peer_host,
                peer_port=peer_port,
            )
        )
        self._send_tasks[session_id] = task
        task.add_done_callback(lambda _: self._send_tasks.pop(session_id, None))
        return session_id

    async def cancel_send(self, session_id: str) -> bool:
        task = self._send_tasks.get(session_id)
        if not task:
            return False
        task.cancel()
        with contextlib.suppress(asyncio.CancelledError):
            await task
        return True

    async def _background_send(
        self,
        *,
        session_id: str,
        stored: StoredFile,
        disk_path: Path,
        peer_host: str,
        peer_port: int,
    ) -> None:
        try:
            await self.send_file_to_peer(
                stored=stored,
                disk_path=disk_path,
                peer_host=peer_host,
                peer_port=peer_port,
                session_id=session_id,
            )
        except asyncio.CancelledError:
            pass
        except Exception:
            pass

    async def send_file_to_peer(
        self,
        *,
        stored: StoredFile,
        disk_path: Path,
        peer_host: str,
        peer_port: int,
        session_id: str | None = None,
    ) -> P2PSendResult:
        session_id = session_id or secrets.token_urlsafe(10)
        await self._connection_manager.register(
            TransferSession(
                session_id=session_id,
                direction="send",
                peer_host=peer_host,
                peer_port=peer_port,
                file_name=stored.original_name,
                total_bytes=stored.size_bytes,
                status="queued",
            )
        )
        writer: asyncio.StreamWriter | None = None
        try:
            await self._connection_manager.update(session_id, status="connecting", transferred_bytes=0)
            try:
                reader, writer = await asyncio.open_connection(peer_host, peer_port)
            except OSError as exc:
                raise P2PTransferError(f"Could not connect to peer {peer_host}:{peer_port}") from exc

            await self._write_frame(
                writer,
                make_control_frame(
                    KIND_OFFER,
                    session_id=session_id,
                    filename=stored.original_name,
                    size=stored.size_bytes,
                    sha256=stored.sha256_hex,
                    sender_name=self._settings.device_name,
                    content_type=stored.content_type or "application/octet-stream",
                ),
            )
            response = await self._read_frame(reader)
            if response["kind"] != KIND_ACCEPT:
                reason = response.get("payload", {}).get("reason", "peer rejected transfer")
                raise P2PTransferError(str(reason))

            await self._connection_manager.update(session_id, status="sending", transferred_bytes=0)
            sent = 0
            chunk_index = 0
            try:
                plaintext_iterator = self._file_cipher.iter_decrypt_path(disk_path, expected_plaintext_size=stored.size_bytes)
            except StreamingDecryptError as exc:
                raise P2PTransferError("Stored source file could not be decrypted for sending.") from exc

            for chunk in plaintext_iterator:
                for bounded in _iter_bounded_chunks(chunk, max_size=MAX_FRAME_CHUNK_BYTES):
                    await self._write_frame(
                        writer,
                        make_control_frame(
                            KIND_CHUNK,
                            session_id=session_id,
                            index=chunk_index,
                            data=base64.b64encode(bounded).decode("ascii"),
                        ),
                    )
                    sent += len(bounded)
                    chunk_index += 1
                    await self._connection_manager.update(session_id, status="sending", transferred_bytes=sent)

            await self._write_frame(
                writer,
                make_control_frame(
                    KIND_DONE,
                    session_id=session_id,
                    total_bytes=sent,
                    sha256=stored.sha256_hex,
                ),
            )
            await self._connection_manager.finish(session_id, status="completed")
            return P2PSendResult(
                session_id=session_id,
                peer=f"{peer_host}:{peer_port}",
                bytes_sent=sent,
                sha256_hex=stored.sha256_hex,
            )
        except asyncio.CancelledError:
            with contextlib.suppress(Exception):
                if writer:
                    await self._write_frame(
                        writer,
                        make_control_frame(KIND_ERROR, session_id=session_id, reason="Transfer canceled by sender."),
                    )
            await self._connection_manager.finish(session_id, status="canceled", error="Canceled by user.")
            raise
        except Exception as exc:
            error_text = str(exc) or exc.__class__.__name__
            await self._connection_manager.finish(session_id, status="failed", error=error_text)
            if isinstance(exc, P2PTransferError):
                raise
            raise P2PTransferError(error_text) from exc
        finally:
            if writer:
                writer.close()
                await writer.wait_closed()

    async def _handle_client(self, reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
        peer_addr = writer.get_extra_info("peername")
        remote_host = peer_addr[0] if isinstance(peer_addr, tuple) and peer_addr else "unknown"
        plaintext_temp_path: Path | None = None
        buffer_file = None
        digest = hashlib.sha256()
        bytes_received = 0
        session_id = ""
        try:
            offer_frame = await self._read_frame(reader)
            if offer_frame["kind"] != KIND_OFFER:
                raise P2PTransferError("Expected OFFER frame first.")

            payload = offer_frame["payload"]
            session_id = str(payload.get("session_id") or "")
            filename = str(payload.get("filename") or "received.bin")
            content_type = str(payload.get("content_type") or "application/octet-stream")
            expected_size = int(payload.get("size", 0))
            expected_sha = str(payload.get("sha256") or "")
            if not session_id:
                raise P2PTransferError("Missing session id.")
            if expected_size <= 0 or expected_size > self._settings.max_upload_bytes:
                raise P2PTransferError("Incoming file size is invalid or exceeds configured limits.")
            if len(expected_sha) != 64:
                raise P2PTransferError("Incoming SHA-256 checksum is invalid.")

            await self._connection_manager.register(
                TransferSession(
                    session_id=session_id,
                    direction="receive",
                    peer_host=remote_host,
                    peer_port=int(peer_addr[1]) if isinstance(peer_addr, tuple) else 0,
                    file_name=filename,
                    total_bytes=expected_size,
                    status="receiving",
                )
            )
            await self._write_frame(writer, make_control_frame(KIND_ACCEPT, session_id=session_id))

            plaintext_temp_path = self._settings.storage_dir / f".p2p-{session_id}.plain.part"
            buffer_file = plaintext_temp_path.open("wb")

            while True:
                frame = await self._read_frame(reader)
                kind = frame["kind"]
                payload = frame["payload"]
                if kind == KIND_CHUNK:
                    chunk_session = str(payload.get("session_id") or "")
                    if chunk_session != session_id:
                        raise P2PTransferError("Chunk session mismatch.")
                    encoded = payload.get("data")
                    if not isinstance(encoded, str):
                        raise P2PTransferError("Chunk frame has invalid payload.")
                    try:
                        chunk = base64.b64decode(encoded.encode("ascii"), validate=True)
                    except (ValueError, UnicodeEncodeError) as exc:
                        raise P2PTransferError("Chunk payload is not valid base64.") from exc
                    if not chunk:
                        continue
                    bytes_received += len(chunk)
                    if bytes_received > self._settings.max_upload_bytes:
                        raise P2PTransferError("Incoming file exceeded maximum allowed size.")
                    digest.update(chunk)
                    buffer_file.write(chunk)
                    await self._connection_manager.update(
                        session_id,
                        status="receiving",
                        transferred_bytes=bytes_received,
                    )
                    continue

                if kind == KIND_DONE:
                    done_session = str(payload.get("session_id") or "")
                    done_bytes = int(payload.get("total_bytes", -1))
                    done_sha = str(payload.get("sha256") or "")
                    if done_session != session_id:
                        raise P2PTransferError("DONE session mismatch.")
                    if done_bytes != bytes_received:
                        raise P2PTransferError("Transferred byte count does not match DONE metadata.")
                    actual_sha = digest.hexdigest()
                    if done_sha and done_sha != actual_sha:
                        raise P2PTransferError("Transferred file checksum mismatch.")
                    if expected_size != bytes_received:
                        raise P2PTransferError("Transferred file size does not match OFFER metadata.")
                    if expected_sha != actual_sha:
                        raise P2PTransferError("Transferred file checksum does not match OFFER metadata.")
                    break

                if kind == KIND_ERROR:
                    reason = str(payload.get("reason") or "peer sent error")
                    raise P2PTransferError(reason)

                raise P2PTransferError(f"Unexpected frame kind while receiving: {kind}")

            buffer_file.close()
            buffer_file = None
            await self._connection_manager.update(session_id, status="persisting", transferred_bytes=bytes_received)

            await asyncio.to_thread(
                self._persist_received_plaintext,
                plaintext_temp_path,
                filename,
                content_type,
                bytes_received,
                digest.hexdigest(),
                remote_host,
            )
            await self._connection_manager.finish(session_id, status="completed")
            if self._on_files_changed:
                await self._on_files_changed()
        except Exception as exc:
            if session_id:
                await self._connection_manager.finish(session_id, status="failed", error=str(exc) or exc.__class__.__name__)
                with contextlib.suppress(Exception):
                    await self._write_frame(
                        writer,
                        make_control_frame(KIND_ERROR, session_id=session_id, reason=str(exc) or exc.__class__.__name__),
                    )
        finally:
            if buffer_file:
                buffer_file.close()
            if plaintext_temp_path:
                plaintext_temp_path.unlink(missing_ok=True)
            writer.close()
            await writer.wait_closed()

    def _persist_received_plaintext(
        self,
        plaintext_path: Path,
        original_name: str,
        content_type: str,
        size_bytes: int,
        sha256_hex: str,
        remote_host: str,
    ) -> None:
        file_id = secrets.token_urlsafe(12)
        stored_name = f"{file_id}.bin"
        temp_encrypted = self._settings.storage_dir / f".{stored_name}.part"
        final_encrypted = self._settings.storage_dir / stored_name

        with plaintext_path.open("rb") as source:
            encrypted_size, encrypted_sha = self._file_cipher.encrypt_fileobj_to_path(
                source,
                temp_encrypted,
                max_plaintext_bytes=self._settings.max_upload_bytes,
                chunk_size=self._settings.stream_chunk_bytes,
            )
        if encrypted_size != size_bytes:
            temp_encrypted.unlink(missing_ok=True)
            raise P2PTransferError("Encrypted persisted size mismatch after receive.")
        if encrypted_sha != sha256_hex:
            temp_encrypted.unlink(missing_ok=True)
            raise P2PTransferError("Encrypted persisted checksum mismatch after receive.")

        temp_encrypted.replace(final_encrypted)
        self._db.insert_file(
            file_id=file_id,
            original_name=Path(original_name).name or "received.bin",
            stored_name=stored_name,
            content_type=content_type or "application/octet-stream",
            size_bytes=size_bytes,
            sha256_hex=sha256_hex,
            uploaded_by=f"peer:{remote_host}",
        )

    async def _write_frame(self, writer: asyncio.StreamWriter, frame: dict[str, object]) -> None:
        writer.write(encode_control_frame(frame))
        await writer.drain()

    async def _read_frame(self, reader: asyncio.StreamReader) -> dict[str, object]:
        line = await reader.readline()
        if not line:
            raise P2PTransferError("Peer closed the connection unexpectedly.")
        try:
            return decode_control_frame(line.strip())
        except ProtocolError as exc:
            raise P2PTransferError(str(exc)) from exc


def _iter_bounded_chunks(payload: bytes, *, max_size: int) -> list[bytes]:
    if max_size <= 0:
        raise ValueError("max_size must be positive")
    if not payload:
        return []
    return [payload[i : i + max_size] for i in range(0, len(payload), max_size)]

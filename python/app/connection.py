from __future__ import annotations

import asyncio
import time
from collections import deque
from dataclasses import dataclass
from typing import Literal

SessionDirection = Literal["send", "receive"]
SessionStatus = Literal["queued", "connecting", "sending", "receiving", "persisting", "completed", "failed", "canceled"]

@dataclass(slots=True)
class TransferSession:
    session_id: str
    direction: SessionDirection
    peer_host: str
    peer_port: int
    file_name: str
    total_bytes: int
    transferred_bytes: int = 0
    status: SessionStatus = "queued"
    error: str | None = None
    started_at: float = 0.0
    updated_at: float = 0.0

    def as_dict(self) -> dict[str, object]:
        progress_percent = 0
        if self.total_bytes > 0:
            progress_percent = min(100, int((self.transferred_bytes / self.total_bytes) * 100))
        return {
            "session_id": self.session_id,
            "direction": self.direction,
            "peer_host": self.peer_host,
            "peer_port": self.peer_port,
            "file_name": self.file_name,
            "total_bytes": self.total_bytes,
            "transferred_bytes": self.transferred_bytes,
            "progress_percent": progress_percent,
            "status": self.status,
            "error": self.error,
            "started_at": self.started_at,
            "updated_at": self.updated_at,
        }


class ConnectionManager:
    def __init__(self, *, max_recent: int = 40) -> None:
        self._sessions: dict[str, TransferSession] = {}
        self._recent: deque[TransferSession] = deque(maxlen=max_recent)
        self._lock = asyncio.Lock()

    async def register(self, session: TransferSession) -> None:
        async with self._lock:
            now = time.time()
            if session.started_at <= 0:
                session.started_at = now
            session.updated_at = now
            self._sessions[session.session_id] = session

    async def update(
        self,
        session_id: str,
        *,
        status: SessionStatus | None = None,
        transferred_bytes: int | None = None,
        error: str | None = None,
    ) -> TransferSession | None:
        async with self._lock:
            session = self._sessions.get(session_id)
            if not session:
                return None
            if status is not None:
                session.status = status
            if transferred_bytes is not None:
                session.transferred_bytes = max(0, min(transferred_bytes, session.total_bytes or transferred_bytes))
            if error is not None:
                session.error = error
            session.updated_at = time.time()
            return session

    async def finish(self, session_id: str, *, status: SessionStatus, error: str | None = None) -> TransferSession | None:
        async with self._lock:
            session = self._sessions.pop(session_id, None)
            if not session:
                return None
            session.status = status
            session.error = error
            if status == "completed":
                session.transferred_bytes = max(session.transferred_bytes, session.total_bytes)
            session.updated_at = time.time()
            self._recent.appendleft(session)
            return session

    async def get(self, session_id: str) -> TransferSession | None:
        async with self._lock:
            if session_id in self._sessions:
                return self._sessions[session_id]
            for session in self._recent:
                if session.session_id == session_id:
                    return session
            return None

    async def list_active(self) -> list[TransferSession]:
        async with self._lock:
            return list(self._sessions.values())

    async def list_recent(self) -> list[TransferSession]:
        async with self._lock:
            return list(self._recent)

    async def list_all(self) -> list[TransferSession]:
        async with self._lock:
            return sorted(
                [*self._sessions.values(), *self._recent],
                key=lambda session: session.updated_at,
                reverse=True,
            )

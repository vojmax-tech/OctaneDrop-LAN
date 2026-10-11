from __future__ import annotations

import asyncio
import contextlib
import ipaddress
import json
import socket
import time
from dataclasses import dataclass
from typing import Any

from .config import Settings


@dataclass(slots=True)
class Peer:
    id: str
    device_name: str
    app_name: str
    host: str
    port: int
    transfer_port: int
    last_seen: float

    def as_dict(self) -> dict[str, Any]:
        return {
            "id": self.id,
            "device_name": self.device_name,
            "app_name": self.app_name,
            "host": self.host,
            "port": self.port,
            "transfer_port": self.transfer_port,
            "last_seen": self.last_seen,
        }


class PeerDiscoveryService:
    def __init__(self, settings: Settings) -> None:
        self.settings = settings
        self._peers: dict[str, Peer] = {}
        self._running = False
        self._task: asyncio.Task[None] | None = None

    async def start(self) -> None:
        if self._running:
            return
        self._running = True
        self._task = asyncio.create_task(self._run())

    async def stop(self) -> None:
        self._running = False
        if self._task:
            self._task.cancel()
            with contextlib.suppress(asyncio.CancelledError):
                await self._task
            self._task = None

    async def _run(self) -> None:
        broadcaster = asyncio.create_task(self._broadcast_loop())
        listener = asyncio.create_task(self._listen_loop())
        try:
            await asyncio.gather(broadcaster, listener)
        except asyncio.CancelledError:
            pass
        finally:
            broadcaster.cancel()
            listener.cancel()
            with contextlib.suppress(asyncio.CancelledError):
                await broadcaster
            with contextlib.suppress(asyncio.CancelledError):
                await listener

    async def _broadcast_loop(self) -> None:
        while self._running:
            payload = {
                "app_name": self.settings.app_name,
                "device_name": self.settings.device_name,
                "host": self.settings.primary_lan_ip or self.settings.host,
                "port": self.settings.port,
                "transfer_port": self.settings.p2p_transfer_port,
                "ts": int(time.time()),
            }
            data = json.dumps(payload, separators=(",", ":"), ensure_ascii=True).encode("utf-8")
            try:
                with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
                    sock.setsockopt(socket.SOL_SOCKET, socket.SO_BROADCAST, 1)
                    sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
                    sock.sendto(data, ("255.255.255.255", self.settings.discovery_port))
            except OSError:
                pass
            await asyncio.sleep(self.settings.discovery_interval_seconds)

    async def _listen_loop(self) -> None:
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
            sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            try:
                sock.bind(("", self.settings.discovery_port))
            except OSError:
                return
            sock.settimeout(1.0)

            while self._running:
                try:
                    data, addr = await asyncio.to_thread(sock.recvfrom, 4096)
                except socket.timeout:
                    self._expire()
                    continue
                except OSError:
                    continue

                try:
                    payload = json.loads(data.decode("utf-8"))
                except (UnicodeDecodeError, json.JSONDecodeError):
                    continue

                sender_host = str(addr[0] if isinstance(addr, tuple) and addr else "")
                advertised_host = str(payload.get("host") or "")
                host = _resolve_peer_host(advertised_host, sender_host)
                try:
                    port = int(payload.get("port", 0))
                    transfer_port = int(payload.get("transfer_port", port + 1))
                except (TypeError, ValueError):
                    continue
                if not host or not port or not transfer_port:
                    continue

                local_identity = f"{self.settings.primary_lan_ip or self.settings.host}:{self.settings.port}"
                peer_id = f"{host}:{port}"
                if peer_id == local_identity:
                    continue

                self._peers[peer_id] = Peer(
                    id=peer_id,
                    device_name=str(payload.get("device_name") or "Unknown Device"),
                    app_name=str(payload.get("app_name") or "VoltDrop"),
                    host=str(host),
                    port=port,
                    transfer_port=transfer_port,
                    last_seen=time.time(),
                )
                self._expire()

    def _expire(self) -> None:
        threshold = time.time() - self.settings.discovery_ttl_seconds
        stale = [peer_id for peer_id, peer in self._peers.items() if peer.last_seen < threshold]
        for peer_id in stale:
            self._peers.pop(peer_id, None)

    def get_peers(self) -> list[dict[str, Any]]:
        self._expire()
        return [peer.as_dict() for peer in sorted(self._peers.values(), key=lambda item: item.last_seen, reverse=True)]

    def handle_network_change(self) -> None:
        # Drop stale peers quickly when interfaces/subnets rotate.
        self._peers.clear()


def _resolve_peer_host(advertised_host: str, sender_host: str) -> str:
    advertised = advertised_host.strip()
    if _host_is_connectable(advertised):
        return advertised
    return sender_host.strip()


def _host_is_connectable(host: str) -> bool:
    if not host:
        return False
    lowered = host.strip().lower()
    if lowered in {"localhost", "0.0.0.0", "::", "::1", "127.0.0.1"}:
        return False
    try:
        parsed = ipaddress.ip_address(host)
    except ValueError:
        return True
    return not (parsed.is_unspecified or parsed.is_loopback)

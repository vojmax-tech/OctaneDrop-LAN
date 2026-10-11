from __future__ import annotations

from app.discovery import _host_is_connectable, _resolve_peer_host


def test_resolve_peer_host_prefers_sender_when_advertised_is_unspecified() -> None:
    resolved = _resolve_peer_host("0.0.0.0", "192.168.1.42")
    assert resolved == "192.168.1.42"


def test_resolve_peer_host_keeps_valid_advertised_host() -> None:
    resolved = _resolve_peer_host("192.168.1.99", "192.168.1.42")
    assert resolved == "192.168.1.99"


def test_host_is_connectable_filters_local_only_values() -> None:
    assert not _host_is_connectable("127.0.0.1")
    assert not _host_is_connectable("0.0.0.0")
    assert not _host_is_connectable("localhost")
    assert _host_is_connectable("192.168.1.10")

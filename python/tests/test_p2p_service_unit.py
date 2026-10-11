from __future__ import annotations

from app.p2p_service import _iter_bounded_chunks


def test_iter_bounded_chunks_splits_payload() -> None:
    payload = b"abcdefghij"
    chunks = _iter_bounded_chunks(payload, max_size=4)
    assert chunks == [b"abcd", b"efgh", b"ij"]


def test_iter_bounded_chunks_empty_payload() -> None:
    assert _iter_bounded_chunks(b"", max_size=8) == []

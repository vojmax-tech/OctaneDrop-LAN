from __future__ import annotations

import hashlib
from pathlib import Path
from typing import Iterator

DEFAULT_TRANSFER_CHUNK_BYTES = 1024 * 1024


def iter_file_chunks(path: Path, *, chunk_size: int = DEFAULT_TRANSFER_CHUNK_BYTES) -> Iterator[bytes]:
    if chunk_size <= 0:
        raise ValueError("chunk_size must be positive")
    with path.open("rb") as source:
        while True:
            chunk = source.read(chunk_size)
            if not chunk:
                break
            yield chunk


def sha256_file(path: Path, *, chunk_size: int = DEFAULT_TRANSFER_CHUNK_BYTES) -> str:
    digest = hashlib.sha256()
    for chunk in iter_file_chunks(path, chunk_size=chunk_size):
        digest.update(chunk)
    return digest.hexdigest()

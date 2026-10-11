from __future__ import annotations

import io
import secrets
from pathlib import Path

from app.config import human_bytes
from app.db import StoredFile
from app.main import compute_files_signature, render_content_disposition, unique_archive_member_name
from app.security import FileCipher, hash_password, verify_password


def test_hash_password_roundtrip() -> None:
    password = "correct-horse-battery-staple"
    hashed = hash_password(password)

    assert hashed.startswith("scrypt$")
    assert verify_password(password, hashed)
    assert not verify_password("wrong-password", hashed)


def test_compute_files_signature_changes_when_file_changes() -> None:
    base = [
        StoredFile(
            id="a1",
            original_name="first.txt",
            stored_name="a1.bin",
            content_type="text/plain",
            size_bytes=5,
            sha256_hex="0" * 64,
            uploaded_at="2026-03-30T00:00:00Z",
            uploaded_by="admin",
        ),
        StoredFile(
            id="b2",
            original_name="second.txt",
            stored_name="b2.bin",
            content_type="text/plain",
            size_bytes=6,
            sha256_hex="1" * 64,
            uploaded_at="2026-03-30T00:05:00Z",
            uploaded_by="admin",
        ),
    ]
    changed = [
        StoredFile(
            id="a1",
            original_name="first.txt",
            stored_name="a1.bin",
            content_type="text/plain",
            size_bytes=5,
            sha256_hex="f" * 64,
            uploaded_at="2026-03-30T00:00:00Z",
            uploaded_by="admin",
        ),
        base[1],
    ]

    assert compute_files_signature(base) != compute_files_signature(changed)


def test_content_disposition_uses_ascii_fallback_and_utf8_name() -> None:
    header = render_content_disposition('weird "na\\me".txt')

    assert "attachment;" in header
    assert 'filename="' in header
    assert "filename*=UTF-8''" in header


def test_file_cipher_stream_roundtrip() -> None:
    data = (b"VoltDrop-" * 50_000) + b"end"
    cipher = FileCipher(b"1" * 32)
    temp_root = Path(".test_tmp")
    temp_root.mkdir(parents=True, exist_ok=True)
    encrypted_path = temp_root / f"blob-{secrets.token_hex(8)}.bin"

    try:
        size, digest = cipher.encrypt_fileobj_to_path(
            source=io.BytesIO(data),
            destination=encrypted_path,
            max_plaintext_bytes=len(data) + 1,
            chunk_size=64 * 1024,
        )
        decrypted = b"".join(cipher.iter_decrypt_path(encrypted_path, expected_plaintext_size=size))

        assert size == len(data)
        assert len(digest) == 64
        assert decrypted == data
        assert human_bytes(size).endswith(("KB", "MB"))
    finally:
        encrypted_path.unlink(missing_ok=True)


def test_unique_archive_member_name_deduplicates_names() -> None:
    used: set[str] = set()
    first = unique_archive_member_name("report.pdf", used)
    second = unique_archive_member_name("report.pdf", used)
    third = unique_archive_member_name("report.pdf", used)

    assert first == "report.pdf"
    assert second == "report (2).pdf"
    assert third == "report (3).pdf"


def test_unique_archive_member_name_uses_safe_basename() -> None:
    used: set[str] = set()
    value = unique_archive_member_name("..\\folder\\sample.txt", used)

    assert value == "sample.txt"

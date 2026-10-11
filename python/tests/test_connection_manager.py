from __future__ import annotations

import asyncio

from app.connection import ConnectionManager, TransferSession


def test_connection_manager_tracks_lifecycle() -> None:
    async def _run() -> None:
        manager = ConnectionManager()
        session = TransferSession(
            session_id="s1",
            direction="send",
            peer_host="192.168.1.55",
            peer_port=8766,
            file_name="demo.bin",
            total_bytes=100,
        )
        await manager.register(session)
        await manager.update("s1", status="sending", transferred_bytes=37)
        active = await manager.list_active()
        assert len(active) == 1
        assert active[0].status == "sending"
        assert active[0].transferred_bytes == 37

        await manager.finish("s1", status="completed")
        active_after = await manager.list_active()
        recent = await manager.list_recent()
        assert len(active_after) == 0
        assert len(recent) == 1
        assert recent[0].status == "completed"
        assert recent[0].transferred_bytes == 100
        assert recent[0].as_dict()["progress_percent"] == 100

    asyncio.run(_run())

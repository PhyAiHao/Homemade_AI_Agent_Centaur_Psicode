from __future__ import annotations

import asyncio
import json
from typing import Any

try:
    import msgpack  # type: ignore[import-not-found]
except Exception as _msgpack_err:  # catch ALL errors, not just ImportError
    import sys as _sys
    print(
        f"WARNING: Failed to import msgpack: {type(_msgpack_err).__name__}: {_msgpack_err}\n"
        f"  Python: {_sys.executable}\n"
        f"  Path: {_sys.path}",
        file=_sys.stderr,
    )
    msgpack = None  # type: ignore[assignment]


FRAME_LENGTH_BYTES = 4
BYTE_ORDER = "big"


def encode_payload(payload: dict[str, Any]) -> bytes:
    if msgpack is not None:
        return msgpack.packb(payload, use_bin_type=True)
    return json.dumps(payload, separators=(",", ":"), sort_keys=True).encode("utf-8")


def decode_payload(payload: bytes) -> dict[str, Any]:
    # Auto-detect format: msgpack payloads start with a byte >= 0x80
    # (fixmap, map16, map32 etc.), valid JSON/UTF-8 never starts with those.
    is_binary = bool(payload and payload[0] >= 0x80)

    if is_binary:
        if msgpack is None:
            raise RuntimeError(
                "Received msgpack-encoded IPC frame but 'msgpack' is not installed. "
                "Run:  pip install msgpack"
            )
        data = msgpack.unpackb(payload, raw=False)
    elif msgpack is not None:
        # Could be either format — try msgpack first, fall back to JSON
        try:
            data = msgpack.unpackb(payload, raw=False)
        except Exception:
            data = json.loads(payload.decode("utf-8"))
    else:
        data = json.loads(payload.decode("utf-8"))

    if not isinstance(data, dict):
        raise ValueError("IPC payload must decode to an object")
    return data


def frame_payload(payload: dict[str, Any]) -> bytes:
    encoded = encode_payload(payload)
    length = len(encoded).to_bytes(FRAME_LENGTH_BYTES, BYTE_ORDER)
    return length + encoded


async def read_frame(reader: asyncio.StreamReader) -> dict[str, Any] | None:
    try:
        header = await reader.readexactly(FRAME_LENGTH_BYTES)
    except asyncio.IncompleteReadError as error:
        if not error.partial:
            return None
        raise
    length = int.from_bytes(header, BYTE_ORDER)
    body = await reader.readexactly(length)
    return decode_payload(body)


async def write_frame(
    writer: asyncio.StreamWriter, payload: dict[str, Any]
) -> None:
    writer.write(frame_payload(payload))
    await writer.drain()

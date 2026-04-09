from __future__ import annotations

import asyncio
from collections.abc import Sequence
from pathlib import Path
from typing import Any

try:
    import httpx
except ImportError:  # pragma: no cover - dependency is declared in pyproject
    httpx = None  # type: ignore[assignment]

from ..types.base import AgentBaseModel

FILES_API_BETA_HEADER = "files-api-2025-04-14,oauth-2025-04-20"
ANTHROPIC_VERSION = "2023-06-01"
DEFAULT_CONCURRENCY = 5
MAX_FILE_SIZE_BYTES = 500 * 1024 * 1024


class FileSpec(AgentBaseModel):
    file_id: str
    relative_path: str


class FilesApiConfig(AgentBaseModel):
    oauth_token: str
    session_id: str
    base_url: str = "https://api.anthropic.com"
    timeout_seconds: float = 60.0


class DownloadResult(AgentBaseModel):
    file_id: str
    path: str
    success: bool
    error: str | None = None
    bytes_written: int | None = None


class UploadResult(AgentBaseModel):
    path: str
    success: bool
    file_id: str | None = None
    size: int | None = None
    error: str | None = None


class FileMetadata(AgentBaseModel):
    filename: str
    file_id: str
    size: int


def build_download_path(base_path: str | Path, session_id: str, relative_path: str) -> str | None:
    base = Path(base_path).resolve()
    normalized = Path(relative_path)
    if normalized.is_absolute() or ".." in normalized.parts:
        return None
    return str(base / session_id / "uploads" / normalized)


def parse_file_specs(file_specs: Sequence[str]) -> list[FileSpec]:
    parsed: list[FileSpec] = []
    for raw_spec in file_specs:
        for spec in raw_spec.split():
            if ":" not in spec:
                continue
            file_id, relative_path = spec.split(":", 1)
            if not file_id or not relative_path:
                continue
            parsed.append(FileSpec(file_id=file_id, relative_path=relative_path))
    return parsed


async def download_file(
    file_id: str,
    config: FilesApiConfig,
    *,
    client: Any | None = None,
) -> bytes:
    _ensure_httpx()
    close_client = client is None
    http_client = client or httpx.AsyncClient(timeout=config.timeout_seconds)
    try:
        response = await http_client.get(
            f"{config.base_url}/v1/files/{file_id}/content",
            headers=_headers(config),
        )
        response.raise_for_status()
        return bytes(response.content)
    finally:
        if close_client:
            await http_client.aclose()


async def download_and_save_file(
    attachment: FileSpec,
    config: FilesApiConfig,
    *,
    base_path: str | Path,
    client: Any | None = None,
) -> DownloadResult:
    destination = build_download_path(base_path, config.session_id, attachment.relative_path)
    if destination is None:
        return DownloadResult(
            file_id=attachment.file_id,
            path="",
            success=False,
            error=f"Invalid file path: {attachment.relative_path}",
        )

    try:
        payload = await download_file(attachment.file_id, config, client=client)
        output_path = Path(destination)
        output_path.parent.mkdir(parents=True, exist_ok=True)
        output_path.write_bytes(payload)
        return DownloadResult(
            file_id=attachment.file_id,
            path=str(output_path),
            success=True,
            bytes_written=len(payload),
        )
    except Exception as error:  # pragma: no cover - network path
        return DownloadResult(
            file_id=attachment.file_id,
            path=destination,
            success=False,
            error=str(error),
        )


async def download_session_files(
    files: Sequence[FileSpec],
    config: FilesApiConfig,
    *,
    base_path: str | Path,
    concurrency: int = DEFAULT_CONCURRENCY,
    client: Any | None = None,
) -> list[DownloadResult]:
    semaphore = asyncio.Semaphore(max(1, concurrency))

    async def worker(file_spec: FileSpec) -> DownloadResult:
        async with semaphore:
            return await download_and_save_file(
                file_spec,
                config,
                base_path=base_path,
                client=client,
            )

    return list(await asyncio.gather(*(worker(file_spec) for file_spec in files)))


async def upload_file(
    file_path: str | Path,
    relative_path: str,
    config: FilesApiConfig,
    *,
    client: Any | None = None,
) -> UploadResult:
    _ensure_httpx()
    payload = Path(file_path).read_bytes()
    if len(payload) > MAX_FILE_SIZE_BYTES:
        return UploadResult(
            path=relative_path,
            success=False,
            error=f"File exceeds maximum size of {MAX_FILE_SIZE_BYTES} bytes",
        )

    files = {"file": (Path(relative_path).name, payload, "application/octet-stream")}
    data = {"purpose": "user_data"}

    close_client = client is None
    http_client = client or httpx.AsyncClient(timeout=max(config.timeout_seconds, 120.0))
    try:
        response = await http_client.post(
            f"{config.base_url}/v1/files",
            headers=_headers(config),
            files=files,
            data=data,
        )
        response.raise_for_status()
        body = response.json()
        return UploadResult(
            path=relative_path,
            success=True,
            file_id=body.get("id"),
            size=len(payload),
        )
    except Exception as error:  # pragma: no cover - network path
        return UploadResult(path=relative_path, success=False, error=str(error))
    finally:
        if close_client:
            await http_client.aclose()


async def list_files_created_after(
    after_created_at: str,
    config: FilesApiConfig,
    *,
    client: Any | None = None,
) -> list[FileMetadata]:
    _ensure_httpx()
    close_client = client is None
    http_client = client or httpx.AsyncClient(timeout=config.timeout_seconds)
    files: list[FileMetadata] = []
    after_id: str | None = None
    try:
        while True:
            params = {"after_created_at": after_created_at}
            if after_id:
                params["after_id"] = after_id
            response = await http_client.get(
                f"{config.base_url}/v1/files",
                headers=_headers(config),
                params=params,
            )
            response.raise_for_status()
            payload = response.json()
            items = payload.get("data", [])
            for item in items:
                files.append(
                    FileMetadata(
                        filename=item.get("filename", ""),
                        file_id=item.get("id", ""),
                        size=int(item.get("size_bytes", 0) or 0),
                    )
                )
            if not payload.get("has_more"):
                break
            last = items[-1] if items else None
            after_id = last.get("id") if isinstance(last, dict) else None
            if not after_id:
                break
        return files
    finally:
        if close_client:
            await http_client.aclose()


def _headers(config: FilesApiConfig) -> dict[str, str]:
    return {
        "Authorization": f"Bearer {config.oauth_token}",
        "anthropic-version": ANTHROPIC_VERSION,
        "anthropic-beta": FILES_API_BETA_HEADER,
    }


def _ensure_httpx() -> None:
    if httpx is None:  # pragma: no cover - only triggered without dependency installed
        raise RuntimeError("httpx is required to use the files API helpers")

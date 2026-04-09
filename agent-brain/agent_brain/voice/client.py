from __future__ import annotations

import base64
from dataclasses import dataclass
from pathlib import Path

from ..ipc_types import VoiceStart, VoiceTranscript
from .keyterms import get_voice_keyterms
from .stt_stream import StreamingSTTClient, TranscriptResult, create_synthetic_voice_wav

try:
    import sounddevice  # type: ignore
except ImportError:  # pragma: no cover - dependency declared in pyproject
    sounddevice = None  # type: ignore[assignment]


@dataclass
class CapturedAudio:
    audio_bytes: bytes
    audio_b64: str
    duration_ms: int
    sample_rate: int


class VoiceModeClient:
    def __init__(self, *, stt_client: StreamingSTTClient | None = None) -> None:
        self.stt_client = stt_client or StreamingSTTClient()

    def check_recording_availability(self) -> dict[str, object]:
        if sounddevice is not None:
            return {"available": True, "mode": "microphone"}
        return {
            "available": True,
            "mode": "synthetic",
            "reason": "sounddevice is not installed; synthetic capture is used in offline environments.",
        }

    def capture_text_audio(
        self,
        transcript: str,
        *,
        sample_rate: int = 16_000,
        duration_ms: int = 350,
    ) -> CapturedAudio:
        audio_bytes = create_synthetic_voice_wav(
            transcript,
            sample_rate=sample_rate,
            duration_ms=duration_ms,
        )
        return CapturedAudio(
            audio_bytes=audio_bytes,
            audio_b64=base64.b64encode(audio_bytes).decode("ascii"),
            duration_ms=duration_ms,
            sample_rate=sample_rate,
        )

    def start_session(
        self,
        *,
        language: str = "en",
        keyterms: list[str] | None = None,
        transcript_hint: str | None = None,
    ):
        return self.stt_client.create_session(
            language=language,
            keyterms=keyterms,
            transcript_hint=transcript_hint,
        )

    def transcribe_audio_bytes(
        self,
        audio_bytes: bytes,
        *,
        language: str = "en",
        keyterms: list[str] | None = None,
        transcript_hint: str | None = None,
    ) -> TranscriptResult:
        return self.stt_client.transcribe_audio(
            audio_bytes,
            language=language,
            keyterms=keyterms,
            transcript_hint=transcript_hint,
        )

    def transcribe_file(
        self,
        path: str | Path,
        *,
        language: str = "en",
        keyterms: list[str] | None = None,
        transcript_hint: str | None = None,
    ) -> TranscriptResult:
        return self.transcribe_audio_bytes(
            Path(path).expanduser().read_bytes(),
            language=language,
            keyterms=keyterms,
            transcript_hint=transcript_hint,
        )


class VoiceService:
    def __init__(self, *, client: VoiceModeClient | None = None) -> None:
        self.client = client or VoiceModeClient()

    async def handle(self, request: VoiceStart) -> VoiceTranscript:
        keyterms = request.keyterms or get_voice_keyterms(
            project_dir=request.project_dir,
            branch_name=request.branch_name,
            recent_files=request.recent_files,
        )
        language = request.language or "en"
        audio_bytes = self._resolve_audio_bytes(request)
        result = self.client.transcribe_audio_bytes(
            audio_bytes,
            language=language,
            keyterms=keyterms,
            transcript_hint=request.transcript_hint,
        )
        return VoiceTranscript(
            request_id=request.request_id,
            text=result.text,
            is_final=result.is_final,
            metadata={
                "source": result.source,
                "language": result.language,
                "duration_ms": result.duration_ms,
                "keyterms": result.keyterms,
                **result.metadata,
            },
        )

    def _resolve_audio_bytes(self, request: VoiceStart) -> bytes:
        if request.audio_b64:
            return base64.b64decode(request.audio_b64)
        if request.audio_path:
            return Path(request.audio_path).expanduser().read_bytes()
        return b""

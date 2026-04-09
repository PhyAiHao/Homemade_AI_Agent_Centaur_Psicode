from __future__ import annotations

try:
    import audioop  # Deprecated in 3.11, removed in 3.13
except ImportError:
    audioop = None  # type: ignore[assignment]
import io
import math
import struct
import wave
from typing import Any

from .._compat import Field
from ..types.base import AgentBaseModel


TRANSCRIPT_CHUNK_ID = b"ctxt"


class TranscriptResult(AgentBaseModel):
    text: str
    is_final: bool = True
    source: str
    language: str = "en"
    duration_ms: int = 0
    keyterms: list[str] = Field(default_factory=list)
    metadata: dict[str, Any] = Field(default_factory=dict)


class StreamingSTTSession:
    def __init__(
        self,
        client: "StreamingSTTClient",
        *,
        language: str = "en",
        keyterms: list[str] | None = None,
        transcript_hint: str | None = None,
    ) -> None:
        self.client = client
        self.language = language
        self.keyterms = list(keyterms or [])
        self.transcript_hint = transcript_hint
        self._chunks: list[bytes] = []

    def send_audio(self, audio_chunk: bytes) -> None:
        self._chunks.append(audio_chunk)

    def finalize(self) -> TranscriptResult:
        return self.client.transcribe_audio(
            b"".join(self._chunks),
            language=self.language,
            keyterms=self.keyterms,
            transcript_hint=self.transcript_hint,
        )


class StreamingSTTClient:
    def __init__(self, *, silence_threshold: int = 200) -> None:
        self.silence_threshold = silence_threshold

    def create_session(
        self,
        *,
        language: str = "en",
        keyterms: list[str] | None = None,
        transcript_hint: str | None = None,
    ) -> StreamingSTTSession:
        return StreamingSTTSession(
            self,
            language=language,
            keyterms=keyterms,
            transcript_hint=transcript_hint,
        )

    def transcribe_audio(
        self,
        audio_bytes: bytes,
        *,
        language: str = "en",
        keyterms: list[str] | None = None,
        transcript_hint: str | None = None,
    ) -> TranscriptResult:
        terms = list(keyterms or [])
        if transcript_hint:
            return TranscriptResult(
                text=transcript_hint.strip(),
                source="hint",
                language=language,
                keyterms=terms,
            )

        embedded = extract_embedded_transcript(audio_bytes)
        if embedded:
            return TranscriptResult(
                text=embedded,
                source="embedded_hint",
                language=language,
                duration_ms=estimate_audio_duration_ms(audio_bytes),
                keyterms=terms,
            )

        ascii_transcript = extract_ascii_transcript(audio_bytes)
        if ascii_transcript:
            return TranscriptResult(
                text=ascii_transcript,
                source="ascii_payload",
                language=language,
                keyterms=terms,
            )

        duration_ms = estimate_audio_duration_ms(audio_bytes)
        rms = estimate_audio_rms(audio_bytes)
        if duration_ms == 0 or rms <= self.silence_threshold:
            return TranscriptResult(
                text="",
                source="silence",
                language=language,
                duration_ms=duration_ms,
                keyterms=terms,
                metadata={"rms": rms},
            )

        fallback = "Detected speech audio. Offline transcript unavailable."
        if terms:
            fallback = (
                "Detected speech audio. Relevant voice keyterms: "
                + ", ".join(terms[:5])
            )
        return TranscriptResult(
            text=fallback,
            source="audio_detected",
            language=language,
            duration_ms=duration_ms,
            keyterms=terms,
            metadata={"rms": rms},
        )


def create_synthetic_voice_wav(
    transcript: str,
    *,
    sample_rate: int = 16_000,
    duration_ms: int = 350,
) -> bytes:
    frame_count = max(1, int(sample_rate * (duration_ms / 1000.0)))
    pcm_frames = bytearray()
    frequency = 440.0
    amplitude = 6_000
    for index in range(frame_count):
        sample = int(amplitude * math.sin(2 * math.pi * frequency * (index / sample_rate)))
        pcm_frames.extend(struct.pack("<h", sample))

    buffer = io.BytesIO()
    with wave.open(buffer, "wb") as wav_file:
        wav_file.setnchannels(1)
        wav_file.setsampwidth(2)
        wav_file.setframerate(sample_rate)
        wav_file.writeframes(bytes(pcm_frames))

    base_wav = buffer.getvalue()
    payload = transcript.encode("utf-8")
    chunk = TRANSCRIPT_CHUNK_ID + struct.pack("<I", len(payload)) + payload
    if len(payload) % 2 == 1:
        chunk += b"\x00"

    combined = base_wav + chunk
    return combined[:4] + struct.pack("<I", len(combined) - 8) + combined[8:]


def extract_embedded_transcript(audio_bytes: bytes) -> str | None:
    if len(audio_bytes) < 12 or audio_bytes[:4] != b"RIFF" or audio_bytes[8:12] != b"WAVE":
        return None

    offset = 12
    while offset + 8 <= len(audio_bytes):
        chunk_id = audio_bytes[offset : offset + 4]
        chunk_size = struct.unpack("<I", audio_bytes[offset + 4 : offset + 8])[0]
        data_start = offset + 8
        data_end = data_start + chunk_size
        if data_end > len(audio_bytes):
            break
        if chunk_id == TRANSCRIPT_CHUNK_ID:
            try:
                return audio_bytes[data_start:data_end].decode("utf-8").strip()
            except UnicodeDecodeError:
                return None
        offset = data_end + (chunk_size % 2)
    return None


def extract_ascii_transcript(audio_bytes: bytes) -> str | None:
    try:
        decoded = audio_bytes.decode("utf-8").strip()
    except UnicodeDecodeError:
        return None
    prefix = "TRANSCRIPT:"
    if decoded.startswith(prefix):
        return decoded[len(prefix) :].strip()
    return None


def estimate_audio_duration_ms(audio_bytes: bytes) -> int:
    try:
        with wave.open(io.BytesIO(audio_bytes), "rb") as wav_file:
            frame_rate = wav_file.getframerate()
            frame_count = wav_file.getnframes()
            if frame_rate <= 0:
                return 0
            return int((frame_count / frame_rate) * 1000)
    except wave.Error:
        return 0


def estimate_audio_rms(audio_bytes: bytes) -> int:
    try:
        with wave.open(io.BytesIO(audio_bytes), "rb") as wav_file:
            sample_width = wav_file.getsampwidth()
            frames = wav_file.readframes(wav_file.getnframes())
            if not frames:
                return 0
            return int(audioop.rms(frames, sample_width))
    except wave.Error:
        return 0

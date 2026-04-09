"""
Media generation tools — Text-to-Speech, Text-to-Image, Text-to-Video.

These are called by the Rust agent's tool system via IPC, just like
any other tool. The LLM decides WHEN to use them based on user intent;
these functions handle the actual API calls to specialized AI services.

Supported services:
  TTS:   ElevenLabs, OpenAI TTS, Edge TTS (free)
  Image: DALL-E, Stability AI
  Video: Runway, Kling
  Music: Suno

Each function returns a file path to the generated media.
"""

from __future__ import annotations

import os
import json
import asyncio
import tempfile
from pathlib import Path
from typing import Any


# ── Text-to-Speech ─────────────────────────────────────────────────────────


async def text_to_speech(
    text: str,
    output_path: str | None = None,
    voice: str = "alloy",
    provider: str = "auto",
) -> dict[str, Any]:
    """Convert text to speech audio file.

    Args:
        text: Text to convert to speech
        output_path: Where to save the audio file (default: temp file)
        voice: Voice ID (provider-specific)
        provider: "elevenlabs", "openai", "edge" (free), or "auto"

    Returns:
        {"path": "/path/to/audio.mp3", "duration_sec": 12.3, "provider": "openai"}
    """
    if not output_path:
        output_path = os.path.join(tempfile.gettempdir(), "agent_tts_output.mp3")

    # Auto-detect: prefer ElevenLabs > OpenAI > Edge TTS (free fallback)
    if provider == "auto":
        if os.environ.get("ELEVENLABS_API_KEY"):
            provider = "elevenlabs"
        elif os.environ.get("OPENAI_API_KEY"):
            provider = "openai"
        else:
            provider = "edge"

    if provider == "elevenlabs":
        return await _tts_elevenlabs(text, output_path, voice)
    elif provider == "openai":
        return await _tts_openai(text, output_path, voice)
    elif provider == "edge":
        return await _tts_edge(text, output_path, voice)
    else:
        raise ValueError(f"Unknown TTS provider: {provider}")


async def _tts_elevenlabs(text: str, output_path: str, voice: str) -> dict[str, Any]:
    """ElevenLabs TTS — highest quality, 10K chars/month free."""
    try:
        from elevenlabs import ElevenLabs
    except ImportError:
        raise RuntimeError("elevenlabs package not installed. Run: pip install elevenlabs")

    api_key = os.environ.get("ELEVENLABS_API_KEY")
    if not api_key:
        raise RuntimeError("ELEVENLABS_API_KEY not set")

    client = ElevenLabs(api_key=api_key)

    # Default voice mapping
    voice_id = voice if len(voice) > 10 else {
        "alloy": "21m00Tcm4TlvDq8ikWAM",  # Rachel
        "echo": "29vD33N1CtxCmqQRPOHJ",    # Drew
        "nova": "EXAVITQu4vr4xnSDxMaL",    # Bella
    }.get(voice, "21m00Tcm4TlvDq8ikWAM")

    audio = client.text_to_speech.convert(
        text=text,
        voice_id=voice_id,
        model_id="eleven_multilingual_v2",
        output_format="mp3_44100_128",
    )

    with open(output_path, "wb") as f:
        for chunk in audio:
            f.write(chunk)

    return {"path": output_path, "provider": "elevenlabs", "chars": len(text)}


async def _tts_openai(text: str, output_path: str, voice: str) -> dict[str, Any]:
    """OpenAI TTS — good quality, pay-per-use."""
    try:
        from openai import OpenAI
    except ImportError:
        raise RuntimeError("openai package not installed. Run: pip install openai")

    api_key = os.environ.get("OPENAI_API_KEY")
    if not api_key:
        raise RuntimeError("OPENAI_API_KEY not set")

    client = OpenAI(api_key=api_key)

    # Voices: alloy, echo, fable, onyx, nova, shimmer
    response = client.audio.speech.create(
        model="tts-1",
        voice=voice if voice in ("alloy", "echo", "fable", "onyx", "nova", "shimmer") else "alloy",
        input=text,
    )

    response.stream_to_file(output_path)
    return {"path": output_path, "provider": "openai", "chars": len(text)}


async def _tts_edge(text: str, output_path: str, voice: str) -> dict[str, Any]:
    """Microsoft Edge TTS — free, unlimited, decent quality."""
    try:
        import edge_tts
    except ImportError:
        raise RuntimeError("edge-tts package not installed. Run: pip install edge-tts")

    # Default voice mapping
    edge_voice = {
        "alloy": "en-US-AriaNeural",
        "echo": "en-US-GuyNeural",
        "nova": "en-US-JennyNeural",
    }.get(voice, "en-US-AriaNeural")

    communicate = edge_tts.Communicate(text, edge_voice)
    await communicate.save(output_path)
    return {"path": output_path, "provider": "edge", "chars": len(text)}


# ── Text-to-Image ──────────────────────────────────────────────────────────


async def text_to_image(
    prompt: str,
    output_path: str | None = None,
    size: str = "1024x1024",
    provider: str = "auto",
) -> dict[str, Any]:
    """Generate an image from a text prompt.

    Args:
        prompt: Image description
        output_path: Where to save (default: temp file)
        size: Image size (e.g., "1024x1024")
        provider: "openai" (DALL-E), "stability", or "auto"
    """
    if not output_path:
        output_path = os.path.join(tempfile.gettempdir(), "agent_image_output.png")

    if provider == "auto":
        if os.environ.get("OPENAI_API_KEY"):
            provider = "openai"
        elif os.environ.get("STABILITY_API_KEY"):
            provider = "stability"
        else:
            raise RuntimeError("No image API key found. Set OPENAI_API_KEY or STABILITY_API_KEY")

    if provider == "openai":
        return await _image_openai(prompt, output_path, size)
    elif provider == "stability":
        return await _image_stability(prompt, output_path)
    else:
        raise ValueError(f"Unknown image provider: {provider}")


async def _image_openai(prompt: str, output_path: str, size: str) -> dict[str, Any]:
    """OpenAI DALL-E image generation."""
    import base64
    try:
        from openai import OpenAI
    except ImportError:
        raise RuntimeError("openai package not installed")

    client = OpenAI(api_key=os.environ["OPENAI_API_KEY"])
    response = client.images.generate(
        model="dall-e-3",
        prompt=prompt,
        size=size,
        quality="standard",
        response_format="b64_json",
        n=1,
    )

    image_data = base64.b64decode(response.data[0].b64_json)
    with open(output_path, "wb") as f:
        f.write(image_data)

    return {"path": output_path, "provider": "openai/dall-e-3", "revised_prompt": response.data[0].revised_prompt}


async def _image_stability(prompt: str, output_path: str) -> dict[str, Any]:
    """Stability AI image generation."""
    import httpx

    api_key = os.environ.get("STABILITY_API_KEY")
    if not api_key:
        raise RuntimeError("STABILITY_API_KEY not set")

    async with httpx.AsyncClient() as client:
        response = await client.post(
            "https://api.stability.ai/v2beta/stable-image/generate/sd3",
            headers={"Authorization": f"Bearer {api_key}", "Accept": "image/*"},
            files={"none": ""},
            data={"prompt": prompt, "output_format": "png"},
            timeout=60.0,
        )
        response.raise_for_status()

    with open(output_path, "wb") as f:
        f.write(response.content)

    return {"path": output_path, "provider": "stability/sd3"}


# ── Text-to-Video ──────────────────────────────────────────────────────────


async def text_to_video(
    prompt: str,
    output_path: str | None = None,
    duration: int = 5,
    provider: str = "auto",
) -> dict[str, Any]:
    """Generate a video from a text prompt.

    Args:
        prompt: Video description
        output_path: Where to save (default: temp file)
        duration: Video duration in seconds
        provider: "runway", "kling", or "auto"
    """
    if not output_path:
        output_path = os.path.join(tempfile.gettempdir(), "agent_video_output.mp4")

    if provider == "auto":
        if os.environ.get("RUNWAY_API_KEY"):
            provider = "runway"
        elif os.environ.get("KLING_API_KEY"):
            provider = "kling"
        else:
            raise RuntimeError("No video API key found. Set RUNWAY_API_KEY or KLING_API_KEY")

    if provider == "runway":
        return await _video_runway(prompt, output_path, duration)
    elif provider == "kling":
        return await _video_kling(prompt, output_path, duration)
    else:
        raise ValueError(f"Unknown video provider: {provider}")


async def _video_runway(prompt: str, output_path: str, duration: int) -> dict[str, Any]:
    """Runway Gen-3 video generation."""
    import httpx

    api_key = os.environ.get("RUNWAY_API_KEY")
    if not api_key:
        raise RuntimeError("RUNWAY_API_KEY not set")

    async with httpx.AsyncClient(timeout=120.0) as client:
        # Start generation
        resp = await client.post(
            "https://api.dev.runwayml.com/v1/text_to_video",
            headers={"Authorization": f"Bearer {api_key}", "Content-Type": "application/json"},
            json={"text_prompt": prompt, "seconds": duration, "model": "gen3a_turbo"},
        )
        resp.raise_for_status()
        task_id = resp.json().get("id")

        # Poll for completion
        for _ in range(60):
            await asyncio.sleep(5)
            status_resp = await client.get(
                f"https://api.dev.runwayml.com/v1/tasks/{task_id}",
                headers={"Authorization": f"Bearer {api_key}"},
            )
            status = status_resp.json()
            if status.get("status") == "SUCCEEDED":
                video_url = status["output"][0]
                video_data = await client.get(video_url)
                with open(output_path, "wb") as f:
                    f.write(video_data.content)
                return {"path": output_path, "provider": "runway/gen3a", "duration": duration}
            elif status.get("status") == "FAILED":
                raise RuntimeError(f"Runway generation failed: {status.get('failure')}")

    raise RuntimeError("Runway generation timed out")


async def _video_kling(prompt: str, output_path: str, duration: int) -> dict[str, Any]:
    """Kling AI video generation."""
    import httpx

    api_key = os.environ.get("KLING_API_KEY")
    if not api_key:
        raise RuntimeError("KLING_API_KEY not set")

    async with httpx.AsyncClient(timeout=120.0) as client:
        resp = await client.post(
            "https://api.klingai.com/v1/videos/text2video",
            headers={"Authorization": f"Bearer {api_key}", "Content-Type": "application/json"},
            json={"prompt": prompt, "duration": str(duration), "model": "kling-v1"},
        )
        resp.raise_for_status()
        task_id = resp.json().get("data", {}).get("task_id")

        # Poll for completion
        for _ in range(60):
            await asyncio.sleep(5)
            status_resp = await client.get(
                f"https://api.klingai.com/v1/videos/text2video/{task_id}",
                headers={"Authorization": f"Bearer {api_key}"},
            )
            result = status_resp.json().get("data", {})
            if result.get("task_status") == "succeed":
                video_url = result["task_result"]["videos"][0]["url"]
                video_data = await client.get(video_url)
                with open(output_path, "wb") as f:
                    f.write(video_data.content)
                return {"path": output_path, "provider": "kling", "duration": duration}
            elif result.get("task_status") == "failed":
                raise RuntimeError(f"Kling generation failed: {result.get('task_status_msg')}")

    raise RuntimeError("Kling generation timed out")


# ── Tool Definitions (for agent tool registry) ──────────────────────────────


MEDIA_TOOL_DEFINITIONS = [
    {
        "name": "TextToSpeech",
        "description": "Convert text to speech audio file. Supports ElevenLabs, OpenAI TTS, and Edge TTS (free). Returns the path to the generated audio file.",
        "input_schema": {
            "type": "object",
            "properties": {
                "text": {"type": "string", "description": "Text to convert to speech"},
                "output_path": {"type": "string", "description": "Output file path (optional, defaults to temp)"},
                "voice": {"type": "string", "description": "Voice ID: alloy, echo, nova, shimmer, fable, onyx"},
                "provider": {"type": "string", "enum": ["auto", "elevenlabs", "openai", "edge"], "description": "TTS provider (default: auto)"},
            },
            "required": ["text"],
        },
    },
    {
        "name": "TextToImage",
        "description": "Generate an image from a text description. Supports DALL-E 3 and Stability AI. Returns the path to the generated image.",
        "input_schema": {
            "type": "object",
            "properties": {
                "prompt": {"type": "string", "description": "Image description"},
                "output_path": {"type": "string", "description": "Output file path (optional)"},
                "size": {"type": "string", "description": "Image size: 1024x1024, 1792x1024, 1024x1792"},
                "provider": {"type": "string", "enum": ["auto", "openai", "stability"], "description": "Image provider"},
            },
            "required": ["prompt"],
        },
    },
    {
        "name": "TextToVideo",
        "description": "Generate a video from a text description. Supports Runway Gen-3 and Kling AI. Returns the path to the generated video. Takes 30-120 seconds.",
        "input_schema": {
            "type": "object",
            "properties": {
                "prompt": {"type": "string", "description": "Video description"},
                "output_path": {"type": "string", "description": "Output file path (optional)"},
                "duration": {"type": "integer", "description": "Video duration in seconds (default: 5)"},
                "provider": {"type": "string", "enum": ["auto", "runway", "kling"], "description": "Video provider"},
            },
            "required": ["prompt"],
        },
    },
]


# ── Audio + Image → Video (ffmpeg) ─────────────────────────────────────────


async def audio_image_to_video(
    audio_path: str,
    image_path: str,
    output_path: str | None = None,
) -> dict[str, Any]:
    """Combine a static image + audio file into an MP4 video using ffmpeg.

    This is how you turn a book cover + narration into a YouTube-ready video.
    """
    if not output_path:
        output_path = os.path.join(tempfile.gettempdir(), "agent_combined_video.mp4")

    # Verify ffmpeg is installed
    proc = await asyncio.create_subprocess_exec(
        "ffmpeg", "-version",
        stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL,
    )
    if await proc.wait() != 0:
        raise RuntimeError("ffmpeg is not installed. Run: brew install ffmpeg")

    # Combine: loop image for duration of audio, mux with audio
    cmd = [
        "ffmpeg", "-y",
        "-loop", "1",
        "-i", image_path,
        "-i", audio_path,
        "-c:v", "libx264",
        "-tune", "stillimage",
        "-c:a", "aac",
        "-b:a", "192k",
        "-pix_fmt", "yuv420p",
        "-shortest",
        output_path,
    ]

    proc = await asyncio.create_subprocess_exec(
        *cmd,
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE,
    )
    _, stderr = await proc.communicate()

    if proc.returncode != 0:
        raise RuntimeError(f"ffmpeg failed: {stderr.decode()[:500]}")

    # Get duration
    probe_cmd = [
        "ffprobe", "-v", "quiet", "-show_entries", "format=duration",
        "-of", "csv=p=0", output_path
    ]
    probe = await asyncio.create_subprocess_exec(
        *probe_cmd, stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.DEVNULL,
    )
    dur_out, _ = await probe.communicate()
    duration = float(dur_out.decode().strip()) if dur_out.decode().strip() else 0

    return {"path": output_path, "duration_sec": duration}


# ── YouTube Upload ─────────────────────────────────────────────────────────


async def youtube_upload(
    video_path: str,
    title: str,
    description: str = "",
    tags: list[str] | None = None,
    privacy: str = "private",
    category: str = "22",  # 22 = People & Blogs
) -> dict[str, Any]:
    """Upload a video to YouTube using the YouTube Data API v3.

    Requires:
      - YOUTUBE_CLIENT_ID and YOUTUBE_CLIENT_SECRET env vars
      - Or a pre-existing OAuth token at ~/.agent/youtube_token.json

    First run will open a browser for OAuth authorization.

    Args:
        video_path: Path to the MP4 file
        title: Video title
        description: Video description
        tags: List of tags
        privacy: "private", "unlisted", or "public"
        category: YouTube category ID (22 = People & Blogs, 27 = Education)
    """
    try:
        from google.oauth2.credentials import Credentials
        from google_auth_oauthlib.flow import InstalledAppFlow
        from googleapiclient.discovery import build
        from googleapiclient.http import MediaFileUpload
    except ImportError:
        raise RuntimeError(
            "YouTube upload requires: pip install google-auth-oauthlib google-api-python-client"
        )

    if not os.path.exists(video_path):
        raise FileNotFoundError(f"Video file not found: {video_path}")

    token_path = os.path.expanduser("~/.agent/youtube_token.json")
    credentials = None

    # Try to load existing token
    if os.path.exists(token_path):
        credentials = Credentials.from_authorized_user_file(token_path)
        if credentials and credentials.expired and credentials.refresh_token:
            from google.auth.transport.requests import Request
            credentials.refresh(Request())

    # If no valid token, do OAuth flow
    if not credentials or not credentials.valid:
        client_id = os.environ.get("YOUTUBE_CLIENT_ID")
        client_secret = os.environ.get("YOUTUBE_CLIENT_SECRET")
        if not client_id or not client_secret:
            raise RuntimeError(
                "YouTube upload requires YOUTUBE_CLIENT_ID and YOUTUBE_CLIENT_SECRET env vars.\n"
                "Get them from: https://console.cloud.google.com/apis/credentials\n"
                "Enable 'YouTube Data API v3' in your Google Cloud project."
            )

        flow = InstalledAppFlow.from_client_config(
            {
                "installed": {
                    "client_id": client_id,
                    "client_secret": client_secret,
                    "auth_uri": "https://accounts.google.com/o/oauth2/auth",
                    "token_uri": "https://oauth2.googleapis.com/token",
                    "redirect_uris": ["http://localhost"],
                }
            },
            scopes=["https://www.googleapis.com/auth/youtube.upload"],
        )
        credentials = flow.run_local_server(port=0)

        # Save token for future use
        os.makedirs(os.path.dirname(token_path), exist_ok=True)
        with open(token_path, "w") as f:
            f.write(credentials.to_json())

    # Build YouTube API client
    youtube = build("youtube", "v3", credentials=credentials)

    # Upload
    body = {
        "snippet": {
            "title": title,
            "description": description,
            "tags": tags or [],
            "categoryId": category,
        },
        "status": {
            "privacyStatus": privacy,
            "selfDeclaredMadeForKids": False,
        },
    }

    media = MediaFileUpload(video_path, mimetype="video/mp4", resumable=True)

    request = youtube.videos().insert(
        part="snippet,status",
        body=body,
        media_body=media,
    )

    response = None
    while response is None:
        _, response = request.next_chunk()

    video_id = response["id"]
    video_url = f"https://youtu.be/{video_id}"

    return {
        "video_id": video_id,
        "url": video_url,
        "title": title,
        "privacy": privacy,
    }


# ── Updated Tool Definitions ──────────────────────────────────────────────

MEDIA_TOOL_DEFINITIONS.extend([
    {
        "name": "AudioImageToVideo",
        "description": "Combine a static image and an audio file into an MP4 video using ffmpeg. "
                       "Perfect for creating audiobook videos or narrated slideshows.",
        "input_schema": {
            "type": "object",
            "properties": {
                "audio_path": {"type": "string", "description": "Path to the audio file (MP3/WAV)"},
                "image_path": {"type": "string", "description": "Path to the image file (PNG/JPG)"},
                "output_path": {"type": "string", "description": "Output MP4 file path (optional)"},
            },
            "required": ["audio_path", "image_path"],
        },
    },
    {
        "name": "YouTubeUpload",
        "description": "Upload an MP4 video to YouTube. Requires YouTube API credentials. "
                       "First run opens a browser for OAuth login. Videos default to 'private'.",
        "input_schema": {
            "type": "object",
            "properties": {
                "video_path": {"type": "string", "description": "Path to the MP4 video file"},
                "title": {"type": "string", "description": "Video title"},
                "description": {"type": "string", "description": "Video description"},
                "tags": {"type": "array", "items": {"type": "string"}, "description": "Video tags"},
                "privacy": {"type": "string", "enum": ["private", "unlisted", "public"], "description": "Privacy (default: private)"},
                "category": {"type": "string", "description": "YouTube category ID (22=People&Blogs, 27=Education)"},
            },
            "required": ["video_path", "title"],
        },
    },
])


async def execute_media_tool(tool_name: str, input_data: dict[str, Any]) -> dict[str, Any]:
    """Route a media tool call to the correct handler."""
    if tool_name == "TextToSpeech":
        return await text_to_speech(
            text=input_data["text"],
            output_path=input_data.get("output_path"),
            voice=input_data.get("voice", "alloy"),
            provider=input_data.get("provider", "auto"),
        )
    elif tool_name == "TextToImage":
        return await text_to_image(
            prompt=input_data["prompt"],
            output_path=input_data.get("output_path"),
            size=input_data.get("size", "1024x1024"),
            provider=input_data.get("provider", "auto"),
        )
    elif tool_name == "TextToVideo":
        return await text_to_video(
            prompt=input_data["prompt"],
            output_path=input_data.get("output_path"),
            duration=input_data.get("duration", 5),
            provider=input_data.get("provider", "auto"),
        )
    elif tool_name == "AudioImageToVideo":
        return await audio_image_to_video(
            audio_path=input_data["audio_path"],
            image_path=input_data["image_path"],
            output_path=input_data.get("output_path"),
        )
    elif tool_name == "YouTubeUpload":
        return await youtube_upload(
            video_path=input_data["video_path"],
            title=input_data["title"],
            description=input_data.get("description", ""),
            tags=input_data.get("tags"),
            privacy=input_data.get("privacy", "private"),
            category=input_data.get("category", "22"),
        )
    else:
        raise ValueError(f"Unknown media tool: {tool_name}")

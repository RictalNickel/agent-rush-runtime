"""Optional voice service for Agent Rush: speech to text and speech output.

Speech to text uses Moonshine (MIT) through its ONNX package. Speech output uses
LuxTTS (Apache-2.0) with one reference voice clip. Each engine is optional. A
missing engine answers 501 on its own route, and GET /health says which are ready.

Serve on localhost only (see voice/voice.service and docs/VOICE.md):
    uvicorn server:app --host 127.0.0.1 --port 8790
"""

from __future__ import annotations

import io
import logging
import os
import shutil
import subprocess
import tempfile
import threading
import wave
from contextlib import asynccontextmanager
from pathlib import Path

import numpy as np
from fastapi import FastAPI, File, HTTPException, UploadFile
from fastapi.responses import Response
from pydantic import BaseModel, Field

log = logging.getLogger("agent-voice")

HERE = Path(__file__).resolve().parent
REF_WAV = Path(os.environ.get("VOICE_REF_WAV", HERE / "ref.wav"))
DEVICE = os.environ.get("VOICE_DEVICE", "cpu")
THREADS = int(os.environ.get("VOICE_THREADS", "4"))
STT_MODEL = os.environ.get("VOICE_STT_MODEL", "moonshine/base")

MAX_UPLOAD_BYTES = 25 * 1024 * 1024
MAX_CLIP_SECONDS = 60  # Moonshine accepts 0.1 s to 64 s per call
MIN_CLIP_SECONDS = 0.1
MAX_TEXT_CHARS = 1000
FFMPEG_TIMEOUT_SECONDS = 60
SPEECH_SAMPLE_RATE = 48000  # LuxTTS output rate
REF_MIN_SECONDS = 3.0  # LuxTTS needs at least 3 seconds of reference speech
REF_MAX_SECONDS = 5.0  # only the first 5 seconds become the prompt

engines: dict[str, object] = {}
engine_errors: dict[str, str] = {}


class Transcriber:
    """Moonshine base model, ONNX on CPU. One clip at a time."""

    def __init__(self) -> None:
        if shutil.which("ffmpeg") is None:
            raise RuntimeError("ffmpeg is not installed")
        from moonshine_onnx import MoonshineOnnxModel

        self._model = MoonshineOnnxModel(model_name=STT_MODEL)
        self._lock = threading.Lock()

    def transcribe(self, pcm_16k_mono: bytes) -> str:
        from moonshine_onnx import transcribe as moonshine_transcribe

        audio = np.frombuffer(pcm_16k_mono, dtype="<i2").astype(np.float32) / 32768.0
        with self._lock:
            pieces = moonshine_transcribe(audio, self._model)
        return " ".join(p.strip() for p in pieces).strip()


def trimmed_reference(path: Path) -> tuple[str, float]:
    """Cut the reference clip at its quietest 20 ms between 3 and 5 seconds.

    LuxTTS treats the first seconds of the clip as its prompt. A cut in the middle of a word
    often makes the model continue that word at the start of a reply, so the cut goes in a pause.
    Returns a temporary WAV path and its length in seconds.
    """
    import librosa
    import soundfile as sf

    rate = 16000
    y, _ = librosa.load(str(path), sr=rate, mono=True, duration=REF_MAX_SECONDS)
    frame = int(0.02 * rate)
    lo = int(REF_MIN_SECONDS * rate)
    if len(y) < lo + frame:
        raise RuntimeError(f"reference clip is shorter than {REF_MIN_SECONDS:g} seconds")
    quiet = min(
        range(lo, len(y) - frame + 1, frame),
        key=lambda s: float(np.sqrt(np.mean(y[s : s + frame] ** 2))),
    )
    cut = quiet + frame
    fd, out = tempfile.mkstemp(prefix="voice-ref-", suffix=".wav")
    os.close(fd)
    sf.write(out, y[:cut], rate)
    return out, cut / rate


class Speaker:
    """LuxTTS with one reference voice, cloned once at startup."""

    def __init__(self) -> None:
        if not REF_WAV.is_file():
            raise RuntimeError(f"no reference voice at {REF_WAV}")
        from zipvoice.luxvoice import LuxTTS

        prompt_path, prompt_seconds = trimmed_reference(REF_WAV)
        try:
            self._tts = LuxTTS("YatharthS/LuxTTS", device=DEVICE, threads=THREADS)
            self._prompt = self._tts.encode_prompt(prompt_path, duration=prompt_seconds, rms=0.01)
        finally:
            os.remove(prompt_path)
        self._lock = threading.Lock()

    def speak(self, text: str) -> bytes:
        with self._lock:
            # t_shift 0.5 (the library default) reads the text back exactly. 0.9 (the README
            # example) added invented leading words in our test; see docs/VOICE.md.
            wav = self._tts.generate_speech(
                text, self._prompt, num_steps=4, t_shift=0.5, speed=1.0, return_smooth=False
            )
        samples = np.clip(wav.numpy().squeeze(), -1.0, 1.0)
        pcm = (samples * 32767).astype("<i2")
        buf = io.BytesIO()
        with wave.open(buf, "wb") as w:
            w.setnchannels(1)
            w.setsampwidth(2)
            w.setframerate(SPEECH_SAMPLE_RATE)
            w.writeframes(pcm.tobytes())
        return buf.getvalue()


def load_engines() -> None:
    """Load each engine on its own. A failure leaves that engine off, not the service."""
    for name, factory in (("stt", Transcriber), ("tts", Speaker)):
        try:
            engines[name] = factory()
            log.info("%s ready", name)
        except Exception as exc:  # optional engine: report, do not crash
            engine_errors[name] = str(exc) or type(exc).__name__
            log.warning("%s unavailable: %s", name, engine_errors[name])


@asynccontextmanager
async def lifespan(_: FastAPI):
    load_engines()
    yield


app = FastAPI(title="Agent Rush voice", version="1", lifespan=lifespan)


def to_pcm_16k_mono(data: bytes) -> bytes:
    """Convert any audio container ffmpeg understands to raw 16 kHz mono signed 16-bit PCM."""
    proc = subprocess.run(
        [
            "ffmpeg",
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-i",
            "pipe:0",
            "-vn",
            "-ac",
            "1",
            "-ar",
            "16000",
            "-sample_fmt",
            "s16",
            "-f",
            "s16le",
            "pipe:1",
        ],
        input=data,
        capture_output=True,
        timeout=FFMPEG_TIMEOUT_SECONDS,
        check=False,
    )
    if proc.returncode != 0 or not proc.stdout:
        log.debug("ffmpeg failed: %s", proc.stderr[-500:])
        raise ValueError("could not read the audio")
    return proc.stdout


class SpeakRequest(BaseModel):
    text: str = Field(max_length=MAX_TEXT_CHARS)


@app.get("/health")
def health() -> dict[str, bool]:
    return {"stt": "stt" in engines, "tts": "tts" in engines}


@app.post("/transcribe")
def transcribe_audio(audio: UploadFile = File(...)) -> dict[str, str]:
    engine = engines.get("stt")
    if engine is None:
        detail = engine_errors.get("stt", "not loaded")
        raise HTTPException(
            501,
            f"Speech to text is not ready ({detail}). Run voice/install.sh --stt, then restart the voice service.",
        )
    data = audio.file.read(MAX_UPLOAD_BYTES + 1)
    if not data:
        raise HTTPException(400, "The audio upload is empty.")
    if len(data) > MAX_UPLOAD_BYTES:
        raise HTTPException(413, "The audio upload is larger than 25 MB.")
    try:
        pcm = to_pcm_16k_mono(data)
    except subprocess.TimeoutExpired:
        raise HTTPException(504, "Converting the audio took too long. Send a shorter clip.") from None
    except ValueError:
        raise HTTPException(422, "Could not read that audio file. Send WAV, MP3, OGG, WebM or M4A.") from None
    seconds = len(pcm) / 2 / 16000
    if seconds < MIN_CLIP_SECONDS:
        return {"text": ""}
    if seconds > MAX_CLIP_SECONDS:
        raise HTTPException(413, "The clip is longer than 60 seconds. Split it into shorter clips.")
    return {"text": engine.transcribe(pcm)}


@app.post("/speak")
def speak(body: SpeakRequest) -> Response:
    text = body.text.strip()
    if not text:
        raise HTTPException(422, "The text is empty.")
    engine = engines.get("tts")
    if engine is None:
        detail = engine_errors.get("tts", "not loaded")
        raise HTTPException(
            501,
            f"Speech output is not ready ({detail}). Install with voice/install.sh --tts, "
            "add a reference clip as described in docs/VOICE.md, then restart the voice service.",
        )
    return Response(content=engine.speak(text), media_type="audio/wav")

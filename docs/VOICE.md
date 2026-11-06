# Voice (optional)

The voice service adds two things to an Agent Rush control plane: speech to text, so a spoken message can become a chat message, and speech output, so an agent's reply can be played back. It is a small Python service on localhost. It is off by default and is not part of the Vultr deployment.

- **Speech to text:** [Moonshine](https://github.com/moonshine-ai/moonshine) base model (MIT), run on CPU through ONNX.
- **Speech output:** [LuxTTS](https://github.com/ysharma3501/LuxTTS) (Apache-2.0), which copies one reference voice you provide.

Each engine installs and fails on its own. A missing engine answers `501` on its route, and the other keeps working.

## Install

Run on the machine that runs the control plane:

```sh
sudo voice/install.sh --stt --tts --device cpu --systemd
```

| Option | Meaning |
| --- | --- |
| `--stt`, `--tts` | Install one engine. With neither flag, both are installed. |
| `--device cpu\|cuda` | Torch build for LuxTTS. `cpu` (default) needs no GPU. `cuda` pulls several GB of CUDA libraries. |
| `--home DIR` | Where the service code lives. Default `/opt/agent-voice`. |
| `--venv DIR` | Python environment. Default `DIR/venv` under `--home`. |
| `--systemd` | Install and start `agent-voice.service`. Root only. |

The installer is safe to run again. The first start downloads the model weights from Hugging Face, which needs outbound HTTPS.

## Reference voice

LuxTTS copies the voice in one clip. Use a clean recording of at least 3 seconds, with one speaker and little background noise. The service uses the first 5 seconds and cuts them at the quietest point between 3 and 5 seconds, so the prompt ends in a pause. Copy the clip to `/opt/agent-voice/ref.wav` (or set `VOICE_REF_WAV`), then restart:

```sh
sudo cp my-voice.wav /opt/agent-voice/ref.wav
sudo systemctl restart agent-voice
```

Without a reference clip, `/speak` returns `501` with the reason, and `/health` reports `"tts": false`. Only use a voice you have the right to use. The repository ignores `voice/ref.wav`, so a clip placed there is never committed.

## Connect the control plane

When the control plane runs on the same host, set this in its environment file:

```sh
AR_VOICE_URL=http://127.0.0.1:8790
```

The web chat shows the microphone and speaker buttons only when the control plane reports voice as available.

## API

The service binds `127.0.0.1:8790`. It has no authentication, so do not expose the port.

| Route | Request | Response |
| --- | --- | --- |
| `GET /health` | none | `{"stt": true, "tts": false}` |
| `POST /transcribe` | multipart field `audio`: WAV, MP3, OGG, WebM or MP4/M4A audio | `{"text": "..."}` |
| `POST /speak` | JSON `{"text": "..."}`, up to 1000 characters | `audio/wav`, 48 kHz mono 16-bit |

Limits: uploads up to 25 MB, clips up to 60 seconds. The container is recognised from the upload's first bytes, and ffmpeg runs with `-protocol_whitelist pipe` and a demuxer allowlist (`matroska`, `ogg`, `wav`, `mp3`, `mov`), so an upload cannot make it open playlists, other files or URLs. Any other container gets `422`. Errors: `413` for too large, `422` for unreadable audio or empty text, `501` when an engine is not ready (the message says why), `504` when conversion takes too long.

```sh
curl -s http://127.0.0.1:8790/health
curl -s -F audio=@clip.webm http://127.0.0.1:8790/transcribe
curl -s -H 'content-type: application/json' \
  -d '{"text":"Hello, this is a test."}' http://127.0.0.1:8790/speak -o hello.wav
```

## Settings

| Variable | Default | Meaning |
| --- | --- | --- |
| `VOICE_REF_WAV` | `voice/ref.wav` next to `server.py` | Reference clip for speech output |
| `VOICE_DEVICE` | `cpu` | `cpu` or `cuda` for LuxTTS |
| `VOICE_THREADS` | `4` | CPU threads for LuxTTS |
| `VOICE_STT_MODEL` | `moonshine/base` | Moonshine model name |

## Measured on CPU

Measured on a 4 vCPU, 8 GB Ubuntu 24.04 VM with no GPU. Two sentences were spoken and transcribed back; all eight transcriptions matched ignoring case and punctuation. Speech output takes about one second of compute per second of speech. Raw timings are in [evidence/live/voice/README.md](evidence/live/voice/README.md).

Speech output is sampled, so the same text gives slightly different audio each time. The service uses the library's default `t_shift` of 0.5. In testing, the README example value of 0.9 added invented words at the start of replies.

## Licenses

- Moonshine (code and models): MIT.
- LuxTTS (code and weights): Apache-2.0.
- LuxTTS reads the reference clip with OpenAI Whisper tiny (MIT) to get its text.

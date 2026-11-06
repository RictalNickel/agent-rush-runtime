# Voice service: live check on a worker VM (2026-11-05)

What was run: `voice/install.sh --stt --tts --device cpu` into a scratch venv, then `voice/server.py` on `127.0.0.1:8790`. Speech was generated for two sentences and transcribed back, as WAV, WebM and MP3.

Host: Ubuntu 24.04.5, kernel 6.8, 4 vCPU, 7.7 GiB RAM, no GPU. Python 3.12.3, torch 2.8.0 (CPU build), Moonshine `useful-moonshine-onnx` 20251121 (base model), LuxTTS commit `28ae6a6`.

The host is the live worker VM. The test used only `/root/voice-test`. The live services were not touched. Apt added `ffmpeg`, `python3-venv` and `shellcheck` there. The venv and model cache were deleted afterwards because the tree passed 3 GB.

## Install

| Step | Result |
| --- | --- |
| First `voice/install.sh` run (venv created, both engines) | 2 min 26 s |
| Second run (idempotent) | 29 s, both engines importable |

The first run exposed two bugs, both fixed: `tensorboard` is needed at import time, and the import check was silent under `set -e`.

Disk after install and first start: venv 1.8 GB, Hugging Face cache 1.5 GB.

## Round trip

Reference clip: the sample `beckett.wav` that ships in the `moonshine_onnx` package, 10 s of read speech. It stands in for an operator clip. It is not committed.

The reference is cut at the quietest 20 ms between 3 s and 5 s (see "Why the reference is cut", below).

| Sentence | Speak (HTTP) | Speech length | Transcribe WAV, cold / warm | WebM | MP3 | Text match |
| --- | --- | --- | --- | --- | --- | --- |
| "The quick brown fox jumps over the lazy dog." | 5.43 s | 4.60 s | 0.67 s / 0.63 s | 0.63 s | 0.67 s | yes, all four |
| "Agent Rush runs every tool call inside a sealed box." | 5.72 s | 5.50 s | 0.64 s / 0.64 s | 0.64 s | 0.63 s | yes, all four |

Transcripts:

- short, WAV: "The quick brown fox jumps over the lazy dog."
- agent, WAV: "Agent Rush runs every tool call inside a sealed box."

The full output is in `roundtrip-output.txt` in this folder.

Timings are wall-clock from `curl`. Speech output takes about 1.0 to 1.2 seconds of compute per second of speech on this VM, so a 5-second reply takes about 5.5 seconds to return. Streaming playback would need a faster engine or a GPU.

## Error paths

| Request | Response |
| --- | --- |
| `POST /transcribe` with an empty upload | 400 |
| `POST /transcribe` with 2 KB of random bytes | 422, "Could not read that audio file" |
| `POST /speak` with whitespace only | 422 |
| `POST /speak` with 1001 characters | 422 |
| Server with no reference clip: `GET /health` | `{"stt": true, "tts": false}` |
| Same server: `POST /speak` | 501, with the reason and the install hint |
| Same server: `POST /transcribe` | 200 (STT still works) |

Resident memory of the server with both engines loaded: about 2.2 GB.

## Why the reference is cut

Two problems showed up with the plain 5-second cut of the reference. The tables below come from experiment scripts run on the same host, not from the server.

1. Sampling is not seeded. `generate_cpu` draws `torch.randn` on every call, so the same text gives different audio each time.
2. The reference's first 5 seconds are cut at a fixed point. When that point falls inside a word, the model continues the word at the start of the reply. Examples: "Ivy." or "Ah," in front of "The quick brown fox", and "Legend." in front of "Agent Rush".

Runs with the plain 5-second cut:

| t_shift | Steps | Text | Exact matches |
| --- | --- | --- | --- |
| 0.9 (README example) | 4 | short | leading words, e.g. "I'm a bit of a whip. Brooke," |
| 0.5 (library default) | 4 | short | 1 of 4 |
| 0.5 | 4 | 3 s prompt | exact once |

Runs with the reference cut at a pause (quietest 20 ms between 3 and 5 s, at 3.26 s for this clip):

| Text | Exact matches |
| --- | --- |
| short | 4 of 4 |
| agent | 4 of 4 |

The server uses this cut and `t_shift` 0.5. Eight trials out of eight matched exactly, and the final round trip matched in all eight transcriptions shown above.

## Not verified

- CUDA. No GPU here, so `--device cuda` is untested.
- Quality with an operator's own voice. The check used a sample clip from the Moonshine package.
- Accuracy across many sentences. Two sentences, with 4 repeats each for the cut setting.
- Real-time streaming. Generation is slower than real time on 4 vCPU.

## Stop and cleanup

The server was stopped (port 8790 closed, no tracebacks in the log). `/root/voice-test` was deleted after the numbers above were copied out, because it was 3.3 GB.

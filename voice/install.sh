#!/usr/bin/env bash
# Install the optional voice service: Moonshine speech to text and LuxTTS speech output.
#
# Usage: voice/install.sh [--stt] [--tts] [--device cpu|cuda] [--home DIR] [--venv DIR] [--systemd]
#   --stt        install speech to text only
#   --tts        install speech output only (default with neither flag: both)
#   --device     torch build for LuxTTS: cpu (default, CPU wheels) or cuda (PyPI CUDA wheels, several GB)
#   --home       install root for the service code (default /opt/agent-voice, or $AGENT_VOICE_HOME)
#   --venv       Python virtual environment (default: HOME/venv)
#   --systemd    install and start agent-voice.service (root only)
#
# Safe to run again: pip reinstalls the pinned versions, the service code is replaced in place.
# Nothing here listens on a public address. The service binds 127.0.0.1:8790.
set -euo pipefail

SRC_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
HOME_DIR="${AGENT_VOICE_HOME:-/opt/agent-voice}"
VENV_DIR=""
WANT_STT=0
WANT_TTS=0
DEVICE=cpu
SYSTEMD=0

# Pinned upstream versions. Torch must match what LuxTTS was tested with.
TORCH_VERSION=2.8.0
TORCHAUDIO_VERSION=2.8.0
LUXTTS_COMMIT=28ae6a61151684fffc9d1a7aa15eafa02286fe0b

while [ $# -gt 0 ]; do
  case "$1" in
    --stt) WANT_STT=1 ;;
    --tts) WANT_TTS=1 ;;
    --device) DEVICE="${2:?--device needs cpu or cuda}"; shift ;;
    --home) HOME_DIR="${2:?--home needs a directory}"; shift ;;
    --venv) VENV_DIR="${2:?--venv needs a directory}"; shift ;;
    --systemd) SYSTEMD=1 ;;
    -h | --help) sed -n '2,13p' "$0"; exit 0 ;;
    *) echo "Unknown option: $1" >&2; exit 2 ;;
  esac
  shift
done

if [ "$WANT_STT" -eq 0 ] && [ "$WANT_TTS" -eq 0 ]; then WANT_STT=1; WANT_TTS=1; fi
case "$DEVICE" in cpu | cuda) ;; *) echo "--device must be cpu or cuda" >&2; exit 2 ;; esac
VENV_DIR="${VENV_DIR:-$HOME_DIR/venv}"

echo "Voice install: stt=$WANT_STT tts=$WANT_TTS device=$DEVICE"
echo "  code:  $HOME_DIR/app"
echo "  venv:  $VENV_DIR"

# System packages: venv support, ffmpeg for uploads, git for the LuxTTS commit pin.
apt_needed=()
python3 -c 'import ensurepip' 2>/dev/null || apt_needed+=(python3-venv)
if [ "$WANT_STT" -eq 1 ] && ! command -v ffmpeg >/dev/null 2>&1; then apt_needed+=(ffmpeg); fi
if [ "$WANT_TTS" -eq 1 ] && ! command -v git >/dev/null 2>&1; then apt_needed+=(git); fi
if [ "${#apt_needed[@]}" -gt 0 ]; then
  if [ "$(id -u)" -ne 0 ]; then
    echo "Missing system packages: ${apt_needed[*]}. Install them as root, then run this again." >&2
    exit 1
  fi
  DEBIAN_FRONTEND=noninteractive apt-get update -q
  DEBIAN_FRONTEND=noninteractive apt-get install -yq "${apt_needed[@]}"
fi

if [ ! -x "$VENV_DIR/bin/python" ]; then
  python3 -m venv "$VENV_DIR"
  echo "  created venv"
fi
PIP=("$VENV_DIR/bin/python" -m pip)
"${PIP[@]}" install -q --upgrade pip
"${PIP[@]}" install -q -r "$SRC_DIR/requirements-base.txt"

if [ "$WANT_STT" -eq 1 ]; then
  "${PIP[@]}" install -q -r "$SRC_DIR/requirements-stt.txt"
fi

if [ "$WANT_TTS" -eq 1 ]; then
  torch_extra=()
  if [ "$DEVICE" = cpu ]; then
    # CPU wheels: no CUDA runtime is pulled in. pip prefers the +cpu local build of the same version.
    torch_extra=(--extra-index-url https://download.pytorch.org/whl/cpu)
  fi
  "${PIP[@]}" install -q "${torch_extra[@]}" "torch==$TORCH_VERSION" "torchaudio==$TORCHAUDIO_VERSION"
  "${PIP[@]}" install -q -r "$SRC_DIR/requirements-tts.txt"
  "${PIP[@]}" install -q --no-deps "git+https://github.com/ysharma3501/LuxTTS.git@$LUXTTS_COMMIT"
fi

install -d -m 0755 "$HOME_DIR/app"
install -m 0644 "$SRC_DIR/server.py" "$HOME_DIR/app/server.py"

# Import checks fail the install: a silent broken engine would only show up as 501 later.
if [ "$WANT_STT" -eq 1 ]; then
  "$VENV_DIR/bin/python" -c 'import moonshine_onnx'
  echo "  speech to text: importable"
fi
if [ "$WANT_TTS" -eq 1 ]; then
  "$VENV_DIR/bin/python" -c 'import zipvoice.luxvoice'
  echo "  speech output: importable"
fi

if [ "$SYSTEMD" -eq 1 ]; then
  if [ "$(id -u)" -ne 0 ]; then
    echo "--systemd needs root" >&2
    exit 1
  fi
  sed -e "s#/opt/agent-voice/venv#$VENV_DIR#g" \
    -e "s#/opt/agent-voice#$HOME_DIR#g" \
    -e "s#VOICE_DEVICE=cpu#VOICE_DEVICE=$DEVICE#" \
    "$SRC_DIR/voice.service" > /etc/systemd/system/agent-voice.service
  systemctl daemon-reload
  systemctl enable --now agent-voice
  echo "  systemd: agent-voice.service enabled and started (127.0.0.1:8790)"
fi

if [ ! -f "$HOME_DIR/ref.wav" ]; then
  echo "No reference voice yet. Copy a clip of at least 3 seconds to $HOME_DIR/ref.wav, then restart the service."
  echo "See docs/VOICE.md."
fi
echo "Voice install finished."

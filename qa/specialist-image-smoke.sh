#!/usr/bin/env bash
# Qualify the actual immutable image, including installed executables and helpers.
set -euo pipefail
image=${1:?Usage: qa/specialist-image-smoke.sh IMAGE FAMILY}
family=${2:?Image family required}
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
[[ $family =~ ^(core|python|browser|platform|data|documents|mobile|finance|suite|finance-service)$ ]] || exit 2
[[ $(docker image inspect --format '{{.Config.User}}' "$image") == 1000:1000 ]] || {
    printf 'Image must default to uid/gid 1000:1000.\n' >&2; exit 1;
}
runtime=()
if [[ -n ${BOXORO_SMOKE_RUNTIME:-} ]]; then runtime+=(--runtime "$BOXORO_SMOKE_RUNTIME"); fi
interpreter=/usr/bin/python3
if [[ $family == finance-service ]]; then interpreter=/opt/openbb/venv/bin/python; fi
docker run --rm "${runtime[@]}" --read-only --network none \
    --cap-drop ALL --security-opt no-new-privileges --pids-limit 256 \
    --memory 6g --cpus 2 \
    --tmpfs /workspace:rw,nosuid,nodev,uid=1000,gid=1000,size=512m \
    --tmpfs /tmp:rw,nosuid,nodev,mode=1777,size=1g \
    --tmpfs /home/agent:rw,nosuid,nodev,uid=1000,gid=1000,size=512m \
    --tmpfs /var/lib/boxoro-finance:rw,nosuid,nodev,uid=1000,gid=1000,mode=0700,size=256m \
    --mount "type=bind,src=$root/qa/specialist-image-smoke.py,dst=/opt/specialist-smoke.py,readonly" \
    --entrypoint "$interpreter" "$image" -I /opt/specialist-smoke.py "$family"

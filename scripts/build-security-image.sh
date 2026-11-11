#!/usr/bin/env bash
# Build locally, verify, and record actual Docker identities. Never pushes.
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
base=''
target=core
tag=boxoro-security:local
metadata="$root/security-image-build.json"
smoke=1
usage() {
    cat <<'HELP'
Usage: scripts/build-security-image.sh [options]
  --base IMAGE        Registry/repository@sha256:<manifest> or local sha256:<image ID>.
                      If omitted, build deploy/workspace.Dockerfile locally first.
  --target TARGET     core (default) or malware (ClamAV + YARA; larger resource budget).
  --tag TAG           Local output tag (default boxoro-security:local).
  --metadata FILE     Save observed image IDs, RepoDigests and lock hash as JSON.
  --skip-smoke        Build and record identities without running container checks.
  --help              Show this help.

Requires Docker with BuildKit, Python 3, and outbound access to pinned upstream
artifacts/PyPI/Rust (plus Ubuntu repositories for a new base or malware builder).
Optional BOXORO_BUILD_CA_FILE is supplied as the transient BuildKit proxy_ca secret
for the security layers and requires --base. The existing workspace Dockerfile
does not consume that secret; prepare its base separately when a proxy CA is needed.
The script does not publish an image or manufacture a registry manifest digest.
HELP
}
while (($#)); do
    case "$1" in
        --base|--target|--tag|--metadata)
            (($# >= 2)) || { printf 'Missing value for %s\n' "$1" >&2; exit 2; }
            case "$1" in
                --base) base=$2 ;;
                --target) target=$2 ;;
                --tag) tag=$2 ;;
                --metadata) metadata=$2 ;;
            esac
            shift 2 ;;
        --skip-smoke) smoke=0; shift ;;
        --help|-h) usage; exit 0 ;;
        *) printf 'Unknown option: %s\n' "$1" >&2; exit 2 ;;
    esac
done
[[ $target == core || $target == malware ]] || { printf 'Target must be core or malware.\n' >&2; exit 2; }
if [[ -n $base && ! $base =~ ^sha256:[a-f0-9]{64}$ && ! $base =~ ^[^[:space:]@]+@sha256:[a-f0-9]{64}$ ]]; then
    printf 'Base must be an immutable registry digest or an existing local image ID.\n' >&2
    exit 2
fi
if [[ -n ${BOXORO_BUILD_CA_FILE:-} && -z $base ]]; then
    printf 'BOXORO_BUILD_CA_FILE requires --base: the existing workspace build does not consume the proxy_ca secret. Prepare its base separately.\n' >&2
    exit 2
fi
command -v docker >/dev/null || { printf 'Docker is required; no image has been built.\n' >&2; exit 1; }
command -v python3 >/dev/null || { printf 'Python 3 is required.\n' >&2; exit 1; }
docker info >/dev/null
export DOCKER_BUILDKIT=1
build_args=(--platform linux/amd64)
for proxy in HTTP_PROXY HTTPS_PROXY NO_PROXY http_proxy https_proxy no_proxy; do
    if [[ -n ${!proxy:-} ]]; then build_args+=(--build-arg "$proxy"); fi
done
if [[ -n ${BOXORO_BUILD_CA_FILE:-} ]]; then
    [[ -f $BOXORO_BUILD_CA_FILE ]] || { printf 'BOXORO_BUILD_CA_FILE is not a file.\n' >&2; exit 2; }
    build_args+=(--secret "id=proxy_ca,src=$BOXORO_BUILD_CA_FILE")
fi
local_base_tag="boxoro-workspace-security-base:build-$(date +%s)-$$"
base_mode=registry_manifest
if [[ -z $base ]]; then
    base_mode=locally_built_workspace
    printf 'Building the existing workspace image for linux/amd64.\n' >&2
    docker build "${build_args[@]}" -f "$root/deploy/workspace.Dockerfile" -t "$local_base_tag" "$root/deploy" >&2
    base=$local_base_tag
elif [[ $base == sha256:* ]]; then
    base_mode=local_image_id
    docker image inspect "$base" >/dev/null
    docker tag "$base" "$local_base_tag"
    base=$local_base_tag
else
    docker pull --platform linux/amd64 "$base" >&2
fi
base_id=$(docker image inspect --format '{{.Id}}' "$base")
[[ $base_id =~ ^sha256:[a-f0-9]{64}$ ]] || { printf 'Docker returned an invalid base image ID.\n' >&2; exit 1; }
base_platform=$(docker image inspect --format '{{.Os}}/{{.Architecture}}' "$base")
[[ $base_platform == linux/amd64 ]] || { printf 'Only linux/amd64 is validated for this workspace base.\n' >&2; exit 1; }

docker build "${build_args[@]}" --target "$target" \
    --build-arg "BOXORO_WORKSPACE_IMAGE=$base" \
    -f "$root/deploy/security.Dockerfile" -t "$tag" "$root/deploy" >&2
[[ $(docker image inspect --format '{{.Id}}' "$base") == "$base_id" ]] || {
    printf 'Local base image identity changed during the build.\n' >&2; exit 1;
}
image_id=$(docker image inspect --format '{{.Id}}' "$tag")
[[ $image_id =~ ^sha256:[a-f0-9]{64}$ ]] || { printf 'Docker returned an invalid output image ID.\n' >&2; exit 1; }
if ((smoke)); then
    bash "$root/qa/security-image-smoke.sh" "$image_id" "$target" >&2
fi
image_inspect=$(mktemp)
base_inspect=$(mktemp)
trap 'rm -f "$image_inspect" "$base_inspect"' EXIT
docker image inspect "$image_id" > "$image_inspect"
docker image inspect "$base_id" > "$base_inspect"
python3 - "$image_inspect" "$base_inspect" "$root" "$base_mode" "$base" "$target" "$metadata" "$smoke" <<'PY'
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import subprocess
import sys
image_file, base_file, root, base_mode, base_ref, target, output, smoke = sys.argv[1:]
image = json.loads(Path(image_file).read_text())[0]
base = json.loads(Path(base_file).read_text())[0]
root = Path(root)
result = {
    'built_at': datetime.now(timezone.utc).isoformat(),
    'target': target,
    'platform': image['Os'] + '/' + image['Architecture'],
    'image_id': image['Id'],
    'repo_digests': image.get('RepoDigests') or [],
    'base': {'mode': base_mode, 'reference': base_ref, 'image_id': base['Id'], 'repo_digests': base.get('RepoDigests') or []},
    'source_commit': subprocess.check_output(['git', '-C', str(root), 'rev-parse', 'HEAD'], text=True).strip(),
    'source_dirty': bool(subprocess.check_output(['git', '-C', str(root), 'status', '--porcelain'], text=True).strip()),
    'tools_lock_sha256': hashlib.sha256((root / 'deploy/security/tools.lock.json').read_bytes()).hexdigest(),
    'requirements_lock_sha256': hashlib.sha256((root / 'deploy/security/requirements.lock').read_bytes()).hexdigest(),
    'container_smoke': 'passed' if smoke == '1' else 'not_run',
    'identity_note': 'image_id is the local Docker config digest. repo_digests contains only observed registry manifest digests; it may be empty until publishing/pulling.',
}
destination = Path(output)
destination.parent.mkdir(parents=True, exist_ok=True)
text = json.dumps(result, indent=2) + '\n'
destination.write_text(text)
print(text, end='')
PY
printf 'Built %s. Use the local image_id on this Docker host; use an observed RepoDigest for registry distribution.\n' "$image_id" >&2

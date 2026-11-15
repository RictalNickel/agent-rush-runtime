#!/usr/bin/env bash
# Compose the existing security image and the specialist suite on one worker.
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
base=''
tag=boxoro-agents:local
security_target=malware
metadata="$root/full-agent-image-build.json"
while (($#)); do
    case "$1" in
        --base|--tag|--security-target|--metadata)
            (($# >= 2)) || { printf 'Missing value for %s\n' "$1" >&2; exit 2; }
            case "$1" in
                --base) base=$2 ;;
                --tag) tag=$2 ;;
                --security-target) security_target=$2 ;;
                --metadata) metadata=$2 ;;
            esac
            shift 2 ;;
        --help|-h)
            printf '%s\n' 'Usage: scripts/build-full-agent-image.sh [--base IMMUTABLE_WORKSPACE_IMAGE] [--tag TAG] [--security-target core|malware] [--metadata FILE]' \
                'Builds the security image (malware by default), adds the specialist suite, and runs both smoke suites. Does not publish.'
            exit 0 ;;
        *) printf 'Unknown option: %s\n' "$1" >&2; exit 2 ;;
    esac
done
[[ $security_target == core || $security_target == malware ]] || exit 2
temporary=$(mktemp -d)
trap 'rm -rf -- "$temporary"' EXIT
security_args=(--target "$security_target" --tag "boxoro-security-composition:build-$$" --metadata "$temporary/security.json")
if [[ -n $base ]]; then security_args+=(--base "$base"); fi
bash "$root/scripts/build-security-image.sh" "${security_args[@]}" > "$temporary/security.stdout"
security_id=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["image_id"])' "$temporary/security.json")
bash "$root/scripts/build-specialist-image.sh" --base "$security_id" --target suite \
    --tag "$tag" --metadata "$temporary/specialist.json" > "$temporary/specialist.stdout"
full_id=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["image_id"])' "$temporary/specialist.json")
# Qualify the original scanner paths again in the final combined image.
bash "$root/qa/security-image-smoke.sh" "$full_id" "$security_target" >&2
python3 - "$temporary/security.json" "$temporary/specialist.json" "$metadata" <<'PY'
import json
from pathlib import Path
import sys
security = json.loads(Path(sys.argv[1]).read_text())
specialist = json.loads(Path(sys.argv[2]).read_text())
receipt = {"image_id": specialist["image_id"], "repo_digests": specialist["repo_digests"],
           "security_base": security, "specialist": specialist, "combined_security_smoke": "passed"}
destination = Path(sys.argv[3])
destination.parent.mkdir(parents=True, exist_ok=True)
text = json.dumps(receipt, indent=2) + "\n"
destination.write_text(text)
print(text, end="")
PY

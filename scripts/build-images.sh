#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
# `build-images.sh --workspace` builds only the session workspace image (on the worker)
# and prints the digest reference to record in deploy/images.lock.json as "workspace".
if [ "${1:-}" = "--workspace" ]; then
  docker build -f deploy/workspace.Dockerfile -t agent-rush-workspace:local deploy
  echo "agent-rush-workspace@$(docker image inspect agent-rush-workspace:local --format '{{.Id}}')"
  exit 0
fi
: "${CARGO_HOME:=/workspace/.toolchains/cargo}"
: "${RUSTUP_HOME:=/workspace/.toolchains/rustup}"
export CARGO_HOME RUSTUP_HOME
export PATH="$CARGO_HOME/bin:$PATH"
cargo build --locked --release -j 4
build_context="$(mktemp -d /tmp/agent-rush-image.XXXXXX)"
trap 'rm -rf "$build_context"' EXIT
python3 - "$build_context" <<'PY'
import hashlib,json,pathlib,shutil,sys
root=pathlib.Path.cwd();ctx=pathlib.Path(sys.argv[1]);lock=json.loads((root/'deploy/images.lock.json').read_text())
assert (root/'deploy/control.Dockerfile').read_text().splitlines()[0]=='FROM '+lock['control_base'], 'Base image differs from lock'
assert lock['cli_sandbox'] in (root/'src/worker.rs').read_text(), 'Sandbox image differs from worker policy'
certs=b''.join(p.read_bytes() for p in sorted(pathlib.Path('/usr/share/ca-certificates/mozilla').glob('*.crt')))
assert certs and hashlib.sha256(certs).hexdigest()==lock['public_ca_bundle_sha256'], 'Public CA bundle differs from reviewed lock; refresh/verify it before building'
(ctx/'ca-certificates.crt').write_bytes(certs)
shutil.copyfile('/usr/share/doc/ca-certificates/copyright',ctx/'ca-certificates-copyright')
shutil.copyfile(root/'target/release/agent-rush',ctx/'agent-rush');(ctx/'agent-rush').chmod(0o755)
shutil.copyfile(root/'deploy/control.Dockerfile',ctx/'Dockerfile')
(ctx/'state').mkdir();(ctx/'state'/'.keep').touch()
# Context is these exact public inputs; no checkout, environment file, SSH key or credential directory.
(ctx/'base-ref').write_text(lock['control_base']);(ctx/'sandbox-ref').write_text(lock['cli_sandbox'])
PY
control_base="$(cat "$build_context/base-ref")"
sandbox_base="$(cat "$build_context/sandbox-ref")"
rm "$build_context/base-ref" "$build_context/sandbox-ref"
docker image inspect "$control_base" >/dev/null 2>&1 || docker pull "$control_base"
docker image inspect "$sandbox_base" >/dev/null 2>&1 || docker pull "$sandbox_base"
source_revision="$(git rev-parse HEAD)"
docker build --platform linux/amd64 --network=none --label "org.opencontainers.image.revision=$source_revision" --tag agent-rush-control:local "$build_context"
mkdir -p /workspace/artifacts
docker image inspect agent-rush-control:local --format '{{json .}}' > /workspace/artifacts/agent-rush-control-image.json
docker save agent-rush-control:local | gzip -1 > /workspace/artifacts/agent-rush-control-image.tar.gz
printf 'Built native control image; sandbox image present. No containers or cloud machines started.\n'

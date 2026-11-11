#!/usr/bin/env bash
# Run real scanner smoke checks in a non-root, read-only, networkless container.
set -euo pipefail
image=${1:?Usage: qa/security-image-smoke.sh IMAGE [core|malware]}
target=${2:-core}
[[ $target == core || $target == malware ]] || { printf 'Unknown image target.\n' >&2; exit 2; }
[[ $(docker image inspect --format '{{.Config.User}}' "$image") == 1000:1000 ]] || {
    printf 'Security image must default to uid/gid 1000:1000.\n' >&2; exit 1;
}
runtime=()
if [[ -n ${BOXORO_SMOKE_RUNTIME:-} ]]; then runtime+=(--runtime "$BOXORO_SMOKE_RUNTIME"); fi
docker run --rm -i "${runtime[@]}" --read-only --network none \
    --cap-drop ALL --security-opt no-new-privileges --pids-limit 128 \
    --memory 4g --cpus 2 \
    --tmpfs /workspace:rw,nosuid,nodev,uid=1000,gid=1000,size=256m \
    --tmpfs /tmp:rw,nosuid,nodev,mode=1777,size=512m \
    --tmpfs /home/agent:rw,nosuid,nodev,uid=1000,gid=1000,size=128m \
    --env "BOXORO_SMOKE_TARGET=$target" --entrypoint /usr/bin/python3 "$image" -I - <<'PY'
import errno
import json
import os
from pathlib import Path
import subprocess

assert os.getuid() == 1000, 'image user is not uid 1000'
try:
    Path('/opt/security/smoke-write').write_text('must fail')
except OSError as error:
    assert error.errno in (errno.EROFS, errno.EACCES), error
else:
    raise AssertionError('security installation is writable')

environment = {
    'PATH': '/opt/security/bin:/usr/bin:/bin', 'HOME': '/home/agent',
    'TMPDIR': '/tmp', 'LANG': 'C.UTF-8', 'SEMGREP_SEND_METRICS': 'off',
    'SEMGREP_ENABLE_VERSION_CHECK': '0', 'SYFT_CHECK_FOR_APP_UPDATE': 'false',
    'TRIVY_DISABLE_TELEMETRY': 'true',
}
lock = json.loads(Path('/opt/security/tools.lock.json').read_text())
versions = {tool['name']: tool['version'] for tool in lock['tools']}
commands = {
    'gitleaks': ['version'], 'trivy': ['--version'], 'syft': ['version'],
    'ruff': ['--version'], 'shellcheck': ['--version'], 'hadolint': ['--version'],
    'semgrep': ['--version'], 'rustfmt': ['--version'],
}
if os.environ['BOXORO_SMOKE_TARGET'] == 'malware':
    commands.update({'yara': ['--version'], 'clamscan': ['--version'],
                     'freshclam': ['--config-file=/dev/null', '--version']})
versions['semgrep'] = lock['python']['semgrep_version']
versions['clamscan'] = versions['freshclam'] = versions['clamav']
for name, arguments in commands.items():
    result = subprocess.run(['/opt/security/bin/' + name, *arguments], env=environment,
                            capture_output=True, text=True, timeout=60, check=True)
    if name in versions:
        assert versions[name] in result.stdout + result.stderr, f'unexpected {name} version'

helper = ['/usr/bin/python3', '-I', '/opt/box/bin/security-scan']
def request(payload):
    result = subprocess.run(helper + [json.dumps(payload)], env=environment,
                            capture_output=True, text=True, timeout=150, check=True)
    return json.loads(result.stdout)

doctor = request({'action': 'doctor'})
for name in commands:
    if name in doctor['tools']:
        assert doctor['tools'][name]['installed'], f'runner cannot find {name}'
assert doctor['tool_manifest']['schema_version'] == 1

project = Path('/workspace/smoke-project')
project.mkdir()
# Synthetic, locally generated pattern; this is not an issued credential.
test_token = 'ghp_' + 'aB3dE5fG7hJ9kL2mN4pQ6rS8tU0vW1xY3zA5'
(project / 'config.py').write_text('API_TOKEN = ' + repr(test_token) + '\n')
secrets = request({'action': 'scan', 'profile': 'secrets', 'path': 'smoke-project', 'offline': True, 'timeout_seconds': 120})
assert secrets['gate'] == 'fail', secrets
assert secrets['summary']['findings'] >= 1, secrets
assert test_token not in json.dumps(secrets)
for relative in secrets['reports'].values():
    report = Path('/workspace') / relative
    assert report.is_file(), str(report)
    assert test_token not in report.read_text(), 'report contains raw synthetic credential'

# This image deliberately ships no mutable vulnerability/signature database.
# Missing data must remain incomplete, never a pass.
(project / 'Cargo.lock').write_text('version = 4\n\n[[package]]\nname = "fixture"\nversion = "0.1.0"\n')
dependencies = request({'action': 'scan', 'profile': 'dependencies', 'path': 'smoke-project', 'offline': True, 'timeout_seconds': 120})
assert dependencies['gate'] == 'incomplete', dependencies
print(json.dumps({'smoke': 'passed', 'tools': sorted(commands), 'uid': os.getuid(),
                  'root_read_only': True, 'network': 'none', 'synthetic_secret_redacted': True,
                  'missing_database_gate': dependencies['gate']}))
PY

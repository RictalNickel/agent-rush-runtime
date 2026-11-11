# syntax=docker/dockerfile:1.7
# The build script supplies an immutable registry digest or a verified local
# workspace image. No base tag default is provided deliberately.
ARG BOXORO_WORKSPACE_IMAGE
FROM ${BOXORO_WORKSPACE_IMAGE} AS core
ARG TARGETOS
ARG TARGETARCH
USER 0:0

LABEL org.opencontainers.image.title="Boxoro security workspace" \
      org.opencontainers.image.description="Offline-first static security tools; optional malware target; no credentials or databases baked in"

COPY security/tools.lock.json security/requirements.lock security/install-tools.py /opt/security/

# A build proxy CA, when required, is a transient BuildKit secret. It is never
# retained in a layer. TLS verification stays enabled for all downloads.
RUN --mount=type=secret,id=proxy_ca <<'SH'
set -eu
test "${TARGETOS:-linux}/${TARGETARCH:-amd64}" = linux/amd64
build_ca=/etc/ssl/certs/ca-certificates.crt
if [ -s /run/secrets/proxy_ca ]; then
    cat "$build_ca" /run/secrets/proxy_ca > /tmp/boxoro-build-ca.pem
    build_ca=/tmp/boxoro-build-ca.pem
fi
export SSL_CERT_FILE="$build_ca"
python3 -I /opt/security/install-tools.py --platform linux/amd64 --profile core
python3 -m venv /opt/security/venv
/opt/security/venv/bin/pip --isolated --disable-pip-version-check install \
    --index-url https://pypi.org/simple --cert "$build_ca" \
    --require-hashes --only-binary=:all: --no-cache-dir \
    -r /opt/security/requirements.lock
/opt/security/venv/bin/pip --isolated --disable-pip-version-check check
ln -s /opt/security/venv/bin/semgrep /opt/security/bin/semgrep
# Use the inherited, version-pinned toolchain; the runtime invokes the actual
# binary rather than a rustup proxy that could honor workspace toolchain files.
CARGO_HOME=/opt/rust/cargo RUSTUP_HOME=/opt/rust/rustup \
    /opt/rust/cargo/bin/rustup component add rustfmt --toolchain 1.90.0-x86_64-unknown-linux-gnu
ln -s /opt/rust/rustup/toolchains/1.90.0-x86_64-unknown-linux-gnu/bin/rustfmt /opt/security/bin/rustfmt
rm -rf /tmp/boxoro-security-artifacts /tmp/boxoro-build-ca.pem
SH

RUN /opt/security/venv/bin/python -I - <<'PY'
import hashlib
import importlib.metadata
import json
from pathlib import Path
import subprocess
root = Path('/opt/security')
lock = json.loads((root / 'tools.lock.json').read_text())
version = importlib.metadata.version('semgrep')
assert version == lock['python']['semgrep_version'], 'unexpected Semgrep installation'
manifest = json.loads((root / 'versions.json').read_text())
manifest['tools']['semgrep'] = {
    'version': version,
    'kind': 'hashed-python-wheels',
    'license': lock['python']['license'],
    'requirements_sha256': hashlib.sha256((root / 'requirements.lock').read_bytes()).hexdigest(),
}
manifest['tools']['rustfmt'] = {
    'toolchain': lock['rustfmt']['toolchain'],
    'version': subprocess.check_output(['/opt/security/bin/rustfmt', '--version'], text=True).strip(),
    'kind': 'rustup-verified-component',
}
(root / 'versions.json').write_text(json.dumps(manifest, indent=2, sort_keys=True) + '\n')
PY

COPY box-tools/security-scan /opt/box/bin/security-scan
COPY security/boxoro_security /opt/box/security/boxoro_security
COPY security/rules /opt/box/security/rules
RUN chmod 0755 /opt/box/bin/security-scan \
 && ln -s /opt/security/tools.lock.json /opt/box/security/tools.lock.json \
 && ln -s /opt/security/versions.json /opt/box/security/versions.json \
 && chmod -R a+rX /opt/security /opt/box/security \
 && find /opt/box/security -type d -name __pycache__ -prune -exec rm -rf '{}' +

ENV PATH=/opt/security/bin:/opt/security/venv/bin:${PATH} \
    SEMGREP_SEND_METRICS=off \
    SEMGREP_ENABLE_VERSION_CHECK=0 \
    SYFT_CHECK_FOR_APP_UPDATE=false \
    TRIVY_DISABLE_TELEMETRY=true
RUN printf 'export PATH=%s\n' "$PATH" > /etc/profile.d/01-security-path.sh
USER 1000:1000
WORKDIR /workspace
CMD ["sleep", "infinity"]

# Compile the exact upstream YARA commit in a disposable stage. Only binaries,
# notices and provenance are copied into the malware image.
FROM core AS malware-builder
USER 0:0
RUN --mount=type=secret,id=proxy_ca <<'SH'
set -eu
build_ca=/etc/ssl/certs/ca-certificates.crt
if [ -s /run/secrets/proxy_ca ]; then
    cat "$build_ca" /run/secrets/proxy_ca > /tmp/boxoro-build-ca.pem
    build_ca=/tmp/boxoro-build-ca.pem
fi
export SSL_CERT_FILE="$build_ca" GIT_SSL_CAINFO="$build_ca"
apt-get -o "Acquire::https::CaInfo=$build_ca" update
apt-get -o "Acquire::https::CaInfo=$build_ca" install -y --no-install-recommends autoconf automake libtool libssl-dev
python3 -I /opt/security/install-tools.py --platform linux/amd64 --profile malware --only yara
rm -rf /var/lib/apt/lists/* /tmp/boxoro-build-ca.pem
SH

USER 1000:1000

FROM core AS malware
USER 0:0
COPY --from=malware-builder /opt/security/bin/yara /opt/security/bin/yarac /opt/security/bin/
COPY --from=malware-builder /opt/security/licenses/yara /opt/security/licenses/yara
COPY --from=malware-builder /opt/security/versions.json /opt/security/versions.json
RUN --mount=type=secret,id=proxy_ca <<'SH'
set -eu
build_ca=/etc/ssl/certs/ca-certificates.crt
if [ -s /run/secrets/proxy_ca ]; then
    cat "$build_ca" /run/secrets/proxy_ca > /tmp/boxoro-build-ca.pem
    build_ca=/tmp/boxoro-build-ca.pem
fi
export SSL_CERT_FILE="$build_ca"
# Disallow service startup while installing the verified upstream package.
made_policy=0
if [ ! -e /usr/sbin/policy-rc.d ]; then
    printf '#!/bin/sh\nexit 101\n' > /usr/sbin/policy-rc.d
    chmod 0755 /usr/sbin/policy-rc.d
    made_policy=1
fi
python3 -I /opt/security/install-tools.py --platform linux/amd64 --profile malware --only clamav
# The vendor package includes a 376 MB Rust build archive. The runtime links
# libclamav.so; the static archive is only needed when building other programs.
rm -f /usr/local/lib/libclamav_rust.a
/opt/security/bin/yara --version
/opt/security/bin/clamscan --version
/opt/security/bin/freshclam --config-file=/dev/null --version
if [ "$made_policy" = 1 ]; then rm /usr/sbin/policy-rc.d; fi
rm -rf /tmp/boxoro-security-artifacts /tmp/boxoro-build-ca.pem
SH

USER 1000:1000
WORKDIR /workspace
CMD ["sleep", "infinity"]

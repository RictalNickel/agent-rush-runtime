# syntax=docker/dockerfile:1.7
# Supply a reviewed immutable Boxoro workspace base. The build script can build
# deploy/workspace.Dockerfile first and records its observed image identity.
ARG BOXORO_WORKSPACE_IMAGE
FROM ${BOXORO_WORKSPACE_IMAGE} AS base
USER root
# Runtime/catalog edits must not reinstall the independently pinned Node layer.
COPY agents/install.py agents/install-node.py agents/node.lock.json /opt/box/agents/
# This cache belongs to the trusted builder, never to a workspace or image layer.
# Its key separates platform, Python/Node major versions and cache policy. Locked
# sharing serializes writers across concurrent family builds; hashes still gate
# every reused artifact, and source-built Python wheels are not cached.
RUN --mount=type=cache,id=boxoro-specialists-linux-amd64-py312-node24-v1,target=/var/cache/boxoro-specialists,sharing=locked \
    /usr/bin/python3 /opt/box/agents/install-node.py --cache /var/cache/boxoro-specialists/binaries
COPY agents /opt/box/agents
COPY box-tools/specialist box-tools/specialist-analysis box-tools/finance /opt/box/bin/
COPY finance /opt/finance
RUN chmod 0755 /opt/box/bin/specialist /opt/box/bin/specialist-analysis /opt/box/bin/finance \
 && install -d /opt/box/agents/bin \
 && ln -sf /opt/box/bin/specialist-analysis /opt/box/agents/bin/boxoro-analysis \
 && ln -sf /opt/box/bin/finance /opt/box/agents/bin/finance \
 && /opt/box/venv/bin/python -c 'from playwright.sync_api import sync_playwright; from pathlib import Path; p=sync_playwright().start(); executable=Path(p.chromium.executable_path); assert executable.is_file(), executable; Path("/opt/box/chromium").symlink_to(executable); p.stop()' \
 && rm -rf /tmp/boxoro-agent-downloads
ENV PATH=/opt/box/node/bin:/opt/box/agents/bin:/opt/security/bin:/opt/security/venv/bin:/opt/box/bin:/usr/local/bin:/usr/bin:/bin:/opt/rust/cargo/bin \
    PLAYWRIGHT_BROWSERS_PATH=/ms-playwright \
    PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1 \
    PUPPETEER_SKIP_DOWNLOAD=true
# Runs after the security base's 01-security-path.sh in a combined image.
RUN printf '%s\n' 'export PATH=/opt/box/node/bin:/opt/box/agents/bin:/opt/security/bin:/opt/security/venv/bin:/opt/box/bin:/workspace/.local/bin:/workspace/.npm-global/bin:/home/agent/.cargo/bin:/opt/rust/cargo/bin:/usr/local/bin:/usr/bin:/bin' > /etc/profile.d/02-specialist-path.sh
WORKDIR /workspace

FROM base AS core
RUN --mount=type=cache,id=boxoro-specialists-linux-amd64-py312-node24-v1,target=/var/cache/boxoro-specialists,sharing=locked \
    BOXORO_AGENT_CACHE=/var/cache/boxoro-specialists bash /opt/box/agents/build-family.sh core
USER 1000:1000
CMD ["sleep", "infinity"]

FROM base AS python
RUN --mount=type=cache,id=boxoro-specialists-linux-amd64-py312-node24-v1,target=/var/cache/boxoro-specialists,sharing=locked \
    BOXORO_AGENT_CACHE=/var/cache/boxoro-specialists bash /opt/box/agents/build-family.sh python
USER 1000:1000
CMD ["sleep", "infinity"]

FROM base AS browser
RUN --mount=type=cache,id=boxoro-specialists-linux-amd64-py312-node24-v1,target=/var/cache/boxoro-specialists,sharing=locked \
    BOXORO_AGENT_CACHE=/var/cache/boxoro-specialists bash /opt/box/agents/build-family.sh browser
USER 1000:1000
CMD ["sleep", "infinity"]

FROM base AS platform
RUN --mount=type=cache,id=boxoro-specialists-linux-amd64-py312-node24-v1,target=/var/cache/boxoro-specialists,sharing=locked \
    BOXORO_AGENT_CACHE=/var/cache/boxoro-specialists bash /opt/box/agents/build-family.sh platform
USER 1000:1000
CMD ["sleep", "infinity"]

FROM base AS data
RUN --mount=type=cache,id=boxoro-specialists-linux-amd64-py312-node24-v1,target=/var/cache/boxoro-specialists,sharing=locked \
    BOXORO_AGENT_CACHE=/var/cache/boxoro-specialists bash /opt/box/agents/build-family.sh data
USER 1000:1000
CMD ["sleep", "infinity"]

FROM base AS documents
RUN --mount=type=cache,id=boxoro-specialists-linux-amd64-py312-node24-v1,target=/var/cache/boxoro-specialists,sharing=locked \
    BOXORO_AGENT_CACHE=/var/cache/boxoro-specialists bash /opt/box/agents/build-family.sh documents
USER 1000:1000
CMD ["sleep", "infinity"]

# Android static analysis and adb client. An emulator is a separate host service;
# no KVM device or privileged Docker socket is granted to this workspace image.
FROM base AS mobile
RUN --mount=type=cache,id=boxoro-specialists-linux-amd64-py312-node24-v1,target=/var/cache/boxoro-specialists,sharing=locked \
    BOXORO_AGENT_CACHE=/var/cache/boxoro-specialists bash /opt/box/agents/build-family.sh mobile
USER 1000:1000
CMD ["sleep", "infinity"]

FROM base AS finance
RUN --mount=type=cache,id=boxoro-specialists-linux-amd64-py312-node24-v1,target=/var/cache/boxoro-specialists,sharing=locked \
    BOXORO_AGENT_CACHE=/var/cache/boxoro-specialists bash /opt/box/agents/build-family.sh finance \
 && /usr/bin/python3 -m venv /opt/finance/venv \
 && /opt/finance/venv/bin/pip install --no-cache-dir --require-hashes --only-binary=:all: -r /opt/finance/requirements-advanced.lock \
 && /opt/finance/venv/bin/pip check
USER 1000:1000
CMD ["sleep", "infinity"]

FROM base AS suite
RUN --mount=type=cache,id=boxoro-specialists-linux-amd64-py312-node24-v1,target=/var/cache/boxoro-specialists,sharing=locked \
    BOXORO_AGENT_CACHE=/var/cache/boxoro-specialists bash /opt/box/agents/build-family.sh suite \
 && /usr/bin/python3 -m venv /opt/finance/venv \
 && /opt/finance/venv/bin/pip install --no-cache-dir --require-hashes --only-binary=:all: -r /opt/finance/requirements-advanced.lock \
 && /opt/finance/venv/bin/pip check
USER 1000:1000
CMD ["sleep", "infinity"]

# Trusted control-side service. Never use this target as AR_BOX_IMAGE or mount
# an untrusted workspace into it. Bind its port to host loopback and supply a
# secret-file mount at deployment; no credentials are build inputs.
FROM base AS finance-service
RUN /usr/bin/python3 -m venv /opt/openbb/venv \
 && /opt/openbb/venv/bin/pip install --no-cache-dir --require-hashes --only-binary=:all: -r /opt/finance/requirements.lock \
 && /opt/openbb/venv/bin/pip check \
 && install -d -o 1000 -g 1000 -m 0700 /var/lib/boxoro-finance
ENV FINANCE_OPENBB_STATE_DIR=/var/lib/boxoro-finance \
    FINANCE_OPENBB_BIND=0.0.0.0
USER 1000:1000
WORKDIR /opt/finance
EXPOSE 8768
CMD ["/opt/openbb/venv/bin/python", "-I", "/opt/finance/finance_server.py"]

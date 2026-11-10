# Opt-in Debian Bookworm workspace for security engineering sessions.
# It does not replace the restricted one-shot Rust sandbox.
# Build from repo root:
# docker build --platform linux/amd64 -f deploy/security-workspace.Dockerfile -t agent-rush-security:local .
FROM rust:1.90.0-bookworm@sha256:3914072ca0c3b8aad871db9169a651ccfce30cf58303e5d6f2db16d1d8a7e58f

ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
      bash coreutils findutils procps less file tree zip unzip xz-utils tar \
      git curl wget jq ripgrep fd-find ca-certificates build-essential pkg-config \
      python3 python3-pip python3-venv python3-dev nodejs npm \
      shellcheck cppcheck flawfinder clang clang-tidy clang-format \
      openssl libssl-dev binutils gdb strace \
      yara clamav libimage-exiftool-perl \
  && ln -s /usr/bin/fdfind /usr/local/bin/fd \
  && rm -rf /var/lib/apt/lists/*

COPY security/requirements.txt /opt/security/requirements.txt
RUN python3 -m venv /opt/security/venv \
  && /opt/security/venv/bin/pip install --no-cache-dir -r /opt/security/requirements.txt

COPY security/bin/security-scan /opt/security/bin/security-scan
COPY security/catalog.json /opt/security/catalog.json
COPY security/rules/semgrep.yml /opt/security/rules/semgrep.yml
RUN chmod 0755 /opt/security/bin/security-scan \
  && groupadd -g 1000 agent \
  && useradd -u 1000 -g 1000 -m -s /bin/bash agent \
  && mkdir -p /workspace \
  && chown 1000:1000 /workspace

# The worker launches this as UID 1000 with --runtime=runsc, a read-only root,
# dropped capabilities, no-new-privileges, workspace quota and egress controls.
# No secrets, Docker socket, host mounts or container management privileges.
ENV HOME=/home/agent USER=agent \
    PYTHONUSERBASE=/workspace/.local \
    PYTHONDONTWRITEBYTECODE=1 \
    PATH=/opt/security/venv/bin:/opt/security/bin:/usr/local/cargo/bin:/usr/local/bin:/usr/local/sbin:/usr/sbin:/usr/bin:/sbin:/bin
USER 1000:1000
WORKDIR /workspace
CMD ["sleep","infinity"]

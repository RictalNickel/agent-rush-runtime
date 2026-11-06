FROM mcr.microsoft.com/playwright:v1.64.1-noble@sha256:2f05f8a734509a7b3f26a912639488ee08f58a7b292d14d62271bcff9038768f
# Session workspace box: one per agent session, run under gVisor (runsc) on network ar-box
# with a read-only root, tmpfs /tmp and /home/agent (both executable, never setuid), and a
# named volume at /workspace.
# Build: docker build -f deploy/workspace.Dockerfile -t agent-rush-workspace:local deploy
# The image holds tools only; no credential, key or token is ever baked in or mounted.

ARG RUST_VERSION=1.90.0
ARG RUSTUP_VERSION=1.28.2
ARG PLAYWRIGHT_PY_VERSION=1.64.1
ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      bash coreutils findutils procps less file tree zip unzip xz-utils \
      git curl wget jq ripgrep fd-find ca-certificates build-essential pkg-config \
      python3-pip python3-venv python3-dev \
      pandoc libreoffice-writer-nogui libreoffice-calc-nogui libreoffice-impress-nogui \
      imagemagick librsvg2-bin ffmpeg poppler-utils qpdf fonts-dejavu-core \
      python3-numpy python3-pandas python3-matplotlib python3-pil \
 && ln -s /usr/bin/fdfind /usr/local/bin/fd \
 && rm -rf /var/lib/apt/lists/*

# uid 1000 is the box user; the base image calls it "ubuntu".
RUN usermod -l agent -d /home/agent ubuntu \
 && groupmod -n agent ubuntu \
 && install -d -o 1000 -g 1000 -m 0755 /workspace /home/agent

# Rust toolchain (minimal profile), shared read-only; per-box caches live in $HOME.
ENV RUSTUP_HOME=/opt/rust/rustup
RUN curl -fsSLo /tmp/rustup-init "https://static.rust-lang.org/rustup/archive/${RUSTUP_VERSION}/x86_64-unknown-linux-gnu/rustup-init" \
 && curl -fsSLo /tmp/rustup-init.sha256 "https://static.rust-lang.org/rustup/archive/${RUSTUP_VERSION}/x86_64-unknown-linux-gnu/rustup-init.sha256" \
 && echo "$(cut -d' ' -f1 /tmp/rustup-init.sha256)  /tmp/rustup-init" | sha256sum -c - \
 && chmod +x /tmp/rustup-init \
 && CARGO_HOME=/opt/rust/cargo /tmp/rustup-init -y --no-modify-path --profile minimal --default-toolchain "${RUST_VERSION}" \
 && rm /tmp/rustup-init /tmp/rustup-init.sha256 \
 && chmod -R a+rX /opt/rust

# Helpers' Python environment: Playwright drives the image's own Chromium over CDP.
RUN python3 -m venv /opt/box/venv \
 && /opt/box/venv/bin/pip install --no-cache-dir "playwright==${PLAYWRIGHT_PY_VERSION}"
COPY box-tools/browse box-tools/websearch box-tools/fetch box-tools/convert /opt/box/bin/
COPY box-tools/README.md /opt/box/README.md
RUN chmod 0755 /opt/box/bin/*

# Packages a command installs for itself (`pip install --user`, or plain `pip install`,
# which falls back to a user install; `npm install -g`) go to /workspace, the box's own
# disk: they survive parking, when /home/agent and /tmp (tmpfs) are lost. HOME stays a
# tmpfs for caches and dotfiles. Cargo's registry is a large cache, so CARGO_HOME stays there.
ENV HOME=/home/agent \
    USER=agent \
    CARGO_HOME=/home/agent/.cargo \
    PYTHONUSERBASE=/workspace/.local \
    NPM_CONFIG_PREFIX=/workspace/.npm-global \
    PIP_BREAK_SYSTEM_PACKAGES=1 \
    PIP_NO_CACHE_DIR=1 \
    PYTHONDONTWRITEBYTECODE=1 \
    MPLBACKEND=Agg \
    PLAYWRIGHT_BROWSERS_PATH=/ms-playwright \
    PATH=/opt/box/bin:/workspace/.local/bin:/workspace/.npm-global/bin:/home/agent/.local/bin:/home/agent/.cargo/bin:/opt/rust/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
# Commands run as `bash -lc`, and Ubuntu's /etc/profile resets PATH for login shells.
RUN echo "export PATH=$PATH" > /etc/profile.d/00-box-path.sh
USER 1000:1000
WORKDIR /workspace
CMD ["sleep", "infinity"]

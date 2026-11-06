#!/usr/bin/env bash
# Bootstrap a fresh Ubuntu 24.04 Vultr worker: Docker, gVisor (runsc) with enforced cgroups,
# Rust 1.90.0 toolchain and the digest-pinned sandbox image. Run as root over SSH.
# Never weakens isolation: runsc is registered without --ignore-cgroups.
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive

apt-get update -q
apt-get install -yq ca-certificates curl gnupg build-essential pkg-config git jq docker.io

# gVisor from its signed apt repository.
install -d -m 0755 /usr/share/keyrings
curl -fsSL https://gvisor.dev/archive.key | gpg --dearmor --yes -o /usr/share/keyrings/gvisor-archive-keyring.gpg
echo "deb [arch=amd64 signed-by=/usr/share/keyrings/gvisor-archive-keyring.gpg] https://storage.googleapis.com/gvisor/releases release main" \
  > /etc/apt/sources.list.d/gvisor.list
apt-get update -q
apt-get install -yq runsc

# Docker with systemd cgroup driver and runsc as an additional (non-default) runtime.
install -d -m 0755 /etc/docker
cat > /etc/docker/daemon.json <<'JSON'
{
  "exec-opts": ["native.cgroupdriver=systemd"],
  "runtimes": { "runsc": { "path": "/usr/bin/runsc" } },
  "live-restore": true,
  "log-driver": "local"
}
JSON
systemctl enable docker
systemctl restart docker

# Rust toolchain, checksum-verified rustup-init.
if [ ! -x /root/.cargo/bin/rustup ]; then
  tmp="$(mktemp -d)"
  curl -fsSL https://static.rust-lang.org/rustup/dist/x86_64-unknown-linux-gnu/rustup-init -o "$tmp/rustup-init"
  curl -fsSL https://static.rust-lang.org/rustup/dist/x86_64-unknown-linux-gnu/rustup-init.sha256 -o "$tmp/rustup-init.sha256"
  (cd "$tmp" && sed 's#\*.*#*rustup-init#' rustup-init.sha256 | sha256sum -c)
  chmod +x "$tmp/rustup-init"
  "$tmp/rustup-init" -y --profile minimal --default-toolchain 1.90.0 --component clippy,rustfmt
  rm -rf "$tmp"
fi

# Pinned sandbox image.
docker pull "rust:1.90.0-bookworm@sha256:3914072ca0c3b8aad871db9169a651ccfce30cf58303e5d6f2db16d1d8a7e58f"

echo "--- versions"
docker version --format 'docker {{.Server.Version}}'
runsc --version | head -1
/root/.cargo/bin/rustc --version
docker info --format 'cgroup={{.CgroupDriver}} v{{.CgroupVersion}} mem={{.MemoryLimit}} cpu={{.CPUCfsQuota}} pids={{.PidsLimit}} runtimes={{json .Runtimes}}'

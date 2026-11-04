#!/usr/bin/env bash
set -euo pipefail
cd /workspace/Hackathon
export CARGO_HOME=/workspace/.toolchains/cargo
export RUSTUP_HOME=/workspace/.toolchains/rustup
export PATH="$CARGO_HOME/bin:$PATH"
command -v cc >/dev/null
if [ ! -x "$CARGO_HOME/bin/rustup" ]; then
    setup_dir="$(mktemp -d /tmp/agent-rush-rust.XXXXXX)"
    trap 'rm -rf "$setup_dir"' EXIT
    curl --fail --silent --show-error --location \
      https://static.rust-lang.org/rustup/dist/x86_64-unknown-linux-gnu/rustup-init \
      -o "$setup_dir/rustup-init"
    curl --fail --silent --show-error --location \
      https://static.rust-lang.org/rustup/dist/x86_64-unknown-linux-gnu/rustup-init.sha256 \
      -o "$setup_dir/rustup-init.sha256"
    (cd "$setup_dir" && sha256sum -c rustup-init.sha256)
    chmod +x "$setup_dir/rustup-init"
    "$setup_dir/rustup-init" -y --profile minimal --default-toolchain 1.90.0 --no-modify-path
fi
rustup toolchain install 1.90.0 --profile minimal --component clippy,rustfmt
cargo build --locked -j 4
cargo test --locked -j 4
# Real sandbox qualification is separate and mandatory before worker use.
# Never create paid VMs, change isolation or inject credentials during installation.
